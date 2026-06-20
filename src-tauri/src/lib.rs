use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use toml_edit::{Array, DocumentMut, Item, Value as TomlValue};

const MARKER: &str = "# managed-by: codex-config-manager";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderConfig {
    id: String,
    name: String,
    enabled: bool,
    config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagerState {
    active_provider_id: String,
    #[serde(default)]
    applied_provider_id: Option<String>,
    base_template_name: String,
    base: Value,
    providers: Vec<ProviderConfig>,
    last_applied: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
struct ProviderSummary {
    id: String,
    name: String,
    enabled: bool,
    pending_changes: usize,
}

#[derive(Debug, Clone, Serialize)]
struct ConfigRow {
    path: String,
    value: Value,
    source: String,
    changed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DiffEntry {
    path: String,
    current: Option<Value>,
    desired: Option<Value>,
    action: String,
    source: String,
}

#[derive(Debug, Clone, Serialize)]
struct AppState {
    codex_config_path: String,
    manager_dir: String,
    current_config_raw: String,
    current_config_exists: bool,
    active_provider_id: String,
    base_template_name: String,
    base_toml: String,
    base: Value,
    providers: Vec<ProviderSummary>,
    active_provider: Option<ProviderConfig>,
    active_provider_toml: String,
    desired: Value,
    final_preview_toml: String,
    summary: Vec<ConfigRow>,
    diffs: Vec<DiffEntry>,
    marker_present: bool,
}

#[derive(Debug, Deserialize)]
struct SaveProviderPayload {
    provider_id: String,
    provider_name: Option<String>,
    config_toml: String,
}

#[derive(Debug, Deserialize)]
struct SaveBasePayload {
    base_template_name: String,
    base_toml: String,
}

fn home_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .ok_or_else(|| "无法读取 USERPROFILE".to_string())
    }

    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "无法读取 HOME".to_string())
    }
}

fn codex_home() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(path));
    }

    Ok(home_dir()?.join(".codex"))
}

fn manager_dir() -> Result<PathBuf, String> {
    Ok(codex_home()?.join("config-manager"))
}

fn state_path() -> Result<PathBuf, String> {
    Ok(manager_dir()?.join("state.json"))
}

fn codex_config_path() -> Result<PathBuf, String> {
    Ok(codex_home()?.join("config.toml"))
}

fn default_state() -> ManagerState {
    ManagerState {
        active_provider_id: String::new(),
        applied_provider_id: None,
        base_template_name: "默认模板".to_string(),
        base: json!({}),
        providers: vec![],
        last_applied: None,
    }
}

fn ensure_state_file() -> Result<(), String> {
    fs::create_dir_all(manager_dir()?).map_err(|err| format!("无法创建管理目录: {err}"))?;

    let path = state_path()?;
    if !path.exists() {
        save_state(&default_state())?;
    }

    Ok(())
}

fn load_state_file() -> Result<ManagerState, String> {
    ensure_state_file()?;
    let raw = fs::read_to_string(state_path()?).map_err(|err| format!("无法读取状态文件: {err}"))?;
    let state: ManagerState =
        serde_json::from_str(&raw).map_err(|err| format!("状态文件 JSON 无效: {err}"))?;

    if is_legacy_seed_state(&state) {
        let state = default_state();
        save_state(&state)?;
        return Ok(state);
    }

    Ok(state)
}

fn is_legacy_seed_state(state: &ManagerState) -> bool {
    if state.last_applied.is_some() || state.providers.len() != 3 {
        return false;
    }

    let ids = state
        .providers
        .iter()
        .map(|provider| provider.id.as_str())
        .collect::<Vec<_>>();

    ids == ["openai", "openrouter", "local"]
        && state.base
            == json!({
                "approval_policy": "on-request",
                "sandbox_mode": "workspace-write",
                "model_reasoning_effort": "high"
            })
}

fn save_state(state: &ManagerState) -> Result<(), String> {
    fs::create_dir_all(manager_dir()?).map_err(|err| format!("无法创建管理目录: {err}"))?;
    let raw =
        serde_json::to_string_pretty(state).map_err(|err| format!("无法序列化状态文件: {err}"))?;
    fs::write(state_path()?, raw).map_err(|err| format!("无法写入状态文件: {err}"))
}

fn active_provider(state: &ManagerState) -> Option<ProviderConfig> {
    state
        .providers
        .iter()
        .find(|provider| provider.id == state.active_provider_id)
        .cloned()
        .or_else(|| state.providers.first().cloned())
}

