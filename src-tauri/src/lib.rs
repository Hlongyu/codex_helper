use axum::{
    body::{Body, Bytes},
    extract::{Path as AxumPath, State as AxumState},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri},
    response::Response,
    routing::any,
    Router,
};
use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use futures_util::{stream::BoxStream, StreamExt};
use reqwest::header::{
    ACCEPT, AUTHORIZATION, CONNECTION, CONTENT_ENCODING, CONTENT_LENGTH, HOST, TRANSFER_ENCODING,
    UPGRADE,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tokio::sync::oneshot;
use toml_edit::{Array, DocumentMut, Item, Value as TomlValue};

const MARKER: &str = "# managed-by: xxswitch";
const LEGACY_MARKER: &str = "# managed-by: codex-config-manager";
const DEFAULT_ROUTER_HOST: &str = "127.0.0.1";
const DEFAULT_ROUTER_PORT: u16 = 18080;
const DEFAULT_ROUTER_TOKEN: &str = "xxswitch-local-token";
const LEGACY_DEFAULT_ROUTER_TOKEN: &str = "codex-helper-local-token";
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_RESPONSE_HEADER_TIMEOUT_SECS: u64 = 30;
const DEFAULT_STREAM_IDLE_TIMEOUT_SECS: u64 = 120;
const MAX_CONNECT_TIMEOUT_SECS: u64 = 120;
const MAX_RESPONSE_HEADER_TIMEOUT_SECS: u64 = 600;
const MAX_STREAM_IDLE_TIMEOUT_SECS: u64 = 3_600;
const MAX_PROXY_BODY_BYTES: usize = 32 * 1024 * 1024;
const CODEX_MODEL_CONTEXT_WINDOW: i64 = 256_000;
const CODEX_MODEL_AUTO_COMPACT_TOKEN_LIMIT: i64 = 243_200;
const CODEX_MODEL_EFFECTIVE_CONTEXT_WINDOW_PERCENT: i64 = 95;
const PI_PROVIDER_ID: &str = "xxswitch";
const LEGACY_PI_PROVIDER_ID: &str = "codex-helper";
const PI_PROVIDER_API: &str = "openai-responses";
static ROUTE_LOG_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static PROVIDER_HEALTH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const AUTO_DISABLE_FAILURE_THRESHOLD: u32 = 3;
const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_SHOW_ID: &str = "show";
const TRAY_QUIT_ID: &str = "quit";
const GITHUB_RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/Hlongyu/codex_helper/releases/latest";
const GITHUB_RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/Hlongyu/codex_helper/releases/download/";
#[cfg(target_os = "macos")]
const MACOS_BUNDLE_IDENTIFIER: &str = "com.local.xxswitch";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderConfig {
    id: String,
    name: String,
    #[serde(default)]
    status: ProviderStatus,
    enabled: bool,
    #[serde(default)]
    consecutive_failure_count: u32,
    #[serde(default)]
    auto_disabled_day: Option<String>,
    #[serde(default)]
    last_failure_reason: Option<String>,
    #[serde(default)]
    last_failure_at_ms: Option<i64>,
    config: Value,
    #[serde(default)]
    wire_api: ProviderWireApi,
    #[serde(default)]
    service_tier: String,
    #[serde(default)]
    connection_test_model: String,
    #[serde(default)]
    allowed_models: Vec<String>,
    #[serde(default)]
    model_mappings: Vec<ModelMapping>,
    #[serde(default)]
    balance_query: BalanceQueryConfig,
    #[serde(default)]
    balance_status: Option<BalanceStatus>,
    #[serde(default)]
    connection_status: Option<ConnectionStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClaudeProviderConfig {
    id: String,
    name: String,
    #[serde(default)]
    status: ProviderStatus,
    enabled: bool,
    #[serde(default)]
    consecutive_failure_count: u32,
    #[serde(default)]
    auto_disabled_day: Option<String>,
    #[serde(default)]
    last_failure_reason: Option<String>,
    #[serde(default)]
    last_failure_at_ms: Option<i64>,
    base_url: String,
    api_key: String,
    #[serde(default)]
    connection_test_model: String,
    #[serde(default)]
    allowed_models: Vec<String>,
    #[serde(default)]
    model_mappings: Vec<ModelMapping>,
    #[serde(default)]
    connection_status: Option<ConnectionStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderStatus {
    Enabled,
    Disabled,
    AutoDisabled,
}

impl Default for ProviderStatus {
    fn default() -> Self {
        Self::Enabled
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ModelMapping {
    source: String,
    target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderWireApi {
    Responses,
    ChatCompletions,
}

impl Default for ProviderWireApi {
    fn default() -> Self {
        Self::Responses
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConnectionStatus {
    ok: bool,
    latency_ms: Option<u64>,
    checked_at: Option<u64>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RouterConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    remote_compaction_enabled: bool,
    #[serde(default = "default_router_host")]
    host: String,
    #[serde(default = "default_router_port")]
    port: u16,
    #[serde(default = "default_router_token")]
    local_token: String,
    #[serde(default = "default_connect_timeout_secs")]
    connect_timeout_secs: u64,
    #[serde(default = "default_response_header_timeout_secs")]
    response_header_timeout_secs: u64,
    #[serde(default = "default_stream_idle_timeout_secs")]
    stream_idle_timeout_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AgentClientKind {
    Codex,
    Claude,
    Pi,
}

impl AgentClientKind {
    fn all() -> [Self; 3] {
        [Self::Codex, Self::Claude, Self::Pi]
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Pi => "Pi",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillLocationConfig {
    path: String,
    #[serde(default = "default_true")]
    writable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillOrigin {
    client: AgentClientKind,
    skill_location: String,
    original_path: String,
    original_dir_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SharedSkillConfig {
    identity: String,
    library_dir_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    sharing_scope: Vec<AgentClientKind>,
    #[serde(default)]
    origin: Option<SkillOrigin>,
    #[serde(default)]
    created_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedSkillExposureConfig {
    skill_identity: String,
    client: AgentClientKind,
    path: String,
    #[serde(default)]
    skill_location: String,
    #[serde(default)]
    created_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SkillManagementConfig {
    #[serde(default)]
    shared_skills: Vec<SharedSkillConfig>,
    #[serde(default)]
    exposures: Vec<ManagedSkillExposureConfig>,
}

#[derive(Debug, Clone, Serialize)]
struct SkillLocationView {
    path: String,
    writable: bool,
    managed: bool,
    exists: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SkillClientView {
    client: AgentClientKind,
    label: String,
    managed_skill_location: String,
    skill_locations: Vec<SkillLocationView>,
}

#[derive(Debug, Clone, Serialize)]
struct ClientSkillView {
    client: AgentClientKind,
    client_label: String,
    skill_location: String,
    path: String,
    dir_name: String,
    identity: String,
    description: String,
    managed: bool,
    shared: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SkillExposureView {
    client: AgentClientKind,
    client_label: String,
    path: String,
    health: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct SharedSkillView {
    identity: String,
    description: String,
    library_dir_name: String,
    path: String,
    sharing_scope: Vec<AgentClientKind>,
    origin: Option<SkillOrigin>,
    exposures: Vec<SkillExposureView>,
}

#[derive(Debug, Clone, Serialize)]
struct SkillConflictView {
    kind: String,
    identity: String,
    client: Option<AgentClientKind>,
    path: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct SkillManagementView {
    library_root: String,
    clients: Vec<SkillClientView>,
    shared_skills: Vec<SharedSkillView>,
    client_skills: Vec<ClientSkillView>,
    conflicts: Vec<SkillConflictView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ClientTargetConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    skill_locations: Vec<SkillLocationConfig>,
    #[serde(default)]
    managed_skill_location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ClientConfigs {
    #[serde(default)]
    codex: ClientTargetConfig,
    #[serde(default)]
    claude: ClientTargetConfig,
    #[serde(default)]
    pi: ClientTargetConfig,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            remote_compaction_enabled: false,
            host: default_router_host(),
            port: default_router_port(),
            local_token: default_router_token(),
            connect_timeout_secs: default_connect_timeout_secs(),
            response_header_timeout_secs: default_response_header_timeout_secs(),
            stream_idle_timeout_secs: default_stream_idle_timeout_secs(),
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
    #[serde(default)]
    active_claude_provider_id: String,
    #[serde(default)]
    claude_providers: Vec<ClaudeProviderConfig>,
    last_applied: Option<Value>,
    #[serde(default)]
    applied_history: Vec<AppliedProviderSnapshot>,
    #[serde(default)]
    router: RouterConfig,
    #[serde(default)]
    clients: ClientConfigs,
    #[serde(default)]
    router_backup: Option<RouterApplyBackup>,
    #[serde(default)]
    claude_backup: Option<ClaudeApplyBackup>,
    #[serde(default)]
    pi_backup: Option<PiApplyBackup>,
    #[serde(default)]
    skills: SkillManagementConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RouterApplyBackup {
    #[serde(default)]
    model_provider: RouterFieldBackup,
    #[serde(default)]
    custom_name: RouterFieldBackup,
    #[serde(default)]
    remote_compaction_v2: RouterFieldBackup,
    #[serde(default)]
    custom_base_url: RouterFieldBackup,
    #[serde(default)]
    custom_token: RouterFieldBackup,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ClaudeApplyBackup {
    #[serde(default)]
    base_url: RouterFieldBackup,
    #[serde(default)]
    auth_token: RouterFieldBackup,
    #[serde(default)]
    api_key: RouterFieldBackup,
    #[serde(default)]
    gateway_model_discovery: RouterFieldBackup,
    #[serde(default)]
    default_fable_model: RouterFieldBackup,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PiApplyBackup {
    #[serde(default)]
    provider: RouterFieldBackup,
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

#[derive(Debug, Clone, Serialize, Default)]
struct RouteUsageSummary {
    request_count: usize,
    input_tokens: i64,
    uncached_input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
    total_tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
struct ProviderSummary {
    id: String,
    name: String,
    status: ProviderStatus,
    enabled: bool,
    consecutive_failure_count: u32,
    auto_disabled_day: Option<String>,
    last_failure_reason: Option<String>,
    last_failure_at_ms: Option<i64>,
    pending_changes: usize,
    base_url: String,
    provider_type: String,
    route_order: usize,
    balance_label: String,
    balance_error: Option<String>,
    latency_ms: Option<u64>,
    latency_label: String,
    latency_error: Option<String>,
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
    app_version: String,
    codex_config_path: String,
    claude_settings_path: String,
    pi_models_path: String,
    manager_dir: String,
    current_config_raw: String,
    current_config_exists: bool,
    active_provider_id: String,
    active_claude_provider_id: String,
    base_template_name: String,
    base_toml: String,
    base: Value,
    providers: Vec<ProviderSummary>,
    claude_providers: Vec<ProviderSummary>,
    active_provider: Option<ProviderConfig>,
    active_claude_provider: Option<ClaudeProviderConfig>,
    active_provider_toml: String,
    desired: Value,
    final_preview_toml: String,
    summary: Vec<ConfigRow>,
    diffs: Vec<DiffEntry>,
    marker_present: bool,
    router: RouterConfig,
    clients: ClientConfigs,
    router_status: RouterStatus,
}

#[derive(Debug, Clone, Serialize)]
struct UpdateCheckInfo {
    current_version: String,
    latest_version: String,
    available: bool,
    installable: bool,
    asset_name: Option<String>,
    release_url: String,
}

#[derive(Debug, Clone, Serialize)]
struct UpdateInstallResult {
    message: String,
    manual_install: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdatePlatform {
    Windows,
    Macos,
    Unsupported,
}

#[derive(Debug, Deserialize)]
struct SaveProviderPayload {
    provider_id: String,
    provider_name: Option<String>,
    config_toml: String,
    balance_query: Option<BalanceQueryConfig>,
    balance_status: Option<BalanceStatus>,
    connection_test_model: Option<String>,
    allowed_models: Option<Vec<String>>,
    model_mappings: Option<Vec<ModelMapping>>,
    wire_api: Option<ProviderWireApi>,
    service_tier: Option<String>,
    enabled: Option<bool>,
    base_url: Option<String>,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SaveClaudeProviderPayload {
    provider_id: String,
    provider_name: Option<String>,
    enabled: Option<bool>,
    base_url: Option<String>,
    api_key: Option<String>,
    connection_test_model: Option<String>,
    allowed_models: Option<Vec<String>>,
    model_mappings: Option<Vec<ModelMapping>>,
}

#[derive(Debug, Deserialize)]
struct ReorderProvidersPayload {
    provider_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DeleteProviderPayload {
    provider_id: String,
}

#[derive(Debug, Deserialize)]
struct SaveClientConfigsPayload {
    codex_enabled: bool,
    claude_enabled: bool,
    pi_enabled: bool,
}

#[derive(Debug, Deserialize)]
struct SaveSkillClientConfigPayload {
    client: AgentClientKind,
    skill_locations: Vec<SkillLocationConfig>,
    managed_skill_location: String,
}

#[derive(Debug, Deserialize)]
struct PromoteClientSkillPayload {
    client: AgentClientKind,
    skill_path: String,
    sharing_scope: Vec<AgentClientKind>,
}

#[derive(Debug, Deserialize)]
struct ReplaceClientSkillWithSharedPayload {
    client: AgentClientKind,
    skill_path: String,
}

#[derive(Debug, Deserialize)]
struct SetSkillSharingScopePayload {
    skill_identity: String,
    sharing_scope: Vec<AgentClientKind>,
}

#[derive(Debug, Deserialize)]
struct SkillIdentityPayload {
    skill_identity: String,
}

#[derive(Debug, Deserialize)]
struct SaveBasePayload {
    base_template_name: String,
    base_toml: String,
}

#[derive(Debug, Deserialize)]
struct SaveRouterPayload {
    enabled: bool,
    #[serde(default)]
    remote_compaction_enabled: bool,
    host: String,
    port: u16,
    local_token: String,
    #[serde(default = "default_connect_timeout_secs")]
    connect_timeout_secs: u64,
    #[serde(default = "default_response_header_timeout_secs")]
    response_header_timeout_secs: u64,
    #[serde(default = "default_stream_idle_timeout_secs")]
    stream_idle_timeout_secs: u64,
}

#[derive(Debug, Deserialize)]
struct QueryBalancePayload {
    provider_id: String,
    balance_query: Option<BalanceQueryConfig>,
    base_url: Option<String>,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TestProviderConnectionPayload {
    provider_id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    test_model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TestProviderLatencyPayload {
    provider_id: String,
    provider_kind: AgentClientKind,
    model: String,
    prompt: String,
    #[serde(default)]
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct LoadProviderModelsPayload {
    provider_id: String,
    base_url: Option<String>,
    api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ProviderModelsResponse {
    models: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ProviderLatencyTestResponse {
    app_state: AppState,
    ok: bool,
    latency_ms: Option<u64>,
    error: Option<String>,
    reply: String,
}

#[derive(Debug, Clone, Serialize)]
struct ProviderConnectionTestStep {
    key: String,
    label: String,
    status: String,
    latency_ms: Option<u64>,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct ProviderConnectionTestResult {
    ok: bool,
    steps: Vec<ProviderConnectionTestStep>,
}

#[derive(Default)]
struct RouterRuntime {
    handle: Mutex<Option<RouterHandle>>,
}

struct RouterHandle {
    address: String,
    timeouts: RouterTimeouts,
    proxy_state: Arc<ProxyState>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Drop for RouterHandle {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

struct ProxyState {
    upstream: RwLock<UpstreamClient>,
}

#[derive(Clone)]
struct UpstreamClient {
    client: reqwest::Client,
    response_header_timeout_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RouterTimeouts {
    connect_secs: u64,
    response_header_secs: u64,
    stream_idle_secs: u64,
}

impl RouterTimeouts {
    fn from_config(config: &RouterConfig) -> Self {
        Self {
            connect_secs: config.connect_timeout_secs,
            response_header_secs: config.response_header_timeout_secs,
            stream_idle_secs: config.stream_idle_timeout_secs,
        }
    }
}

impl ProxyState {
    fn upstream(&self) -> Result<UpstreamClient, String> {
        self.upstream
            .read()
            .map(|upstream| upstream.clone())
            .map_err(|_| "无法读取上游 HTTP 客户端".to_string())
    }

    fn replace_upstream(&self, upstream: UpstreamClient) -> Result<(), String> {
        let mut current = self
            .upstream
            .write()
            .map_err(|_| "无法更新上游 HTTP 客户端".to_string())?;
        *current = upstream;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseAdapter {
    Passthrough,
    ChatCompletionsToResponses,
}

#[derive(Debug, Clone)]
struct PreparedUpstreamRequest {
    path: String,
    query: String,
    body: Bytes,
    adapter: ResponseAdapter,
    upstream_model: Option<String>,
    tool_context: CodexToolContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CodexToolKind {
    Function,
    Namespace,
    Custom,
    ToolSearch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexToolSpec {
    kind: CodexToolKind,
    name: String,
    namespace: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct CodexToolContext {
    chat_tools: Vec<Value>,
    seen_chat_names: BTreeSet<String>,
    chat_name_to_spec: BTreeMap<String, CodexToolSpec>,
    namespace_name_to_chat_name: BTreeMap<(String, String), String>,
}

#[derive(Debug, Clone, Default)]
struct ChatToolCallState {
    output_index: Option<usize>,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
    added: bool,
    done: bool,
}

const REASONING_CONTENT_FIELD: &str = "reasoning_content";
const LOCAL_REASONING_ENCRYPTED_PREFIX: &str = "xxswitch-local-reasoning-v1:";
const LEGACY_LOCAL_REASONING_ENCRYPTED_PREFIX: &str = "codex-helper-local-reasoning-v1:";
const MISSING_REASONING_CONTENT_FALLBACK: &str =
    "Previous reasoning content was unavailable in Responses history.";

fn reasoning_content_from_value(value: &Value) -> Option<&str> {
    value
        .get(REASONING_CONTENT_FIELD)
        .and_then(Value::as_str)
        .filter(|content| !content.is_empty())
}

fn attach_reasoning_content(value: &mut Value, reasoning_content: Option<&str>) {
    let Some(reasoning_content) = reasoning_content.filter(|content| !content.is_empty()) else {
        return;
    };
    if let Some(object) = value.as_object_mut() {
        object.insert(
            REASONING_CONTENT_FIELD.to_string(),
            Value::String(reasoning_content.to_string()),
        );
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let mut chars = value.as_bytes().chunks_exact(2);
    for pair in &mut chars {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        bytes.push(((high << 4) | low) as u8);
    }
    Some(bytes)
}

fn local_reasoning_encrypted_content(reasoning_content: &str) -> String {
    format!(
        "{LOCAL_REASONING_ENCRYPTED_PREFIX}{}",
        hex_encode(reasoning_content.as_bytes())
    )
}

fn local_reasoning_from_encrypted_content(value: &str) -> Option<String> {
    let encoded = value
        .strip_prefix(LOCAL_REASONING_ENCRYPTED_PREFIX)
        .or_else(|| value.strip_prefix(LEGACY_LOCAL_REASONING_ENCRYPTED_PREFIX))?;
    String::from_utf8(hex_decode(encoded)?).ok()
}

#[derive(Debug, Clone)]
struct UpstreamCandidate {
    provider: ProviderConfig,
    base_url: String,
    token: String,
    route_order: usize,
}

#[derive(Debug, Clone)]
struct ClaudeUpstreamCandidate {
    provider: ClaudeProviderConfig,
    base_url: String,
    token: String,
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
    #[serde(default)]
    remote_compaction_v2: RemoteCompactionV2Audit,
    #[serde(default)]
    upstream_model: Option<String>,
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
    first_byte_ms: Option<u64>,
    total_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RemoteCompactionV2Audit {
    #[serde(default)]
    trigger_received: bool,
    #[serde(default)]
    trigger_forwarded: bool,
    #[serde(default)]
    compaction_response_received: bool,
    #[serde(default)]
    compaction_response_forwarded: bool,
    #[serde(default)]
    compaction_item_reused: bool,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderKind {
    Codex,
    Claude,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderFailureKind {
    Network,
    RateLimit,
    UpstreamServer,
    Authentication,
    ResponseRead,
    Stream,
    Protocol,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderFailureEvent {
    id: String,
    request_id: String,
    started_at_ms: i64,
    day: String,
    provider_kind: ProviderKind,
    provider_id: String,
    provider_name: String,
    model: String,
    path: String,
    failure_kind: ProviderFailureKind,
    status_code: Option<u16>,
    error: String,
    counted: bool,
    consecutive_failure_count: u32,
    auto_disabled: bool,
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
}

#[derive(Debug, Clone, Serialize)]
struct RouteUsageStats {
    generated_at_ms: i64,
    filters: RouteLogFilter,
    summary: RouteUsageSummary,
    today: RouteUsageSummary,
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
    remote_compaction_v2: RemoteCompactionV2Audit,
    upstream_model: Option<String>,
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

#[derive(Default)]
struct PendingRouteLogGuard {
    pending: Option<PendingRouteLog>,
}

impl PendingRouteLogGuard {
    fn arm(&mut self, pending: PendingRouteLog) {
        self.pending = Some(pending);
    }

    fn disarm(&mut self) {
        self.pending = None;
    }

    fn pending(&self) -> &PendingRouteLog {
        self.pending
            .as_ref()
            .expect("route log guard must be armed")
    }

    fn take(&mut self) -> PendingRouteLog {
        self.pending.take().expect("route log guard must be armed")
    }
}

impl Drop for PendingRouteLogGuard {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            finish_cancelled_route_log(pending, TokenUsage::default(), None);
        }
    }
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

struct ChatToResponsesStreamState {
    stream: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    pending: PendingRouteLog,
    status_success: bool,
    first_byte_ms: Option<u64>,
    sse_buffer: String,
    response_id: String,
    created_at: i64,
    model: String,
    output_text: String,
    reasoning_content: String,
    output_index: usize,
    next_output_index: usize,
    tool_context: CodexToolContext,
    tool_calls: BTreeMap<usize, ChatToolCallState>,
    completed_output: Vec<(usize, Value)>,
    sequence_number: u64,
    started: bool,
    text_done: bool,
    completed: bool,
    usage_seen: bool,
    usage: TokenUsage,
    finished: bool,
}

struct ClaudeStreamState {
    stream: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    provider_id: String,
    provider_name: String,
    request_id: String,
    started_at_ms: i64,
    path: String,
    model: String,
    status_success: bool,
    status_code: Option<u16>,
    finished: bool,
}

fn default_balance_path() -> String {
    "/api/usage/token/".to_string()
}

fn default_true() -> bool {
    true
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

fn default_connect_timeout_secs() -> u64 {
    DEFAULT_CONNECT_TIMEOUT_SECS
}

fn default_response_header_timeout_secs() -> u64 {
    DEFAULT_RESPONSE_HEADER_TIMEOUT_SECS
}

fn default_stream_idle_timeout_secs() -> u64 {
    DEFAULT_STREAM_IDLE_TIMEOUT_SECS
}

fn validate_router_timeouts(config: &RouterConfig) -> Result<(), String> {
    let values = [
        (
            "连接超时",
            config.connect_timeout_secs,
            MAX_CONNECT_TIMEOUT_SECS,
        ),
        (
            "响应头超时",
            config.response_header_timeout_secs,
            MAX_RESPONSE_HEADER_TIMEOUT_SECS,
        ),
        (
            "流式响应空闲超时",
            config.stream_idle_timeout_secs,
            MAX_STREAM_IDLE_TIMEOUT_SECS,
        ),
    ];
    for (label, value, maximum) in values {
        if value == 0 || value > maximum {
            return Err(format!("{label}必须在 1 到 {maximum} 秒之间"));
        }
    }
    if config.connect_timeout_secs > config.response_header_timeout_secs {
        return Err("响应头超时不能小于连接超时".to_string());
    }
    if config.response_header_timeout_secs > config.stream_idle_timeout_secs {
        return Err("流式响应空闲超时不能小于响应头超时".to_string());
    }
    Ok(())
}

fn random_router_token() -> String {
    let mut bytes = [0_u8; 32];
    if getrandom::fill(&mut bytes).is_ok() {
        let encoded = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("xxswitch-{encoded}")
    } else {
        let fallback = current_epoch_ms().unwrap_or_default();
        let sequence = ROUTE_LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        format!("xxswitch-{fallback:x}-{sequence:x}")
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

fn claude_router_base_url(config: &RouterConfig) -> String {
    format!("http://{}", router_address(config))
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

fn claude_home() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Ok(PathBuf::from(path));
    }

    Ok(home_dir()?.join(".claude"))
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

fn provider_failure_events_path() -> Result<PathBuf, String> {
    Ok(manager_dir()?.join("provider-failure-events.jsonl"))
}

fn codex_config_path() -> Result<PathBuf, String> {
    Ok(codex_home()?.join("config.toml"))
}

fn claude_settings_path() -> Result<PathBuf, String> {
    Ok(claude_home()?.join("settings.json"))
}

fn pi_models_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".pi").join("agent").join("models.json"))
}

fn skill_library_root() -> Result<PathBuf, String> {
    Ok(manager_dir()?.join("skills"))
}

fn skill_backup_root() -> Result<PathBuf, String> {
    Ok(manager_dir()?.join("skill-backups"))
}

fn default_skill_location_for(client: AgentClientKind) -> Result<PathBuf, String> {
    match client {
        AgentClientKind::Codex => Ok(codex_home()?.join("skills")),
        AgentClientKind::Claude => Ok(claude_home()?.join("skills")),
        AgentClientKind::Pi => Ok(home_dir()?.join(".pi").join("agent").join("skills")),
    }
}

fn expand_user_path(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("路径不能为空".to_string());
    }
    if trimmed == "~" {
        return home_dir();
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        return Ok(home_dir()?.join(rest));
    }
    Ok(PathBuf::from(trimmed))
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

fn client_config(clients: &ClientConfigs, client: AgentClientKind) -> &ClientTargetConfig {
    match client {
        AgentClientKind::Codex => &clients.codex,
        AgentClientKind::Claude => &clients.claude,
        AgentClientKind::Pi => &clients.pi,
    }
}

fn client_config_mut(
    clients: &mut ClientConfigs,
    client: AgentClientKind,
) -> &mut ClientTargetConfig {
    match client {
        AgentClientKind::Codex => &mut clients.codex,
        AgentClientKind::Claude => &mut clients.claude,
        AgentClientKind::Pi => &mut clients.pi,
    }
}

fn normalize_skill_locations_for_client(config: &mut ClientTargetConfig, default_path: PathBuf) {
    let default_path = display_path(&default_path);
    let mut seen = BTreeSet::new();
    let mut locations = Vec::new();
    for location in std::mem::take(&mut config.skill_locations) {
        let path = location.path.trim();
        if path.is_empty() {
            continue;
        }
        let normalized = expand_user_path(path)
            .map(|path| display_path(&path))
            .unwrap_or_else(|_| path.to_string());
        if seen.insert(normalized.clone()) {
            locations.push(SkillLocationConfig {
                path: normalized,
                writable: location.writable,
            });
        }
    }
    if locations.is_empty() {
        locations.push(SkillLocationConfig {
            path: default_path.clone(),
            writable: true,
        });
    }

    let mut managed = config.managed_skill_location.trim().to_string();
    if managed.is_empty() {
        managed = locations
            .iter()
            .find(|location| location.writable)
            .map(|location| location.path.clone())
            .unwrap_or_else(|| default_path.clone());
    } else if let Ok(expanded) = expand_user_path(&managed) {
        managed = display_path(&expanded);
    }
    if !locations.iter().any(|location| location.path == managed) {
        locations.push(SkillLocationConfig {
            path: managed.clone(),
            writable: true,
        });
    }
    if !locations
        .iter()
        .any(|location| location.path == managed && location.writable)
    {
        if let Some(first_writable) = locations.iter().find(|location| location.writable) {
            managed = first_writable.path.clone();
        } else {
            managed = default_path.clone();
            locations.push(SkillLocationConfig {
                path: managed.clone(),
                writable: true,
            });
        }
    }

    config.skill_locations = locations;
    config.managed_skill_location = managed;
}

fn normalize_skill_management_state(state: &mut ManagerState) {
    for client in AgentClientKind::all() {
        if let Ok(default_path) = default_skill_location_for(client) {
            normalize_skill_locations_for_client(
                client_config_mut(&mut state.clients, client),
                default_path,
            );
        }
    }

    let mut seen_skills = BTreeSet::new();
    state.skills.shared_skills.retain_mut(|skill| {
        skill.identity = skill.identity.trim().to_string();
        skill.library_dir_name = skill.library_dir_name.trim().to_string();
        if skill.identity.is_empty() || skill.library_dir_name.is_empty() {
            return false;
        }
        if !seen_skills.insert(skill.identity.clone()) {
            return false;
        }
        let mut seen_scope = BTreeSet::new();
        skill
            .sharing_scope
            .retain(|client| seen_scope.insert(*client));
        true
    });
    let known_skills = state
        .skills
        .shared_skills
        .iter()
        .map(|skill| skill.identity.clone())
        .collect::<BTreeSet<_>>();
    state.skills.exposures.retain_mut(|exposure| {
        exposure.path = exposure.path.trim().to_string();
        exposure.skill_location = exposure.skill_location.trim().to_string();
        known_skills.contains(&exposure.skill_identity) && !exposure.path.is_empty()
    });
    let mut seen_exposures = BTreeSet::new();
    state.skills.exposures.retain(|exposure| {
        seen_exposures.insert((
            exposure.skill_identity.clone(),
            exposure.client,
            exposure.path.clone(),
        ))
    });
}

fn parse_skill_frontmatter_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn parse_skill_metadata_text(text: &str) -> (Option<String>, Option<String>) {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }
    let mut name = None;
    let mut description = None;
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            match key.trim() {
                "name" => name = Some(parse_skill_frontmatter_value(value)),
                "description" => description = Some(parse_skill_frontmatter_value(value)),
                _ => {}
            }
        }
    }
    (
        name.filter(|value| !value.is_empty()),
        description.unwrap_or_default().into(),
    )
}

fn read_skill_metadata(skill_dir: &Path) -> Result<(String, String), String> {
    let skill_md = skill_dir.join("SKILL.md");
    if !skill_md.is_file() {
        return Err("Skill 目录缺少 SKILL.md".to_string());
    }
    let raw = fs::read_to_string(&skill_md)
        .map_err(|err| format!("无法读取 {}: {err}", skill_md.display()))?;
    let (name, description) = parse_skill_metadata_text(&raw);
    let fallback = skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("skill")
        .to_string();
    Ok((name.unwrap_or(fallback), description.unwrap_or_default()))
}

fn safe_skill_slug(identity: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in identity.trim().chars() {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if ch == '-' || ch == '_' {
            Some(ch)
        } else if ch.is_whitespace() || matches!(ch, '/' | '\\' | ':' | '.' | '#') {
            Some('-')
        } else {
            Some('-')
        };
        if let Some(ch) = next {
            if ch == '-' {
                if !last_dash && !slug.is_empty() {
                    slug.push(ch);
                    last_dash = true;
                }
            } else {
                slug.push(ch);
                last_dash = false;
            }
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "skill".to_string()
    } else {
        slug
    }
}

fn unique_skill_library_dir_name(state: &ManagerState, identity: &str) -> Result<String, String> {
    let root = skill_library_root()?;
    let base = safe_skill_slug(identity);
    let used = state
        .skills
        .shared_skills
        .iter()
        .map(|skill| skill.library_dir_name.clone())
        .collect::<BTreeSet<_>>();
    let mut candidate = base.clone();
    let mut index = 2;
    while used.contains(&candidate) || root.join(&candidate).exists() {
        candidate = format!("{base}-{index}");
        index += 1;
    }
    Ok(candidate)
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|err| format!("无法创建目录 {}: {err}", dst.display()))?;
    for entry in
        fs::read_dir(src).map_err(|err| format!("无法读取目录 {}: {err}", src.display()))?
    {
        let entry = entry.map_err(|err| format!("无法读取目录项: {err}"))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|err| format!("无法读取文件类型 {}: {err}", src_path.display()))?;
        if file_type.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else if file_type.is_file() || file_type.is_symlink() {
            fs::copy(&src_path, &dst_path).map_err(|err| {
                format!(
                    "无法复制 {} 到 {}: {err}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn remove_file_or_dir(path: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|err| format!("无法读取 {}: {err}", path.display()))?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path).map_err(|err| format!("无法删除 {}: {err}", path.display()))
    } else {
        fs::remove_dir_all(path).map_err(|err| format!("无法删除 {}: {err}", path.display()))
    }
}

fn create_dir_symlink(src: &Path, dst: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(src, dst).map_err(|err| {
            format!(
                "无法创建 symlink {} -> {}: {err}",
                dst.display(),
                src.display()
            )
        })
    }

    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(src, dst).map_err(|err| {
            format!(
                "无法创建 symlink {} -> {}: {err}",
                dst.display(),
                src.display()
            )
        })
    }
}

fn symlink_points_to(path: &Path, target: &Path) -> bool {
    let Ok(link) = fs::read_link(path) else {
        return false;
    };
    if link == target {
        return true;
    }
    let resolved = if link.is_absolute() {
        link
    } else {
        path.parent().unwrap_or_else(|| Path::new(".")).join(link)
    };
    match (fs::canonicalize(resolved), fs::canonicalize(target)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[derive(Debug, Clone)]
struct DiscoveredSkill {
    client: AgentClientKind,
    skill_location: String,
    path: PathBuf,
    dir_name: String,
    identity: String,
    description: String,
    managed: bool,
}

fn exposure_path_matches(exposure: &ManagedSkillExposureConfig, path: &Path) -> bool {
    exposure.path == display_path(path)
}

fn scan_client_skills(state: &ManagerState) -> Vec<DiscoveredSkill> {
    let mut skills = Vec::new();
    for client in AgentClientKind::all() {
        let config = client_config(&state.clients, client);
        for location in &config.skill_locations {
            let Ok(location_path) = expand_user_path(&location.path) else {
                continue;
            };
            let Ok(entries) = fs::read_dir(&location_path) else {
                continue;
            };
            for entry in entries.flatten() {
                let skill_path = entry.path();
                if !skill_path.join("SKILL.md").is_file() {
                    continue;
                }
                let Ok((identity, description)) = read_skill_metadata(&skill_path) else {
                    continue;
                };
                let dir_name = skill_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_string();
                let managed = state
                    .skills
                    .exposures
                    .iter()
                    .any(|exposure| exposure_path_matches(exposure, &skill_path));
                skills.push(DiscoveredSkill {
                    client,
                    skill_location: display_path(&location_path),
                    path: skill_path,
                    dir_name,
                    identity,
                    description,
                    managed,
                });
            }
        }
    }
    skills
}

fn managed_skill_path_for(
    state: &ManagerState,
    client: AgentClientKind,
    skill: &SharedSkillConfig,
) -> Result<PathBuf, String> {
    let managed_location = &client_config(&state.clients, client).managed_skill_location;
    Ok(expand_user_path(managed_location)?.join(&skill.library_dir_name))
}

fn exposure_health(path: &Path, library_path: &Path) -> (String, String) {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                if symlink_points_to(path, library_path) {
                    ("healthy".to_string(), "已链接到 Skill Library".to_string())
                } else {
                    (
                        "broken".to_string(),
                        "symlink 指向的目标不是该 Shared Skill".to_string(),
                    )
                }
            } else {
                (
                    "broken".to_string(),
                    "路径存在但不是 XXSwitch 管理的 symlink".to_string(),
                )
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (
            "missing".to_string(),
            "Exposure Registry 中有记录，但文件系统入口缺失".to_string(),
        ),
        Err(err) => ("broken".to_string(), format!("无法检查 exposure: {err}")),
    }
}

fn build_skill_management_view(state: &ManagerState) -> Result<SkillManagementView, String> {
    let library_root = skill_library_root()?;
    let discovered = scan_client_skills(state);
    let shared_identities = state
        .skills
        .shared_skills
        .iter()
        .map(|skill| skill.identity.clone())
        .collect::<BTreeSet<_>>();

    let clients = AgentClientKind::all()
        .iter()
        .map(|client| {
            let config = client_config(&state.clients, *client);
            SkillClientView {
                client: *client,
                label: client.label().to_string(),
                managed_skill_location: config.managed_skill_location.clone(),
                skill_locations: config
                    .skill_locations
                    .iter()
                    .map(|location| {
                        let path = expand_user_path(&location.path)
                            .map(|path| display_path(&path))
                            .unwrap_or_else(|_| location.path.clone());
                        SkillLocationView {
                            managed: path == config.managed_skill_location,
                            exists: PathBuf::from(&path).is_dir(),
                            path,
                            writable: location.writable,
                        }
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();

    let client_skills = discovered
        .iter()
        .filter(|skill| !skill.managed)
        .map(|skill| ClientSkillView {
            client: skill.client,
            client_label: skill.client.label().to_string(),
            skill_location: skill.skill_location.clone(),
            path: display_path(&skill.path),
            dir_name: skill.dir_name.clone(),
            identity: skill.identity.clone(),
            description: skill.description.clone(),
            managed: skill.managed,
            shared: shared_identities.contains(&skill.identity),
        })
        .collect::<Vec<_>>();

    let mut conflicts = Vec::new();
    let mut location_counts: BTreeMap<(AgentClientKind, String), Vec<&DiscoveredSkill>> =
        BTreeMap::new();
    for skill in discovered.iter().filter(|skill| !skill.managed) {
        location_counts
            .entry((skill.client, skill.identity.clone()))
            .or_default()
            .push(skill);
        if shared_identities.contains(&skill.identity) {
            conflicts.push(SkillConflictView {
                kind: "name".to_string(),
                identity: skill.identity.clone(),
                client: Some(skill.client),
                path: display_path(&skill.path),
                message: format!(
                    "{} 已有同名 Client Skill；不会自动覆盖或合并 Shared Skill",
                    skill.client.label()
                ),
            });
        }
    }
    for ((client, identity), skills) in location_counts {
        if skills.len() > 1 {
            for skill in skills {
                conflicts.push(SkillConflictView {
                    kind: "location".to_string(),
                    identity: identity.clone(),
                    client: Some(client),
                    path: display_path(&skill.path),
                    message: format!("{} 的多个 Skill Location 中发现同名 Skill", client.label()),
                });
            }
        }
    }

    let shared_skills = state
        .skills
        .shared_skills
        .iter()
        .map(|skill| {
            let library_path = library_root.join(&skill.library_dir_name);
            let mut exposures = state
                .skills
                .exposures
                .iter()
                .filter(|exposure| exposure.skill_identity == skill.identity)
                .map(|exposure| {
                    let path = PathBuf::from(&exposure.path);
                    let (health, message) = exposure_health(&path, &library_path);
                    SkillExposureView {
                        client: exposure.client,
                        client_label: exposure.client.label().to_string(),
                        path: exposure.path.clone(),
                        health,
                        message,
                    }
                })
                .collect::<Vec<_>>();
            let registered_clients = exposures
                .iter()
                .map(|exposure| exposure.client)
                .collect::<BTreeSet<_>>();
            for client in &skill.sharing_scope {
                if registered_clients.contains(client) {
                    continue;
                }
                let target_path = managed_skill_path_for(state, *client, skill)
                    .map(|path| display_path(&path))
                    .unwrap_or_default();
                let message = if !target_path.is_empty() && PathBuf::from(&target_path).exists() {
                    format!(
                        "{} 的目标路径已有非 XXSwitch 管理的同名入口",
                        client.label()
                    )
                } else {
                    "Sharing Scope 包含该客户端，但尚未创建 exposure".to_string()
                };
                exposures.push(SkillExposureView {
                    client: *client,
                    client_label: client.label().to_string(),
                    path: target_path,
                    health: "missing".to_string(),
                    message,
                });
            }
            SharedSkillView {
                identity: skill.identity.clone(),
                description: skill.description.clone(),
                library_dir_name: skill.library_dir_name.clone(),
                path: display_path(&library_path),
                sharing_scope: skill.sharing_scope.clone(),
                origin: skill.origin.clone(),
                exposures,
            }
        })
        .collect::<Vec<_>>();

    Ok(SkillManagementView {
        library_root: display_path(&library_root),
        clients,
        shared_skills,
        client_skills,
        conflicts,
    })
}

fn ensure_scope_contains(scope: &mut Vec<AgentClientKind>, client: AgentClientKind) {
    if !scope.contains(&client) {
        scope.push(client);
    }
    let mut seen = BTreeSet::new();
    scope.retain(|client| seen.insert(*client));
}

fn create_or_register_exposure(
    state: &mut ManagerState,
    skill_identity: &str,
    client: AgentClientKind,
    path: PathBuf,
    skill_location: String,
) -> Result<(), String> {
    if state.skills.exposures.iter().any(|exposure| {
        exposure.skill_identity == skill_identity
            && exposure.client == client
            && exposure.path == display_path(&path)
    }) {
        return Ok(());
    }
    let skill = state
        .skills
        .shared_skills
        .iter()
        .find(|skill| skill.identity == skill_identity)
        .ok_or_else(|| "Shared Skill 不存在".to_string())?;
    let library_path = skill_library_root()?.join(&skill.library_dir_name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() && symlink_points_to(&path, &library_path) {
                state.skills.exposures.push(ManagedSkillExposureConfig {
                    skill_identity: skill_identity.to_string(),
                    client,
                    path: display_path(&path),
                    skill_location,
                    created_at_ms: current_epoch_ms().ok(),
                });
            }
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!("无法创建 Skill Location {}: {err}", parent.display())
                })?;
            }
            create_dir_symlink(&library_path, &path)?;
            state.skills.exposures.push(ManagedSkillExposureConfig {
                skill_identity: skill_identity.to_string(),
                client,
                path: display_path(&path),
                skill_location,
                created_at_ms: current_epoch_ms().ok(),
            });
            Ok(())
        }
        Err(err) => Err(format!("无法检查 {}: {err}", path.display())),
    }
}

fn remove_registered_exposure(exposure: &ManagedSkillExposureConfig) -> Result<(), String> {
    let path = PathBuf::from(&exposure.path);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::remove_file(&path)
            .map_err(|err| format!("无法删除 exposure {}: {err}", path.display())),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("无法检查 exposure {}: {err}", path.display())),
    }
}

fn apply_skill_sharing_scope(state: &mut ManagerState, skill_identity: &str) -> Result<(), String> {
    let skill = state
        .skills
        .shared_skills
        .iter()
        .find(|skill| skill.identity == skill_identity)
        .cloned()
        .ok_or_else(|| "Shared Skill 不存在".to_string())?;
    let desired_clients = skill.sharing_scope.iter().copied().collect::<BTreeSet<_>>();

    let mut retained = Vec::new();
    for exposure in std::mem::take(&mut state.skills.exposures) {
        if exposure.skill_identity == skill_identity && !desired_clients.contains(&exposure.client)
        {
            remove_registered_exposure(&exposure)?;
        } else {
            retained.push(exposure);
        }
    }
    state.skills.exposures = retained;

    for client in desired_clients {
        if state
            .skills
            .exposures
            .iter()
            .any(|exposure| exposure.skill_identity == skill_identity && exposure.client == client)
        {
            continue;
        }
        let target_path = managed_skill_path_for(state, client, &skill)?;
        let skill_location = target_path.parent().map(display_path).unwrap_or_default();
        let _ =
            create_or_register_exposure(state, skill_identity, client, target_path, skill_location);
    }
    Ok(())
}

fn backup_skill_directory(source_path: &Path, identity: &str) -> Result<PathBuf, String> {
    let timestamp = current_epoch_ms().unwrap_or_default();
    let backup_root = skill_backup_root()?;
    fs::create_dir_all(&backup_root)
        .map_err(|err| format!("无法创建 Skill 备份目录 {}: {err}", backup_root.display()))?;
    let backup_path = backup_root.join(format!("{}-{timestamp}", safe_skill_slug(identity)));
    copy_dir_all(source_path, &backup_path)?;
    Ok(backup_path)
}

fn promote_discovered_skill(
    state: &mut ManagerState,
    discovered: &DiscoveredSkill,
    mut sharing_scope: Vec<AgentClientKind>,
) -> Result<(), String> {
    if state
        .skills
        .shared_skills
        .iter()
        .any(|skill| skill.identity == discovered.identity)
    {
        return Err("Skill Library 中已存在同名 Shared Skill".to_string());
    }
    ensure_scope_contains(&mut sharing_scope, discovered.client);

    let library_root = skill_library_root()?;
    fs::create_dir_all(&library_root)
        .map_err(|err| format!("无法创建 Skill Library {}: {err}", library_root.display()))?;
    let dir_name = unique_skill_library_dir_name(state, &discovered.identity)?;
    let library_path = library_root.join(&dir_name);
    let temp_path = library_root.join(format!(
        ".tmp-{dir_name}-{}",
        current_epoch_ms().unwrap_or_default()
    ));
    copy_dir_all(&discovered.path, &temp_path)?;
    let backup_path = backup_skill_directory(&discovered.path, &discovered.identity)?;
    fs::rename(&temp_path, &library_path).map_err(|err| {
        let _ = fs::remove_dir_all(&temp_path);
        format!("无法写入 Skill Library {}: {err}", library_path.display())
    })?;

    if let Err(err) = remove_file_or_dir(&discovered.path) {
        let _ = fs::remove_dir_all(&library_path);
        return Err(err);
    }
    if let Err(err) = create_dir_symlink(&library_path, &discovered.path) {
        let _ = fs::remove_dir_all(&library_path);
        let _ = copy_dir_all(&backup_path, &discovered.path);
        return Err(err);
    }

    state.skills.shared_skills.push(SharedSkillConfig {
        identity: discovered.identity.clone(),
        library_dir_name: dir_name,
        description: discovered.description.clone(),
        sharing_scope,
        origin: Some(SkillOrigin {
            client: discovered.client,
            skill_location: discovered.skill_location.clone(),
            original_path: display_path(&discovered.path),
            original_dir_name: discovered.dir_name.clone(),
        }),
        created_at_ms: current_epoch_ms().ok(),
    });
    state.skills.exposures.push(ManagedSkillExposureConfig {
        skill_identity: discovered.identity.clone(),
        client: discovered.client,
        path: display_path(&discovered.path),
        skill_location: discovered.skill_location.clone(),
        created_at_ms: current_epoch_ms().ok(),
    });
    apply_skill_sharing_scope(state, &discovered.identity)
}

fn replace_client_skill_with_shared_skill(
    state: &mut ManagerState,
    discovered: &DiscoveredSkill,
) -> Result<(), String> {
    let shared = state
        .skills
        .shared_skills
        .iter()
        .find(|skill| skill.identity == discovered.identity)
        .cloned()
        .ok_or_else(|| "Shared Skill 不存在".to_string())?;
    let library_path = skill_library_root()?.join(&shared.library_dir_name);
    let backup_path = backup_skill_directory(&discovered.path, &discovered.identity)?;
    if let Err(err) = remove_file_or_dir(&discovered.path) {
        return Err(err);
    }
    if let Err(err) = create_dir_symlink(&library_path, &discovered.path) {
        let _ = copy_dir_all(&backup_path, &discovered.path);
        return Err(err);
    }
    if let Some(skill) = state
        .skills
        .shared_skills
        .iter_mut()
        .find(|skill| skill.identity == discovered.identity)
    {
        ensure_scope_contains(&mut skill.sharing_scope, discovered.client);
    }
    state.skills.exposures.push(ManagedSkillExposureConfig {
        skill_identity: discovered.identity.clone(),
        client: discovered.client,
        path: display_path(&discovered.path),
        skill_location: discovered.skill_location.clone(),
        created_at_ms: current_epoch_ms().ok(),
    });
    apply_skill_sharing_scope(state, &discovered.identity)
}

fn find_discovered_skill(
    state: &ManagerState,
    client: AgentClientKind,
    skill_path: &str,
) -> Result<DiscoveredSkill, String> {
    let requested = expand_user_path(skill_path)?;
    scan_client_skills(state)
        .into_iter()
        .find(|skill| skill.client == client && skill.path == requested)
        .ok_or_else(|| "未在该 Agent Client 的 Skill Locations 中发现指定 Skill".to_string())
}

fn default_state() -> ManagerState {
    ManagerState {
        active_provider_id: String::new(),
        applied_provider_id: None,
        base_template_name: "默认模板".to_string(),
        base: json!({}),
        providers: vec![],
        active_claude_provider_id: String::new(),
        claude_providers: vec![],
        last_applied: None,
        applied_history: vec![],
        router: RouterConfig::default(),
        clients: ClientConfigs::default(),
        router_backup: None,
        claude_backup: None,
        pi_backup: None,
        skills: SkillManagementConfig::default(),
    }
}

fn provider_day_now() -> String {
    let now = Local::now();
    format!("{:04}-{:02}-{:02}", now.year(), now.month(), now.day())
}

fn provider_enabled_from_status(status: ProviderStatus) -> bool {
    matches!(status, ProviderStatus::Enabled)
}

fn normalize_provider_status(
    status: ProviderStatus,
    enabled: bool,
    auto_disabled_day: &Option<String>,
    today: &str,
) -> ProviderStatus {
    match status {
        ProviderStatus::AutoDisabled if auto_disabled_day.as_deref() == Some(today) => {
            ProviderStatus::AutoDisabled
        }
        ProviderStatus::AutoDisabled => ProviderStatus::Enabled,
        ProviderStatus::Enabled if !enabled => ProviderStatus::Disabled,
        other => other,
    }
}

fn set_provider_status_fields(
    status: ProviderStatus,
    enabled: &mut bool,
    consecutive_failure_count: &mut u32,
    auto_disabled_day: &mut Option<String>,
    last_failure_reason: &mut Option<String>,
    last_failure_at_ms: &mut Option<i64>,
) {
    *enabled = provider_enabled_from_status(status);
    match status {
        ProviderStatus::Enabled | ProviderStatus::Disabled => {
            *consecutive_failure_count = 0;
            *auto_disabled_day = None;
            *last_failure_reason = None;
            *last_failure_at_ms = None;
        }
        ProviderStatus::AutoDisabled => {}
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

fn migrate_legacy_clients_if_missing(
    mut state: ManagerState,
    clients_missing: bool,
) -> ManagerState {
    if clients_missing {
        state.clients.codex.enabled = state.router.enabled;
        state.clients.claude.enabled = false;
    }
    state
}

fn load_state_file() -> Result<ManagerState, String> {
    ensure_state_file()?;
    let raw =
        fs::read_to_string(state_path()?).map_err(|err| format!("无法读取状态文件: {err}"))?;
    let state_value: Value =
        serde_json::from_str(&raw).map_err(|err| format!("状态文件 JSON 无效: {err}"))?;
    let clients_missing = state_value.get("clients").is_none();
    let state: ManagerState = serde_json::from_value(state_value.clone())
        .map_err(|err| format!("状态文件 JSON 无效: {err}"))?;
    let state = migrate_legacy_clients_if_missing(state, clients_missing);

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
    let today = provider_day_now();
    for provider in &mut state.providers {
        let previous_status = provider.status;
        provider.status = normalize_provider_status(
            provider.status,
            provider.enabled,
            &provider.auto_disabled_day,
            &today,
        );
        provider.enabled = provider_enabled_from_status(provider.status);
        if provider.status == ProviderStatus::Disabled
            || (previous_status == ProviderStatus::AutoDisabled
                && provider.status == ProviderStatus::Enabled)
        {
            provider.consecutive_failure_count = 0;
            provider.auto_disabled_day = None;
            provider.last_failure_reason = None;
            provider.last_failure_at_ms = None;
        }
        provider.allowed_models =
            normalize_model_names(std::mem::take(&mut provider.allowed_models));
        provider.model_mappings =
            normalize_model_mappings(std::mem::take(&mut provider.model_mappings));
    }
    for provider in &mut state.claude_providers {
        let previous_status = provider.status;
        provider.status = normalize_provider_status(
            provider.status,
            provider.enabled,
            &provider.auto_disabled_day,
            &today,
        );
        provider.enabled = provider_enabled_from_status(provider.status);
        if provider.status == ProviderStatus::Disabled
            || (previous_status == ProviderStatus::AutoDisabled
                && provider.status == ProviderStatus::Enabled)
        {
            provider.consecutive_failure_count = 0;
            provider.auto_disabled_day = None;
            provider.last_failure_reason = None;
            provider.last_failure_at_ms = None;
        }
        provider.allowed_models =
            normalize_model_names(std::mem::take(&mut provider.allowed_models));
        provider.model_mappings =
            normalize_model_mappings(std::mem::take(&mut provider.model_mappings));
    }
    if state.router.local_token.trim().is_empty()
        || state.router.local_token == DEFAULT_ROUTER_TOKEN
        || state.router.local_token == LEGACY_DEFAULT_ROUTER_TOKEN
    {
        state.router.local_token = random_router_token();
    }
    if state.clients.codex.enabled || state.clients.claude.enabled || state.clients.pi.enabled {
        state.router.enabled = true;
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
                .find(|provider| provider.status == ProviderStatus::Enabled)
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
    if state.active_claude_provider_id.trim().is_empty()
        || !state
            .claude_providers
            .iter()
            .any(|provider| provider.id == state.active_claude_provider_id)
    {
        state.active_claude_provider_id = state
            .claude_providers
            .iter()
            .find(|provider| provider.status == ProviderStatus::Enabled)
            .or_else(|| state.claude_providers.first())
            .map(|provider| provider.id.clone())
            .unwrap_or_default();
    }
    normalize_skill_management_state(&mut state);
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

fn preferred_provider_id(providers: &[ProviderConfig]) -> String {
    providers
        .iter()
        .find(|provider| provider.status == ProviderStatus::Enabled)
        .or_else(|| providers.first())
        .map(|provider| provider.id.clone())
        .unwrap_or_default()
}

fn preferred_claude_provider_id(providers: &[ClaudeProviderConfig]) -> String {
    providers
        .iter()
        .find(|provider| provider.status == ProviderStatus::Enabled)
        .or_else(|| providers.first())
        .map(|provider| provider.id.clone())
        .unwrap_or_default()
}

fn delete_provider_from_state(state: &mut ManagerState, provider_id: &str) -> Result<(), String> {
    let removed_index = state
        .providers
        .iter()
        .position(|provider| provider.id == provider_id)
        .ok_or_else(|| "供应商不存在".to_string())?;
    let removed = state.providers.remove(removed_index);

    let active_provider_missing = state.active_provider_id.trim().is_empty()
        || !state
            .providers
            .iter()
            .any(|provider| provider.id == state.active_provider_id);
    if active_provider_missing {
        state.active_provider_id = preferred_provider_id(&state.providers);
    }

    let applied_provider_missing = state
        .applied_provider_id
        .as_ref()
        .is_some_and(|applied_id| {
            !state
                .providers
                .iter()
                .any(|provider| provider.id == applied_id.as_str())
        });
    if applied_provider_missing {
        state.applied_provider_id = None;
    }

    if !state.router.enabled {
        let removed_desired = desired_config(state, Some(&removed));
        if state.last_applied.as_ref() == Some(&removed_desired) {
            state.last_applied = None;
        }
    }

    Ok(())
}

fn delete_claude_provider_from_state(
    state: &mut ManagerState,
    provider_id: &str,
) -> Result<(), String> {
    let removed_index = state
        .claude_providers
        .iter()
        .position(|provider| provider.id == provider_id)
        .ok_or_else(|| "Claude 供应商不存在".to_string())?;
    state.claude_providers.remove(removed_index);

    let active_provider_missing = state.active_claude_provider_id.trim().is_empty()
        || !state
            .claude_providers
            .iter()
            .any(|provider| provider.id == state.active_claude_provider_id);
    if active_provider_missing {
        state.active_claude_provider_id = preferred_claude_provider_id(&state.claude_providers);
    }

    Ok(())
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
        merge_values(&mut desired, &router_patch_desired(&state.router));
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

fn normalize_model_mappings(mappings: Vec<ModelMapping>) -> Vec<ModelMapping> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for mapping in mappings {
        let source = mapping.source.trim();
        let target = mapping.target.trim();
        if source.is_empty() || target.is_empty() {
            continue;
        }
        let key = source.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        normalized.push(ModelMapping {
            source: source.to_string(),
            target: target.to_string(),
        });
    }
    normalized
}

fn normalize_model_names(models: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for model in models {
        let model = model.trim();
        if model.is_empty() {
            continue;
        }
        let key = model.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        normalized.push(model.to_string());
    }
    normalized
}

fn provider_accepts_model(provider: &ProviderConfig, requested_model: &str) -> bool {
    let requested = requested_model.trim();
    if requested.is_empty() || requested == "未知模型" || provider.allowed_models.is_empty() {
        return true;
    }
    provider
        .allowed_models
        .iter()
        .any(|model| model.trim().eq_ignore_ascii_case(requested))
}

fn mapped_model_for_provider(provider: &ProviderConfig, requested_model: &str) -> Option<String> {
    let requested = requested_model.trim();
    if requested.is_empty() || requested == "未知模型" {
        return None;
    }
    provider
        .model_mappings
        .iter()
        .find(|mapping| mapping.source.trim() == requested)
        .map(|mapping| mapping.target.trim().to_string())
        .filter(|target| !target.is_empty() && target != requested)
}

fn claude_provider_accepts_model(provider: &ClaudeProviderConfig, requested_model: &str) -> bool {
    let requested = requested_model.trim();
    if requested.is_empty() || requested == "未知模型" || provider.allowed_models.is_empty() {
        return true;
    }
    provider
        .allowed_models
        .iter()
        .any(|model| model.trim().eq_ignore_ascii_case(requested))
}

fn mapped_model_for_claude_provider(
    provider: &ClaudeProviderConfig,
    requested_model: &str,
) -> Option<String> {
    let requested = requested_model.trim();
    if requested.is_empty() || requested == "未知模型" {
        return None;
    }
    provider
        .model_mappings
        .iter()
        .find(|mapping| mapping.source.trim() == requested)
        .map(|mapping| mapping.target.trim().to_string())
        .filter(|target| !target.is_empty() && target != requested)
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

fn redacted_claude_provider(mut provider: ClaudeProviderConfig) -> ClaudeProviderConfig {
    provider.api_key.clear();
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

fn current_epoch_ms() -> Result<i64, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("系统时间异常: {err}"))?
        .as_millis() as i64)
}

fn update_platform() -> UpdatePlatform {
    if cfg!(target_os = "windows") {
        UpdatePlatform::Windows
    } else if cfg!(target_os = "macos") {
        UpdatePlatform::Macos
    } else {
        UpdatePlatform::Unsupported
    }
}

fn version_from_update_tag(value: &str) -> Result<Version, String> {
    Version::parse(value.trim().trim_start_matches('v'))
        .map_err(|err| format!("无法解析版本号 {value}: {err}"))
}

fn update_is_available(current_version: &str, latest_version: &str) -> Result<bool, String> {
    Ok(version_from_update_tag(current_version)? < version_from_update_tag(latest_version)?)
}

fn update_asset_for_platform<'a>(
    assets: &'a [GithubReleaseAsset],
    platform: UpdatePlatform,
) -> Option<&'a GithubReleaseAsset> {
    match platform {
        UpdatePlatform::Windows => assets
            .iter()
            .find(|asset| asset.name.to_ascii_lowercase().ends_with("_x64-setup.exe"))
            .or_else(|| {
                assets
                    .iter()
                    .find(|asset| asset.name.to_ascii_lowercase().ends_with(".exe"))
            }),
        UpdatePlatform::Macos => assets
            .iter()
            .find(|asset| asset.name.to_ascii_lowercase().ends_with("_universal.dmg"))
            .or_else(|| {
                assets
                    .iter()
                    .find(|asset| asset.name.to_ascii_lowercase().ends_with(".dmg"))
            }),
        UpdatePlatform::Unsupported => None,
    }
}

fn github_release_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(format!("XXSwitch/{}", env!("XXSWITCH_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|err| format!("无法创建更新检查客户端: {err}"))
}

async fn fetch_latest_github_release() -> Result<GithubRelease, String> {
    github_release_client()?
        .get(GITHUB_RELEASES_LATEST_URL)
        .header(ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|err| format!("检查更新时无法连接 GitHub: {err}"))?
        .error_for_status()
        .map_err(|err| format!("读取 GitHub 最新 Release 失败: {err}"))?
        .json::<GithubRelease>()
        .await
        .map_err(|err| format!("无法解析 GitHub Release 信息: {err}"))
}

fn update_check_info(release: &GithubRelease) -> Result<UpdateCheckInfo, String> {
    let current_version = env!("XXSWITCH_VERSION").to_string();
    let latest_version = release.tag_name.trim().to_string();
    let available = update_is_available(&current_version, &latest_version)?;
    let asset = available
        .then(|| update_asset_for_platform(&release.assets, update_platform()))
        .flatten();

    Ok(UpdateCheckInfo {
        current_version,
        latest_version,
        available,
        installable: asset.is_some(),
        asset_name: asset.map(|asset| asset.name.clone()),
        release_url: release.html_url.clone(),
    })
}

fn update_download_path(asset_name: &str) -> Result<PathBuf, String> {
    let file_name = Path::new(asset_name)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "更新包文件名无效".to_string())?;
    let directory = std::env::temp_dir().join("xxswitch-updates");
    fs::create_dir_all(&directory).map_err(|err| format!("无法创建更新下载目录: {err}"))?;
    let timestamp = current_epoch_ms().unwrap_or_default();
    Ok(directory.join(format!("{timestamp}-{file_name}")))
}

fn verify_update_asset_digest(bytes: &[u8], digest: Option<&str>) -> Result<(), String> {
    let Some(expected) = digest.and_then(|digest| digest.strip_prefix("sha256:")) else {
        return Ok(());
    };
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("更新包 SHA-256 校验失败，请重试".to_string())
    }
}

async fn download_update_asset(asset: &GithubReleaseAsset) -> Result<PathBuf, String> {
    if !asset
        .browser_download_url
        .starts_with(GITHUB_RELEASE_DOWNLOAD_PREFIX)
    {
        return Err("更新包下载地址不受信任".to_string());
    }
    let response = github_release_client()?
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|err| format!("下载更新包失败: {err}"))?
        .error_for_status()
        .map_err(|err| format!("下载更新包失败: {err}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("读取更新包失败: {err}"))?;
    if bytes.is_empty() {
        return Err("下载的更新包为空".to_string());
    }
    verify_update_asset_digest(&bytes, asset.digest.as_deref())?;
    let path = update_download_path(&asset.name)?;
    fs::write(&path, &bytes).map_err(|err| format!("无法保存更新包: {err}"))?;
    Ok(path)
}

#[cfg(target_os = "macos")]
fn macos_app_bundle_from_path(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| {
            ancestor
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        })
        .map(Path::to_path_buf)
}

#[cfg(target_os = "macos")]
fn find_macos_app_bundle(root: &Path, depth: usize) -> Result<Option<PathBuf>, String> {
    if depth == 0 {
        return Ok(None);
    }
    let entries = fs::read_dir(root)
        .map_err(|err| format!("无法读取更新镜像内容 {}: {err}", root.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("无法读取更新镜像条目: {err}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("无法读取更新镜像条目类型: {err}"))?;
        if !file_type.is_dir() {
            continue;
        }
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        {
            return Ok(Some(path));
        }
        if let Some(app) = find_macos_app_bundle(&path, depth - 1)? {
            return Ok(Some(app));
        }
    }
    Ok(None)
}

#[cfg(target_os = "macos")]
fn run_macos_command(command: &mut Command, action: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|err| format!("{action}失败: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if detail.is_empty() {
        format!("{action}失败，退出码 {}", output.status)
    } else {
        format!("{action}失败: {detail}")
    })
}

#[cfg(target_os = "macos")]
fn macos_plist_value(app_bundle: &Path, key: &str) -> Result<String, String> {
    let plist = app_bundle.join("Contents/Info.plist");
    let output = Command::new("/usr/libexec/PlistBuddy")
        .arg("-c")
        .arg(format!("Print :{key}"))
        .arg(&plist)
        .output()
        .map_err(|err| format!("无法读取更新应用信息: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "更新应用缺少 {key}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn macos_codesign_team_id(app_bundle: &Path) -> Option<String> {
    let output = Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=4"])
        .arg(app_bundle)
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .map(str::trim)
        .filter(|team| !team.is_empty() && *team != "not set")
        .map(str::to_string)
}

#[cfg(target_os = "macos")]
fn verify_macos_update_app(
    current_app: Option<&Path>,
    update_app: &Path,
    latest_version: &str,
) -> Result<(), String> {
    let bundle_id = macos_plist_value(update_app, "CFBundleIdentifier")?;
    if bundle_id != MACOS_BUNDLE_IDENTIFIER {
        return Err(format!("更新应用标识不匹配: {bundle_id}"));
    }
    let expected_version = latest_version.trim().trim_start_matches('v');
    let actual_version = macos_plist_value(update_app, "CFBundleShortVersionString")?;
    if actual_version != expected_version {
        return Err(format!(
            "更新应用版本不匹配，期望 {expected_version}，实际 {actual_version}"
        ));
    }
    run_macos_command(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(update_app),
        "校验更新应用签名",
    )?;
    if let Some(current_app) = current_app {
        let current_team = macos_codesign_team_id(current_app);
        let update_team = macos_codesign_team_id(update_app);
        if current_team.is_some() && current_team != update_team {
            return Err("更新应用的开发者签名与当前应用不一致".to_string());
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_path_is_writable(path: &Path) -> bool {
    Command::new("/usr/bin/test")
        .arg("-w")
        .arg(path)
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "macos")]
fn shell_quote(value: &Path) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn install_macos_app_directly(source: &Path, target: &Path, backup: &Path) -> Result<(), String> {
    if backup.exists() {
        fs::remove_dir_all(backup).map_err(|err| format!("无法清理旧更新备份: {err}"))?;
    }
    if target.exists() {
        fs::rename(target, backup).map_err(|err| format!("无法备份当前应用: {err}"))?;
    }
    let install_result = run_macos_command(
        Command::new("/usr/bin/ditto").arg(source).arg(target),
        "复制更新应用",
    );
    if let Err(err) = install_result {
        let _ = fs::remove_dir_all(target);
        if backup.exists() {
            let _ = fs::rename(backup, target);
        }
        return Err(err);
    }
    if backup.exists() {
        fs::remove_dir_all(backup).map_err(|err| format!("无法清理更新备份: {err}"))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_macos_app_with_authorization(
    source: &Path,
    target: &Path,
    backup: &Path,
) -> Result<(), String> {
    let source = shell_quote(source);
    let target = shell_quote(target);
    let backup = shell_quote(backup);
    let shell_command = format!(
        "set -e; /bin/rm -rf {backup}; \
         if [ -e {target} ]; then /bin/mv {target} {backup}; fi; \
         if /usr/bin/ditto {source} {target}; then /bin/rm -rf {backup}; \
         else /bin/rm -rf {target}; if [ -e {backup} ]; then /bin/mv {backup} {target}; fi; exit 1; fi"
    );
    let apple_script = format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_string(&shell_command)
    );
    run_macos_command(
        Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(apple_script),
        "安装更新应用",
    )
}

#[cfg(target_os = "macos")]
fn install_macos_update_from_dmg(
    app: &tauri::AppHandle,
    dmg_path: &Path,
    latest_version: &str,
) -> Result<UpdateInstallResult, String> {
    let current_executable =
        std::env::current_exe().map_err(|err| format!("无法定位当前应用: {err}"))?;
    let current_app = macos_app_bundle_from_path(&current_executable);
    if current_app.is_none() {
        Command::new("/usr/bin/open")
            .arg(dmg_path)
            .spawn()
            .map_err(|err| format!("无法打开更新 DMG: {err}"))?;
        return Ok(UpdateInstallResult {
            message: "当前运行位置不是应用包，已打开更新 DMG，请手动拖入 Applications。"
                .to_string(),
            manual_install: true,
        });
    }

    let work_dir = std::env::temp_dir()
        .join("xxswitch-updates")
        .join(format!("macos-{}", current_epoch_ms().unwrap_or_default()));
    let mount_dir = work_dir.join("mount");
    let staged_app = work_dir.join("XXSwitch.app");
    fs::create_dir_all(&mount_dir).map_err(|err| format!("无法创建 DMG 挂载目录: {err}"))?;

    run_macos_command(
        Command::new("/usr/bin/hdiutil")
            .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
            .arg(&mount_dir)
            .arg(dmg_path),
        "挂载更新 DMG",
    )?;

    let prepare_result = (|| {
        let source_app = find_macos_app_bundle(&mount_dir, 4)?
            .ok_or_else(|| "更新 DMG 中未找到应用包".to_string())?;
        verify_macos_update_app(current_app.as_deref(), &source_app, latest_version)?;
        run_macos_command(
            Command::new("/usr/bin/ditto")
                .arg(&source_app)
                .arg(&staged_app),
            "暂存更新应用",
        )?;
        verify_macos_update_app(current_app.as_deref(), &staged_app, latest_version)
    })();

    let detach_result = run_macos_command(
        Command::new("/usr/bin/hdiutil")
            .arg("detach")
            .arg(&mount_dir),
        "卸载更新 DMG",
    );
    prepare_result?;
    detach_result?;

    let current_app = current_app.expect("checked above");
    let target_app = if current_app.starts_with("/Volumes") {
        PathBuf::from("/Applications/XXSwitch.app")
    } else {
        current_app
    };
    let target_parent = target_app
        .parent()
        .ok_or_else(|| "无法确定应用安装目录".to_string())?;
    let backup_app = target_parent.join(format!(
        ".XXSwitch.app.xxswitch-backup-{}",
        current_epoch_ms().unwrap_or_default()
    ));

    if macos_path_is_writable(target_parent) {
        install_macos_app_directly(&staged_app, &target_app, &backup_app)?;
    } else {
        install_macos_app_with_authorization(&staged_app, &target_app, &backup_app)?;
    }
    verify_macos_update_app(None, &target_app, latest_version)?;

    let _ = fs::remove_file(dmg_path);
    let _ = fs::remove_dir_all(&work_dir);
    Command::new("/bin/sh")
        .args([
            "-c",
            "sleep 1; exec /usr/bin/open \"$1\"",
            "xxswitch-relaunch",
        ])
        .arg(&target_app)
        .spawn()
        .map_err(|err| format!("更新已安装，但无法安排自动重启: {err}"))?;

    let app_for_exit = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        app_for_exit.exit(0);
    });
    Ok(UpdateInstallResult {
        message: "更新已安装，XXSwitch 即将重启。".to_string(),
        manual_install: false,
    })
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

fn apply_claude_provider_connection_draft(
    provider: &mut ClaudeProviderConfig,
    base_url: Option<&str>,
    api_key: Option<&str>,
) -> Result<(), String> {
    if let Some(base_url) = base_url {
        let base_url = base_url.trim();
        if base_url.is_empty() {
            return Err("Base URL 不能为空".to_string());
        }
        provider.base_url = base_url.to_string();
    }
    if let Some(api_key) = api_key {
        provider.api_key = api_key.trim().to_string();
    }
    Ok(())
}

fn status_step(
    key: &str,
    label: &str,
    status: &str,
    latency_ms: Option<u64>,
    message: impl Into<String>,
) -> ProviderConnectionTestStep {
    ProviderConnectionTestStep {
        key: key.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        latency_ms,
        message: message.into(),
    }
}

fn model_id_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(model) => Some(model.trim().to_string()).filter(|model| !model.is_empty()),
        Value::Object(_) => value
            .get("id")
            .or_else(|| value.get("slug"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

fn models_from_response_value(value: &Value) -> Vec<String> {
    let model_values = value
        .get("data")
        .or_else(|| value.get("models"))
        .unwrap_or(value);
    let mut models = model_values
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(model_id_from_value)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}

fn claude_model_value_from_id(model: &str) -> Value {
    let model = model.trim();
    json!({
        "id": model,
        "type": "model",
        "display_name": model,
        "created_at": "1970-01-01T00:00:00Z",
    })
}

fn claude_model_value_from_value(value: &Value) -> Option<Value> {
    let id = model_id_from_value(value)?;
    let mut model = match value {
        Value::Object(object) => Value::Object(object.clone()),
        _ => Value::Object(Map::new()),
    };
    let object = model.as_object_mut()?;
    object.insert("id".to_string(), Value::String(id.clone()));
    object
        .entry("type".to_string())
        .or_insert_with(|| Value::String("model".to_string()));
    object
        .entry("display_name".to_string())
        .or_insert_with(|| Value::String(id.clone()));
    object
        .entry("created_at".to_string())
        .or_insert_with(|| Value::String("1970-01-01T00:00:00Z".to_string()));
    Some(model)
}

fn push_claude_model_value(models: &mut Vec<Value>, seen: &mut BTreeSet<String>, model: Value) {
    let Some(id) = model_id_from_value(&model) else {
        return;
    };
    if seen.insert(id.to_lowercase()) {
        models.push(model);
    }
}

fn claude_model_values_from_response_value(value: &Value) -> Vec<Value> {
    let model_values = value
        .get("data")
        .or_else(|| value.get("models"))
        .unwrap_or(value);
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for value in model_values.as_array().into_iter().flatten() {
        if let Some(model) = claude_model_value_from_value(value) {
            push_claude_model_value(&mut models, &mut seen, model);
        }
    }
    models
}

fn claude_route_model_values(
    provider: &ClaudeProviderConfig,
    upstream_models: &[Value],
) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();

    if provider.allowed_models.is_empty() {
        for model in upstream_models {
            push_claude_model_value(&mut models, &mut seen, model.clone());
        }
        for mapping in &provider.model_mappings {
            if !mapping.source.trim().is_empty() && !mapping.target.trim().is_empty() {
                push_claude_model_value(
                    &mut models,
                    &mut seen,
                    claude_model_value_from_id(&mapping.source),
                );
            }
        }
        return models;
    }

    for allowed_model in &provider.allowed_models {
        let allowed_model = allowed_model.trim();
        if allowed_model.is_empty() {
            continue;
        }
        let upstream_model = upstream_models.iter().find(|model| {
            model_id_from_value(model).is_some_and(|id| id.eq_ignore_ascii_case(allowed_model))
        });
        push_claude_model_value(
            &mut models,
            &mut seen,
            upstream_model
                .cloned()
                .unwrap_or_else(|| claude_model_value_from_id(allowed_model)),
        );
    }

    models
}

fn claude_models_value(models: Vec<Value>) -> Value {
    let ids = models
        .iter()
        .filter_map(model_id_from_value)
        .collect::<Vec<_>>();
    json!({
        "data": models,
        "first_id": ids.first().cloned(),
        "last_id": ids.last().cloned(),
        "has_more": false,
    })
}

async fn run_provider_connection_test(
    provider: &ProviderConfig,
    requested_model: Option<&str>,
) -> ProviderConnectionTestResult {
    let mut steps = Vec::new();
    let Some(base_url) =
        custom_provider_base_url(provider).filter(|value| !value.trim().is_empty())
    else {
        steps.push(status_step(
            "base",
            "基础连接",
            "failed",
            None,
            "Base URL 为空",
        ));
        return ProviderConnectionTestResult { ok: false, steps };
    };
    let Some(token) = custom_provider_token(provider).filter(|value| !value.trim().is_empty())
    else {
        steps.push(status_step(
            "base",
            "基础连接",
            "failed",
            None,
            "API Key 为空",
        ));
        return ProviderConnectionTestResult { ok: false, steps };
    };

    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            steps.push(status_step(
                "base",
                "基础连接",
                "failed",
                None,
                format!("创建 HTTP 客户端失败: {err}"),
            ));
            return ProviderConnectionTestResult { ok: false, steps };
        }
    };

    let models_url = join_url(&base_url, "models");
    let started = Instant::now();
    let models_response = client
        .get(&models_url)
        .bearer_auth(token.trim())
        .header("accept", "application/json")
        .send()
        .await;
    let models_latency = started.elapsed().as_millis() as u64;
    let available_models = match models_response {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
                let models = models_from_response_value(&body);
                steps.push(status_step(
                    "base",
                    "基础连接",
                    "ok",
                    Some(models_latency),
                    "上游可访问",
                ));
                steps.push(status_step(
                    "models",
                    "模型接口",
                    "ok",
                    Some(models_latency),
                    "鉴权通过",
                ));
                models
            } else if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                steps.push(status_step(
                    "base",
                    "基础连接",
                    "ok",
                    Some(models_latency),
                    "上游可访问",
                ));
                steps.push(status_step(
                    "models",
                    "模型接口",
                    "failed",
                    Some(models_latency),
                    format!("鉴权失败 HTTP {}", status.as_u16()),
                ));
                return ProviderConnectionTestResult { ok: false, steps };
            } else {
                steps.push(status_step(
                    "base",
                    "基础连接",
                    "ok",
                    Some(models_latency),
                    "上游可访问",
                ));
                steps.push(status_step(
                    "models",
                    "模型接口",
                    "failed",
                    Some(models_latency),
                    format!("/models 返回 HTTP {}，无法验证模型", status.as_u16()),
                ));
                return ProviderConnectionTestResult { ok: false, steps };
            }
        }
        Err(err) => {
            steps.push(status_step(
                "base",
                "基础连接",
                "failed",
                Some(models_latency),
                format!("请求 /models 失败: {err}"),
            ));
            return ProviderConnectionTestResult { ok: false, steps };
        }
    };

    let selected_model = requested_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .or_else(|| {
            let saved = provider.connection_test_model.trim();
            (!saved.is_empty()).then_some(saved)
        })
        .or_else(|| available_models.first().map(String::as_str));
    let Some(test_model) = selected_model else {
        steps.push(status_step(
            "model",
            "模型可用性",
            "failed",
            None,
            "没有可用于测试的模型",
        ));
        return ProviderConnectionTestResult { ok: false, steps };
    };

    if available_models.iter().any(|model| model == test_model) {
        steps.push(status_step(
            "model",
            "模型可用性",
            "ok",
            None,
            format!("模型在列表中: {test_model}"),
        ));
    } else {
        steps.push(status_step(
            "model",
            "模型可用性",
            "failed",
            None,
            format!("模型不在 /models 列表中: {test_model}"),
        ));
    }

    let ok = steps.iter().all(|step| step.status != "failed");
    ProviderConnectionTestResult { ok, steps }
}

fn latency_error_message(status: reqwest::StatusCode, body: &str) -> String {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.split_whitespace().collect::<Vec<_>>().join(" "));
    let message = message.chars().take(240).collect::<String>();
    if message.is_empty() {
        format!("模型请求返回 HTTP {}", status.as_u16())
    } else {
        format!("模型请求返回 HTTP {}: {message}", status.as_u16())
    }
}

#[derive(Debug, Clone, Copy)]
enum LatencyResponseFormat {
    Responses,
    ChatCompletions,
    ClaudeMessages,
}

#[derive(Debug)]
struct ProviderLatencyProbe {
    latency_ms: u64,
    reply: String,
}

fn reply_from_response_value(value: &Value, format: LatencyResponseFormat) -> String {
    match format {
        LatencyResponseFormat::Responses => {
            if let Some(text) = value.get("output_text").and_then(Value::as_str) {
                return text.to_string();
            }
            value
                .get("output")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.get("content").and_then(json_string_content))
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default()
        }
        LatencyResponseFormat::ChatCompletions => value
            .pointer("/choices/0/message/content")
            .and_then(json_string_content)
            .unwrap_or_default(),
        LatencyResponseFormat::ClaudeMessages => value
            .get("content")
            .and_then(json_string_content)
            .unwrap_or_default(),
    }
}

fn reply_from_stream_event(value: &Value, format: LatencyResponseFormat) -> Option<String> {
    match format {
        LatencyResponseFormat::Responses => {
            if value.get("type").and_then(Value::as_str) == Some("response.output_text.delta") {
                return value
                    .get("delta")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            value
                .pointer("/choices/0/delta/content")
                .and_then(json_string_content)
        }
        LatencyResponseFormat::ChatCompletions => value
            .pointer("/choices/0/delta/content")
            .and_then(json_string_content),
        LatencyResponseFormat::ClaudeMessages => {
            if value.get("type").and_then(Value::as_str) == Some("content_block_delta") {
                return value
                    .pointer("/delta/text")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            None
        }
    }
}

fn reply_from_response_text(text: &str, format: LatencyResponseFormat, stream: bool) -> String {
    if !stream {
        return serde_json::from_str::<Value>(text)
            .ok()
            .map(|value| reply_from_response_value(&value, format))
            .unwrap_or_default();
    }

    let mut reply = String::new();
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
            if let Some(delta) = reply_from_stream_event(&value, format) {
                reply.push_str(&delta);
            }
        }
    }
    if reply.is_empty() {
        serde_json::from_str::<Value>(text)
            .ok()
            .map(|value| reply_from_response_value(&value, format))
            .unwrap_or_default()
    } else {
        reply
    }
}

async fn finish_latency_request(
    response: Result<reqwest::Response, reqwest::Error>,
    started: Instant,
    format: LatencyResponseFormat,
    stream: bool,
) -> Result<ProviderLatencyProbe, String> {
    let response = response.map_err(|err| format!("模型请求失败: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("读取模型响应失败: {err}"))?;
    let latency_ms = started.elapsed().as_millis() as u64;
    if status.is_success() {
        Ok(ProviderLatencyProbe {
            latency_ms,
            reply: reply_from_response_text(&body, format, stream),
        })
    } else {
        Err(latency_error_message(status, &body))
    }
}

async fn measure_codex_provider_latency(
    provider: &ProviderConfig,
    model: &str,
    prompt: &str,
    stream: bool,
) -> Result<ProviderLatencyProbe, String> {
    let base_url = custom_provider_base_url(provider)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Base URL 为空".to_string())?;
    let token = custom_provider_token(provider)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "API Key 为空".to_string())?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|err| format!("创建 HTTP 客户端失败: {err}"))?;
    let (path, format, mut body) = match provider.wire_api {
        ProviderWireApi::Responses => (
            "responses",
            LatencyResponseFormat::Responses,
            json!({
                "model": model,
                "input": prompt,
                "max_output_tokens": 16,
                "stream": stream,
            }),
        ),
        ProviderWireApi::ChatCompletions => (
            "chat/completions",
            LatencyResponseFormat::ChatCompletions,
            json!({
                "model": model,
                "messages": [{ "role": "user", "content": prompt }],
                "max_tokens": 16,
                "stream": stream,
            }),
        ),
    };
    if !provider.service_tier.trim().is_empty() {
        if let Some(object) = body.as_object_mut() {
            object.insert(
                "service_tier".to_string(),
                Value::String(provider.service_tier.trim().to_string()),
            );
        }
    }

    let started = Instant::now();
    let response = client
        .post(join_url(&base_url, path))
        .bearer_auth(token.trim())
        .header(
            "accept",
            if stream {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .json(&body)
        .send()
        .await;
    finish_latency_request(response, started, format, stream).await
}

async fn measure_claude_provider_latency(
    provider: &ClaudeProviderConfig,
    model: &str,
    prompt: &str,
    stream: bool,
) -> Result<ProviderLatencyProbe, String> {
    let base_url = provider.base_url.trim();
    if base_url.is_empty() {
        return Err("Base URL 为空".to_string());
    }
    let api_key = provider.api_key.trim();
    if api_key.is_empty() {
        return Err("API Key 为空".to_string());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|err| format!("创建 HTTP 客户端失败: {err}"))?;
    let body = json!({
        "model": model,
        "max_tokens": 16,
        "messages": [{ "role": "user", "content": prompt }],
        "stream": stream,
    });
    let started = Instant::now();
    let response = client
        .post(join_url(base_url, "messages"))
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header(
            "accept",
            if stream {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .json(&body)
        .send()
        .await;
    finish_latency_request(
        response,
        started,
        LatencyResponseFormat::ClaudeMessages,
        stream,
    )
    .await
}

fn latency_connection_status(result: Result<u64, String>) -> ConnectionStatus {
    match result {
        Ok(latency_ms) => ConnectionStatus {
            ok: true,
            latency_ms: Some(latency_ms),
            checked_at: now_epoch_seconds().ok(),
            error: None,
        },
        Err(error) => ConnectionStatus {
            ok: false,
            latency_ms: None,
            checked_at: now_epoch_seconds().ok(),
            error: Some(error),
        },
    }
}

async fn fetch_provider_models(provider: &ProviderConfig) -> Result<(Vec<String>, u64), String> {
    let base_url = custom_provider_base_url(provider)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Base URL 为空".to_string())?;
    let token = custom_provider_token(provider)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "API Key 为空".to_string())?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|err| format!("创建 HTTP 客户端失败: {err}"))?;

    let models_url = join_url(&base_url, "models");
    let started = Instant::now();
    let response = client
        .get(&models_url)
        .bearer_auth(token.trim())
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("请求 /models 失败: {err}"))?;
    let latency_ms = started.elapsed().as_millis() as u64;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(format!("鉴权失败 HTTP {}", status.as_u16()));
    }
    if !status.is_success() {
        return Err(format!("/models 返回 HTTP {}", status.as_u16()));
    }

    let value = response
        .json::<Value>()
        .await
        .map_err(|err| format!("/models 响应不是有效 JSON: {err}"))?;
    Ok((models_from_response_value(&value), latency_ms))
}

async fn fetch_claude_provider_model_values(
    client: &reqwest::Client,
    provider: &ClaudeProviderConfig,
) -> Result<(Vec<Value>, u64), String> {
    let base_url = provider.base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err("Base URL 为空".to_string());
    }
    let token = provider.api_key.trim();
    if token.is_empty() {
        return Err("API Key 为空".to_string());
    }

    let upstream_path = claude_upstream_path("models");
    let models_url = join_url(base_url, &upstream_path);
    let started = Instant::now();
    let response = client
        .get(&models_url)
        .header("x-api-key", token)
        .header("anthropic-version", "2023-06-01")
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("请求 Claude 模型列表失败: {err}"))?;
    let latency_ms = started.elapsed().as_millis() as u64;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(format!("鉴权失败 HTTP {}", status.as_u16()));
    }
    if !status.is_success() {
        return Err(format!("Claude 模型列表返回 HTTP {}", status.as_u16()));
    }

    let value = response
        .json::<Value>()
        .await
        .map_err(|err| format!("Claude 模型列表响应不是有效 JSON: {err}"))?;
    Ok((claude_model_values_from_response_value(&value), latency_ms))
}

async fn fetch_claude_provider_models(
    provider: &ClaudeProviderConfig,
) -> Result<(Vec<String>, u64), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|err| format!("创建 HTTP 客户端失败: {err}"))?;
    let (models, latency_ms) = fetch_claude_provider_model_values(&client, provider).await?;
    Ok((
        models.iter().filter_map(model_id_from_value).collect(),
        latency_ms,
    ))
}

fn connection_latency_from_test(result: &ProviderConnectionTestResult) -> Option<u64> {
    result
        .steps
        .iter()
        .find(|step| step.key == "models")
        .and_then(|step| step.latency_ms)
        .or_else(|| {
            result
                .steps
                .iter()
                .find(|step| step.key == "base")
                .and_then(|step| step.latency_ms)
        })
}

fn connection_status_from_test(result: &ProviderConnectionTestResult) -> ConnectionStatus {
    let latency_ms = connection_latency_from_test(result);
    let error = result
        .steps
        .iter()
        .find(|step| step.status == "failed")
        .map(|step| step.message.clone());

    ConnectionStatus {
        ok: result.ok,
        latency_ms,
        checked_at: now_epoch_seconds().ok(),
        error,
    }
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
                | "model_providers.custom.name"
                | "model_providers.custom.base_url"
                | "model_providers.custom.experimental_bearer_token"
                | "features.remote_compaction_v2"
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
    let marker_present = raw.contains(MARKER) || raw.contains(LEGACY_MARKER);
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
        custom_name: capture_toml_field(doc, "model_providers.custom.name"),
        remote_compaction_v2: capture_toml_field(doc, "features.remote_compaction_v2"),
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
        restore_toml_field(&mut doc, "model_providers.custom.name", &backup.custom_name)?;
        restore_toml_field(
            &mut doc,
            "features.remote_compaction_v2",
            &backup.remote_compaction_v2,
        )?;
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
            "model_providers.custom.name",
            "model_providers.custom.base_url",
            "model_providers.custom.experimental_bearer_token",
            "features.remote_compaction_v2",
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
    let custom_name = if router.remote_compaction_enabled {
        "OpenAI"
    } else {
        "custom"
    };
    set_toml_path(
        &mut doc,
        "model_providers.custom.name",
        &Value::String(custom_name.to_string()),
    )?;
    set_toml_path(
        &mut doc,
        "features.remote_compaction_v2",
        &Value::Bool(router.remote_compaction_enabled),
    )?;

    let mut raw = doc.to_string();
    if !marker_present && !raw.contains(MARKER) {
        raw = format!("{MARKER}\n{raw}");
    }

    Ok(raw)
}

fn router_patch_desired(router: &RouterConfig) -> Value {
    let mut custom = Map::new();
    custom.insert(
        "base_url".to_string(),
        Value::String(router_base_url(router)),
    );
    custom.insert(
        "experimental_bearer_token".to_string(),
        Value::String(router.local_token.clone()),
    );
    custom.insert(
        "name".to_string(),
        Value::String(
            if router.remote_compaction_enabled {
                "OpenAI"
            } else {
                "custom"
            }
            .to_string(),
        ),
    );
    json!({
        "model_provider": "custom",
        "model_providers": {
            "custom": custom,
        },
        "features": {
            "remote_compaction_v2": router.remote_compaction_enabled,
        },
    })
}

fn router_patch_matches_current(current_json: &Value, router: &RouterConfig) -> bool {
    let desired = router_patch_desired(router);
    let desired_flat = flatten(&desired);
    let current_flat = flatten(current_json);
    desired_flat
        .iter()
        .all(|(path, desired)| current_flat.get(path) == Some(desired))
}

fn read_claude_settings() -> Result<(Value, String, bool), String> {
    let path = claude_settings_path()?;
    if !path.exists() {
        return Ok((Value::Object(Map::new()), String::new(), false));
    }

    let raw = fs::read_to_string(&path).map_err(|err| format!("无法读取 Claude 设置: {err}"))?;
    let settings = serde_json::from_str::<Value>(&raw)
        .map_err(|err| format!("Claude 设置 JSON 无效: {err}"))?;
    if !settings.is_object() {
        return Err("Claude 设置 JSON 顶层必须是对象".to_string());
    }
    Ok((settings, raw, true))
}

fn json_env_value(settings: &Value, key: &str) -> Option<Value> {
    settings
        .get("env")
        .and_then(Value::as_object)
        .and_then(|env| env.get(key))
        .cloned()
}

fn capture_json_env_field(settings: &Value, key: &str) -> RouterFieldBackup {
    let value = json_env_value(settings, key);
    RouterFieldBackup {
        existed: value.is_some(),
        value,
    }
}

fn ensure_settings_object(value: &mut Value) -> Result<&mut Map<String, Value>, String> {
    value
        .as_object_mut()
        .ok_or_else(|| "Claude 设置 JSON 顶层必须是对象".to_string())
}

fn ensure_env_object(settings: &mut Value) -> Result<&mut Map<String, Value>, String> {
    let settings = ensure_settings_object(settings)?;
    let entry = settings
        .entry("env".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry
        .as_object_mut()
        .ok_or_else(|| "Claude 设置 env 必须是对象".to_string())
}

fn set_json_env_field(settings: &mut Value, key: &str, value: Value) -> Result<(), String> {
    ensure_env_object(settings)?.insert(key.to_string(), value);
    Ok(())
}

fn restore_json_env_field(
    settings: &mut Value,
    key: &str,
    backup: &RouterFieldBackup,
) -> Result<(), String> {
    if backup.existed {
        if let Some(value) = backup.value.as_ref() {
            set_json_env_field(settings, key, value.clone())?;
        } else {
            ensure_env_object(settings)?.remove(key);
        }
    } else {
        ensure_env_object(settings)?.remove(key);
    }
    Ok(())
}

fn capture_claude_backup(settings: &Value) -> ClaudeApplyBackup {
    ClaudeApplyBackup {
        base_url: capture_json_env_field(settings, "ANTHROPIC_BASE_URL"),
        auth_token: capture_json_env_field(settings, "ANTHROPIC_AUTH_TOKEN"),
        api_key: capture_json_env_field(settings, "ANTHROPIC_API_KEY"),
        gateway_model_discovery: capture_json_env_field(
            settings,
            "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY",
        ),
        default_fable_model: capture_json_env_field(settings, "ANTHROPIC_DEFAULT_FABLE_MODEL"),
    }
}

fn claude_patch_desired(router: &RouterConfig) -> Value {
    json!({
        "ANTHROPIC_BASE_URL": claude_router_base_url(router),
        "ANTHROPIC_API_KEY": router.local_token,
        "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY": "1",
        "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5",
    })
}

fn claude_patch_matches_current(settings: &Value, router: &RouterConfig) -> bool {
    if json_env_value(settings, "ANTHROPIC_AUTH_TOKEN").is_some() {
        return false;
    }
    let desired = claude_patch_desired(router);
    let Some(desired) = desired.as_object() else {
        return false;
    };
    desired
        .iter()
        .all(|(key, value)| json_env_value(settings, key) == Some(value.clone()))
}

fn render_claude_patch_json(mut settings: Value, router: &RouterConfig) -> Result<String, String> {
    set_json_env_field(
        &mut settings,
        "ANTHROPIC_BASE_URL",
        Value::String(claude_router_base_url(router)),
    )?;
    ensure_env_object(&mut settings)?.remove("ANTHROPIC_AUTH_TOKEN");
    set_json_env_field(
        &mut settings,
        "ANTHROPIC_API_KEY",
        Value::String(router.local_token.clone()),
    )?;
    set_json_env_field(
        &mut settings,
        "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY",
        Value::String("1".to_string()),
    )?;
    set_json_env_field(
        &mut settings,
        "ANTHROPIC_DEFAULT_FABLE_MODEL",
        Value::String("claude-fable-5".to_string()),
    )?;
    serde_json::to_string_pretty(&settings).map_err(|err| format!("无法生成 Claude 设置: {err}"))
}

fn restore_claude_backup_json(
    mut settings: Value,
    backup: Option<&ClaudeApplyBackup>,
    router: &RouterConfig,
) -> Result<String, String> {
    if let Some(backup) = backup {
        restore_json_env_field(&mut settings, "ANTHROPIC_BASE_URL", &backup.base_url)?;
        restore_json_env_field(&mut settings, "ANTHROPIC_AUTH_TOKEN", &backup.auth_token)?;
        restore_json_env_field(&mut settings, "ANTHROPIC_API_KEY", &backup.api_key)?;
        restore_json_env_field(
            &mut settings,
            "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY",
            &backup.gateway_model_discovery,
        )?;
        restore_json_env_field(
            &mut settings,
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            &backup.default_fable_model,
        )?;
    } else {
        let desired = claude_patch_desired(router);
        if json_env_value(&settings, "ANTHROPIC_BASE_URL")
            == desired.get("ANTHROPIC_BASE_URL").cloned()
        {
            ensure_env_object(&mut settings)?.remove("ANTHROPIC_BASE_URL");
        }
        if json_env_value(&settings, "ANTHROPIC_AUTH_TOKEN")
            == Some(Value::String(router.local_token.clone()))
        {
            ensure_env_object(&mut settings)?.remove("ANTHROPIC_AUTH_TOKEN");
        }
        if json_env_value(&settings, "ANTHROPIC_API_KEY")
            == desired.get("ANTHROPIC_API_KEY").cloned()
        {
            ensure_env_object(&mut settings)?.remove("ANTHROPIC_API_KEY");
        }
        if json_env_value(&settings, "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY")
            == desired
                .get("CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY")
                .cloned()
        {
            ensure_env_object(&mut settings)?.remove("CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY");
        }
        if json_env_value(&settings, "ANTHROPIC_DEFAULT_FABLE_MODEL")
            == desired.get("ANTHROPIC_DEFAULT_FABLE_MODEL").cloned()
        {
            ensure_env_object(&mut settings)?.remove("ANTHROPIC_DEFAULT_FABLE_MODEL");
        }
    }
    serde_json::to_string_pretty(&settings).map_err(|err| format!("无法生成 Claude 设置: {err}"))
}

fn read_pi_models_config() -> Result<(Value, String, bool), String> {
    let path = pi_models_path()?;
    if !path.exists() {
        return Ok((Value::Object(Map::new()), String::new(), false));
    }

    let raw = fs::read_to_string(&path).map_err(|err| format!("无法读取 Pi 模型配置: {err}"))?;
    let config = serde_json::from_str::<Value>(&raw)
        .map_err(|err| format!("Pi 模型配置 JSON 无效: {err}"))?;
    if !config.is_object() {
        return Err("Pi 模型配置 JSON 顶层必须是对象".to_string());
    }
    Ok((config, raw, true))
}

fn ensure_json_object<'a>(
    value: &'a mut Value,
    label: &str,
) -> Result<&'a mut Map<String, Value>, String> {
    value
        .as_object_mut()
        .ok_or_else(|| format!("{label} 必须是对象"))
}

fn ensure_pi_providers_object(config: &mut Value) -> Result<&mut Map<String, Value>, String> {
    let config = ensure_json_object(config, "Pi 模型配置 JSON 顶层")?;
    let entry = config
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry
        .as_object_mut()
        .ok_or_else(|| "Pi providers 必须是对象".to_string())
}

fn capture_pi_backup(config: &Value) -> PiApplyBackup {
    let provider = config
        .get("providers")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get(PI_PROVIDER_ID))
        .cloned();
    PiApplyBackup {
        provider: RouterFieldBackup {
            existed: provider.is_some(),
            value: provider,
        },
    }
}

fn route_models_for_pi(state: &ManagerState) -> Vec<String> {
    let candidates = state
        .providers
        .iter()
        .enumerate()
        .filter(|(_, provider)| provider.status == ProviderStatus::Enabled)
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
    configured_route_models(&candidates)
}

fn pi_provider_value(router: &RouterConfig, models: &[String]) -> Value {
    json!({
        "baseUrl": router_base_url(router),
        "api": PI_PROVIDER_API,
        "apiKey": router.local_token,
        "models": models
            .iter()
            .map(|model| {
                json!({
                    "id": model,
                    "contextWindow": CODEX_MODEL_CONTEXT_WINDOW,
                    "reasoning": true,
                    "thinkingLevelMap": {
                        "off": null,
                        "minimal": null,
                        "low": "low",
                        "medium": "medium",
                        "high": "high",
                        "xhigh": "xhigh"
                    }
                })
            })
            .collect::<Vec<_>>()
    })
}

fn render_pi_models_config(
    mut config: Value,
    router: &RouterConfig,
    models: &[String],
) -> Result<String, String> {
    let providers = ensure_pi_providers_object(&mut config)?;
    providers.remove(LEGACY_PI_PROVIDER_ID);
    providers.insert(
        PI_PROVIDER_ID.to_string(),
        pi_provider_value(router, models),
    );
    serde_json::to_string_pretty(&config).map_err(|err| format!("无法生成 Pi 模型配置: {err}"))
}

fn restore_pi_models_config(
    mut config: Value,
    backup: Option<&PiApplyBackup>,
) -> Result<String, String> {
    let providers = ensure_pi_providers_object(&mut config)?;
    providers.remove(LEGACY_PI_PROVIDER_ID);
    if let Some(backup) = backup {
        if backup.provider.existed {
            if let Some(value) = backup.provider.value.as_ref() {
                providers.insert(PI_PROVIDER_ID.to_string(), value.clone());
            } else {
                providers.remove(PI_PROVIDER_ID);
            }
        } else {
            providers.remove(PI_PROVIDER_ID);
        }
    } else {
        providers.remove(PI_PROVIDER_ID);
    }
    serde_json::to_string_pretty(&config).map_err(|err| format!("无法生成 Pi 模型配置: {err}"))
}

fn pi_patch_matches_current(config: &Value, router: &RouterConfig, models: &[String]) -> bool {
    config
        .get("providers")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get(PI_PROVIDER_ID))
        == Some(&pi_provider_value(router, models))
}

fn ensure_router_config_applied(state: &mut ManagerState) -> Result<bool, String> {
    if !state.clients.codex.enabled {
        return Ok(false);
    }

    let (doc, marker_present, _, _) = read_current_toml()?;
    let desired = router_patch_desired(&state.router);
    let current_json = toml_doc_to_json(&doc);
    if router_patch_matches_current(&current_json, &state.router)
        && state.last_applied.as_ref() == Some(&desired)
    {
        return Ok(false);
    }

    let was_managed = marker_present
        || state.router_backup.is_some()
        || state.last_applied.as_ref() == Some(&desired);
    if !was_managed {
        return Ok(false);
    }

    if state.router_backup.is_none() {
        state.router_backup = Some(capture_router_backup(&doc));
    }
    let raw = render_router_patch_toml(doc, marker_present, &state.router)?;
    fs::write(codex_config_path()?, raw).map_err(|err| format!("无法写入 Codex 配置: {err}"))?;
    state.last_applied = Some(desired);
    state.applied_provider_id = None;
    Ok(true)
}

fn ensure_claude_config_applied(state: &mut ManagerState) -> Result<bool, String> {
    if !state.clients.claude.enabled {
        return Ok(false);
    }

    let (settings, _, _) = read_claude_settings()?;
    if claude_patch_matches_current(&settings, &state.router) {
        return Ok(false);
    }

    if state.claude_backup.is_none() {
        state.claude_backup = Some(capture_claude_backup(&settings));
    }
    fs::create_dir_all(claude_home()?).map_err(|err| format!("无法创建 Claude 设置目录: {err}"))?;
    let raw = render_claude_patch_json(settings, &state.router)?;
    fs::write(claude_settings_path()?, raw)
        .map_err(|err| format!("无法写入 Claude 设置: {err}"))?;
    Ok(true)
}

fn ensure_pi_config_applied(state: &mut ManagerState) -> Result<bool, String> {
    if !state.clients.pi.enabled {
        return Ok(false);
    }

    let models = route_models_for_pi(state);
    if models.is_empty() {
        return Err("Pi 接管需要至少一个已启用且配置完整的 Codex 供应商路由模型".to_string());
    }

    let (config, _, _) = read_pi_models_config()?;
    if pi_patch_matches_current(&config, &state.router, &models) {
        return Ok(false);
    }

    if state.pi_backup.is_none() {
        state.pi_backup = Some(capture_pi_backup(&config));
    }
    fs::create_dir_all(
        pi_models_path()?
            .parent()
            .ok_or_else(|| "无法定位 Pi 模型配置目录".to_string())?,
    )
    .map_err(|err| format!("无法创建 Pi 模型配置目录: {err}"))?;
    let raw = render_pi_models_config(config, &state.router, &models)?;
    fs::write(pi_models_path()?, raw).map_err(|err| format!("无法写入 Pi 模型配置: {err}"))?;
    Ok(true)
}

fn ensure_client_configs_applied(state: &mut ManagerState) -> Result<bool, String> {
    if state.clients.codex.enabled || state.clients.claude.enabled || state.clients.pi.enabled {
        state.router.enabled = true;
    }
    let codex_changed = ensure_router_config_applied(state)?;
    let claude_changed = ensure_claude_config_applied(state)?;
    let pi_changed = ensure_pi_config_applied(state)?;
    Ok(codex_changed || claude_changed || pi_changed)
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
    let has_enabled = state
        .providers
        .iter()
        .any(|provider| provider.status == ProviderStatus::Enabled);
    let has_auto_disabled = state
        .providers
        .iter()
        .any(|provider| provider.status == ProviderStatus::AutoDisabled);
    let candidates = state
        .providers
        .iter()
        .enumerate()
        .filter(|(_, provider)| provider.status == ProviderStatus::Enabled)
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

    if candidates.is_empty() && !has_enabled && has_auto_disabled {
        return Err("所有供应商今日已自动禁用".to_string());
    }
    if candidates.is_empty() && !has_enabled {
        return Err("没有已启用的供应商".to_string());
    }
    if candidates.is_empty() {
        return Err("没有已启用且配置完整的供应商".to_string());
    }
    Ok((state.router, candidates))
}

fn upstream_claude_candidates() -> Result<(RouterConfig, Vec<ClaudeUpstreamCandidate>), String> {
    let state = load_state_file()?;
    if !state.router.enabled {
        return Err("本地路由未启用".to_string());
    }
    if !state.clients.claude.enabled {
        return Err("Claude 接管未启用".to_string());
    }
    let has_enabled = state
        .claude_providers
        .iter()
        .any(|provider| provider.status == ProviderStatus::Enabled);
    let has_auto_disabled = state
        .claude_providers
        .iter()
        .any(|provider| provider.status == ProviderStatus::AutoDisabled);
    let candidates = state
        .claude_providers
        .iter()
        .filter(|provider| provider.status == ProviderStatus::Enabled)
        .filter_map(|provider| {
            let base_url = provider.base_url.trim().trim_end_matches('/');
            let token = provider.api_key.trim();
            if base_url.is_empty() || token.is_empty() {
                return None;
            }
            Some(ClaudeUpstreamCandidate {
                provider: provider.clone(),
                base_url: base_url.to_string(),
                token: token.to_string(),
            })
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() && !has_enabled && has_auto_disabled {
        return Err("所有 Claude 供应商今日已自动禁用".to_string());
    }
    if candidates.is_empty() && !has_enabled {
        return Err("没有已启用的 Claude 供应商".to_string());
    }
    if candidates.is_empty() {
        return Err("没有已启用且配置完整的 Claude 供应商".to_string());
    }
    Ok((state.router, candidates))
}

fn auto_disabled_codex_provider_supports_model(model: &str) -> bool {
    load_state_file()
        .map(|state| {
            state.providers.iter().any(|provider| {
                provider.status == ProviderStatus::AutoDisabled
                    && provider_accepts_model(provider, model)
                    && custom_provider_base_url(provider)
                        .is_some_and(|value| !value.trim().is_empty())
                    && custom_provider_token(provider).is_some_and(|value| !value.trim().is_empty())
            })
        })
        .unwrap_or(false)
}

fn auto_disabled_claude_provider_supports_model(model: &str) -> bool {
    load_state_file()
        .map(|state| {
            state.claude_providers.iter().any(|provider| {
                provider.status == ProviderStatus::AutoDisabled
                    && claude_provider_accepts_model(provider, model)
                    && !provider.base_url.trim().is_empty()
                    && !provider.api_key.trim().is_empty()
            })
        })
        .unwrap_or(false)
}

fn configured_route_models(candidates: &[UpstreamCandidate]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for candidate in candidates {
        for model in &candidate.provider.allowed_models {
            let model = model.trim();
            if model.is_empty() || model.eq_ignore_ascii_case("gpt-image-2") {
                continue;
            }
            let key = model.to_lowercase();
            if !seen.insert(key) {
                continue;
            }
            models.push(model.to_string());
        }
    }
    models
}

fn codex_model_catalog_requested(query: Option<&str>) -> bool {
    query
        .unwrap_or_default()
        .split('&')
        .filter(|part| !part.is_empty())
        .any(|part| {
            let key = part.split_once('=').map(|(key, _)| key).unwrap_or(part);
            key == "client_version"
        })
}

fn openai_models_value(models: Vec<String>) -> Value {
    let data = models
        .into_iter()
        .map(|model| {
            json!({
                "id": model,
                "object": "model",
                "created": 0,
                "owned_by": "xxswitch"
            })
        })
        .collect::<Vec<_>>();
    json!({ "object": "list", "data": data })
}

fn codex_model_catalog_templates_from_value(value: &Value) -> Vec<Value> {
    value
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn read_codex_model_catalog_templates(path: &Path) -> Vec<Value> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    codex_model_catalog_templates_from_value(&value)
}

fn load_cached_codex_model_catalog_templates() -> Vec<Value> {
    let Ok(path) = codex_home().map(|path| path.join("models_cache.json")) else {
        return Vec::new();
    };
    read_codex_model_catalog_templates(&path)
}

fn codex_cli_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    let mut push = |path: PathBuf| {
        if !path.as_os_str().is_empty() && seen.insert(path.clone()) {
            candidates.push(path);
        }
    };

    if let Some(path) = std::env::var_os("CODEX_CLI_PATH") {
        push(PathBuf::from(path));
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            #[cfg(target_os = "windows")]
            push(parent.join("codex.exe"));
            #[cfg(not(target_os = "windows"))]
            push(parent.join("codex"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        for app_name in ["ChatGPT.app", "Codex.app"] {
            push(
                Path::new("/Applications")
                    .join(app_name)
                    .join("Contents/Resources/codex"),
            );
        }
        if let Ok(home) = home_dir() {
            for app_name in ["ChatGPT.app", "Codex.app"] {
                push(
                    home.join("Applications")
                        .join(app_name)
                        .join("Contents/Resources/codex"),
                );
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let local_app_data = PathBuf::from(local_app_data);
            for app_name in ["ChatGPT", "Codex"] {
                push(
                    local_app_data
                        .join("Programs")
                        .join(app_name)
                        .join("resources/codex.exe"),
                );
                push(local_app_data.join(app_name).join("resources/codex.exe"));
            }
        }
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            let program_files = PathBuf::from(program_files);
            for app_name in ["ChatGPT", "Codex"] {
                push(program_files.join(app_name).join("resources/codex.exe"));
            }
        }
    }

    #[cfg(target_os = "windows")]
    push(PathBuf::from("codex.exe"));
    #[cfg(not(target_os = "windows"))]
    push(PathBuf::from("codex"));

    candidates
}

fn load_bundled_codex_model_catalog_templates() -> Vec<Value> {
    for candidate in codex_cli_candidates() {
        let Ok(output) = Command::new(candidate)
            .args(["debug", "models", "--bundled"])
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&output.stdout) else {
            continue;
        };
        let templates = codex_model_catalog_templates_from_value(&value);
        if !templates.is_empty() {
            return templates;
        }
    }
    Vec::new()
}

fn merge_codex_model_catalog_templates(preferred: Vec<Value>, fallback: Vec<Value>) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::with_capacity(preferred.len() + fallback.len());
    for entry in preferred.into_iter().chain(fallback) {
        if let Some(slug) = entry.get("slug").and_then(Value::as_str) {
            if !seen.insert(slug.trim().to_ascii_lowercase()) {
                continue;
            }
        }
        merged.push(entry);
    }
    merged
}

fn load_codex_model_catalog_templates() -> Vec<Value> {
    static TEMPLATES: OnceLock<Vec<Value>> = OnceLock::new();
    TEMPLATES
        .get_or_init(|| {
            merge_codex_model_catalog_templates(
                load_bundled_codex_model_catalog_templates(),
                load_cached_codex_model_catalog_templates(),
            )
        })
        .clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexGpt56Variant {
    Generic,
    Sol,
    Terra,
    Luna,
}

fn codex_gpt_5_6_variant(model: &str) -> Option<CodexGpt56Variant> {
    let model = model.trim().to_ascii_lowercase();
    match model.as_str() {
        "gpt-5.6" => Some(CodexGpt56Variant::Generic),
        "gpt-5.6-sol" => Some(CodexGpt56Variant::Sol),
        "gpt-5.6-terra" => Some(CodexGpt56Variant::Terra),
        "gpt-5.6-luna" => Some(CodexGpt56Variant::Luna),
        _ if model.starts_with("gpt-5.6-") => Some(CodexGpt56Variant::Generic),
        _ => None,
    }
}

fn is_gpt_5_6_model(model: &str) -> bool {
    codex_gpt_5_6_variant(model).is_some()
}

fn codex_reasoning_level_specs(model: &str) -> Vec<(&'static str, &'static str)> {
    let mut levels = vec![
        ("low", "Fast responses with lighter reasoning"),
        (
            "medium",
            "Balances speed and reasoning depth for everyday tasks",
        ),
        ("high", "Greater reasoning depth for complex problems"),
        ("xhigh", "Extra high reasoning depth for complex problems"),
    ];
    if let Some(variant) = codex_gpt_5_6_variant(model) {
        levels.push(("max", "Maximum reasoning depth for the hardest problems"));
        if variant != CodexGpt56Variant::Luna {
            levels.push(("ultra", "Maximum reasoning with automatic task delegation"));
        }
    }
    levels
}

fn codex_reasoning_level_entry(effort: &str, description: &str) -> Value {
    json!({ "effort": effort, "description": description })
}

fn codex_reasoning_efforts_equal(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn codex_reasoning_effort_allowed(model: &str, effort: &str) -> bool {
    !matches!(codex_gpt_5_6_variant(model), Some(CodexGpt56Variant::Luna))
        || !effort.trim().eq_ignore_ascii_case("ultra")
}

fn apply_codex_model_reasoning_levels(model: &str, object: &mut Map<String, Value>) {
    if !is_gpt_5_6_model(model) {
        return;
    }

    let existing = object
        .get("supported_reasoning_levels")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let specs = codex_reasoning_level_specs(model);
    let mut levels = specs
        .iter()
        .map(|(effort, description)| {
            existing
                .iter()
                .find(|entry| {
                    entry
                        .get("effort")
                        .and_then(Value::as_str)
                        .is_some_and(|value| codex_reasoning_efforts_equal(value, effort))
                })
                .cloned()
                .unwrap_or_else(|| codex_reasoning_level_entry(effort, description))
        })
        .collect::<Vec<_>>();

    for entry in existing {
        let Some(effort) = entry.get("effort").and_then(Value::as_str) else {
            continue;
        };
        if !codex_reasoning_effort_allowed(model, effort) {
            continue;
        }
        if levels.iter().any(|existing| {
            existing
                .get("effort")
                .and_then(Value::as_str)
                .is_some_and(|value| codex_reasoning_efforts_equal(value, effort))
        }) {
            continue;
        }
        levels.push(entry);
    }

    object.insert(
        "supported_reasoning_levels".to_string(),
        Value::Array(levels),
    );
}

fn apply_codex_gpt_5_6_profile(model: &str, object: &mut Map<String, Value>) {
    let Some(variant) = codex_gpt_5_6_variant(model) else {
        return;
    };
    let (display_name, default_reasoning_level, multi_agent_version) = match variant {
        CodexGpt56Variant::Sol => ("GPT-5.6-Sol".to_string(), "low", "v2"),
        CodexGpt56Variant::Terra => ("GPT-5.6-Terra".to_string(), "medium", "v2"),
        CodexGpt56Variant::Luna => ("GPT-5.6-Luna".to_string(), "medium", "v1"),
        CodexGpt56Variant::Generic => {
            let display_name = if model.trim().eq_ignore_ascii_case("gpt-5.6") {
                "GPT-5.6".to_string()
            } else {
                model.trim().to_string()
            };
            (display_name, "medium", "v2")
        }
    };

    object.insert("display_name".to_string(), Value::String(display_name));
    object.insert(
        "default_reasoning_level".to_string(),
        Value::String(default_reasoning_level.to_string()),
    );
    object.insert(
        "multi_agent_version".to_string(),
        Value::String(multi_agent_version.to_string()),
    );
    object.insert("use_responses_lite".to_string(), Value::Bool(true));
    object.insert(
        "tool_mode".to_string(),
        Value::String("code_mode_only".to_string()),
    );
    object.insert(
        "supports_parallel_tool_calls".to_string(),
        Value::Bool(true),
    );
}

fn fallback_codex_model_catalog_entry(model: &str) -> Value {
    let supported_reasoning_levels = codex_reasoning_level_specs(model)
        .into_iter()
        .map(|(effort, description)| codex_reasoning_level_entry(effort, description))
        .collect::<Vec<_>>();
    json!({
        "slug": model,
        "display_name": model,
        "description": "Routed by XXSwitch.",
        "default_reasoning_level": "medium",
        "supported_reasoning_levels": supported_reasoning_levels,
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 1,
        "additional_speed_tiers": [],
        "service_tiers": [],
        "availability_nux": null,
        "upgrade": null,
        "base_instructions": "You are Codex, a coding agent.",
        "model_messages": {
            "instructions_template": "You are Codex, a coding agent.\n\n{{ personality }}",
            "instructions_variables": {
                "personality_default": "",
                "personality_friendly": "",
                "personality_pragmatic": ""
            }
        },
        "supports_reasoning_summaries": true,
        "default_reasoning_summary": "none",
        "support_verbosity": true,
        "default_verbosity": "low",
        "apply_patch_tool_type": "freeform",
        "web_search_tool_type": "text_and_image",
        "truncation_policy": { "mode": "tokens", "limit": 10000 },
        "supports_parallel_tool_calls": true,
        "supports_image_detail_original": true,
        "experimental_supported_tools": [],
        "input_modalities": ["text", "image"],
        "supports_search_tool": true,
        "use_responses_lite": false
    })
}

fn apply_codex_model_context_fields(object: &mut Map<String, Value>) {
    object.insert(
        "context_window".to_string(),
        Value::Number(CODEX_MODEL_CONTEXT_WINDOW.into()),
    );
    object.insert(
        "max_context_window".to_string(),
        Value::Number(CODEX_MODEL_CONTEXT_WINDOW.into()),
    );
    object.insert(
        "auto_compact_token_limit".to_string(),
        Value::Number(CODEX_MODEL_AUTO_COMPACT_TOKEN_LIMIT.into()),
    );
    object.insert(
        "effective_context_window_percent".to_string(),
        Value::Number(CODEX_MODEL_EFFECTIVE_CONTEXT_WINDOW_PERCENT.into()),
    );
}

fn codex_model_catalog_entry(model: &str, templates: &[Value]) -> Value {
    let exact_template = templates.iter().find(|entry| {
        entry
            .get("slug")
            .and_then(Value::as_str)
            .is_some_and(|slug| slug.eq_ignore_ascii_case(model))
    });
    let matched_exact_template = exact_template.is_some();
    let mut entry = exact_template
        .cloned()
        .or_else(|| {
            if !is_gpt_5_6_model(model) {
                return None;
            }
            templates
                .iter()
                .find(|entry| {
                    entry
                        .get("slug")
                        .and_then(Value::as_str)
                        .is_some_and(|slug| {
                            slug.eq_ignore_ascii_case("gpt-5.6-sol")
                                || slug.eq_ignore_ascii_case("gpt-5.6-terra")
                        })
                })
                .cloned()
        })
        .or_else(|| {
            templates
                .iter()
                .find(|entry| {
                    entry
                        .get("slug")
                        .and_then(Value::as_str)
                        .is_some_and(|slug| {
                            slug.eq_ignore_ascii_case("gpt-5.4")
                                || slug.eq_ignore_ascii_case("gpt-5.5")
                        })
                })
                .cloned()
        })
        .or_else(|| templates.first().cloned())
        .unwrap_or_else(|| fallback_codex_model_catalog_entry(model));

    if let Some(object) = entry.as_object_mut() {
        object.insert("slug".to_string(), Value::String(model.to_string()));
        let has_display_name = object
            .get("display_name")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        if !matched_exact_template || !has_display_name {
            object.insert("display_name".to_string(), Value::String(model.to_string()));
        }
        apply_codex_model_reasoning_levels(model, object);
        apply_codex_gpt_5_6_profile(model, object);
        apply_codex_model_context_fields(object);
        object.insert("visibility".to_string(), Value::String("list".to_string()));
        object.insert("supported_in_api".to_string(), Value::Bool(true));
    }

    entry
}

fn codex_models_catalog_value_with_templates(models: Vec<String>, templates: &[Value]) -> Value {
    let models = models
        .into_iter()
        .map(|model| codex_model_catalog_entry(&model, templates))
        .collect::<Vec<_>>();
    json!({ "models": models })
}

fn codex_models_catalog_value(models: Vec<String>) -> Value {
    let templates = load_codex_model_catalog_templates();
    codex_models_catalog_value_with_templates(models, &templates)
}

fn json_response(value: Value) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json; charset=utf-8")
        .body(Body::from(value.to_string()))
        .unwrap_or_else(|_| Response::new(Body::from("{}")))
}

fn models_response(models: Vec<String>) -> Response {
    json_response(openai_models_value(models))
}

fn codex_models_response(models: Vec<String>) -> Response {
    json_response(codex_models_catalog_value(models))
}

fn claude_models_response(models: Vec<Value>) -> Response {
    json_response(claude_models_value(models))
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get("authorization")?.to_str().ok()?.trim();
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
}

fn local_proxy_token(headers: &HeaderMap) -> Option<&str> {
    bearer_token(headers).or_else(|| headers.get("x-api-key")?.to_str().ok().map(str::trim))
}

fn claude_models_request(headers: &HeaderMap) -> bool {
    headers.contains_key("x-api-key")
        || headers.contains_key("anthropic-version")
        || headers.contains_key("anthropic-beta")
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

fn reasoning_effort_from_request_body(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/reasoning/effort")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .map(|effort| effort.trim().to_ascii_lowercase())
        .filter(|effort| !effort.is_empty())
}

fn reasoning_effort_requires_responses(effort: Option<&str>) -> bool {
    effort.is_some_and(|effort| {
        effort.eq_ignore_ascii_case("max") || effort.eq_ignore_ascii_case("ultra")
    })
}

fn provider_supports_reasoning_effort(
    provider: &ProviderConfig,
    reasoning_effort: Option<&str>,
) -> bool {
    !reasoning_effort_requires_responses(reasoning_effort)
        || provider.wire_api == ProviderWireApi::Responses
}

fn request_has_compaction_trigger(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| value.get("compaction_trigger").cloned())
        .is_some_and(|value| !value.is_null())
}

fn value_contains_compaction_item(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(value_contains_compaction_item),
        Value::Object(object) => {
            object.get("type").and_then(Value::as_str) == Some("compaction")
                || object.values().any(value_contains_compaction_item)
        }
        _ => false,
    }
}

fn remote_compaction_v2_audit_from_request_body(body: &[u8]) -> RemoteCompactionV2Audit {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return RemoteCompactionV2Audit::default();
    };
    RemoteCompactionV2Audit {
        trigger_received: value
            .get("compaction_trigger")
            .is_some_and(|value| !value.is_null()),
        compaction_item_reused: value
            .get("input")
            .is_some_and(value_contains_compaction_item),
        ..RemoteCompactionV2Audit::default()
    }
}

fn response_contains_compaction_item(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .is_some_and(|value| value_contains_compaction_item(&value))
}

fn sse_event_contains_compaction_item(event: &str) -> bool {
    event
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .filter(|data| !data.is_empty() && *data != "[DONE]")
        .any(|data| response_contains_compaction_item(data.as_bytes()))
}

fn decoded_proxy_request_body(headers: &HeaderMap, body: &[u8]) -> Result<Bytes, String> {
    let encodings = headers
        .get(CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_ascii_lowercase)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let is_zstd_frame = body.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]);
    if (encodings.is_empty() || encodings.iter().all(|encoding| encoding == "identity"))
        && !is_zstd_frame
    {
        return Ok(Bytes::copy_from_slice(body));
    }
    if !encodings.is_empty()
        && encodings
            .iter()
            .any(|encoding| encoding != "identity" && encoding != "zstd")
    {
        return Err(format!(
            "不支持的请求 Content-Encoding: {}",
            encodings.join(", ")
        ));
    }

    let decoder = zstd::stream::read::Decoder::new(Cursor::new(body))
        .map_err(|err| format!("无法解压 zstd 请求体: {err}"))?;
    let mut limited = decoder.take((MAX_PROXY_BODY_BYTES + 1) as u64);
    let mut decoded = Vec::new();
    limited
        .read_to_end(&mut decoded)
        .map_err(|err| format!("无法读取 zstd 请求体: {err}"))?;
    if decoded.len() > MAX_PROXY_BODY_BYTES {
        return Err(format!(
            "解压后的请求体超过 {} MiB 限制",
            MAX_PROXY_BODY_BYTES / 1024 / 1024
        ));
    }

    Ok(Bytes::from(decoded))
}

fn body_with_provider_overrides(
    body: &[u8],
    mapped_model: Option<&str>,
    service_tier: Option<&str>,
) -> Bytes {
    let mapped_model = mapped_model
        .map(str::trim)
        .filter(|model| !model.is_empty());
    let service_tier = service_tier.map(str::trim).filter(|tier| !tier.is_empty());
    if mapped_model.is_none() && service_tier.is_none() {
        return Bytes::copy_from_slice(body);
    }
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return Bytes::copy_from_slice(body);
    };
    let Some(object) = value.as_object_mut() else {
        return Bytes::copy_from_slice(body);
    };
    if let Some(mapped_model) = mapped_model {
        if !object.get("model").is_some_and(Value::is_string) {
            return Bytes::copy_from_slice(body);
        }
        object.insert("model".to_string(), Value::String(mapped_model.to_string()));
    }
    if let Some(service_tier) = service_tier {
        object.insert(
            "service_tier".to_string(),
            Value::String(service_tier.to_string()),
        );
    }
    serde_json::to_vec(&value)
        .map(Bytes::from)
        .unwrap_or_else(|_| Bytes::copy_from_slice(body))
}

fn prepare_upstream_request(
    provider: &ProviderConfig,
    path: &str,
    query: &str,
    body: &[u8],
    requested_model: &str,
) -> Result<PreparedUpstreamRequest, String> {
    let upstream_model = mapped_model_for_provider(provider, requested_model);
    let service_tier = provider.service_tier.as_str();
    if provider.wire_api == ProviderWireApi::ChatCompletions
        && path.trim_matches('/') == "responses"
    {
        let (body, tool_context) =
            responses_to_chat_request_body(body, upstream_model.as_deref(), Some(service_tier))?;
        Ok(PreparedUpstreamRequest {
            path: "chat/completions".to_string(),
            query: String::new(),
            body,
            adapter: ResponseAdapter::ChatCompletionsToResponses,
            upstream_model,
            tool_context,
        })
    } else {
        Ok(PreparedUpstreamRequest {
            path: path.to_string(),
            query: query.to_string(),
            body: body_with_provider_overrides(body, upstream_model.as_deref(), Some(service_tier)),
            adapter: ResponseAdapter::Passthrough,
            upstream_model,
            tool_context: CodexToolContext::default(),
        })
    }
}

fn json_string_content(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let mut text = String::new();
            for item in items {
                if let Some(value) = item.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(value);
                } else if let Some(value) = item.get("content").and_then(Value::as_str) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(value);
                } else if item.get("type").and_then(Value::as_str) == Some("input_text") {
                    if let Some(value) = item.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(value);
                    }
                } else {
                    return None;
                }
            }
            Some(text)
        }
        _ => None,
    }
}

const TOOL_SEARCH_PROXY_NAME: &str = "tool_search";
const CUSTOM_TOOL_INPUT_FIELD: &str = "input";
const CUSTOM_TOOL_DESCRIPTION_PREFIX: &str = "Original Codex custom tool definition:";

impl CodexToolContext {
    fn chat_tools(&self) -> &[Value] {
        &self.chat_tools
    }

    fn lookup_chat_name(&self, chat_name: &str) -> Option<&CodexToolSpec> {
        self.chat_name_to_spec.get(chat_name)
    }

    fn is_custom_tool_chat_name(&self, chat_name: &str) -> bool {
        self.lookup_chat_name(chat_name)
            .is_some_and(|spec| spec.kind == CodexToolKind::Custom)
    }

    fn chat_name_for_response_function(&self, name: &str, namespace: Option<&str>) -> String {
        if let Some(namespace) = namespace.filter(|value| !value.trim().is_empty()) {
            if let Some(chat_name) = self
                .namespace_name_to_chat_name
                .get(&(namespace.to_string(), name.to_string()))
            {
                return chat_name.clone();
            }
            return flatten_namespace_tool_name(namespace, name);
        }
        name.to_string()
    }

    fn add_chat_tool(&mut self, chat_name: String, spec: CodexToolSpec, chat_tool: Value) {
        if chat_name.trim().is_empty() || self.seen_chat_names.contains(&chat_name) {
            return;
        }
        self.seen_chat_names.insert(chat_name.clone());
        if let Some(namespace) = spec.namespace.as_ref() {
            self.namespace_name_to_chat_name
                .insert((namespace.clone(), spec.name.clone()), chat_name.clone());
        }
        self.chat_name_to_spec.insert(chat_name, spec);
        self.chat_tools.push(chat_tool);
    }

    fn add_function_tool(&mut self, tool: &Value, namespace: Option<&str>) {
        let Some(original_name) = responses_tool_name(tool) else {
            return;
        };
        let chat_name = namespace
            .map(|namespace| flatten_namespace_tool_name(namespace, &original_name))
            .unwrap_or_else(|| original_name.clone());
        let Some(chat_tool) = responses_function_tool_to_chat_tool(tool, &chat_name) else {
            return;
        };
        self.add_chat_tool(
            chat_name,
            CodexToolSpec {
                kind: if namespace.is_some() {
                    CodexToolKind::Namespace
                } else {
                    CodexToolKind::Function
                },
                name: original_name,
                namespace: namespace.map(str::to_string),
            },
            chat_tool,
        );
    }

    fn add_custom_tool(&mut self, tool: &Value) {
        let Some(name) = responses_tool_name(tool) else {
            return;
        };
        let description = format!(
            "{CUSTOM_TOOL_DESCRIPTION_PREFIX}\n```json\n{}\n```",
            compact_json(tool)
        );
        self.add_chat_tool(
            name.clone(),
            CodexToolSpec {
                kind: CodexToolKind::Custom,
                name: name.clone(),
                namespace: None,
            },
            json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": {
                        "type": "object",
                        "properties": {
                            CUSTOM_TOOL_INPUT_FIELD: {
                                "type": "string",
                                "description": "Raw string input for the original Codex custom tool."
                            }
                        },
                        "required": [CUSTOM_TOOL_INPUT_FIELD]
                    }
                }
            }),
        );
    }

    fn add_tool_search_tool(&mut self) {
        self.add_chat_tool(
            TOOL_SEARCH_PROXY_NAME.to_string(),
            CodexToolSpec {
                kind: CodexToolKind::ToolSearch,
                name: TOOL_SEARCH_PROXY_NAME.to_string(),
                namespace: None,
            },
            json!({
                "type": "function",
                "function": {
                    "name": TOOL_SEARCH_PROXY_NAME,
                    "description": "Search and load Codex tools, plugins, connectors, and MCP namespaces.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "limit": { "type": "integer" }
                        },
                        "required": ["query"]
                    }
                }
            }),
        );
    }

    fn add_namespace_tool(&mut self, tool: &Value) {
        let Some(namespace) = tool.get("name").and_then(Value::as_str) else {
            return;
        };
        let Some(children) = tool
            .get("tools")
            .or_else(|| tool.get("children"))
            .and_then(Value::as_array)
        else {
            return;
        };
        for child in children {
            if child.get("type").and_then(Value::as_str) == Some("function") {
                self.add_function_tool(child, Some(namespace));
            }
        }
    }

    fn add_response_tool(&mut self, tool: &Value) {
        match tool {
            Value::String(name) => self.add_custom_tool(&json!({
                "type": "custom",
                "name": name
            })),
            Value::Object(_) => match tool.get("type").and_then(Value::as_str) {
                Some("function") => self.add_function_tool(tool, None),
                Some("custom") => self.add_custom_tool(tool),
                Some("tool_search") => self.add_tool_search_tool(),
                Some("namespace") => self.add_namespace_tool(tool),
                _ => {}
            },
            _ => {}
        }
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn short_stable_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:08x}", hash as u32)
}

fn flatten_namespace_tool_name(namespace: &str, name: &str) -> String {
    let full_name = format!("{namespace}__{name}");
    if full_name.len() <= 64 {
        return full_name;
    }
    let suffix = format!("__{}", short_stable_hash(&full_name));
    let prefix_len = 64_usize.saturating_sub(suffix.len());
    let mut prefix = String::new();
    for ch in full_name.chars() {
        if prefix.len() + ch.len_utf8() > prefix_len {
            break;
        }
        prefix.push(ch);
    }
    format!("{prefix}{suffix}")
}

fn responses_tool_name(tool: &Value) -> Option<String> {
    tool.get("function")
        .and_then(|function| function.get("name"))
        .or_else(|| tool.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn responses_function_tool_to_chat_tool(tool: &Value, chat_name: &str) -> Option<Value> {
    if tool.get("type").and_then(Value::as_str) != Some("function") {
        return None;
    }
    if let Some(function) = tool.get("function") {
        let mut function = function.clone();
        if let Some(object) = function.as_object_mut() {
            object.insert("name".to_string(), Value::String(chat_name.to_string()));
            if let Some(strict) = tool.get("strict") {
                object.entry("strict".to_string()).or_insert(strict.clone());
            }
        }
        return Some(json!({ "type": "function", "function": function }));
    }
    let mut function = json!({
        "name": chat_name,
        "description": tool.get("description").cloned().unwrap_or(Value::Null),
        "parameters": tool.get("parameters").cloned().unwrap_or_else(|| json!({ "type": "object", "properties": {} }))
    });
    if let Some(strict) = tool.get("strict") {
        function["strict"] = strict.clone();
    }
    Some(json!({ "type": "function", "function": function }))
}

fn collect_tool_search_output_tools(value: &Value, context: &mut CodexToolContext) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_tool_search_output_tools(item, context);
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("tool_search_output") {
                if let Some(tools) = object.get("tools").and_then(Value::as_array) {
                    for tool in tools {
                        context.add_response_tool(tool);
                    }
                }
            }
            for value in object.values() {
                collect_tool_search_output_tools(value, context);
            }
        }
        _ => {}
    }
}

fn build_codex_tool_context_from_request(value: &Value) -> CodexToolContext {
    let mut context = CodexToolContext::default();
    if let Some(tools) = value.get("tools").and_then(Value::as_array) {
        for tool in tools {
            context.add_response_tool(tool);
        }
    }
    if let Some(input) = value.get("input") {
        collect_tool_search_output_tools(input, &mut context);
    }
    context
}

fn canonical_tool_arguments(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(value) => compact_json(value),
        None => "{}".to_string(),
    }
}

fn responses_custom_tool_call_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
    let input = item
        .get(CUSTOM_TOOL_INPUT_FIELD)
        .cloned()
        .unwrap_or(Value::Null);
    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": compact_json(&json!({ CUSTOM_TOOL_INPUT_FIELD: input }))
        }
    })
}

fn responses_tool_search_call_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": TOOL_SEARCH_PROXY_NAME,
            "arguments": canonical_tool_arguments(item.get("arguments"))
        }
    })
}

fn response_tool_output_text(item: &Value) -> String {
    item.get("output")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| item.get("output").map(compact_json))
        .unwrap_or_default()
}

fn responses_reasoning_item_text(item: &Value) -> Option<String> {
    reasoning_content_from_value(item)
        .map(str::to_string)
        .or_else(|| {
            item.get("text")
                .and_then(Value::as_str)
                .filter(|content| !content.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            item.get("content")
                .and_then(json_string_content)
                .filter(|content| !content.is_empty())
        })
        .or_else(|| {
            item.get("summary")
                .and_then(json_string_content)
                .filter(|content| !content.is_empty())
        })
        .or_else(|| {
            item.get("encrypted_content")
                .and_then(Value::as_str)
                .and_then(local_reasoning_from_encrypted_content)
        })
        .filter(|content| !content.is_empty())
}

fn responses_role_to_chat_role(role: &str) -> &str {
    match role {
        "developer" => "system",
        other => other,
    }
}

fn responses_input_item_type(item: &Value) -> &str {
    item.get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
}

fn is_responses_tool_call_item_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "function_call" | "custom_tool_call" | "tool_search_call"
    )
}

fn responses_input_tool_call_to_chat_tool_call(
    item: &Value,
    tool_context: &CodexToolContext,
) -> Result<Value, String> {
    match responses_input_item_type(item) {
        "function_call" => {
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "function_call 缺少 name".to_string())?;
            let namespace = item.get("namespace").and_then(Value::as_str);
            let chat_name = tool_context.chat_name_for_response_function(name, namespace);
            let arguments = canonical_tool_arguments(item.get("arguments"));
            Ok(json!({
                "id": call_id,
                "type": "function",
                "function": {
                    "name": chat_name,
                    "arguments": arguments
                }
            }))
        }
        "custom_tool_call" => Ok(responses_custom_tool_call_to_chat_tool_call(item)),
        "tool_search_call" => Ok(responses_tool_search_call_to_chat_tool_call(item)),
        other => Err(format!("Chat Completions 适配不支持 input 类型: {other}")),
    }
}

fn append_chat_tool_call(message: &mut Value, tool_call: Value) {
    if let Some(tool_calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) {
        tool_calls.push(tool_call);
    }
}

fn responses_input_item_to_chat_message(
    item: &Value,
    tool_context: &CodexToolContext,
) -> Result<Value, String> {
    let item_type = responses_input_item_type(item);
    match item_type {
        "message" => {
            let role = item
                .get("role")
                .and_then(Value::as_str)
                .map(responses_role_to_chat_role)
                .unwrap_or("user")
                .to_string();
            let content = item
                .get("content")
                .and_then(json_string_content)
                .or_else(|| item.get("text").and_then(Value::as_str).map(str::to_string))
                .ok_or_else(|| "Chat Completions 适配暂只支持文本 input".to_string())?;
            Ok(json!({ "role": role, "content": content }))
        }
        "function_call" | "custom_tool_call" | "tool_search_call" => {
            let tool_call = responses_input_tool_call_to_chat_tool_call(item, tool_context)?;
            let mut message = json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [tool_call]
            });
            attach_reasoning_content(&mut message, reasoning_content_from_value(item));
            Ok(message)
        }
        "function_call_output" => {
            let call_id = item
                .get("call_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "function_call_output 缺少 call_id".to_string())?;
            Ok(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": response_tool_output_text(item)
            }))
        }
        "custom_tool_call_output" | "tool_search_output" => {
            let call_id = item
                .get("call_id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{item_type} 缺少 call_id"))?;
            Ok(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": response_tool_output_text(item)
            }))
        }
        "reasoning" => Ok(json!({ "role": "assistant", "content": "" })),
        other => Err(format!("Chat Completions 适配不支持 input 类型: {other}")),
    }
}

fn chat_model_requires_reasoning_content_fallback(model: &str) -> bool {
    model.to_ascii_lowercase().contains("deepseek")
}

fn responses_input_to_chat_messages(
    value: &Value,
    tool_context: &CodexToolContext,
    missing_reasoning_fallback: bool,
) -> Result<Vec<Value>, String> {
    match value {
        Value::String(text) => Ok(vec![json!({ "role": "user", "content": text })]),
        Value::Array(items) => {
            let mut messages = Vec::new();
            let mut pending_tool_call_message: Option<Value> = None;
            let mut pending_reasoning_content: Option<String> = None;

            for item in items {
                let item_type = responses_input_item_type(item);
                if item_type == "reasoning" {
                    if let Some(reasoning_content) = responses_reasoning_item_text(item) {
                        pending_reasoning_content = Some(reasoning_content);
                    }
                    continue;
                }

                if is_responses_tool_call_item_type(item_type) {
                    let tool_call =
                        responses_input_tool_call_to_chat_tool_call(item, tool_context)?;
                    let item_reasoning_content =
                        reasoning_content_from_value(item).map(str::to_string);
                    if pending_tool_call_message.is_none() {
                        let mut message = json!({
                            "role": "assistant",
                            "content": null,
                            "tool_calls": []
                        });
                        let reasoning_content = item_reasoning_content
                            .as_deref()
                            .or(pending_reasoning_content.as_deref())
                            .or_else(|| {
                                missing_reasoning_fallback
                                    .then_some(MISSING_REASONING_CONTENT_FALLBACK)
                            });
                        attach_reasoning_content(&mut message, reasoning_content);
                        pending_tool_call_message = Some(message);
                    } else if let Some(reasoning_content) = item_reasoning_content.as_deref() {
                        if let Some(message) = pending_tool_call_message.as_mut() {
                            attach_reasoning_content(message, Some(reasoning_content));
                        }
                    }
                    if let Some(message) = pending_tool_call_message.as_mut() {
                        append_chat_tool_call(message, tool_call);
                    }
                    pending_reasoning_content = None;
                    continue;
                }

                if let Some(message) = pending_tool_call_message.take() {
                    messages.push(message);
                }
                messages.push(responses_input_item_to_chat_message(item, tool_context)?);
            }

            if let Some(message) = pending_tool_call_message {
                messages.push(message);
            }
            Ok(messages)
        }
        _ => Err("Chat Completions 适配暂只支持文本 input".to_string()),
    }
}

fn responses_tool_choice_to_chat(
    value: Option<&Value>,
    tool_context: &CodexToolContext,
) -> Option<Value> {
    let value = value?;
    if value.is_string() {
        return Some(value.clone());
    }
    match value.get("type").and_then(Value::as_str) {
        Some("function") => {
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let namespace = value.get("namespace").and_then(Value::as_str);
            Some(json!({
                "type": "function",
                "function": {
                    "name": tool_context.chat_name_for_response_function(name, namespace)
                }
            }))
        }
        Some("custom") => Some(json!({
            "type": "function",
            "function": {
                "name": value.get("name").cloned().unwrap_or(Value::Null)
            }
        })),
        Some("tool_search") => Some(json!({
            "type": "function",
            "function": {
                "name": TOOL_SEARCH_PROXY_NAME
            }
        })),
        _ => Some(value.clone()),
    }
}

fn responses_to_chat_request_body(
    body: &[u8],
    mapped_model: Option<&str>,
    service_tier: Option<&str>,
) -> Result<(Bytes, CodexToolContext), String> {
    let mut value = serde_json::from_slice::<Value>(body)
        .map_err(|err| format!("Responses 请求体不是有效 JSON: {err}"))?;
    let tool_context = build_codex_tool_context_from_request(&value);
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Responses 请求体必须是 JSON 对象".to_string())?;

    if object.contains_key("previous_response_id") {
        return Err("Chat Completions 适配不支持 previous_response_id".to_string());
    }

    if let Some(model) = mapped_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        object.insert("model".to_string(), Value::String(model.to_string()));
    }
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| "Responses 请求缺少 model".to_string())?
        .to_string();
    let missing_reasoning_fallback = chat_model_requires_reasoning_content_fallback(&model);

    let input = object
        .get("input")
        .ok_or_else(|| "Responses 请求缺少 input".to_string())?;
    let mut messages = Vec::new();
    if let Some(instructions) = object.get("instructions").and_then(Value::as_str) {
        if !instructions.trim().is_empty() {
            messages.push(json!({ "role": "system", "content": instructions }));
        }
    }
    messages.extend(responses_input_to_chat_messages(
        input,
        &tool_context,
        missing_reasoning_fallback,
    )?);

    let mut chat = Map::new();
    chat.insert("model".to_string(), Value::String(model));
    chat.insert("messages".to_string(), Value::Array(messages));
    for (from, to) in [
        ("max_output_tokens", "max_tokens"),
        ("temperature", "temperature"),
        ("top_p", "top_p"),
        ("stop", "stop"),
        ("stream", "stream"),
        ("presence_penalty", "presence_penalty"),
        ("frequency_penalty", "frequency_penalty"),
        ("seed", "seed"),
        ("service_tier", "service_tier"),
    ] {
        if let Some(value) = object.get(from) {
            chat.insert(to.to_string(), value.clone());
        }
    }
    if let Some(service_tier) = service_tier.map(str::trim).filter(|tier| !tier.is_empty()) {
        chat.insert(
            "service_tier".to_string(),
            Value::String(service_tier.to_string()),
        );
    }
    if object
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        chat.insert(
            "stream_options".to_string(),
            json!({ "include_usage": true }),
        );
    }
    if !tool_context.chat_tools().is_empty() {
        chat.insert(
            "tools".to_string(),
            Value::Array(tool_context.chat_tools().to_vec()),
        );
    }
    if let Some(tool_choice) =
        responses_tool_choice_to_chat(object.get("tool_choice"), &tool_context)
    {
        chat.insert("tool_choice".to_string(), tool_choice);
    }
    if let Some(value) = object.get("parallel_tool_calls") {
        chat.insert("parallel_tool_calls".to_string(), value.clone());
    }
    if !chat
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty())
    {
        chat.remove("tool_choice");
        chat.remove("parallel_tool_calls");
    }

    serde_json::to_vec(&Value::Object(chat))
        .map(|bytes| (Bytes::from(bytes), tool_context))
        .map_err(|err| format!("无法生成 Chat Completions 请求: {err}"))
}

fn chat_usage_to_responses_usage(usage: Option<&Value>) -> Value {
    let usage = usage.cloned().unwrap_or_else(|| json!({}));
    let input_tokens = scalar_number_at_path(&usage, &["prompt_tokens"]).unwrap_or_default() as i64;
    let output_tokens =
        scalar_number_at_path(&usage, &["completion_tokens"]).unwrap_or_default() as i64;
    let total_tokens = scalar_number_at_path(&usage, &["total_tokens"])
        .map(|value| value as i64)
        .unwrap_or_else(|| input_tokens.saturating_add(output_tokens));
    let cached_tokens = scalar_number_at_path(&usage, &["prompt_tokens_details", "cached_tokens"])
        .or_else(|| scalar_number_at_path(&usage, &["prompt_cache_hit_tokens"]))
        .unwrap_or_default() as i64;
    let reasoning_tokens =
        scalar_number_at_path(&usage, &["completion_tokens_details", "reasoning_tokens"])
            .unwrap_or_default() as i64;
    json!({
        "input_tokens": input_tokens,
        "input_tokens_details": { "cached_tokens": cached_tokens },
        "output_tokens": output_tokens,
        "output_tokens_details": { "reasoning_tokens": reasoning_tokens },
        "total_tokens": total_tokens
    })
}

fn custom_tool_input_from_chat_arguments(arguments: &str) -> String {
    if arguments.trim().is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(object)) => object
            .get(CUSTOM_TOOL_INPUT_FIELD)
            .and_then(Value::as_str)
            .unwrap_or(arguments)
            .to_string(),
        _ => arguments.to_string(),
    }
}

