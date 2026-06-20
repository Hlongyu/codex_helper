use chrono::{DateTime, Datelike, Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Mutex;
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
    #[serde(default)]
    applied_history: Vec<AppliedProviderSnapshot>,
    #[serde(default)]
    pricing: Vec<PricingRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppliedProviderSnapshot {
    provider_id: String,
    provider_name: String,
    model_provider: String,
    base_url_hash: Option<String>,
    applied_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PricingRule {
    id: String,
    provider_match: String,
    model_match: String,
    input_per_million: f64,
    cached_input_per_million: f64,
    output_per_million: f64,
    reasoning_output_per_million: f64,
    currency: String,
    #[serde(default)]
    source: String,
}

impl Default for PricingRule {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            provider_match: "*".to_string(),
            model_match: "*".to_string(),
            input_per_million: 0.0,
            cached_input_per_million: 0.0,
            output_per_million: 0.0,
            reasoning_output_per_million: 0.0,
            currency: "USD".to_string(),
            source: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TokenUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    cached_input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    reasoning_output_tokens: i64,
    #[serde(default)]
    total_tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
struct UsageSummary {
    request_count: usize,
    input_tokens: i64,
    uncached_input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
    total_tokens: i64,
    estimated_cost: f64,
    currency: String,
}

impl Default for UsageSummary {
    fn default() -> Self {
        Self {
            request_count: 0,
            input_tokens: 0,
            uncached_input_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            total_tokens: 0,
            estimated_cost: 0.0,
            currency: "USD".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct UsageDailyPoint {
    day: String,
    request_count: usize,
    total_tokens: i64,
    estimated_cost: f64,
    providers: Vec<UsageProviderPoint>,
}

#[derive(Debug, Clone, Serialize)]
struct UsageProviderPoint {
    provider_key: String,
    provider_name: String,
    request_count: usize,
    input_tokens: i64,
    uncached_input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
    total_tokens: i64,
    estimated_cost: f64,
    known: bool,
}

#[derive(Debug, Clone, Serialize)]
struct UsageMonthlyPoint {
    month: String,
    request_count: usize,
    total_tokens: i64,
    estimated_cost: f64,
}

#[derive(Debug, Clone, Serialize)]
struct UsageDetailRow {
    timestamp: String,
    day: String,
    session_id: String,
    provider_key: String,
    provider_name: String,
    model: String,
    input_tokens: i64,
    uncached_input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
    total_tokens: i64,
    estimated_cost: f64,
    cost_breakdown: String,
    pricing_model_match: String,
    pricing_source: String,
    currency: String,
    source: String,
}

#[derive(Debug, Clone, Serialize)]
struct UsageStats {
    generated_at_ms: i64,
    source_dir: String,
    filters: UsageStatsFilter,
    summary: UsageSummary,
    today: UsageSummary,
    this_month: UsageSummary,
    daily: Vec<UsageDailyPoint>,
    monthly: Vec<UsageMonthlyPoint>,
    providers: Vec<UsageProviderPoint>,
    details: Vec<UsageDetailRow>,
    pricing: Vec<PricingRule>,
    available_providers: Vec<UsageFilterOption>,
    available_models: Vec<String>,
    available_days: Vec<String>,
    unknown_provider_count: usize,
    parsed_files: usize,
    parsed_events: usize,
    filtered_events: usize,
    detail_page: usize,
    detail_page_size: usize,
    detail_total_pages: usize,
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

#[derive(Debug, Clone)]
struct UsageEvent {
    timestamp: DateTime<Utc>,
    day: String,
    month: String,
    session_id: String,
    provider_key: String,
    provider_name: String,
    provider_known: bool,
    model: String,
    usage: TokenUsage,
    estimated_cost: f64,
    cost_breakdown: String,
    pricing_model_match: String,
    pricing_source: String,
    currency: String,
    source: String,
}

#[derive(Debug, Clone, Default)]
struct UsageCache {
    events: Vec<UsageEvent>,
    source_dir: String,
    parsed_files: usize,
    loaded_at_ms: i64,
}

#[derive(Default)]
struct UsageCacheState {
    cache: Mutex<Option<UsageCache>>,
}

#[derive(Debug, Clone)]
struct CostEstimate {
    amount: f64,
    currency: String,
    breakdown: String,
    pricing_model_match: String,
    pricing_source: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct UsageStatsFilter {
    #[serde(default)]
    start_day: Option<String>,
    #[serde(default)]
    end_day: Option<String>,
    #[serde(default)]
    provider_key: Option<String>,
    #[serde(default)]
    provider_name: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    page: Option<usize>,
    #[serde(default)]
    page_size: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct LoadUsageStatsPayload {
    #[serde(default)]
    filter: Option<UsageStatsFilter>,
    #[serde(default)]
    force_refresh: bool,
}

#[derive(Debug, Clone, Serialize)]
struct UsageFilterOption {
    provider_key: String,
    provider_name: String,
    request_count: usize,
    known: bool,
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

fn sessions_dir() -> Result<PathBuf, String> {
    Ok(codex_home()?.join("sessions"))
}

fn default_state() -> ManagerState {
    ManagerState {
        active_provider_id: String::new(),
        applied_provider_id: None,
        base_template_name: "默认模板".to_string(),
        base: json!({}),
        providers: vec![],
        last_applied: None,
        applied_history: vec![],
        pricing: default_pricing_rules(),
    }
}

fn default_pricing_rules() -> Vec<PricingRule> {
    const SOURCE: &str = "OpenAI API pricing, standard GPT models, USD per 1M tokens";

    fn rule(id: &str, model: &str, input: f64, cached_input: f64, output: f64) -> PricingRule {
        PricingRule {
            id: id.to_string(),
            provider_match: "*".to_string(),
            model_match: model.to_string(),
            input_per_million: input,
            cached_input_per_million: cached_input,
            output_per_million: output,
            reasoning_output_per_million: 0.0,
            currency: "USD".to_string(),
            source: SOURCE.to_string(),
        }
    }

    vec![
        rule("gpt-5-5-pro", "gpt-5.5-pro*", 30.0, 0.0, 180.0),
        rule("gpt-5-5", "gpt-5.5*", 5.0, 0.5, 30.0),
        rule("gpt-5-4-pro", "gpt-5.4-pro*", 30.0, 0.0, 180.0),
        rule("gpt-5-4-mini", "gpt-5.4-mini*", 0.75, 0.075, 4.5),
        rule("gpt-5-4-nano", "gpt-5.4-nano*", 0.2, 0.02, 1.25),
        rule("gpt-5-4", "gpt-5.4*", 2.5, 0.25, 15.0),
        rule("gpt-5-3-codex", "gpt-5.3-codex*", 1.75, 0.175, 14.0),
        rule("gpt-5-3-chat", "gpt-5.3-chat-latest", 1.75, 0.175, 14.0),
        rule("gpt-5-2-pro", "gpt-5.2-pro*", 21.0, 0.0, 168.0),
        rule("gpt-5-2-codex", "gpt-5.2-codex*", 1.75, 0.175, 14.0),
        rule("gpt-5-2-chat", "gpt-5.2-chat-latest", 1.75, 0.175, 14.0),
        rule("gpt-5-2", "gpt-5.2*", 1.75, 0.175, 14.0),
        rule(
            "gpt-5-1-codex-mini",
            "gpt-5.1-codex-mini*",
            0.25,
            0.025,
            2.0,
        ),
        rule("gpt-5-1-codex", "gpt-5.1-codex*", 1.25, 0.125, 10.0),
        rule("gpt-5-1-chat", "gpt-5.1-chat-latest", 1.25, 0.125, 10.0),
        rule("gpt-5-1", "gpt-5.1*", 1.25, 0.125, 10.0),
        rule("gpt-5-codex", "gpt-5-codex*", 1.25, 0.125, 10.0),
        rule("gpt-5-pro", "gpt-5-pro*", 15.0, 0.0, 120.0),
        rule("gpt-5-mini", "gpt-5-mini*", 0.25, 0.025, 2.0),
        rule("gpt-5-nano", "gpt-5-nano*", 0.05, 0.005, 0.4),
        rule("gpt-5-chat", "gpt-5-chat-latest", 1.25, 0.125, 10.0),
        rule("gpt-5", "gpt-5*", 1.25, 0.125, 10.0),
        rule("codex-mini-latest", "codex-mini-latest", 1.5, 0.375, 6.0),
        rule("chat-latest", "chat-latest", 5.0, 0.5, 30.0),
        rule("gpt-4-1-mini", "gpt-4.1-mini*", 0.4, 0.1, 1.6),
        rule("gpt-4-1-nano", "gpt-4.1-nano*", 0.1, 0.025, 0.4),
        rule("gpt-4-1", "gpt-4.1*", 2.0, 0.5, 8.0),
        rule("gpt-4o-2024-05-13", "gpt-4o-2024-05-13", 5.0, 0.0, 15.0),
        rule("gpt-4o-mini", "gpt-4o-mini*", 0.15, 0.075, 0.6),
        rule("gpt-4o", "gpt-4o*", 2.5, 1.25, 10.0),
    ]
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

    let normalized = normalize_state(state.clone());
    let state_value =
        serde_json::to_value(&state).map_err(|err| format!("无法检查状态文件: {err}"))?;
    let normalized_value =
        serde_json::to_value(&normalized).map_err(|err| format!("无法检查状态文件: {err}"))?;
    if state_value != normalized_value {
        save_state(&normalized)?;
    }

    Ok(normalized)
}

fn normalize_state(mut state: ManagerState) -> ManagerState {
    state.pricing = default_pricing_rules();
    let provider_exists = |provider_id: &str| {
        state
            .providers
            .iter()
            .any(|provider| provider.id == provider_id)
    };
    let applied_provider_id = state
        .applied_provider_id
        .clone()
        .filter(|provider_id| provider_exists(provider_id))
        .or_else(|| {
            state
                .providers
                .iter()
                .find(|provider| provider.enabled)
                .map(|provider| provider.id.clone())
        })
        .or_else(|| {
            state.last_applied.as_ref().and_then(|last_applied| {
                state
                    .providers
                    .iter()
                    .find(|provider| desired_config(&state, Some(provider)) == *last_applied)
                    .map(|provider| provider.id.clone())
            })
        });
    if let Some(applied_provider_id) = applied_provider_id {
        state.applied_provider_id = Some(applied_provider_id.clone());
        if state.active_provider_id.trim().is_empty()
            || !state
                .providers
                .iter()
                .any(|provider| provider.id == state.active_provider_id)
        {
            state.active_provider_id = applied_provider_id.clone();
        }
        for provider in &mut state.providers {
            provider.enabled = provider.id == applied_provider_id;
        }
    }
    state
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

fn model_provider_name(provider: &ProviderConfig) -> String {
    provider
        .config
        .pointer("/model_provider")
        .and_then(Value::as_str)
        .unwrap_or("custom")
        .to_string()
}

fn hash_text(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn current_epoch_ms() -> Result<i64, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("系统时间异常: {err}"))?
        .as_millis() as i64)
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

fn collect_jsonl_files(dir: PathBuf, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in
        fs::read_dir(&dir).map_err(|err| format!("无法读取目录 {}: {err}", dir.display()))?
    {
        let entry = entry.map_err(|err| format!("无法读取目录项: {err}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }

    Ok(())
}

fn timestamp_to_local_parts(timestamp: DateTime<Utc>) -> (String, String) {
    let local = timestamp.with_timezone(&Local);
    (
        format!(
            "{:04}-{:02}-{:02}",
            local.year(),
            local.month(),
            local.day()
        ),
        format!("{:04}-{:02}", local.year(), local.month()),
    )
}

fn parse_event_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn usage_from_value(value: &Value) -> TokenUsage {
    TokenUsage {
        input_tokens: number_at_path(value, &["input_tokens"]).unwrap_or_default() as i64,
        cached_input_tokens: number_at_path(value, &["cached_input_tokens"]).unwrap_or_default()
            as i64,
        output_tokens: number_at_path(value, &["output_tokens"]).unwrap_or_default() as i64,
        reasoning_output_tokens: number_at_path(value, &["reasoning_output_tokens"])
            .unwrap_or_default() as i64,
        total_tokens: number_at_path(value, &["total_tokens"]).unwrap_or_default() as i64,
    }
}

fn usage_is_zero(usage: &TokenUsage) -> bool {
    usage.input_tokens == 0
        && usage.cached_input_tokens == 0
        && usage.output_tokens == 0
        && usage.reasoning_output_tokens == 0
        && usage.total_tokens == 0
}

fn usage_delta(current: &TokenUsage, previous: Option<&TokenUsage>) -> TokenUsage {
    if let Some(previous) = previous {
        TokenUsage {
            input_tokens: current.input_tokens.saturating_sub(previous.input_tokens),
            cached_input_tokens: current
                .cached_input_tokens
                .saturating_sub(previous.cached_input_tokens),
            output_tokens: current.output_tokens.saturating_sub(previous.output_tokens),
            reasoning_output_tokens: current
                .reasoning_output_tokens
                .saturating_sub(previous.reasoning_output_tokens),
            total_tokens: current.total_tokens.saturating_sub(previous.total_tokens),
        }
    } else {
        current.clone()
    }
}

fn add_usage(summary: &mut UsageSummary, usage: &TokenUsage, cost: f64) {
    summary.request_count += 1;
    summary.input_tokens += usage.input_tokens;
    summary.uncached_input_tokens += usage
        .input_tokens
        .saturating_sub(usage.cached_input_tokens)
        .max(0);
    summary.cached_input_tokens += usage.cached_input_tokens;
    summary.output_tokens += usage.output_tokens;
    summary.reasoning_output_tokens += usage.reasoning_output_tokens;
    summary.total_tokens += usage.total_tokens;
    summary.estimated_cost += cost;
}

fn add_event_usage(summary: &mut UsageSummary, event: &UsageEvent) {
    if summary.request_count == 0 {
        summary.currency = event.currency.clone();
    }
    add_usage(summary, &event.usage, event.estimated_cost);
}

fn matches_pricing_pattern(pattern: &str, value: &str) -> bool {
    let pattern = pattern.trim().to_lowercase();
    let value = value.trim().to_lowercase();

    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    pattern == value
}

fn pricing_specificity(pattern: &str) -> i32 {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern == "*" {
        0
    } else if pattern.ends_with('*') {
        100 + pattern.trim_end_matches('*').len() as i32
    } else {
        10_000 + pattern.len() as i32
    }
}

fn select_pricing_rule<'a>(
    pricing: &'a [PricingRule],
    provider_key: &str,
    provider_name: &str,
    model: &str,
) -> Option<&'a PricingRule> {
    pricing
        .iter()
        .filter(|rule| {
            let provider_matches = matches_pricing_pattern(&rule.provider_match, provider_key)
                || matches_pricing_pattern(&rule.provider_match, provider_name);
            provider_matches && matches_pricing_pattern(&rule.model_match, model)
        })
        .max_by_key(|rule| {
            pricing_specificity(&rule.provider_match) + pricing_specificity(&rule.model_match) * 2
        })
}

fn estimate_cost(
    pricing: &[PricingRule],
    provider_key: &str,
    provider_name: &str,
    model: &str,
    usage: &TokenUsage,
) -> CostEstimate {
    let rule = select_pricing_rule(pricing, provider_key, provider_name, model).cloned();
    let rule = rule.unwrap_or_else(|| PricingRule {
        id: "unpriced".to_string(),
        provider_match: "*".to_string(),
        model_match: "未匹配官方 GPT 价格".to_string(),
        input_per_million: 0.0,
        cached_input_per_million: 0.0,
        output_per_million: 0.0,
        reasoning_output_per_million: 0.0,
        currency: "USD".to_string(),
        source: "未匹配到 OpenAI GPT 官方价格，按 0 估算".to_string(),
    });

    let million = 1_000_000.0;
    let billable_input_tokens = usage
        .input_tokens
        .saturating_sub(usage.cached_input_tokens)
        .max(0);
    let input_cost = billable_input_tokens as f64 / million * rule.input_per_million;
    let cached_input_cost =
        usage.cached_input_tokens as f64 / million * rule.cached_input_per_million;
    let output_cost = usage.output_tokens as f64 / million * rule.output_per_million;
    let reasoning_cost =
        usage.reasoning_output_tokens as f64 / million * rule.reasoning_output_per_million;
    let cost = input_cost + cached_input_cost + output_cost + reasoning_cost;
    let source = if rule.source.trim().is_empty() {
        "OpenAI API pricing, USD per 1M tokens".to_string()
    } else {
        rule.source.clone()
    };
    let mut breakdown = vec![
        format!(
            "模型匹配: {}",
            if rule.model_match.trim().is_empty() {
                "未匹配"
            } else {
                rule.model_match.as_str()
            }
        ),
        format!(
            "输入: {} tokens × ${:.4}/1M = ${:.6}",
            billable_input_tokens, rule.input_per_million, input_cost
        ),
        format!(
            "缓存输入: {} tokens × ${:.4}/1M = ${:.6}",
            usage.cached_input_tokens, rule.cached_input_per_million, cached_input_cost
        ),
        format!(
            "输出: {} tokens × ${:.4}/1M = ${:.6}",
            usage.output_tokens, rule.output_per_million, output_cost
        ),
    ];
    if usage.reasoning_output_tokens > 0 {
        if rule.reasoning_output_per_million > 0.0 {
            breakdown.push(format!(
                "推理输出: {} tokens × ${:.4}/1M = ${:.6}",
                usage.reasoning_output_tokens, rule.reasoning_output_per_million, reasoning_cost
            ));
        } else {
            breakdown.push(format!(
                "推理输出: {} tokens，作为输出细分展示，不重复计费",
                usage.reasoning_output_tokens
            ));
        }
    }
    breakdown.push(format!("合计: ${cost:.6} {}", rule.currency));
    breakdown.push(format!("来源: {source}"));

    CostEstimate {
        amount: cost,
        currency: rule.currency,
        breakdown: breakdown.join("\n"),
        pricing_model_match: rule.model_match,
        pricing_source: source,
    }
}

fn provider_snapshot_at<'a>(
    state: &'a ManagerState,
    provider_key: &str,
    timestamp_ms: i64,
) -> Option<&'a AppliedProviderSnapshot> {
    state
        .applied_history
        .iter()
        .filter(|snapshot| {
            snapshot.model_provider.eq_ignore_ascii_case(provider_key)
                && snapshot.applied_at_ms <= timestamp_ms
        })
        .max_by_key(|snapshot| snapshot.applied_at_ms)
}

fn inferred_generic_provider(state: &ManagerState, provider_key: &str) -> Option<String> {
    if let Some(applied_provider_id) = state.applied_provider_id.as_deref() {
        if let Some(provider) = state.providers.iter().find(|provider| {
            provider.id == applied_provider_id
                && model_provider_name(provider).eq_ignore_ascii_case(provider_key)
        }) {
            return Some(provider.name.clone());
        }
    }

    let matching_providers = state
        .providers
        .iter()
        .filter(|provider| model_provider_name(provider).eq_ignore_ascii_case(provider_key))
        .collect::<Vec<_>>();
    if matching_providers.len() == 1 {
        return Some(matching_providers[0].name.clone());
    }

    let matching_history = state
        .applied_history
        .iter()
        .filter(|snapshot| snapshot.model_provider.eq_ignore_ascii_case(provider_key))
        .collect::<Vec<_>>();
    let mut names = matching_history
        .iter()
        .map(|snapshot| snapshot.provider_name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    if names.len() == 1 {
        return names.pop();
    }

    None
}

fn resolve_provider(
    state: &ManagerState,
    provider_key: &str,
    timestamp: DateTime<Utc>,
) -> (String, bool) {
    let provider_key = provider_key.trim();
    if provider_key.is_empty() {
        return ("未知供应商".to_string(), false);
    }

    if let Some(provider) = state.providers.iter().find(|provider| {
        provider.id.eq_ignore_ascii_case(provider_key)
            || provider.name.eq_ignore_ascii_case(provider_key)
    }) {
        return (provider.name.clone(), true);
    }

    let timestamp_ms = timestamp.timestamp_millis();
    if let Some(snapshot) = provider_snapshot_at(state, provider_key, timestamp_ms) {
        return (snapshot.provider_name.clone(), true);
    }

    let generic_provider_key = ["custom", "3rd", "unknown"]
        .iter()
        .any(|value| value.eq_ignore_ascii_case(provider_key));
    if generic_provider_key {
        if let Some(provider_name) = inferred_generic_provider(state, provider_key) {
            return (provider_name, true);
        }
        return (format!("未知供应商 · {provider_key}"), false);
    }

    let matching_providers = state
        .providers
        .iter()
        .filter(|provider| model_provider_name(provider).eq_ignore_ascii_case(provider_key))
        .collect::<Vec<_>>();
    if matching_providers.len() == 1 {
        return (matching_providers[0].name.clone(), true);
    }

    (format!("未知供应商 · {provider_key}"), false)
}

fn parse_session_usage_file(
    path: &PathBuf,
    state: &ManagerState,
) -> Result<Vec<UsageEvent>, String> {
    let file = fs::File::open(path)
        .map_err(|err| format!("无法打开会话文件 {}: {err}", path.display()))?;
    let reader = BufReader::new(file);
    let mut session_id = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut provider_key = "unknown".to_string();
    let mut current_model = "unknown".to_string();
    let mut previous_total: Option<TokenUsage> = None;
    let mut events = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(|err| format!("无法读取会话文件 {}: {err}", path.display()))?;
        let value = match serde_json::from_str::<Value>(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let payload = match value.get("payload").and_then(Value::as_object) {
            Some(payload) => payload,
            None => continue,
        };
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if event_type == "session_meta" {
            if let Some(id) = payload.get("id").and_then(Value::as_str) {
                session_id = id.to_string();
            }
            if let Some(found) = payload.get("model_provider").and_then(Value::as_str) {
                provider_key = found.to_string();
            }
            continue;
        }

        if event_type == "turn_context" {
            if let Some(found) = payload.get("model").and_then(Value::as_str) {
                current_model = found.to_string();
            }
            continue;
        }

        if event_type != "event_msg"
            || payload.get("type").and_then(Value::as_str) != Some("token_count")
        {
            continue;
        }

        let timestamp = match parse_event_timestamp(&value) {
            Some(timestamp) => timestamp,
            None => continue,
        };
        let info = match payload.get("info").and_then(Value::as_object) {
            Some(info) => info,
            None => continue,
        };
        let last_usage = info
            .get("last_token_usage")
            .map(usage_from_value)
            .unwrap_or_default();
        let total_usage = info.get("total_token_usage").map(usage_from_value);
        let usage = if let Some(total_usage) = total_usage {
            let delta = usage_delta(&total_usage, previous_total.as_ref());
            previous_total = Some(total_usage);
            if usage_is_zero(&delta) {
                continue;
            }
            delta
        } else {
            if usage_is_zero(&last_usage) {
                continue;
            }
            last_usage
        };

        let (day, month) = timestamp_to_local_parts(timestamp);
        let (provider_name, provider_known) = resolve_provider(state, &provider_key, timestamp);
        let cost = estimate_cost(
            &state.pricing,
            &provider_key,
            &provider_name,
            &current_model,
            &usage,
        );

        events.push(UsageEvent {
            timestamp,
            day,
            month,
            session_id: session_id.clone(),
            provider_key: provider_key.clone(),
            provider_name,
            provider_known,
            model: current_model.clone(),
            usage,
            estimated_cost: cost.amount,
            cost_breakdown: cost.breakdown,
            pricing_model_match: cost.pricing_model_match,
            pricing_source: cost.pricing_source,
            currency: cost.currency,
            source: path.display().to_string(),
        });
    }

    Ok(events)
}

fn group_provider_points(events: &[&UsageEvent]) -> Vec<UsageProviderPoint> {
    let mut map = BTreeMap::<String, UsageProviderPoint>::new();
    for event in events {
        let entry = map
            .entry(format!("{}::{}", event.provider_key, event.provider_name))
            .or_insert_with(|| UsageProviderPoint {
                provider_key: event.provider_key.clone(),
                provider_name: event.provider_name.clone(),
                request_count: 0,
                input_tokens: 0,
                uncached_input_tokens: 0,
                cached_input_tokens: 0,
                output_tokens: 0,
                reasoning_output_tokens: 0,
                total_tokens: 0,
                estimated_cost: 0.0,
                known: event.provider_known,
            });
        entry.request_count += 1;
        entry.input_tokens += event.usage.input_tokens;
        entry.uncached_input_tokens += event
            .usage
            .input_tokens
            .saturating_sub(event.usage.cached_input_tokens)
            .max(0);
        entry.cached_input_tokens += event.usage.cached_input_tokens;
        entry.output_tokens += event.usage.output_tokens;
        entry.reasoning_output_tokens += event.usage.reasoning_output_tokens;
        entry.total_tokens += event.usage.total_tokens;
        entry.estimated_cost += event.estimated_cost;
        entry.known = entry.known || event.provider_known;
    }

    let mut points = map.into_values().collect::<Vec<_>>();
    points.sort_by(|left, right| {
        right
            .request_count
            .cmp(&left.request_count)
            .then_with(|| right.total_tokens.cmp(&left.total_tokens))
    });
    points
}

fn normalize_filter_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value != "*")
}

fn usage_event_matches_filter(event: &UsageEvent, filter: &UsageStatsFilter) -> bool {
    if let Some(start_day) = normalize_filter_text(filter.start_day.clone()) {
        if event.day.as_str() < start_day.as_str() {
            return false;
        }
    }
    if let Some(end_day) = normalize_filter_text(filter.end_day.clone()) {
        if event.day.as_str() > end_day.as_str() {
            return false;
        }
    }
    if let Some(provider_key) = normalize_filter_text(filter.provider_key.clone()) {
        if !event.provider_key.eq_ignore_ascii_case(&provider_key) {
            return false;
        }
    }
    if let Some(provider_name) = normalize_filter_text(filter.provider_name.clone()) {
        if !event.provider_name.eq_ignore_ascii_case(&provider_name) {
            return false;
        }
    }
    if let Some(model) = normalize_filter_text(filter.model.clone()) {
        if !event.model.eq_ignore_ascii_case(&model) {
            return false;
        }
    }
    true
}

fn collect_available_providers(events: &[UsageEvent]) -> Vec<UsageFilterOption> {
    let mut map = BTreeMap::<String, UsageFilterOption>::new();
    for event in events {
        let entry = map
            .entry(format!("{}::{}", event.provider_key, event.provider_name))
            .or_insert_with(|| UsageFilterOption {
                provider_key: event.provider_key.clone(),
                provider_name: event.provider_name.clone(),
                request_count: 0,
                known: event.provider_known,
            });
        entry.request_count += 1;
        entry.known = entry.known || event.provider_known;
    }

    let mut providers = map.into_values().collect::<Vec<_>>();
    providers.sort_by(|left, right| {
        right
            .request_count
            .cmp(&left.request_count)
            .then_with(|| left.provider_name.cmp(&right.provider_name))
    });
    providers
}

fn collect_available_models(events: &[UsageEvent]) -> Vec<String> {
    let mut models = events
        .iter()
        .map(|event| event.model.clone())
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}

fn collect_available_days(events: &[UsageEvent]) -> Vec<String> {
    let mut days = events
        .iter()
        .map(|event| event.day.clone())
        .collect::<Vec<_>>();
    days.sort();
    days.dedup();
    days
}

fn usage_detail_row(event: &UsageEvent) -> UsageDetailRow {
    UsageDetailRow {
        timestamp: event
            .timestamp
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        day: event.day.clone(),
        session_id: event.session_id.clone(),
        provider_key: event.provider_key.clone(),
        provider_name: event.provider_name.clone(),
        model: event.model.clone(),
        input_tokens: event.usage.input_tokens,
        uncached_input_tokens: event
            .usage
            .input_tokens
            .saturating_sub(event.usage.cached_input_tokens)
            .max(0),
        cached_input_tokens: event.usage.cached_input_tokens,
        output_tokens: event.usage.output_tokens,
        reasoning_output_tokens: event.usage.reasoning_output_tokens,
        total_tokens: event.usage.total_tokens,
        estimated_cost: event.estimated_cost,
        cost_breakdown: event.cost_breakdown.clone(),
        pricing_model_match: event.pricing_model_match.clone(),
        pricing_source: event.pricing_source.clone(),
        currency: event.currency.clone(),
        source: event.source.clone(),
    }
}

fn normalized_page_size(page_size: Option<usize>) -> usize {
    match page_size.unwrap_or(50) {
        0..=20 => 20,
        21..=50 => 50,
        51..=100 => 100,
        _ => 100,
    }
}

fn load_usage_cache(state: &ManagerState) -> Result<UsageCache, String> {
    let sessions_dir = sessions_dir()?;
    let mut files = Vec::new();
    collect_jsonl_files(sessions_dir.clone(), &mut files)?;
    files.sort();

    let mut events = Vec::new();
    let mut parsed_files = 0;
    for file in files {
        let file_events = parse_session_usage_file(&file, state)?;
        if !file_events.is_empty() {
            parsed_files += 1;
        }
        events.extend(file_events);
    }
    events.sort_by_key(|event| event.timestamp);

    Ok(UsageCache {
        events,
        source_dir: sessions_dir.display().to_string(),
        parsed_files,
        loaded_at_ms: current_epoch_ms()?,
    })
}

fn build_usage_stats_from_cache(
    cache: &UsageCache,
    filter: UsageStatsFilter,
) -> Result<UsageStats, String> {
    let events = &cache.events;
    let available_providers = collect_available_providers(events);
    let available_models = collect_available_models(events);
    let available_days = collect_available_days(events);

    let now = Local::now();
    let today_key = format!("{:04}-{:02}-{:02}", now.year(), now.month(), now.day());
    let this_month_key = format!("{:04}-{:02}", now.year(), now.month());

    let mut today = UsageSummary::default();
    let mut this_month = UsageSummary::default();
    for event in events {
        if event.day == today_key {
            add_event_usage(&mut today, event);
        }
        if event.month == this_month_key {
            add_event_usage(&mut this_month, event);
        }
    }

    let filtered_events = events
        .iter()
        .filter(|event| usage_event_matches_filter(event, &filter))
        .collect::<Vec<_>>();

    let mut summary = UsageSummary::default();
    let mut daily_map = BTreeMap::<String, Vec<&UsageEvent>>::new();
    let mut monthly_map = BTreeMap::<String, Vec<&UsageEvent>>::new();

    for event in &filtered_events {
        add_event_usage(&mut summary, event);
        daily_map.entry(event.day.clone()).or_default().push(event);
        monthly_map
            .entry(event.month.clone())
            .or_default()
            .push(event);
    }

    let mut daily = daily_map
        .into_iter()
        .map(|(day, day_events)| {
            let mut total = UsageSummary::default();
            for event in &day_events {
                add_event_usage(&mut total, event);
            }
            UsageDailyPoint {
                day,
                request_count: total.request_count,
                total_tokens: total.total_tokens,
                estimated_cost: total.estimated_cost,
                providers: group_provider_points(&day_events),
            }
        })
        .collect::<Vec<_>>();
    daily.sort_by(|left, right| left.day.cmp(&right.day));

    let monthly = monthly_map
        .into_iter()
        .map(|(month, month_events)| {
            let mut total = UsageSummary::default();
            for event in month_events {
                add_event_usage(&mut total, event);
            }
            UsageMonthlyPoint {
                month,
                request_count: total.request_count,
                total_tokens: total.total_tokens,
                estimated_cost: total.estimated_cost,
            }
        })
        .collect::<Vec<_>>();

    let providers = group_provider_points(&filtered_events);
    let unknown_provider_count = providers.iter().filter(|provider| !provider.known).count();
    let page_size = normalized_page_size(filter.page_size);
    let detail_total_pages = filtered_events.len().div_ceil(page_size).max(1);
    let detail_page = filter.page.unwrap_or(1).clamp(1, detail_total_pages);
    let start = (detail_page - 1) * page_size;

    let details = filtered_events
        .iter()
        .rev()
        .skip(start)
        .take(page_size)
        .map(|event| usage_detail_row(event))
        .collect::<Vec<_>>();

    Ok(UsageStats {
        generated_at_ms: cache.loaded_at_ms,
        source_dir: cache.source_dir.clone(),
        filters: filter,
        summary,
        today,
        this_month,
        daily,
        monthly,
        providers,
        details,
        pricing: default_pricing_rules(),
        available_providers,
        available_models,
        available_days,
        unknown_provider_count,
        parsed_files: cache.parsed_files,
        parsed_events: events.len(),
        filtered_events: filtered_events.len(),
        detail_page,
        detail_page_size: page_size,
        detail_total_pages,
    })
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
        if let Some(provider) = provider.as_ref() {
            state.applied_history.push(AppliedProviderSnapshot {
                provider_id: provider.id.clone(),
                provider_name: provider.name.clone(),
                model_provider: model_provider_name(provider),
                base_url_hash: custom_provider_base_url(provider)
                    .map(|base_url| hash_text(&base_url)),
                applied_at_ms: current_epoch_ms()?,
            });
            if state.applied_history.len() > 300 {
                state
                    .applied_history
                    .drain(0..state.applied_history.len() - 300);
            }
        }
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
fn load_usage_stats(
    payload: Option<LoadUsageStatsPayload>,
    usage_cache: tauri::State<UsageCacheState>,
) -> Result<UsageStats, String> {
    let state = load_state_file()?;
    let payload = payload.unwrap_or_default();
    let filter = payload.filter.unwrap_or_default();

    if !payload.force_refresh {
        if let Some(cache) = usage_cache
            .cache
            .lock()
            .map_err(|_| "统计缓存锁定失败".to_string())?
            .clone()
        {
            return build_usage_stats_from_cache(&cache, filter);
        }
    }

    let cache = load_usage_cache(&state)?;
    let stats = build_usage_stats_from_cache(&cache, filter)?;
    *usage_cache
        .cache
        .lock()
        .map_err(|_| "统计缓存锁定失败".to_string())? = Some(cache);
    Ok(stats)
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
        .manage(UsageCacheState::default())
        .invoke_handler(tauri::generate_handler![
            load_app_state,
            select_provider,
            save_provider,
            preview_provider,
            add_provider,
            save_base_template,
            apply_config,
            load_usage_stats,
            query_provider_balance
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