fn resolved_applied_provider_id(state: &ManagerState) -> Option<String> {
    if state
        .applied_provider_id
        .as_ref()
        .is_some_and(|id| {
            state
                .providers
                .iter()
                .any(|provider| provider.id.as_str() == id.as_str())
        })
    {
        return state.applied_provider_id.clone();
    }

    let last_applied = state.last_applied.as_ref()?;
    state
        .providers
        .iter()
        .find(|provider| desired_config(state, Some(provider)) == *last_applied)
        .map(|provider| provider.id.clone())
}

fn merge_values(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, overlay_value) in overlay_map {
                match base_map.get_mut(key) {
                    Some(base_value) => merge_values(base_value, overlay_value),
                    None => {
                        base_map.insert(key.clone(), overlay_value.clone());
                    }
                }
            }
        }
        (base_slot, overlay_value) => {
            *base_slot = overlay_value.clone();
        }
    }
}

fn desired_config(state: &ManagerState, provider: Option<&ProviderConfig>) -> Value {
    let mut desired = state.base.clone();
    if let Some(provider) = provider {
        merge_values(&mut desired, &provider.config);
    }
    desired
}

fn flatten(value: &Value) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    flatten_into(None, value, &mut out);
    out
}

fn flatten_into(prefix: Option<String>, value: &Value, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = prefix
                    .as_ref()
                    .map(|prefix| format!("{prefix}.{key}"))
                    .unwrap_or_else(|| key.clone());
                flatten_into(Some(path), child, out);
            }
        }
        _ => {
            if let Some(path) = prefix {
                out.insert(path, value.clone());
            }
        }
    }
}

fn source_for_path(state: &ManagerState, provider: Option<&ProviderConfig>, path: &str) -> String {
    if provider
        .map(|provider| flatten(&provider.config).contains_key(path))
        .unwrap_or(false)
    {
        "供应商".to_string()
    } else if flatten(&state.base).contains_key(path) {
        "基础模板".to_string()
    } else {
        "当前配置".to_string()
    }
}

fn read_current_toml() -> Result<(DocumentMut, bool, String, bool), String> {
    let path = codex_config_path()?;
    if !path.exists() {
        return Ok((DocumentMut::new(), false, String::new(), false));
    }

    let raw = fs::read_to_string(&path).map_err(|err| format!("无法读取 Codex 配置: {err}"))?;
    let marker_present = raw.contains(MARKER);
    let doc = raw
        .parse::<DocumentMut>()
        .map_err(|err| format!("Codex 配置 TOML 无效: {err}"))?;

    Ok((doc, marker_present, raw, true))
}

fn toml_doc_to_json(doc: &DocumentMut) -> Value {
    let item = Item::Table(doc.as_table().clone());
    toml_item_to_json(&item)
}

fn toml_text_to_json(raw: &str) -> Result<Value, String> {
    let doc = raw
        .parse::<DocumentMut>()
        .map_err(|err| format!("TOML 无效: {err}"))?;
    Ok(toml_doc_to_json(&doc))
}

fn json_to_toml_text(value: &Value) -> Result<String, String> {
    let mut doc = DocumentMut::new();
    if let Value::Object(map) = value {
        for (key, child) in map {
            doc.as_table_mut().insert(key, json_to_toml_item(child)?);
        }
        Ok(doc.to_string())
    } else {
        Err("顶层配置必须是 TOML 表".to_string())
    }
}

fn toml_item_to_json(item: &Item) -> Value {
    match item {
        Item::Value(value) => toml_value_to_json(value),
        Item::Table(table) => {
            let mut map = Map::new();
            for (key, item) in table.iter() {
                if !item.is_none() {
                    map.insert(key.to_string(), toml_item_to_json(item));
                }
            }
            Value::Object(map)
        }
        Item::ArrayOfTables(array) => {
            Value::Array(array.iter().map(|table| toml_item_to_json(&Item::Table(table.clone()))).collect())
        }
        Item::None => Value::Null,
    }
}

fn toml_value_to_json(value: &TomlValue) -> Value {
    match value {
        TomlValue::String(value) => Value::String(value.value().to_string()),
        TomlValue::Integer(value) => Value::Number((*value.value()).into()),
        TomlValue::Float(value) => serde_json::Number::from_f64(*value.value())
            .map(Value::Number)
            .unwrap_or(Value::Null),
        TomlValue::Boolean(value) => Value::Bool(*value.value()),
        TomlValue::Datetime(value) => Value::String(value.value().to_string()),
        TomlValue::Array(array) => Value::Array(array.iter().map(toml_value_to_json).collect()),
        TomlValue::InlineTable(table) => {
            let mut map = Map::new();
            for (key, value) in table.iter() {
                map.insert(key.to_string(), toml_value_to_json(value));
            }
            Value::Object(map)
        }
    }
}

