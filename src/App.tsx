import { type PointerEvent, useEffect, useId, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import brandMark from "../output/icons/codex-helper-mark-transparent.png";
import "./App.css";

type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

type BalanceQueryType = "disabled" | "new_api" | "sub2_api" | "ai_gate";
type NewApiBalanceTarget = "token_quota" | "account_balance";
type BalanceAuthMode = "provider_token" | "separate_token";
type ProviderWireApi = "responses" | "chat_completions";
type ProviderStatus = "enabled" | "disabled" | "auto_disabled";

type BalanceQueryConfig = {
  enabled: boolean;
  query_type: BalanceQueryType;
  new_api_target: NewApiBalanceTarget;
  endpoint: string;
  path: string;
  auth_mode: BalanceAuthMode;
  query_token: string;
  new_api_user_id: string;
};

type BalanceStatus = {
  amount?: string | null;
  label: string;
  checked_at?: number | null;
  error?: string | null;
};

type ProviderModelsResponse = {
  models: string[];
};

type ModelMapping = {
  source: string;
  target: string;
};

type RouterConfig = {
  enabled: boolean;
  remote_compaction_enabled: boolean;
  model_provider: string;
  host: string;
  port: number;
  local_token: string;
  connect_timeout_secs: number;
  response_header_timeout_secs: number;
  stream_idle_timeout_secs: number;
};

type SkillLocationConfig = {
  path: string;
  writable: boolean;
};

type ClientTargetConfig = {
  enabled: boolean;
  skill_locations: SkillLocationConfig[];
  managed_skill_location: string;
};

type ClientConfigs = {
  codex: ClientTargetConfig;
  claude: ClientTargetConfig;
  pi: ClientTargetConfig;
};

type SkillLocationView = SkillLocationConfig & {
  managed: boolean;
  exists: boolean;
};

type SkillClientView = {
  client: AgentClientKind;
  label: string;
  managed_skill_location: string;
  skill_locations: SkillLocationView[];
};

type ClientSkillView = {
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

type SkillOrigin = {
  client: AgentClientKind;
  skill_location: string;
  original_path: string;
  original_dir_name: string;
};

type SkillExposureView = {
  client: AgentClientKind;
  client_label: string;
  path: string;
  health: string;
  message: string;
};

type SharedSkillView = {
  identity: string;
  description: string;
  library_dir_name: string;
  path: string;
  sharing_scope: AgentClientKind[];
  origin?: SkillOrigin | null;
  exposures: SkillExposureView[];
};

type SkillConflictView = {
  kind: string;
  identity: string;
  client?: AgentClientKind | null;
  path: string;
  message: string;
};

type SkillManagementView = {
  library_root: string;
  clients: SkillClientView[];
  shared_skills: SharedSkillView[];
  client_skills: ClientSkillView[];
  conflicts: SkillConflictView[];
};

type RouterStatus = {
  running: boolean;
  address: string;
  error?: string | null;
};

type ProviderConfig = {
  id: string;
  name: string;
  status: ProviderStatus;
  enabled: boolean;
  consecutive_failure_count: number;
  auto_disabled_day?: string | null;
  last_failure_reason?: string | null;
  last_failure_at_ms?: number | null;
  config: JsonValue;
  wire_api: ProviderWireApi;
  service_tier: string;
  connection_test_model: string;
  allowed_models: string[];
  model_mappings: ModelMapping[];
  balance_query: BalanceQueryConfig;
  balance_status?: BalanceStatus | null;
  connection_status?: unknown;
};

type ClaudeProviderConfig = {
  id: string;
  name: string;
  status: ProviderStatus;
  enabled: boolean;
  consecutive_failure_count: number;
  auto_disabled_day?: string | null;
  last_failure_reason?: string | null;
  last_failure_at_ms?: number | null;
  base_url: string;
  api_key: string;
  connection_test_model: string;
  allowed_models: string[];
  model_mappings: ModelMapping[];
  connection_status?: unknown;
};

type ProviderSummary = {
  id: string;
  name: string;
  status: ProviderStatus;
  enabled: boolean;
  consecutive_failure_count: number;
  auto_disabled_day?: string | null;
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

type ProviderLatencyDialogState = {
  provider: ProviderSummary;
  providerKind: ProviderKind;
  models: string[];
  selectedModel: string;
  prompt: string;
  streaming: boolean;
  preparing: boolean;
  testing: boolean;
  result: { ok: boolean; latencyMs?: number | null; message: string; reply?: string } | null;
};

type ProviderLatencyTestResponse = {
  app_state: AppState;
  ok: boolean;
  latency_ms?: number | null;
  error?: string | null;
  reply: string;
};

type UsageSummary = {
  request_count: number;
  input_tokens: number;
  uncached_input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  reasoning_output_tokens: number;
  total_tokens: number;
};

type RouteRequestLog = {
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
  upstream_started_ms?: number | null;
  response_header_ms?: number | null;
  upstream_request_id?: string | null;
  total_ms: number;
};

type RouteLogFilter = {
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

type RouteLogFilterOption = {
  id: string;
  name: string;
  request_count: number;
};

type RouteLogsResponse = {
  logs: RouteRequestLog[];
  total: number;
  page: number;
  page_size: number;
  total_pages: number;
  available_providers: RouteLogFilterOption[];
  available_models: string[];
  available_days: string[];
};

type RouteUsageBucket = {
  label: string;
  request_count: number;
  input_tokens: number;
  uncached_input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  total_tokens: number;
};

type RouteUsageBreakdown = RouteUsageBucket & {
  key: string;
  label: string;
};

type RouteUsageStats = {
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

type UpdateCheckInfo = {
  current_version: string;
  latest_version: string;
  available: boolean;
  installable: boolean;
  asset_name?: string | null;
  release_url: string;
};

type UpdateInstallResult = {
  message: string;
  manual_install: boolean;
};

type AppState = {
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
  clients: ClientConfigs;
  multi_agent_enabled: boolean;
  router_status: RouterStatus;
};

type Screen = "dashboard" | "route" | "providers" | "skills" | "usage" | "requests" | "settings";
type EditorTab = "base" | "models" | "balance";
type AgentClientKind = "codex" | "claude" | "pi";
type ProviderKind = "codex" | "claude";
type TimeRange = "today" | "week" | "month" | "all";
type TrendMetric = "tokens" | "requests";

const isTauriRuntime = "__TAURI_INTERNALS__" in window;
const ROUTER_TIMEOUT_LIMITS = {
  connect_timeout_secs: 120,
  response_header_timeout_secs: 600,
  stream_idle_timeout_secs: 3600,
} as const;

function defaultBalanceQuery(endpoint = ""): BalanceQueryConfig {
  return {
    enabled: false,
    query_type: "disabled",
    new_api_target: "token_quota",
    endpoint,
    path: "/api/usage/token/",
    auth_mode: "provider_token",
    query_token: "",
    new_api_user_id: "",
  };
}

function defaultRouterConfig(): RouterConfig {
  return {
    enabled: false,
    remote_compaction_enabled: false,
    model_provider: "custom",
    host: "127.0.0.1",
    port: 18080,
    local_token: "",
    connect_timeout_secs: 10,
    response_header_timeout_secs: 180,
    stream_idle_timeout_secs: 180,
  };
}

function routerModelProviderError(router: RouterConfig) {
  const providerName = router.model_provider.trim();
  if (!providerName) return "Provider 名称不能为空。";
  if (providerName.length > 64) return "Provider 名称不能超过 64 个字符。";
  if (!/^[A-Za-z0-9_-]+$/.test(providerName)) {
    return "Provider 名称只能包含英文字母、数字、下划线和连字符。";
  }
  return "";
}

function routerTimeoutError(router: RouterConfig) {
  if (
    !Number.isInteger(router.connect_timeout_secs) ||
    router.connect_timeout_secs < 1 ||
    router.connect_timeout_secs > ROUTER_TIMEOUT_LIMITS.connect_timeout_secs ||
    !Number.isInteger(router.response_header_timeout_secs) ||
    router.response_header_timeout_secs < 1 ||
    router.response_header_timeout_secs > ROUTER_TIMEOUT_LIMITS.response_header_timeout_secs ||
    !Number.isInteger(router.stream_idle_timeout_secs) ||
    router.stream_idle_timeout_secs < 1 ||
    router.stream_idle_timeout_secs > ROUTER_TIMEOUT_LIMITS.stream_idle_timeout_secs
  ) {
    return "请输入允许范围内的整数秒数。";
  }
  if (router.connect_timeout_secs > router.response_header_timeout_secs) {
    return "响应头超时不能小于连接超时。";
  }
  if (router.response_header_timeout_secs > router.stream_idle_timeout_secs) {
    return "流空闲超时不能小于响应头超时。";
  }
  return "";
}

function defaultClientConfigs(): ClientConfigs {
  return {
    codex: { enabled: false, skill_locations: [], managed_skill_location: "" },
    claude: { enabled: false, skill_locations: [], managed_skill_location: "" },
    pi: { enabled: false, skill_locations: [], managed_skill_location: "" },
  };
}

function agentClientLabel(client: AgentClientKind) {
  if (client === "codex") return "Codex";
  if (client === "claude") return "Claude";
  return "Pi";
}

function jsonPath(value: JsonValue | undefined | null, path: string[]) {
  let current: JsonValue | undefined | null = value;
  for (const key of path) {
    if (!current || typeof current !== "object" || Array.isArray(current)) return undefined;
    current = (current as Record<string, JsonValue>)[key];
  }
  return typeof current === "string" ? current : undefined;
}

function endpointFromBaseUrl(baseUrl: string) {
  return baseUrl.trim().replace(/\/v1\/?$/, "").replace(/\/$/, "");
}

function defaultBalancePath(queryType: BalanceQueryType, target: NewApiBalanceTarget) {
  if (queryType === "sub2_api") return "/v1/usage";
  if (queryType === "ai_gate") return "/api/me/upstreams/usage";
  if (queryType === "new_api" && target === "account_balance") return "/api/user/self";
  return "/api/usage/token/";
}

function normalizeBalanceQuery(config?: BalanceQueryConfig | null, endpoint = "") {
  const queryType = config?.query_type ?? "disabled";
  const target = config?.new_api_target ?? "token_quota";
  const next = {
    ...defaultBalanceQuery(endpoint),
    ...(config ?? {}),
    endpoint: config?.endpoint || endpoint,
    path: config?.path || defaultBalancePath(queryType, target),
  };
  if (next.query_type === "new_api" && next.new_api_target === "account_balance") {
    next.auth_mode = "separate_token";
  } else if (next.query_type === "ai_gate") {
    next.auth_mode = "provider_token";
  }
  return next;
}

function normalizeModelNames(models: string[]) {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const value of models) {
    const model = value.trim();
    if (!model) continue;
    const key = model.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    normalized.push(model);
  }
  return normalized;
}

function formatCompact(value: number) {
  const abs = Math.abs(value || 0);
  if (abs >= 1_000_000) return `${(value / 1_000_000).toFixed(abs >= 10_000_000 ? 1 : 2)}M`;
  if (abs >= 1_000) return `${(value / 1_000).toFixed(abs >= 100_000 ? 0 : 1)}K`;
  return Math.round(value || 0).toLocaleString("zh-CN");
}

function formatTokenCount(value: number) {
  return Math.round(value || 0).toLocaleString("zh-CN");
}

function dateKey(date: Date) {
  const year = date.getFullYear();
  const month = `${date.getMonth() + 1}`.padStart(2, "0");
  const day = `${date.getDate()}`.padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function filterForRange(range: TimeRange): Pick<RouteLogFilter, "start_day" | "end_day"> {
  if (range === "all") return { start_day: undefined, end_day: undefined };
  const end = new Date();
  const start = new Date(end);
  if (range === "week") start.setDate(end.getDate() - 6);
  if (range === "month") start.setDate(1);
  return { start_day: dateKey(start), end_day: dateKey(end) };
}

function rangeLabel(range: TimeRange) {
  if (range === "today") return "今日";
  if (range === "week") return "本周";
  if (range === "month") return "本月";
  return "全部";
}

function trendMetricLabel(metric: TrendMetric) {
  if (metric === "tokens") return "总 Token";
  return "成功请求";
}

function bucketGranularityLabel(value?: string) {
  if (value === "month") return "按月聚合";
  if (value === "day") return "按日聚合";
  return "按小时聚合";
}

function trendBucketValue(bucket: Pick<RouteUsageBucket, "total_tokens" | "request_count">, metric: TrendMetric) {
  if (metric === "tokens") return bucket.total_tokens;
  return bucket.request_count;
}

function emptyTrendBucket(label: string): RouteUsageBucket {
  return {
    label,
    request_count: 0,
    input_tokens: 0,
    uncached_input_tokens: 0,
    cached_input_tokens: 0,
    output_tokens: 0,
    total_tokens: 0,
  };
}

function parseDateKey(value: string) {
  const [year, month, day] = value.split("-").map(Number);
  return new Date(year, (month || 1) - 1, day || 1);
}

function completeTrendBuckets(
  buckets: RouteUsageBucket[],
  range: TimeRange,
  granularity?: string,
): RouteUsageBucket[] {
  const effectiveGranularity = granularity ?? (range === "all" ? "month" : range === "today" ? "hour" : "day");
  const bucketByLabel = new Map(buckets.map((bucket) => [bucket.label, bucket]));

  if (effectiveGranularity === "hour") {
    return Array.from({ length: 24 }, (_, hour) => {
      const label = `${String(hour).padStart(2, "0")}:00`;
      return bucketByLabel.get(label) ?? emptyTrendBucket(label);
    });
  }

  if (effectiveGranularity === "day" || range === "week" || range === "month") {
    const { start_day, end_day } = filterForRange(range);
    if (start_day && end_day) {
      const rows: RouteUsageBucket[] = [];
      const cursor = parseDateKey(start_day);
      const end = parseDateKey(end_day);
      while (cursor <= end && rows.length < 370) {
        const label = dateKey(cursor);
        rows.push(bucketByLabel.get(label) ?? emptyTrendBucket(label));
        cursor.setDate(cursor.getDate() + 1);
      }
      return rows;
    }
  }

  const sorted = [...buckets].sort((left, right) => left.label.localeCompare(right.label));
  return sorted.length ? sorted : [emptyTrendBucket("暂无")];
}

function niceTrendMax(value: number) {
  if (value <= 0) return 1;
  const exponent = Math.floor(Math.log10(value));
  const base = Math.pow(10, exponent);
  const fraction = value / base;
  const niceFraction = fraction <= 1 ? 1 : fraction <= 2 ? 2 : fraction <= 5 ? 5 : 10;
  return niceFraction * base;
}

function trendTickIndexes(count: number, maxTicks = 6) {
  if (count <= maxTicks) return Array.from({ length: count }, (_, index) => index);
  const indexes = new Set<number>();
  for (let index = 0; index < maxTicks; index += 1) {
    indexes.add(Math.round((index * (count - 1)) / (maxTicks - 1)));
  }
  return [...indexes].sort((left, right) => left - right);
}

function formatTrendAxisValue(value: number, metric: TrendMetric) {
  if (metric === "tokens") return formatTokenCount(value);
  return String(Math.round(value));
}

function formatTrendXAxisLabel(label: string, granularity?: string) {
  if (label === "暂无") return label;
  if (granularity === "hour") return label;
  if (granularity === "month") return label;
  const [, month, day] = label.split("-");
  if (month && day) return `${month}-${day}`;
  return label;
}

function formatTokenTriplet(log: RouteRequestLog) {
  if (!log.total_tokens) return "-";
  return `${formatTokenCount(log.uncached_input_tokens)} / ${formatTokenCount(log.cached_input_tokens)} / ${formatTokenCount(log.output_tokens)}`;
}

function remoteCompactionV2AuditLabel(log: RouteRequestLog) {
  const audit = log.remote_compaction_v2;
  if (!audit) return "";
  const steps: string[] = [];
  if (audit.trigger_received) {
    steps.push(audit.trigger_forwarded ? "V2 触发已转发" : "V2 触发未转发");
  }
  if (audit.compaction_response_received) {
    steps.push(audit.compaction_response_forwarded ? "已返回 compaction" : "compaction 未透传");
  }
  if (audit.compaction_item_reused) steps.push("已复用 compaction");
  return steps.join(" · ");
}

function formatMs(value?: number | null) {
  if (value == null) return "-";
  if (value >= 1000) return `${(value / 1000).toFixed(value >= 10_000 ? 1 : 1)} s`;
  return `${Math.round(value)} ms`;
}

function formatDuration(value: number) {
  if (value >= 1000) return `${(value / 1000).toFixed(1)} s`;
  return `${Math.round(value)} ms`;
}

type RouteTimingPhase = {
  label: string;
  durationMs: number;
};

function routeTimingPhases(log: RouteRequestLog): RouteTimingPhase[] {
  const upstreamStarted = log.upstream_started_ms;
  const responseHeader = log.response_header_ms;
  const firstByte = log.first_byte_ms;
  if (upstreamStarted == null) return [];

  const phases: RouteTimingPhase[] = [
    {
      label: log.route_attempts > 1 ? "发起最终上游前（含前置重试）" : "发起上游前",
      durationMs: upstreamStarted,
    },
  ];
  if (responseHeader == null) {
    phases.push({
      label: "等待响应头至结束",
      durationMs: Math.max(0, log.total_ms - upstreamStarted),
    });
    return phases;
  }

  phases.push({
    label: "等待响应头",
    durationMs: Math.max(0, responseHeader - upstreamStarted),
  });
  if (firstByte == null) {
    phases.push({
      label: "响应头后至结束",
      durationMs: Math.max(0, log.total_ms - responseHeader),
    });
    return phases;
  }

  phases.push({
    label: "响应头至首字节",
    durationMs: Math.max(0, firstByte - responseHeader),
  });
  phases.push({
    label: "首字节后至结束",
    durationMs: Math.max(0, log.total_ms - firstByte),
  });
  return phases;
}

function formatTimingDetails(log: RouteRequestLog) {
  const lines = [
    `本地请求 ID：${log.id}`,
    `上游请求 ID：${log.upstream_request_id || "未返回"}`,
    `总耗时：${formatDuration(log.total_ms)}`,
    `路由尝试：${log.route_attempts} 次`,
  ];
  const phases = routeTimingPhases(log);
  if (phases.length === 0) {
    lines.push("分段耗时：旧记录未采集");
    return lines.join("\n");
  }
  phases.forEach((phase) => lines.push(`${phase.label}：${formatDuration(phase.durationMs)}`));
  return lines.join("\n");
}

function formatLogDateTime(value: number) {
  if (!value) return "-";
  return new Date(value).toLocaleString("zh-CN", { hour12: false });
}

function formatLogTime(value: number) {
  if (!value) return "--:--:--";
  return new Date(value).toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}

function statusMeta(status: string) {
  if (status === "success") return { label: "成功", tone: "ok" };
  if (status === "failed") return { label: "失败", tone: "danger" };
  if (status === "cancelled") return { label: "已取消", tone: "amber" };
  return { label: "进行中", tone: "cyan" };
}

function routeResultTone(result: string) {
  if (result.includes("切换") || result.includes("取消")) return "amber";
  if (result.includes("未完成") || result.includes("重试")) return "danger";
  return "ok";
}

function csvEscape(value: string | number | null | undefined) {
  const text = String(value ?? "");
  return `"${text.replace(/"/g, '""')}"`;
}

function exportRouteLogsCsv(logs: RouteRequestLog[]) {
  const headers = [
    "时间",
    "状态",
    "模型",
    "供应商",
    "V2 压缩审计",
    "Token",
    "首字延迟",
    "总耗时",
    "路由结果",
    "错误",
  ];
  const rows = logs.map((log) => [
    new Date(log.started_at_ms).toLocaleString("zh-CN"),
    statusMeta(log.status).label,
    log.model,
    log.provider_name,
    remoteCompactionV2AuditLabel(log),
    log.total_tokens,
    log.first_byte_ms ?? "",
    log.total_ms,
    log.route_result,
    log.error ?? "",
  ]);
  const csv = [headers, ...rows].map((row) => row.map(csvEscape).join(",")).join("\n");
  const url = URL.createObjectURL(new Blob([`\uFEFF${csv}`], { type: "text/csv;charset=utf-8" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = `xxswitch-route-logs-${Date.now()}.csv`;
  link.click();
  URL.revokeObjectURL(url);
}

function formatBalanceForCard(label: string) {
  if (!label || label === "未配置") return "";
  return label.replace(/^账户余额\s*/, "").replace(/^Key额度\s*/, "").replace(/^余额\s*/, "");
}

function providerBalanceMeta(provider: ProviderSummary) {
  const label = formatBalanceForCard(provider.balance_label);
  const error = provider.balance_error?.trim() ?? "";
  if (provider.balance_label === "不适用" || provider.provider_type === "Claude") {
    return { value: "不适用", detail: "无需余额查询", tone: "muted" };
  }
  if (!label || error === "未启用余额查询") {
    return { value: "未启用", detail: "余额查询", tone: "muted" };
  }
  if (error) {
    return { value: label || "查询异常", detail: "检查余额配置", tone: "danger" };
  }
  return { value: label, detail: "账户余额", tone: "ok" };
}

function providerLatencyMeta(provider: ProviderSummary) {
  if (provider.latency_error) {
    return { value: "测试失败", detail: "连接异常", tone: "danger" };
  }
  if (provider.latency_ms == null) {
    return { value: "未测试", detail: "点击测速", tone: "muted" };
  }
  return {
    value: provider.latency_label && provider.latency_label !== "-"
      ? provider.latency_label
      : `${provider.latency_ms} ms`,
    detail: provider.latency_ms > 500 ? "响应较慢" : "连接正常",
    tone: provider.latency_ms > 500 ? "warn" : "ok",
  };
}

function providerStatus(provider: ProviderSummary) {
  if (provider.status === "auto_disabled") return { label: "自动禁用", tone: "danger" };
  if (provider.status === "disabled" || !provider.enabled) return { label: "已停用", tone: "muted" };
  if (provider.latency_error) return { label: "异常", tone: "warn" };
  if (provider.latency_ms != null && provider.latency_ms > 500) {
    return { label: "高延迟", tone: "warn" };
  }
  return { label: "可用", tone: "ok" };
}

function usageProviderOptions(
  loggedProviders: RouteLogFilterOption[],
  configuredProviders: ProviderSummary[],
) {
  const configuredNames = new Map(
    configuredProviders.map((provider) => [provider.id, provider.name]),
  );
  const seen = new Set<string>();
  const options = loggedProviders.map((provider) => {
    seen.add(provider.id);
    return {
      ...provider,
      name: configuredNames.get(provider.id) ?? provider.name,
    };
  });

  for (const provider of configuredProviders) {
    if (seen.has(provider.id)) continue;
    seen.add(provider.id);
    options.push({
      id: provider.id,
      name: provider.name,
      request_count: 0,
    });
  }

  return options;
}

function routeBaseUrl(router: RouterConfig | RouterStatus) {
  if ("address" in router) return `http://${router.address}/v1`;
  return `http://${router.host || "127.0.0.1"}:${router.port || 18080}/v1`;
}

function claudeBaseUrl(router: RouterConfig | RouterStatus) {
  if ("address" in router) return `http://${router.address}`;
  return `http://${router.host || "127.0.0.1"}:${router.port || 18080}`;
}

function serviceOk(state: AppState | null) {
  return Boolean(state?.router.enabled && state.router_status.running);
}

function apiKeyPreview(value: string) {
  if (!value) return "";
  if (value.length <= 8) return "sk-••••••••";
  return `${value.slice(0, 3)}••••••••••••••••`;
}

function savedApiKeyLabel(value: string) {
  return value ? apiKeyPreview(value) : "使用已保存转发 Key";
}

function providerFields(provider: ProviderConfig | null | undefined) {
  const baseUrl = jsonPath(provider?.config, ["model_providers", "custom", "base_url"]) ?? "";
  const apiKey =
    jsonPath(provider?.config, [
      "model_providers",
      "custom",
      "experimental_bearer_token",
    ]) ?? "";
  return { baseUrl, apiKey };
}

async function callCommand<T>(command: string, args?: Record<string, unknown>) {
  if (isTauriRuntime) {
    return invoke<T>(command, args);
  }
  throw new Error(`当前环境不支持命令：${command}`);
}

function NavIcon({ type }: { type: Screen }) {
  const common = {
    fill: "none",
    stroke: "currentColor",
    viewBox: "0 0 24 24",
  };
  return (
    <svg
      aria-hidden="true"
      className="nav-glyph"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={1.8}
      {...common}
    >
      {type === "dashboard" && (
        <>
          <path d="M4 4h7v7H4z" />
          <path d="M13 4h7v5h-7z" />
          <path d="M4 13h7v7H4z" />
          <path d="M13 11h7v9h-7z" />
        </>
      )}
      {type === "route" && (
        <>
          <path d="M5 12h6" />
          <path d="M13 6h6" />
          <path d="M13 18h6" />
          <path d="M11 12c2.2 0 2.2-6 4.8-6" />
          <path d="M11 12c2.2 0 2.2 6 4.8 6" />
          <circle cx="5" cy="12" r="2" />
          <circle cx="19" cy="6" r="2" />
          <circle cx="19" cy="18" r="2" />
        </>
      )}
      {type === "providers" && (
        <>
          <path d="M6 5h12v14H6z" />
          <path d="M9 9h6" />
          <path d="M9 13h6" />
          <path d="M9 17h3" />
        </>
      )}
      {type === "skills" && (
        <>
          <path d="M5 5h6v6H5z" />
          <path d="M13 5h6v6h-6z" />
          <path d="M5 13h6v6H5z" />
          <path d="M14 14h4" />
          <path d="M16 12v4" />
          <path d="M11 8h2" />
          <path d="M8 11v2" />
        </>
      )}
      {type === "usage" && (
        <>
          <path d="M4 19V5" />
          <path d="M4 19h16" />
          <path d="m7 15 3.5-4 3 2.5L19 7" />
        </>
      )}
      {type === "requests" && (
        <>
          <rect height="16" rx="2" width="14" x="5" y="4" />
          <path d="M8.5 8.5h7" />
          <path d="M8.5 12h5.5" />
          <path d="M8.5 15.5h7" />
        </>
      )}
      {type === "settings" && (
        <>
          <circle cx="12" cy="12" r="3" />
          <path d="M12 2.8v3" />
          <path d="M12 18.2v3" />
          <path d="m4.2 4.2 2.1 2.1" />
          <path d="m17.7 17.7 2.1 2.1" />
          <path d="M2.8 12h3" />
          <path d="M18.2 12h3" />
          <path d="m4.2 19.8 2.1-2.1" />
          <path d="m17.7 6.3 2.1-2.1" />
        </>
      )}
    </svg>
  );
}

function Toggle({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <button
      aria-label={label}
      aria-pressed={checked}
      className={`switch ${checked ? "on" : ""}`}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      type="button"
    >
      <span />
    </button>
  );
}

function StatusPill({ ok }: { ok: boolean }) {
  return (
    <div className={`service-pill ${ok ? "ok" : "warn"}`}>
      <span />
      {ok ? "服务正常" : "服务未接管"}
    </div>
  );
}

function ToolIcon({ type }: { type: "settings" | "models" | "latency" | "balance" }) {
  return (
    <svg
      aria-hidden="true"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={2}
      viewBox="0 0 24 24"
    >
      {type === "settings" && (
        <>
          <circle cx="12" cy="12" r="3" />
          <path d="M12 3v3" />
          <path d="M12 18v3" />
          <path d="m5.6 5.6 2.1 2.1" />
          <path d="m16.3 16.3 2.1 2.1" />
          <path d="M3 12h3" />
          <path d="M18 12h3" />
          <path d="m5.6 18.4 2.1-2.1" />
          <path d="m16.3 7.7 2.1-2.1" />
        </>
      )}
      {type === "models" && (
        <>
          <path d="M5 6.5 12 3l7 3.5-7 3.5-7-3.5Z" />
          <path d="m5 11 7 3.5 7-3.5" />
          <path d="m5 15.5 7 3.5 7-3.5" />
        </>
      )}
      {type === "latency" && (
        <>
          <path d="M4 13a8 8 0 1 1 16 0" />
          <path d="M12 13l4-4" />
          <path d="M8 20h8" />
          <path d="M12 17v3" />
        </>
      )}
      {type === "balance" && (
        <>
          <path d="M20 7h-9a4 4 0 0 0 0 8h9" />
          <path d="M16 11h4v8H6a4 4 0 0 1-4-4V7a4 4 0 0 1 4-4h12" />
          <path d="M17 15h.01" />
          <path d="M7 7h.01" />
        </>
      )}
    </svg>
  );
}

function App() {
  const [appState, setAppState] = useState<AppState | null>(null);
  const [skillManagement, setSkillManagement] = useState<SkillManagementView | null>(null);
  const [routeUsageStats, setRouteUsageStats] = useState<RouteUsageStats | null>(null);
  const [routeLogs, setRouteLogs] = useState<RouteLogsResponse | null>(null);
  const [usageRange, setUsageRange] = useState<TimeRange>("today");
  const [usageFilter, setUsageFilter] = useState<RouteLogFilter>(() => ({
    model: "",
    page_size: 20,
    ...filterForRange("today"),
  }));
  const [trendMetric, setTrendMetric] = useState<TrendMetric>("tokens");
  const [requestFilter, setRequestFilter] = useState<RouteLogFilter>({ model: "", page_size: 20 });
  const [requestAutoRefresh, setRequestAutoRefresh] = useState(true);
  const [screen, setScreen] = useState<Screen>("dashboard");
  const [providerKind, setProviderKind] = useState<ProviderKind>("codex");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [updateCheck, setUpdateCheck] = useState<UpdateCheckInfo | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateConfirming, setUpdateConfirming] = useState(false);
  const [updateMessage, setUpdateMessage] = useState("");
  const [editorOpen, setEditorOpen] = useState(false);
  const [editorKind, setEditorKind] = useState<ProviderKind>("codex");
  const [editorTab, setEditorTab] = useState<EditorTab>("base");
  const [editingId, setEditingId] = useState("");
  const [providerName, setProviderName] = useState("");
  const [providerBaseUrl, setProviderBaseUrl] = useState("");
  const [providerApiKey, setProviderApiKey] = useState("");
  const [providerApiKeyDirty, setProviderApiKeyDirty] = useState(false);
  const [providerEnabled, setProviderEnabled] = useState(true);
  const [providerEnabledDirty, setProviderEnabledDirty] = useState(false);
  const [providerWireApi, setProviderWireApi] = useState<ProviderWireApi>("responses");
  const [providerFastMode, setProviderFastMode] = useState(false);
  const [providerTestModel, setProviderTestModel] = useState("");
  const [providerModels, setProviderModels] = useState<string[]>([]);
  const [allowedModels, setAllowedModels] = useState<string[]>([]);
  const [modelMappings, setModelMappings] = useState<ModelMapping[]>([]);
  const [balanceQuery, setBalanceQuery] = useState<BalanceQueryConfig>(() =>
    defaultBalanceQuery(),
  );
  const [balanceTestStatus, setBalanceTestStatus] = useState<BalanceStatus | null>(null);
  const [routerDraft, setRouterDraft] = useState<RouterConfig>(() => defaultRouterConfig());
  const [secretVisible, setSecretVisible] = useState(false);
  const [balanceTokenVisible, setBalanceTokenVisible] = useState(false);
  const [latencyDialog, setLatencyDialog] = useState<ProviderLatencyDialogState | null>(null);
  const [newProviderCount, setNewProviderCount] = useState(1);
  const didInitialRefresh = useRef(false);
  const shellRef = useRef<HTMLElement>(null);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function refreshRouteUsage(filter = usageFilter) {
    const usage = await callCommand<RouteUsageStats>("load_route_usage_stats", {
      payload: { filter },
    });
    setRouteUsageStats(usage);
  }

  async function refreshRouteLogs(filter = requestFilter) {
    const logs = await callCommand<RouteLogsResponse>("load_route_logs", {
      payload: { filter },
    });
    setRouteLogs(logs);
  }

  async function refreshAppState() {
    const state = await callCommand<AppState>("load_app_state");
    setAppState(state);
    setRouterDraft(state.router ?? defaultRouterConfig());
  }

  async function refreshSkillManagement() {
    const view = await callCommand<SkillManagementView>("load_skill_management");
    setSkillManagement(view);
  }

  async function refresh() {
    await refreshAppState();
    try {
      await Promise.all([refreshRouteUsage(), refreshRouteLogs()]);
    } catch {
      setRouteUsageStats(null);
      setRouteLogs(null);
    }
  }

  async function checkForUpdate() {
    if (updateBusy) return;
    setUpdateBusy(true);
    setUpdateConfirming(false);
    setUpdateMessage("");
    try {
      setUpdateCheck(await callCommand<UpdateCheckInfo>("check_for_update"));
    } catch (err) {
      setUpdateMessage(`检查更新失败：${String(err)}`);
    } finally {
      setUpdateBusy(false);
    }
  }

  async function installUpdate() {
    if (!updateCheck?.available || !updateCheck.installable || updateBusy) return;
    setUpdateBusy(true);
    setUpdateConfirming(false);
    setUpdateMessage(
      updateCheck.asset_name?.toLowerCase().endsWith(".dmg")
        ? "正在下载并校验更新，完成后应用会自动替换并重启，请勿退出…"
        : "正在下载更新，完成后会启动安装程序，请勿退出…",
    );
    try {
      const result = await callCommand<UpdateInstallResult>("install_update");
      setUpdateMessage(result.message);
    } catch (err) {
      setUpdateMessage(`更新失败：${String(err)}`);
    } finally {
      setUpdateBusy(false);
    }
  }

  useEffect(() => {
    refresh().catch((err) => setError(String(err)));
  }, []);

  useEffect(() => {
    if (!didInitialRefresh.current) {
      didInitialRefresh.current = true;
      return;
    }

    let cancelled = false;
    async function refreshActiveScreen() {
      try {
        if (screen === "dashboard" || screen === "usage") {
          await refreshRouteUsage(usageFilter);
        } else if (screen === "requests") {
          await refreshRouteLogs(requestFilter);
        } else if (screen === "route" || screen === "providers") {
          await refreshAppState();
        } else if (screen === "skills") {
          await refreshSkillManagement();
        }
      } catch (err) {
        if (!cancelled) setError(String(err));
      }
    }

    refreshActiveScreen();
    return () => {
      cancelled = true;
    };
  }, [screen]);

  useEffect(() => {
    if (screen !== "requests" || !requestAutoRefresh) return;
    const timer = window.setInterval(() => {
      refreshRouteLogs(requestFilter).catch((err) => setError(String(err)));
    }, 5000);
    return () => window.clearInterval(timer);
  }, [requestAutoRefresh, requestFilter, screen]);

  useEffect(() => {
    if (shellRef.current) shellRef.current.scrollTop = 0;
  }, [screen]);

  const activeProvider = useMemo(() => {
    if (!appState) return null;
    return (
      appState.providers.find((provider) => provider.id === appState.active_provider_id) ??
      appState.providers[0] ??
      null
    );
  }, [appState]);
  const requestCount = routeUsageStats?.success_count ?? routeUsageStats?.summary.request_count ?? 0;
  const uncachedInput = routeUsageStats?.summary.uncached_input_tokens ?? 0;
  const cachedInput = routeUsageStats?.summary.cached_input_tokens ?? 0;
  const outputTokens = routeUsageStats?.summary.output_tokens ?? 0;
  const totalTokens = routeUsageStats?.summary.total_tokens ?? 0;
  const modelRows = (routeUsageStats?.models ?? [])
    .map((row) => ({
      model: row.label,
      requests: row.request_count,
      tokens: row.total_tokens,
    }))
    .slice(0, 3);
  const failedCount = routeUsageStats?.failed_count ?? 0;
  const totalFinishedCount = requestCount + failedCount;
  const successRate = totalFinishedCount ? (requestCount / totalFinishedCount) * 100 : 0;

  function fillProviderEditor(targetFull: ProviderConfig, summary: ProviderSummary | null, tab: EditorTab) {
    const fields = providerFields(targetFull);
    setEditorKind("codex");
    setEditingId(summary?.id ?? targetFull.id);
    setProviderName(summary?.name ?? targetFull.name ?? "");
    setProviderBaseUrl(summary?.base_url || fields.baseUrl);
    setProviderApiKey(fields.apiKey);
    setProviderApiKeyDirty(false);
    setProviderEnabled((summary?.status ?? targetFull.status) === "enabled");
    setProviderEnabledDirty(false);
    setProviderWireApi(targetFull.wire_api ?? "responses");
    setProviderFastMode(targetFull.service_tier?.trim().toLowerCase() === "priority");
    setProviderTestModel(targetFull.connection_test_model ?? "");
    setProviderModels(targetFull.allowed_models?.length ? targetFull.allowed_models : targetFull.connection_test_model ? [targetFull.connection_test_model] : []);
    setAllowedModels(targetFull.allowed_models ?? []);
    setModelMappings(targetFull.model_mappings ?? []);
    setBalanceQuery(
      normalizeBalanceQuery(
        targetFull.balance_query,
        endpointFromBaseUrl(summary?.base_url || fields.baseUrl),
      ),
    );
    setEditorTab(tab);
    setSecretVisible(false);
    setBalanceTokenVisible(false);
    setBalanceTestStatus(targetFull.balance_status ?? null);
    setEditorOpen(true);
  }

  function fillClaudeProviderEditor(targetFull: ClaudeProviderConfig, summary: ProviderSummary | null) {
    setEditorKind("claude");
    setEditingId(summary?.id ?? targetFull.id);
    setProviderName(summary?.name ?? targetFull.name ?? "");
    setProviderBaseUrl(summary?.base_url || targetFull.base_url || "");
    setProviderApiKey(targetFull.api_key || "");
    setProviderApiKeyDirty(false);
    setProviderEnabled((summary?.status ?? targetFull.status) === "enabled");
    setProviderEnabledDirty(false);
    setProviderTestModel(targetFull.connection_test_model ?? "");
    setProviderModels(targetFull.allowed_models?.length ? targetFull.allowed_models : targetFull.connection_test_model ? [targetFull.connection_test_model] : []);
    setAllowedModels(targetFull.allowed_models ?? []);
    setModelMappings(targetFull.model_mappings ?? []);
    setEditorTab("base");
    setSecretVisible(false);
    setEditorOpen(true);
  }

  async function openProviderEditor(provider?: ProviderSummary, tab: EditorTab = "base") {
    const targetSummary = provider ?? activeProvider;
    if (!targetSummary) return;
    await run(async () => {
      const targetFull = await callCommand<ProviderConfig>("get_provider", {
        providerId: targetSummary.id,
      });
      fillProviderEditor(targetFull, targetSummary, tab);
    });
  }

  async function openClaudeProviderEditor(provider?: ProviderSummary) {
    const targetSummary = provider ?? appState?.claude_providers[0] ?? null;
    if (!targetSummary) return;
    await run(async () => {
      const targetFull = await callCommand<ClaudeProviderConfig>("get_claude_provider", {
        providerId: targetSummary.id,
      });
      fillClaudeProviderEditor(targetFull, targetSummary);
    });
  }

  async function addProvider() {
    await run(async () => {
      const name = `新供应商 ${newProviderCount}`;
      const state = await callCommand<AppState>("add_provider", { name });
      setNewProviderCount((value) => value + 1);
      setAppState(state);
      const created = state.providers.find((provider) => provider.id === state.active_provider_id);
      if (created) {
        const targetFull = await callCommand<ProviderConfig>("get_provider", {
          providerId: created.id,
        });
        fillProviderEditor(targetFull, created, "base");
      }
    });
  }

  async function addClaudeProvider() {
    await run(async () => {
      const name = `新 Claude 供应商 ${newProviderCount}`;
      const state = await callCommand<AppState>("add_claude_provider", { name });
      setNewProviderCount((value) => value + 1);
      setAppState(state);
      const created = state.claude_providers.find((provider) => provider.id === state.active_claude_provider_id);
      if (created) {
        const targetFull = await callCommand<ClaudeProviderConfig>("get_claude_provider", {
          providerId: created.id,
        });
        fillClaudeProviderEditor(targetFull, created);
      }
    });
  }

  async function saveProvider() {
    if (!editingId) return;
    await run(async () => {
      const nextBalance = {
        ...balanceQuery,
        endpoint: balanceQuery.endpoint || endpointFromBaseUrl(providerBaseUrl),
      };
      const payload: Record<string, unknown> = {
        provider_id: editingId,
        provider_name: providerName,
        config_toml: "",
        base_url: providerBaseUrl,
        wire_api: providerWireApi,
        service_tier: providerFastMode ? "priority" : "",
        balance_query: nextBalance,
        balance_status: balanceTestStatus,
        connection_test_model: providerTestModel,
        allowed_models: allowedModels,
        model_mappings: modelMappings,
      };
      if (providerApiKeyDirty) {
        payload.api_key = providerApiKey;
      }
      if (providerEnabledDirty) {
        payload.enabled = providerEnabled;
      }
      const state = await callCommand<AppState>("save_provider", {
        payload,
      });
      setAppState(state);
      setEditorOpen(false);
    });
  }

  async function saveClaudeProvider() {
    if (!editingId) return;
    await run(async () => {
      const payload: Record<string, unknown> = {
        provider_id: editingId,
        provider_name: providerName,
        base_url: providerBaseUrl,
        connection_test_model: providerTestModel,
        allowed_models: allowedModels,
        model_mappings: modelMappings,
      };
      if (providerApiKeyDirty) {
        payload.api_key = providerApiKey;
      }
      if (providerEnabledDirty) {
        payload.enabled = providerEnabled;
      }
      const state = await callCommand<AppState>("save_claude_provider", {
        payload,
      });
      setAppState(state);
      setEditorOpen(false);
    });
  }

  async function deleteProvider() {
    if (!editingId) return;
    await run(async () => {
      const state = await callCommand<AppState>("delete_provider", {
        payload: { provider_id: editingId },
      });
      setAppState(state);
      setEditorOpen(false);
      setEditingId("");
    });
  }

  async function deleteClaudeProvider() {
    if (!editingId) return;
    await run(async () => {
      const state = await callCommand<AppState>("delete_claude_provider", {
        payload: { provider_id: editingId },
      });
      setAppState(state);
      setEditorOpen(false);
      setEditingId("");
    });
  }

  async function testBalance() {
    if (!editingId) return;
    await run(async () => {
      const nextBalance = {
        ...balanceQuery,
        endpoint: balanceQuery.endpoint || endpointFromBaseUrl(providerBaseUrl),
      };
      const payload: Record<string, unknown> = {
        provider_id: editingId,
        base_url: providerBaseUrl,
        balance_query: nextBalance,
      };
      if (providerApiKeyDirty) {
        payload.api_key = providerApiKey;
      }
      const state = await callCommand<AppState>("query_provider_balance", {
        payload,
      });
      setAppState(state);
      if (state.active_provider?.id === editingId) {
        setBalanceTestStatus(state.active_provider.balance_status ?? null);
      } else {
        const summary = state.providers.find((provider) => provider.id === editingId);
        setBalanceTestStatus(summary
          ? {
              amount: null,
              label: summary.balance_label,
              checked_at: null,
              error: summary.balance_error ?? null,
            }
          : null);
      }
    });
  }

  async function loadProviderModels() {
    if (!editingId) return;
    await run(async () => {
      const payload: Record<string, unknown> = {
        provider_id: editingId,
        base_url: providerBaseUrl,
        api_key: null,
        balance_query: {
          ...balanceQuery,
          endpoint: balanceQuery.endpoint || endpointFromBaseUrl(providerBaseUrl),
        },
      };
      if (providerApiKeyDirty) {
        payload.api_key = providerApiKey;
      }
      const result = await callCommand<ProviderModelsResponse>("load_provider_models", {
        payload,
      });
      setProviderModels(result.models);
      if (allowedModels.length === 0 && result.models.length > 0) {
        setAllowedModels(result.models);
      }
      if (!providerTestModel && result.models[0]) {
        setProviderTestModel(result.models[0]);
      }
    });
  }

  async function loadClaudeProviderModels() {
    if (!editingId) return;
    await run(async () => {
      const payload: Record<string, unknown> = {
        provider_id: editingId,
        base_url: providerBaseUrl,
        api_key: null,
      };
      if (providerApiKeyDirty) {
        payload.api_key = providerApiKey;
      }
      const result = await callCommand<ProviderModelsResponse>("load_claude_provider_models", {
        payload,
      });
      setProviderModels(result.models);
      if (allowedModels.length === 0 && result.models.length > 0) {
        setAllowedModels(result.models);
      }
      if (!providerTestModel && result.models[0]) {
        setProviderTestModel(result.models[0]);
      }
    });
  }

  async function toggleProvider(provider: ProviderSummary, enabled: boolean) {
    await run(async () => {
      const state = await callCommand<AppState>("save_provider", {
        payload: {
          provider_id: provider.id,
          provider_name: provider.name,
          config_toml: "",
          base_url: provider.base_url,
          enabled,
        },
      });
      setAppState(state);
    });
  }

  async function toggleClaudeProvider(provider: ProviderSummary, enabled: boolean) {
    await run(async () => {
      const state = await callCommand<AppState>("save_claude_provider", {
        payload: {
          provider_id: provider.id,
          provider_name: provider.name,
          base_url: provider.base_url,
          enabled,
        },
      });
      setAppState(state);
    });
  }

  async function reorderProviders(providerIds: string[]) {
    await run(async () => {
      const state = await callCommand<AppState>("reorder_providers", {
        payload: { provider_ids: providerIds },
      });
      setAppState(state);
    });
  }

  async function reorderClaudeProviders(providerIds: string[]) {
    await run(async () => {
      const state = await callCommand<AppState>("reorder_claude_providers", {
        payload: { provider_ids: providerIds },
      });
      setAppState(state);
    });
  }

  async function openProviderLatencyDialog(provider: ProviderSummary, kind: ProviderKind) {
    setLatencyDialog({
      provider,
      providerKind: kind,
      models: [],
      selectedModel: "",
      prompt: "hi",
      streaming: false,
      preparing: true,
      testing: false,
      result: null,
    });

    try {
      const config = kind === "claude"
        ? await callCommand<ClaudeProviderConfig>("get_claude_provider", { providerId: provider.id })
        : await callCommand<ProviderConfig>("get_provider", { providerId: provider.id });
      let models = normalizeModelNames([
        ...config.allowed_models,
        ...config.model_mappings.map((mapping) => mapping.target),
        config.connection_test_model,
      ]);
      try {
        const response = await callCommand<ProviderModelsResponse>(
          kind === "claude" ? "load_claude_provider_models" : "load_provider_models",
          { payload: { provider_id: provider.id } },
        );
        if (response.models.length > 0) models = response.models;
      } catch (err) {
        if (models.length === 0) throw err;
      }
      const preferredModel = config.connection_test_model.trim();
      const selectedModel = models.find((model) => model.toLowerCase() === preferredModel.toLowerCase())
        ?? models[0]
        ?? "";
      setLatencyDialog((current) => current?.provider.id === provider.id
        ? { ...current, models, selectedModel, preparing: false }
        : current);
    } catch (err) {
      setLatencyDialog((current) => current?.provider.id === provider.id
        ? {
            ...current,
            preparing: false,
            result: { ok: false, message: String(err) },
          }
        : current);
    }
  }

  async function testProviderLatency() {
    if (!latencyDialog || !latencyDialog.selectedModel || !latencyDialog.prompt.trim()) return;
    const target = latencyDialog;
    setLatencyDialog({ ...target, testing: true, result: null });
    try {
      const response = await callCommand<ProviderLatencyTestResponse>("test_provider_latency_state", {
        payload: {
          provider_id: target.provider.id,
          provider_kind: target.providerKind,
          model: target.selectedModel,
          prompt: target.prompt.trim(),
          stream: target.streaming,
        },
      });
      setAppState(response.app_state);
      setLatencyDialog((current) => current?.provider.id === target.provider.id
        ? {
            ...current,
            testing: false,
            result: {
              ok: response.ok,
              latencyMs: response.latency_ms,
              message: response.ok
                ? `${target.selectedModel} ${target.streaming ? "流式响应完成" : "同步响应正常"}`
                : response.error || "测速失败",
              reply: response.reply,
            },
          }
        : current);
    } catch (err) {
      setLatencyDialog((current) => current?.provider.id === target.provider.id
        ? {
            ...current,
            testing: false,
            result: { ok: false, message: String(err) },
          }
        : current);
    }
  }

  async function refreshProviderBalance(provider: ProviderSummary) {
    await run(async () => {
      const state = await callCommand<AppState>("query_provider_balance", {
        payload: { provider_id: provider.id, base_url: null, api_key: null },
      });
      setAppState(state);
    });
  }

  async function saveRouter(nextRouter: RouterConfig, apply = false) {
    await run(async () => {
      const saved = await callCommand<AppState>("save_router_config", {
        payload: nextRouter,
      });
      if (apply) {
        const applied = await callCommand<AppState>("apply_config");
        setAppState(applied);
        setRouterDraft(applied.router);
      } else {
        setAppState(saved);
        setRouterDraft(saved.router);
      }
    });
  }

  async function saveClientConfig(kind: AgentClientKind, enabled: boolean) {
    await run(async () => {
      const clients = appState?.clients ?? defaultClientConfigs();
      const state = await callCommand<AppState>("save_client_configs", {
        payload: {
          codex_enabled: kind === "codex" ? enabled : clients.codex.enabled,
          claude_enabled: kind === "claude" ? enabled : clients.claude.enabled,
          pi_enabled: kind === "pi" ? enabled : clients.pi.enabled,
        },
      });
      setAppState(state);
      setRouterDraft(state.router);
    });
  }

  async function saveMultiAgentEnabled(enabled: boolean) {
    await run(async () => {
      const state = await callCommand<AppState>("save_multi_agent_enabled", {
        payload: { enabled },
      });
      setAppState(state);
      setRouterDraft(state.router);
    });
  }

  async function saveSkillClientConfig(
    client: AgentClientKind,
    skillLocations: SkillLocationConfig[],
    managedSkillLocation: string,
  ) {
    await run(async () => {
      const view = await callCommand<SkillManagementView>("save_skill_client_config", {
        payload: {
          client,
          skill_locations: skillLocations,
          managed_skill_location: managedSkillLocation,
        },
      });
      setSkillManagement(view);
    });
  }

  async function promoteClientSkill(skill: ClientSkillView) {
    await run(async () => {
      const view = await callCommand<SkillManagementView>("promote_client_skill", {
        payload: {
          client: skill.client,
          skill_path: skill.path,
          sharing_scope: [skill.client],
        },
      });
      setSkillManagement(view);
    });
  }

  async function useExistingSharedSkill(skill: ClientSkillView) {
    await run(async () => {
      const view = await callCommand<SkillManagementView>("replace_client_skill_with_shared", {
        payload: {
          client: skill.client,
          skill_path: skill.path,
        },
      });
      setSkillManagement(view);
    });
  }

  async function setSkillSharingScope(skill: SharedSkillView, scope: AgentClientKind[]) {
    await run(async () => {
      const view = await callCommand<SkillManagementView>("set_skill_sharing_scope", {
        payload: {
          skill_identity: skill.identity,
          sharing_scope: scope,
        },
      });
      setSkillManagement(view);
    });
  }

  async function deleteSharedSkill(skill: SharedSkillView) {
    if (!window.confirm(`删除 Shared Skill「${skill.identity}」并移除所有 exposure？`)) return;
    await run(async () => {
      const view = await callCommand<SkillManagementView>("delete_shared_skill", {
        payload: {
          skill_identity: skill.identity,
        },
      });
      setSkillManagement(view);
    });
  }

  async function applyUsageFilter(patch: Partial<RouteLogFilter>) {
    const next = { ...usageFilter, ...patch, page: patch.page ?? 1 };
    setUsageFilter(next);
    await run(async () => refreshRouteUsage(next));
  }

  async function applyUsageRange(range: TimeRange) {
    setUsageRange(range);
    const next = { ...usageFilter, ...filterForRange(range), page: 1 };
    setUsageFilter(next);
    await run(async () => refreshRouteUsage(next));
  }

  async function applyRequestFilter(patch: Partial<RouteLogFilter>) {
    const next = { ...requestFilter, ...patch, page: patch.page ?? 1 };
    setRequestFilter(next);
    await run(async () => refreshRouteLogs(next));
  }

  async function applyTodayRequestFilter() {
    const today = dateKey(new Date());
    await applyRequestFilter({ start_day: today, end_day: today, page: 1 });
  }

  function updateBalanceQuery(patch: Partial<BalanceQueryConfig>) {
    setBalanceTestStatus(null);
    setBalanceQuery((current) => {
      const next = { ...current, ...patch };
      if (patch.query_type) {
        next.enabled = patch.query_type !== "disabled";
        next.path = defaultBalancePath(patch.query_type, next.new_api_target);
      }
      if (patch.new_api_target) {
        next.path = defaultBalancePath(next.query_type, patch.new_api_target);
      }
      if (next.query_type === "new_api" && next.new_api_target === "account_balance") {
        next.auth_mode = "separate_token";
      } else if (next.query_type === "ai_gate") {
        next.auth_mode = "provider_token";
      }
      return next;
    });
  }

  if (!appState) {
    return (
      <main className="loading-screen">
        <div className="brand-logo">
          <img src={brandMark} alt="" />
        </div>
        <strong>XXSwitch</strong>
        <p>{error || "正在加载本地网关状态"}</p>
        {error && <button onClick={() => refresh()}>重试</button>}
      </main>
    );
  }

  const routerOn = serviceOk(appState);
  const navGroups = [
    {
      label: "工作区",
      items: [
        ["dashboard", "总览"],
        ["route", "路由"],
        ["providers", "供应商"],
        ["usage", "使用统计"],
        ["requests", "请求记录"],
      ],
    },
    {
      label: "管理",
      items: [
        ["skills", "Skills"],
        ["settings", "设置"],
      ],
    },
  ] as const;

  return (
    <main className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-logo">
            <img src={brandMark} alt="" />
          </div>
          <div>
            <strong>XXSwitch</strong>
            <small>LOCAL AI GATEWAY</small>
          </div>
        </div>

        <nav className="nav">
          {navGroups.map((group) => (
            <div className="nav-section" key={group.label}>
              <span className="nav-section-label">{group.label}</span>
              {group.items.map(([key, label]) => (
                <button
                  className={screen === key ? "active" : ""}
                  key={key}
                  onClick={() => setScreen(key as Screen)}
                  type="button"
                >
                  <NavIcon type={key as Screen} />
                  <span>{label}</span>
                </button>
              ))}
            </div>
          ))}
        </nav>

        <div className="sidebar-meta">
          <span className={routerOn ? "online" : "offline"}>
            <i />
            {routerOn ? "网关运行中" : "网关未运行"}
          </span>
          <small>v{appState.app_version}</small>
        </div>
      </aside>

      <section className="shell" ref={shellRef}>
        <header className="topbar">
          <div>
            <h1>
              {screen === "dashboard"
                ? "总览"
                : screen === "route"
                  ? "路由"
                : screen === "providers"
                  ? "供应商"
                  : screen === "skills"
                    ? "Skill 管理"
                  : screen === "usage"
                    ? "使用统计"
                    : screen === "requests"
                      ? "请求记录"
                      : "设置"}
            </h1>
            <p>
              {screen === "dashboard"
                ? "用量、供应商与请求质量"
                : screen === "route"
                  ? "管理 Codex、Claude 接管与本地代理"
                : screen === "providers"
                  ? "管理上游连接、余额监控与路由顺序"
                  : screen === "skills"
                    ? "管理 Codex、Claude 与 Pi 之间的本机 Shared Skills"
                : screen === "usage"
                    ? "分析经过路由的调用、Token 与供应商使用情况"
                    : screen === "requests"
                      ? "经 XXSwitch 转发的请求"
                      : "本地网关运行参数"}
            </p>
          </div>
          <div className="top-actions">
            <StatusPill ok={routerOn} />
            <div className="client-switch">
              <span>Codex</span>
              <Toggle
                checked={appState.clients.codex.enabled}
                disabled={busy}
                onChange={(checked) => void saveClientConfig("codex", checked)}
              />
            </div>
            <div className="client-switch">
              <span>Claude</span>
              <Toggle
                checked={appState.clients.claude.enabled}
                disabled={busy}
                onChange={(checked) => void saveClientConfig("claude", checked)}
              />
            </div>
            <div className="client-switch">
              <span>Pi</span>
              <Toggle
                checked={appState.clients.pi.enabled}
                disabled={busy}
                onChange={(checked) => void saveClientConfig("pi", checked)}
              />
            </div>
          </div>
        </header>

        {error && <div className="error-banner">{error}</div>}

        {screen === "dashboard" && (
          <Dashboard
            activeProvider={activeProvider}
            cachedInput={cachedInput}
            modelRows={modelRows}
            onRangeChange={applyUsageRange}
            onTrendMetricChange={setTrendMetric}
            outputTokens={outputTokens}
            providers={appState.providers}
            requestCount={requestCount}
            stats={routeUsageStats}
            successRate={successRate}
            timeRange={usageRange}
            totalTokens={totalTokens}
            trendMetric={trendMetric}
            uncachedInput={uncachedInput}
          />
        )}

        {screen === "route" && (
          <RouteScreen
            appState={appState}
            busy={busy}
            onSaveClientConfig={saveClientConfig}
            onSaveMultiAgentEnabled={saveMultiAgentEnabled}
            routerDraft={routerDraft}
            routerOn={routerOn}
            setRouterDraft={setRouterDraft}
            onSaveRouter={saveRouter}
          />
        )}

        {screen === "providers" && (
          <ProvidersScreen
            busy={busy}
            onAdd={providerKind === "claude" ? addClaudeProvider : addProvider}
            onEdit={(provider, tab) => {
              if (providerKind === "claude") {
                void openClaudeProviderEditor(provider);
              } else {
                void openProviderEditor(provider, tab);
              }
            }}
            onKindChange={setProviderKind}
            onRefreshBalance={providerKind === "claude" ? async (_provider) => undefined : refreshProviderBalance}
            onReorder={providerKind === "claude" ? reorderClaudeProviders : reorderProviders}
            onTestLatency={(provider) => openProviderLatencyDialog(provider, providerKind)}
            onToggle={providerKind === "claude" ? toggleClaudeProvider : toggleProvider}
            providerKind={providerKind}
            providers={providerKind === "claude" ? appState.claude_providers : appState.providers}
          />
        )}

        {screen === "skills" && (
          <SkillManagementScreen
            busy={busy}
            onDeleteSharedSkill={deleteSharedSkill}
            onPromote={promoteClientSkill}
            onRefresh={refreshSkillManagement}
            onSaveClientConfig={saveSkillClientConfig}
            onSetSharingScope={setSkillSharingScope}
            onUseExistingSharedSkill={useExistingSharedSkill}
            view={skillManagement}
          />
        )}

        {screen === "usage" && (
          <UsageScreen
            configuredProviders={[...appState.providers, ...appState.claude_providers]}
            filter={usageFilter}
            onFilter={applyUsageFilter}
            onRangeChange={applyUsageRange}
            onRefresh={() => run(() => refreshRouteUsage(usageFilter))}
            onTrendMetricChange={setTrendMetric}
            stats={routeUsageStats}
            timeRange={usageRange}
            trendMetric={trendMetric}
          />
        )}

        {screen === "requests" && (
          <RequestLogsScreen
            autoRefresh={requestAutoRefresh}
            filter={requestFilter}
            logs={routeLogs}
            onFilter={applyRequestFilter}
            onRefresh={() => run(() => refreshRouteLogs(requestFilter))}
            onSetAutoRefresh={setRequestAutoRefresh}
            onToday={applyTodayRequestFilter}
          />
        )}

        {screen === "settings" && (
          <section className="settings-grid">
            <article className="page-panel">
              <div className="panel-head">
                <div>
                  <h2>设置</h2>
                  <p>应用信息与通用偏好设置入口。路由接管与本地代理配置已移动到路由页。</p>
                </div>
              </div>
              <div className="settings-list">
                <div className="settings-row">
                  <span>当前版本</span>
                  <strong>{appState.app_version}</strong>
                </div>
                <div className="settings-row update-check-row">
                  <div>
                    <span>远端更新</span>
                    <small>从 GitHub Release 获取最新稳定版本</small>
                  </div>
                  <button className="ghost" disabled={updateBusy} onClick={() => void checkForUpdate()} type="button">
                    {updateBusy ? "正在检查…" : "检查更新"}
                  </button>
                </div>
              </div>
              {updateCheck && (
                <div className={`update-result ${updateCheck.available ? "available" : "latest"}`}>
                  <div>
                    <strong>
                      {updateCheck.available
                        ? `发现新版本 ${updateCheck.latest_version}`
                        : "当前已经是最新版本"}
                    </strong>
                    <span>
                      {updateCheck.available
                        ? updateCheck.installable
                          ? `将安装 ${updateCheck.asset_name}`
                          : "该 Release 的当前系统安装包仍在构建或不可用"
                        : `远端版本：${updateCheck.latest_version}`}
                    </span>
                  </div>
                  {updateCheck.available && updateCheck.installable && (
                    <div className="update-actions">
                      {updateConfirming && !updateBusy && (
                        <span className="update-confirm-text">
                          {updateCheck.asset_name?.toLowerCase().endsWith(".dmg")
                            ? "确认后将自动替换当前应用并重启；没有写入权限时 macOS 会请求管理员授权。"
                            : "确认后将下载安装包并退出当前应用。"}
                        </span>
                      )}
                      {updateConfirming && !updateBusy && (
                        <button className="ghost" onClick={() => setUpdateConfirming(false)} type="button">
                          取消
                        </button>
                      )}
                      <button
                        className="primary"
                        disabled={updateBusy}
                        onClick={() => {
                          if (updateConfirming) {
                            void installUpdate();
                          } else {
                            setUpdateConfirming(true);
                            setUpdateMessage("");
                          }
                        }}
                        type="button"
                      >
                        {updateBusy ? "正在更新…" : updateConfirming ? "确认更新" : "一键更新"}
                      </button>
                    </div>
                  )}
                </div>
              )}
              {updateMessage && <div className="update-message">{updateMessage}</div>}
            </article>
          </section>
        )}
      </section>

      {latencyDialog && (
        <ProviderLatencyDialog
          state={latencyDialog}
          onClose={() => setLatencyDialog(null)}
          onPromptChange={(prompt) => setLatencyDialog((current) => current ? { ...current, prompt, result: null } : current)}
          onModelChange={(selectedModel) => setLatencyDialog((current) => current ? { ...current, selectedModel, result: null } : current)}
          onStreamingChange={(streaming) => setLatencyDialog((current) => current ? { ...current, streaming, result: null } : current)}
          onTest={() => void testProviderLatency()}
        />
      )}

      {editorOpen && editorKind === "codex" && (
        <ProviderEditor
          balanceQuery={balanceQuery}
          balanceTestStatus={balanceTestStatus}
          balanceTokenVisible={balanceTokenVisible}
          busy={busy}
          allowedModels={allowedModels}
          modelMappings={modelMappings}
          onBalanceTokenVisible={setBalanceTokenVisible}
          onClose={() => setEditorOpen(false)}
          onDelete={deleteProvider}
          onLoadProviderModels={loadProviderModels}
          onSave={saveProvider}
          onTab={setEditorTab}
          onTestBalance={testBalance}
          onUpdateBalance={updateBalanceQuery}
          providerApiKey={providerApiKey}
          providerBaseUrl={providerBaseUrl}
          providerModels={providerModels}
          providerName={providerName}
          providerFastMode={providerFastMode}
          providerWireApi={providerWireApi}
          secretVisible={secretVisible}
          setProviderApiKey={(value) => {
            setProviderApiKey(value);
            setBalanceTestStatus(null);
            setProviderModels([]);
            setProviderTestModel("");
          }}
          setProviderApiKeyDirty={setProviderApiKeyDirty}
          setProviderBaseUrl={(value) => {
            setProviderBaseUrl(value);
            setBalanceTestStatus(null);
            setProviderModels([]);
            setProviderTestModel("");
          }}
          setAllowedModels={setAllowedModels}
          setModelMappings={setModelMappings}
          setProviderName={setProviderName}
          setProviderFastMode={setProviderFastMode}
          setProviderWireApi={setProviderWireApi}
          setSecretVisible={setSecretVisible}
          tab={editorTab}
        />
      )}
      {editorOpen && editorKind === "claude" && (
        <ClaudeProviderEditor
          allowedModels={allowedModels}
          busy={busy}
          modelMappings={modelMappings}
          onClose={() => setEditorOpen(false)}
          onDelete={deleteClaudeProvider}
          onLoadProviderModels={loadClaudeProviderModels}
          onSave={saveClaudeProvider}
          providerApiKey={providerApiKey}
          providerBaseUrl={providerBaseUrl}
          providerModels={providerModels}
          providerName={providerName}
          secretVisible={secretVisible}
          setAllowedModels={setAllowedModels}
          setModelMappings={setModelMappings}
          setProviderApiKey={(value) => {
            setProviderApiKey(value);
            setProviderApiKeyDirty(true);
            setProviderModels([]);
            setProviderTestModel("");
          }}
          setProviderApiKeyDirty={setProviderApiKeyDirty}
          setProviderBaseUrl={(value) => {
            setProviderBaseUrl(value);
            setProviderModels([]);
            setProviderTestModel("");
          }}
          setProviderName={setProviderName}
          setSecretVisible={setSecretVisible}
        />
      )}
    </main>
  );
}

function RouteScreen({
  appState,
  busy,
  onSaveClientConfig,
  onSaveMultiAgentEnabled,
  onSaveRouter,
  routerDraft,
  routerOn,
  setRouterDraft,
}: {
  appState: AppState;
  busy: boolean;
  onSaveClientConfig: (kind: AgentClientKind, enabled: boolean) => Promise<void>;
  onSaveMultiAgentEnabled: (enabled: boolean) => Promise<void>;
  onSaveRouter: (nextRouter: RouterConfig, apply?: boolean) => Promise<void>;
  routerDraft: RouterConfig;
  routerOn: boolean;
  setRouterDraft: (router: RouterConfig) => void;
}) {
  const providerNameError = routerModelProviderError(routerDraft);
  const timeoutError = routerTimeoutError(routerDraft);
  const routerSettingsValid = !providerNameError && !timeoutError;
  const updateTimeout = (
    field: "connect_timeout_secs" | "response_header_timeout_secs" | "stream_idle_timeout_secs",
    rawValue: string,
  ) => {
    const value = Number(rawValue);
    setRouterDraft({
      ...routerDraft,
      [field]: Number.isInteger(value) && value >= 0 ? value : 0,
    });
  };

  return (
    <section className="route-page">
      <div className="section-title">
        <h2>路由配置</h2>
        <button className="ghost">测试完整链路</button>
      </div>

      <div className="route-config-grid">
        <article className="route-card route-codex-card">
          <div className="route-card-head">
            <h3>Codex 接管</h3>
            <span className={`state-pill ${appState.clients.codex.enabled ? "ok" : "warn"}`}>
              <span />
              {appState.clients.codex.enabled ? "已接管" : "未接管"}
            </span>
          </div>
          <label className="compact-field">
            <span>配置文件</span>
            <div className="copy-field">
              <strong>{appState.codex_config_path}</strong>
              <button className="ghost small">打开文件</button>
            </div>
          </label>
          <label className="compact-field">
            <span>当前接管地址</span>
            <div className="copy-field accent">
              <strong>{routeBaseUrl(routerDraft)}</strong>
              <button className="ghost small">复制</button>
            </div>
          </label>
          <label className="compact-field route-provider-field">
            <span>Provider 名称</span>
            <input
              aria-invalid={Boolean(providerNameError)}
              maxLength={64}
              spellCheck={false}
              value={routerDraft.model_provider}
              onChange={(event) =>
                setRouterDraft({ ...routerDraft, model_provider: event.currentTarget.value })
              }
            />
            {providerNameError ? <small>{providerNameError}</small> : null}
          </label>
          <div className="route-diff-row">
            <span>openai_base_url → 本地代理地址</span>
            <button className="ghost small">查看变更</button>
            <Toggle
              checked={appState.clients.codex.enabled}
              disabled={busy}
              onChange={(checked) => void onSaveClientConfig("codex", checked)}
            />
          </div>
          <div className="route-toggle-line">
            <div>
              <strong>启用远程压缩</strong>
              <p>
                开启时将 model_providers.{routerDraft.model_provider || "custom"}.name 设为
                OpenAI；关闭时与 Provider 名称一致。
              </p>
            </div>
            <Toggle
              checked={routerDraft.remote_compaction_enabled}
              disabled={busy}
              onChange={(remote_compaction_enabled) =>
                setRouterDraft({ ...routerDraft, remote_compaction_enabled })
              }
            />
          </div>
          <div className="route-toggle-line">
            <div>
              <strong>启用子代理</strong>
              <p>控制 Codex 的 multi_agent 功能；切换后新建任务生效。</p>
            </div>
            <Toggle
              checked={appState.multi_agent_enabled}
              disabled={busy}
              label="启用 Codex 子代理"
              onChange={(enabled) => void onSaveMultiAgentEnabled(enabled)}
            />
          </div>
        </article>

        <article className="route-card">
          <div className="route-card-head">
            <h3>Claude 接管</h3>
            <span className={`state-pill ${appState.clients.claude.enabled ? "ok" : "warn"}`}>
              <span />
              {appState.clients.claude.enabled ? "已接管" : "未接管"}
            </span>
          </div>
          <label className="compact-field">
            <span>设置文件</span>
            <div className="copy-field">
              <strong>{appState.claude_settings_path}</strong>
              <button className="ghost small">打开文件</button>
            </div>
          </label>
          <label className="compact-field">
            <span>Claude Base URL</span>
            <div className="copy-field accent">
              <strong>{claudeBaseUrl(routerDraft)}</strong>
              <button className="ghost small">复制</button>
            </div>
          </label>
          <div className="route-diff-row">
            <span>ANTHROPIC_BASE_URL → 本地代理地址</span>
            <button className="ghost small">查看变更</button>
            <Toggle
              checked={appState.clients.claude.enabled}
              disabled={busy}
              onChange={(checked) => void onSaveClientConfig("claude", checked)}
            />
          </div>
        </article>

        <article className="route-card">
          <div className="route-card-head">
            <h3>Pi 接管</h3>
            <span className={`state-pill ${appState.clients.pi.enabled ? "ok" : "warn"}`}>
              <span />
              {appState.clients.pi.enabled ? "已接管" : "未接管"}
            </span>
          </div>
          <label className="compact-field">
            <span>模型配置</span>
            <div className="copy-field">
              <strong>{appState.pi_models_path}</strong>
              <button className="ghost small">打开文件</button>
            </div>
          </label>
          <label className="compact-field">
            <span>Responses Base URL</span>
            <div className="copy-field accent">
              <strong>{routeBaseUrl(routerDraft)}</strong>
              <button className="ghost small">复制</button>
            </div>
          </label>
          <div className="route-diff-row">
            <span>models.json → xxswitch provider</span>
            <button className="ghost small">自动同步</button>
            <Toggle
              checked={appState.clients.pi.enabled}
              disabled={busy}
              onChange={(checked) => void onSaveClientConfig("pi", checked)}
            />
          </div>
        </article>

        <article className="route-card">
          <div className="route-card-head">
            <h3>本地代理</h3>
            <span className={`state-pill ${routerOn ? "ok" : "warn"}`}>
              <span />
              {routerOn ? "运行中" : "未运行"}
            </span>
          </div>
          <div className="route-form-grid">
            <label className="compact-field">
              <span>监听地址</span>
              <input
                value={routerDraft.host}
                onChange={(event) => setRouterDraft({ ...routerDraft, host: event.currentTarget.value })}
              />
            </label>
            <label className="compact-field">
              <span>监听端口</span>
              <input
                inputMode="numeric"
                value={String(routerDraft.port)}
                onChange={(event) =>
                  setRouterDraft({
                    ...routerDraft,
                    port: Number(event.currentTarget.value.replace(/\D/g, "")) || 0,
                  })
                }
              />
            </label>
          </div>
          <div className="route-timeout-settings">
            <div>
              <strong>上游超时</strong>
              <p>每个供应商独立计时，超时后继续尝试下一供应商。</p>
            </div>
            <div className="route-timeout-grid">
              <label className="compact-field">
                <span>连接超时（秒）</span>
                <input
                  aria-invalid={
                    routerDraft.connect_timeout_secs < 1 ||
                    routerDraft.connect_timeout_secs > ROUTER_TIMEOUT_LIMITS.connect_timeout_secs
                  }
                  max={ROUTER_TIMEOUT_LIMITS.connect_timeout_secs}
                  min={1}
                  step={1}
                  type="number"
                  value={routerDraft.connect_timeout_secs}
                  onChange={(event) => updateTimeout("connect_timeout_secs", event.currentTarget.value)}
                />
              </label>
              <label className="compact-field">
                <span>响应头超时（秒）</span>
                <input
                  aria-invalid={
                    routerDraft.response_header_timeout_secs < 1 ||
                    routerDraft.response_header_timeout_secs >
                      ROUTER_TIMEOUT_LIMITS.response_header_timeout_secs ||
                    routerDraft.response_header_timeout_secs < routerDraft.connect_timeout_secs
                  }
                  max={ROUTER_TIMEOUT_LIMITS.response_header_timeout_secs}
                  min={1}
                  step={1}
                  type="number"
                  value={routerDraft.response_header_timeout_secs}
                  onChange={(event) =>
                    updateTimeout("response_header_timeout_secs", event.currentTarget.value)
                  }
                />
              </label>
              <label className="compact-field">
                <span>流空闲超时（秒）</span>
                <input
                  aria-invalid={
                    routerDraft.stream_idle_timeout_secs < 1 ||
                    routerDraft.stream_idle_timeout_secs > ROUTER_TIMEOUT_LIMITS.stream_idle_timeout_secs ||
                    routerDraft.stream_idle_timeout_secs < routerDraft.response_header_timeout_secs
                  }
                  max={ROUTER_TIMEOUT_LIMITS.stream_idle_timeout_secs}
                  min={1}
                  step={1}
                  type="number"
                  value={routerDraft.stream_idle_timeout_secs}
                  onChange={(event) => updateTimeout("stream_idle_timeout_secs", event.currentTarget.value)}
                />
              </label>
            </div>
            {timeoutError ? <small>{timeoutError}</small> : null}
          </div>
          <div className="route-toggle-line">
            <div>
              <strong>启动程序后自动运行代理</strong>
            </div>
            <Toggle
              checked={routerDraft.enabled}
              disabled={busy}
              onChange={(enabled) => setRouterDraft({ ...routerDraft, enabled })}
            />
          </div>
          <div className="route-toggle-line">
            <div>
              <strong>允许局域网访问</strong>
              <p>关闭时仅本机可以连接</p>
            </div>
            <Toggle
              checked={routerDraft.host !== "127.0.0.1"}
              onChange={(checked) =>
                setRouterDraft({ ...routerDraft, host: checked ? "0.0.0.0" : "127.0.0.1" })
              }
            />
          </div>
        </article>
      </div>

      <article className="route-card route-rules-card">
        <div className="panel-head">
          <div>
            <h2>转发规则</h2>
            <p>请求按照供应商列表中的顺序选择可用上游。</p>
          </div>
        </div>
        <div className="routing-mode">
          <span>当前路由方式</span>
          <strong>按供应商顺序故障转移</strong>
          <button className="ghost small">由供应商列表顺序决定</button>
        </div>
        <div className="route-rule-line">
          <div>
            <strong>会话固定</strong>
            <p>同一 Codex 会话优先继续使用当前供应商，即将支持</p>
          </div>
          <Toggle checked={false} disabled onChange={() => undefined} />
        </div>
        <div className="route-rule-line">
          <div>
            <strong>余额不足时自动跳过</strong>
            <p>供应商余额低于其配置阈值时不再分配新请求，即将支持</p>
          </div>
          <Toggle checked={false} disabled onChange={() => undefined} />
        </div>
        <button className="route-strategy-row" type="button">
          <strong>故障转移策略</strong>
          <span>已支持网络错误、429、5xx 顺序重试；连续失败冷却即将支持</span>
          <b>›</b>
        </button>
      </article>

      <div className="route-footer-actions">
        <button className="ghost" onClick={() => setRouterDraft(appState.router)} type="button">
          恢复默认
        </button>
        <button
          className="primary"
          disabled={busy || !routerSettingsValid}
          onClick={() => onSaveRouter(routerDraft, true)}
          type="button"
        >
          保存修改
        </button>
      </div>
    </section>
  );
}


function SkillClientConfigCard({
  busy,
  client,
  onSave,
}: {
  busy: boolean;
  client: SkillClientView;
  onSave: (
    client: AgentClientKind,
    skillLocations: SkillLocationConfig[],
    managedSkillLocation: string,
  ) => Promise<void>;
}) {
  const [locationsText, setLocationsText] = useState(() =>
    client.skill_locations.map((location) => location.path).join("\n"),
  );
  const [managedLocation, setManagedLocation] = useState(client.managed_skill_location);

  useEffect(() => {
    setLocationsText(client.skill_locations.map((location) => location.path).join("\n"));
    setManagedLocation(client.managed_skill_location);
  }, [client.client, client.managed_skill_location, client.skill_locations]);

  const parsedLocations = locationsText
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((path) => ({ path, writable: true }));

  return (
    <article className="skill-card">
      <div className="card-head">
        <div>
          <h3>{client.label}</h3>
          <p>扫描 Skill Location 的直接子目录；新 exposure 只写入 Managed Skill Location。</p>
        </div>
        <button
          className="ghost"
          disabled={busy}
          onClick={() => void onSave(client.client, parsedLocations, managedLocation)}
          type="button"
        >
          保存路径
        </button>
      </div>
      <label className="field skill-field">
        <span>Skill Locations（一行一个）</span>
        <textarea
          onChange={(event) => setLocationsText(event.currentTarget.value)}
          rows={4}
          value={locationsText}
        />
      </label>
      <label className="field skill-field">
        <span>Managed Skill Location</span>
        <input
          onChange={(event) => setManagedLocation(event.currentTarget.value)}
          value={managedLocation}
        />
      </label>
      <div className="skill-location-list">
        {client.skill_locations.map((location) => (
          <span className={location.exists ? "skill-pill ok" : "skill-pill warn"} key={location.path}>
            {location.managed ? "Managed · " : "Scan · "}
            {location.exists ? "存在" : "缺失"}
          </span>
        ))}
      </div>
    </article>
  );
}

function SkillManagementScreen({
  busy,
  onDeleteSharedSkill,
  onPromote,
  onRefresh,
  onSaveClientConfig,
  onSetSharingScope,
  onUseExistingSharedSkill,
  view,
}: {
  busy: boolean;
  onDeleteSharedSkill: (skill: SharedSkillView) => Promise<void>;
  onPromote: (skill: ClientSkillView) => Promise<void>;
  onRefresh: () => Promise<void>;
  onSaveClientConfig: (
    client: AgentClientKind,
    skillLocations: SkillLocationConfig[],
    managedSkillLocation: string,
  ) => Promise<void>;
  onSetSharingScope: (skill: SharedSkillView, scope: AgentClientKind[]) => Promise<void>;
  onUseExistingSharedSkill: (skill: ClientSkillView) => Promise<void>;
  view: SkillManagementView | null;
}) {
  const clients: AgentClientKind[] = ["codex", "claude", "pi"];

  function nextScope(skill: SharedSkillView, client: AgentClientKind, checked: boolean) {
    const current = new Set(skill.sharing_scope);
    if (checked) current.add(client);
    else current.delete(client);
    return clients.filter((item) => current.has(item));
  }

  if (!view) {
    return (
      <section className="skills-page">
        <article className="page-panel empty-state">
          <h2>Skill 管理</h2>
          <p>扫描本机已知 Agent Client 的 Skill Locations，发现可共享的 Skill。</p>
          <button className="primary" disabled={busy} onClick={() => void onRefresh()} type="button">
            扫描 Skills
          </button>
        </article>
      </section>
    );
  }

  return (
    <section className="skills-page">
      <div className="section-title">
        <div>
          <h2>Skill Management</h2>
          <p className="muted">Skill Library Root: {view.library_root}</p>
        </div>
        <button className="primary" disabled={busy} onClick={() => void onRefresh()} type="button">
          重新扫描
        </button>
      </div>

      <div className="skill-client-grid">
        {view.clients.map((client) => (
          <SkillClientConfigCard
            busy={busy}
            client={client}
            key={client.client}
            onSave={onSaveClientConfig}
          />
        ))}
      </div>

      {view.conflicts.length > 0 && (
        <article className="page-panel skill-section">
          <div className="panel-head">
            <div>
              <h2>冲突</h2>
              <p>同名冲突不会自动覆盖、合并或重命名。</p>
            </div>
          </div>
          <div className="skill-list">
            {view.conflicts.map((conflict) => (
              <div className="skill-row conflict" key={`${conflict.kind}-${conflict.path}`}>
                <div>
                  <strong>{conflict.identity}</strong>
                  <small>{conflict.client ? agentClientLabel(conflict.client) : "Library"} · {conflict.path}</small>
                </div>
                <span className="skill-pill warn">{conflict.kind}</span>
                <p>{conflict.message}</p>
              </div>
            ))}
          </div>
        </article>
      )}

      <article className="page-panel skill-section">
        <div className="panel-head">
          <div>
            <h2>Shared Skills</h2>
            <p>每个 Shared Skill 的 Sharing Scope 保存后立即应用为 symlink exposure。</p>
          </div>
        </div>
        <div className="skill-list">
          {view.shared_skills.length === 0 && <p className="muted">暂无 Shared Skill。</p>}
          {view.shared_skills.map((skill) => (
            <div className="shared-skill" key={skill.identity}>
              <div className="shared-skill-main">
                <div>
                  <strong>{skill.identity}</strong>
                  <small>{skill.description || skill.path}</small>
                  {skill.origin && (
                    <small>Origin: {agentClientLabel(skill.origin.client)} · {skill.origin.original_path}</small>
                  )}
                </div>
                <button className="ghost danger" disabled={busy} onClick={() => void onDeleteSharedSkill(skill)} type="button">
                  删除
                </button>
              </div>
              <div className="scope-row">
                {clients.map((client) => (
                  <label key={client}>
                    <input
                      checked={skill.sharing_scope.includes(client)}
                      disabled={busy}
                      onChange={(event) =>
                        void onSetSharingScope(skill, nextScope(skill, client, event.currentTarget.checked))
                      }
                      type="checkbox"
                    />
                    {agentClientLabel(client)}
                  </label>
                ))}
              </div>
              <div className="exposure-list">
                {skill.exposures.map((exposure) => (
                  <div className="exposure-row" key={`${skill.identity}-${exposure.client}-${exposure.path}`}>
                    <span className={`skill-pill ${exposure.health === "healthy" ? "ok" : "warn"}`}>
                      {exposure.client_label} · {exposure.health}
                    </span>
                    <code>{exposure.path || "未创建"}</code>
                    <small>{exposure.message}</small>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </article>

      <article className="page-panel skill-section">
        <div className="panel-head">
          <div>
            <h2>Client Skills</h2>
            <p>从已知 Agent Client 的 Skill Locations 发现；公用后会进入 Skill Library。</p>
          </div>
        </div>
        <div className="skill-list">
          {view.client_skills.length === 0 && <p className="muted">暂无未共享 Client Skill。</p>}
          {view.client_skills.map((skill) => (
            <div className="skill-row" key={`${skill.client}-${skill.path}`}>
              <div>
                <strong>{skill.identity}</strong>
                <small>{skill.client_label} · {skill.path}</small>
                {skill.description && <small>{skill.description}</small>}
              </div>
              {skill.shared ? (
                <button
                  className="ghost"
                  disabled={busy}
                  onClick={() => void onUseExistingSharedSkill(skill)}
                  type="button"
                >
                  使用已有 Shared Skill
                </button>
              ) : (
                <button className="primary" disabled={busy} onClick={() => void onPromote(skill)} type="button">
                  公用
                </button>
              )}
            </div>
          ))}
        </div>
      </article>
    </section>
  );
}

function UsageScreen({
  configuredProviders,
  filter,
  onFilter,
  onRangeChange,
  onRefresh,
  onTrendMetricChange,
  stats,
  timeRange,
  trendMetric,
}: {
  configuredProviders: ProviderSummary[];
  filter: RouteLogFilter;
  onFilter: (patch: Partial<RouteLogFilter>) => Promise<void>;
  onRangeChange: (range: TimeRange) => Promise<void>;
  onRefresh: () => void;
  onTrendMetricChange: (metric: TrendMetric) => void;
  stats: RouteUsageStats | null;
  timeRange: TimeRange;
  trendMetric: TrendMetric;
}) {
  const summary = stats?.summary ?? {
    request_count: 0,
    input_tokens: 0,
    uncached_input_tokens: 0,
    cached_input_tokens: 0,
    output_tokens: 0,
    reasoning_output_tokens: 0,
    total_tokens: 0,
  };
  const trendBuckets = stats?.buckets.length
    ? stats.buckets
    : [emptyTrendBucket("00:00")];
  const totalCalls = (stats?.models ?? []).reduce((total, row) => total + row.request_count, 0) || 1;
  const availableProviders = usageProviderOptions(
    stats?.available_providers ?? [],
    configuredProviders,
  );

  return (
    <section className="usage-page">
      <div className="section-title">
        <h2>统计概览</h2>
        <button className="ghost" onClick={onRefresh} type="button">
          重新统计
        </button>
      </div>

      <div className="usage-filter-bar">
        <div className="range-tabs">
          {(["today", "week", "month", "all"] as TimeRange[]).map((range) => (
            <button
              className={timeRange === range ? "active" : ""}
              key={range}
              onClick={() => onRangeChange(range)}
              type="button"
            >
              {rangeLabel(range)}
            </button>
          ))}
        </div>
        <select value={filter.provider_id ?? ""} onChange={(event) => onFilter({ provider_id: event.currentTarget.value })}>
          <option value="">全部供应商</option>
          {availableProviders.map((provider) => (
            <option key={provider.id} value={provider.id}>{provider.name}</option>
          ))}
        </select>
        <select value={filter.model ?? ""} onChange={(event) => onFilter({ model: event.currentTarget.value })}>
          <option value="">全部模型</option>
          {(stats?.available_models ?? []).map((model) => (
            <option key={model} value={model}>{model}</option>
          ))}
        </select>
        <button className="ghost" type="button">{bucketGranularityLabel(stats?.bucket_granularity)}</button>
        <small>本程序路由日志</small>
      </div>

      <div className="metric-grid">
        <Metric title="调用次数" value={String(stats?.success_count ?? summary.request_count)} tone="green" />
        <Metric title="总 Token" value={formatCompact(summary.total_tokens)} tone="purple" sub={rangeLabel(timeRange)} />
        <Metric title="非缓存输入" value={formatCompact(summary.uncached_input_tokens)} tone="cyan" sub={`${formatCompact(summary.input_tokens)} 输入`} />
        <Metric title="输出 Token" value={formatCompact(summary.output_tokens)} tone="amber" sub="路由响应" />
      </div>

      <div className="usage-main-grid">
        <article className="card route-trend-card">
          <div className="card-head">
            <div>
              <h3>使用趋势</h3>
              <p>{rangeLabel(timeRange)} {bucketGranularityLabel(stats?.bucket_granularity)}的{trendMetricLabel(trendMetric)}</p>
            </div>
            <div className="mini-tabs">
              {(["tokens", "requests"] as TrendMetric[]).map((metric) => (
                <button
                  className={trendMetric === metric ? "active" : ""}
                  key={metric}
                  onClick={() => onTrendMetricChange(metric)}
                  type="button"
                >
                  {trendMetricLabel(metric)}
                </button>
              ))}
            </div>
          </div>
          <TrendLineChart
            buckets={trendBuckets}
            granularity={stats?.bucket_granularity}
            metric={trendMetric}
            range={timeRange}
          />
        </article>

        <article className="card call-distribution">
          <div className="card-head">
            <div>
              <h3>调用分布</h3>
              <p>按模型统计成功调用</p>
            </div>
            <div className="mini-tabs">
              <button className="active">按模型</button>
              <button>按供应商</button>
            </div>
          </div>
          {(stats?.models ?? []).slice(0, 4).map((row) => (
            <div className="distribution-row" key={row.key}>
              <div>
                <strong>{row.label}</strong>
                <b>{row.request_count} 次</b>
              </div>
              <span><i style={{ width: `${Math.max(3, (row.request_count / totalCalls) * 100)}%` }} /></span>
              <small>{Math.round((row.request_count / totalCalls) * 100)}%</small>
            </div>
          ))}
        </article>
      </div>

      <article className="route-table-card usage-detail-card">
        <div className="panel-head">
          <div>
            <h2>用量明细</h2>
            <p>展示经过 XXSwitch 转发并成功记录 Token 的调用。</p>
          </div>
        </div>
        <div className="usage-detail-table">
          <header>
            <span>时间</span>
            <span>模型</span>
            <span>供应商</span>
            <span>非缓存输入</span>
            <span>缓存输入</span>
            <span>输出</span>
            <span>请求</span>
            <span>总 Token</span>
          </header>
          {(stats?.details ?? []).slice(0, 8).map((log) => (
            <div key={log.id}>
              <span>{formatLogTime(log.started_at_ms)}</span>
              <strong>{log.model}</strong>
              <span>{log.provider_name}</span>
              <span>{formatTokenCount(log.uncached_input_tokens)}</span>
              <span>{formatTokenCount(log.cached_input_tokens)}</span>
              <span>{formatTokenCount(log.output_tokens)}</span>
              <b>{log.status === "success" ? 1 : 0}</b>
              <b>{formatTokenCount(log.total_tokens)}</b>
            </div>
          ))}
        </div>
      </article>
    </section>
  );
}

function RequestLogsScreen({
  autoRefresh,
  filter,
  logs,
  onFilter,
  onRefresh,
  onSetAutoRefresh,
  onToday,
}: {
  autoRefresh: boolean;
  filter: RouteLogFilter;
  logs: RouteLogsResponse | null;
  onFilter: (patch: Partial<RouteLogFilter>) => Promise<void>;
  onRefresh: () => void;
  onSetAutoRefresh: (enabled: boolean) => void;
  onToday: () => Promise<void>;
}) {
  const rows = logs?.logs ?? [];
  const [selectedLog, setSelectedLog] = useState<RouteRequestLog | null>(null);

  return (
    <section className="requests-page">
      <div className="section-title">
        <h2>请求列表</h2>
        <button className="ghost" onClick={() => exportRouteLogsCsv(rows)} type="button">
          导出当前页 CSV
        </button>
      </div>
      <div className="request-filter-bar">
        <div className="search-box">
          <span />
          <input
            placeholder="搜索请求 ID 或错误信息"
            value={filter.query ?? ""}
            onChange={(event) => onFilter({ query: event.currentTarget.value })}
          />
        </div>
        <select value={filter.status ?? ""} onChange={(event) => onFilter({ status: event.currentTarget.value })}>
          <option value="">全部状态</option>
          <option value="success">成功</option>
          <option value="failed">失败</option>
          <option value="cancelled">已取消</option>
        </select>
        <select value={filter.provider_id ?? ""} onChange={(event) => onFilter({ provider_id: event.currentTarget.value })}>
          <option value="">全部供应商</option>
          {(logs?.available_providers ?? []).map((provider) => (
            <option key={provider.id} value={provider.id}>{provider.name}</option>
          ))}
        </select>
        <select value={filter.model ?? ""} onChange={(event) => onFilter({ model: event.currentTarget.value })}>
          <option value="">全部模型</option>
          {(logs?.available_models ?? []).map((model) => (
            <option key={model} value={model}>{model}</option>
          ))}
        </select>
        <button className="ghost" onClick={onToday} type="button">今日</button>
        <span>实时刷新</span>
        <Toggle
          checked={autoRefresh}
          onChange={(checked) => {
            onSetAutoRefresh(checked);
            if (checked) onRefresh();
          }}
        />
        <small>共 {logs?.total ?? 0} 条</small>
      </div>

      <article className="route-table-card request-log-card">
        <div className="request-log-table">
          <header>
            <span>状态</span>
            <span>时间</span>
            <span>模型</span>
            <span>供应商</span>
            <span>Token（非缓存 / 缓存 / 输出）</span>
            <span>首字延迟</span>
            <span>总耗时</span>
            <span>路由结果</span>
          </header>
          {rows.map((log) => {
            const status = statusMeta(log.status);
            return (
              <div
                aria-label={`查看请求 ${log.id} 详情`}
                className={[
                  log.route_attempts > 1 ? "selected" : "",
                  selectedLog?.id === log.id ? "active" : "",
                ].filter(Boolean).join(" ")}
                key={log.id}
                onClick={() => setSelectedLog(log)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    setSelectedLog(log);
                  }
                }}
                role="button"
                tabIndex={0}
              >
                <div className="status-cell">
                  <span className={`dot ${status.tone}`} />
                  <b className={`${status.tone}-text`}>{status.label}</b>
                </div>
                <strong>{formatLogTime(log.started_at_ms)}</strong>
                <strong>{log.model}</strong>
                <strong>{log.provider_name}</strong>
                <span>{formatTokenTriplet(log)}</span>
                <b>{formatMs(log.first_byte_ms)}</b>
                <b title={formatTimingDetails(log)}>{formatDuration(log.total_ms)}</b>
                <span className={`route-result ${routeResultTone(log.route_result)}`}>
                  {log.route_result}
                </span>
              </div>
            );
          })}
        </div>
        <footer className="table-pagination">
          <span>点击任意记录查看路由时间线、Token 明细和错误详情。</span>
          <div>
            <button className="ghost small" disabled={(logs?.page ?? 1) <= 1} onClick={() => onFilter({ page: (logs?.page ?? 1) - 1 })}>‹</button>
            <b>{logs?.page ?? 1}</b>
            <button className="ghost small" disabled={(logs?.page ?? 1) >= (logs?.total_pages ?? 1)} onClick={() => onFilter({ page: (logs?.page ?? 1) + 1 })}>›</button>
          </div>
        </footer>
      </article>
      {selectedLog ? <RequestLogDialog log={selectedLog} onClose={() => setSelectedLog(null)} /> : null}
    </section>
  );
}

function RequestLogDialog({ log, onClose }: { log: RouteRequestLog; onClose: () => void }) {
  const status = statusMeta(log.status);
  const phases = routeTimingPhases(log);
  const compactionAudit = remoteCompactionV2AuditLabel(log);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  return (
    <div
      className="latency-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        aria-labelledby="request-log-dialog-title"
        aria-modal="true"
        className="latency-dialog request-log-dialog"
        role="dialog"
      >
        <header>
          <div className="request-log-dialog-heading">
            <div>
              <h2 id="request-log-dialog-title">请求详情</h2>
              <p title={log.id}>{log.id}</p>
            </div>
            <span className={`request-detail-status ${status.tone}`}>
              <span className={`dot ${status.tone}`} />
              {status.label}
            </span>
          </div>
          <button aria-label="关闭" className="close" onClick={onClose} title="关闭" type="button">×</button>
        </header>

        <div className="latency-dialog-body request-log-dialog-body">
          <section className="request-detail-section">
            <header>
              <h3>请求概况</h3>
              <span>{formatLogDateTime(log.started_at_ms)}</span>
            </header>
            <div className="request-detail-grid">
              <div><span>请求</span><strong>{log.method} {log.path}</strong></div>
              <div><span>状态码</span><strong>{log.status_code ?? "-"}</strong></div>
              <div><span>模型</span><strong>{log.model || "-"}</strong></div>
              <div><span>上游模型</span><strong>{log.upstream_model || log.model || "-"}</strong></div>
              <div><span>供应商</span><strong>{log.provider_name}</strong></div>
              <div><span>路由尝试</span><strong>{log.route_attempts} 次</strong></div>
            </div>
          </section>

          <section className="request-detail-section">
            <header>
              <h3>耗时阶段</h3>
              <strong>{formatDuration(log.total_ms)}</strong>
            </header>
            {phases.length > 0 ? (
              <div className="request-timing-list">
                {phases.map((phase) => {
                  const width = log.total_ms > 0
                    ? Math.max(phase.durationMs > 0 ? 2 : 0, (phase.durationMs / log.total_ms) * 100)
                    : 0;
                  return (
                    <div className="request-timing-row" key={phase.label}>
                      <span>{phase.label}</span>
                      <div><i style={{ width: `${Math.min(100, width)}%` }} /></div>
                      <strong>{formatDuration(phase.durationMs)}</strong>
                    </div>
                  );
                })}
              </div>
            ) : (
              <p className="request-detail-empty">该记录创建时尚未采集分段耗时。</p>
            )}
          </section>

          <section className="request-detail-section">
            <header><h3>Token 明细</h3></header>
            <div className="request-token-grid">
              <div><span>输入</span><strong>{formatTokenCount(log.input_tokens)}</strong></div>
              <div><span>非缓存</span><strong>{formatTokenCount(log.uncached_input_tokens)}</strong></div>
              <div><span>缓存</span><strong>{formatTokenCount(log.cached_input_tokens)}</strong></div>
              <div><span>输出</span><strong>{formatTokenCount(log.output_tokens)}</strong></div>
              <div><span>推理输出</span><strong>{formatTokenCount(log.reasoning_output_tokens)}</strong></div>
              <div><span>总计</span><strong>{formatTokenCount(log.total_tokens)}</strong></div>
            </div>
          </section>

          <section className="request-detail-section">
            <header><h3>路由详情</h3></header>
            <dl className="request-route-details">
              <div><dt>路由结果</dt><dd>{log.route_result}</dd></div>
              <div><dt>上游链路</dt><dd>{log.upstream_chain.join(" → ") || "-"}</dd></div>
              <div><dt>本地请求 ID</dt><dd><code>{log.id}</code></dd></div>
              <div><dt>上游请求 ID</dt><dd><code>{log.upstream_request_id || "未返回"}</code></dd></div>
              {compactionAudit ? <div><dt>远程压缩</dt><dd>{compactionAudit}</dd></div> : null}
            </dl>
          </section>

          {log.error ? (
            <section className="request-detail-section request-error-detail">
              <header><h3>错误详情</h3></header>
              <pre>{log.error}</pre>
            </section>
          ) : null}
        </div>

        <footer>
          <button className="primary" onClick={onClose} type="button">关闭</button>
        </footer>
      </section>
    </div>
  );
}

function Dashboard(props: {
  activeProvider: ProviderSummary | null;
  cachedInput: number;
  modelRows: Array<{ model: string; requests: number; tokens: number }>;
  onRangeChange: (range: TimeRange) => Promise<void>;
  onTrendMetricChange: (metric: TrendMetric) => void;
  outputTokens: number;
  providers: ProviderSummary[];
  requestCount: number;
  stats: RouteUsageStats | null;
  successRate: number;
  timeRange: TimeRange;
  totalTokens: number;
  trendMetric: TrendMetric;
  uncachedInput: number;
}) {
  const {
    cachedInput,
    modelRows,
    onRangeChange,
    onTrendMetricChange,
    outputTokens,
    providers,
    requestCount,
    stats,
    successRate,
    timeRange,
    totalTokens,
    trendMetric,
    uncachedInput,
  } = props;
  const availableProviders = providers.filter((provider) => provider.enabled && provider.status === "enabled").length;
  const successCount = stats?.success_count ?? Math.max(requestCount - (stats?.failed_count ?? 0), 0);
  const activeRequests = stats?.running_count ?? 0;
  const avgFirstByte = stats?.average_first_byte_ms ?? null;
  const avgTotalMs = stats?.average_total_ms ?? null;
  const trendBuckets = stats?.buckets.length
    ? stats.buckets
    : [emptyTrendBucket("暂无")];
  const tokenTooltip = [
    `非缓存输入: ${formatTokenCount(uncachedInput)}`,
    `缓存输入: ${formatTokenCount(cachedInput)}`,
    `输出: ${formatTokenCount(outputTokens)}`,
  ].join("\n");

  return (
    <section className="dashboard">
      <div className="section-title">
        <h2>使用概览</h2>
        <div className="range-tabs">
          {(["today", "week", "month", "all"] as TimeRange[]).map((range) => (
            <button
              className={timeRange === range ? "active" : ""}
              key={range}
              onClick={() => onRangeChange(range)}
              type="button"
            >
              {rangeLabel(range)}
            </button>
          ))}
        </div>
      </div>

      <div className="metric-grid">
        <Metric title="请求数" value={String(Math.max(successCount, 0))} tone="green" />
        <Metric title="总 Token" value={formatCompact(totalTokens)} tone="cyan" tooltip={tokenTooltip} />
        <Metric title="成功率" value={`${successRate.toFixed(1)}%`} tone="purple" />
      </div>

      <div className="dashboard-grid">
        <article className="card trend-card">
          <div className="card-head">
            <div>
              <h3>使用趋势</h3>
              <p>{rangeLabel(timeRange)} {bucketGranularityLabel(stats?.bucket_granularity)}的{trendMetricLabel(trendMetric)}</p>
            </div>
            <div className="mini-tabs">
              {(["tokens", "requests"] as TrendMetric[]).map((metric) => (
                <button
                  className={trendMetric === metric ? "active" : ""}
                  key={metric}
                  onClick={() => onTrendMetricChange(metric)}
                  type="button"
                >
                  {trendMetricLabel(metric)}
                </button>
              ))}
            </div>
          </div>
          <TrendLineChart
            buckets={trendBuckets}
            compact
            granularity={stats?.bucket_granularity}
            metric={trendMetric}
            range={timeRange}
          />
        </article>

        <article className="card provider-card">
          <h3>供应商</h3>
          <div className="provider-health-list">
            {providers.slice(0, 4).map((provider) => {
              const status = providerStatus(provider);
              const balance = providerBalanceMeta(provider);
              const latency = providerLatencyMeta(provider);
              return (
                <div className="provider-health" key={provider.id}>
                  <span className={`dot ${status.tone}`} />
                  <div className="provider-health-summary">
                    <strong>{provider.name}</strong>
                    <small>{status.label}</small>
                  </div>
                  <div className="provider-health-facts">
                    <span className={`provider-health-fact ${balance.tone}`} title={provider.balance_error ?? undefined}>
                      <small>余额</small>
                      <b>{balance.value}</b>
                    </span>
                    <span className={`provider-health-fact ${latency.tone}`} title={provider.latency_error ?? undefined}>
                      <small>延迟</small>
                      <b>{latency.value}</b>
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
          <div className="health-bar">
            <span style={{ width: `${providers.length ? (availableProviders / providers.length) * 100 : 0}%` }} />
          </div>
          <footer>
            <span>可用供应商 {availableProviders}/{providers.length}</span>
            <b>{providers.length ? Math.round((availableProviders / providers.length) * 100) : 0}%</b>
          </footer>
        </article>

        <article className="card donut-card">
          <h3>Token 构成</h3>
          <p>{rangeLabel(timeRange)}共计 {formatCompact(totalTokens)} Token</p>
          <div className="donut-area">
            <div className="donut">
              <strong>{formatCompact(totalTokens)}</strong>
              <span>总 Token</span>
            </div>
            <div className="legend">
              <Legend color="cyan" label="非缓存输入" value={formatCompact(uncachedInput)} />
              <Legend color="blue" label="缓存输入" value={formatCompact(cachedInput)} />
              <Legend color="amber" label="输出" value={formatCompact(outputTokens)} />
            </div>
          </div>
        </article>

        <article className="card model-card">
          <h3>模型用量</h3>
          <div className="model-table">
            <header>
              <span>模型</span>
              <span>请求</span>
              <span>Token</span>
              <span>占比</span>
            </header>
            {(modelRows.length ? modelRows : [{ model: "gpt-5.5", requests: 0, tokens: 0 }]).map((row, index) => (
              <div className="model-row" key={row.model}>
                <strong>{row.model}</strong>
                <span>{row.requests}</span>
                <span>{formatCompact(row.tokens)}</span>
                <b>{totalTokens ? `${Math.round((row.tokens / totalTokens) * 100)}%` : "0%"}</b>
                <i style={{ width: `${Math.max(18, 92 - index * 27)}%` }} />
              </div>
            ))}
          </div>
        </article>

        <article className="card quality-card">
          <h3>请求质量</h3>
          <p>{rangeLabel(timeRange)}通过本地网关的请求</p>
          <Quality label="成功率" value={`${successRate.toFixed(1)}%`} tone="green" />
          <Quality label="平均首字延迟" value={formatMs(avgFirstByte)} tone="cyan" />
          <Quality label="平均总耗时" value={avgTotalMs == null ? "-" : formatDuration(avgTotalMs)} tone="purple" />
          <Quality label="当前活跃请求" value={String(activeRequests)} tone="amber" />
        </article>
      </div>
    </section>
  );
}

function Metric({
  title,
  value,
  tone,
  sub,
  tooltip,
}: {
  title: string;
  value: string;
  tone: "purple" | "cyan" | "blue" | "amber" | "green";
  sub?: string;
  tooltip?: string;
}) {
  return (
    <article className="metric" title={tooltip}>
      <span className={`dot ${tone}`} />
      <p>{title}</p>
      <strong>{value}</strong>
      {sub && <small>{sub}</small>}
    </article>
  );
}

function TrendLineChart({
  buckets,
  compact,
  granularity,
  metric,
  range,
}: {
  buckets: RouteUsageBucket[];
  compact?: boolean;
  granularity?: string;
  metric: TrendMetric;
  range: TimeRange;
}) {
  const gradientId = `${useId().replace(/:/g, "")}-${metric}`;
  const [hoveredPoint, setHoveredPoint] = useState<{
    bucket: RouteUsageBucket;
    x: number;
    y: number;
  } | null>(null);
  const effectiveGranularity = granularity ?? (range === "all" ? "month" : range === "today" ? "hour" : "day");
  const completedBuckets = completeTrendBuckets(buckets, range, granularity);
  const values = completedBuckets.map((bucket) => trendBucketValue(bucket, metric));
  const rawMax = Math.max(...values, 0);
  const yMax = niceTrendMax(rawMax);
  const chartWidth = 640;
  const chartHeight = compact ? 172 : 210;
  const padding = { top: 12, right: 16, bottom: 24, left: 72 };
  const plotWidth = chartWidth - padding.left - padding.right;
  const plotHeight = chartHeight - padding.top - padding.bottom;
  const xFor = (index: number) =>
    padding.left + (completedBuckets.length <= 1 ? plotWidth / 2 : (index / (completedBuckets.length - 1)) * plotWidth);
  const yFor = (value: number) => padding.top + plotHeight - (yMax ? (value / yMax) * plotHeight : 0);
  const points = completedBuckets.map((bucket, index) => ({
    bucket,
    value: values[index] ?? 0,
    x: xFor(index),
    y: yFor(values[index] ?? 0),
  }));
  const linePoints = points.map((point) => `${point.x},${point.y}`).join(" ");
  const areaPoints = [
    `${padding.left},${padding.top + plotHeight}`,
    ...points.map((point) => `${point.x},${point.y}`),
    `${padding.left + plotWidth},${padding.top + plotHeight}`,
  ].join(" ");
  const yTicks = [1, 0.75, 0.5, 0.25, 0].map((ratio) => ({
    ratio,
    value: yMax * ratio,
    y: padding.top + plotHeight - ratio * plotHeight,
  }));
  const xTickIndexes = trendTickIndexes(completedBuckets.length);

  return (
    <div className="trend-line-chart">
      <div className="trend-y-axis">
        {yTicks.map((tick) => (
          <span key={tick.ratio} style={{ top: `${(tick.y / chartHeight) * 100}%` }}>
            {formatTrendAxisValue(tick.value, metric)}
          </span>
        ))}
      </div>
      <svg className="trend-svg" viewBox={`0 0 ${chartWidth} ${chartHeight}`} role="img" aria-label={`${trendMetricLabel(metric)}趋势`}>
        <defs>
          <linearGradient id={gradientId} x1="0" x2="0" y1="0" y2="1">
            <stop offset="0%" stopColor="#0f8f68" stopOpacity="0.22" />
            <stop offset="100%" stopColor="#0f8f68" stopOpacity="0.02" />
          </linearGradient>
        </defs>
        {yTicks.map((tick) => (
          <line
            className="trend-grid-line"
            key={tick.ratio}
            x1={padding.left}
            x2={padding.left + plotWidth}
            y1={tick.y}
            y2={tick.y}
          />
        ))}
        <line className="trend-axis-line" x1={padding.left} x2={padding.left} y1={padding.top} y2={padding.top + plotHeight} />
        <line className="trend-axis-line" x1={padding.left} x2={padding.left + plotWidth} y1={padding.top + plotHeight} y2={padding.top + plotHeight} />
        <polygon className="trend-area" fill={`url(#${gradientId})`} points={areaPoints} />
        <polyline className="trend-line" points={linePoints} />
        {points.map((point, index) => (
          <circle
            className="trend-point"
            cx={point.x}
            cy={point.y}
            key={`${point.bucket.label}-${index}`}
            onBlur={() => setHoveredPoint(null)}
            onFocus={() => setHoveredPoint({ bucket: point.bucket, x: point.x, y: point.y })}
            onMouseEnter={() => setHoveredPoint({ bucket: point.bucket, x: point.x, y: point.y })}
            onMouseLeave={() => setHoveredPoint(null)}
            r="5"
            tabIndex={0}
          />
        ))}
        {xTickIndexes.map((index) => {
          const point = points[index];
          const isFirst = index === 0;
          const isLast = index === completedBuckets.length - 1;
          return (
            <text
              className="trend-x-tick"
              key={`${point.bucket.label}-${index}`}
              textAnchor={isFirst ? "start" : isLast ? "end" : "middle"}
              x={point.x}
              y={chartHeight - 4}
            >
              {formatTrendXAxisLabel(point.bucket.label, effectiveGranularity)}
            </text>
          );
        })}
      </svg>
      {hoveredPoint && (
        <div
          className="trend-tooltip"
          style={{
            left: `${(hoveredPoint.x / chartWidth) * 100}%`,
            top: `${(hoveredPoint.y / chartHeight) * 100}%`,
          }}
        >
          <strong>{formatTrendXAxisLabel(hoveredPoint.bucket.label, effectiveGranularity)}</strong>
          <span>
            {metric === "tokens"
              ? `${formatTokenCount(hoveredPoint.bucket.total_tokens)} Token`
              : `${hoveredPoint.bucket.request_count} 次调用`}
          </span>
          <small>{formatTokenCount(hoveredPoint.bucket.total_tokens)} Token · {hoveredPoint.bucket.request_count} 次</small>
        </div>
      )}
    </div>
  );
}

function Legend({ color, label, value }: { color: string; label: string; value: string }) {
  return (
    <div className="legend-row">
      <span className={`dot ${color}`} />
      <em>{label}</em>
      <strong>{value}</strong>
    </div>
  );
}

function Quality({ label, value, tone }: { label: string; value: string; tone: string }) {
  return (
    <div className="quality-row">
      <span className={`dot ${tone}`} />
      <em>{label}</em>
      <strong>{value}</strong>
    </div>
  );
}

function ProviderLatencyDialog({
  onClose,
  onModelChange,
  onPromptChange,
  onStreamingChange,
  onTest,
  state,
}: {
  onClose: () => void;
  onModelChange: (model: string) => void;
  onPromptChange: (prompt: string) => void;
  onStreamingChange: (streaming: boolean) => void;
  onTest: () => void;
  state: ProviderLatencyDialogState;
}) {
  const canTest = !state.preparing && !state.testing && Boolean(state.selectedModel) && Boolean(state.prompt.trim());
  return (
    <div
      className="latency-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !state.testing) onClose();
      }}
    >
      <section aria-labelledby="latency-dialog-title" aria-modal="true" className="latency-dialog" role="dialog">
        <header>
          <div className="latency-dialog-title">
            <span><ToolIcon type="latency" /></span>
            <div>
              <h2 id="latency-dialog-title">模型测速</h2>
              <p>{state.provider.name} · {state.provider.base_url}</p>
            </div>
          </div>
          <button aria-label="关闭" className="close" disabled={state.testing} onClick={onClose} title="关闭" type="button">×</button>
        </header>

        <div className="latency-dialog-body">
          <div className="latency-mode-field">
            <span>请求方式</span>
            <div aria-label="请求方式" className="latency-mode-control" role="group">
              <button
                className={!state.streaming ? "active" : ""}
                disabled={state.testing}
                onClick={() => onStreamingChange(false)}
                type="button"
              >
                同步
              </button>
              <button
                className={state.streaming ? "active" : ""}
                disabled={state.testing}
                onClick={() => onStreamingChange(true)}
                type="button"
              >
                流式
              </button>
            </div>
          </div>

          <label className="field">
            <span>测试模型</span>
            <select
              disabled={state.preparing || state.testing || state.models.length === 0}
              value={state.selectedModel}
              onChange={(event) => onModelChange(event.currentTarget.value)}
            >
              {state.preparing && <option value="">正在读取模型…</option>}
              {!state.preparing && state.models.length === 0 && <option value="">没有可用模型</option>}
              {state.models.map((model) => <option key={model} value={model}>{model}</option>)}
            </select>
          </label>

          <label className="field">
            <span>测试内容</span>
            <textarea
              disabled={state.testing}
              maxLength={2000}
              rows={4}
              value={state.prompt}
              onChange={(event) => onPromptChange(event.currentTarget.value)}
            />
            <small>{state.prompt.length} / 2000</small>
          </label>

          {state.result && (
            <div aria-live="polite" className={`latency-test-result ${state.result.ok ? "ok" : "failed"}`}>
              <div className="latency-result-summary">
                <span className={`dot ${state.result.ok ? "green" : "danger"}`} />
                <div>
                  <strong>{state.result.ok ? `${state.result.latencyMs} ms` : "测速失败"}</strong>
                  <p>{state.result.message}</p>
                </div>
              </div>
              {state.result.ok && (
                <div className="latency-reply">
                  <span>模型回复</span>
                  <pre>{state.result.reply?.trim() || "未返回文本内容"}</pre>
                </div>
              )}
            </div>
          )}
        </div>

        <footer>
          <button className="ghost" disabled={state.testing} onClick={onClose} type="button">取消</button>
          <button className="primary" disabled={!canTest} onClick={onTest} type="button">
            {state.testing ? "正在测速…" : state.result ? "重新测速" : "开始测速"}
          </button>
        </footer>
      </section>
    </div>
  );
}

function ProvidersScreen({
  busy,
  onAdd,
  onEdit,
  onKindChange,
  onRefreshBalance,
  onReorder,
  onTestLatency,
  onToggle,
  providerKind,
  providers,
}: {
  busy: boolean;
  onAdd: () => void;
  onEdit: (provider: ProviderSummary, tab: EditorTab) => void;
  onKindChange: (kind: ProviderKind) => void;
  onRefreshBalance: (provider: ProviderSummary) => Promise<void>;
  onReorder: (providerIds: string[]) => Promise<void>;
  onTestLatency: (provider: ProviderSummary) => Promise<void>;
  onToggle: (provider: ProviderSummary, enabled: boolean) => void;
  providerKind: ProviderKind;
  providers: ProviderSummary[];
}) {
  const [draggingProviderId, setDraggingProviderId] = useState<string | null>(null);
  const [dragOverProviderId, setDragOverProviderId] = useState<string | null>(null);
  const draggingProviderIdRef = useRef<string | null>(null);
  const dragStartY = useRef(0);

  function nextProviderOrder(draggedId: string, clientY: number) {
    const providerIds = providers.map((provider) => provider.id);
    const draggedIndex = providerIds.indexOf(draggedId);
    if (draggedIndex < 0 || providerIds.length < 2) return providerIds;

    const rows = Array.from(document.querySelectorAll<HTMLElement>("[data-provider-row-id]"));
    let insertIndex = providerIds.length;
    for (const row of rows) {
      const rowId = row.dataset.providerRowId;
      if (!rowId || rowId === draggedId) continue;
      const bounds = row.getBoundingClientRect();
      if (clientY < bounds.top + bounds.height / 2) {
        insertIndex = providerIds.indexOf(rowId);
        break;
      }
    }

    const nextIds = [...providerIds];
    const [movedId] = nextIds.splice(draggedIndex, 1);
    if (insertIndex > draggedIndex) {
      insertIndex -= 1;
    }
    nextIds.splice(Math.max(0, Math.min(insertIndex, nextIds.length)), 0, movedId);
    return nextIds;
  }

  function finishPointerReorder(providerId: string, clientY: number) {
    if (draggingProviderIdRef.current !== providerId) return;
    draggingProviderIdRef.current = null;
    setDraggingProviderId(null);
    setDragOverProviderId(null);
    if (Math.abs(clientY - dragStartY.current) < 4) return;

    const currentIds = providers.map((provider) => provider.id);
    const nextIds = nextProviderOrder(providerId, clientY);
    if (nextIds.join("\u0000") !== currentIds.join("\u0000")) {
      void onReorder(nextIds);
    }
  }

  function handlePointerDown(event: PointerEvent<HTMLButtonElement>, providerId: string) {
    if (busy) return;
    event.preventDefault();
    dragStartY.current = event.clientY;
    draggingProviderIdRef.current = providerId;
    setDraggingProviderId(providerId);
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function handlePointerUp(event: PointerEvent<HTMLButtonElement>, providerId: string) {
    event.preventDefault();
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    finishPointerReorder(providerId, event.clientY);
  }

  function handlePointerMove(event: PointerEvent<HTMLButtonElement>, providerId: string) {
    if (draggingProviderIdRef.current !== providerId) return;
    const targetRow = Array.from(
      document.querySelectorAll<HTMLElement>("[data-provider-row-id]"),
    ).find((row) => {
      if (row.dataset.providerRowId === providerId) return false;
      const bounds = row.getBoundingClientRect();
      return event.clientY >= bounds.top && event.clientY <= bounds.bottom;
    });
    setDragOverProviderId(targetRow?.dataset.providerRowId ?? null);
  }

  return (
    <section className="providers-page">
      <div className="provider-toolbar">
        <div className="protocol-switch">
          <button
            className={providerKind === "codex" ? "active" : ""}
            onClick={() => onKindChange("codex")}
            type="button"
          >
            Codex
          </button>
          <button
            className={providerKind === "claude" ? "active" : ""}
            onClick={() => onKindChange("claude")}
            type="button"
          >
            Claude
          </button>
        </div>
        <div className="search-box">
          <span />
          <input placeholder="搜索名称或 Base URL" />
        </div>
        <button>全部状态</button>
        <div className="auto-check">
          <span>自动检查</span>
          <button>每 5 分钟</button>
        </div>
        <small>共 {providers.length} 个</small>
      </div>

      <div className="provider-actions">
        <button className="ghost">检查全部</button>
        <button className="primary" onClick={onAdd}>+ 添加{providerKind === "claude" ? " Claude" : ""}供应商</button>
      </div>

      <article className="provider-table">
        <header>
          <span />
          <span>状态</span>
          <span>供应商</span>
          <span>余额</span>
          <span>延迟</span>
          <span>启用</span>
          <span>操作</span>
        </header>
        {providers.map((provider) => {
          const status = providerStatus(provider);
          const balance = providerBalanceMeta(provider);
          const latency = providerLatencyMeta(provider);
          return (
            <div
              className={`provider-line ${provider.status === "enabled" ? "selected" : ""} ${draggingProviderId === provider.id ? "dragging" : ""} ${dragOverProviderId === provider.id ? "drop-target" : ""}`}
              data-provider-row-id={provider.id}
              key={provider.id}
            >
              <button
                aria-label="拖动调整优先级"
                className="drag-handle"
                disabled={busy}
                onPointerCancel={() => {
                  draggingProviderIdRef.current = null;
                  setDraggingProviderId(null);
                  setDragOverProviderId(null);
                }}
                onPointerDown={(event) => handlePointerDown(event, provider.id)}
                onPointerMove={(event) => handlePointerMove(event, provider.id)}
                onPointerUp={(event) => handlePointerUp(event, provider.id)}
                title="拖动调整优先级"
                type="button"
              >
                <span aria-hidden="true" className="drag-grip">
                  <i />
                  <i />
                  <i />
                  <i />
                  <i />
                  <i />
                </span>
              </button>
              <div className={`provider-status ${status.tone}`}>
                <span className={`dot ${status.tone}`} />
                <b>{status.label}</b>
              </div>
              <div className="provider-name-cell">
                <strong>{provider.name}</strong>
                <small>{provider.base_url || "未配置 Base URL"}</small>
              </div>
              <div
                className={`balance-cell provider-fact ${balance.tone}`}
                title={provider.balance_error ?? undefined}
              >
                <strong>{balance.value}</strong>
                <small>{balance.detail}</small>
              </div>
              <div
                className={`latency-cell provider-fact ${latency.tone}`}
                title={provider.latency_error ?? undefined}
              >
                <strong>{latency.value}</strong>
                <small>{latency.detail}</small>
              </div>
              <Toggle checked={provider.status === "enabled"} disabled={busy} onChange={(checked) => onToggle(provider, checked)} />
              <div className="provider-toolbox">
                <button aria-label="设置" className="provider-action-button main-action" disabled={busy} onClick={() => onEdit(provider, "base")} title="设置" type="button">
                  <ToolIcon type="settings" />
                  <span>设置</span>
                </button>
                <button aria-label="测试延迟" className="provider-action-button" disabled={busy} onClick={() => void onTestLatency(provider)} title="测试延迟" type="button">
                  <ToolIcon type="latency" />
                  <span>测速</span>
                </button>
                <button aria-label="刷新余额" className="provider-action-button" disabled={busy} onClick={() => void onRefreshBalance(provider)} title="刷新余额" type="button">
                  <ToolIcon type="balance" />
                  <span>余额</span>
                </button>
              </div>
            </div>
          );
        })}
        <footer>拖动左侧图标可调整优先级；列表越靠上越优先。</footer>
      </article>
    </section>
  );
}

function ProviderEditor(props: {
  allowedModels: string[];
  balanceQuery: BalanceQueryConfig;
  balanceTestStatus: BalanceStatus | null;
  balanceTokenVisible: boolean;
  busy: boolean;
  modelMappings: ModelMapping[];
  onBalanceTokenVisible: (visible: boolean) => void;
  onClose: () => void;
  onDelete: () => void;
  onLoadProviderModels: () => void;
  onSave: () => void;
  onTab: (tab: EditorTab) => void;
  onTestBalance: () => void;
  onUpdateBalance: (patch: Partial<BalanceQueryConfig>) => void;
  providerApiKey: string;
  providerBaseUrl: string;
  providerModels: string[];
  providerName: string;
  providerFastMode: boolean;
  providerWireApi: ProviderWireApi;
  secretVisible: boolean;
  setAllowedModels: (models: string[]) => void;
  setProviderApiKey: (value: string) => void;
  setProviderApiKeyDirty: (dirty: boolean) => void;
  setProviderBaseUrl: (value: string) => void;
  setModelMappings: (mappings: ModelMapping[]) => void;
  setProviderName: (value: string) => void;
  setProviderFastMode: (value: boolean) => void;
  setProviderWireApi: (value: ProviderWireApi) => void;
  setSecretVisible: (value: boolean) => void;
  tab: EditorTab;
}) {
  const {
    allowedModels,
    balanceQuery,
    balanceTestStatus,
    balanceTokenVisible,
    busy,
    modelMappings,
    onBalanceTokenVisible,
    onClose,
    onDelete,
    onLoadProviderModels,
    onSave,
    onTab,
    onTestBalance,
    onUpdateBalance,
    providerApiKey,
    providerBaseUrl,
    providerModels,
    providerName,
    providerFastMode,
    providerWireApi,
    secretVisible,
    setAllowedModels,
    setProviderApiKey,
    setProviderApiKeyDirty,
    setProviderBaseUrl,
    setModelMappings,
    setProviderName,
    setProviderFastMode,
    setProviderWireApi,
    setSecretVisible,
    tab,
  } = props;
  const [customAllowedModel, setCustomAllowedModel] = useState("");
  const [deleteConfirming, setDeleteConfirming] = useState(false);
  const [modelSearch, setModelSearch] = useState("");
  const allowedModelOptions = normalizeModelNames([...providerModels, ...allowedModels]);
  const visibleModelOptions = allowedModelOptions.filter((model) =>
    model.toLowerCase().includes(modelSearch.trim().toLowerCase()),
  );
  const allModelsAllowed = allowedModels.length === 0;
  const upstreamPath = providerWireApi === "chat_completions" ? "chat/completions" : "responses";
  const updateAllowedModels = (models: string[]) => setAllowedModels(normalizeModelNames(models));
  const toggleAllowedModel = (model: string, checked: boolean) => {
    const currentModels = allModelsAllowed ? allowedModelOptions : allowedModels;
    updateAllowedModels(
      checked
        ? [...currentModels, model]
        : currentModels.filter((current) => current.toLowerCase() !== model.toLowerCase()),
    );
  };
  const addCustomAllowedModel = () => {
    const model = customAllowedModel.trim();
    if (!model) return;
    updateAllowedModels([...(allModelsAllowed ? allowedModelOptions : allowedModels), model]);
    setCustomAllowedModel("");
  };

  return (
    <div className="drawer-backdrop">
      <aside className="provider-drawer">
        <header>
          <div>
            <h2>编辑供应商</h2>
            <p>{providerName || "未命名供应商"}</p>
          </div>
          <button className="close" onClick={onClose}>×</button>
        </header>

        <nav className="drawer-tabs">
          <button className={tab === "base" ? "active" : ""} onClick={() => onTab("base")}>基础配置</button>
          <button className={tab === "models" ? "active" : ""} onClick={() => onTab("models")}>模型配置</button>
          <button className={tab === "balance" ? "active" : ""} onClick={() => onTab("balance")}>余额查询</button>
        </nav>

        <section className="drawer-body">
          {tab === "base" && (
            <>
              <label className="field">
                <span>供应商名称</span>
                <input value={providerName} onChange={(event) => setProviderName(event.currentTarget.value)} />
              </label>
              <label className="field">
                <span>Base URL</span>
                <input value={providerBaseUrl} onChange={(event) => setProviderBaseUrl(event.currentTarget.value)} />
                <small>请求将转发至 {providerBaseUrl ? `${providerBaseUrl.replace(/\/$/, "")}/${upstreamPath}` : "-"}</small>
              </label>
              <label className="field">
                <span>转发 API Key</span>
                <div className="secret-field">
                  <input
                    type={secretVisible ? "text" : "password"}
                    value={providerApiKey}
                    onChange={(event) => {
                      setProviderApiKey(event.currentTarget.value);
                      setProviderApiKeyDirty(true);
                    }}
                    placeholder="留空保持已保存的 API Key"
                  />
                  <button onClick={() => setSecretVisible(!secretVisible)} type="button">
                    {secretVisible ? "隐藏" : "显示"}
                  </button>
                </div>
              </label>
              <label className="field">
                <span>接口协议</span>
                <select
                  value={providerWireApi}
                  onChange={(event) => setProviderWireApi(event.currentTarget.value as ProviderWireApi)}
                >
                  <option value="responses">Responses API</option>
                  <option value="chat_completions">Chat Completions 兼容</option>
                </select>
                <small>上游不支持 Responses API 时选择 Chat Completions 兼容模式</small>
              </label>
              <div className="provider-fast-mode-row">
                <strong>强制启用 Fast 模式</strong>
                <Toggle
                  checked={providerFastMode}
                  label="强制启用 Fast 模式"
                  onChange={setProviderFastMode}
                />
              </div>
            </>
          )}

          {tab === "models" && (
            <>
              <section className="model-config-intro">
                <div>
                  <span className="section-kicker">模型来源</span>
                  <h3>配置可路由模型</h3>
                  <p>{providerBaseUrl || "尚未配置 Base URL"}</p>
                </div>
                <button className="primary model-fetch-button" disabled={busy} onClick={onLoadProviderModels} type="button">
                  <ToolIcon type="models" />
                  {busy ? "正在获取" : providerModels.length ? "重新从上游获取" : "从上游获取模型"}
                </button>
              </section>

              <div className="model-config-stats">
                <div>
                  <span>上游已发现</span>
                  <strong>{providerModels.length}</strong>
                  <small>个模型</small>
                </div>
                <div>
                  <span>当前已启用</span>
                  <strong>{allModelsAllowed ? "全部" : allowedModels.length}</strong>
                  <small>{allModelsAllowed ? "兼容任意模型" : "个模型"}</small>
                </div>
              </div>

              <div className="model-filter-box model-manager">
                <div className="mapping-box-head">
                  <div>
                    <strong>启用模型</strong>
                    <p>取消不希望参与路由的模型；修改会在保存供应商后生效。</p>
                  </div>
                  <button
                    className="ghost small"
                    disabled={allowedModelOptions.length === 0}
                    onClick={() => updateAllowedModels(allowedModelOptions)}
                    type="button"
                  >
                    全部启用
                  </button>
                </div>
                <label className="model-search-field">
                  <span aria-hidden="true" />
                  <input
                    placeholder="搜索上游模型"
                    type="search"
                    value={modelSearch}
                    onChange={(event) => setModelSearch(event.currentTarget.value)}
                  />
                </label>
                <div className="model-option-grid">
                  {allowedModelOptions.length === 0 && (
                    <div className="model-empty-state">
                      <strong>还没有上游模型</strong>
                      <p>确认 Base URL 和 API Key 后，从上游读取可用模型。</p>
                      <button className="ghost small" disabled={busy} onClick={onLoadProviderModels} type="button">
                        从上游获取
                      </button>
                    </div>
                  )}
                  {allowedModelOptions.length > 0 && visibleModelOptions.length === 0 && (
                    <p className="mapping-empty">没有匹配“{modelSearch.trim()}”的模型。</p>
                  )}
                  {visibleModelOptions.map((model) => {
                    const checked = allModelsAllowed || allowedModels.some((current) => current.toLowerCase() === model.toLowerCase());
                    return (
                      <label className={checked ? "model-option selected" : "model-option"} key={model}>
                        <input
                          checked={checked}
                          onChange={(event) => toggleAllowedModel(model, event.currentTarget.checked)}
                          type="checkbox"
                        />
                        <span>{model}</span>
                      </label>
                    );
                  })}
                </div>
                <div className="custom-model-row">
                  <input
                    placeholder="手动添加未出现在上游列表中的模型"
                    value={customAllowedModel}
                    onChange={(event) => setCustomAllowedModel(event.currentTarget.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        addCustomAllowedModel();
                      }
                    }}
                  />
                  <button className="ghost small" disabled={!customAllowedModel.trim()} onClick={addCustomAllowedModel} type="button">
                    添加模型
                  </button>
                </div>
              </div>

              <div className="mapping-box">
                <div className="mapping-box-head">
                  <div>
                    <strong>模型映射</strong>
                    <p>客户端模型匹配后，转发为此供应商支持的上游模型。</p>
                  </div>
                  <button
                    className="ghost small"
                    onClick={() => setModelMappings([...modelMappings, { source: "", target: "" }])}
                    type="button"
                  >
                    添加映射
                  </button>
                </div>
                <div className="mapping-grid">
                  <span>客户端模型</span>
                  <span>上游模型</span>
                  <span />
                  {modelMappings.length === 0 && (
                    <p className="mapping-empty">未配置映射时，请求模型将原样转发。</p>
                  )}
                  {modelMappings.map((mapping, index) => (
                    <div className="mapping-row" key={index}>
                      <input
                        placeholder="gpt-5.5"
                        value={mapping.source}
                        onChange={(event) => {
                          const next = [...modelMappings];
                          next[index] = { ...mapping, source: event.currentTarget.value };
                          setModelMappings(next);
                        }}
                      />
                      <input
                        placeholder="deepseek-v4-pro"
                        value={mapping.target}
                        onChange={(event) => {
                          const next = [...modelMappings];
                          next[index] = { ...mapping, target: event.currentTarget.value };
                          setModelMappings(next);
                        }}
                      />
                      <button
                        aria-label="删除映射"
                        className="icon-button"
                        onClick={() => setModelMappings(modelMappings.filter((_, rowIndex) => rowIndex !== index))}
                        title="删除映射"
                        type="button"
                      >
                        ×
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            </>
          )}

          {tab === "balance" && (
            <>
              <div className="switch-row-card">
                <div>
                  <strong>启用余额查询</strong>
                  <p>定时获取上游账户余额并显示在供应商列表中</p>
                </div>
                <Toggle
                  checked={balanceQuery.enabled}
                  onChange={(checked) =>
                    onUpdateBalance({
                      enabled: checked,
                      query_type: checked
                        ? balanceQuery.query_type === "disabled"
                          ? "new_api"
                          : balanceQuery.query_type
                        : "disabled",
                    })
                  }
                />
              </div>
              <label className="field">
                <span>接口类型</span>
                <select
                  value={balanceQuery.query_type}
                  onChange={(event) => onUpdateBalance({ query_type: event.currentTarget.value as BalanceQueryType })}
                >
                  <option value="new_api">New API</option>
                  <option value="sub2_api">Sub2API</option>
                  <option value="ai_gate">AI Gate</option>
                  <option value="disabled">不查询</option>
                </select>
              </label>
              {balanceQuery.query_type === "new_api" && (
                <label className="field">
                  <span>查询目标</span>
                  <select
                    value={balanceQuery.new_api_target}
                    onChange={(event) =>
                      onUpdateBalance({ new_api_target: event.currentTarget.value as NewApiBalanceTarget })
                    }
                  >
                    <option value="token_quota">API Key 额度</option>
                    <option value="account_balance">账户余额</option>
                  </select>
                </label>
              )}
              <label className="field">
                <span>查询地址</span>
                <input
                  value={balanceQuery.endpoint}
                  onChange={(event) => onUpdateBalance({ endpoint: event.currentTarget.value })}
                  placeholder={endpointFromBaseUrl(providerBaseUrl)}
                />
                <small>
                  {balanceQuery.query_type === "ai_gate"
                    ? "默认复用供应商 Base URL 和 Key；Base URL 中的服务前缀会保留，仅移除末尾的 /v1"
                    : "默认使用供应商 Base URL，并自动移除末尾的 /v1"}
                </small>
              </label>
              <label className="field">
                <span>访问令牌</span>
                <div className="secret-field">
                  <input
                    type={balanceTokenVisible ? "text" : "password"}
                    value={balanceQuery.auth_mode === "provider_token" ? savedApiKeyLabel(providerApiKey) : balanceQuery.query_token}
                    onChange={(event) => onUpdateBalance({ query_token: event.currentTarget.value, auth_mode: "separate_token" })}
                    readOnly={balanceQuery.auth_mode === "provider_token"}
                  />
                  <button onClick={() => onBalanceTokenVisible(!balanceTokenVisible)} type="button">
                    {balanceTokenVisible ? "隐藏" : "显示"}
                  </button>
                </div>
              </label>
              {balanceQuery.query_type === "new_api" && balanceQuery.new_api_target === "account_balance" && (
                <label className="field half">
                  <span>用户 ID</span>
                  <input
                    inputMode="numeric"
                    value={balanceQuery.new_api_user_id}
                    onChange={(event) =>
                      onUpdateBalance({ new_api_user_id: event.currentTarget.value.replace(/\D/g, "") })
                    }
                  />
                </label>
              )}
              {balanceQuery.query_type === "new_api" && (
                <div className="quota-box">
                  <strong>余额换算</strong>
                  <span>500000 quota</span>
                  <em>=</em>
                  <b>1 USD</b>
                  <button>编辑比例</button>
                </div>
              )}
              <div className={`balance-test-box ${balanceTestStatus?.error ? "failed" : balanceTestStatus ? "ok" : ""}`}>
                <div>
                  <strong>
                    {balanceTestStatus?.error
                      ? "查询失败"
                      : balanceTestStatus
                        ? "查询成功"
                        : "等待测试"}
                  </strong>
                  <p>
                    {balanceTestStatus?.error
                      ? balanceTestStatus.error
                      : balanceTestStatus
                        ? balanceTestStatus.label
                        : "使用当前表单配置发起一次余额查询，不会保存草稿。"}
                  </p>
                </div>
                <button disabled={busy} onClick={onTestBalance} type="button">
                  {busy ? "查询中" : "测试查询"}
                </button>
              </div>
            </>
          )}

        </section>

        <footer>
          <button
            className="danger"
            disabled={busy}
            onClick={() => deleteConfirming ? onDelete() : setDeleteConfirming(true)}
            type="button"
          >
            {deleteConfirming ? "确认删除" : "删除"}
          </button>
          {deleteConfirming ? <small className="danger-text">会保留历史请求日志；再次点击确认删除。</small> : <div />}
          <button className="ghost" onClick={deleteConfirming ? () => setDeleteConfirming(false) : onClose}>
            {deleteConfirming ? "取消删除" : "取消"}
          </button>
          <button className="primary" disabled={busy} onClick={onSave}>保存修改</button>
        </footer>
      </aside>
    </div>
  );
}

function ClaudeProviderEditor(props: {
  allowedModels: string[];
  busy: boolean;
  modelMappings: ModelMapping[];
  onClose: () => void;
  onDelete: () => void;
  onLoadProviderModels: () => void;
  onSave: () => void;
  providerApiKey: string;
  providerBaseUrl: string;
  providerModels: string[];
  providerName: string;
  secretVisible: boolean;
  setAllowedModels: (models: string[]) => void;
  setModelMappings: (mappings: ModelMapping[]) => void;
  setProviderApiKey: (value: string) => void;
  setProviderApiKeyDirty: (dirty: boolean) => void;
  setProviderBaseUrl: (value: string) => void;
  setProviderName: (value: string) => void;
  setSecretVisible: (value: boolean) => void;
}) {
  const {
    allowedModels,
    busy,
    modelMappings,
    onClose,
    onDelete,
    onLoadProviderModels,
    onSave,
    providerApiKey,
    providerBaseUrl,
    providerModels,
    providerName,
    secretVisible,
    setAllowedModels,
    setModelMappings,
    setProviderApiKey,
    setProviderApiKeyDirty,
    setProviderBaseUrl,
    setProviderName,
    setSecretVisible,
  } = props;
  const [customAllowedModel, setCustomAllowedModel] = useState("");
  const [deleteConfirming, setDeleteConfirming] = useState(false);
  const [modelSearch, setModelSearch] = useState("");
  const updateAllowedModels = (models: string[]) => setAllowedModels(normalizeModelNames(models));
  const allowedModelOptions = normalizeModelNames([...providerModels, ...allowedModels]);
  const visibleModelOptions = allowedModelOptions.filter((model) =>
    model.toLowerCase().includes(modelSearch.trim().toLowerCase()),
  );
  const allModelsAllowed = allowedModels.length === 0;
  const toggleAllowedModel = (model: string, checked: boolean) => {
    const currentModels = allModelsAllowed ? allowedModelOptions : allowedModels;
    updateAllowedModels(
      checked
        ? [...currentModels, model]
        : currentModels.filter((current) => current.toLowerCase() !== model.toLowerCase()),
    );
  };
  const addCustomAllowedModel = () => {
    const model = customAllowedModel.trim();
    if (!model) return;
    updateAllowedModels([...(allModelsAllowed ? allowedModelOptions : allowedModels), model]);
    setCustomAllowedModel("");
  };

  return (
    <div className="drawer-backdrop">
      <aside className="provider-drawer">
        <header>
          <div>
            <h2>编辑 Claude 供应商</h2>
            <p>{providerName || "未命名 Claude 供应商"}</p>
          </div>
          <button className="close" onClick={onClose}>×</button>
        </header>

        <section className="drawer-body">
          <label className="field">
            <span>供应商名称</span>
            <input value={providerName} onChange={(event) => setProviderName(event.currentTarget.value)} />
          </label>
          <label className="field">
            <span>Base URL</span>
            <input
              placeholder="https://api.anthropic.com/v1"
              value={providerBaseUrl}
              onChange={(event) => setProviderBaseUrl(event.currentTarget.value)}
            />
            <small>按填写值原样拼接 /messages 与 /models；需要 /v1 的供应商请写到 Base URL 中。</small>
          </label>
          <label className="field">
            <span>API Key</span>
            <div className="secret-field">
              <input
                type={secretVisible ? "text" : "password"}
                value={providerApiKey}
                onChange={(event) => {
                  setProviderApiKey(event.currentTarget.value);
                  setProviderApiKeyDirty(true);
                }}
                placeholder="留空保持已保存的 API Key"
              />
              <button onClick={() => setSecretVisible(!secretVisible)} type="button">
                {secretVisible ? "隐藏" : "显示"}
              </button>
            </div>
          </label>
          <section className="model-config-intro compact">
            <div>
              <span className="section-kicker">模型来源</span>
              <h3>模型配置</h3>
              <p>{providerBaseUrl || "尚未配置 Base URL"}</p>
            </div>
            <button className="primary model-fetch-button" disabled={busy} onClick={onLoadProviderModels} type="button">
              <ToolIcon type="models" />
              {busy ? "正在获取" : providerModels.length ? "重新获取" : "从上游获取模型"}
            </button>
          </section>
          <div className="model-filter-box model-manager">
            <div className="mapping-box-head">
              <div>
                <strong>启用模型</strong>
                <p>{allowedModels.length ? `已启用 ${allowedModels.length} 个 Claude 模型` : "当前兼容任意 Claude 模型"}</p>
              </div>
              <button
                className="ghost small"
                disabled={allowedModelOptions.length === 0}
                onClick={() => updateAllowedModels(allowedModelOptions)}
                type="button"
              >
                全部启用
              </button>
            </div>
            <label className="model-search-field">
              <span aria-hidden="true" />
              <input
                placeholder="搜索 Claude 模型"
                type="search"
                value={modelSearch}
                onChange={(event) => setModelSearch(event.currentTarget.value)}
              />
            </label>
            <div className="model-option-grid">
              {allowedModelOptions.length === 0 && (
                <div className="model-empty-state">
                  <strong>还没有上游模型</strong>
                  <p>确认 Base URL 和 API Key 后，从上游读取可用模型。</p>
                  <button className="ghost small" disabled={busy} onClick={onLoadProviderModels} type="button">
                    从上游获取
                  </button>
                </div>
              )}
              {allowedModelOptions.length > 0 && visibleModelOptions.length === 0 && (
                <p className="mapping-empty">没有匹配“{modelSearch.trim()}”的模型。</p>
              )}
              {visibleModelOptions.map((model) => {
                const checked = allModelsAllowed || allowedModels.some((current) => current.toLowerCase() === model.toLowerCase());
                return (
                  <label className={checked ? "model-option selected" : "model-option"} key={model}>
                    <input
                      checked={checked}
                      onChange={(event) => toggleAllowedModel(model, event.currentTarget.checked)}
                      type="checkbox"
                    />
                    <span>{model}</span>
                  </label>
                );
              })}
            </div>
            <div className="custom-model-row">
              <input
                placeholder="手动输入模型，例如 claude-sonnet-4-5"
                value={customAllowedModel}
                onChange={(event) => setCustomAllowedModel(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    addCustomAllowedModel();
                  }
                }}
              />
              <button className="ghost small" disabled={!customAllowedModel.trim()} onClick={addCustomAllowedModel} type="button">
                添加模型
              </button>
            </div>
          </div>
          <div className="mapping-box">
            <div className="mapping-box-head">
              <div>
                <strong>模型映射</strong>
                <p>只替换 Anthropic 请求体顶层 model 字段</p>
              </div>
              <button
                className="ghost small"
                onClick={() => setModelMappings([...modelMappings, { source: "", target: "" }])}
                type="button"
              >
                添加映射
              </button>
            </div>
            <div className="mapping-grid">
              <span>客户端模型</span>
              <span>上游模型</span>
              <span />
              {modelMappings.length === 0 && (
                <p className="mapping-empty">未配置映射时，请求模型将原样转发。</p>
              )}
              {modelMappings.map((mapping, index) => (
                <div className="mapping-row" key={index}>
                  <input
                    placeholder="claude-sonnet-4-5"
                    value={mapping.source}
                    onChange={(event) => {
                      const next = [...modelMappings];
                      next[index] = { ...mapping, source: event.currentTarget.value };
                      setModelMappings(next);
                    }}
                  />
                  <input
                    placeholder="claude-3-5-sonnet-latest"
                    value={mapping.target}
                    onChange={(event) => {
                      const next = [...modelMappings];
                      next[index] = { ...mapping, target: event.currentTarget.value };
                      setModelMappings(next);
                    }}
                  />
                  <button
                    aria-label="删除映射"
                    className="icon-button"
                    onClick={() => setModelMappings(modelMappings.filter((_, rowIndex) => rowIndex !== index))}
                    title="删除映射"
                    type="button"
                  >
                    ×
                  </button>
                </div>
              ))}
            </div>
          </div>
        </section>

        <footer>
          <button
            className="danger"
            disabled={busy}
            onClick={() => deleteConfirming ? onDelete() : setDeleteConfirming(true)}
            type="button"
          >
            {deleteConfirming ? "确认删除" : "删除"}
          </button>
          {deleteConfirming ? <small className="danger-text">会保留历史请求日志；再次点击确认删除。</small> : <div />}
          <button className="ghost" onClick={deleteConfirming ? () => setDeleteConfirming(false) : onClose}>
            {deleteConfirming ? "取消删除" : "取消"}
          </button>
          <button className="primary" disabled={busy} onClick={onSave}>保存修改</button>
        </footer>
      </aside>
    </div>
  );
}

export default App;