fn parse_tool_arguments_object(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str::<Value>(arguments)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({ "query": arguments }))
}

fn response_tool_call_item_id(
    call_id: &str,
    chat_name: &str,
    context: &CodexToolContext,
) -> String {
    match context.lookup_chat_name(chat_name).map(|spec| &spec.kind) {
        Some(CodexToolKind::Custom) => format!("ctc_{call_id}"),
        Some(CodexToolKind::ToolSearch) => format!("tsc_{call_id}"),
        _ => format!("fc_{call_id}"),
    }
}

fn response_tool_call_item_from_chat_name(
    item_id: &str,
    status: &str,
    call_id: &str,
    chat_name: &str,
    arguments: &str,
    context: &CodexToolContext,
) -> Value {
    match context.lookup_chat_name(chat_name) {
        Some(spec) if spec.kind == CodexToolKind::Custom => json!({
            "id": item_id,
            "type": "custom_tool_call",
            "status": status,
            "call_id": call_id,
            "name": spec.name,
            "input": custom_tool_input_from_chat_arguments(arguments)
        }),
        Some(spec) if spec.kind == CodexToolKind::ToolSearch => json!({
            "id": item_id,
            "type": "tool_search_call",
            "status": status,
            "call_id": call_id,
            "execution": "client",
            "arguments": parse_tool_arguments_object(arguments)
        }),
        Some(spec) => {
            let mut item = json!({
                "id": item_id,
                "type": "function_call",
                "call_id": call_id,
                "name": spec.name,
                "arguments": arguments,
                "status": status
            });
            if let Some(namespace) = spec.namespace.as_ref() {
                item["namespace"] = Value::String(namespace.clone());
            }
            item
        }
        None => json!({
            "id": item_id,
            "type": "function_call",
            "call_id": call_id,
            "name": chat_name,
            "arguments": arguments,
            "status": status
        }),
    }
}