fn json_to_toml_item(value: &Value) -> Result<Item, String> {
    match value {
        Value::Null => Err("不支持将 null 写入 TOML".to_string()),
        Value::Bool(value) => Ok(Item::Value(TomlValue::from(*value))),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(Item::Value(TomlValue::from(value)))
            } else if let Some(value) = value.as_f64() {
                Ok(Item::Value(TomlValue::from(value)))
            } else {
                Err("不支持该数字类型".to_string())
            }
        }
        Value::String(value) => Ok(Item::Value(TomlValue::from(value.as_str()))),
        Value::Array(values) => {
            let converted = values
                .iter()
                .map(json_to_toml_item)
                .map(|item| {
                    item.and_then(|item| match item {
                        Item::Value(value) => Ok(value),
                        _ => Err("数组中暂不支持对象类型".to_string()),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut array = Array::new();
            for value in converted {
                array.push_formatted(value);
            }
            Ok(Item::Value(TomlValue::Array(array)))
        }
        Value::Object(map) => {
            let mut table = toml_edit::Table::new();
            for (key, child) in map {
                table.insert(key, json_to_toml_item(child)?);
            }
            Ok(Item::Table(table))
        }
    }
}

fn set_toml_path(doc: &mut DocumentMut, path: &str, value: &Value) -> Result<(), String> {
    let parts = path.split('.').collect::<Vec<_>>();
    if parts.is_empty() {
        return Err("路径为空".to_string());
    }

    let mut table = doc.as_table_mut();
    for part in &parts[..parts.len() - 1] {
        if !table.contains_key(*part) {
            table.insert(*part, Item::Table(toml_edit::Table::new()));
        }

        table = table
            .get_mut(*part)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| format!("路径 {part} 不是表，无法写入子项"))?;
    }

    table.insert(parts[parts.len() - 1], json_to_toml_item(value)?);
    Ok(())
}

fn render_final_disk_toml(
    mut doc: DocumentMut,
    marker_present: bool,
    desired: &Value,
) -> Result<String, String> {
    for (path, value) in flatten(desired) {
        set_toml_path(&mut doc, &path, &value)?;
    }

    let mut raw = doc.to_string();
    if !marker_present && !raw.contains(MARKER) {
        raw = format!("{MARKER}\n{raw}");
    }

    Ok(raw)
}

fn compute_diffs(
    state: &ManagerState,
    provider: Option<&ProviderConfig>,
    current_json: &Value,
    desired: &Value,
) -> Vec<DiffEntry> {
    let current_flat = flatten(current_json);
    let desired_flat = flatten(desired);
    let last_applied_flat = state.last_applied.as_ref().map(flatten).unwrap_or_default();

    desired_flat
        .iter()
        .filter_map(|(path, desired_value)| {
            let current = current_flat.get(path);
            let last_applied = last_applied_flat.get(path);

            if current == Some(desired_value) {
                return None;
            }

            let action = if current.is_none() {
                "新增"
            } else if current != last_applied && last_applied.is_some() {
                "冲突"
            } else {
                "更新"
            };

            Some(DiffEntry {
                path: path.clone(),
                current: current.cloned(),
                desired: Some(desired_value.clone()),
                action: action.to_string(),
                source: source_for_path(state, provider, path),
            })
        })
        .collect()
}

fn build_app_state(state: ManagerState) -> Result<AppState, String> {
    let provider = active_provider(&state);
    let applied_provider_id = resolved_applied_provider_id(&state);
    let desired = desired_config(&state, provider.as_ref());
    let (doc, marker_present, current_config_raw, current_config_exists) = read_current_toml()?;
    let current_json = toml_doc_to_json(&doc);
    let final_preview_toml = render_final_disk_toml(doc, marker_present, &desired)?;
    let diffs = compute_diffs(&state, provider.as_ref(), &current_json, &desired);
    let desired_flat = flatten(&desired);

    let summary = desired_flat
        .iter()
        .take(10)
        .map(|(path, value)| ConfigRow {
            path: path.clone(),
            value: value.clone(),
            source: source_for_path(&state, provider.as_ref(), path),
            changed: diffs.iter().any(|diff| diff.path == *path),
        })
        .collect::<Vec<_>>();

    let providers = state
        .providers
        .iter()
        .map(|provider| {
            let provider_desired = desired_config(&state, Some(provider));
            let pending_changes =
                compute_diffs(&state, Some(provider), &current_json, &provider_desired).len();
            ProviderSummary {
                id: provider.id.clone(),
                name: provider.name.clone(),
                enabled: applied_provider_id
                    .as_deref()
                    .is_some_and(|id| id == provider.id.as_str()),
                pending_changes,
            }
        })
        .collect();

    Ok(AppState {
        codex_config_path: codex_config_path()?.display().to_string(),
        manager_dir: manager_dir()?.display().to_string(),
        current_config_raw,
        current_config_exists,
        active_provider_id: state.active_provider_id,
        base_template_name: state.base_template_name,
        base_toml: json_to_toml_text(&state.base)?,
        base: state.base,
        providers,
        active_provider_toml: provider
            .as_ref()
            .map(|provider| json_to_toml_text(&provider.config))
            .transpose()?
            .unwrap_or_default(),
        active_provider: provider,
        desired: desired.clone(),
        final_preview_toml,
        summary,
        diffs,
        marker_present,
    })
}

#[tauri::command]
fn load_app_state() -> Result<AppState, String> {
    build_app_state(load_state_file()?)
}

#[tauri::command]
fn select_provider(provider_id: String) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    if state.providers.iter().all(|provider| provider.id != provider_id) {
        return Err("供应商不存在".to_string());
    }

    state.active_provider_id = provider_id;
    save_state(&state)?;
    build_app_state(state)
}

