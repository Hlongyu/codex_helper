export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export type BalanceQueryType = "disabled" | "new_api" | "sub2_api" | "ai_gate";
export type NewApiBalanceTarget = "token_quota" | "account_balance";
export type BalanceAuthMode = "provider_token" | "separate_token";
export type ProviderWireApi = "responses" | "chat_completions";
export type ProviderServiceTier = "" | "priority" | "ultrafast";
export type ProviderStatus = "enabled" | "disabled" | "auto_disabled";
export type AutoDisableResetPeriod = "daily" | "weekly" | "monthly" | "never";
export type SlowMode = "off" | "fixed" | "random";

export type BalanceQueryConfig = {
  enabled: boolean;
  query_type: BalanceQueryType;
  new_api_target: NewApiBalanceTarget;
  endpoint: string;
  path: string;
  auth_mode: BalanceAuthMode;
  query_token: string;
  new_api_user_id: string;
  ai_gate_account_id?: number | null;
};

export type AiGateBalanceAccount = {
  account_id: number;
  account_name: string;
  available_balance?: number | null;
  unit: string;
};

export type BalanceStatus = {
  amount?: string | null;
  label: string;
  checked_at?: number | null;
  error?: string | null;
  accounts?: AiGateBalanceAccount[];
};

export type ProviderModelsResponse = {
  models: string[];
};

export type ModelMapping = {
  source: string;
  target: string;
};

export type RouterConfig = {
  enabled: boolean;
  debug_mode: boolean;
  force_disable_openai_auth: boolean;
  remote_compaction_enabled: boolean;
  token_budget_enabled: boolean;
  websockets_enabled: boolean;
  zstd_decompression_enabled: boolean;
  gpt56_long_context_enabled: boolean;
  gpt56_long_context_window: number;
  auto_disable_enabled: boolean;
  auto_disable_failure_threshold: number;
  model_provider: string;
  codex_model: string;
  host: string;
  port: number;
  local_token: string;
  connect_timeout_secs: number;
  response_header_timeout_secs: number;
  stream_idle_timeout_secs: number;
  slow_mode: SlowMode;
  slow_delay_secs: number;
  slow_delay_max_secs: number;
};

export type SkillLocationConfig = {
  path: string;
  writable: boolean;
};

export type ClientTargetConfig = {
  enabled: boolean;
  skill_locations: SkillLocationConfig[];
  managed_skill_location: string;
};

export type ClientConfigs = {
  codex: ClientTargetConfig;
  claude: ClientTargetConfig;
  pi: ClientTargetConfig;
};

export type SkillLocationView = SkillLocationConfig & {
  managed: boolean;
  exists: boolean;
};

export type SkillClientView = {
  client: AgentClientKind;
  label: string;
  managed_skill_location: string;
  skill_locations: SkillLocationView[];
};

export type ClientSkillView = {
  client: AgentClientKind;
  client_label: string;
  skill_location: string;
  path: string;
  dir_name: string;
  identity: string;
  description: string;
  managed: boolean;
  shared: boolean;
};

export type SkillOrigin = {
  client: AgentClientKind;
  skill_location: string;
  original_path: string;
  original_dir_name: string;
};

export type SkillExposureView = {
  client: AgentClientKind;
  client_label: string;
  path: string;
  health: string;
  message: string;
};

export type SharedSkillView = {
  identity: string;
  description: string;
  library_dir_name: string;
  path: string;
  sharing_scope: AgentClientKind[];
  origin?: SkillOrigin | null;
  exposures: SkillExposureView[];
};

export type SkillConflictView = {
  kind: string;
  identity: string;
  client?: AgentClientKind | null;
  path: string;
  message: string;
};

export type SkillManagementView = {
  library_root: string;
  clients: SkillClientView[];
  shared_skills: SharedSkillView[];
  client_skills: ClientSkillView[];
  conflicts: SkillConflictView[];
};

export type RouterStatus = {
  running: boolean;
  address: string;
  error?: string | null;
};