fn response_message_item(output_text: &str, status: &str) -> Value {
    json!({
        "id": "msg_0",
        "type": "message",
        "role": "assistant",
        "status": status,
        "content": [{ "type": "output_text", "annotations": [], "text": output_text }]
    })
}

fn response_reasoning_item(reasoning_content: &str) -> Value {
    json!({
        "id": "rs_0",
        "type": "reasoning",
        "summary": [],
        "content": null,
        "encrypted_content": local_reasoning_encrypted_content(reasoning_content)
    })
}

fn chat_message_to_responses_output(message: &Value, context: &CodexToolContext) -> Vec<Value> {
    let mut output = Vec::new();
    let reasoning_content = reasoning_content_from_value(message);
    if let Some(reasoning_content) = reasoning_content {
        output.push(response_reasoning_item(reasoning_content));
    }
    if let Some(content) = message.get("content").and_then(Value::as_str) {
        if !content.is_empty() {
            output.push(response_message_item(content, "completed"));
        }
    }
    if let Some(Value::Array(tool_calls)) = message.get("tool_calls") {
        for call in tool_calls {
            if call.get("type").and_then(Value::as_str) != Some("function") {
                continue;
            }
            let function = call.get("function").unwrap_or(&Value::Null);
            let chat_name = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let call_id = call.get("id").and_then(Value::as_str).unwrap_or("call_0");
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let item_id = response_tool_call_item_id(call_id, chat_name, context);
            let mut response_item = response_tool_call_item_from_chat_name(
                &item_id,
                "completed",
                call_id,
                chat_name,
                arguments,
                context,
            );
            attach_reasoning_content(&mut response_item, reasoning_content);
            output.push(response_item);
        }
    }
    output
}

