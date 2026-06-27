use axum::{
    body::{Body, Bytes},
    extract::{Path, State as AxumState},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri},
    response::Response,
    routing::any,
    Router,
};
use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use futures_util::{stream::BoxStream, StreamExt};
use reqwest::header::{
    AUTHORIZATION, CONNECTION, CONTENT_LENGTH, HOST, TRANSFER_ENCODING, UPGRADE,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;
use tokio::sync::oneshot;
use toml_edit::{Array, DocumentMut, Item, Value as TomlValue};

const MARKER: &str = "# managed-by: codex-config-manager";
const DEFAULT_ROUTER_HOST: &str = "127.0.0.1";
const DEFAULT_ROUTER_PORT: u16 = 18080;
const DEFAULT_ROUTER_TOKEN: &str = "codex-helper-local-token";
const MAX_PROXY_BODY_BYTES: usize = 32 * 1024 * 1024;
static ROUTE_LOG_SEQUENCE: AtomicU64 = AtomicU64::new(1);

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
struct RouterConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_router_host")]
    host: String,
    #[serde(default = "default_router_port")]
    port: u16,
    #[serde(default = "default_router_token")]
    local_token: String,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_router_host(),
            port: default_router_port(),
            local_token: default_router_token(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct RouterStatus {
    running: bool,
    address: String,
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
    #[serde(default)]
    router: RouterConfig,
    #[serde(default)]
    router_backup: Option<RouterApplyBackup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RouterApplyBackup {
    #[serde(default)]
    model_provider: RouterFieldBackup,
    #[serde(default)]
    custom_base_url: RouterFieldBackup,
    #[serde(default)]
    custom_token: RouterFieldBackup,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RouterFieldBackup {
    #[serde(default)]
    existed: bool,
    #[serde(default)]
    value: Option<Value>,
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
    base_url: String,
    provider_type: String,
    route_order: usize,
    balance_label: String,
    balance_error: Option<String>,
    latency_label: String,
    last_checked_label: String,
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
    router: RouterConfig,
    router_status: RouterStatus,
}

#[derive(Debug, Deserialize)]
struct SaveProviderPayload {
    provider_id: String,
    provider_name: Option<String>,
    config_toml: String,
    balance_query: Option<BalanceQueryConfig>,
    balance_status: Option<BalanceStatus>,
    enabled: Option<bool>,
    base_url: Option<String>,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SaveBasePayload {
    base_template_name: String,
    base_toml: String,
}

#[derive(Debug, Deserialize)]
struct SaveRouterPayload {
    enabled: bool,
    host: String,
    port: u16,
    local_token: String,
}

#[derive(Debug, Deserialize)]
struct QueryBalancePayload {
    provider_id: String,
    balance_query: Option<BalanceQueryConfig>,
    base_url: Option<String>,
    api_key: Option<String>,
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

#[derive(Default)]
struct RouterRuntime {
    handle: Mutex<Option<RouterHandle>>,
}

struct RouterHandle {
    address: String,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Drop for RouterHandle {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

#[derive(Clone)]
struct ProxyState {
    client: reqwest::Client,
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

#[derive(Debug, Clone)]
struct UpstreamCandidate {
    provider: ProviderConfig,
    base_url: String,
    token: String,
    route_order: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RouteRequestLog {
    id: String,
    started_at_ms: i64,
    day: String,
    hour: String,
    method: String,
    path: String,
    model: String,
    provider_id: String,
    provider_name: String,
    provider_order: usize,
    upstream_chain: Vec<String>,
    status: String,
    status_code: Option<u16>,
    error: Option<String>,
    route_result: String,
    route_attempts: usize,
    input_tokens: i64,
    uncached_input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
    total_tokens: i64,
    estimated_cost: f64,
    currency: String,
    cost_breakdown: String,
    pricing_model_match: String,
    pricing_source: String,
    first_byte_ms: Option<u64>,
    total_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RouteLogFilter {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    provider_name: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    start_day: Option<String>,
    #[serde(default)]
    end_day: Option<String>,
    #[serde(default)]
    page: Option<usize>,
    #[serde(default)]
    page_size: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct LoadRouteLogsPayload {
    #[serde(default)]
    filter: Option<RouteLogFilter>,
}

#[derive(Debug, Clone, Serialize)]
struct RouteLogFilterOption {
    id: String,
    name: String,
    request_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct RouteLogsResponse {
    logs: Vec<RouteRequestLog>,
    total: usize,
    page: usize,
    page_size: usize,
    total_pages: usize,
    available_providers: Vec<RouteLogFilterOption>,
    available_models: Vec<String>,
    available_days: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RouteUsageBucket {
    label: String,
    request_count: usize,
    input_tokens: i64,
    uncached_input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    estimated_cost: f64,
}

#[derive(Debug, Clone, Serialize)]
struct RouteUsageBreakdown {
    key: String,
    label: String,
    request_count: usize,
    input_tokens: i64,
    uncached_input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    estimated_cost: f64,
}

#[derive(Debug, Clone, Serialize)]
struct RouteUsageStats {
    generated_at_ms: i64,
    filters: RouteLogFilter,
    summary: UsageSummary,
    today: UsageSummary,
    failed_count: usize,
    success_count: usize,
    running_count: usize,
    average_first_byte_ms: Option<u64>,
    average_total_ms: Option<u64>,
    bucket_granularity: String,
    buckets: Vec<RouteUsageBucket>,
    providers: Vec<RouteUsageBreakdown>,
    models: Vec<RouteUsageBreakdown>,
    details: Vec<RouteRequestLog>,
    total: usize,
    page: usize,
    page_size: usize,
    total_pages: usize,
    available_providers: Vec<RouteLogFilterOption>,
    available_models: Vec<String>,
    available_days: Vec<String>,
}

#[derive(Debug, Clone)]
struct PendingRouteLog {
    id: String,
    started_at_ms: i64,
    method: String,
    path: String,
    model: String,
    provider_id: String,
    provider_name: String,
    provider_order: usize,
    upstream_chain: Vec<String>,
    status_code: Option<u16>,
    route_result: String,
    route_attempts: usize,
    error: Option<String>,
    start: Instant,
}

struct RouteStreamState {
    stream: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    pending: PendingRouteLog,
    status_success: bool,
    first_byte_ms: Option<u64>,
    sse_buffer: String,
    usage: TokenUsage,
    finished: bool,
}

fn default_balance_path() -> String {
    "/api/usage/token/".to_string()
}

fn default_router_host() -> String {
    DEFAULT_ROUTER_HOST.to_string()
}

fn default_router_port() -> u16 {
    DEFAULT_ROUTER_PORT
}

fn default_router_token() -> String {
    random_router_token()
}

fn random_router_token() -> String {
    let mut bytes = [0_u8; 32];
    if getrandom::fill(&mut bytes).is_ok() {
        let encoded = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("codex-helper-{encoded}")
    } else {
        let fallback = current_epoch_ms().unwrap_or_default();
        let sequence = ROUTE_LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        format!("codex-helper-{fallback:x}-{sequence:x}")
    }
}

fn router_address(config: &RouterConfig) -> String {
    let host = config.host.trim();
    let host = if host.is_empty() {
        DEFAULT_ROUTER_HOST
    } else {
        host
    };
    format!("{host}:{}", config.port)
}

fn router_base_url(config: &RouterConfig) -> String {
    format!("http://{}/v1", router_address(config))
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

fn route_logs_path() -> Result<PathBuf, String> {
    Ok(manager_dir()?.join("route-logs.jsonl"))
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
        router: RouterConfig::default(),
        router_backup: None,
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
    if state.router.local_token.trim().is_empty()
        || state.router.local_token == DEFAULT_ROUTER_TOKEN
    {
        state.router.local_token = random_router_token();
    }
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
    } else if state.active_provider_id.trim().is_empty()
        || !state
            .providers
            .iter()
            .any(|provider| provider.id == state.active_provider_id)
    {
        state.active_provider_id = state
            .providers
            .first()
            .map(|provider| provider.id.clone())
            .unwrap_or_default();
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

fn set_json_path(value: &mut Value, path: &[&str], next: Value) -> Result<(), String> {
    if path.is_empty() {
        *value = next;
        return Ok(());
    }
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }

    let mut current = value;
    for key in &path[..path.len() - 1] {
        let map = current
            .as_object_mut()
            .ok_or_else(|| format!("路径 {key} 不是对象"))?;
        current = map
            .entry((*key).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !current.is_object() {
            *current = Value::Object(Map::new());
        }
    }

    let map = current
        .as_object_mut()
        .ok_or_else(|| "无法写入 JSON 路径".to_string())?;
    map.insert(path[path.len() - 1].to_string(), next);
    Ok(())
}

fn desired_config(state: &ManagerState, provider: Option<&ProviderConfig>) -> Value {
    let mut desired = state.base.clone();
    if state.router.enabled {
        merge_values(
            &mut desired,
            &json!({
                "model_provider": "custom",
                "model_providers": {
                    "custom": {
                        "base_url": router_base_url(&state.router),
                        "experimental_bearer_token": state.router.local_token,
                    }
                }
            }),
        );
    } else if let Some(provider) = provider {
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

fn provider_type(provider: &ProviderConfig) -> String {
    match provider.balance_query.query_type {
        BalanceQueryType::NewApi => "New API".to_string(),
        BalanceQueryType::Sub2Api => "Sub2API".to_string(),
        BalanceQueryType::Disabled => "通用兼容".to_string(),
    }
}

fn model_provider_name(provider: &ProviderConfig) -> String {
    provider
        .config
        .pointer("/model_provider")
        .and_then(Value::as_str)
        .unwrap_or("custom")
        .to_string()
}

fn redacted_provider(mut provider: ProviderConfig) -> ProviderConfig {
    if let Err(err) = set_json_path(
        &mut provider.config,
        &["model_providers", "custom", "experimental_bearer_token"],
        Value::String(String::new()),
    ) {
        eprintln!("无法脱敏供应商 Token: {err}");
    }
    provider.balance_query.query_token.clear();
    provider
}

fn redacted_config_value(mut value: Value) -> Value {
    if value
        .pointer("/model_providers/custom/experimental_bearer_token")
        .is_some()
    {
        if let Err(err) = set_json_path(
            &mut value,
            &["model_providers", "custom", "experimental_bearer_token"],
            Value::String(String::new()),
        ) {
            eprintln!("无法脱敏配置 Token: {err}");
        }
    }
    value
}

fn redacted_toml_text(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let Ok(mut doc) = raw.parse::<DocumentMut>() else {
        return raw.to_string();
    };
    if toml_path_value(&doc, "model_providers.custom.experimental_bearer_token").is_some() {
        if let Err(err) = set_toml_path(
            &mut doc,
            "model_providers.custom.experimental_bearer_token",
            &Value::String(String::new()),
        ) {
            eprintln!("无法脱敏 TOML Token: {err}");
        }
    }
    doc.to_string()
}

fn redacted_path_value(path: &str, value: &Value) -> Value {
    if path == "model_providers.custom.experimental_bearer_token" {
        Value::String(String::new())
    } else {
        value.clone()
    }
}

fn redacted_diff(mut diff: DiffEntry) -> DiffEntry {
    if diff.path == "model_providers.custom.experimental_bearer_token" {
        diff.current = diff.current.map(|_| Value::String(String::new()));
        diff.desired = diff.desired.map(|_| Value::String(String::new()));
    }
    diff
}

fn elapsed_label(checked_at: Option<u64>) -> String {
    let Some(checked_at) = checked_at else {
        return "未检查".to_string();
    };
    let Ok(now) = now_epoch_seconds() else {
        return "刚刚".to_string();
    };
    let elapsed = now.saturating_sub(checked_at);
    if elapsed < 60 {
        "刚刚".to_string()
    } else if elapsed < 3600 {
        format!("{} 分钟前", elapsed / 60)
    } else if elapsed < 86_400 {
        format!("{} 小时前", elapsed / 3600)
    } else {
        format!("{} 天前", elapsed / 86_400)
    }
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

fn merge_balance_config_draft(
    provider: &ProviderConfig,
    mut draft: BalanceQueryConfig,
) -> BalanceQueryConfig {
    if draft.auth_mode == BalanceAuthMode::SeparateToken && draft.query_token.trim().is_empty() {
        draft.query_token = provider.balance_query.query_token.clone();
    }
    normalize_balance_config(draft, provider)
}

fn apply_provider_connection_draft(
    provider: &mut ProviderConfig,
    base_url: Option<&str>,
    api_key: Option<&str>,
) -> Result<(), String> {
    if base_url.is_none() && api_key.is_none() {
        return Ok(());
    }

    let mut config = provider.config.clone();
    if let Some(base_url) = base_url {
        let base_url = base_url.trim();
        if base_url.is_empty() {
            return Err("Base URL 不能为空".to_string());
        }
        set_json_path(
            &mut config,
            &["model_provider"],
            Value::String("custom".to_string()),
        )?;
        set_json_path(
            &mut config,
            &["model_providers", "custom", "base_url"],
            Value::String(base_url.to_string()),
        )?;
    }
    if let Some(api_key) = api_key {
        set_json_path(
            &mut config,
            &["model_provider"],
            Value::String("custom".to_string()),
        )?;
        set_json_path(
            &mut config,
            &["model_providers", "custom", "experimental_bearer_token"],
            Value::String(api_key.trim().to_string()),
        )?;
    }
    provider.config = config;
    Ok(())
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
    if state.router.enabled
        && matches!(
            path,
            "model_provider"
                | "model_providers.custom.base_url"
                | "model_providers.custom.experimental_bearer_token"
        )
    {
        return "本地路由".to_string();
    }

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

fn toml_path_value(doc: &DocumentMut, path: &str) -> Option<Value> {
    let mut table = doc.as_table();
    let parts = path.split('.').collect::<Vec<_>>();
    let (last, parents) = parts.split_last()?;
    for part in parents {
        table = table.get(part)?.as_table()?;
    }
    table
        .get(last)
        .filter(|item| !item.is_none())
        .map(toml_item_to_json)
}

fn remove_toml_path(doc: &mut DocumentMut, path: &str) {
    let parts = path.split('.').collect::<Vec<_>>();
    let Some((last, parents)) = parts.split_last() else {
        return;
    };

    let mut table = doc.as_table_mut();
    for part in parents {
        let Some(next) = table.get_mut(part).and_then(Item::as_table_mut) else {
            return;
        };
        table = next;
    }
    table.remove(last);
}

fn capture_toml_field(doc: &DocumentMut, path: &str) -> RouterFieldBackup {
    let value = toml_path_value(doc, path);
    RouterFieldBackup {
        existed: value.is_some(),
        value,
    }
}

fn capture_router_backup(doc: &DocumentMut) -> RouterApplyBackup {
    RouterApplyBackup {
        model_provider: capture_toml_field(doc, "model_provider"),
        custom_base_url: capture_toml_field(doc, "model_providers.custom.base_url"),
        custom_token: capture_toml_field(doc, "model_providers.custom.experimental_bearer_token"),
    }
}

fn restore_toml_field(
    doc: &mut DocumentMut,
    path: &str,
    backup: &RouterFieldBackup,
) -> Result<(), String> {
    if backup.existed {
        if let Some(value) = backup.value.as_ref() {
            set_toml_path(doc, path, value)?;
        } else {
            remove_toml_path(doc, path);
        }
    } else {
        remove_toml_path(doc, path);
    }
    Ok(())
}

fn restore_router_backup(
    mut doc: DocumentMut,
    backup: Option<&RouterApplyBackup>,
    router: &RouterConfig,
) -> Result<String, String> {
    if let Some(backup) = backup {
        restore_toml_field(&mut doc, "model_provider", &backup.model_provider)?;
        restore_toml_field(
            &mut doc,
            "model_providers.custom.base_url",
            &backup.custom_base_url,
        )?;
        restore_toml_field(
            &mut doc,
            "model_providers.custom.experimental_bearer_token",
            &backup.custom_token,
        )?;
    } else {
        let desired = router_patch_desired(router);
        let desired_flat = flatten(&desired);
        let current = toml_doc_to_json(&doc);
        let current_flat = flatten(&current);
        for path in [
            "model_provider",
            "model_providers.custom.base_url",
            "model_providers.custom.experimental_bearer_token",
        ] {
            if desired_flat.get(path) == current_flat.get(path) {
                remove_toml_path(&mut doc, path);
            }
        }
    }
    Ok(doc.to_string())
}

fn render_router_patch_toml(
    mut doc: DocumentMut,
    marker_present: bool,
    router: &RouterConfig,
) -> Result<String, String> {
    set_toml_path(
        &mut doc,
        "model_provider",
        &Value::String("custom".to_string()),
    )?;
    set_toml_path(
        &mut doc,
        "model_providers.custom.base_url",
        &Value::String(router_base_url(router)),
    )?;
    set_toml_path(
        &mut doc,
        "model_providers.custom.experimental_bearer_token",
        &Value::String(router.local_token.clone()),
    )?;

    let mut raw = doc.to_string();
    if !marker_present && !raw.contains(MARKER) {
        raw = format!("{MARKER}\n{raw}");
    }

    Ok(raw)
}

fn router_patch_desired(router: &RouterConfig) -> Value {
    json!({
        "model_provider": "custom",
        "model_providers": {
            "custom": {
                "base_url": router_base_url(router),
                "experimental_bearer_token": router.local_token,
            }
        }
    })
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
    let path = path.trim().trim_start_matches('/');
    format!("{endpoint}/{path}")
}

fn is_hop_by_hop_header(name: &HeaderName) -> bool {
    name == CONNECTION
        || name == TRANSFER_ENCODING
        || name == UPGRADE
        || name.as_str().eq_ignore_ascii_case("keep-alive")
        || name.as_str().eq_ignore_ascii_case("te")
        || name.as_str().eq_ignore_ascii_case("trailer")
        || name.as_str().eq_ignore_ascii_case("proxy-authenticate")
        || name.as_str().eq_ignore_ascii_case("proxy-authorization")
}

fn proxy_error(status: StatusCode, message: impl Into<String>) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json; charset=utf-8")
        .body(Body::from(
            json!({ "error": { "message": message.into() } }).to_string(),
        ))
        .unwrap_or_else(|_| Response::new(Body::from("proxy error")))
}

fn upstream_candidates() -> Result<(RouterConfig, Vec<UpstreamCandidate>), String> {
    let state = load_state_file()?;
    if !state.router.enabled {
        return Err("本地路由未启用".to_string());
    }
    let candidates = state
        .providers
        .iter()
        .enumerate()
        .filter(|(_, provider)| provider.enabled)
        .filter_map(|(index, provider)| {
            let base_url =
                custom_provider_base_url(provider).filter(|value| !value.trim().is_empty())?;
            let token = custom_provider_token(provider).filter(|value| !value.trim().is_empty())?;
            Some(UpstreamCandidate {
                provider: provider.clone(),
                base_url,
                token,
                route_order: index + 1,
            })
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return Err("没有已启用且配置完整的供应商".to_string());
    }
    Ok((state.router, candidates))
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get("authorization")?.to_str().ok()?.trim();
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
}

fn model_from_request_body(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "未知模型".to_string())
}

fn usage_from_response_value(value: &Value) -> TokenUsage {
    value
        .get("usage")
        .or_else(|| value.pointer("/response/usage"))
        .map(usage_from_value)
        .unwrap_or_default()
}

fn usage_from_sse_event(event: &str) -> TokenUsage {
    let mut usage = TokenUsage::default();
    let mut data = String::new();
    for line in event.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix("data:") else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() || value == "[DONE]" {
            continue;
        }
        if !data.is_empty() {
            data.push('\n');
        }
        data.push_str(value);
    }
    if data.is_empty() {
        return usage;
    }
    if let Ok(value) = serde_json::from_str::<Value>(&data) {
        let next = usage_from_response_value(&value);
        if !usage_is_zero(&next) {
            usage = next;
        }
    }
    usage
}

fn next_sse_event_boundary(buffer: &str) -> Option<(usize, usize)> {
    let bytes = buffer.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                if index + 1 < bytes.len() && bytes[index + 1] == b'\n' {
                    return Some((index, 2));
                }
                if index + 2 < bytes.len() && bytes[index + 1] == b'\r' && bytes[index + 2] == b'\n'
                {
                    return Some((index, 3));
                }
            }
            b'\r' => {
                if index + 3 < bytes.len()
                    && bytes[index + 1] == b'\n'
                    && bytes[index + 2] == b'\r'
                    && bytes[index + 3] == b'\n'
                {
                    return Some((index, 4));
                }
                if index + 1 < bytes.len() && bytes[index + 1] == b'\r' {
                    return Some((index, 2));
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn ingest_sse_chunk(buffer: &mut String, usage: &mut TokenUsage, bytes: &[u8]) {
    buffer.push_str(&String::from_utf8_lossy(bytes));
    while let Some((index, delimiter_len)) = next_sse_event_boundary(buffer) {
        let event = buffer[..index].to_string();
        buffer.drain(..index + delimiter_len);
        let next = usage_from_sse_event(&event);
        if !usage_is_zero(&next) {
            *usage = next;
        }
    }

    if buffer.len() > 128 * 1024 {
        let keep_from = buffer.len().saturating_sub(64 * 1024);
        let tail = buffer[keep_from..].to_string();
        *buffer = tail;
    }
}

fn finish_sse_usage(buffer: &mut String, usage: &mut TokenUsage) {
    if !buffer.trim().is_empty() {
        let next = usage_from_sse_event(buffer);
        if !usage_is_zero(&next) {
            *usage = next;
        }
    }
    buffer.clear();
}

fn route_stream_ingest(state: &mut RouteStreamState, bytes: &[u8]) {
    let mut buffer = std::mem::take(&mut state.sse_buffer);
    let mut usage = std::mem::take(&mut state.usage);
    ingest_sse_chunk(&mut buffer, &mut usage, bytes);
    state.sse_buffer = buffer;
    state.usage = usage;
}

fn route_stream_finish_usage(state: &mut RouteStreamState) {
    let mut buffer = std::mem::take(&mut state.sse_buffer);
    let mut usage = std::mem::take(&mut state.usage);
    finish_sse_usage(&mut buffer, &mut usage);
    state.sse_buffer = buffer;
    state.usage = usage;
}

fn usage_from_response_text(text: &str) -> TokenUsage {
    let mut usage = serde_json::from_str::<Value>(text)
        .ok()
        .map(|value| usage_from_response_value(&value))
        .unwrap_or_default();
    if !usage_is_zero(&usage) {
        return usage;
    }

    for line in text.lines() {
        let line = line.trim();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(data) {
            let next = usage_from_response_value(&value);
            if !usage_is_zero(&next) {
                usage = next;
            }
        }
    }
    usage
}

fn finish_route_log(
    pending: PendingRouteLog,
    status_success: bool,
    usage: TokenUsage,
    first_byte_ms: Option<u64>,
) {
    let total_ms = pending.start.elapsed().as_millis() as u64;
    let (day, hour) = timestamp_to_route_parts(pending.started_at_ms);
    let cost = estimate_cost(
        &default_pricing_rules(),
        &pending.provider_id,
        &pending.provider_name,
        &pending.model,
        &usage,
    );
    let status = if pending.error.is_some() || !status_success {
        "failed"
    } else {
        "success"
    };
    let log = RouteRequestLog {
        id: pending.id,
        started_at_ms: pending.started_at_ms,
        day,
        hour,
        method: pending.method,
        path: pending.path,
        model: pending.model,
        provider_id: pending.provider_id,
        provider_name: pending.provider_name,
        provider_order: pending.provider_order,
        upstream_chain: pending.upstream_chain,
        status: status.to_string(),
        status_code: pending.status_code,
        error: pending.error,
        route_result: pending.route_result,
        route_attempts: pending.route_attempts,
        input_tokens: usage.input_tokens,
        uncached_input_tokens: usage
            .input_tokens
            .saturating_sub(usage.cached_input_tokens)
            .max(0),
        cached_input_tokens: usage.cached_input_tokens,
        output_tokens: usage.output_tokens,
        reasoning_output_tokens: usage.reasoning_output_tokens,
        total_tokens: usage.total_tokens,
        estimated_cost: cost.amount,
        currency: cost.currency,
        cost_breakdown: cost.breakdown,
        pricing_model_match: cost.pricing_model_match,
        pricing_source: cost.pricing_source,
        first_byte_ms,
        total_ms,
    };
    if let Err(err) = append_route_log(&log) {
        eprintln!("{err}");
    }
}

fn build_pending_route_log(
    started_at_ms: i64,
    start: Instant,
    candidate: &UpstreamCandidate,
    method: &Method,
    path: &str,
    model: &str,
    upstream_chain: &[String],
    status_code: Option<u16>,
    route_attempts: usize,
    error: Option<String>,
) -> PendingRouteLog {
    PendingRouteLog {
        id: request_id(started_at_ms),
        started_at_ms,
        method: method.as_str().to_string(),
        path: format!("/v1/{path}"),
        model: model.to_string(),
        provider_id: candidate.provider.id.clone(),
        provider_name: candidate.provider.name.clone(),
        provider_order: candidate.route_order,
        upstream_chain: upstream_chain.to_vec(),
        status_code,
        route_result: if route_attempts > 1 {
            format!("切换 {} 次", route_attempts - 1)
        } else if error.is_some() {
            "未完成".to_string()
        } else {
            "直连".to_string()
        },
        route_attempts,
        error,
        start,
    }
}

impl futures_util::Stream for RouteStreamState {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.stream.poll_next_unpin(cx) {
            std::task::Poll::Ready(Some(Ok(bytes))) => {
                if self.first_byte_ms.is_none() {
                    self.first_byte_ms = Some(self.pending.start.elapsed().as_millis() as u64);
                }
                route_stream_ingest(&mut self, bytes.as_ref());
                std::task::Poll::Ready(Some(Ok(bytes)))
            }
            std::task::Poll::Ready(Some(Err(err))) => {
                if !self.finished {
                    self.finished = true;
                    self.pending.error = Some(format!("读取上游流失败: {err}"));
                    route_stream_finish_usage(&mut self);
                    let pending = self.pending.clone();
                    let usage = self.usage.clone();
                    finish_route_log(pending, false, usage, self.first_byte_ms);
                }
                std::task::Poll::Ready(Some(Err(std::io::Error::other(err))))
            }
            std::task::Poll::Ready(None) => {
                if !self.finished {
                    self.finished = true;
                    route_stream_finish_usage(&mut self);
                    let pending = self.pending.clone();
                    let usage = self.usage.clone();
                    finish_route_log(pending, self.status_success, usage, self.first_byte_ms);
                }
                std::task::Poll::Ready(None)
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

async fn proxy_request(
    AxumState(proxy_state): AxumState<Arc<ProxyState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(path): Path<String>,
    body: Body,
) -> Response {
    let request_started = Instant::now();
    let request_started_at_ms = current_epoch_ms().unwrap_or_default();
    let (router, candidates) = match upstream_candidates() {
        Ok(config) => config,
        Err(err) => return proxy_error(StatusCode::BAD_GATEWAY, err),
    };
    if bearer_token(&headers) != Some(router.local_token.trim()) {
        return proxy_error(StatusCode::UNAUTHORIZED, "本地路由 Token 无效");
    }

    let query = uri
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let body_bytes = match axum::body::to_bytes(body, MAX_PROXY_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => return proxy_error(StatusCode::BAD_REQUEST, format!("无法读取请求体: {err}")),
    };
    let model = model_from_request_body(&body_bytes);

    let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(method) => method,
        Err(err) => return proxy_error(StatusCode::BAD_REQUEST, format!("请求方法无效: {err}")),
    };

    let mut upstream_chain = Vec::new();
    let mut last_error = String::new();
    for (attempt_index, candidate) in candidates.iter().enumerate() {
        upstream_chain.push(candidate.provider.name.clone());
        let upstream_url = join_url(&candidate.base_url, &path) + &query;
        let mut request = proxy_state
            .client
            .request(reqwest_method.clone(), upstream_url)
            .body(body_bytes.clone());

        for (name, value) in headers.iter() {
            if name == AUTHORIZATION
                || name == HOST
                || name == CONTENT_LENGTH
                || is_hop_by_hop_header(name)
            {
                continue;
            }
            request = request.header(name.as_str(), value.as_bytes());
        }
        request = request.header(AUTHORIZATION, format!("Bearer {}", candidate.token));

        let upstream = match request.send().await {
            Ok(response) => response,
            Err(err) => {
                last_error = format!("转发到 {} 失败: {err}", candidate.provider.name);
                if attempt_index + 1 < candidates.len() {
                    continue;
                }
                let pending = build_pending_route_log(
                    request_started_at_ms,
                    request_started,
                    candidate,
                    &method,
                    &path,
                    &model,
                    &upstream_chain,
                    None,
                    attempt_index + 1,
                    Some(last_error.clone()),
                );
                finish_route_log(pending, false, TokenUsage::default(), None);
                return proxy_error(StatusCode::BAD_GATEWAY, last_error);
            }
        };

        let status =
            StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let should_retry =
            matches!(status.as_u16(), 429 | 500..=599) && attempt_index + 1 < candidates.len();
        if should_retry {
            last_error = format!("{} 返回 {}", candidate.provider.name, status.as_u16());
            continue;
        }

        let pending = build_pending_route_log(
            request_started_at_ms,
            request_started,
            candidate,
            &method,
            &path,
            &model,
            &upstream_chain,
            Some(status.as_u16()),
            attempt_index + 1,
            if status.is_success() {
                None
            } else {
                Some(format!("上游返回 {}", status.as_u16()))
            },
        );
        let status_success = status.is_success();
        let content_type = upstream
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_lowercase();
        let is_event_stream = content_type.contains("text/event-stream");
        let response_headers = upstream.headers().clone();

        if is_event_stream {
            let mut builder = Response::builder().status(status);
            if let Some(headers_mut) = builder.headers_mut() {
                for (name, value) in response_headers.iter() {
                    if name == CONTENT_LENGTH || is_hop_by_hop_header(name) {
                        continue;
                    }
                    if let (Ok(header_name), Ok(header_value)) = (
                        HeaderName::from_bytes(name.as_str().as_bytes()),
                        HeaderValue::from_bytes(value.as_bytes()),
                    ) {
                        headers_mut.insert(header_name, header_value);
                    }
                }
            }
            let stream = RouteStreamState {
                stream: upstream.bytes_stream().boxed(),
                pending,
                status_success,
                first_byte_ms: None,
                sse_buffer: String::new(),
                usage: TokenUsage::default(),
                finished: false,
            };
            return builder
                .body(Body::from_stream(stream))
                .unwrap_or_else(|_| proxy_error(StatusCode::BAD_GATEWAY, "无法创建上游响应"));
        }

        let first_byte_ms = Some(pending.start.elapsed().as_millis() as u64);
        let bytes = match upstream.bytes().await {
            Ok(bytes) => bytes,
            Err(err) => {
                let mut failed = pending;
                failed.error = Some(format!("读取上游响应失败: {err}"));
                finish_route_log(failed, false, TokenUsage::default(), first_byte_ms);
                return proxy_error(StatusCode::BAD_GATEWAY, format!("读取上游响应失败: {err}"));
            }
        };
        let usage = usage_from_response_text(&String::from_utf8_lossy(&bytes));
        finish_route_log(pending, status_success, usage, first_byte_ms);

        let mut builder = Response::builder().status(status);
        if let Some(headers_mut) = builder.headers_mut() {
            for (name, value) in response_headers.iter() {
                if name == CONTENT_LENGTH || is_hop_by_hop_header(name) {
                    continue;
                }
                if let (Ok(header_name), Ok(header_value)) = (
                    HeaderName::from_bytes(name.as_str().as_bytes()),
                    HeaderValue::from_bytes(value.as_bytes()),
                ) {
                    headers_mut.insert(header_name, header_value);
                }
            }
        }
        return builder
            .body(Body::from(bytes))
            .unwrap_or_else(|_| proxy_error(StatusCode::BAD_GATEWAY, "无法创建上游响应"));
    }

    proxy_error(
        StatusCode::BAD_GATEWAY,
        if last_error.is_empty() {
            "没有可用的上游供应商".to_string()
        } else {
            last_error
        },
    )
}

async fn proxy_not_found() -> Response {
    proxy_error(StatusCode::NOT_FOUND, "本地路由只代理 /v1/* 请求")
}

fn router_status(runtime: &RouterRuntime, config: &RouterConfig) -> RouterStatus {
    if let Ok(handle) = runtime.handle.lock() {
        if let Some(handle) = handle.as_ref() {
            return RouterStatus {
                running: true,
                address: handle.address.clone(),
                error: None,
            };
        }
    }

    RouterStatus {
        running: false,
        address: router_address(config),
        error: if config.enabled {
            Some("本地路由未运行，点击应用后会尝试启动。".to_string())
        } else {
            None
        },
    }
}

fn stop_router(runtime: &RouterRuntime) {
    if let Ok(mut handle) = runtime.handle.lock() {
        handle.take();
    }
}

fn ensure_router(runtime: &RouterRuntime, config: &RouterConfig) -> Result<(), String> {
    if !config.enabled {
        stop_router(runtime);
        return Ok(());
    }

    let address = router_address(config);
    if let Ok(handle) = runtime.handle.lock() {
        if handle
            .as_ref()
            .is_some_and(|handle| handle.address == address)
        {
            return Ok(());
        }
    }

    stop_router(runtime);
    let socket_addr: SocketAddr = address
        .parse()
        .map_err(|err| format!("本地路由监听地址无效 {address}: {err}"))?;
    let std_listener = std::net::TcpListener::bind(socket_addr)
        .map_err(|err| format!("无法启动本地路由 {address}: {err}"))?;
    std_listener
        .set_nonblocking(true)
        .map_err(|err| format!("无法配置本地路由监听: {err}"))?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let proxy_state = Arc::new(ProxyState {
        client: reqwest::Client::new(),
    });
    let app = Router::new()
        .route("/v1/{*path}", any(proxy_request))
        .fallback(proxy_not_found)
        .with_state(proxy_state);

    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("无法创建本地路由监听: {err}");
                return;
            }
        };
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    let mut handle = runtime
        .handle
        .lock()
        .map_err(|_| "无法锁定本地路由状态".to_string())?;
    *handle = Some(RouterHandle {
        address,
        shutdown: Some(shutdown_tx),
    });
    Ok(())
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

fn scalar_number_at_path(value: &Value, path: &[&str]) -> Option<f64> {
    value_at_path(value, path).and_then(|value| match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    })
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

fn timestamp_to_route_parts(timestamp_ms: i64) -> (String, String) {
    let local = DateTime::<Utc>::from_timestamp_millis(timestamp_ms)
        .unwrap_or_else(Utc::now)
        .with_timezone(&Local);
    (
        format!(
            "{:04}-{:02}-{:02}",
            local.year(),
            local.month(),
            local.day()
        ),
        format!("{:02}:00", local.hour()),
    )
}

fn request_id(timestamp_ms: i64) -> String {
    let sequence = ROUTE_LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("req-{timestamp_ms:x}-{sequence:x}")
}

fn normalize_route_filter_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value != "all")
}

fn read_route_logs() -> Result<Vec<RouteRequestLog>, String> {
    let path = route_logs_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(&path)
        .map_err(|err| format!("无法读取路由日志 {}: {err}", path.display()))?;
    let reader = BufReader::new(file);
    let mut logs = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|err| format!("无法读取路由日志行: {err}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(log) = serde_json::from_str::<RouteRequestLog>(&line) {
            logs.push(log);
        }
    }
    logs.sort_by(|left, right| right.started_at_ms.cmp(&left.started_at_ms));
    Ok(logs)
}

fn append_route_log(log: &RouteRequestLog) -> Result<(), String> {
    fs::create_dir_all(manager_dir()?).map_err(|err| format!("无法创建管理目录: {err}"))?;
    let line = serde_json::to_string(log).map_err(|err| format!("无法序列化路由日志: {err}"))?;
    let path = route_logs_path()?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| format!("无法写入路由日志 {}: {err}", path.display()))?;
    writeln!(file, "{line}").map_err(|err| format!("无法写入路由日志: {err}"))
}

fn usage_from_route_log(log: &RouteRequestLog) -> TokenUsage {
    TokenUsage {
        input_tokens: log.input_tokens,
        cached_input_tokens: log.cached_input_tokens,
        output_tokens: log.output_tokens,
        reasoning_output_tokens: log.reasoning_output_tokens,
        total_tokens: log.total_tokens,
    }
}

fn route_log_matches_filter(log: &RouteRequestLog, filter: &RouteLogFilter) -> bool {
    if let Some(query) = normalize_route_filter_text(filter.query.clone()) {
        let query = query.to_lowercase();
        let haystack = format!(
            "{} {} {} {} {} {}",
            log.id, log.path, log.model, log.provider_name, log.status, log.route_result
        )
        .to_lowercase();
        if !haystack.contains(&query) {
            return false;
        }
    }
    if let Some(status) = normalize_route_filter_text(filter.status.clone()) {
        if !log.status.eq_ignore_ascii_case(&status) {
            return false;
        }
    }
    if let Some(provider_id) = normalize_route_filter_text(filter.provider_id.clone()) {
        if log.provider_id != provider_id {
            return false;
        }
    }
    if let Some(provider_name) = normalize_route_filter_text(filter.provider_name.clone()) {
        if !log.provider_name.eq_ignore_ascii_case(&provider_name) {
            return false;
        }
    }
    if let Some(model) = normalize_route_filter_text(filter.model.clone()) {
        if !log.model.eq_ignore_ascii_case(&model) {
            return false;
        }
    }
    if let Some(start_day) = normalize_route_filter_text(filter.start_day.clone()) {
        if log.day < start_day {
            return false;
        }
    }
    if let Some(end_day) = normalize_route_filter_text(filter.end_day.clone()) {
        if log.day > end_day {
            return false;
        }
    }
    true
}

fn add_route_log_usage(summary: &mut UsageSummary, log: &RouteRequestLog) {
    if summary.request_count == 0 {
        summary.currency = log.currency.clone();
    }
    add_usage(summary, &usage_from_route_log(log), log.estimated_cost);
}

fn is_success_route_log(log: &RouteRequestLog) -> bool {
    log.status == "success"
}

fn route_available_providers(logs: &[RouteRequestLog]) -> Vec<RouteLogFilterOption> {
    let mut map = BTreeMap::<String, RouteLogFilterOption>::new();
    for log in logs {
        let entry = map
            .entry(log.provider_id.clone())
            .or_insert_with(|| RouteLogFilterOption {
                id: log.provider_id.clone(),
                name: log.provider_name.clone(),
                request_count: 0,
            });
        entry.name = log.provider_name.clone();
        entry.request_count += 1;
    }
    let mut providers = map.into_values().collect::<Vec<_>>();
    providers.sort_by(|left, right| {
        right
            .request_count
            .cmp(&left.request_count)
            .then_with(|| left.name.cmp(&right.name))
    });
    providers
}

fn route_available_models(logs: &[RouteRequestLog]) -> Vec<String> {
    let mut models = logs
        .iter()
        .map(|log| log.model.clone())
        .filter(|model| !model.trim().is_empty() && model != "未知模型")
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}

fn route_available_days(logs: &[RouteRequestLog]) -> Vec<String> {
    let mut days = logs.iter().map(|log| log.day.clone()).collect::<Vec<_>>();
    days.sort();
    days.dedup();
    days
}

fn route_breakdown_by(
    logs: &[&RouteRequestLog],
    key_for: impl Fn(&RouteRequestLog) -> (String, String),
) -> Vec<RouteUsageBreakdown> {
    let mut map = BTreeMap::<String, RouteUsageBreakdown>::new();
    for log in logs {
        let (key, label) = key_for(log);
        let entry = map
            .entry(key.clone())
            .or_insert_with(|| RouteUsageBreakdown {
                key,
                label,
                request_count: 0,
                input_tokens: 0,
                uncached_input_tokens: 0,
                cached_input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                estimated_cost: 0.0,
            });
        entry.request_count += 1;
        entry.input_tokens += log.input_tokens;
        entry.uncached_input_tokens += log.uncached_input_tokens;
        entry.cached_input_tokens += log.cached_input_tokens;
        entry.output_tokens += log.output_tokens;
        entry.total_tokens += log.total_tokens;
        entry.estimated_cost += log.estimated_cost;
    }
    let mut rows = map.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .estimated_cost
            .partial_cmp(&left.estimated_cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.request_count.cmp(&left.request_count))
    });
    rows
}

fn route_bucket_label(log: &RouteRequestLog, filter: &RouteLogFilter) -> (String, String) {
    if filter.start_day.is_none() && filter.end_day.is_none() {
        return (
            log.day.chars().take(7).collect::<String>(),
            "month".to_string(),
        );
    }
    if let (Some(start), Some(end)) = (&filter.start_day, &filter.end_day) {
        if start == end {
            return (log.hour.clone(), "hour".to_string());
        }
    }
    (log.day.clone(), "day".to_string())
}

fn build_route_logs_response(
    logs: Vec<RouteRequestLog>,
    filter: RouteLogFilter,
) -> RouteLogsResponse {
    let filtered = logs
        .iter()
        .filter(|log| route_log_matches_filter(log, &filter))
        .cloned()
        .collect::<Vec<_>>();
    let page_size = normalized_page_size(filter.page_size);
    let total_pages = filtered.len().div_ceil(page_size).max(1);
    let page = filter.page.unwrap_or(1).clamp(1, total_pages);
    let start = (page - 1) * page_size;
    let page_logs = filtered
        .iter()
        .skip(start)
        .take(page_size)
        .cloned()
        .collect::<Vec<_>>();

    RouteLogsResponse {
        logs: page_logs,
        total: filtered.len(),
        page,
        page_size,
        total_pages,
        available_providers: route_available_providers(&logs),
        available_models: route_available_models(&logs),
        available_days: route_available_days(&logs),
    }
}

fn build_route_usage_stats(
    logs: Vec<RouteRequestLog>,
    filter: RouteLogFilter,
) -> Result<RouteUsageStats, String> {
    let filtered = logs
        .iter()
        .filter(|log| route_log_matches_filter(log, &filter))
        .collect::<Vec<_>>();
    let successful = filtered
        .iter()
        .copied()
        .filter(|log| is_success_route_log(log))
        .collect::<Vec<_>>();
    let now = Local::now();
    let today_key = format!("{:04}-{:02}-{:02}", now.year(), now.month(), now.day());
    let mut summary = UsageSummary::default();
    let mut today = UsageSummary::default();
    let mut failed_count = 0;
    let mut success_count = 0;
    let mut running_count = 0;
    let mut first_byte_total = 0_u64;
    let mut first_byte_count = 0_u64;
    let mut total_ms_total = 0_u64;
    let mut total_ms_count = 0_u64;
    let mut bucket_granularity = String::new();
    let mut bucket_map = BTreeMap::<String, RouteUsageBucket>::new();

    for log in &filtered {
        if log.status == "failed" {
            failed_count += 1;
        } else if log.status == "success" {
            success_count += 1;
        } else if log.status == "running" {
            running_count += 1;
        }
        if let Some(first_byte_ms) = log.first_byte_ms {
            first_byte_total = first_byte_total.saturating_add(first_byte_ms);
            first_byte_count += 1;
        }
        total_ms_total = total_ms_total.saturating_add(log.total_ms);
        total_ms_count += 1;
        if !is_success_route_log(log) {
            continue;
        }
        add_route_log_usage(&mut summary, log);
        if log.day == today_key {
            add_route_log_usage(&mut today, log);
        }
        let (bucket_label, granularity) = route_bucket_label(log, &filter);
        if bucket_granularity.is_empty() {
            bucket_granularity = granularity;
        }
        let entry = bucket_map
            .entry(bucket_label.clone())
            .or_insert_with(|| RouteUsageBucket {
                label: bucket_label,
                request_count: 0,
                input_tokens: 0,
                uncached_input_tokens: 0,
                cached_input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                estimated_cost: 0.0,
            });
        entry.request_count += 1;
        entry.input_tokens += log.input_tokens;
        entry.uncached_input_tokens += log.uncached_input_tokens;
        entry.cached_input_tokens += log.cached_input_tokens;
        entry.output_tokens += log.output_tokens;
        entry.total_tokens += log.total_tokens;
        entry.estimated_cost += log.estimated_cost;
    }

    let page_size = normalized_page_size(filter.page_size);
    let total_pages = filtered.len().div_ceil(page_size).max(1);
    let page = filter.page.unwrap_or(1).clamp(1, total_pages);
    let start = (page - 1) * page_size;
    let details = filtered
        .iter()
        .skip(start)
        .take(page_size)
        .map(|log| (*log).clone())
        .collect::<Vec<_>>();
    let bucket_granularity = if bucket_granularity.is_empty() {
        if filter.start_day.is_none() && filter.end_day.is_none() {
            "month".to_string()
        } else if filter.start_day == filter.end_day {
            "hour".to_string()
        } else {
            "day".to_string()
        }
    } else {
        bucket_granularity
    };

    Ok(RouteUsageStats {
        generated_at_ms: current_epoch_ms()?,
        filters: filter,
        summary,
        today,
        failed_count,
        success_count,
        running_count,
        average_first_byte_ms: if first_byte_count > 0 {
            Some(first_byte_total / first_byte_count)
        } else {
            None
        },
        average_total_ms: if total_ms_count > 0 {
            Some(total_ms_total / total_ms_count)
        } else {
            None
        },
        bucket_granularity,
        buckets: bucket_map.into_values().collect(),
        providers: route_breakdown_by(&successful, |log| {
            (log.provider_id.clone(), log.provider_name.clone())
        }),
        models: route_breakdown_by(&successful, |log| {
            let model = if log.model.trim().is_empty() {
                "未知模型".to_string()
            } else {
                log.model.clone()
            };
            (model.clone(), model)
        }),
        details,
        total: filtered.len(),
        page,
        page_size,
        total_pages,
        available_providers: route_available_providers(&logs),
        available_models: route_available_models(&logs),
        available_days: route_available_days(&logs),
    })
}

fn parse_event_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn usage_from_value(value: &Value) -> TokenUsage {
    let input_tokens = scalar_number_at_path(value, &["input_tokens"])
        .or_else(|| scalar_number_at_path(value, &["prompt_tokens"]))
        .unwrap_or_default() as i64;
    let cached_input_tokens = scalar_number_at_path(value, &["cached_input_tokens"])
        .or_else(|| scalar_number_at_path(value, &["input_tokens_details", "cached_tokens"]))
        .or_else(|| scalar_number_at_path(value, &["prompt_tokens_details", "cached_tokens"]))
        .unwrap_or_default() as i64;
    let output_tokens = scalar_number_at_path(value, &["output_tokens"])
        .or_else(|| scalar_number_at_path(value, &["completion_tokens"]))
        .unwrap_or_default() as i64;
    let reasoning_output_tokens = scalar_number_at_path(value, &["reasoning_output_tokens"])
        .or_else(|| scalar_number_at_path(value, &["output_tokens_details", "reasoning_tokens"]))
        .or_else(|| {
            scalar_number_at_path(value, &["completion_tokens_details", "reasoning_tokens"])
        })
        .unwrap_or_default() as i64;
    let total_tokens = scalar_number_at_path(value, &["total_tokens"])
        .map(|value| value as i64)
        .unwrap_or_else(|| input_tokens.saturating_add(output_tokens));

    TokenUsage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_output_tokens,
        total_tokens,
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

fn build_app_state(state: ManagerState, runtime: &RouterRuntime) -> Result<AppState, String> {
    let provider = active_provider(&state);
    let redacted_active_provider = provider.clone().map(redacted_provider);
    let desired = if state.router.enabled {
        router_patch_desired(&state.router)
    } else {
        provider
            .as_ref()
            .map(|provider| provider.config.clone())
            .unwrap_or_else(|| json!({}))
    };
    let (doc, marker_present, current_config_raw, current_config_exists) = read_current_toml()?;
    let current_json = toml_doc_to_json(&doc);
    let final_preview_toml = if state.router.enabled {
        render_router_patch_toml(doc, marker_present, &state.router)?
    } else {
        current_config_raw.clone()
    };
    let diffs = compute_diffs(&state, provider.as_ref(), &current_json, &desired);
    let redacted_diffs = diffs.iter().cloned().map(redacted_diff).collect::<Vec<_>>();
    let desired_flat = flatten(&desired);

    let summary = desired_flat
        .iter()
        .take(10)
        .map(|(path, value)| ConfigRow {
            path: path.clone(),
            value: redacted_path_value(path, value),
            source: source_for_path(&state, provider.as_ref(), path),
            changed: diffs.iter().any(|diff| diff.path == *path),
        })
        .collect::<Vec<_>>();

    let providers = state
        .providers
        .iter()
        .enumerate()
        .map(|(index, provider)| {
            let provider_desired = if state.router.enabled {
                desired.clone()
            } else {
                desired_config(&state, Some(provider))
            };
            let pending_changes =
                compute_diffs(&state, Some(provider), &current_json, &provider_desired).len();
            let balance_status = provider.balance_status.as_ref();
            ProviderSummary {
                id: provider.id.clone(),
                name: provider.name.clone(),
                enabled: provider.enabled,
                pending_changes,
                base_url: custom_provider_base_url(provider).unwrap_or_default(),
                provider_type: provider_type(provider),
                route_order: index + 1,
                balance_label: balance_status
                    .map(|status| status.label.clone())
                    .unwrap_or_else(|| "未配置".to_string()),
                balance_error: balance_status.and_then(|status| status.error.clone()),
                latency_label: balance_status
                    .map(|status| {
                        if status.error.is_some() {
                            "超时".to_string()
                        } else {
                            "正常".to_string()
                        }
                    })
                    .unwrap_or_else(|| "未检查".to_string()),
                last_checked_label: balance_status
                    .map(|status| elapsed_label(status.checked_at))
                    .unwrap_or_else(|| "未检查".to_string()),
            }
        })
        .collect();

    Ok(AppState {
        codex_config_path: codex_config_path()?.display().to_string(),
        manager_dir: manager_dir()?.display().to_string(),
        current_config_raw: redacted_toml_text(&current_config_raw),
        current_config_exists,
        active_provider_id: state.active_provider_id,
        base_template_name: state.base_template_name,
        base_toml: redacted_toml_text(&json_to_toml_text(&state.base)?),
        base: redacted_config_value(state.base),
        providers,
        active_provider_toml: redacted_active_provider
            .as_ref()
            .map(|provider| json_to_toml_text(&provider.config))
            .transpose()?
            .unwrap_or_default(),
        active_provider: redacted_active_provider,
        desired: redacted_config_value(desired.clone()),
        final_preview_toml: redacted_toml_text(&final_preview_toml),
        summary,
        diffs: redacted_diffs,
        marker_present,
        router: state.router.clone(),
        router_status: router_status(runtime, &state.router),
    })
}

#[tauri::command]
fn load_app_state(router_runtime: tauri::State<RouterRuntime>) -> Result<AppState, String> {
    build_app_state(load_state_file()?, &router_runtime)
}

#[tauri::command]
fn select_provider(
    provider_id: String,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
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
    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn get_provider(provider_id: String) -> Result<ProviderConfig, String> {
    load_state_file()?
        .providers
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .map(redacted_provider)
        .ok_or_else(|| "供应商不存在".to_string())
}

#[tauri::command]
fn save_provider(
    payload: SaveProviderPayload,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
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

    if let Some(enabled) = payload.enabled {
        provider.enabled = enabled;
    }
    if payload.base_url.is_some() || payload.api_key.is_some() {
        apply_provider_connection_draft(
            provider,
            payload.base_url.as_deref(),
            payload.api_key.as_deref(),
        )?;
    } else if !payload.config_toml.trim().is_empty() {
        provider.config = toml_text_to_json(&payload.config_toml)?;
    }
    if let Some(balance_query) = payload.balance_query {
        let previous_balance_query = provider.balance_query.clone();
        let next_balance_query = merge_balance_config_draft(provider, balance_query);
        let balance_changed = next_balance_query != previous_balance_query;
        provider.balance_query = next_balance_query;
        provider.balance_status = payload.balance_status.or_else(|| {
            (!balance_changed)
                .then(|| provider.balance_status.clone())
                .flatten()
        });
    }
    save_state(&state)?;
    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn preview_provider(
    payload: SaveProviderPayload,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
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
        provider.balance_query = merge_balance_config_draft(provider, balance_query);
        provider.balance_status = None;
    }
    state.active_provider_id = active_provider_id;

    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn add_provider(
    name: String,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
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
        enabled: true,
        balance_query: BalanceQueryConfig::default(),
        balance_status: None,
        config: toml_text_to_json(
            "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"\"\nexperimental_bearer_token = \"\"\n",
        )?,
    });

    save_state(&state)?;
    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn save_base_template(
    payload: SaveBasePayload,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    state.base_template_name = payload.base_template_name;
    state.base = toml_text_to_json(&payload.base_toml)?;
    save_state(&state)?;
    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn save_router_config(
    payload: SaveRouterPayload,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    let host = payload.host.trim();
    if payload.enabled && host.is_empty() {
        return Err("本地路由监听地址不能为空".to_string());
    }
    if payload.enabled && payload.port == 0 {
        return Err("本地路由端口无效".to_string());
    }
    let local_token = payload.local_token.trim();
    if payload.enabled && local_token.is_empty() {
        return Err("本地路由 Token 不能为空".to_string());
    }

    state.router = RouterConfig {
        enabled: payload.enabled,
        host: if host.is_empty() {
            default_router_host()
        } else {
            host.to_string()
        },
        port: payload.port,
        local_token: if local_token.is_empty() {
            default_router_token()
        } else {
            local_token.to_string()
        },
    };
    save_state(&state)?;
    ensure_router(&router_runtime, &state.router)?;
    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn apply_config(router_runtime: tauri::State<RouterRuntime>) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    let config_path = codex_config_path()?;

    fs::create_dir_all(codex_home()?).map_err(|err| format!("无法创建 Codex 目录: {err}"))?;

    if state.router.enabled {
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
        if state.router_backup.is_none() {
            state.router_backup = Some(capture_router_backup(&doc));
        }
        let raw = render_router_patch_toml(doc, marker_present, &state.router)?;

        fs::write(&config_path, raw).map_err(|err| format!("无法写入 Codex 配置: {err}"))?;
        state.last_applied = Some(router_patch_desired(&state.router));
    } else if config_path.exists() {
        let backup_name = format!(
            "config.toml.{}.bak",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|err| format!("系统时间异常: {err}"))?
                .as_secs()
        );
        fs::copy(&config_path, manager_dir()?.join(backup_name))
            .map_err(|err| format!("无法备份现有配置: {err}"))?;
        let (doc, _, _, _) = read_current_toml()?;
        let raw = restore_router_backup(doc, state.router_backup.as_ref(), &state.router)?;
        fs::write(&config_path, raw).map_err(|err| format!("无法写入 Codex 配置: {err}"))?;
        state.last_applied = None;
        state.router_backup = None;
    }
    state.applied_provider_id = None;
    save_state(&state)?;
    ensure_router(&router_runtime, &state.router)?;
    build_app_state(state, &router_runtime)
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
fn load_route_logs(payload: Option<LoadRouteLogsPayload>) -> Result<RouteLogsResponse, String> {
    let filter = payload
        .and_then(|payload| payload.filter)
        .unwrap_or_default();
    Ok(build_route_logs_response(read_route_logs()?, filter))
}

#[tauri::command]
fn load_route_usage_stats(
    payload: Option<LoadRouteLogsPayload>,
) -> Result<RouteUsageStats, String> {
    let filter = payload
        .and_then(|payload| payload.filter)
        .unwrap_or_default();
    build_route_usage_stats(read_route_logs()?, filter)
}

#[tauri::command]
async fn query_provider_balance(
    payload: QueryBalancePayload,
    router_runtime: tauri::State<'_, RouterRuntime>,
) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    let provider = state
        .providers
        .iter()
        .find(|provider| provider.id == payload.provider_id)
        .cloned()
        .ok_or_else(|| "供应商不存在".to_string())?;
    let mut test_provider = provider.clone();
    apply_provider_connection_draft(
        &mut test_provider,
        payload.base_url.as_deref(),
        payload.api_key.as_deref(),
    )?;
    if let Some(balance_query) = payload.balance_query {
        test_provider.balance_query = merge_balance_config_draft(&test_provider, balance_query);
    }

    let status = fetch_balance(&test_provider).await;
    if let Some(provider) = state
        .providers
        .iter_mut()
        .find(|provider| provider.id == payload.provider_id)
    {
        provider.balance_status = Some(status);
    }

    save_state(&state)?;
    build_app_state(state, &router_runtime)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(UsageCacheState::default())
        .manage(RouterRuntime::default())
        .setup(|app| {
            let state = load_state_file()?;
            let router_runtime = app.state::<RouterRuntime>();
            if let Err(err) = ensure_router(&router_runtime, &state.router) {
                eprintln!("本地路由启动失败: {err}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_app_state,
            select_provider,
            get_provider,
            save_provider,
            preview_provider,
            add_provider,
            save_base_template,
            save_router_config,
            apply_config,
            load_usage_stats,
            load_route_logs,
            load_route_usage_stats,
            query_provider_balance
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_responses_usage_from_crlf_sse_chunks() {
        let mut buffer = String::new();
        let mut usage = TokenUsage::default();
        let first = b"event: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\r\n\r\n";
        ingest_sse_chunk(&mut buffer, &mut usage, first);
        assert!(usage_is_zero(&usage));

        let usage_event = b"event: response.completed\r\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":123,\"input_tokens_details\":{\"cached_tokens\":45},\"output_tokens\":67,\"output_tokens_details\":{\"reasoning_tokens\":8},\"total_tokens\":190}}}\r\n\r\n";
        let split = 80;
        ingest_sse_chunk(&mut buffer, &mut usage, &usage_event[..split]);
        assert!(usage_is_zero(&usage));
        ingest_sse_chunk(&mut buffer, &mut usage, &usage_event[split..]);

        assert_eq!(usage.input_tokens, 123);
        assert_eq!(usage.cached_input_tokens, 45);
        assert_eq!(usage.output_tokens, 67);
        assert_eq!(usage.reasoning_output_tokens, 8);
        assert_eq!(usage.total_tokens, 190);
        assert!(buffer.is_empty());
    }

    #[test]
    fn parses_chat_completion_usage_aliases() {
        let value = json!({
            "usage": {
                "prompt_tokens": 30,
                "prompt_tokens_details": { "cached_tokens": 12 },
                "completion_tokens": 20,
                "completion_tokens_details": { "reasoning_tokens": 5 },
                "total_tokens": 50
            }
        });

        let usage = usage_from_response_value(&value);
        assert_eq!(usage.input_tokens, 30);
        assert_eq!(usage.cached_input_tokens, 12);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.reasoning_output_tokens, 5);
        assert_eq!(usage.total_tokens, 50);
    }

    fn route_log_for_stats_test(status: &str, provider_id: &str, cost: f64) -> RouteRequestLog {
        RouteRequestLog {
            id: format!("test-{status}-{provider_id}"),
            started_at_ms: 1_782_470_400_000,
            day: "2026-06-27".to_string(),
            hour: "10:00".to_string(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            model: "test-model".to_string(),
            provider_id: provider_id.to_string(),
            provider_name: provider_id.to_string(),
            provider_order: 1,
            upstream_chain: vec![provider_id.to_string()],
            status: status.to_string(),
            status_code: if status == "success" {
                Some(200)
            } else {
                Some(500)
            },
            error: if status == "success" {
                None
            } else {
                Some("upstream failed".to_string())
            },
            route_result: status.to_string(),
            route_attempts: 1,
            input_tokens: 7,
            uncached_input_tokens: 5,
            cached_input_tokens: 2,
            output_tokens: 3,
            reasoning_output_tokens: 1,
            total_tokens: 10,
            estimated_cost: cost,
            currency: "USD".to_string(),
            cost_breakdown: String::new(),
            pricing_model_match: "test-model".to_string(),
            pricing_source: "test".to_string(),
            first_byte_ms: Some(100),
            total_ms: 200,
        }
    }

    #[test]
    fn route_usage_counts_and_cost_only_successful_requests() {
        let stats = build_route_usage_stats(
            vec![
                route_log_for_stats_test("success", "provider-a", 0.000123),
                route_log_for_stats_test("failed", "provider-b", 9.0),
            ],
            RouteLogFilter {
                start_day: Some("2026-06-27".to_string()),
                end_day: Some("2026-06-27".to_string()),
                page_size: Some(50),
                ..Default::default()
            },
        )
        .expect("route usage stats should build");

        assert_eq!(stats.total, 2);
        assert_eq!(stats.details.len(), 2);
        assert_eq!(stats.success_count, 1);
        assert_eq!(stats.failed_count, 1);
        assert_eq!(stats.summary.request_count, 1);
        assert_eq!(stats.summary.total_tokens, 10);
        assert!((stats.summary.estimated_cost - 0.000123).abs() < f64::EPSILON);
        assert_eq!(stats.buckets.len(), 1);
        assert_eq!(stats.buckets[0].request_count, 1);
        assert_eq!(stats.providers.len(), 1);
        assert_eq!(stats.providers[0].key, "provider-a");
        assert_eq!(stats.models[0].request_count, 1);
    }

    #[test]
    fn empty_balance_token_draft_preserves_saved_token() {
        let provider = ProviderConfig {
            id: "provider-a".to_string(),
            name: "Provider A".to_string(),
            enabled: true,
            config: json!({
                "model_provider": "custom",
                "model_providers": {
                    "custom": {
                        "base_url": "https://example.com",
                        "experimental_bearer_token": "provider-token"
                    }
                }
            }),
            balance_query: BalanceQueryConfig {
                enabled: true,
                query_type: BalanceQueryType::NewApi,
                new_api_target: NewApiBalanceTarget::TokenQuota,
                endpoint: "https://example.com".to_string(),
                path: "/api/usage/token/".to_string(),
                auth_mode: BalanceAuthMode::SeparateToken,
                query_token: "saved-balance-token".to_string(),
                new_api_user_id: String::new(),
            },
            balance_status: None,
        };
        let mut draft = provider.balance_query.clone();
        draft.query_token.clear();

        let merged = merge_balance_config_draft(&provider, draft);

        assert_eq!(merged.query_token, "saved-balance-token");
    }
}