export type ProviderConfig = {
  id: string;
  name: string;
  status: ProviderStatus;
  enabled: boolean;
  fusion_enabled: boolean;
  auto_disable_override: boolean;
  auto_disable_enabled: boolean;
  auto_disable_failure_threshold: number;
  consecutive_failure_count: number;
  auto_disabled_day?: string | null;
  auto_disable_reset_period: AutoDisableResetPeriod;
  auto_disable_reset_weekday: number;
  auto_disable_reset_day: number;
  auto_disable_reset_time: string;
  last_failure_reason?: string | null;
  last_failure_at_ms?: number | null;
  config: JsonValue;
  wire_api: ProviderWireApi;
  image_detail_original_to_auto: boolean;
  supports_websockets: boolean;
  service_tier: string;
  connection_test_model: string;
  allowed_models: string[];
  model_mappings: ModelMapping[];
  balance_query: BalanceQueryConfig;
  balance_status?: BalanceStatus | null;
  connection_status?: unknown;
};

export type ClaudeProviderConfig = {
  id: string;
  name: string;
  status: ProviderStatus;
  enabled: boolean;
  fusion_enabled: boolean;
  auto_disable_override: boolean;
  auto_disable_enabled: boolean;
  auto_disable_failure_threshold: number;
  consecutive_failure_count: number;
  auto_disabled_day?: string | null;
  auto_disable_reset_period: AutoDisableResetPeriod;
  auto_disable_reset_weekday: number;
  auto_disable_reset_day: number;
  auto_disable_reset_time: string;
  last_failure_reason?: string | null;
  last_failure_at_ms?: number | null;
  base_url: string;
  api_key: string;
  connection_test_model: string;
  allowed_models: string[];
  model_mappings: ModelMapping[];
  connection_status?: unknown;
};

export type ProviderSummary = {
  id: string;
  name: string;
  status: ProviderStatus;
  enabled: boolean;
  auto_disable_override: boolean;
  auto_disable_enabled: boolean;
  auto_disable_failure_threshold: number;
  consecutive_failure_count: number;
  auto_disabled_day?: string | null;
  auto_disable_reset_period: AutoDisableResetPeriod;
  auto_disable_reset_weekday: number;
  auto_disable_reset_day: number;
  auto_disable_reset_time: string;
  last_failure_reason?: string | null;
  last_failure_at_ms?: number | null;
  pending_changes: number;
  base_url: string;
  provider_type: string;
  balance_label: string;
  balance_error?: string | null;
  latency_ms?: number | null;
  latency_label: string;
  latency_error?: string | null;
};

export type ProviderLatencyDialogState = {
  provider: ProviderSummary;
  providerKind: ProviderKind;
  models: string[];
  selectedModel: string;
  prompt: string;
  streaming: boolean;
  preparing: boolean;
  testing: boolean;
  result: {
    ok: boolean;
    latencyMs?: number | null;
    message: string;
    reply?: string;
  } | null;
};

export type ProviderLatencyTestResponse = {
  app_state: AppState;
  ok: boolean;
  latency_ms?: number | null;
  error?: string | null;
  reply: string;
};

export type UsageSummary = {
  request_count: number;
  input_tokens: number;
  uncached_input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  reasoning_output_tokens: number;
  total_tokens: number;
};

export type RequestBodyInfo = {
  content_type?: string | null;
  content_encoding?: string | null;
  zstd_detected: boolean;
  decompression_enabled: boolean;
  decompressed: boolean;
  inbound_bytes: number;
  decoded_bytes: number;
  upstream_bytes: number;
};

export type RouteRequestLog = {
  id: string;
  started_at_ms: number;
  day: string;
  hour: string;
  method: string;
  path: string;
  model: string;
  remote_compaction_v2?: {
    trigger_received: boolean;
    trigger_forwarded: boolean;
    compaction_response_received: boolean;
    compaction_response_forwarded: boolean;
    compaction_item_reused: boolean;
  } | null;
  upstream_model?: string | null;
  request_body?: RequestBodyInfo | null;
  requested_service_tier?: string | null;
  actual_service_tier?: string | null;
  provider_id: string;
  provider_name: string;
  provider_order: number;
  upstream_chain: string[];
  status: "success" | "failed" | "running" | "cancelled" | string;
  status_code?: number | null;
  error?: string | null;
  route_result: string;
  route_attempts: number;
  input_tokens: number;
  uncached_input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  reasoning_output_tokens: number;
  total_tokens: number;
  first_byte_ms?: number | null;
  slow_delay_ms?: number | null;
  upstream_started_ms?: number | null;
  response_header_ms?: number | null;
  upstream_request_id?: string | null;
  debug_capture_available: boolean;
  debug_capture_error?: string | null;
  total_ms: number;
};