fn chat_completion_to_responses_value(value: &Value, context: &CodexToolContext) -> Value {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_chatcmpl");
    let created_at = value
        .get("created")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let model = value.get("model").cloned().unwrap_or(Value::Null);
    let message = value
        .pointer("/choices/0/message")
        .cloned()
        .unwrap_or_else(|| json!({ "role": "assistant", "content": "" }));
    let output = chat_message_to_responses_output(&message, context);
    let output_text = output
        .iter()
        .flat_map(|item| {
            item.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");

    json!({
        "id": id,
        "object": "response",
        "created_at": created_at,
        "status": "completed",
        "model": model,
        "output": output,
        "output_text": output_text,
        "usage": chat_usage_to_responses_usage(value.get("usage"))
    })
}

fn chat_completion_to_responses_bytes(
    bytes: &[u8],
    context: &CodexToolContext,
) -> Result<Bytes, String> {
    let value = serde_json::from_slice::<Value>(bytes)
        .map_err(|err| format!("Chat Completions 响应不是有效 JSON: {err}"))?;
    serde_json::to_vec(&chat_completion_to_responses_value(&value, context))
        .map(Bytes::from)
        .map_err(|err| format!("无法生成 Responses 响应: {err}"))
}

fn sse_event(event: &str, data: Value) -> String {
    format!("event: {event}\ndata: {}\n\n", data)
}

fn token_usage_to_responses_usage(usage: &TokenUsage) -> Value {
    json!({
        "input_tokens": usage.input_tokens,
        "input_tokens_details": { "cached_tokens": usage.cached_input_tokens },
        "output_tokens": usage.output_tokens,
        "output_tokens_details": { "reasoning_tokens": usage.reasoning_output_tokens },
        "total_tokens": usage.total_tokens
    })
}

fn chat_stream_response_completed_event(
    sequence_number: u64,
    response_id: &str,
    created_at: i64,
    model: &str,
    output_text: &str,
    completed_output: &[(usize, Value)],
    usage: &TokenUsage,
) -> Bytes {
    let mut output = Vec::new();
    if !output_text.is_empty() {
        output.push((0, response_message_item(output_text, "completed")));
    }
    output.extend(completed_output.iter().cloned());
    output.sort_by_key(|(index, _)| *index);
    let output = output.into_iter().map(|(_, item)| item).collect::<Vec<_>>();
    Bytes::from(sse_event(
        "response.completed",
        json!({
            "type": "response.completed",
            "sequence_number": sequence_number,
            "response": {
                "id": response_id,
                "object": "response",
                "created_at": created_at,
                "status": "completed",
                "error": null,
                "incomplete_details": null,
                "instructions": null,
                "max_output_tokens": null,
                "model": model,
                "usage": token_usage_to_responses_usage(usage),
                "output": output,
                "tools": []
            }
        }),
    ))
}

fn chat_stream_output_item_done_event(sequence_number: u64, output_text: &str) -> Bytes {
    Bytes::from(sse_event(
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "sequence_number": sequence_number,
            "output_index": 0,
            "item": {
                "id": "msg_0",
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{ "type": "output_text", "annotations": [], "text": output_text }]
            }
        }),
    ))
}

fn chat_tool_call_delta_string(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| value.map(compact_json))
        .unwrap_or_default()
}

fn push_chat_tool_call_delta(
    out: &mut Vec<Bytes>,
    sequence_number: &mut u64,
    tool_context: &CodexToolContext,
    tool_calls: &mut BTreeMap<usize, ChatToolCallState>,
    next_output_index: &mut usize,
    tool_call: &Value,
) {
    let chat_index = tool_call
        .get("index")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or_else(|| tool_calls.len());
    let id_delta = tool_call
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let function = tool_call.get("function").unwrap_or(&Value::Null);
    let name_delta = function
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let args_delta = chat_tool_call_delta_string(function.get("arguments"));

    let state = tool_calls.entry(chat_index).or_default();
    if let Some(id) = id_delta.filter(|value| !value.is_empty()) {
        state.call_id = id;
    }
    if let Some(name) = name_delta.filter(|value| !value.is_empty()) {
        state.name = name;
    }
    if !args_delta.is_empty() {
        state.arguments.push_str(&args_delta);
    }

    if !state.added && !state.call_id.is_empty() && !state.name.is_empty() {
        let assigned = *next_output_index;
        *next_output_index += 1;
        state.output_index = Some(assigned);
        state.item_id = response_tool_call_item_id(&state.call_id, &state.name, tool_context);
        state.added = true;
        let item = response_tool_call_item_from_chat_name(
            &state.item_id,
            "in_progress",
            &state.call_id,
            &state.name,
            "",
            tool_context,
        );
        *sequence_number += 1;
        out.push(Bytes::from(sse_event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "sequence_number": *sequence_number,
                "output_index": assigned,
                "item": item
            }),
        )));
    }

    if state.added && !args_delta.is_empty() && !tool_context.is_custom_tool_chat_name(&state.name)
    {
        *sequence_number += 1;
        out.push(Bytes::from(sse_event(
            "response.function_call_arguments.delta",
            json!({
                "type": "response.function_call_arguments.delta",
                "sequence_number": *sequence_number,
                "item_id": state.item_id,
                "output_index": state.output_index.unwrap_or(0),
                "delta": args_delta
            }),
        )));
    }
}