#[tauri::command]
fn save_provider(payload: SaveProviderPayload) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    let provider = state
        .providers
        .iter_mut()
        .find(|provider| provider.id == payload.provider_id)
        .ok_or_else(|| "供应商不存在".to_string())?;

    if let Some(name) = payload.provider_name.as_deref() {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("供应商名称不能为空".to_string());
        }
        provider.name = trimmed.to_string();
    }

    provider.config = toml_text_to_json(&payload.config_toml)?;
    save_state(&state)?;
    build_app_state(state)
}

#[tauri::command]
fn preview_provider(payload: SaveProviderPayload) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    let active_provider_id = payload.provider_id.clone();
    let provider = state
        .providers
        .iter_mut()
        .find(|provider| provider.id == payload.provider_id)
        .ok_or_else(|| "供应商不存在".to_string())?;

    if let Some(name) = payload.provider_name.as_deref() {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("供应商名称不能为空".to_string());
        }
        provider.name = trimmed.to_string();
    }

    provider.config = toml_text_to_json(&payload.config_toml)?;
    state.active_provider_id = active_provider_id;

    build_app_state(state)
}

#[tauri::command]
fn add_provider(name: String) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("供应商名称不能为空".to_string());
    }

    let id_base = trimmed
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let id_base = if id_base.is_empty() {
        "provider".to_string()
    } else {
        id_base
    };

    let mut id = id_base.clone();
    let mut index = 2;
    while state.providers.iter().any(|provider| provider.id == id) {
        id = format!("{id_base}-{index}");
        index += 1;
    }

    state.active_provider_id = id.clone();
    state.providers.push(ProviderConfig {
        id: id.clone(),
        name: trimmed.to_string(),
        enabled: false,
        config: toml_text_to_json(
            "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"\"\nexperimental_bearer_token = \"\"\n",
        )?,
    });

    save_state(&state)?;
    build_app_state(state)
}

#[tauri::command]
fn save_base_template(payload: SaveBasePayload) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    state.base_template_name = payload.base_template_name;
    state.base = toml_text_to_json(&payload.base_toml)?;
    save_state(&state)?;
    build_app_state(state)
}

#[tauri::command]
fn apply_config() -> Result<AppState, String> {
    let mut state = load_state_file()?;
    let provider = active_provider(&state);
    let applied_provider_id = provider.as_ref().map(|provider| provider.id.clone());
    let desired = desired_config(&state, provider.as_ref());
    let config_path = codex_config_path()?;

    fs::create_dir_all(codex_home()?).map_err(|err| format!("无法创建 Codex 目录: {err}"))?;

    if config_path.exists() {
        let backup_name = format!(
            "config.toml.{}.bak",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|err| format!("系统时间异常: {err}"))?
                .as_secs()
        );
        fs::copy(&config_path, manager_dir()?.join(backup_name))
            .map_err(|err| format!("无法备份现有配置: {err}"))?;
    }

    let (doc, marker_present, _, _) = read_current_toml()?;
    let raw = render_final_disk_toml(doc, marker_present, &desired)?;

    fs::write(&config_path, raw).map_err(|err| format!("无法写入 Codex 配置: {err}"))?;
    state.last_applied = Some(desired);
    if let Some(applied_provider_id) = applied_provider_id {
        state.applied_provider_id = Some(applied_provider_id.clone());
        state.active_provider_id = applied_provider_id.clone();
        for provider in &mut state.providers {
            provider.enabled = provider.id == applied_provider_id;
        }
    } else {
        state.applied_provider_id = None;
        for provider in &mut state.providers {
            provider.enabled = false;
        }
    }
    save_state(&state)?;
    build_app_state(state)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            load_app_state,
            select_provider,
            save_provider,
            preview_provider,
            add_provider,
            save_base_template,
            apply_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