export type DebugHeader = {
  name: string;
  value: string;
};

export type DebugBodySnapshot = {
  text: string;
  captured_bytes: number;
  total_bytes: number;
  truncated: boolean;
};

export type DebugRequestSnapshot = {
  method: string;
  url: string;
  headers: DebugHeader[];
  body: DebugBodySnapshot;
};

export type DebugResponseSnapshot = {
  status_code?: number | null;
  headers: DebugHeader[];
  body: DebugBodySnapshot;
};

export type RouteDebugCapture = {
  version: number;
  request_id: string;
  started_at_ms: number;
  provider_name: string;
  request_body?: RequestBodyInfo | null;
  inbound_request: DebugRequestSnapshot;
  upstream_request: DebugRequestSnapshot;
  upstream_response: DebugResponseSnapshot;
};

export type RouteLogFilter = {
  query?: string;
  status?: string;
  provider_id?: string;
  provider_name?: string;
  model?: string;
  start_day?: string;
  end_day?: string;
  page?: number;
  page_size?: number;
};

export type RouteLogFilterOption = {
  id: string;
  name: string;
  request_count: number;
};

export type RouteLogsResponse = {
  logs: RouteRequestLog[];
  total: number;
  page: number;
  page_size: number;
  total_pages: number;
  available_providers: RouteLogFilterOption[];
  available_models: string[];
  available_days: string[];
};

export type RouteUsageBucket = {
  label: string;
  request_count: number;
  input_tokens: number;
  uncached_input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  total_tokens: number;
};

export type RouteUsageBreakdown = RouteUsageBucket & {
  key: string;
  label: string;
};

export type RouteUsageStats = {
  generated_at_ms: number;
  filters: RouteLogFilter;
  summary: UsageSummary;
  today: UsageSummary;
  failed_count: number;
  success_count: number;
  running_count: number;
  average_first_byte_ms?: number | null;
  average_total_ms?: number | null;
  bucket_granularity: "hour" | "day" | "month" | string;
  buckets: RouteUsageBucket[];
  providers: RouteUsageBreakdown[];
  models: RouteUsageBreakdown[];
  details: RouteRequestLog[];
  total: number;
  page: number;
  page_size: number;
  total_pages: number;
  available_providers: RouteLogFilterOption[];
  available_models: string[];
  available_days: string[];
};

export type UpdateCheckInfo = {
  current_version: string;
  latest_version: string;
  available: boolean;
  installable: boolean;
  asset_name?: string | null;
  release_url: string;
};

export type UpdateInstallResult = {
  message: string;
  manual_install: boolean;
};

export type AppState = {
  app_version: string;
  codex_config_path: string;
  claude_settings_path: string;
  pi_models_path: string;
  manager_dir: string;
  current_config_raw: string;
  current_config_exists: boolean;
  active_provider_id: string;
  active_claude_provider_id: string;
  providers: ProviderSummary[];
  claude_providers: ProviderSummary[];
  active_provider: ProviderConfig | null;
  active_claude_provider: ClaudeProviderConfig | null;
  active_provider_toml: string;
  final_preview_toml: string;
  diffs: Array<{ path: string; action: string }>;
  router: RouterConfig;
  codex_models: string[];
  capability_version: string;
  capability_updated_at?: string | null;
  clients: ClientConfigs;
  multi_agent_enabled: boolean;
  router_status: RouterStatus;
};

export type Screen =
  | "dashboard"
  | "route"
  | "providers"
  | "skills"
  | "usage"
  | "requests"
  | "settings";
export type EditorTab = "base" | "models" | "balance";
export type AgentClientKind = "codex" | "claude" | "pi";
export type ProviderKind = "codex" | "claude";
export type TimeRange = "today" | "week" | "month" | "all";
export type TrendMetric = "tokens" | "requests";