fn finalize_stream_tool_calls(
    out: &mut Vec<Bytes>,
    sequence_number: &mut u64,
    tool_context: &CodexToolContext,
    tool_calls: &mut BTreeMap<usize, ChatToolCallState>,
    next_output_index: &mut usize,
    completed_output: &mut Vec<(usize, Value)>,
    reasoning_content: Option<&str>,
) {
    if let Some(reasoning_content) = reasoning_content.filter(|content| !content.is_empty()) {
        if !tool_calls.is_empty() {
            let output_index = *next_output_index;
            *next_output_index += 1;
            let item = response_reasoning_item(reasoning_content);
            *sequence_number += 1;
            out.push(Bytes::from(sse_event(
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "sequence_number": *sequence_number,
                    "output_index": output_index,
                    "item": item
                }),
            )));
            *sequence_number += 1;
            out.push(Bytes::from(sse_event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "sequence_number": *sequence_number,
                    "output_index": output_index,
                    "item": item
                }),
            )));
            completed_output.push((output_index, item));
        }
    }
    for (index, state) in tool_calls.iter_mut() {
        if state.done {
            continue;
        }
        if !state.added {
            if state.call_id.is_empty() {
                state.call_id = format!("call_{index}");
            }
            if state.name.is_empty() {
                state.name = "unknown_tool".to_string();
            }
            let assigned = *next_output_index;
            *next_output_index += 1;
            state.output_index = Some(assigned);
            state.item_id = response_tool_call_item_id(&state.call_id, &state.name, tool_context);
            state.added = true;
            *sequence_number += 1;
            out.push(Bytes::from(sse_event(
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "sequence_number": *sequence_number,
                    "output_index": state.output_index.unwrap_or(0),
                    "item": response_tool_call_item_from_chat_name(
                        &state.item_id,
                        "in_progress",
                        &state.call_id,
                        &state.name,
                        "",
                        tool_context
                    )
                }),
            )));
        }
        let output_index = state.output_index.unwrap_or(0);
        let mut item = response_tool_call_item_from_chat_name(
            &state.item_id,
            "completed",
            &state.call_id,
            &state.name,
            &state.arguments,
            tool_context,
        );
        attach_reasoning_content(&mut item, reasoning_content);
        if tool_context.is_custom_tool_chat_name(&state.name) {
            let input = custom_tool_input_from_chat_arguments(&state.arguments);
            if !input.is_empty() {
                *sequence_number += 1;
                out.push(Bytes::from(sse_event(
                    "response.custom_tool_call_input.delta",
                    json!({
                        "type": "response.custom_tool_call_input.delta",
                        "sequence_number": *sequence_number,
                        "item_id": state.item_id,
                        "output_index": output_index,
                        "delta": input
                    }),
                )));
            }
            *sequence_number += 1;
            out.push(Bytes::from(sse_event(
                "response.custom_tool_call_input.done",
                json!({
                    "type": "response.custom_tool_call_input.done",
                    "sequence_number": *sequence_number,
                    "item_id": state.item_id,
                    "output_index": output_index,
                    "input": custom_tool_input_from_chat_arguments(&state.arguments)
                }),
            )));
        } else {
            *sequence_number += 1;
            out.push(Bytes::from(sse_event(
                "response.function_call_arguments.done",
                json!({
                    "type": "response.function_call_arguments.done",
                    "sequence_number": *sequence_number,
                    "item_id": state.item_id,
                    "output_index": output_index,
                    "arguments": state.arguments
                }),
            )));
        }
        *sequence_number += 1;
        out.push(Bytes::from(sse_event(
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "sequence_number": *sequence_number,
                "output_index": output_index,
                "item": item
            }),
        )));
        completed_output.push((output_index, item));
        state.done = true;
    }
}

fn chat_stream_events_to_responses(
    buffer: &mut String,
    bytes: &[u8],
    response_id: &mut String,
    created_at: &mut i64,
    model: &mut String,
    output_text: &mut String,
    reasoning_content: &mut String,
    output_index: &mut usize,
    next_output_index: &mut usize,
    tool_context: &CodexToolContext,
    tool_calls: &mut BTreeMap<usize, ChatToolCallState>,
    completed_output: &mut Vec<(usize, Value)>,
    sequence_number: &mut u64,
    started: &mut bool,
    text_done: &mut bool,
    completed: &mut bool,
    usage_seen: &mut bool,
    usage: &mut TokenUsage,
) -> Vec<Bytes> {
    buffer.push_str(&String::from_utf8_lossy(bytes));
    let mut out = Vec::new();
    while let Some((index, delimiter_len)) = next_sse_event_boundary(buffer) {
        let event = buffer[..index].to_string();
        buffer.drain(..index + delimiter_len);
        for line in event.lines() {
            let line = line.trim();
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if response_id == "resp_chatcmpl" {
                if let Some(id) = value.get("id").and_then(Value::as_str) {
                    *response_id = id.to_string();
                }
            }
            if *created_at == 0 {
                if let Some(created) = value.get("created").and_then(Value::as_i64) {
                    *created_at = created;
                }
            }
            if model.is_empty() {
                if let Some(model_value) = value.get("model").and_then(Value::as_str) {
                    *model = model_value.to_string();
                }
            }
            if !*started {
                *started = true;
                *sequence_number += 1;
                out.push(Bytes::from(sse_event(
                    "response.created",
                    json!({
                        "type": "response.created",
                        "sequence_number": *sequence_number,
                        "response": {
                            "id": response_id,
                            "object": "response",
                            "created_at": *created_at,
                            "status": "in_progress",
                            "model": model,
                            "output": []
                        }
                    }),
                )));
                *sequence_number += 1;
                out.push(Bytes::from(sse_event(
                    "response.output_item.added",
                    json!({
                        "type": "response.output_item.added",
                        "sequence_number": *sequence_number,
                        "output_index": 0,
                        "item": {
                            "id": "msg_0",
                            "type": "message",
                            "role": "assistant",
                            "status": "in_progress",
                            "content": []
                        }
                    }),
                )));
            }
            let choice = value
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|choices| choices.first());
            if let Some(delta) = choice.and_then(|choice| choice.get("delta")) {
                if let Some(text) = reasoning_content_from_value(delta) {
                    reasoning_content.push_str(text);
                }
                if let Some(text) = delta.get("content").and_then(Value::as_str) {
                    if !text.is_empty() {
                        output_text.push_str(text);
                        *sequence_number += 1;
                        out.push(Bytes::from(sse_event(
                            "response.output_text.delta",
                            json!({
                                "type": "response.output_text.delta",
                                "sequence_number": *sequence_number,
                                "item_id": "msg_0",
                                "output_index": 0,
                                "content_index": 0,
                                "delta": text
                            }),
                        )));
                        *output_index += 1;
                    }
                }
                if let Some(tool_call_deltas) = delta.get("tool_calls").and_then(Value::as_array) {
                    for tool_call in tool_call_deltas {
                        push_chat_tool_call_delta(
                            &mut out,
                            sequence_number,
                            tool_context,
                            tool_calls,
                            next_output_index,
                            tool_call,
                        );
                    }
                }
            }
            if let Some(usage_value) = value.get("usage") {
                let next = usage_from_value(&chat_usage_to_responses_usage(Some(usage_value)));
                if !usage_is_zero(&next) {
                    *usage = next;
                    *usage_seen = true;
                }
            }
            let finish_reason = choice
                .and_then(|choice| choice.get("finish_reason"))
                .and_then(Value::as_str);
            if finish_reason.is_some() && !*text_done {
                *text_done = true;
                *sequence_number += 1;
                out.push(Bytes::from(sse_event(
                    "response.output_text.done",
                    json!({
                        "type": "response.output_text.done",
                        "sequence_number": *sequence_number,
                        "item_id": "msg_0",
                        "output_index": 0,
                        "content_index": 0,
                        "text": output_text.clone()
                    }),
                )));
                *sequence_number += 1;
                out.push(chat_stream_output_item_done_event(
                    *sequence_number,
                    output_text,
                ));
                finalize_stream_tool_calls(
                    &mut out,
                    sequence_number,
                    tool_context,
                    tool_calls,
                    next_output_index,
                    completed_output,
                    Some(reasoning_content.as_str()),
                );
            }
            if *text_done && *usage_seen && !*completed {
                *completed = true;
                *sequence_number += 1;
                out.push(chat_stream_response_completed_event(
                    *sequence_number,
                    response_id,
                    *created_at,
                    model,
                    output_text,
                    completed_output,
                    usage,
                ));
            }
        }
    }
    out
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

fn ingest_sse_chunk(buffer: &mut String, usage: &mut TokenUsage, bytes: &[u8]) -> bool {
    buffer.push_str(&String::from_utf8_lossy(bytes));
    let mut compaction_response_received = false;
    while let Some((index, delimiter_len)) = next_sse_event_boundary(buffer) {
        let event = buffer[..index].to_string();
        buffer.drain(..index + delimiter_len);
        compaction_response_received |= sse_event_contains_compaction_item(&event);
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
    compaction_response_received
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
    let compaction_response_received = ingest_sse_chunk(&mut buffer, &mut usage, bytes);
    state.sse_buffer = buffer;
    state.usage = usage;
    if compaction_response_received {
        state
            .pending
            .remote_compaction_v2
            .compaction_response_received = true;
        state
            .pending
            .remote_compaction_v2
            .compaction_response_forwarded = true;
    }
}

fn route_stream_finish_usage(state: &mut RouteStreamState) {
    let mut buffer = std::mem::take(&mut state.sse_buffer);
    let mut usage = std::mem::take(&mut state.usage);
    let compaction_response_received = sse_event_contains_compaction_item(&buffer);
    finish_sse_usage(&mut buffer, &mut usage);
    state.sse_buffer = buffer;
    state.usage = usage;
    if compaction_response_received {
        state
            .pending
            .remote_compaction_v2
            .compaction_response_received = true;
        state
            .pending
            .remote_compaction_v2
            .compaction_response_forwarded = true;
    }
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
    let status = if pending.error.is_some() || !status_success {
        "failed"
    } else {
        "success"
    };
    persist_route_log(pending, status, usage, first_byte_ms);
}

fn finish_cancelled_route_log(
    mut pending: PendingRouteLog,
    usage: TokenUsage,
    first_byte_ms: Option<u64>,
) {
    mark_pending_route_log_cancelled(&mut pending, first_byte_ms);
    persist_route_log(pending, "cancelled", usage, first_byte_ms);
}

fn mark_pending_route_log_cancelled(pending: &mut PendingRouteLog, first_byte_ms: Option<u64>) {
    let phase = if first_byte_ms.is_some() {
        "流式响应尚未结束"
    } else if pending.status_code.is_some() {
        "已收到响应头但尚未收到响应数据"
    } else {
        "尚未收到远端响应"
    };
    pending.error = Some(format!("客户端在完整响应前断开：{phase}"));
    pending.route_result = format!("已取消 · {phase}");
}

fn persist_route_log(
    pending: PendingRouteLog,
    status: &str,
    usage: TokenUsage,
    first_byte_ms: Option<u64>,
) {
    let log = build_finished_route_log(pending, status, usage, first_byte_ms);
    if !is_usage_route_log(&log) {
        return;
    }
    if let Err(err) = append_route_log(&log) {
        eprintln!("{err}");
    }
}

fn build_finished_route_log(
    pending: PendingRouteLog,
    status: &str,
    usage: TokenUsage,
    first_byte_ms: Option<u64>,
) -> RouteRequestLog {
    let total_ms = pending.start.elapsed().as_millis() as u64;
    let (day, hour) = timestamp_to_route_parts(pending.started_at_ms);
    RouteRequestLog {
        id: pending.id,
        started_at_ms: pending.started_at_ms,
        day,
        hour,
        method: pending.method,
        path: pending.path,
        model: pending.model,
        remote_compaction_v2: pending.remote_compaction_v2,
        upstream_model: pending.upstream_model,
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
        first_byte_ms,
        total_ms,
    }
}

fn build_pending_route_log(
    id: String,
    started_at_ms: i64,
    start: Instant,
    candidate: &UpstreamCandidate,
    method: &Method,
    path: &str,
    model: &str,
    remote_compaction_v2: RemoteCompactionV2Audit,
    upstream_model: Option<&str>,
    upstream_chain: &[String],
    status_code: Option<u16>,
    route_attempts: usize,
    error: Option<String>,
) -> PendingRouteLog {
    PendingRouteLog {
        id,
        started_at_ms,
        method: method.as_str().to_string(),
        path: format!("/v1/{path}"),
        model: model.to_string(),
        remote_compaction_v2,
        upstream_model: upstream_model.map(str::to_string),
        provider_id: candidate.provider.id.clone(),
        provider_name: candidate.provider.name.clone(),
        provider_order: candidate.route_order,
        upstream_chain: upstream_chain.to_vec(),
        status_code,
        route_result: if let Some(upstream_model) = upstream_model {
            let prefix = if route_attempts > 1 {
                format!("切换 {} 次", route_attempts - 1)
            } else if error.is_some() {
                "未完成".to_string()
            } else {
                "直连".to_string()
            };
            format!("{prefix} · {model} → {upstream_model}")
        } else if route_attempts > 1 {
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
                    record_provider_failure(
                        ProviderKind::Codex,
                        &self.pending.provider_id,
                        &self.pending.provider_name,
                        &self.pending.id,
                        self.pending.started_at_ms,
                        &self.pending.path,
                        &self.pending.model,
                        ProviderFailureKind::Stream,
                        self.pending.status_code,
                        format!("读取上游流失败: {err}"),
                    );
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
                    if self.status_success {
                        record_provider_success(ProviderKind::Codex, &self.pending.provider_id);
                    } else if matches!(self.pending.status_code, Some(401 | 403 | 429 | 500..=599))
                    {
                        record_provider_failure(
                            ProviderKind::Codex,
                            &self.pending.provider_id,
                            &self.pending.provider_name,
                            &self.pending.id,
                            self.pending.started_at_ms,
                            &self.pending.path,
                            &self.pending.model,
                            provider_failure_kind_for_status(self.pending.status_code),
                            self.pending.status_code,
                            self.pending
                                .error
                                .clone()
                                .unwrap_or_else(|| "上游流式响应失败".to_string()),
                        );
                    }
                }
                std::task::Poll::Ready(None)
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

impl Drop for RouteStreamState {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        route_stream_finish_usage(self);
        finish_cancelled_route_log(self.pending.clone(), self.usage.clone(), self.first_byte_ms);
    }
}

impl futures_util::Stream for ChatToResponsesStreamState {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        loop {
            match self.stream.poll_next_unpin(cx) {
                std::task::Poll::Ready(Some(Ok(bytes))) => {
                    if self.first_byte_ms.is_none() {
                        self.first_byte_ms = Some(self.pending.start.elapsed().as_millis() as u64);
                    }
                    let mut buffer = std::mem::take(&mut self.sse_buffer);
                    let mut response_id = std::mem::take(&mut self.response_id);
                    let mut created_at = self.created_at;
                    let mut model = std::mem::take(&mut self.model);
                    let mut output_text = std::mem::take(&mut self.output_text);
                    let mut reasoning_content = std::mem::take(&mut self.reasoning_content);
                    let mut output_index = self.output_index;
                    let mut next_output_index = self.next_output_index;
                    let tool_context = self.tool_context.clone();
                    let mut tool_calls = std::mem::take(&mut self.tool_calls);
                    let mut completed_output = std::mem::take(&mut self.completed_output);
                    let mut sequence_number = self.sequence_number;
                    let mut started = self.started;
                    let mut text_done = self.text_done;
                    let mut completed = self.completed;
                    let mut usage_seen = self.usage_seen;
                    let mut usage = std::mem::take(&mut self.usage);
                    let events = chat_stream_events_to_responses(
                        &mut buffer,
                        bytes.as_ref(),
                        &mut response_id,
                        &mut created_at,
                        &mut model,
                        &mut output_text,
                        &mut reasoning_content,
                        &mut output_index,
                        &mut next_output_index,
                        &tool_context,
                        &mut tool_calls,
                        &mut completed_output,
                        &mut sequence_number,
                        &mut started,
                        &mut text_done,
                        &mut completed,
                        &mut usage_seen,
                        &mut usage,
                    );
                    self.sse_buffer = buffer;
                    self.response_id = response_id;
                    self.created_at = created_at;
                    self.model = model;
                    self.output_text = output_text;
                    self.reasoning_content = reasoning_content;
                    self.output_index = output_index;
                    self.next_output_index = next_output_index;
                    self.tool_context = tool_context;
                    self.tool_calls = tool_calls;
                    self.completed_output = completed_output;
                    self.sequence_number = sequence_number;
                    self.started = started;
                    self.text_done = text_done;
                    self.completed = completed;
                    self.usage_seen = usage_seen;
                    self.usage = usage;
                    if events.is_empty() {
                        continue;
                    }
                    let mut joined = Vec::new();
                    for event in events {
                        joined.extend_from_slice(&event);
                    }
                    return std::task::Poll::Ready(Some(Ok(Bytes::from(joined))));
                }
                std::task::Poll::Ready(Some(Err(err))) => {
                    if !self.finished {
                        self.finished = true;
                        self.pending.error = Some(format!("读取上游流失败: {err}"));
                        record_provider_failure(
                            ProviderKind::Codex,
                            &self.pending.provider_id,
                            &self.pending.provider_name,
                            &self.pending.id,
                            self.pending.started_at_ms,
                            &self.pending.path,
                            &self.pending.model,
                            ProviderFailureKind::Stream,
                            self.pending.status_code,
                            format!("读取上游流失败: {err}"),
                        );
                        let pending = self.pending.clone();
                        let usage = self.usage.clone();
                        finish_route_log(pending, false, usage, self.first_byte_ms);
                    }
                    return std::task::Poll::Ready(Some(Err(std::io::Error::other(err))));
                }
                std::task::Poll::Ready(None) => {
                    if self.text_done && !self.completed {
                        self.completed = true;
                        self.sequence_number += 1;
                        let completed = chat_stream_response_completed_event(
                            self.sequence_number,
                            &self.response_id,
                            self.created_at,
                            &self.model,
                            &self.output_text,
                            &self.completed_output,
                            &self.usage,
                        );
                        return std::task::Poll::Ready(Some(Ok(completed)));
                    }
                    if !self.finished {
                        self.finished = true;
                        let pending = self.pending.clone();
                        let usage = self.usage.clone();
                        finish_route_log(pending, self.status_success, usage, self.first_byte_ms);
                        if self.status_success {
                            record_provider_success(ProviderKind::Codex, &self.pending.provider_id);
                        } else if matches!(
                            self.pending.status_code,
                            Some(401 | 403 | 429 | 500..=599)
                        ) {
                            record_provider_failure(
                                ProviderKind::Codex,
                                &self.pending.provider_id,
                                &self.pending.provider_name,
                                &self.pending.id,
                                self.pending.started_at_ms,
                                &self.pending.path,
                                &self.pending.model,
                                provider_failure_kind_for_status(self.pending.status_code),
                                self.pending.status_code,
                                self.pending
                                    .error
                                    .clone()
                                    .unwrap_or_else(|| "上游流式响应失败".to_string()),
                            );
                        }
                    }
                    return std::task::Poll::Ready(None);
                }
                std::task::Poll::Pending => return std::task::Poll::Pending,
            }
        }
    }
}

impl Drop for ChatToResponsesStreamState {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        if self.completed {
            finish_route_log(
                self.pending.clone(),
                self.status_success,
                self.usage.clone(),
                self.first_byte_ms,
            );
            if self.status_success {
                record_provider_success(ProviderKind::Codex, &self.pending.provider_id);
            } else if matches!(self.pending.status_code, Some(401 | 403 | 429 | 500..=599)) {
                record_provider_failure(
                    ProviderKind::Codex,
                    &self.pending.provider_id,
                    &self.pending.provider_name,
                    &self.pending.id,
                    self.pending.started_at_ms,
                    &self.pending.path,
                    &self.pending.model,
                    provider_failure_kind_for_status(self.pending.status_code),
                    self.pending.status_code,
                    self.pending
                        .error
                        .clone()
                        .unwrap_or_else(|| "上游流式响应失败".to_string()),
                );
            }
            return;
        }
        finish_cancelled_route_log(self.pending.clone(), self.usage.clone(), self.first_byte_ms);
    }
}

impl futures_util::Stream for ClaudeStreamState {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.stream.poll_next_unpin(cx) {
            std::task::Poll::Ready(Some(Ok(bytes))) => std::task::Poll::Ready(Some(Ok(bytes))),
            std::task::Poll::Ready(Some(Err(err))) => {
                if !self.finished {
                    self.finished = true;
                    record_provider_failure(
                        ProviderKind::Claude,
                        &self.provider_id,
                        &self.provider_name,
                        &self.request_id,
                        self.started_at_ms,
                        &self.path,
                        &self.model,
                        ProviderFailureKind::Stream,
                        self.status_code,
                        format!("读取上游流失败: {err}"),
                    );
                }
                std::task::Poll::Ready(Some(Err(std::io::Error::other(err))))
            }
            std::task::Poll::Ready(None) => {
                if !self.finished {
                    self.finished = true;
                    if self.status_success {
                        record_provider_success(ProviderKind::Claude, &self.provider_id);
                    } else if matches!(self.status_code, Some(401 | 403 | 429 | 500..=599)) {
                        record_provider_failure(
                            ProviderKind::Claude,
                            &self.provider_id,
                            &self.provider_name,
                            &self.request_id,
                            self.started_at_ms,
                            &self.path,
                            &self.model,
                            provider_failure_kind_for_status(self.status_code),
                            self.status_code,
                            self.status_code
                                .map(|status| format!("{} 返回 {}", self.provider_name, status))
                                .unwrap_or_else(|| "上游流式响应失败".to_string()),
                        );
                    }
                }
                std::task::Poll::Ready(None)
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

fn claude_upstream_path(path: &str) -> String {
    path.trim().trim_start_matches('/').to_string()
}

fn copy_upstream_headers(builder: &mut axum::http::response::Builder, headers: &HeaderMap) {
    if let Some(headers_mut) = builder.headers_mut() {
        for (name, value) in headers.iter() {
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
}

async fn send_upstream_request(
    request: reqwest::RequestBuilder,
    response_header_timeout_secs: u64,
) -> Result<reqwest::Response, String> {
    match tokio::time::timeout(
        Duration::from_secs(response_header_timeout_secs),
        request.send(),
    )
    .await
    {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!(
            "等待上游响应头超过 {response_header_timeout_secs} 秒"
        )),
    }
}

async fn proxy_claude_models(proxy_state: Arc<ProxyState>, headers: HeaderMap) -> Response {
    let (router, candidates) = match upstream_claude_candidates() {
        Ok(config) => config,
        Err(err) => return proxy_error(StatusCode::BAD_GATEWAY, err),
    };
    if local_proxy_token(&headers) != Some(router.local_token.trim()) {
        return proxy_error(StatusCode::UNAUTHORIZED, "本地路由 Token 无效");
    }
    let upstream_client = match proxy_state.upstream() {
        Ok(upstream) => upstream,
        Err(err) => return proxy_error(StatusCode::BAD_GATEWAY, err),
    };

    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    let mut last_error = String::new();
    let mut upstream_success = false;

    for candidate in candidates {
        let upstream_models =
            match fetch_claude_provider_model_values(&upstream_client.client, &candidate.provider)
                .await
            {
                Ok((models, _)) => {
                    upstream_success = true;
                    models
                }
                Err(err) => {
                    last_error = format!("读取 {} 模型失败: {err}", candidate.provider.name);
                    Vec::new()
                }
            };

        for model in claude_route_model_values(&candidate.provider, &upstream_models) {
            push_claude_model_value(&mut models, &mut seen, model);
        }
    }

    if models.is_empty() && !upstream_success && !last_error.is_empty() {
        return proxy_error(StatusCode::BAD_GATEWAY, last_error);
    }

    claude_models_response(models)
}

async fn proxy_claude_request(
    proxy_state: Arc<ProxyState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    path: String,
    body: Body,
) -> Response {
    let request_started_at_ms = current_epoch_ms().unwrap_or_default();
    let route_request_id = request_id(request_started_at_ms);
    let (router, candidates) = match upstream_claude_candidates() {
        Ok(config) => config,
        Err(err) => return proxy_error(StatusCode::BAD_GATEWAY, err),
    };
    if local_proxy_token(&headers) != Some(router.local_token.trim()) {
        return proxy_error(StatusCode::UNAUTHORIZED, "本地路由 Token 无效");
    }
    let upstream_client = match proxy_state.upstream() {
        Ok(upstream) => upstream,
        Err(err) => return proxy_error(StatusCode::BAD_GATEWAY, err),
    };

    let query = uri
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let body_bytes = match axum::body::to_bytes(body, MAX_PROXY_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => return proxy_error(StatusCode::BAD_REQUEST, format!("无法读取请求体: {err}")),
    };
    let body_bytes = match decoded_proxy_request_body(&headers, &body_bytes) {
        Ok(bytes) => bytes,
        Err(err) => return proxy_error(StatusCode::BAD_REQUEST, err),
    };
    let model = model_from_request_body(&body_bytes);
    let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(method) => method,
        Err(err) => return proxy_error(StatusCode::BAD_REQUEST, format!("请求方法无效: {err}")),
    };
    let candidates = candidates
        .iter()
        .filter(|candidate| claude_provider_accepts_model(&candidate.provider, &model))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        let auto_disabled_supports_model = !model.trim().is_empty()
            && model != "未知模型"
            && auto_disabled_claude_provider_supports_model(&model);
        return proxy_error(
            StatusCode::BAD_GATEWAY,
            if auto_disabled_supports_model {
                format!("支持模型 {model} 的 Claude 供应商今日已自动禁用")
            } else if model.trim().is_empty() || model == "未知模型" {
                "没有可用的 Claude 上游供应商".to_string()
            } else {
                format!("没有可用的 Claude 上游供应商支持模型: {model}")
            },
        );
    }

    let mut last_error = String::new();
    for (attempt_index, candidate) in candidates.iter().enumerate() {
        let upstream_model = mapped_model_for_claude_provider(&candidate.provider, &model);
        let prepared_body =
            body_with_provider_overrides(&body_bytes, upstream_model.as_deref(), None);
        let upstream_path = claude_upstream_path(&path);
        let upstream_url = join_url(&candidate.base_url, &upstream_path) + &query;
        let mut request = upstream_client
            .client
            .request(reqwest_method.clone(), upstream_url)
            .body(prepared_body);

        for (name, value) in headers.iter() {
            if name == AUTHORIZATION
                || name == HOST
                || name == CONTENT_ENCODING
                || name == CONTENT_LENGTH
                || name.as_str().eq_ignore_ascii_case("x-api-key")
                || is_hop_by_hop_header(name)
            {
                continue;
            }
            request = request.header(name.as_str(), value.as_bytes());
        }
        request = request.header("x-api-key", candidate.token.clone());

        let upstream = match send_upstream_request(
            request,
            upstream_client.response_header_timeout_secs,
        )
        .await
        {
            Ok(response) => response,
            Err(err) => {
                last_error = format!("转发到 {} 失败: {err}", candidate.provider.name);
                record_provider_failure(
                    ProviderKind::Claude,
                    &candidate.provider.id,
                    &candidate.provider.name,
                    &route_request_id,
                    request_started_at_ms,
                    &format!("/{path}"),
                    &model,
                    ProviderFailureKind::Network,
                    None,
                    last_error.clone(),
                );
                if attempt_index + 1 < candidates.len() {
                    continue;
                }
                return proxy_error(StatusCode::BAD_GATEWAY, last_error);
            }
        };

        let status =
            StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let should_retry =
            matches!(status.as_u16(), 429 | 500..=599) && attempt_index + 1 < candidates.len();
        if should_retry {
            last_error = format!("{} 返回 {}", candidate.provider.name, status.as_u16());
            record_provider_failure(
                ProviderKind::Claude,
                &candidate.provider.id,
                &candidate.provider.name,
                &route_request_id,
                request_started_at_ms,
                &format!("/{path}"),
                &model,
                if status.as_u16() == 429 {
                    ProviderFailureKind::RateLimit
                } else {
                    ProviderFailureKind::UpstreamServer
                },
                Some(status.as_u16()),
                last_error.clone(),
            );
            continue;
        }

        let content_type = upstream
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_lowercase();
        let response_headers = upstream.headers().clone();
        let mut builder = Response::builder().status(status);
        copy_upstream_headers(&mut builder, &response_headers);
        if content_type.contains("text/event-stream") {
            let stream = ClaudeStreamState {
                stream: upstream.bytes_stream().boxed(),
                provider_id: candidate.provider.id.clone(),
                provider_name: candidate.provider.name.clone(),
                request_id: route_request_id.clone(),
                started_at_ms: request_started_at_ms,
                path: format!("/{path}"),
                model: model.clone(),
                status_success: status.is_success(),
                status_code: Some(status.as_u16()),
                finished: false,
            };
            return builder
                .body(Body::from_stream(stream))
                .unwrap_or_else(|_| proxy_error(StatusCode::BAD_GATEWAY, "无法创建上游响应"));
        }

        let bytes = match upstream.bytes().await {
            Ok(bytes) => bytes,
            Err(err) => {
                record_provider_failure(
                    ProviderKind::Claude,
                    &candidate.provider.id,
                    &candidate.provider.name,
                    &route_request_id,
                    request_started_at_ms,
                    &format!("/{path}"),
                    &model,
                    ProviderFailureKind::ResponseRead,
                    Some(status.as_u16()),
                    format!("读取上游响应失败: {err}"),
                );
                return proxy_error(StatusCode::BAD_GATEWAY, format!("读取上游响应失败: {err}"));
            }
        };
        if status.is_success() {
            record_provider_success(ProviderKind::Claude, &candidate.provider.id);
        } else if matches!(status.as_u16(), 401 | 403 | 429 | 500..=599) {
            record_provider_failure(
                ProviderKind::Claude,
                &candidate.provider.id,
                &candidate.provider.name,
                &route_request_id,
                request_started_at_ms,
                &format!("/{path}"),
                &model,
                provider_failure_kind_for_status(Some(status.as_u16())),
                Some(status.as_u16()),
                format!("{} 返回 {}", candidate.provider.name, status.as_u16()),
            );
        }
        return builder
            .body(Body::from(bytes))
            .unwrap_or_else(|_| proxy_error(StatusCode::BAD_GATEWAY, "无法创建上游响应"));
    }

    proxy_error(
        StatusCode::BAD_GATEWAY,
        if last_error.is_empty() {
            "没有可用的 Claude 上游供应商".to_string()
        } else {
            last_error
        },
    )
}

async fn proxy_request(
    AxumState(proxy_state): AxumState<Arc<ProxyState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
    body: Body,
) -> Response {
    if method == Method::GET
        && path.trim_matches('/') == "models"
        && claude_models_request(&headers)
    {
        return proxy_claude_models(proxy_state, headers).await;
    }
    if path.trim_matches('/') == "messages" {
        return proxy_claude_request(proxy_state, method, uri, headers, path, body).await;
    }

    let request_started = Instant::now();
    let request_started_at_ms = current_epoch_ms().unwrap_or_default();
    let route_request_id = request_id(request_started_at_ms);
    let (router, candidates) = match upstream_candidates() {
        Ok(config) => config,
        Err(err) => return proxy_error(StatusCode::BAD_GATEWAY, err),
    };
    if bearer_token(&headers) != Some(router.local_token.trim()) {
        return proxy_error(StatusCode::UNAUTHORIZED, "本地路由 Token 无效");
    }
    let upstream_client = match proxy_state.upstream() {
        Ok(upstream) => upstream,
        Err(err) => return proxy_error(StatusCode::BAD_GATEWAY, err),
    };
    if method == Method::GET && path.trim_matches('/') == "models" {
        let models = configured_route_models(&candidates);
        if !models.is_empty() {
            return if codex_model_catalog_requested(uri.query()) {
                codex_models_response(models)
            } else {
                models_response(models)
            };
        }
    }

    let query = uri
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let body_bytes = match axum::body::to_bytes(body, MAX_PROXY_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => return proxy_error(StatusCode::BAD_REQUEST, format!("无法读取请求体: {err}")),
    };
    let body_bytes = match decoded_proxy_request_body(&headers, &body_bytes) {
        Ok(bytes) => bytes,
        Err(err) => return proxy_error(StatusCode::BAD_REQUEST, err),
    };
    let model = model_from_request_body(&body_bytes);
    let reasoning_effort = reasoning_effort_from_request_body(&body_bytes);
    let remote_compaction_v2_audit = remote_compaction_v2_audit_from_request_body(&body_bytes);

    let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(method) => method,
        Err(err) => return proxy_error(StatusCode::BAD_REQUEST, format!("请求方法无效: {err}")),
    };

