use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use toml_edit::{Array, DocumentMut, Item, Value as TomlValue};

const MARKER: &str = "# managed-by: codex-config-manager";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderConfig {
    id: String,
    name: String,
    enabled: bool,
    config: Value,
    #[serde(default)]
    balance_query: BalanceQueryConfig,
    #[serde(default)]
    balance_status: Option<BalanceStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BalanceQueryType {
    Disabled,
    NewApi,
    Sub2Api,
}

impl Default for BalanceQueryType {
    fn default() -> Self {
        Self::Disabled
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NewApiBalanceTarget {
    TokenQuota,
    AccountBalance,
}

impl Default for NewApiBalanceTarget {
    fn default() -> Self {
        Self::TokenQuota
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BalanceAuthMode {
    ProviderToken,
    SeparateToken,
}

impl Default for BalanceAuthMode {
    fn default() -> Self {
        Self::ProviderToken
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BalanceQueryConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    query_type: BalanceQueryType,
    #[serde(default)]
    new_api_target: NewApiBalanceTarget,
    #[serde(default)]
    endpoint: String,
    #[serde(default = "default_balance_path")]
    path: String,
    #[serde(default)]
    auth_mode: BalanceAuthMode,
    #[serde(default)]
    query_token: String,
    #[serde(default)]
    new_api_user_id: String,
}

impl Default for BalanceQueryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            query_type: BalanceQueryType::Disabled,
            new_api_target: NewApiBalanceTarget::TokenQuota,
            endpoint: String::new(),
            path: default_balance_path(),
            auth_mode: BalanceAuthMode::ProviderToken,
            query_token: String::new(),
            new_api_user_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BalanceStatus {
    amount: Option<String>,
    label: String,
    checked_at: Option<u64>,
    error: Option<String>,
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
    balance_query: Option<BalanceQueryConfig>,
}

#[derive(Debug, Deserialize)]
struct SaveBasePayload {
    base_template_name: String,
    base_toml: String,
}

#[derive(Debug, Deserialize)]
struct QueryBalancePayload {
    provider_id: String,
}

fn default_balance_path() -> String {
    "/api/usage/token/".to_string()
}

fn default_balance_path_for(
    query_type: &BalanceQueryType,
    new_api_target: &NewApiBalanceTarget,
) -> String {
    match (query_type, new_api_target) {
        (BalanceQueryType::NewApi, NewApiBalanceTarget::TokenQuota) => {
            "/api/usage/token/".to_string()
        }
        (BalanceQueryType::NewApi, NewApiBalanceTarget::AccountBalance) => {
            "/api/user/self".to_string()
        }
        (BalanceQueryType::Sub2Api, _) => "/v1/usage".to_string(),
        _ => default_balance_path(),
    }
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
    let raw =
        fs::read_to_string(state_path()?).map_err(|err| format!("无法读取状态文件: {err}"))?;
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
    if state.applied_provider_id.as_ref().is_some_and(|id| {
        state
            .providers
            .iter()
            .any(|provider| provider.id.as_str() == id.as_str())
    }) {
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

fn custom_provider_base_url(provider: &ProviderConfig) -> Option<String> {
    provider
        .config
        .pointer("/model_providers/custom/base_url")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn custom_provider_token(provider: &ProviderConfig) -> Option<String> {
    provider
        .config
        .pointer("/model_providers/custom/experimental_bearer_token")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn normalize_balance_config(
    mut config: BalanceQueryConfig,
    provider: &ProviderConfig,
) -> BalanceQueryConfig {
    if config.endpoint.trim().is_empty() {
        let endpoint = custom_provider_base_url(provider).unwrap_or_default();
        config.endpoint = endpoint
            .trim()
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .trim_end_matches('/')
            .to_string();
    }
    let has_legacy_newapi_path = matches!(
        (&config.query_type, &config.new_api_target),
        (BalanceQueryType::NewApi, NewApiBalanceTarget::TokenQuota)
    ) && config.path.trim() == "/api/user/self"
        && config.new_api_user_id.trim().is_empty();

    if config.path.trim().is_empty() || has_legacy_newapi_path {
        config.path = default_balance_path_for(&config.query_type, &config.new_api_target);
    }
    if matches!(
        (&config.query_type, &config.new_api_target),
        (
            BalanceQueryType::NewApi,
            NewApiBalanceTarget::AccountBalance
        )
    ) {
        config.auth_mode = BalanceAuthMode::SeparateToken;
    }
    config
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
        Item::ArrayOfTables(array) => Value::Array(
            array
                .iter()
                .map(|table| toml_item_to_json(&Item::Table(table.clone())))
                .collect(),
        ),
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

fn now_epoch_seconds() -> Result<u64, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("系统时间异常: {err}"))?
        .as_secs())
}

fn join_url(endpoint: &str, path: &str) -> String {
    let endpoint = endpoint.trim().trim_end_matches('/');
    let path = path.trim();
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else {
        format!("{endpoint}/{}", path.trim_start_matches('/'))
    }
}

fn find_balance_value(value: &Value) -> Option<f64> {
    let keys = [
        "balance",
        "remain_balance",
        "remaining_balance",
        "quota",
        "remain_quota",
        "remaining_quota",
        "available_quota",
        "money",
        "credit",
    ];

    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        Value::Object(map) => {
            for key in keys {
                if let Some(found) = map.get(key).and_then(find_balance_value) {
                    return Some(found);
                }
            }
            for key in ["data", "user", "result"] {
                if let Some(found) = map.get(key).and_then(find_balance_value) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn format_balance_amount(amount: f64) -> String {
    if amount.abs() >= 100.0 {
        format!("{amount:.0}")
    } else {
        format!("{amount:.2}")
    }
}

fn format_balance_label(kind: &str, amount: f64, unit: Option<&str>) -> (String, String) {
    let amount = format_balance_amount(amount);
    let display = match unit.filter(|unit| !unit.trim().is_empty()) {
        Some(unit) if unit.eq_ignore_ascii_case("usd") => format!("${amount}"),
        Some(unit) => format!("{amount} {unit}"),
        None => format!("¥ {amount}"),
    };

    (amount, format!("{kind} {display}"))
}

fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
}

fn number_at_path(value: &Value, path: &[&str]) -> Option<f64> {
    value_at_path(value, path).and_then(find_balance_value)
}

fn string_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    value_at_path(value, path).and_then(Value::as_str)
}

fn bool_at_path(value: &Value, path: &[&str]) -> Option<bool> {
    value_at_path(value, path).and_then(Value::as_bool)
}

fn parse_sub2api_balance(value: &Value) -> Option<(String, String)> {
    if let Some(amount) = number_at_path(value, &["quota", "remaining"]) {
        let unit =
            string_at_path(value, &["quota", "unit"]).or_else(|| string_at_path(value, &["unit"]));
        return Some(format_balance_label("额度", amount, unit));
    }

    if let Some(amount) = number_at_path(value, &["balance"]) {
        let unit = string_at_path(value, &["unit"]);
        return Some(format_balance_label("余额", amount, unit));
    }

    if value.get("subscription").is_some() {
        if let Some(amount) = number_at_path(value, &["remaining"]) {
            let unit = string_at_path(value, &["unit"])
                .or_else(|| string_at_path(value, &["subscription", "unit"]));
            return Some(format_balance_label("订阅余量", amount, unit));
        }
    }

    None
}

fn format_quota_label(kind: &str, quota: f64, quota_per_unit: Option<f64>) -> (String, String) {
    if let Some(quota_per_unit) = quota_per_unit.filter(|value| *value > 0.0) {
        let amount = quota / quota_per_unit;
        return format_balance_label(kind, amount, Some("USD"));
    }

    let amount = format_balance_amount(quota);
    (amount.clone(), format!("{kind} {amount}"))
}

fn parse_newapi_balance(
    value: &Value,
    target: &NewApiBalanceTarget,
    quota_per_unit: Option<f64>,
) -> Option<(String, String)> {
    match target {
        NewApiBalanceTarget::TokenQuota => {
            if bool_at_path(value, &["data", "unlimited_quota"]).unwrap_or(false) {
                return Some(("unlimited".to_string(), "Key额度 无限".to_string()));
            }

            number_at_path(value, &["data", "total_available"])
                .map(|quota| format_quota_label("Key额度", quota, quota_per_unit))
        }
        NewApiBalanceTarget::AccountBalance => number_at_path(value, &["data", "quota"])
            .map(|quota| format_quota_label("账户余额", quota, quota_per_unit)),
    }
}

async fn fetch_newapi_quota_per_unit(client: &reqwest::Client, endpoint: &str) -> Option<f64> {
    let url = join_url(endpoint, "/api/status");
    let response = client
        .get(url)
        .header("accept", "application/json")
        .timeout(Duration::from_secs(4))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.json::<Value>().await.ok()?;
    number_at_path(&body, &["data", "quota_per_unit"])
        .or_else(|| number_at_path(&body, &["quota_per_unit"]))
}

async fn fetch_balance(provider: &ProviderConfig) -> BalanceStatus {
    let config = normalize_balance_config(provider.balance_query.clone(), provider);
    let checked_at = now_epoch_seconds().ok();

    if !config.enabled || matches!(config.query_type, BalanceQueryType::Disabled) {
        return BalanceStatus {
            amount: None,
            label: "未配置".to_string(),
            checked_at,
            error: Some("未启用余额查询".to_string()),
        };
    }

    if config.endpoint.trim().is_empty() {
        return BalanceStatus {
            amount: None,
            label: "未配置".to_string(),
            checked_at,
            error: Some("查询地址为空".to_string()),
        };
    }

    let is_newapi_account_balance = matches!(
        (&config.query_type, &config.new_api_target),
        (
            BalanceQueryType::NewApi,
            NewApiBalanceTarget::AccountBalance
        )
    );
    let new_api_user_id = config.new_api_user_id.trim();

    if is_newapi_account_balance {
        if new_api_user_id.is_empty() {
            return BalanceStatus {
                amount: None,
                label: "未配置".to_string(),
                checked_at,
                error: Some("New-Api-User 为空".to_string()),
            };
        }
        if !new_api_user_id.chars().all(|ch| ch.is_ascii_digit()) {
            return BalanceStatus {
                amount: None,
                label: "未配置".to_string(),
                checked_at,
                error: Some("New-Api-User 必须是数字用户 ID".to_string()),
            };
        }
    }

    let token = match (&config.auth_mode, is_newapi_account_balance) {
        (_, true) => config.query_token.clone(),
        (BalanceAuthMode::ProviderToken, false) => {
            custom_provider_token(provider).unwrap_or_default()
        }
        (BalanceAuthMode::SeparateToken, false) => config.query_token.clone(),
    };

    if token.trim().is_empty() {
        return BalanceStatus {
            amount: None,
            label: "未配置".to_string(),
            checked_at,
            error: Some("查询 token 为空".to_string()),
        };
    }

    let url = join_url(&config.endpoint, &config.path);
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            return BalanceStatus {
                amount: None,
                label: "查询失败".to_string(),
                checked_at,
                error: Some(format!("创建 HTTP 客户端失败: {err}")),
            };
        }
    };
    let mut request = client
        .get(url)
        .bearer_auth(token.trim())
        .header("accept", "application/json");

    if is_newapi_account_balance {
        request = request.header("New-Api-User", new_api_user_id);
    }

    let quota_per_unit_task = if matches!(config.query_type, BalanceQueryType::NewApi) {
        let client = client.clone();
        let endpoint = config.endpoint.clone();
        Some(tauri::async_runtime::spawn(async move {
            fetch_newapi_quota_per_unit(&client, &endpoint).await
        }))
    } else {
        None
    };

    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            return BalanceStatus {
                amount: None,
                label: "查询失败".to_string(),
                checked_at,
                error: Some(format!("请求失败: {err}")),
            };
        }
    };

    let status = response.status();
    if !status.is_success() {
        return BalanceStatus {
            amount: None,
            label: "查询失败".to_string(),
            checked_at,
            error: Some(format!("接口返回 HTTP {status}")),
        };
    }

    let body = match response.json::<Value>().await {
        Ok(body) => body,
        Err(err) => {
            return BalanceStatus {
                amount: None,
                label: "查询失败".to_string(),
                checked_at,
                error: Some(format!("响应不是有效 JSON: {err}")),
            };
        }
    };

    if matches!(config.query_type, BalanceQueryType::NewApi) {
        let quota_per_unit = match quota_per_unit_task {
            Some(task) => task.await.ok().flatten(),
            None => None,
        };
        if let Some((amount, label)) =
            parse_newapi_balance(&body, &config.new_api_target, quota_per_unit)
        {
            return BalanceStatus {
                amount: Some(amount),
                label,
                checked_at,
                error: None,
            };
        }
    } else if matches!(config.query_type, BalanceQueryType::Sub2Api) {
        if let Some((amount, label)) = parse_sub2api_balance(&body) {
            return BalanceStatus {
                amount: Some(amount),
                label,
                checked_at,
                error: None,
            };
        }
    } else if let Some(amount) = find_balance_value(&body) {
        let (amount, label) = format_balance_label("余额", amount, None);
        return BalanceStatus {
            amount: Some(amount),
            label,
            checked_at,
            error: None,
        };
    }

    BalanceStatus {
        amount: None,
        label: "查询失败".to_string(),
        checked_at,
        error: Some("未识别余额字段".to_string()),
    }
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
    if state
        .providers
        .iter()
        .all(|provider| provider.id != provider_id)
    {
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
    if let Some(balance_query) = payload.balance_query {
        provider.balance_query = normalize_balance_config(balance_query, provider);
        provider.balance_status = None;
    }
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
    if let Some(balance_query) = payload.balance_query {
        provider.balance_query = normalize_balance_config(balance_query, provider);
        provider.balance_status = None;
    }
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
        balance_query: BalanceQueryConfig::default(),
        balance_status: None,
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

#[tauri::command]
async fn query_provider_balance(payload: QueryBalancePayload) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    let provider = state
        .providers
        .iter()
        .find(|provider| provider.id == payload.provider_id)
        .cloned()
        .ok_or_else(|| "供应商不存在".to_string())?;

    let status = fetch_balance(&provider).await;
    if let Some(provider) = state
        .providers
        .iter_mut()
        .find(|provider| provider.id == payload.provider_id)
    {
        provider.balance_status = Some(status);
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
            apply_config,
            query_provider_balance
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