    let model_candidates = candidates
        .iter()
        .filter(|candidate| provider_accepts_model(&candidate.provider, &model))
        .collect::<Vec<_>>();
    let candidates = model_candidates
        .iter()
        .copied()
        .filter(|candidate| {
            provider_supports_reasoning_effort(&candidate.provider, reasoning_effort.as_deref())
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        if !model_candidates.is_empty()
            && reasoning_effort_requires_responses(reasoning_effort.as_deref())
        {
            return proxy_error(
                StatusCode::BAD_REQUEST,
                format!(
                    "推理强度 {} 需要使用 Responses API 上游",
                    reasoning_effort.as_deref().unwrap_or_default()
                ),
            );
        }
        let auto_disabled_supports_model = !model.trim().is_empty()
            && model != "未知模型"
            && auto_disabled_codex_provider_supports_model(&model);
        return proxy_error(
            StatusCode::BAD_GATEWAY,
            if auto_disabled_supports_model {
                format!("支持模型 {model} 的供应商今日已自动禁用")
            } else if model.trim().is_empty() || model == "未知模型" {
                "没有可用的上游供应商".to_string()
            } else {
                format!("没有可用的上游供应商支持模型: {model}")
            },
        );
    }

    let mut upstream_chain = Vec::new();
    let mut last_error = String::new();
    let mut cancellation_guard = PendingRouteLogGuard::default();
    for (attempt_index, candidate) in candidates.iter().enumerate() {
        cancellation_guard.disarm();
        upstream_chain.push(candidate.provider.name.clone());
        let prepared =
            match prepare_upstream_request(&candidate.provider, &path, &query, &body_bytes, &model)
            {
                Ok(prepared) => prepared,
                Err(err) => {
                    return proxy_error(StatusCode::BAD_REQUEST, err);
                }
            };
        let mut candidate_compaction_audit = remote_compaction_v2_audit.clone();
        if candidate_compaction_audit.trigger_received {
            candidate_compaction_audit.trigger_forwarded =
                request_has_compaction_trigger(&prepared.body);
        }
        let upstream_url = join_url(&candidate.base_url, &prepared.path) + &prepared.query;
        let mut request = upstream_client
            .client
            .request(reqwest_method.clone(), upstream_url)
            .body(prepared.body.clone());

        for (name, value) in headers.iter() {
            if name == AUTHORIZATION
                || name == HOST
                || name == CONTENT_ENCODING
                || name == CONTENT_LENGTH
                || is_hop_by_hop_header(name)
            {
                continue;
            }
            request = request.header(name.as_str(), value.as_bytes());
        }
        request = request.header(AUTHORIZATION, format!("Bearer {}", candidate.token));

        cancellation_guard.arm(build_pending_route_log(
            route_request_id.clone(),
            request_started_at_ms,
            request_started,
            candidate,
            &method,
            &path,
            &model,
            candidate_compaction_audit.clone(),
            prepared.upstream_model.as_deref(),
            &upstream_chain,
            None,
            attempt_index + 1,
            None,
        ));

        let upstream = match send_upstream_request(
            request,
            upstream_client.response_header_timeout_secs,
        )
        .await
        {
            Ok(response) => response,
            Err(err) => {
                last_error = format!("转发到 {} 失败: {err}", candidate.provider.name);
                record_provider_failure(
                    ProviderKind::Codex,
                    &candidate.provider.id,
                    &candidate.provider.name,
                    &route_request_id,
                    request_started_at_ms,
                    &format!("/v1/{path}"),
                    &model,
                    ProviderFailureKind::Network,
                    None,
                    last_error.clone(),
                );
                if attempt_index + 1 < candidates.len() {
                    continue;
                }
                cancellation_guard.arm(build_pending_route_log(
                    route_request_id.clone(),
                    request_started_at_ms,
                    request_started,
                    candidate,
                    &method,
                    &path,
                    &model,
                    candidate_compaction_audit,
                    prepared.upstream_model.as_deref(),
                    &upstream_chain,
                    None,
                    attempt_index + 1,
                    Some(last_error.clone()),
                ));
                let pending = cancellation_guard.take();
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
            record_provider_failure(
                ProviderKind::Codex,
                &candidate.provider.id,
                &candidate.provider.name,
                &route_request_id,
                request_started_at_ms,
                &format!("/v1/{path}"),
                &model,
                if status.as_u16() == 429 {
                    ProviderFailureKind::RateLimit
                } else {
                    ProviderFailureKind::UpstreamServer
                },
                Some(status.as_u16()),
                last_error.clone(),
            );
            continue;
        }

        cancellation_guard.arm(build_pending_route_log(
            route_request_id.clone(),
            request_started_at_ms,
            request_started,
            candidate,
            &method,
            &path,
            &model,
            candidate_compaction_audit,
            prepared.upstream_model.as_deref(),
            &upstream_chain,
            Some(status.as_u16()),
            attempt_index + 1,
            if status.is_success() {
                None
            } else {
                Some(format!("上游返回 {}", status.as_u16()))
            },
        ));
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
            let stream = upstream.bytes_stream().boxed();
            let pending = cancellation_guard.take();
            return match prepared.adapter {
                ResponseAdapter::Passthrough => {
                    let stream = RouteStreamState {
                        stream,
                        pending,
                        status_success,
                        first_byte_ms: None,
                        sse_buffer: String::new(),
                        usage: TokenUsage::default(),
                        finished: false,
                    };
                    builder.body(Body::from_stream(stream)).unwrap_or_else(|_| {
                        proxy_error(StatusCode::BAD_GATEWAY, "无法创建上游响应")
                    })
                }
                ResponseAdapter::ChatCompletionsToResponses => {
                    let stream = ChatToResponsesStreamState {
                        stream,
                        pending,
                        status_success,
                        first_byte_ms: None,
                        sse_buffer: String::new(),
                        response_id: "resp_chatcmpl".to_string(),
                        created_at: 0,
                        model: String::new(),
                        output_text: String::new(),
                        reasoning_content: String::new(),
                        output_index: 0,
                        next_output_index: 1,
                        tool_context: prepared.tool_context.clone(),
                        tool_calls: BTreeMap::new(),
                        completed_output: Vec::new(),
                        sequence_number: 0,
                        started: false,
                        text_done: false,
                        completed: false,
                        usage_seen: false,
                        usage: TokenUsage::default(),
                        finished: false,
                    };
                    builder
                        .header("content-type", "text/event-stream")
                        .body(Body::from_stream(stream))
                        .unwrap_or_else(|_| {
                            proxy_error(StatusCode::BAD_GATEWAY, "无法创建上游响应")
                        })
                }
            };
        }

        let first_byte_ms = Some(cancellation_guard.pending().start.elapsed().as_millis() as u64);
        let bytes = match upstream.bytes().await {
            Ok(bytes) => bytes,
            Err(err) => {
                let mut failed = cancellation_guard.take();
                failed.error = Some(format!("读取上游响应失败: {err}"));
                record_provider_failure(
                    ProviderKind::Codex,
                    &candidate.provider.id,
                    &candidate.provider.name,
                    &route_request_id,
                    request_started_at_ms,
                    &format!("/v1/{path}"),
                    &model,
                    ProviderFailureKind::ResponseRead,
                    Some(status.as_u16()),
                    format!("读取上游响应失败: {err}"),
                );
                finish_route_log(failed, false, TokenUsage::default(), first_byte_ms);
                return proxy_error(StatusCode::BAD_GATEWAY, format!("读取上游响应失败: {err}"));
            }
        };
        let mut pending = cancellation_guard.take();
        let response_is_passthrough = matches!(&prepared.adapter, ResponseAdapter::Passthrough);
        let response_bytes = match prepared.adapter {
            ResponseAdapter::Passthrough => bytes,
            ResponseAdapter::ChatCompletionsToResponses => {
                match chat_completion_to_responses_bytes(&bytes, &prepared.tool_context) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        let mut failed = pending;
                        failed.error = Some(err.clone());
                        record_provider_failure(
                            ProviderKind::Codex,
                            &candidate.provider.id,
                            &candidate.provider.name,
                            &route_request_id,
                            request_started_at_ms,
                            &format!("/v1/{path}"),
                            &model,
                            ProviderFailureKind::Protocol,
                            Some(status.as_u16()),
                            err.clone(),
                        );
                        finish_route_log(failed, false, TokenUsage::default(), first_byte_ms);
                        return proxy_error(StatusCode::BAD_GATEWAY, err);
                    }
                }
            }
        };
        if response_contains_compaction_item(&response_bytes) {
            pending.remote_compaction_v2.compaction_response_received = true;
            pending.remote_compaction_v2.compaction_response_forwarded = response_is_passthrough;
        }
        let usage = usage_from_response_text(&String::from_utf8_lossy(&response_bytes));
        finish_route_log(pending, status_success, usage, first_byte_ms);
        if status_success {
            record_provider_success(ProviderKind::Codex, &candidate.provider.id);
        } else if matches!(status.as_u16(), 401 | 403 | 429 | 500..=599) {
            record_provider_failure(
                ProviderKind::Codex,
                &candidate.provider.id,
                &candidate.provider.name,
                &route_request_id,
                request_started_at_ms,
                &format!("/v1/{path}"),
                &model,
                if matches!(status.as_u16(), 401 | 403) {
                    ProviderFailureKind::Authentication
                } else if status.as_u16() == 429 {
                    ProviderFailureKind::RateLimit
                } else {
                    ProviderFailureKind::UpstreamServer
                },
                Some(status.as_u16()),
                format!("{} 返回 {}", candidate.provider.name, status.as_u16()),
            );
        }

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
            .body(Body::from(response_bytes))
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

fn build_upstream_client(config: &RouterConfig) -> Result<UpstreamClient, String> {
    validate_router_timeouts(config)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
        .read_timeout(Duration::from_secs(config.stream_idle_timeout_secs))
        .build()
        .map_err(|err| format!("无法创建上游 HTTP 客户端: {err}"))?;
    Ok(UpstreamClient {
        client,
        response_header_timeout_secs: config.response_header_timeout_secs,
    })
}

fn ensure_router(runtime: &RouterRuntime, config: &RouterConfig) -> Result<(), String> {
    if !config.enabled {
        stop_router(runtime);
        return Ok(());
    }

    let address = router_address(config);
    let timeouts = RouterTimeouts::from_config(config);
    if let Ok(mut handle) = runtime.handle.lock() {
        if let Some(handle) = handle.as_mut().filter(|handle| handle.address == address) {
            if handle.timeouts != timeouts {
                handle
                    .proxy_state
                    .replace_upstream(build_upstream_client(config)?)?;
                handle.timeouts = timeouts;
            }
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
        upstream: RwLock::new(build_upstream_client(config)?),
    });
    let app = Router::new()
        .route("/v1/{*path}", any(proxy_request))
        .fallback(proxy_not_found)
        .with_state(Arc::clone(&proxy_state));

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
        timeouts,
        proxy_state,
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
            if !is_usage_route_log(&log) {
                continue;
            }
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

fn append_provider_failure_event(event: &ProviderFailureEvent) -> Result<(), String> {
    fs::create_dir_all(manager_dir()?).map_err(|err| format!("无法创建管理目录: {err}"))?;
    let line =
        serde_json::to_string(event).map_err(|err| format!("无法序列化供应商失败事件: {err}"))?;
    let path = provider_failure_events_path()?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| format!("无法写入供应商失败事件 {}: {err}", path.display()))?;
    writeln!(file, "{line}").map_err(|err| format!("无法写入供应商失败事件: {err}"))
}

fn provider_failure_kind_for_status(status_code: Option<u16>) -> ProviderFailureKind {
    match status_code {
        Some(401 | 403) => ProviderFailureKind::Authentication,
        Some(429) => ProviderFailureKind::RateLimit,
        Some(500..=599) => ProviderFailureKind::UpstreamServer,
        _ => ProviderFailureKind::Protocol,
    }
}

fn record_provider_success(provider_kind: ProviderKind, provider_id: &str) {
    let lock = PROVIDER_HEALTH_LOCK.get_or_init(|| Mutex::new(()));
    let Ok(_guard) = lock.lock() else {
        return;
    };
    let Ok(mut state) = load_state_file() else {
        return;
    };
    let changed = match provider_kind {
        ProviderKind::Codex => state
            .providers
            .iter_mut()
            .find(|provider| provider.id == provider_id)
            .map(clear_provider_failure_state)
            .unwrap_or(false),
        ProviderKind::Claude => state
            .claude_providers
            .iter_mut()
            .find(|provider| provider.id == provider_id)
            .map(clear_claude_provider_failure_state)
            .unwrap_or(false),
    };
    if changed {
        let _ = save_state(&state);
    }
}

fn clear_provider_failure_state(provider: &mut ProviderConfig) -> bool {
    let changed = provider.consecutive_failure_count != 0
        || provider.auto_disabled_day.is_some()
        || provider.last_failure_reason.is_some()
        || provider.last_failure_at_ms.is_some()
        || provider.status == ProviderStatus::AutoDisabled;
    if changed {
        provider.status = ProviderStatus::Enabled;
        provider.enabled = true;
        provider.consecutive_failure_count = 0;
        provider.auto_disabled_day = None;
        provider.last_failure_reason = None;
        provider.last_failure_at_ms = None;
    }
    changed
}

fn clear_claude_provider_failure_state(provider: &mut ClaudeProviderConfig) -> bool {
    let changed = provider.consecutive_failure_count != 0
        || provider.auto_disabled_day.is_some()
        || provider.last_failure_reason.is_some()
        || provider.last_failure_at_ms.is_some()
        || provider.status == ProviderStatus::AutoDisabled;
    if changed {
        provider.status = ProviderStatus::Enabled;
        provider.enabled = true;
        provider.consecutive_failure_count = 0;
        provider.auto_disabled_day = None;
        provider.last_failure_reason = None;
        provider.last_failure_at_ms = None;
    }
    changed
}

#[allow(clippy::too_many_arguments)]
fn record_provider_failure(
    provider_kind: ProviderKind,
    provider_id: &str,
    provider_name: &str,
    route_request_id: &str,
    started_at_ms: i64,
    path: &str,
    model: &str,
    failure_kind: ProviderFailureKind,
    status_code: Option<u16>,
    error: String,
) {
    let lock = PROVIDER_HEALTH_LOCK.get_or_init(|| Mutex::new(()));
    let Ok(_guard) = lock.lock() else {
        return;
    };
    let (day, _) = timestamp_to_route_parts(started_at_ms);
    let mut consecutive_failure_count = 1;
    let mut auto_disabled = false;
    let mut counted = true;

    if let Ok(mut state) = load_state_file() {
        match provider_kind {
            ProviderKind::Codex => {
                if let Some(provider) = state
                    .providers
                    .iter_mut()
                    .find(|provider| provider.id == provider_id)
                {
                    if provider.status == ProviderStatus::Disabled {
                        counted = false;
                        consecutive_failure_count = provider.consecutive_failure_count;
                    } else {
                        provider.consecutive_failure_count =
                            provider.consecutive_failure_count.saturating_add(1);
                        provider.last_failure_reason = Some(error.clone());
                        provider.last_failure_at_ms = Some(started_at_ms);
                        if provider.consecutive_failure_count >= AUTO_DISABLE_FAILURE_THRESHOLD {
                            provider.status = ProviderStatus::AutoDisabled;
                            provider.enabled = false;
                            provider.auto_disabled_day = Some(day.clone());
                            auto_disabled = true;
                        }
                        consecutive_failure_count = provider.consecutive_failure_count;
                    }
                }
            }
            ProviderKind::Claude => {
                if let Some(provider) = state
                    .claude_providers
                    .iter_mut()
                    .find(|provider| provider.id == provider_id)
                {
                    if provider.status == ProviderStatus::Disabled {
                        counted = false;
                        consecutive_failure_count = provider.consecutive_failure_count;
                    } else {
                        provider.consecutive_failure_count =
                            provider.consecutive_failure_count.saturating_add(1);
                        provider.last_failure_reason = Some(error.clone());
                        provider.last_failure_at_ms = Some(started_at_ms);
                        if provider.consecutive_failure_count >= AUTO_DISABLE_FAILURE_THRESHOLD {
                            provider.status = ProviderStatus::AutoDisabled;
                            provider.enabled = false;
                            provider.auto_disabled_day = Some(day.clone());
                            auto_disabled = true;
                        }
                        consecutive_failure_count = provider.consecutive_failure_count;
                    }
                }
            }
        }
        let _ = save_state(&state);
    }

    let event = ProviderFailureEvent {
        id: request_id(current_epoch_ms().unwrap_or(started_at_ms)),
        request_id: route_request_id.to_string(),
        started_at_ms,
        day,
        provider_kind,
        provider_id: provider_id.to_string(),
        provider_name: provider_name.to_string(),
        model: model.to_string(),
        path: path.to_string(),
        failure_kind,
        status_code,
        error,
        counted,
        consecutive_failure_count,
        auto_disabled,
    };
    if let Err(err) = append_provider_failure_event(&event) {
        eprintln!("{err}");
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

fn add_route_log_usage(summary: &mut RouteUsageSummary, log: &RouteRequestLog) {
    summary.request_count += 1;
    summary.input_tokens += log.input_tokens;
    summary.uncached_input_tokens += log.uncached_input_tokens;
    summary.cached_input_tokens += log.cached_input_tokens;
    summary.output_tokens += log.output_tokens;
    summary.reasoning_output_tokens += log.reasoning_output_tokens;
    summary.total_tokens += log.total_tokens;
}

fn is_success_route_log(log: &RouteRequestLog) -> bool {
    log.status == "success"
}

fn is_usage_route_log(log: &RouteRequestLog) -> bool {
    matches!(
        (log.method.as_str(), log.path.as_str()),
        ("POST", "/v1/responses") | ("POST", "/v1/chat/completions") | ("POST", "/v1/completions")
    )
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
            });
        entry.request_count += 1;
        entry.input_tokens += log.input_tokens;
        entry.uncached_input_tokens += log.uncached_input_tokens;
        entry.cached_input_tokens += log.cached_input_tokens;
        entry.output_tokens += log.output_tokens;
        entry.total_tokens += log.total_tokens;
    }
    let mut rows = map.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| right.request_count.cmp(&left.request_count));
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
        .filter(|log| is_usage_route_log(log))
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
        available_providers: route_available_providers(&filtered),
        available_models: route_available_models(&filtered),
        available_days: route_available_days(&filtered),
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
        .filter(|log| is_success_route_log(log) && is_usage_route_log(log))
        .collect::<Vec<_>>();
    let now = Local::now();
    let today_key = format!("{:04}-{:02}-{:02}", now.year(), now.month(), now.day());
    let mut summary = RouteUsageSummary::default();
    let mut today = RouteUsageSummary::default();
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
        if !is_success_route_log(log) || !is_usage_route_log(log) {
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
            });
        entry.request_count += 1;
        entry.input_tokens += log.input_tokens;
        entry.uncached_input_tokens += log.uncached_input_tokens;
        entry.cached_input_tokens += log.cached_input_tokens;
        entry.output_tokens += log.output_tokens;
        entry.total_tokens += log.total_tokens;
    }

    let page_size = normalized_page_size(filter.page_size);
    let usage_filtered = filtered
        .iter()
        .copied()
        .filter(|log| is_usage_route_log(log))
        .collect::<Vec<_>>();
    let total_pages = usage_filtered.len().div_ceil(page_size).max(1);
    let page = filter.page.unwrap_or(1).clamp(1, total_pages);
    let start = (page - 1) * page_size;
    let details = usage_filtered
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
        total: usage_filtered.len(),
        page,
        page_size,
        total_pages,
        available_providers: route_available_providers(&logs),
        available_models: route_available_models(&logs),
        available_days: route_available_days(&logs),
    })
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

fn normalized_page_size(page_size: Option<usize>) -> usize {
    match page_size.unwrap_or(50) {
        0..=20 => 20,
        21..=50 => 50,
        51..=100 => 100,
        _ => 100,
    }
}

fn build_app_state(state: ManagerState, runtime: &RouterRuntime) -> Result<AppState, String> {
    let provider = active_provider(&state);
    let redacted_active_provider = provider.clone().map(redacted_provider);
    let redacted_active_claude_provider = state
        .claude_providers
        .iter()
        .find(|provider| provider.id == state.active_claude_provider_id)
        .cloned()
        .map(redacted_claude_provider);
    let desired = if state.clients.codex.enabled {
        router_patch_desired(&state.router)
    } else {
        provider
            .as_ref()
            .map(|provider| provider.config.clone())
            .unwrap_or_else(|| json!({}))
    };
    let (doc, marker_present, current_config_raw, current_config_exists) = read_current_toml()?;
    let current_json = toml_doc_to_json(&doc);
    let final_preview_toml = if state.clients.codex.enabled {
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
            let provider_desired = if state.clients.codex.enabled {
                desired.clone()
            } else {
                desired_config(&state, Some(provider))
            };
            let pending_changes =
                compute_diffs(&state, Some(provider), &current_json, &provider_desired).len();
            let balance_status = provider.balance_status.as_ref();
            let connection_status = provider.connection_status.as_ref();
            ProviderSummary {
                id: provider.id.clone(),
                name: provider.name.clone(),
                status: provider.status,
                enabled: provider.enabled,
                consecutive_failure_count: provider.consecutive_failure_count,
                auto_disabled_day: provider.auto_disabled_day.clone(),
                last_failure_reason: provider.last_failure_reason.clone(),
                last_failure_at_ms: provider.last_failure_at_ms,
                pending_changes,
                base_url: custom_provider_base_url(provider).unwrap_or_default(),
                provider_type: provider_type(provider),
                route_order: index + 1,
                balance_label: balance_status
                    .map(|status| status.label.clone())
                    .unwrap_or_else(|| "未配置".to_string()),
                balance_error: balance_status.and_then(|status| status.error.clone()),
                latency_ms: connection_status.and_then(|status| status.latency_ms),
                latency_label: connection_status
                    .and_then(|status| status.latency_ms)
                    .map(|latency_ms| format!("{latency_ms} ms"))
                    .unwrap_or_else(|| "-".to_string()),
                latency_error: connection_status.and_then(|status| status.error.clone()),
            }
        })
        .collect();

    let claude_providers = state
        .claude_providers
        .iter()
        .enumerate()
        .map(|(index, provider)| {
            let connection_status = provider.connection_status.as_ref();
            ProviderSummary {
                id: provider.id.clone(),
                name: provider.name.clone(),
                status: provider.status,
                enabled: provider.enabled,
                consecutive_failure_count: provider.consecutive_failure_count,
                auto_disabled_day: provider.auto_disabled_day.clone(),
                last_failure_reason: provider.last_failure_reason.clone(),
                last_failure_at_ms: provider.last_failure_at_ms,
                pending_changes: 0,
                base_url: provider.base_url.clone(),
                provider_type: "Claude".to_string(),
                route_order: index + 1,
                balance_label: "不适用".to_string(),
                balance_error: None,
                latency_ms: connection_status.and_then(|status| status.latency_ms),
                latency_label: connection_status
                    .and_then(|status| status.latency_ms)
                    .map(|latency_ms| format!("{latency_ms} ms"))
                    .unwrap_or_else(|| "-".to_string()),
                latency_error: connection_status.and_then(|status| status.error.clone()),
            }
        })
        .collect();

    Ok(AppState {
        app_version: env!("XXSWITCH_VERSION").to_string(),
        codex_config_path: codex_config_path()?.display().to_string(),
        claude_settings_path: claude_settings_path()?.display().to_string(),
        pi_models_path: pi_models_path()?.display().to_string(),
        manager_dir: manager_dir()?.display().to_string(),
        current_config_raw: redacted_toml_text(&current_config_raw),
        current_config_exists,
        active_provider_id: state.active_provider_id,
        active_claude_provider_id: state.active_claude_provider_id,
        base_template_name: state.base_template_name,
        base_toml: redacted_toml_text(&json_to_toml_text(&state.base)?),
        base: redacted_config_value(state.base),
        providers,
        claude_providers,
        active_provider_toml: redacted_active_provider
            .as_ref()
            .map(|provider| json_to_toml_text(&provider.config))
            .transpose()?
            .unwrap_or_default(),
        active_provider: redacted_active_provider,
        active_claude_provider: redacted_active_claude_provider,
        desired: redacted_config_value(desired.clone()),
        final_preview_toml: redacted_toml_text(&final_preview_toml),
        summary,
        diffs: redacted_diffs,
        marker_present,
        router: state.router.clone(),
        clients: state.clients.clone(),
        router_status: router_status(runtime, &state.router),
    })
}

#[tauri::command]
fn load_app_state(router_runtime: tauri::State<RouterRuntime>) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    if ensure_client_configs_applied(&mut state)? {
        save_state(&state)?;
    }
    build_app_state(state, &router_runtime)
}

#[tauri::command]
async fn check_for_update() -> Result<UpdateCheckInfo, String> {
    update_check_info(&fetch_latest_github_release().await?)
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<UpdateInstallResult, String> {
    let release = fetch_latest_github_release().await?;
    let info = update_check_info(&release)?;
    if !info.available {
        return Err("当前已经是最新版本".to_string());
    }
    let asset = update_asset_for_platform(&release.assets, update_platform())
        .ok_or_else(|| "该 Release 尚未包含当前系统可用的安装包".to_string())?;
    let path = download_update_asset(asset).await?;

    #[cfg(target_os = "windows")]
    {
        Command::new(&path)
            .spawn()
            .map_err(|err| format!("无法启动更新安装程序: {err}"))?;
        let app_for_exit = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            app_for_exit.exit(0);
        });
        return Ok(UpdateInstallResult {
            message: "已启动更新安装程序，XXSwitch 即将退出。".to_string(),
            manual_install: false,
        });
    }

    #[cfg(target_os = "macos")]
    {
        return install_macos_update_from_dmg(&app, &path, &info.latest_version);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = app;
        let _ = path;
        Err("当前系统暂不支持一键更新".to_string())
    }
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
        .ok_or_else(|| "供应商不存在".to_string())
}

#[tauri::command]
fn get_claude_provider(provider_id: String) -> Result<ClaudeProviderConfig, String> {
    load_state_file()?
        .claude_providers
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| "Claude 供应商不存在".to_string())
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
        let status = if enabled {
            ProviderStatus::Enabled
        } else {
            ProviderStatus::Disabled
        };
        provider.status = status;
        set_provider_status_fields(
            status,
            &mut provider.enabled,
            &mut provider.consecutive_failure_count,
            &mut provider.auto_disabled_day,
            &mut provider.last_failure_reason,
            &mut provider.last_failure_at_ms,
        );
    }
    if let Some(connection_test_model) = payload.connection_test_model {
        provider.connection_test_model = connection_test_model.trim().to_string();
    }
    if let Some(allowed_models) = payload.allowed_models {
        provider.allowed_models = normalize_model_names(allowed_models);
    }
    if let Some(wire_api) = payload.wire_api {
        provider.wire_api = wire_api;
    }
    if let Some(service_tier) = payload.service_tier {
        provider.service_tier = service_tier.trim().to_string();
    }
    if let Some(model_mappings) = payload.model_mappings {
        provider.model_mappings = normalize_model_mappings(model_mappings);
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
    ensure_client_configs_applied(&mut state)?;
    save_state(&state)?;
    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn save_claude_provider(
    payload: SaveClaudeProviderPayload,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    let provider = state
        .claude_providers
        .iter_mut()
        .find(|provider| provider.id == payload.provider_id)
        .ok_or_else(|| "Claude 供应商不存在".to_string())?;

    if let Some(name) = payload.provider_name.as_deref() {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("Claude 供应商名称不能为空".to_string());
        }
        provider.name = trimmed.to_string();
    }
    if let Some(enabled) = payload.enabled {
        let status = if enabled {
            ProviderStatus::Enabled
        } else {
            ProviderStatus::Disabled
        };
        provider.status = status;
        set_provider_status_fields(
            status,
            &mut provider.enabled,
            &mut provider.consecutive_failure_count,
            &mut provider.auto_disabled_day,
            &mut provider.last_failure_reason,
            &mut provider.last_failure_at_ms,
        );
    }
    if let Some(base_url) = payload.base_url {
        provider.base_url = base_url.trim().trim_end_matches('/').to_string();
    }
    if let Some(api_key) = payload.api_key {
        provider.api_key = api_key.trim().to_string();
    }
    if let Some(connection_test_model) = payload.connection_test_model {
        provider.connection_test_model = connection_test_model.trim().to_string();
    }
    if let Some(allowed_models) = payload.allowed_models {
        provider.allowed_models = normalize_model_names(allowed_models);
    }
    if let Some(model_mappings) = payload.model_mappings {
        provider.model_mappings = normalize_model_mappings(model_mappings);
    }

    save_state(&state)?;
    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn delete_provider(
    payload: DeleteProviderPayload,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    delete_provider_from_state(&mut state, &payload.provider_id)?;
    ensure_client_configs_applied(&mut state)?;
    save_state(&state)?;
    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn delete_claude_provider(
    payload: DeleteProviderPayload,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    delete_claude_provider_from_state(&mut state, &payload.provider_id)?;
    save_state(&state)?;
    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn reorder_providers(
    payload: ReorderProvidersPayload,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    if payload.provider_ids.len() != state.providers.len() {
        return Err("供应商顺序与当前列表不匹配".to_string());
    }

    let current_ids = state
        .providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect::<BTreeSet<_>>();
    let requested_ids = payload
        .provider_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if current_ids != requested_ids || requested_ids.len() != payload.provider_ids.len() {
        return Err("供应商顺序包含未知或重复项".to_string());
    }

    let mut providers_by_id = state
        .providers
        .into_iter()
        .map(|provider| (provider.id.clone(), provider))
        .collect::<BTreeMap<_, _>>();
    state.providers = payload
        .provider_ids
        .into_iter()
        .map(|provider_id| {
            providers_by_id
                .remove(&provider_id)
                .ok_or_else(|| "供应商不存在".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    save_state(&state)?;
    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn reorder_claude_providers(
    payload: ReorderProvidersPayload,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    if payload.provider_ids.len() != state.claude_providers.len() {
        return Err("Claude 供应商顺序与当前列表不匹配".to_string());
    }

    let current_ids = state
        .claude_providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect::<BTreeSet<_>>();
    let requested_ids = payload
        .provider_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if current_ids != requested_ids || requested_ids.len() != payload.provider_ids.len() {
        return Err("Claude 供应商顺序包含未知或重复项".to_string());
    }

    let mut providers_by_id = state
        .claude_providers
        .into_iter()
        .map(|provider| (provider.id.clone(), provider))
        .collect::<BTreeMap<_, _>>();
    state.claude_providers = payload
        .provider_ids
        .into_iter()
        .map(|provider_id| {
            providers_by_id
                .remove(&provider_id)
                .ok_or_else(|| "Claude 供应商不存在".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

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
    if let Some(allowed_models) = payload.allowed_models {
        provider.allowed_models = normalize_model_names(allowed_models);
    }
    if let Some(wire_api) = payload.wire_api {
        provider.wire_api = wire_api;
    }
    if let Some(service_tier) = payload.service_tier {
        provider.service_tier = service_tier.trim().to_string();
    }
    if let Some(model_mappings) = payload.model_mappings {
        provider.model_mappings = normalize_model_mappings(model_mappings);
    }
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
        status: ProviderStatus::Enabled,
        enabled: true,
        consecutive_failure_count: 0,
        auto_disabled_day: None,
        last_failure_reason: None,
        last_failure_at_ms: None,
        wire_api: ProviderWireApi::Responses,
        service_tier: String::new(),
        connection_test_model: String::new(),
        allowed_models: Vec::new(),
        model_mappings: Vec::new(),
        balance_query: BalanceQueryConfig::default(),
        balance_status: None,
        connection_status: None,
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
fn add_claude_provider(
    name: String,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Claude 供应商名称不能为空".to_string());
    }

    let id_base = trimmed
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let id_base = if id_base.is_empty() {
        "claude-provider".to_string()
    } else {
        id_base
    };

    let mut id = id_base.clone();
    let mut index = 2;
    while state
        .claude_providers
        .iter()
        .any(|provider| provider.id == id)
    {
        id = format!("{id_base}-{index}");
        index += 1;
    }

    state.active_claude_provider_id = id.clone();
    state.claude_providers.push(ClaudeProviderConfig {
        id: id.clone(),
        name: trimmed.to_string(),
        status: ProviderStatus::Enabled,
        enabled: true,
        consecutive_failure_count: 0,
        auto_disabled_day: None,
        last_failure_reason: None,
        last_failure_at_ms: None,
        base_url: String::new(),
        api_key: String::new(),
        connection_test_model: String::new(),
        allowed_models: Vec::new(),
        model_mappings: Vec::new(),
        connection_status: None,
    });

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
    let router = RouterConfig {
        enabled: payload.enabled,
        remote_compaction_enabled: payload.remote_compaction_enabled,
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
        connect_timeout_secs: payload.connect_timeout_secs,
        response_header_timeout_secs: payload.response_header_timeout_secs,
        stream_idle_timeout_secs: payload.stream_idle_timeout_secs,
    };
    validate_router_timeouts(&router)?;
    state.router = router;
    ensure_client_configs_applied(&mut state)?;
    save_state(&state)?;
    ensure_router(&router_runtime, &state.router)?;
    build_app_state(state, &router_runtime)
}

#[tauri::command]
fn save_client_configs(
    payload: SaveClientConfigsPayload,
    router_runtime: tauri::State<RouterRuntime>,
) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    state.clients.codex.enabled = payload.codex_enabled;
    state.clients.claude.enabled = payload.claude_enabled;
    state.clients.pi.enabled = payload.pi_enabled;
    if state.clients.pi.enabled && route_models_for_pi(&state).is_empty() {
        return Err("Pi 接管需要至少一个已启用且配置完整的 Codex 供应商路由模型".to_string());
    }
    if state.clients.codex.enabled || state.clients.claude.enabled || state.clients.pi.enabled {
        state.router.enabled = true;
    }
    save_state(&state)?;
    apply_config(router_runtime)
}

#[tauri::command]
fn load_skill_management() -> Result<SkillManagementView, String> {
    let state = load_state_file()?;
    build_skill_management_view(&state)
}

#[tauri::command]
fn save_skill_client_config(
    payload: SaveSkillClientConfigPayload,
) -> Result<SkillManagementView, String> {
    let mut state = load_state_file()?;
    let config = client_config_mut(&mut state.clients, payload.client);
    config.skill_locations = payload.skill_locations;
    config.managed_skill_location = payload.managed_skill_location;
    normalize_skill_management_state(&mut state);
    save_state(&state)?;
    build_skill_management_view(&state)
}

#[tauri::command]
fn promote_client_skill(payload: PromoteClientSkillPayload) -> Result<SkillManagementView, String> {
    let mut state = load_state_file()?;
    let discovered = find_discovered_skill(&state, payload.client, &payload.skill_path)?;
    if discovered.managed {
        return Err("该 Skill 已是 Managed Skill Exposure".to_string());
    }
    promote_discovered_skill(&mut state, &discovered, payload.sharing_scope)?;
    normalize_skill_management_state(&mut state);
    save_state(&state)?;
    build_skill_management_view(&state)
}

#[tauri::command]
fn replace_client_skill_with_shared(
    payload: ReplaceClientSkillWithSharedPayload,
) -> Result<SkillManagementView, String> {
    let mut state = load_state_file()?;
    let discovered = find_discovered_skill(&state, payload.client, &payload.skill_path)?;
    if discovered.managed {
        return Ok(build_skill_management_view(&state)?);
    }
    replace_client_skill_with_shared_skill(&mut state, &discovered)?;
    normalize_skill_management_state(&mut state);
    save_state(&state)?;
    build_skill_management_view(&state)
}

#[tauri::command]
fn set_skill_sharing_scope(
    payload: SetSkillSharingScopePayload,
) -> Result<SkillManagementView, String> {
    let mut state = load_state_file()?;
    let skill = state
        .skills
        .shared_skills
        .iter_mut()
        .find(|skill| skill.identity == payload.skill_identity)
        .ok_or_else(|| "Shared Skill 不存在".to_string())?;
    let mut seen = BTreeSet::new();
    skill.sharing_scope = payload
        .sharing_scope
        .into_iter()
        .filter(|client| seen.insert(*client))
        .collect();
    apply_skill_sharing_scope(&mut state, &payload.skill_identity)?;
    normalize_skill_management_state(&mut state);
    save_state(&state)?;
    build_skill_management_view(&state)
}

#[tauri::command]
fn delete_shared_skill(payload: SkillIdentityPayload) -> Result<SkillManagementView, String> {
    let mut state = load_state_file()?;
    let skill = state
        .skills
        .shared_skills
        .iter()
        .find(|skill| skill.identity == payload.skill_identity)
        .cloned()
        .ok_or_else(|| "Shared Skill 不存在".to_string())?;
    let exposures = state
        .skills
        .exposures
        .iter()
        .filter(|exposure| exposure.skill_identity == skill.identity)
        .cloned()
        .collect::<Vec<_>>();
    for exposure in &exposures {
        remove_registered_exposure(exposure)?;
    }
    state
        .skills
        .exposures
        .retain(|exposure| exposure.skill_identity != skill.identity);
    let library_path = skill_library_root()?.join(&skill.library_dir_name);
    if library_path.exists() {
        remove_file_or_dir(&library_path)?;
    }
    state
        .skills
        .shared_skills
        .retain(|shared| shared.identity != skill.identity);
    normalize_skill_management_state(&mut state);
    save_state(&state)?;
    build_skill_management_view(&state)
}

#[tauri::command]
fn apply_config(router_runtime: tauri::State<RouterRuntime>) -> Result<AppState, String> {
    let mut state = load_state_file()?;
    let config_path = codex_config_path()?;

    if state.clients.codex.enabled || state.clients.claude.enabled || state.clients.pi.enabled {
        state.router.enabled = true;
    }

    if state.clients.codex.enabled {
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

    let claude_path = claude_settings_path()?;
    if state.clients.claude.enabled {
        fs::create_dir_all(claude_home()?)
            .map_err(|err| format!("无法创建 Claude 设置目录: {err}"))?;
        if claude_path.exists() {
            let backup_name = format!(
                "claude-settings.{}.json.bak",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|err| format!("系统时间异常: {err}"))?
                    .as_secs()
            );
            fs::copy(&claude_path, manager_dir()?.join(backup_name))
                .map_err(|err| format!("无法备份现有 Claude 设置: {err}"))?;
        }
        let (settings, _, _) = read_claude_settings()?;
        if state.claude_backup.is_none() {
            state.claude_backup = Some(capture_claude_backup(&settings));
        }
        let raw = render_claude_patch_json(settings, &state.router)?;
        fs::write(&claude_path, raw).map_err(|err| format!("无法写入 Claude 设置: {err}"))?;
    } else if claude_path.exists() || state.claude_backup.is_some() {
        let (settings, _, exists) = read_claude_settings()?;
        if exists {
            let backup_name = format!(
                "claude-settings.{}.json.bak",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|err| format!("系统时间异常: {err}"))?
                    .as_secs()
            );
            fs::copy(&claude_path, manager_dir()?.join(backup_name))
                .map_err(|err| format!("无法备份现有 Claude 设置: {err}"))?;
        }
        let raw =
            restore_claude_backup_json(settings, state.claude_backup.as_ref(), &state.router)?;
        fs::create_dir_all(claude_home()?)
            .map_err(|err| format!("无法创建 Claude 设置目录: {err}"))?;
        fs::write(&claude_path, raw).map_err(|err| format!("无法写入 Claude 设置: {err}"))?;
        state.claude_backup = None;
    }

    let pi_path = pi_models_path()?;
    if state.clients.pi.enabled {
        let models = route_models_for_pi(&state);
        if models.is_empty() {
            return Err("Pi 接管需要至少一个已启用且配置完整的 Codex 供应商路由模型".to_string());
        }
        fs::create_dir_all(
            pi_path
                .parent()
                .ok_or_else(|| "无法定位 Pi 模型配置目录".to_string())?,
        )
        .map_err(|err| format!("无法创建 Pi 模型配置目录: {err}"))?;
        if pi_path.exists() {
            let backup_name = format!(
                "pi-models.{}.json.bak",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|err| format!("系统时间异常: {err}"))?
                    .as_secs()
            );
            fs::copy(&pi_path, manager_dir()?.join(backup_name))
                .map_err(|err| format!("无法备份现有 Pi 模型配置: {err}"))?;
        }
        let (config, _, _) = read_pi_models_config()?;
        if state.pi_backup.is_none() {
            state.pi_backup = Some(capture_pi_backup(&config));
        }
        let raw = render_pi_models_config(config, &state.router, &models)?;
        fs::write(&pi_path, raw).map_err(|err| format!("无法写入 Pi 模型配置: {err}"))?;
    } else if pi_path.exists() || state.pi_backup.is_some() {
        let (config, _, exists) = read_pi_models_config()?;
        if exists {
            let backup_name = format!(
                "pi-models.{}.json.bak",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|err| format!("系统时间异常: {err}"))?
                    .as_secs()
            );
            fs::copy(&pi_path, manager_dir()?.join(backup_name))
                .map_err(|err| format!("无法备份现有 Pi 模型配置: {err}"))?;
        }
        let raw = restore_pi_models_config(config, state.pi_backup.as_ref())?;
        fs::create_dir_all(
            pi_path
                .parent()
                .ok_or_else(|| "无法定位 Pi 模型配置目录".to_string())?,
        )
        .map_err(|err| format!("无法创建 Pi 模型配置目录: {err}"))?;
        fs::write(&pi_path, raw).map_err(|err| format!("无法写入 Pi 模型配置: {err}"))?;
        state.pi_backup = None;
    }
    state.applied_provider_id = None;
    save_state(&state)?;
    ensure_router(&router_runtime, &state.router)?;
    build_app_state(state, &router_runtime)
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
async fn load_provider_models(
    payload: LoadProviderModelsPayload,
) -> Result<ProviderModelsResponse, String> {
    let state = load_state_file()?;
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
    let (models, _) = fetch_provider_models(&test_provider).await?;
    Ok(ProviderModelsResponse { models })
}

#[tauri::command]
async fn load_claude_provider_models(
    payload: LoadProviderModelsPayload,
) -> Result<ProviderModelsResponse, String> {
    let state = load_state_file()?;
    let provider = state
        .claude_providers
        .iter()
        .find(|provider| provider.id == payload.provider_id)
        .cloned()
        .ok_or_else(|| "Claude 供应商不存在".to_string())?;
    let mut test_provider = provider.clone();
    apply_claude_provider_connection_draft(
        &mut test_provider,
        payload.base_url.as_deref(),
        payload.api_key.as_deref(),
    )?;
    let (models, _) = fetch_claude_provider_models(&test_provider).await?;
    Ok(ProviderModelsResponse { models })
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

#[tauri::command]
async fn test_provider_connection(
    payload: TestProviderConnectionPayload,
) -> Result<ProviderConnectionTestResult, String> {
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

    let result = run_provider_connection_test(&test_provider, payload.test_model.as_deref()).await;
    if payload.base_url.is_none() && payload.api_key.is_none() {
        if let Some(provider) = state
            .providers
            .iter_mut()
            .find(|provider| provider.id == payload.provider_id)
        {
            provider.connection_status = Some(connection_status_from_test(&result));
        }
        save_state(&state)?;
    }

    Ok(result)
}

#[tauri::command]
async fn test_provider_connection_state(
    payload: TestProviderConnectionPayload,
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

    let result = run_provider_connection_test(&test_provider, payload.test_model.as_deref()).await;
    if let Some(provider) = state
        .providers
        .iter_mut()
        .find(|provider| provider.id == payload.provider_id)
    {
        provider.connection_status = Some(connection_status_from_test(&result));
    }

    save_state(&state)?;
    build_app_state(state, &router_runtime)
}

#[tauri::command]
async fn test_provider_latency_state(
    payload: TestProviderLatencyPayload,
    router_runtime: tauri::State<'_, RouterRuntime>,
) -> Result<ProviderLatencyTestResponse, String> {
    let model = payload.model.trim();
    if model.is_empty() {
        return Err("请选择测试模型".to_string());
    }
    let prompt = payload.prompt.trim();
    if prompt.is_empty() {
        return Err("测试内容不能为空".to_string());
    }

    let mut state = load_state_file()?;
    let result = match payload.provider_kind {
        AgentClientKind::Codex => {
            let provider = state
                .providers
                .iter()
                .find(|provider| provider.id == payload.provider_id)
                .cloned()
                .ok_or_else(|| "供应商不存在".to_string())?;
            measure_codex_provider_latency(&provider, model, prompt, payload.stream).await
        }
        AgentClientKind::Claude => {
            let provider = state
                .claude_providers
                .iter()
                .find(|provider| provider.id == payload.provider_id)
                .cloned()
                .ok_or_else(|| "Claude 供应商不存在".to_string())?;
            measure_claude_provider_latency(&provider, model, prompt, payload.stream).await
        }
        AgentClientKind::Pi => return Err("Pi 不支持供应商模型测速".to_string()),
    };

    let (ok, latency_ms, error, reply) = match result {
        Ok(probe) => (true, Some(probe.latency_ms), None, probe.reply),
        Err(error) => (false, None, Some(error), String::new()),
    };
    let status = latency_connection_status(match &error {
        Some(error) => Err(error.clone()),
        None => Ok(latency_ms.unwrap_or_default()),
    });
    match payload.provider_kind {
        AgentClientKind::Codex => {
            if let Some(provider) = state
                .providers
                .iter_mut()
                .find(|provider| provider.id == payload.provider_id)
            {
                provider.connection_status = Some(status);
            }
        }
        AgentClientKind::Claude => {
            if let Some(provider) = state
                .claude_providers
                .iter_mut()
                .find(|provider| provider.id == payload.provider_id)
            {
                provider.connection_status = Some(status);
            }
        }
        AgentClientKind::Pi => unreachable!(),
    }

    save_state(&state)?;
    Ok(ProviderLatencyTestResponse {
        app_state: build_app_state(state, &router_runtime)?,
        ok,
        latency_ms,
        error,
        reply,
    })
}

fn show_main_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };

    if let Err(err) = window.show() {
        eprintln!("显示主窗口失败: {err}");
    }
    if let Err(err) = window.unminimize() {
        eprintln!("恢复主窗口失败: {err}");
    }
    if let Err(err) = window.set_focus() {
        eprintln!("聚焦主窗口失败: {err}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(RouterRuntime::default())
        .setup(|app| {
            let show_item = MenuItem::with_id(app, TRAY_SHOW_ID, "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, TRAY_QUIT_ID, "退出", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;
            let mut tray_builder = TrayIconBuilder::new()
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .tooltip("XXSwitch")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    TRAY_SHOW_ID => show_main_window(app),
                    TRAY_QUIT_ID => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }
            tray_builder.build(app)?;

            let mut state = load_state_file()?;
            match ensure_client_configs_applied(&mut state) {
                Ok(true) => {
                    if let Err(err) = save_state(&state) {
                        eprintln!("{err}");
                    }
                }
                Ok(false) => {}
                Err(err) => eprintln!("客户端接管配置检查失败: {err}"),
            }
            let router_runtime = app.state::<RouterRuntime>();
            if let Err(err) = ensure_router(&router_runtime, &state.router) {
                eprintln!("本地路由启动失败: {err}");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == MAIN_WINDOW_LABEL {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Err(err) = window.hide() {
                        eprintln!("隐藏主窗口失败: {err}");
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            load_app_state,
            check_for_update,
            install_update,
            select_provider,
            get_provider,
            get_claude_provider,
            save_provider,
            save_claude_provider,
            delete_provider,
            delete_claude_provider,
            reorder_providers,
            reorder_claude_providers,
            preview_provider,
            add_provider,
            add_claude_provider,
            save_base_template,
            save_router_config,
            save_client_configs,
            load_skill_management,
            save_skill_client_config,
            promote_client_skill,
            replace_client_skill_with_shared,
            set_skill_sharing_scope,
            delete_shared_skill,
            apply_config,
            load_route_logs,
            load_route_usage_stats,
            load_provider_models,
            load_claude_provider_models,
            query_provider_balance,
            test_provider_connection,
            test_provider_connection_state,
            test_provider_latency_state
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_provider_config(id: &str, status: ProviderStatus, enabled: bool) -> ProviderConfig {
        ProviderConfig {
            id: id.to_string(),
            name: id.to_string(),
            status,
            enabled,
            consecutive_failure_count: 0,
            auto_disabled_day: None,
            last_failure_reason: None,
            last_failure_at_ms: None,
            config: json!({}),
            wire_api: ProviderWireApi::Responses,
            service_tier: String::new(),
            connection_test_model: String::new(),
            allowed_models: Vec::new(),
            model_mappings: Vec::new(),
            balance_query: BalanceQueryConfig::default(),
            balance_status: None,
            connection_status: None,
        }
    }

    fn test_claude_provider_config(
        id: &str,
        status: ProviderStatus,
        enabled: bool,
    ) -> ClaudeProviderConfig {
        ClaudeProviderConfig {
            id: id.to_string(),
            name: id.to_string(),
            status,
            enabled,
            consecutive_failure_count: 0,
            auto_disabled_day: None,
            last_failure_reason: None,
            last_failure_at_ms: None,
            base_url: String::new(),
            api_key: String::new(),
            connection_test_model: String::new(),
            allowed_models: Vec::new(),
            model_mappings: Vec::new(),
            connection_status: None,
        }
    }

    #[test]
    fn legacy_router_config_receives_timeout_defaults() {
        let router = serde_json::from_value::<RouterConfig>(json!({
            "enabled": true,
            "remote_compaction_enabled": false,
            "host": "127.0.0.1",
            "port": 18080,
            "local_token": "local-token"
        }))
        .unwrap();

        assert_eq!(router.connect_timeout_secs, DEFAULT_CONNECT_TIMEOUT_SECS);
        assert_eq!(
            router.response_header_timeout_secs,
            DEFAULT_RESPONSE_HEADER_TIMEOUT_SECS
        );
        assert_eq!(
            router.stream_idle_timeout_secs,
            DEFAULT_STREAM_IDLE_TIMEOUT_SECS
        );
    }

    #[test]
    fn validates_router_timeout_ranges() {
        let mut router = RouterConfig::default();
        assert!(validate_router_timeouts(&router).is_ok());

        router.connect_timeout_secs = 0;
        assert!(validate_router_timeouts(&router)
            .unwrap_err()
            .contains("连接超时"));

        router.connect_timeout_secs = DEFAULT_CONNECT_TIMEOUT_SECS;
        router.response_header_timeout_secs = MAX_RESPONSE_HEADER_TIMEOUT_SECS + 1;
        assert!(validate_router_timeouts(&router)
            .unwrap_err()
            .contains("响应头超时"));

        router.response_header_timeout_secs = DEFAULT_RESPONSE_HEADER_TIMEOUT_SECS;
        router.stream_idle_timeout_secs = MAX_STREAM_IDLE_TIMEOUT_SECS + 1;
        assert!(validate_router_timeouts(&router)
            .unwrap_err()
            .contains("流式响应空闲超时"));

        router.stream_idle_timeout_secs = DEFAULT_STREAM_IDLE_TIMEOUT_SECS;
        router.connect_timeout_secs = 31;
        router.response_header_timeout_secs = 30;
        assert_eq!(
            validate_router_timeouts(&router).unwrap_err(),
            "响应头超时不能小于连接超时"
        );

        router.connect_timeout_secs = DEFAULT_CONNECT_TIMEOUT_SECS;
        router.response_header_timeout_secs = 121;
        router.stream_idle_timeout_secs = 120;
        assert_eq!(
            validate_router_timeouts(&router).unwrap_err(),
            "流式响应空闲超时不能小于响应头超时"
        );
    }

    #[test]
    fn compares_update_versions_with_v_prefix_and_prereleases() {
        assert!(update_is_available("v1.1.7", "v1.1.8").unwrap());
        assert!(update_is_available("v1.1.8-beta.1", "v1.1.8").unwrap());
        assert!(!update_is_available("v1.1.8", "v1.1.8").unwrap());
        assert!(!update_is_available("v1.1.9", "v1.1.8").unwrap());
    }

    #[test]
    fn selects_release_asset_for_each_supported_update_platform() {
        let assets = vec![
            GithubReleaseAsset {
                name: "XXSwitch_1.1.8_universal.dmg".to_string(),
                browser_download_url: format!("{GITHUB_RELEASE_DOWNLOAD_PREFIX}v1.1.8/mac.dmg"),
                digest: None,
            },
            GithubReleaseAsset {
                name: "XXSwitch_1.1.8_x64-setup.exe".to_string(),
                browser_download_url: format!("{GITHUB_RELEASE_DOWNLOAD_PREFIX}v1.1.8/windows.exe"),
                digest: None,
            },
        ];

        assert_eq!(
            update_asset_for_platform(&assets, UpdatePlatform::Windows)
                .map(|asset| asset.name.as_str()),
            Some("XXSwitch_1.1.8_x64-setup.exe")
        );
        assert_eq!(
            update_asset_for_platform(&assets, UpdatePlatform::Macos)
                .map(|asset| asset.name.as_str()),
            Some("XXSwitch_1.1.8_universal.dmg")
        );
        assert!(update_asset_for_platform(&assets, UpdatePlatform::Unsupported).is_none());
    }

    #[test]
    fn verifies_github_release_asset_sha256_digest() {
        assert!(verify_update_asset_digest(
            b"codex-helper",
            Some("sha256:129713b6f0f6e251c45e57f52f26f0e753b666b9f984a042127488ac6567fdbd")
        )
        .is_ok());
        assert!(verify_update_asset_digest(b"tampered", Some("sha256:deadbeef")).is_err());
        assert!(verify_update_asset_digest(b"legacy", None).is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn locates_macos_app_bundle_from_executable_path() {
        let executable = Path::new("/Applications/XXSwitch.app/Contents/MacOS/xxswitch");
        assert_eq!(
            macos_app_bundle_from_path(executable).as_deref(),
            Some(Path::new("/Applications/XXSwitch.app"))
        );
        assert!(macos_app_bundle_from_path(Path::new("/tmp/xxswitch")).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn quotes_macos_update_paths_for_shell_commands() {
        assert_eq!(
            shell_quote(Path::new("/Applications/XXSwitch.app")),
            "'/Applications/XXSwitch.app'"
        );
        assert_eq!(
            shell_quote(Path::new("/tmp/owner's app.app")),
            "'/tmp/owner'\\''s app.app'"
        );
    }

    struct ChatStreamTestState {
        buffer: String,
        response_id: String,
        created_at: i64,
        model: String,
        output_text: String,
        reasoning_content: String,
        output_index: usize,
        next_output_index: usize,
        tool_context: CodexToolContext,
        tool_calls: BTreeMap<usize, ChatToolCallState>,
        completed_output: Vec<(usize, Value)>,
        sequence_number: u64,
        started: bool,
        text_done: bool,
        completed: bool,
        usage_seen: bool,
        usage: TokenUsage,
    }

    impl ChatStreamTestState {
        fn new(tool_context: CodexToolContext) -> Self {
            Self {
                buffer: String::new(),
                response_id: "resp_chatcmpl".to_string(),
                created_at: 0,
                model: String::new(),
                output_text: String::new(),
                reasoning_content: String::new(),
                output_index: 0,
                next_output_index: 1,
                tool_context,
                tool_calls: BTreeMap::new(),
                completed_output: Vec::new(),
                sequence_number: 0,
                started: false,
                text_done: false,
                completed: false,
                usage_seen: false,
                usage: TokenUsage::default(),
            }
        }

        fn ingest(&mut self, chunk: &[u8]) -> Vec<Bytes> {
            chat_stream_events_to_responses(
                &mut self.buffer,
                chunk,
                &mut self.response_id,
                &mut self.created_at,
                &mut self.model,
                &mut self.output_text,
                &mut self.reasoning_content,
                &mut self.output_index,
                &mut self.next_output_index,
                &self.tool_context,
                &mut self.tool_calls,
                &mut self.completed_output,
                &mut self.sequence_number,
                &mut self.started,
                &mut self.text_done,
                &mut self.completed,
                &mut self.usage_seen,
                &mut self.usage,
            )
        }
    }

    #[test]
    fn migrates_legacy_router_enabled_to_codex_client() {
        let mut state = default_state();
        state.router.enabled = true;

        let migrated = migrate_legacy_clients_if_missing(state, true);

        assert!(migrated.clients.codex.enabled);
        assert!(!migrated.clients.claude.enabled);
        assert!(migrated.router.enabled);
    }

    #[test]
    fn preserves_existing_clients_when_state_is_not_legacy() {
        let mut state = default_state();
        state.router.enabled = true;
        state.clients.codex.enabled = false;
        state.clients.claude.enabled = true;

        let migrated = migrate_legacy_clients_if_missing(state, false);

        assert!(!migrated.clients.codex.enabled);
        assert!(migrated.clients.claude.enabled);
    }

    #[test]
    fn normalizes_legacy_disabled_provider_to_disabled_status() {
        let mut state = default_state();
        state.providers = vec![test_provider_config(
            "provider-a",
            ProviderStatus::Enabled,
            false,
        )];

        let normalized = normalize_state(state);

        assert_eq!(normalized.providers[0].status, ProviderStatus::Disabled);
        assert!(!normalized.providers[0].enabled);
    }

    #[test]
    fn keeps_auto_disabled_provider_for_current_day() {
        let mut provider = test_provider_config("provider-a", ProviderStatus::AutoDisabled, false);
        provider.consecutive_failure_count = AUTO_DISABLE_FAILURE_THRESHOLD;
        provider.auto_disabled_day = Some(provider_day_now());
        provider.last_failure_reason = Some("upstream failed".to_string());
        provider.last_failure_at_ms = Some(1);
        let mut state = default_state();
        state.providers = vec![provider];

        let normalized = normalize_state(state);

        assert_eq!(normalized.providers[0].status, ProviderStatus::AutoDisabled);
        assert!(!normalized.providers[0].enabled);
        assert_eq!(
            normalized.providers[0].consecutive_failure_count,
            AUTO_DISABLE_FAILURE_THRESHOLD
        );
    }

    #[test]
    fn keeps_enabled_provider_failure_sequence_across_normalization() {
        let mut provider = test_provider_config("provider-a", ProviderStatus::Enabled, true);
        provider.consecutive_failure_count = AUTO_DISABLE_FAILURE_THRESHOLD - 1;
        provider.last_failure_reason = Some("upstream failed".to_string());
        provider.last_failure_at_ms = Some(1);
        let mut state = default_state();
        state.providers = vec![provider];

        let normalized = normalize_state(state);

        assert_eq!(normalized.providers[0].status, ProviderStatus::Enabled);
        assert!(normalized.providers[0].enabled);
        assert_eq!(
            normalized.providers[0].consecutive_failure_count,
            AUTO_DISABLE_FAILURE_THRESHOLD - 1
        );
        assert_eq!(
            normalized.providers[0].last_failure_reason.as_deref(),
            Some("upstream failed")
        );
    }

    #[test]
    fn recovers_auto_disabled_provider_after_provider_day_changes() {
        let mut provider = test_provider_config("provider-a", ProviderStatus::AutoDisabled, false);
        provider.consecutive_failure_count = AUTO_DISABLE_FAILURE_THRESHOLD;
        provider.auto_disabled_day = Some("1900-01-01".to_string());
        provider.last_failure_reason = Some("upstream failed".to_string());
        provider.last_failure_at_ms = Some(1);
        let mut state = default_state();
        state.providers = vec![provider];

        let normalized = normalize_state(state);

        assert_eq!(normalized.providers[0].status, ProviderStatus::Enabled);
        assert!(normalized.providers[0].enabled);
        assert_eq!(normalized.providers[0].consecutive_failure_count, 0);
        assert_eq!(normalized.providers[0].auto_disabled_day, None);
        assert_eq!(normalized.providers[0].last_failure_reason, None);
    }

    #[test]
    fn provider_success_clears_auto_disabled_state() {
        let mut provider = test_provider_config("provider-a", ProviderStatus::AutoDisabled, false);
        provider.consecutive_failure_count = AUTO_DISABLE_FAILURE_THRESHOLD;
        provider.auto_disabled_day = Some(provider_day_now());
        provider.last_failure_reason = Some("upstream failed".to_string());
        provider.last_failure_at_ms = Some(1);

        assert!(clear_provider_failure_state(&mut provider));
        assert_eq!(provider.status, ProviderStatus::Enabled);
        assert!(provider.enabled);
        assert_eq!(provider.consecutive_failure_count, 0);
        assert_eq!(provider.auto_disabled_day, None);
        assert_eq!(provider.last_failure_reason, None);
    }

    #[test]
    fn deleting_provider_removes_it_and_reselects_active() {
        let mut state = default_state();
        state.providers = vec![
            test_provider_config("provider-a", ProviderStatus::Enabled, true),
            test_provider_config("provider-b", ProviderStatus::Enabled, true),
        ];
        state.active_provider_id = "provider-a".to_string();
        state.applied_provider_id = Some("provider-a".to_string());

        delete_provider_from_state(&mut state, "provider-a").unwrap();

        assert_eq!(state.providers.len(), 1);
        assert_eq!(state.providers[0].id, "provider-b");
        assert_eq!(state.active_provider_id, "provider-b");
        assert_eq!(state.applied_provider_id, None);
    }

    #[test]
    fn deleting_claude_provider_removes_it_and_reselects_active() {
        let mut state = default_state();
        state.claude_providers = vec![
            test_claude_provider_config("claude-a", ProviderStatus::Disabled, false),
            test_claude_provider_config("claude-b", ProviderStatus::Enabled, true),
        ];
        state.active_claude_provider_id = "claude-a".to_string();

        delete_claude_provider_from_state(&mut state, "claude-a").unwrap();

        assert_eq!(state.claude_providers.len(), 1);
        assert_eq!(state.claude_providers[0].id, "claude-b");
        assert_eq!(state.active_claude_provider_id, "claude-b");
    }

    #[test]
    fn patches_and_restores_claude_settings_env_fields() {
        let settings = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://old.example",
                "ANTHROPIC_AUTH_TOKEN": "old-token",
                "ANTHROPIC_API_KEY": "old-api-key",
                "OTHER_ENV": "kept"
            },
            "permissions": {
                "allow": ["Bash(ls)"]
            }
        });
        let backup = capture_claude_backup(&settings);
        let router = RouterConfig {
            enabled: true,
            remote_compaction_enabled: false,
            host: "127.0.0.1".to_string(),
            port: 18080,
            local_token: "local-token".to_string(),
            ..RouterConfig::default()
        };

        let patched = render_claude_patch_json(settings, &router).unwrap();
        let patched = serde_json::from_str::<Value>(&patched).unwrap();
        assert_eq!(
            json_env_value(&patched, "ANTHROPIC_BASE_URL"),
            Some(Value::String("http://127.0.0.1:18080".to_string()))
        );
        assert_eq!(json_env_value(&patched, "ANTHROPIC_AUTH_TOKEN"), None);
        assert_eq!(
            json_env_value(&patched, "ANTHROPIC_API_KEY"),
            Some(Value::String("local-token".to_string()))
        );
        assert_eq!(
            json_env_value(&patched, "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY"),
            Some(Value::String("1".to_string()))
        );
        assert_eq!(
            json_env_value(&patched, "ANTHROPIC_DEFAULT_FABLE_MODEL"),
            Some(Value::String("claude-fable-5".to_string()))
        );
        assert_eq!(
            json_env_value(&patched, "OTHER_ENV"),
            Some(Value::String("kept".to_string()))
        );

        let restored = restore_claude_backup_json(patched, Some(&backup), &router).unwrap();
        let restored = serde_json::from_str::<Value>(&restored).unwrap();
        assert_eq!(
            json_env_value(&restored, "ANTHROPIC_BASE_URL"),
            Some(Value::String("https://old.example".to_string()))
        );
        assert_eq!(
            json_env_value(&restored, "ANTHROPIC_AUTH_TOKEN"),
            Some(Value::String("old-token".to_string()))
        );
        assert_eq!(
            json_env_value(&restored, "ANTHROPIC_API_KEY"),
            Some(Value::String("old-api-key".to_string()))
        );
        assert_eq!(
            json_env_value(&restored, "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY"),
            None
        );
        assert_eq!(
            json_env_value(&restored, "ANTHROPIC_DEFAULT_FABLE_MODEL"),
            None
        );
        assert_eq!(
            json_env_value(&restored, "OTHER_ENV"),
            Some(Value::String("kept".to_string()))
        );
    }

    #[test]
    fn remote_compaction_switches_custom_provider_name() {
        let doc = r#"
model_provider = "custom"

[model_providers.custom]
name = "Example Provider"
base_url = "https://example.com/v1"
experimental_bearer_token = "example-token"
"#
        .parse::<DocumentMut>()
        .unwrap();
        let enabled_router = RouterConfig {
            enabled: true,
            remote_compaction_enabled: true,
            host: "127.0.0.1".to_string(),
            port: 18080,
            local_token: "local-token".to_string(),
            ..RouterConfig::default()
        };

        let patched = render_router_patch_toml(doc, false, &enabled_router)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(
            toml_path_value(&patched, "model_providers.custom.name"),
            Some(Value::String("OpenAI".to_string()))
        );
        assert_eq!(
            router_patch_desired(&enabled_router)
                .pointer("/model_providers/custom/name")
                .and_then(Value::as_str),
            Some("OpenAI")
        );
        assert_eq!(
            toml_path_value(&patched, "features.remote_compaction_v2"),
            Some(Value::Bool(true))
        );
        assert_eq!(
            router_patch_desired(&enabled_router).pointer("/features/remote_compaction_v2"),
            Some(&Value::Bool(true))
        );

        let disabled_router = RouterConfig {
            remote_compaction_enabled: false,
            ..enabled_router
        };
        let restored = render_router_patch_toml(patched, true, &disabled_router)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(
            toml_path_value(&restored, "model_providers.custom.name"),
            Some(Value::String("custom".to_string()))
        );
        assert_eq!(
            router_patch_desired(&disabled_router)
                .pointer("/model_providers/custom/name")
                .and_then(Value::as_str),
            Some("custom")
        );
        assert_eq!(
            toml_path_value(&restored, "features.remote_compaction_v2"),
            Some(Value::Bool(false))
        );
    }

    #[test]
    fn forwards_remote_compaction_with_the_original_model_name() {
        let provider = test_provider_config("provider-a", ProviderStatus::Enabled, true);
        let body = br#"{"model":"gpt-5.6-sol","input":[],"parallel_tool_calls":false}"#;

        let prepared =
            prepare_upstream_request(&provider, "responses/compact", "", body, "gpt-5.6-sol")
                .unwrap();
        let forwarded: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(prepared.path, "responses/compact");
        assert_eq!(
            forwarded.get("model").and_then(Value::as_str),
            Some("gpt-5.6-sol")
        );
    }

    #[test]
    fn decodes_zstd_request_before_model_routing() {
        let raw = br#"{"model":"gpt-5.6-sol","input":[],"parallel_tool_calls":false}"#;
        let compressed = zstd::stream::encode_all(Cursor::new(raw), 0).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("zstd"));

        let decoded = decoded_proxy_request_body(&headers, &compressed).unwrap();

        assert_eq!(decoded.as_ref(), raw);
        assert_eq!(model_from_request_body(&decoded), "gpt-5.6-sol");

        let decoded_without_header =
            decoded_proxy_request_body(&HeaderMap::new(), &compressed).unwrap();
        assert_eq!(decoded_without_header.as_ref(), raw);
    }

    #[test]
    fn audits_remote_compaction_v2_without_storing_request_content() {
        let request = br#"{
            "model":"gpt-5.6-sol",
            "compaction_trigger":{"reason":"context_limit"},
            "input":[{"type":"compaction","id":"cmp_test"}]
        }"#;
        let audit = remote_compaction_v2_audit_from_request_body(request);
        let forwarded = body_with_provider_overrides(request, Some("upstream-model"), None);
        let response = br#"{
            "output":[{"type":"compaction","id":"cmp_result"}]
        }"#;
        let sse = "event: response.output_item.done\n\
data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\",\"id\":\"cmp_result\"}}\n\n";

        assert!(audit.trigger_received);
        assert!(audit.compaction_item_reused);
        assert!(request_has_compaction_trigger(&forwarded));
        assert!(response_contains_compaction_item(response));
        assert!(sse_event_contains_compaction_item(sse));
    }

    #[test]
    fn maps_and_filters_claude_provider_models() {
        let provider = ClaudeProviderConfig {
            id: "anthropic".to_string(),
            name: "Anthropic".to_string(),
            status: ProviderStatus::Enabled,
            enabled: true,
            consecutive_failure_count: 0,
            auto_disabled_day: None,
            last_failure_reason: None,
            last_failure_at_ms: None,
            base_url: "https://api.anthropic.com".to_string(),
            api_key: "sk-ant".to_string(),
            connection_test_model: String::new(),
            allowed_models: vec!["claude-sonnet-4-5".to_string()],
            model_mappings: vec![ModelMapping {
                source: "claude-sonnet-4-5".to_string(),
                target: "claude-3-5-sonnet-latest".to_string(),
            }],
            connection_status: None,
        };

        assert!(claude_provider_accepts_model(
            &provider,
            "CLAUDE-SONNET-4-5"
        ));
        assert!(!claude_provider_accepts_model(&provider, "claude-opus-4-1"));
        assert_eq!(
            mapped_model_for_claude_provider(&provider, "claude-sonnet-4-5"),
            Some("claude-3-5-sonnet-latest".to_string())
        );
    }

    #[test]
    fn claude_upstream_path_does_not_inject_v1() {
        assert_eq!(claude_upstream_path("messages"), "messages");
        assert_eq!(claude_upstream_path("/models"), "models");
        assert_eq!(
            join_url(
                "https://provider.example",
                &claude_upstream_path("messages")
            ),
            "https://provider.example/messages"
        );
        assert_eq!(
            join_url(
                "https://provider.example/v1",
                &claude_upstream_path("messages")
            ),
            "https://provider.example/v1/messages"
        );
    }

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

    fn route_log_for_stats_test(status: &str, provider_id: &str) -> RouteRequestLog {
        route_log_with_path_for_stats_test(status, provider_id, "/v1/responses")
    }

    fn route_log_with_path_for_stats_test(
        status: &str,
        provider_id: &str,
        path: &str,
    ) -> RouteRequestLog {
        RouteRequestLog {
            id: format!("test-{status}-{provider_id}"),
            started_at_ms: 1_782_470_400_000,
            day: "2026-06-27".to_string(),
            hour: "10:00".to_string(),
            method: "POST".to_string(),
            path: path.to_string(),
            model: "test-model".to_string(),
            remote_compaction_v2: RemoteCompactionV2Audit::default(),
            upstream_model: None,
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
            first_byte_ms: Some(100),
            total_ms: 200,
        }
    }

    fn pending_route_log_for_test(status_code: Option<u16>) -> PendingRouteLog {
        PendingRouteLog {
            id: "test-cancelled-provider-a".to_string(),
            started_at_ms: 1_782_470_400_000,
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            model: "test-model".to_string(),
            remote_compaction_v2: RemoteCompactionV2Audit::default(),
            upstream_model: None,
            provider_id: "provider-a".to_string(),
            provider_name: "Provider A".to_string(),
            provider_order: 1,
            upstream_chain: vec!["Provider A".to_string()],
            status_code,
            route_result: "直连".to_string(),
            route_attempts: 1,
            error: None,
            start: Instant::now(),
        }
    }

    #[test]
    fn cancelled_route_log_describes_waiting_phase() {
        let mut pending = pending_route_log_for_test(None);
        mark_pending_route_log_cancelled(&mut pending, None);
        let log = build_finished_route_log(pending, "cancelled", TokenUsage::default(), None);

        assert_eq!(log.status, "cancelled");
        assert_eq!(log.route_result, "已取消 · 尚未收到远端响应");
        assert_eq!(
            log.error.as_deref(),
            Some("客户端在完整响应前断开：尚未收到远端响应")
        );
        assert_eq!(log.first_byte_ms, None);
        assert_eq!(log.total_tokens, 0);
    }

    #[test]
    fn cancelled_stream_route_log_keeps_partial_usage() {
        let mut pending = pending_route_log_for_test(Some(200));
        mark_pending_route_log_cancelled(&mut pending, Some(125));
        let log = build_finished_route_log(
            pending,
            "cancelled",
            TokenUsage {
                input_tokens: 11,
                cached_input_tokens: 3,
                output_tokens: 5,
                reasoning_output_tokens: 2,
                total_tokens: 16,
            },
            Some(125),
        );

        assert_eq!(log.route_result, "已取消 · 流式响应尚未结束");
        assert_eq!(log.first_byte_ms, Some(125));
        assert_eq!(log.uncached_input_tokens, 8);
        assert_eq!(log.cached_input_tokens, 3);
        assert_eq!(log.output_tokens, 5);
        assert_eq!(log.total_tokens, 16);
    }

    #[test]
    fn route_usage_counts_tokens_only_for_successful_requests() {
        let stats = build_route_usage_stats(
            vec![
                route_log_for_stats_test("success", "provider-a"),
                route_log_for_stats_test("failed", "provider-b"),
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
        assert_eq!(stats.buckets.len(), 1);
        assert_eq!(stats.buckets[0].request_count, 1);
        assert_eq!(stats.providers.len(), 1);
        assert_eq!(stats.providers[0].key, "provider-a");
        assert_eq!(stats.models[0].request_count, 1);
    }

    #[test]
    fn route_usage_ignores_successful_non_generation_requests() {
        let stats = build_route_usage_stats(
            vec![
                route_log_for_stats_test("success", "provider-a"),
                route_log_with_path_for_stats_test("success", "provider-a", "/v1/models"),
            ],
            RouteLogFilter {
                start_day: Some("2026-06-27".to_string()),
                end_day: Some("2026-06-27".to_string()),
                page_size: Some(50),
                ..Default::default()
            },
        )
        .expect("route usage stats should build");

        assert_eq!(stats.total, 1);
        assert_eq!(stats.details.len(), 1);
        assert_eq!(stats.success_count, 2);
        assert_eq!(stats.summary.request_count, 1);
        assert_eq!(stats.summary.total_tokens, 10);
        assert_eq!(stats.buckets[0].request_count, 1);
        assert_eq!(stats.providers[0].request_count, 1);
        assert_eq!(stats.models[0].request_count, 1);
    }

    #[test]
    fn route_logs_ignore_non_generation_requests() {
        let response = build_route_logs_response(
            vec![
                route_log_for_stats_test("success", "provider-a"),
                route_log_with_path_for_stats_test("success", "provider-b", "/v1/models"),
            ],
            RouteLogFilter {
                page_size: Some(50),
                ..Default::default()
            },
        );

        assert_eq!(response.total, 1);
        assert_eq!(response.logs.len(), 1);
        assert_eq!(response.logs[0].path, "/v1/responses");
        assert_eq!(response.available_providers.len(), 1);
        assert_eq!(response.available_providers[0].id, "provider-a");
    }

    #[test]
    fn route_logs_can_filter_cancelled_requests() {
        let response = build_route_logs_response(
            vec![
                route_log_for_stats_test("success", "provider-a"),
                route_log_for_stats_test("cancelled", "provider-b"),
                route_log_for_stats_test("failed", "provider-c"),
            ],
            RouteLogFilter {
                status: Some("cancelled".to_string()),
                page_size: Some(50),
                ..Default::default()
            },
        );

        assert_eq!(response.total, 1);
        assert_eq!(response.logs[0].status, "cancelled");
        assert_eq!(response.logs[0].provider_id, "provider-b");
    }

    #[test]
    fn empty_balance_token_draft_preserves_saved_token() {
        let provider = ProviderConfig {
            id: "provider-a".to_string(),
            name: "Provider A".to_string(),
            status: ProviderStatus::Enabled,
            enabled: true,
            consecutive_failure_count: 0,
            auto_disabled_day: None,
            last_failure_reason: None,
            last_failure_at_ms: None,
            config: json!({
                "model_provider": "custom",
                "model_providers": {
                    "custom": {
                        "base_url": "https://example.com",
                        "experimental_bearer_token": "provider-token"
                    }
                }
            }),
            wire_api: ProviderWireApi::Responses,
            service_tier: String::new(),
            connection_test_model: String::new(),
            allowed_models: Vec::new(),
            model_mappings: Vec::new(),
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
            connection_status: None,
        };
        let mut draft = provider.balance_query.clone();
        draft.query_token.clear();

        let merged = merge_balance_config_draft(&provider, draft);

        assert_eq!(merged.query_token, "saved-balance-token");
    }

    #[test]
    fn maps_request_model_for_provider_body() {
        let provider = ProviderConfig {
            id: "provider-a".to_string(),
            name: "Provider A".to_string(),
            status: ProviderStatus::Enabled,
            enabled: true,
            consecutive_failure_count: 0,
            auto_disabled_day: None,
            last_failure_reason: None,
            last_failure_at_ms: None,
            config: json!({}),
            wire_api: ProviderWireApi::Responses,
            service_tier: String::new(),
            connection_test_model: String::new(),
            allowed_models: vec!["gpt-5.5".to_string(), "gpt-5.4".to_string()],
            model_mappings: vec![
                ModelMapping {
                    source: "gpt-5.5".to_string(),
                    target: "deepseek-v4-pro".to_string(),
                },
                ModelMapping {
                    source: "gpt-5.4".to_string(),
                    target: "deepseek-flash".to_string(),
                },
            ],
            balance_query: BalanceQueryConfig::default(),
            balance_status: None,
            connection_status: None,
        };
        let body = br#"{"model":"gpt-5.5","input":"hello"}"#;
        let mapped = mapped_model_for_provider(&provider, "gpt-5.5");
        let rewritten = body_with_provider_overrides(body, mapped.as_deref(), None);
        let value = serde_json::from_slice::<Value>(&rewritten).expect("mapped body is json");

        assert_eq!(mapped.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(
            value.get("model").and_then(Value::as_str),
            Some("deepseek-v4-pro")
        );
        assert_eq!(model_from_request_body(body), "gpt-5.5");
        assert!(provider_accepts_model(&provider, "gpt-5.5"));
        assert!(!provider_accepts_model(&provider, "gpt-5.3"));
    }

    #[test]
    fn preserves_advanced_reasoning_effort_for_responses_providers() {
        let provider = test_provider_config("provider-a", ProviderStatus::Enabled, true);

        for effort in ["max", "ultra"] {
            let body = json!({
                "model": "gpt-5.6-sol",
                "input": "hello",
                "reasoning": { "effort": effort }
            })
            .to_string();
            let prepared = prepare_upstream_request(
                &provider,
                "responses",
                "",
                body.as_bytes(),
                "gpt-5.6-sol",
            )
            .expect("request prepares");
            let value =
                serde_json::from_slice::<Value>(&prepared.body).expect("prepared body is json");

            assert_eq!(
                value.pointer("/reasoning/effort").and_then(Value::as_str),
                Some(effort)
            );
            assert_eq!(
                reasoning_effort_from_request_body(body.as_bytes()).as_deref(),
                Some(effort)
            );
            assert!(provider_supports_reasoning_effort(&provider, Some(effort)));
        }
    }

    #[test]
    fn advanced_reasoning_requires_responses_provider() {
        let mut provider = test_provider_config("provider-a", ProviderStatus::Enabled, true);
        provider.wire_api = ProviderWireApi::ChatCompletions;

        assert!(!provider_supports_reasoning_effort(&provider, Some("max")));
        assert!(!provider_supports_reasoning_effort(
            &provider,
            Some("ultra")
        ));
        assert!(provider_supports_reasoning_effort(&provider, Some("xhigh")));
        assert!(provider_supports_reasoning_effort(&provider, None));
    }

    #[test]
    fn normalizes_model_mapping_drafts() {
        let mappings = normalize_model_mappings(vec![
            ModelMapping {
                source: " gpt-5.5 ".to_string(),
                target: " deepseek-v4-pro ".to_string(),
            },
            ModelMapping {
                source: "GPT-5.5".to_string(),
                target: "duplicate".to_string(),
            },
            ModelMapping {
                source: "gpt-5.4".to_string(),
                target: String::new(),
            },
        ]);

        assert_eq!(
            mappings,
            vec![ModelMapping {
                source: "gpt-5.5".to_string(),
                target: "deepseek-v4-pro".to_string(),
            }]
        );
    }

    #[test]
    fn normalizes_allowed_model_drafts() {
        let models = normalize_model_names(vec![
            " gpt-5.5 ".to_string(),
            "GPT-5.5".to_string(),
            "gpt-5.4".to_string(),
            String::new(),
        ]);

        assert_eq!(models, vec!["gpt-5.5", "gpt-5.4"]);
    }

    #[test]
    fn configured_route_models_merge_allowed_models_in_order() {
        let candidates = vec![
            UpstreamCandidate {
                provider: ProviderConfig {
                    id: "provider-a".to_string(),
                    name: "Provider A".to_string(),
                    status: ProviderStatus::Enabled,
                    enabled: true,
                    consecutive_failure_count: 0,
                    auto_disabled_day: None,
                    last_failure_reason: None,
                    last_failure_at_ms: None,
                    config: json!({}),
                    wire_api: ProviderWireApi::Responses,
                    service_tier: String::new(),
                    connection_test_model: String::new(),
                    allowed_models: vec![
                        "gpt-5.5".to_string(),
                        "gpt-image-2".to_string(),
                        "gpt-5.4".to_string(),
                    ],
                    model_mappings: Vec::new(),
                    balance_query: BalanceQueryConfig::default(),
                    balance_status: None,
                    connection_status: None,
                },
                base_url: "https://example-a.com/v1".to_string(),
                token: "token-a".to_string(),
                route_order: 1,
            },
            UpstreamCandidate {
                provider: ProviderConfig {
                    id: "provider-b".to_string(),
                    name: "Provider B".to_string(),
                    status: ProviderStatus::Enabled,
                    enabled: true,
                    consecutive_failure_count: 0,
                    auto_disabled_day: None,
                    last_failure_reason: None,
                    last_failure_at_ms: None,
                    config: json!({}),
                    wire_api: ProviderWireApi::ChatCompletions,
                    service_tier: String::new(),
                    connection_test_model: String::new(),
                    allowed_models: vec!["GPT-5.4".to_string(), "gpt-5.3".to_string()],
                    model_mappings: Vec::new(),
                    balance_query: BalanceQueryConfig::default(),
                    balance_status: None,
                    connection_status: None,
                },
                base_url: "https://example-b.com/v1".to_string(),
                token: "token-b".to_string(),
                route_order: 2,
            },
        ];

        assert_eq!(
            configured_route_models(&candidates),
            vec!["gpt-5.5", "gpt-5.4", "gpt-5.3"]
        );
    }

    #[test]
    fn maps_allowed_responses_model_before_chat_completions_forwarding() {
        let provider = ProviderConfig {
            id: "provider-b".to_string(),
            name: "Provider B".to_string(),
            status: ProviderStatus::Enabled,
            enabled: true,
            consecutive_failure_count: 0,
            auto_disabled_day: None,
            last_failure_reason: None,
            last_failure_at_ms: None,
            config: json!({}),
            wire_api: ProviderWireApi::ChatCompletions,
            service_tier: String::new(),
            connection_test_model: String::new(),
            allowed_models: vec!["gpt-5.2".to_string()],
            model_mappings: vec![ModelMapping {
                source: "gpt-5.2".to_string(),
                target: "glm-5.2".to_string(),
            }],
            balance_query: BalanceQueryConfig::default(),
            balance_status: None,
            connection_status: None,
        };
        let body = br#"{"model":"gpt-5.2","input":"hello"}"#;

        let prepared = prepare_upstream_request(&provider, "responses", "", body, "gpt-5.2")
            .expect("request prepares");
        let value = serde_json::from_slice::<Value>(&prepared.body).expect("prepared body is json");

        assert!(provider_accepts_model(&provider, "gpt-5.2"));
        assert_eq!(prepared.path, "chat/completions");
        assert_eq!(prepared.upstream_model.as_deref(), Some("glm-5.2"));
        assert_eq!(value.get("model").and_then(Value::as_str), Some("glm-5.2"));
    }

    #[test]
    fn forces_provider_service_tier_on_forwarded_requests() {
        let mut provider = ProviderConfig {
            id: "provider-a".to_string(),
            name: "Provider A".to_string(),
            status: ProviderStatus::Enabled,
            enabled: true,
            consecutive_failure_count: 0,
            auto_disabled_day: None,
            last_failure_reason: None,
            last_failure_at_ms: None,
            config: json!({}),
            wire_api: ProviderWireApi::Responses,
            service_tier: "priority".to_string(),
            connection_test_model: String::new(),
            allowed_models: Vec::new(),
            model_mappings: Vec::new(),
            balance_query: BalanceQueryConfig::default(),
            balance_status: None,
            connection_status: None,
        };
        let body = br#"{"model":"gpt-5.2","input":"hello","service_tier":"default"}"#;

        let prepared = prepare_upstream_request(&provider, "responses", "", body, "gpt-5.2")
            .expect("request prepares");
        let value = serde_json::from_slice::<Value>(&prepared.body).expect("prepared body is json");

        assert_eq!(
            value.get("service_tier").and_then(Value::as_str),
            Some("priority")
        );

        provider.wire_api = ProviderWireApi::ChatCompletions;
        let prepared = prepare_upstream_request(&provider, "responses", "", body, "gpt-5.2")
            .expect("chat request prepares");
        let value = serde_json::from_slice::<Value>(&prepared.body).expect("prepared body is json");

        assert_eq!(
            value.get("service_tier").and_then(Value::as_str),
            Some("priority")
        );
        assert_eq!(prepared.path, "chat/completions");
    }

    #[test]
    fn converts_responses_request_to_chat_completions() {
        let body = json!({
            "model": "gpt-5.5",
            "instructions": "Be concise.",
            "input": "hello",
            "max_output_tokens": 7,
            "stream": true,
            "tools": [{
                "type": "function",
                "name": "lookup",
                "description": "Lookup data",
                "parameters": { "type": "object", "properties": {} }
            }]
        })
        .to_string();

        let (converted, _) =
            responses_to_chat_request_body(body.as_bytes(), Some("deepseek-chat"), None)
                .expect("request converts");
        let value = serde_json::from_slice::<Value>(&converted).expect("converted json");

        assert_eq!(
            value.get("model").and_then(Value::as_str),
            Some("deepseek-chat")
        );
        assert_eq!(value.get("max_tokens").and_then(Value::as_i64), Some(7));
        assert_eq!(
            value.pointer("/messages/0/role").and_then(Value::as_str),
            Some("system")
        );
        assert_eq!(
            value.pointer("/messages/1/role").and_then(Value::as_str),
            Some("user")
        );
        assert_eq!(
            value.pointer("/tools/0/type").and_then(Value::as_str),
            Some("function")
        );
        assert_eq!(
            value
                .pointer("/stream_options/include_usage")
                .and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn maps_responses_developer_role_for_chat_completions() {
        let body = json!({
            "model": "gpt-5.5",
            "input": [{
                "type": "message",
                "role": "developer",
                "content": "Follow project instructions."
            }]
        })
        .to_string();

        let (converted, _) =
            responses_to_chat_request_body(body.as_bytes(), Some("deepseek-chat"), None)
                .expect("request converts");
        let value = serde_json::from_slice::<Value>(&converted).expect("converted json");

        assert_eq!(
            value.pointer("/messages/0/role").and_then(Value::as_str),
            Some("system")
        );
        assert_eq!(
            value.pointer("/messages/0/content").and_then(Value::as_str),
            Some("Follow project instructions.")
        );
    }

    #[test]
    fn converts_responses_custom_tool_to_chat_function() {
        let body = json!({
            "model": "gpt-5.5",
            "input": "edit",
            "tool_choice": { "type": "custom", "name": "apply_patch" },
            "parallel_tool_calls": true,
            "tools": [{
                "type": "custom",
                "name": "apply_patch",
                "description": "Apply a patch",
                "format": { "type": "grammar", "syntax": "lark", "definition": "start: /.+/" }
            }]
        })
        .to_string();

        let (converted, context) =
            responses_to_chat_request_body(body.as_bytes(), Some("deepseek-chat"), None)
                .expect("request converts");
        let value = serde_json::from_slice::<Value>(&converted).expect("converted json");

        assert_eq!(
            value.pointer("/tools/0/type").and_then(Value::as_str),
            Some("function")
        );
        assert_eq!(
            value
                .pointer("/tools/0/function/name")
                .and_then(Value::as_str),
            Some("apply_patch")
        );
        assert_eq!(
            value
                .pointer("/tools/0/function/parameters/properties/input/type")
                .and_then(Value::as_str),
            Some("string")
        );
        assert_eq!(
            value
                .pointer("/tool_choice/function/name")
                .and_then(Value::as_str),
            Some("apply_patch")
        );
        assert!(context.is_custom_tool_chat_name("apply_patch"));
    }

    #[test]
    fn converts_chat_completion_response_to_responses() {
        let body = json!({
            "id": "chatcmpl-1",
            "created": 123,
            "model": "deepseek-chat",
            "choices": [{
                "message": { "role": "assistant", "content": "你好" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "prompt_tokens_details": { "cached_tokens": 4 },
                "completion_tokens": 3,
                "completion_tokens_details": { "reasoning_tokens": 1 },
                "total_tokens": 13
            }
        })
        .to_string();

        let converted =
            chat_completion_to_responses_bytes(body.as_bytes(), &CodexToolContext::default())
                .expect("response converts");
        let value = serde_json::from_slice::<Value>(&converted).expect("converted json");

        assert_eq!(
            value.get("object").and_then(Value::as_str),
            Some("response")
        );
        assert_eq!(
            value.get("output_text").and_then(Value::as_str),
            Some("你好")
        );
        assert_eq!(
            value.pointer("/usage/input_tokens").and_then(Value::as_i64),
            Some(10)
        );
        assert_eq!(
            value
                .pointer("/usage/input_tokens_details/cached_tokens")
                .and_then(Value::as_i64),
            Some(4)
        );
    }

    #[test]
    fn restores_chat_function_call_to_custom_tool_call() {
        let request = json!({
            "model": "gpt-5.5",
            "input": "edit",
            "tools": [{ "type": "custom", "name": "apply_patch" }]
        });
        let context = build_codex_tool_context_from_request(&request);
        let body = json!({
            "id": "chatcmpl-1",
            "created": 123,
            "model": "deepseek-chat",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_patch",
                        "type": "function",
                        "function": {
                            "name": "apply_patch",
                            "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })
        .to_string();

        let converted = chat_completion_to_responses_bytes(body.as_bytes(), &context)
            .expect("response converts");
        let value = serde_json::from_slice::<Value>(&converted).expect("converted json");

        assert_eq!(
            value.pointer("/output/0/type").and_then(Value::as_str),
            Some("custom_tool_call")
        );
        assert_eq!(
            value.pointer("/output/0/name").and_then(Value::as_str),
            Some("apply_patch")
        );
        assert_eq!(
            value.pointer("/output/0/input").and_then(Value::as_str),
            Some("*** Begin Patch\n*** End Patch")
        );
    }

    #[test]
    fn preserves_reasoning_content_for_chat_tool_call_round_trip() {
        let body = json!({
            "id": "chatcmpl-1",
            "created": 123,
            "model": "deepseek-chat",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "reasoning_content": "I need to call a tool.",
                    "tool_calls": [{
                        "id": "call_lookup",
                        "type": "function",
                        "function": {
                            "name": "lookup",
                            "arguments": "{\"query\":\"weather\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })
        .to_string();

        let converted =
            chat_completion_to_responses_bytes(body.as_bytes(), &CodexToolContext::default())
                .expect("response converts");
        let response = serde_json::from_slice::<Value>(&converted).expect("converted json");

        assert_eq!(
            response.pointer("/output/0/type").and_then(Value::as_str),
            Some("reasoning")
        );
        let encrypted_reasoning = response
            .pointer("/output/0/encrypted_content")
            .and_then(Value::as_str)
            .expect("reasoning item stores content");
        assert_eq!(
            local_reasoning_from_encrypted_content(encrypted_reasoning).as_deref(),
            Some("I need to call a tool.")
        );
        assert_eq!(
            response
                .pointer("/output/1/reasoning_content")
                .and_then(Value::as_str),
            Some("I need to call a tool.")
        );

        let next_request = json!({
            "model": "gpt-5.5",
            "input": response.get("output").cloned().unwrap_or_else(|| json!([]))
        })
        .to_string();
        let (converted_request, _) =
            responses_to_chat_request_body(next_request.as_bytes(), Some("deepseek-chat"), None)
                .expect("request converts");
        let request = serde_json::from_slice::<Value>(&converted_request).expect("request json");

        assert_eq!(
            request
                .pointer("/messages/0/reasoning_content")
                .and_then(Value::as_str),
            Some("I need to call a tool.")
        );
        assert_eq!(
            request
                .pointer("/messages/0/tool_calls/0/id")
                .and_then(Value::as_str),
            Some("call_lookup")
        );
    }

    #[test]
    fn applies_local_reasoning_item_to_next_chat_tool_call() {
        let body = json!({
            "model": "gpt-5.5",
            "input": [
                {
                    "type": "reasoning",
                    "summary": [],
                    "content": null,
                    "encrypted_content": local_reasoning_encrypted_content("Need a tool.")
                },
                {
                    "type": "function_call",
                    "call_id": "call_lookup",
                    "name": "lookup",
                    "arguments": "{\"query\":\"weather\"}"
                }
            ]
        })
        .to_string();

        let (converted, _) =
            responses_to_chat_request_body(body.as_bytes(), Some("deepseek-chat"), None)
                .expect("request converts");
        let request = serde_json::from_slice::<Value>(&converted).expect("request json");

        assert_eq!(
            request
                .pointer("/messages/0/reasoning_content")
                .and_then(Value::as_str),
            Some("Need a tool.")
        );
        assert_eq!(
            request
                .pointer("/messages/0/tool_calls/0/id")
                .and_then(Value::as_str),
            Some("call_lookup")
        );
    }

    #[test]
    fn adds_deepseek_reasoning_fallback_for_legacy_tool_call_history() {
        let body = json!({
            "model": "gpt-5.5",
            "input": [{
                "type": "function_call",
                "call_id": "call_lookup",
                "name": "lookup",
                "arguments": "{\"query\":\"weather\"}"
            }]
        })
        .to_string();

        let (converted, _) =
            responses_to_chat_request_body(body.as_bytes(), Some("deepseek-chat"), None)
                .expect("request converts");
        let request = serde_json::from_slice::<Value>(&converted).expect("request json");

        assert_eq!(
            request
                .pointer("/messages/0/reasoning_content")
                .and_then(Value::as_str),
            Some(MISSING_REASONING_CONTENT_FALLBACK)
        );

        let (converted, _) = responses_to_chat_request_body(body.as_bytes(), Some("glm-5.2"), None)
            .expect("request converts");
        let request = serde_json::from_slice::<Value>(&converted).expect("request json");
        assert!(request.pointer("/messages/0/reasoning_content").is_none());
    }

    #[test]
    fn merges_consecutive_responses_tool_calls_for_chat_history() {
        let body = json!({
            "model": "gpt-5.5",
            "input": [
                {
                    "type": "reasoning",
                    "encrypted_content": local_reasoning_encrypted_content("Need both tools.")
                },
                {
                    "type": "function_call",
                    "call_id": "call_one",
                    "name": "lookup",
                    "arguments": "{\"query\":\"one\"}"
                },
                {
                    "type": "function_call",
                    "call_id": "call_two",
                    "name": "lookup",
                    "arguments": "{\"query\":\"two\"}"
                }
            ]
        })
        .to_string();

        let (converted, _) =
            responses_to_chat_request_body(body.as_bytes(), Some("deepseek-chat"), None)
                .expect("request converts");
        let request = serde_json::from_slice::<Value>(&converted).expect("request json");

        assert_eq!(
            request
                .pointer("/messages/0/reasoning_content")
                .and_then(Value::as_str),
            Some("Need both tools.")
        );
        assert_eq!(
            request
                .pointer("/messages/0/tool_calls")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            request
                .get("messages")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn converts_namespace_and_tool_search_tools() {
        let body = json!({
            "model": "gpt-5.5",
            "input": "search",
            "tool_choice": { "type": "function", "namespace": "web", "name": "open" },
            "tools": [
                {
                    "type": "namespace",
                    "name": "web",
                    "tools": [{
                        "type": "function",
                        "name": "open",
                        "description": "Open URL",
                        "parameters": { "type": "object", "properties": { "url": { "type": "string" } } }
                    }]
                },
                { "type": "tool_search" }
            ]
        })
        .to_string();

        let (converted, context) =
            responses_to_chat_request_body(body.as_bytes(), None, None).expect("request converts");
        let value = serde_json::from_slice::<Value>(&converted).expect("converted json");

        assert_eq!(
            value
                .pointer("/tools/0/function/name")
                .and_then(Value::as_str),
            Some("web__open")
        );
        assert_eq!(
            value
                .pointer("/tools/1/function/name")
                .and_then(Value::as_str),
            Some(TOOL_SEARCH_PROXY_NAME)
        );
        assert_eq!(
            value
                .pointer("/tool_choice/function/name")
                .and_then(Value::as_str),
            Some("web__open")
        );
        assert_eq!(
            context
                .lookup_chat_name("web__open")
                .and_then(|spec| spec.namespace.as_deref()),
            Some("web")
        );
    }

    #[test]
    fn drops_tool_choice_when_no_chat_tools_survive() {
        let body = json!({
            "model": "gpt-5.5",
            "input": "hello",
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "tools": [{ "type": "unsupported", "name": "ignored" }]
        })
        .to_string();

        let (converted, _) = responses_to_chat_request_body(body.as_bytes(), None, None)
            .expect("unsupported tools are filtered");
        let value = serde_json::from_slice::<Value>(&converted).expect("converted json");

        assert!(value.get("tools").is_none());
        assert!(value.get("tool_choice").is_none());
        assert!(value.get("parallel_tool_calls").is_none());
    }

    #[test]
    fn restores_namespace_and_tool_search_calls() {
        let request = json!({
            "model": "gpt-5.5",
            "input": "search",
            "tools": [
                {
                    "type": "namespace",
                    "name": "web",
                    "tools": [{ "type": "function", "name": "open", "parameters": { "type": "object" } }]
                },
                { "type": "tool_search" }
            ]
        });
        let context = build_codex_tool_context_from_request(&request);
        let body = json!({
            "id": "chatcmpl-1",
            "created": 123,
            "model": "deepseek-chat",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_open",
                            "type": "function",
                            "function": { "name": "web__open", "arguments": "{\"url\":\"https://example.com\"}" }
                        },
                        {
                            "id": "call_search",
                            "type": "function",
                            "function": { "name": "tool_search", "arguments": "{\"query\":\"gmail\",\"limit\":3}" }
                        }
                    ]
                },
                "finish_reason": "tool_calls"
            }]
        })
        .to_string();

        let converted = chat_completion_to_responses_bytes(body.as_bytes(), &context)
            .expect("response converts");
        let value = serde_json::from_slice::<Value>(&converted).expect("converted json");

        assert_eq!(
            value.pointer("/output/0/type").and_then(Value::as_str),
            Some("function_call")
        );
        assert_eq!(
            value.pointer("/output/0/namespace").and_then(Value::as_str),
            Some("web")
        );
        assert_eq!(
            value.pointer("/output/0/name").and_then(Value::as_str),
            Some("open")
        );
        assert_eq!(
            value.pointer("/output/1/type").and_then(Value::as_str),
            Some("tool_search_call")
        );
        assert_eq!(
            value
                .pointer("/output/1/arguments/query")
                .and_then(Value::as_str),
            Some("gmail")
        );
    }

    #[test]
    fn converts_chat_stream_chunk_to_responses_sse() {
        let chunk = b"data: {\"id\":\"chatcmpl-1\",\"created\":123,\"model\":\"deepseek-chat\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n";
        let usage_chunk = b"data: {\"id\":\"chatcmpl-1\",\"created\":123,\"model\":\"deepseek-chat\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n";
        let mut state = ChatStreamTestState::new(CodexToolContext::default());

        let first = state.ingest(chunk);
        let second = state.ingest(usage_chunk);
        let text = first
            .into_iter()
            .chain(second)
            .map(|bytes| String::from_utf8(bytes.to_vec()).expect("utf8"))
            .collect::<String>();

        assert!(text.contains("response.output_text.delta"));
        assert!(text.contains("\"delta\":\"hi\""));
        assert!(text.contains("\"text\":\"hi\""));
        assert!(text.contains("response.completed"));
        assert_eq!(state.usage.input_tokens, 2);
        assert_eq!(state.usage.output_tokens, 1);
    }

    #[test]
    fn waits_for_late_chat_stream_usage_before_completed() {
        let text_chunk = b"data: {\"id\":\"chatcmpl-1\",\"created\":123,\"model\":\"deepseek-chat\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n";
        let finish_chunk = b"data: {\"id\":\"chatcmpl-1\",\"created\":123,\"model\":\"deepseek-chat\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
        let usage_chunk = b"data: {\"id\":\"chatcmpl-1\",\"created\":123,\"model\":\"deepseek-chat\",\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n";
        let mut state = ChatStreamTestState::new(CodexToolContext::default());

        let first = state.ingest(text_chunk);
        let second = state.ingest(finish_chunk);
        let third = state.ingest(usage_chunk);
        let before_usage = first
            .into_iter()
            .chain(second)
            .map(|bytes| String::from_utf8(bytes.to_vec()).expect("utf8"))
            .collect::<String>();
        let after_usage = third
            .into_iter()
            .map(|bytes| String::from_utf8(bytes.to_vec()).expect("utf8"))
            .collect::<String>();

        assert!(!before_usage.contains("response.completed"));
        assert!(after_usage.contains("response.completed"));
        assert!(after_usage.contains("\"total_tokens\":3"));
        assert_eq!(state.usage.total_tokens, 3);
    }

    #[test]
    fn restores_streamed_chat_function_call_to_custom_tool_call() {
        let request = json!({
            "model": "gpt-5.5",
            "input": "edit",
            "tools": [{ "type": "custom", "name": "apply_patch" }]
        });
        let context = build_codex_tool_context_from_request(&request);
        let tool_start = b"data: {\"id\":\"chatcmpl-1\",\"created\":123,\"model\":\"deepseek-chat\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_patch\",\"type\":\"function\",\"function\":{\"name\":\"apply_patch\"}}]},\"finish_reason\":null}]}\n\n";
        let tool_args = b"data: {\"id\":\"chatcmpl-1\",\"created\":123,\"model\":\"deepseek-chat\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"input\\\":\\\"patch text\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n";
        let mut state = ChatStreamTestState::new(context);

        let first = state.ingest(tool_start);
        let second = state.ingest(tool_args);
        let text = first
            .into_iter()
            .chain(second)
            .map(|bytes| String::from_utf8(bytes.to_vec()).expect("utf8"))
            .collect::<String>();

        assert!(text.contains("\"type\":\"custom_tool_call\""));
        assert!(text.contains("response.custom_tool_call_input.done"));
        assert!(text.contains("\"input\":\"patch text\""));
        assert!(!text.contains("response.function_call_arguments.delta"));
        assert!(text.contains("response.completed"));
    }

    #[test]
    fn preserves_streamed_reasoning_content_for_chat_tool_calls() {
        let reasoning_start = b"data: {\"id\":\"chatcmpl-1\",\"created\":123,\"model\":\"deepseek-chat\",\"choices\":[{\"delta\":{\"reasoning_content\":\"I should \"},\"finish_reason\":null}]}\n\n";
        let tool_start = b"data: {\"id\":\"chatcmpl-1\",\"created\":123,\"model\":\"deepseek-chat\",\"choices\":[{\"delta\":{\"reasoning_content\":\"look it up.\",\"tool_calls\":[{\"index\":0,\"id\":\"call_lookup\",\"type\":\"function\",\"function\":{\"name\":\"lookup\"}}]},\"finish_reason\":null}]}\n\n";
        let tool_args = b"data: {\"id\":\"chatcmpl-1\",\"created\":123,\"model\":\"deepseek-chat\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"query\\\":\\\"weather\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n";
        let mut state = ChatStreamTestState::new(CodexToolContext::default());

        let first = state.ingest(reasoning_start);
        let second = state.ingest(tool_start);
        let third = state.ingest(tool_args);
        let text = first
            .into_iter()
            .chain(second)
            .chain(third)
            .map(|bytes| String::from_utf8(bytes.to_vec()).expect("utf8"))
            .collect::<String>();

        assert!(text.contains("\"type\":\"reasoning\""));
        assert!(text.contains(LOCAL_REASONING_ENCRYPTED_PREFIX));
        assert!(text.contains("\"reasoning_content\":\"I should look it up.\""));
        assert!(text.contains("response.output_item.done"));
        assert!(text.contains("response.completed"));
    }

    #[test]
    fn connection_status_latency_uses_remote_access_latency() {
        let result = ProviderConnectionTestResult {
            ok: false,
            steps: vec![
                status_step("base", "基础连接", "ok", Some(180), "上游可访问"),
                status_step("models", "模型接口", "ok", Some(180), "鉴权通过"),
                status_step(
                    "model",
                    "模型可用性",
                    "failed",
                    None,
                    "模型不在 /models 列表中: test-model",
                ),
            ],
        };

        let status = connection_status_from_test(&result);

        assert_eq!(status.latency_ms, Some(180));
        assert_eq!(
            status.error.as_deref(),
            Some("模型不在 /models 列表中: test-model")
        );
    }

    #[test]
    fn latency_error_message_prefers_upstream_json_message() {
        let message = latency_error_message(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"model is unavailable"}}"#,
        );

        assert_eq!(message, "模型请求返回 HTTP 400: model is unavailable");
    }

    #[test]
    fn latency_connection_status_records_success_and_failure() {
        let success = latency_connection_status(Ok(245));
        assert!(success.ok);
        assert_eq!(success.latency_ms, Some(245));
        assert!(success.error.is_none());

        let failure = latency_connection_status(Err("timeout".to_string()));
        assert!(!failure.ok);
        assert_eq!(failure.latency_ms, None);
        assert_eq!(failure.error.as_deref(), Some("timeout"));
    }

    #[test]
    fn parses_synchronous_responses_reply() {
        let body = r#"{
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "Hi there"}]
            }]
        }"#;

        assert_eq!(
            reply_from_response_text(body, LatencyResponseFormat::Responses, false),
            "Hi there"
        );
    }

    #[test]
    fn parses_chat_completion_stream_reply() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" there\"}}]}\n\n",
            "data: [DONE]\n\n"
        );

        assert_eq!(
            reply_from_response_text(body, LatencyResponseFormat::ChatCompletions, true),
            "Hi there"
        );
    }

    #[test]
    fn parses_claude_stream_reply() {
        let body = concat!(
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"!\"}}\n\n"
        );

        assert_eq!(
            reply_from_response_text(body, LatencyResponseFormat::ClaudeMessages, true),
            "Hello!"
        );
    }

    #[test]
    fn parses_model_ids_from_models_response() {
        let models = models_from_response_value(&json!({
            "data": [
                { "id": "gpt-5" },
                { "id": " gpt-5-mini " },
                { "id": "gpt-5" },
                { "object": "model" }
            ]
        }));

        assert_eq!(models, vec!["gpt-5", "gpt-5-mini"]);
    }

    #[test]
    fn parses_model_ids_from_compat_models_response() {
        let models = models_from_response_value(&json!({
            "models": ["doubao-seed-1-6", " doubao-seed-1-6-thinking ", ""]
        }));

        assert_eq!(models, vec!["doubao-seed-1-6", "doubao-seed-1-6-thinking"]);
    }

    #[test]
    fn parses_claude_model_objects_from_models_response() {
        let models = claude_model_values_from_response_value(&json!({
            "data": [
                {
                    "id": "claude-fable-5",
                    "type": "model",
                    "display_name": "Claude Fable 5",
                    "created_at": "2026-01-01T00:00:00Z"
                },
                { "id": "claude-fable-5" },
                "claude-sonnet-5"
            ],
            "has_more": false
        }));

        let ids = models
            .iter()
            .filter_map(model_id_from_value)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["claude-fable-5", "claude-sonnet-5"]);
        assert_eq!(
            models[0].get("display_name").and_then(Value::as_str),
            Some("Claude Fable 5")
        );
    }

    #[test]
    fn claude_route_models_respect_allowed_models() {
        let mut provider = ClaudeProviderConfig {
            id: "anthropic".to_string(),
            name: "Anthropic".to_string(),
            status: ProviderStatus::Enabled,
            enabled: true,
            consecutive_failure_count: 0,
            auto_disabled_day: None,
            last_failure_reason: None,
            last_failure_at_ms: None,
            base_url: "https://api.anthropic.com".to_string(),
            api_key: "sk-ant".to_string(),
            connection_test_model: String::new(),
            allowed_models: vec!["claude-fable-5".to_string(), "local-alias".to_string()],
            model_mappings: vec![ModelMapping {
                source: "local-alias".to_string(),
                target: "claude-sonnet-5".to_string(),
            }],
            connection_status: None,
        };
        let upstream = claude_model_values_from_response_value(&json!({
            "data": [
                { "id": "claude-sonnet-5" },
                { "id": "claude-fable-5", "display_name": "Claude Fable 5" }
            ]
        }));

        let models = claude_route_model_values(&provider, &upstream);
        let ids = models
            .iter()
            .filter_map(model_id_from_value)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["claude-fable-5", "local-alias"]);
        assert_eq!(
            models[0].get("display_name").and_then(Value::as_str),
            Some("Claude Fable 5")
        );

        provider.allowed_models.clear();
        let models = claude_route_model_values(&provider, &upstream);
        let ids = models
            .iter()
            .filter_map(model_id_from_value)
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["claude-sonnet-5", "claude-fable-5", "local-alias"]
        );
    }

    #[test]
    fn parses_model_ids_from_codex_catalog_response() {
        let models = models_from_response_value(&json!({
            "models": [
                { "slug": "gpt-5.5" },
                { "slug": " gpt-5.4 " },
                { "id": "legacy-id" },
                { "object": "model" }
            ]
        }));

        assert_eq!(models, vec!["gpt-5.4", "gpt-5.5", "legacy-id"]);
    }

    #[test]
    fn detects_codex_model_catalog_requests() {
        assert!(codex_model_catalog_requested(Some(
            "client_version=0.142.1"
        )));
        assert!(codex_model_catalog_requested(Some(
            "foo=bar&client_version=1"
        )));
        assert!(!codex_model_catalog_requested(None));
        assert!(!codex_model_catalog_requested(Some("limit=100")));
    }

    #[test]
    fn codex_catalog_restores_default_context_fields() {
        let template = json!({
            "slug": "gpt-5.4",
            "display_name": "GPT-5.4",
            "description": "template",
            "base_instructions": "keep template instructions",
            "context_window": 272000,
            "max_context_window": 1000000,
            "auto_compact_token_limit": 950000,
            "effective_context_window_percent": 95,
            "supported_in_api": true,
            "visibility": "list"
        });
        let catalog =
            codex_models_catalog_value_with_templates(vec!["gpt-5.5".to_string()], &[template]);
        let model = catalog
            .pointer("/models/0")
            .and_then(Value::as_object)
            .expect("catalog model");

        assert_eq!(model.get("slug").and_then(Value::as_str), Some("gpt-5.5"));
        assert_eq!(
            model.get("base_instructions").and_then(Value::as_str),
            Some("keep template instructions")
        );
        assert_eq!(
            model.get("context_window").and_then(Value::as_i64),
            Some(256_000)
        );
        assert_eq!(
            model.get("max_context_window").and_then(Value::as_i64),
            Some(256_000)
        );
        assert_eq!(
            model
                .get("auto_compact_token_limit")
                .and_then(Value::as_i64),
            Some(243_200)
        );
        assert_eq!(
            model
                .get("effective_context_window_percent")
                .and_then(Value::as_i64),
            Some(95)
        );
    }

    #[test]
    fn codex_catalog_applies_gpt_5_6_variant_profiles() {
        let template = json!({
            "slug": "gpt-5.5",
            "display_name": "GPT-5.5",
            "supported_reasoning_levels": [
                { "effort": "low", "description": "template low" },
                { "effort": "medium", "description": "template medium" },
                { "effort": "high", "description": "template high" },
                { "effort": "xhigh", "description": "template xhigh" }
            ]
        });

        let max_and_ultra = ["low", "medium", "high", "xhigh", "max", "ultra"];
        let max_only = ["low", "medium", "high", "xhigh", "max"];
        for (
            model_name,
            expected_efforts,
            expected_display_name,
            expected_default_effort,
            expected_multi_agent_version,
        ) in [
            (
                "gpt-5.6",
                max_and_ultra.as_slice(),
                "GPT-5.6",
                "medium",
                "v2",
            ),
            (
                "gpt-5.6-sol",
                max_and_ultra.as_slice(),
                "GPT-5.6-Sol",
                "low",
                "v2",
            ),
            (
                "GPT-5.6-TERRA",
                max_and_ultra.as_slice(),
                "GPT-5.6-Terra",
                "medium",
                "v2",
            ),
            (
                "gpt-5.6-luna",
                max_only.as_slice(),
                "GPT-5.6-Luna",
                "medium",
                "v1",
            ),
        ] {
            let catalog = codex_models_catalog_value_with_templates(
                vec![model_name.to_string()],
                std::slice::from_ref(&template),
            );
            let levels = catalog
                .pointer("/models/0/supported_reasoning_levels")
                .and_then(Value::as_array)
                .expect("reasoning levels");
            let efforts = levels
                .iter()
                .filter_map(|level| level.get("effort").and_then(Value::as_str))
                .collect::<Vec<_>>();

            assert_eq!(efforts, expected_efforts);
            assert_eq!(
                levels
                    .first()
                    .and_then(|level| level.get("description"))
                    .and_then(Value::as_str),
                Some("template low")
            );
            assert_eq!(
                catalog
                    .pointer("/models/0/display_name")
                    .and_then(Value::as_str),
                Some(expected_display_name)
            );
            assert_eq!(
                catalog
                    .pointer("/models/0/default_reasoning_level")
                    .and_then(Value::as_str),
                Some(expected_default_effort)
            );
            assert_eq!(
                catalog
                    .pointer("/models/0/multi_agent_version")
                    .and_then(Value::as_str),
                Some(expected_multi_agent_version)
            );
            assert_eq!(
                catalog
                    .pointer("/models/0/use_responses_lite")
                    .and_then(Value::as_bool),
                Some(true)
            );
            assert_eq!(
                catalog
                    .pointer("/models/0/tool_mode")
                    .and_then(Value::as_str),
                Some("code_mode_only")
            );
        }

        for (model_name, expected_efforts) in [
            ("gpt-5.6-sol", max_and_ultra.as_slice()),
            ("gpt-5.6-luna", max_only.as_slice()),
        ] {
            let fallback_catalog =
                codex_models_catalog_value_with_templates(vec![model_name.to_string()], &[]);
            let fallback_efforts = fallback_catalog
                .pointer("/models/0/supported_reasoning_levels")
                .and_then(Value::as_array)
                .expect("fallback reasoning levels")
                .iter()
                .filter_map(|level| level.get("effort").and_then(Value::as_str))
                .collect::<Vec<_>>();
            assert_eq!(fallback_efforts, expected_efforts);
        }
    }

    #[test]
    fn codex_catalog_removes_stale_ultra_from_luna_profile() {
        let template = json!({
            "slug": "gpt-5.6-luna",
            "supported_reasoning_levels": [
                { "effort": "low", "description": "low" },
                { "effort": "max", "description": "max" },
                { "effort": "ultra", "description": "stale ultra" },
                { "effort": "future", "description": "future effort" }
            ],
            "multi_agent_version": "v2"
        });
        let catalog = codex_models_catalog_value_with_templates(
            vec!["gpt-5.6-luna".to_string()],
            std::slice::from_ref(&template),
        );
        let efforts = catalog
            .pointer("/models/0/supported_reasoning_levels")
            .and_then(Value::as_array)
            .expect("reasoning levels")
            .iter()
            .filter_map(|level| level.get("effort").and_then(Value::as_str))
            .collect::<Vec<_>>();

        assert_eq!(
            efforts,
            vec!["low", "medium", "high", "xhigh", "max", "future"]
        );
        assert_eq!(
            catalog
                .pointer("/models/0/multi_agent_version")
                .and_then(Value::as_str),
            Some("v1")
        );
    }

    #[test]
    fn codex_catalog_prefers_bundled_templates_over_cached_templates() {
        let bundled = vec![
            json!({
                "slug": "gpt-5.6-sol",
                "template_source": "bundled",
                "multi_agent_version": "v2"
            }),
            json!({
                "slug": "gpt-5.6-terra",
                "template_source": "bundled"
            }),
        ];
        let cached = vec![
            json!({
                "slug": "GPT-5.6-SOL",
                "template_source": "self-poisoned-cache",
                "multi_agent_version": null
            }),
            json!({
                "slug": "custom-model",
                "template_source": "cache"
            }),
        ];

        let merged = merge_codex_model_catalog_templates(bundled, cached);
        assert_eq!(merged.len(), 3);
        let sol = merged
            .iter()
            .find(|entry| {
                entry
                    .get("slug")
                    .and_then(Value::as_str)
                    .is_some_and(|slug| slug.eq_ignore_ascii_case("gpt-5.6-sol"))
            })
            .expect("sol template");
        assert_eq!(
            sol.get("template_source").and_then(Value::as_str),
            Some("bundled")
        );
        assert!(merged
            .iter()
            .any(|entry| { entry.get("slug").and_then(Value::as_str) == Some("custom-model") }));
    }

    #[test]
    fn codex_catalog_deduplicates_existing_advanced_reasoning_levels() {
        let template = json!({
            "slug": "gpt-5.6-sol",
            "supported_reasoning_levels": [
                { "effort": "low", "description": "low" },
                { "effort": "max", "description": "existing max" },
                { "effort": "MAX", "description": "duplicate max" },
                { "effort": "ultra", "description": "existing ultra" }
            ]
        });
        let catalog = codex_models_catalog_value_with_templates(
            vec!["gpt-5.6-sol".to_string()],
            std::slice::from_ref(&template),
        );
        let levels = catalog
            .pointer("/models/0/supported_reasoning_levels")
            .and_then(Value::as_array)
            .expect("reasoning levels");
        let efforts = levels
            .iter()
            .filter_map(|level| level.get("effort").and_then(Value::as_str))
            .collect::<Vec<_>>();

        assert_eq!(
            efforts,
            vec!["low", "medium", "high", "xhigh", "max", "ultra"]
        );
        assert_eq!(
            levels
                .iter()
                .find(|level| level.get("effort").and_then(Value::as_str) == Some("max"))
                .and_then(|level| level.get("description"))
                .and_then(Value::as_str),
            Some("existing max")
        );
    }

    #[test]
    fn codex_catalog_keeps_gpt_5_5_reasoning_levels_unchanged() {
        let template = json!({
            "slug": "gpt-5.5",
            "display_name": "GPT-5.5",
            "supported_reasoning_levels": [
                { "effort": "low", "description": "low" },
                { "effort": "xhigh", "description": "xhigh" }
            ]
        });
        let catalog = codex_models_catalog_value_with_templates(
            vec!["gpt-5.5".to_string()],
            std::slice::from_ref(&template),
        );
        let efforts = catalog
            .pointer("/models/0/supported_reasoning_levels")
            .and_then(Value::as_array)
            .expect("reasoning levels")
            .iter()
            .filter_map(|level| level.get("effort").and_then(Value::as_str))
            .collect::<Vec<_>>();

        assert_eq!(efforts, vec!["low", "xhigh"]);
        assert_eq!(
            catalog
                .pointer("/models/0/display_name")
                .and_then(Value::as_str),
            Some("GPT-5.5")
        );
    }

    #[test]
    fn pi_models_config_migrates_legacy_provider_to_xxswitch() {
        let config = json!({
            "providers": {
                "other": {
                    "baseUrl": "https://other.example/v1",
                    "api": "openai-completions",
                    "models": [{ "id": "other-model" }]
                },
                "codex-helper": {
                    "baseUrl": "https://old.example/v1",
                    "api": "openai-completions",
                    "apiKey": "old-token",
                    "models": [{ "id": "old-model" }]
                }
            },
            "unrelated": true
        });
        let backup = capture_pi_backup(&config);
        let router = RouterConfig {
            enabled: true,
            remote_compaction_enabled: false,
            host: "127.0.0.1".to_string(),
            port: 18080,
            local_token: "local-token".to_string(),
            ..RouterConfig::default()
        };

        let raw = render_pi_models_config(config, &router, &["gpt-5.5".to_string()]).unwrap();
        let patched = serde_json::from_str::<Value>(&raw).unwrap();

        assert_eq!(
            patched
                .pointer("/providers/other/models/0/id")
                .and_then(Value::as_str),
            Some("other-model")
        );
        assert_eq!(
            patched
                .pointer("/providers/xxswitch/baseUrl")
                .and_then(Value::as_str),
            Some("http://127.0.0.1:18080/v1")
        );
        assert_eq!(
            patched
                .pointer("/providers/xxswitch/api")
                .and_then(Value::as_str),
            Some("openai-responses")
        );
        assert_eq!(
            patched
                .pointer("/providers/xxswitch/apiKey")
                .and_then(Value::as_str),
            Some("local-token")
        );
        assert_eq!(
            patched
                .pointer("/providers/xxswitch/models/0/id")
                .and_then(Value::as_str),
            Some("gpt-5.5")
        );
        assert_eq!(
            patched
                .pointer("/providers/xxswitch/models/0/contextWindow")
                .and_then(Value::as_i64),
            Some(256_000)
        );
        assert_eq!(
            patched
                .pointer("/providers/xxswitch/models/0/reasoning")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            patched.pointer("/providers/xxswitch/models/0/thinkingLevelMap"),
            Some(&json!({
                "off": null,
                "minimal": null,
                "low": "low",
                "medium": "medium",
                "high": "high",
                "xhigh": "xhigh"
            }))
        );

        let raw = restore_pi_models_config(patched, Some(&backup)).unwrap();
        let restored = serde_json::from_str::<Value>(&raw).unwrap();
        assert!(restored.pointer("/providers/xxswitch").is_none());
        assert!(restored.pointer("/providers/codex-helper").is_none());
    }

    #[test]
    fn pi_models_config_removes_xxswitch_without_backup() {
        let config = json!({
            "providers": {
                "other": { "models": [{ "id": "other-model" }] },
                "xxswitch": { "models": [{ "id": "generated-model" }] }
            }
        });

        let raw = restore_pi_models_config(config, None).unwrap();
        let restored = serde_json::from_str::<Value>(&raw).unwrap();

        assert!(restored.pointer("/providers/xxswitch").is_none());
        assert_eq!(
            restored
                .pointer("/providers/other/models/0/id")
                .and_then(Value::as_str),
            Some("other-model")
        );
    }
}
