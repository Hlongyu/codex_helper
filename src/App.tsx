import { useEffect, useId, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

type BalanceQueryType = "disabled" | "new_api" | "sub2_api";
type NewApiBalanceTarget = "token_quota" | "account_balance";
type BalanceAuthMode = "provider_token" | "separate_token";

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

type ProviderConnectionTestStep = {
  key: string;
  label: string;
  status: "ok" | "warn" | "failed" | string;
  latency_ms?: number | null;
  message: string;
};

type ProviderConnectionTestResult = {
  ok: boolean;
  steps: ProviderConnectionTestStep[];
};

type RouterConfig = {
  enabled: boolean;
  host: string;
  port: number;
  local_token: string;
};

type RouterStatus = {
  running: boolean;
  address: string;
  error?: string | null;
};

type ProviderConfig = {
  id: string;
  name: string;
  enabled: boolean;
  config: JsonValue;
  balance_query: BalanceQueryConfig;
  balance_status?: BalanceStatus | null;
};

type ProviderSummary = {
  id: string;
  name: string;
  enabled: boolean;
  pending_changes: number;
  base_url: string;
  provider_type: string;
  route_order: number;
  balance_label: string;
  balance_error?: string | null;
  latency_label: string;
  last_checked_label: string;
};

type UsageSummary = {
  request_count: number;
  input_tokens: number;
  uncached_input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  reasoning_output_tokens: number;
  total_tokens: number;
  estimated_cost: number;
  currency: string;
};

type RouteRequestLog = {
  id: string;
  started_at_ms: number;
  day: string;
  hour: string;
  method: string;
  path: string;
  model: string;
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
  estimated_cost: number;
  currency: string;
  cost_breakdown: string;
  pricing_model_match: string;
  pricing_source: string;
  first_byte_ms?: number | null;
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
  estimated_cost: number;
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

type AppState = {
  codex_config_path: string;
  manager_dir: string;
  current_config_raw: string;
  current_config_exists: boolean;
  active_provider_id: string;
  providers: ProviderSummary[];
  active_provider: ProviderConfig | null;
  active_provider_toml: string;
  final_preview_toml: string;
  diffs: Array<{ path: string; action: string }>;
  router: RouterConfig;
  router_status: RouterStatus;
};

type Screen = "dashboard" | "route" | "providers" | "usage" | "requests" | "settings";
type EditorTab = "base" | "balance" | "route";
type TimeRange = "today" | "week" | "month" | "all";
type TrendMetric = "cost" | "tokens" | "requests";

const isTauriRuntime = "__TAURI_INTERNALS__" in window;

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
    host: "127.0.0.1",
    port: 18080,
    local_token: "",
  };
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
  }
  return next;
}

function formatMoney(value: number, currency = "USD") {
  const prefix = currency.toUpperCase() === "USD" ? "$" : `${currency} `;
  return `${prefix}${(value || 0).toFixed(6)}`;
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
  if (metric === "cost") return "消费";
  if (metric === "tokens") return "总 Token";
  return "成功请求";
}

function bucketGranularityLabel(value?: string) {
  if (value === "month") return "按月聚合";
  if (value === "day") return "按日聚合";
  return "按小时聚合";
}

function trendBucketValue(bucket: Pick<RouteUsageBucket, "estimated_cost" | "total_tokens" | "request_count">, metric: TrendMetric) {
  if (metric === "cost") return bucket.estimated_cost;
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
    estimated_cost: 0,
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

function niceTrendMax(value: number, metric: TrendMetric) {
  if (value <= 0) return metric === "cost" ? 0.000001 : 1;
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
  if (metric === "cost") return formatMoney(value);
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

function formatMs(value?: number | null) {
  if (value == null) return "-";
  if (value >= 1000) return `${(value / 1000).toFixed(value >= 10_000 ? 1 : 1)} s`;
  return `${Math.round(value)} ms`;
}

function formatDuration(value: number) {
  if (value >= 1000) return `${(value / 1000).toFixed(1)} s`;
  return `${Math.round(value)} ms`;
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
  if (result.includes("切换")) return "amber";
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
    "Token",
    "首字延迟",
    "总耗时",
    "消费",
    "路由结果",
    "错误",
  ];
  const rows = logs.map((log) => [
    new Date(log.started_at_ms).toLocaleString("zh-CN"),
    statusMeta(log.status).label,
    log.model,
    log.provider_name,
    log.total_tokens,
    log.first_byte_ms ?? "",
    log.total_ms,
    log.estimated_cost.toFixed(6),
    log.route_result,
    log.error ?? "",
  ]);
  const csv = [headers, ...rows].map((row) => row.map(csvEscape).join(",")).join("\n");
  const url = URL.createObjectURL(new Blob([`\uFEFF${csv}`], { type: "text/csv;charset=utf-8" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = `codex-helper-route-logs-${Date.now()}.csv`;
  link.click();
  URL.revokeObjectURL(url);
}

function formatBalanceForCard(label: string) {
  if (!label || label === "未配置") return "未配置";
  return label.replace(/^账户余额\s*/, "").replace(/^Key额度\s*/, "").replace(/^余额\s*/, "");
}

function providerStatus(provider: ProviderSummary) {
  if (!provider.enabled) return { label: "不可用", tone: "danger" };
  if (provider.balance_error) return { label: "高延迟", tone: "warn" };
  return { label: "正常", tone: "ok" };
}

function routeBaseUrl(router: RouterConfig | RouterStatus) {
  if ("address" in router) return `http://${router.address}/v1`;
  return `http://${router.host || "127.0.0.1"}:${router.port || 18080}/v1`;
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
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <button
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

function App() {
  const [appState, setAppState] = useState<AppState | null>(null);
  const [routeUsageStats, setRouteUsageStats] = useState<RouteUsageStats | null>(null);
  const [routeLogs, setRouteLogs] = useState<RouteLogsResponse | null>(null);
  const [usageRange, setUsageRange] = useState<TimeRange>("today");
  const [usageFilter, setUsageFilter] = useState<RouteLogFilter>(() => ({
    model: "",
    page_size: 20,
    ...filterForRange("today"),
  }));
  const [trendMetric, setTrendMetric] = useState<TrendMetric>("cost");
  const [requestFilter, setRequestFilter] = useState<RouteLogFilter>({ model: "", page_size: 20 });
  const [requestAutoRefresh, setRequestAutoRefresh] = useState(true);
  const [screen, setScreen] = useState<Screen>("dashboard");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [editorOpen, setEditorOpen] = useState(false);
  const [editorTab, setEditorTab] = useState<EditorTab>("base");
  const [editingId, setEditingId] = useState("");
  const [providerName, setProviderName] = useState("");
  const [providerBaseUrl, setProviderBaseUrl] = useState("");
  const [providerApiKey, setProviderApiKey] = useState("");
  const [providerApiKeyDirty, setProviderApiKeyDirty] = useState(false);
  const [providerEnabled, setProviderEnabled] = useState(true);
  const [connectionTestResult, setConnectionTestResult] = useState<ProviderConnectionTestResult | null>(null);
  const [balanceQuery, setBalanceQuery] = useState<BalanceQueryConfig>(() =>
    defaultBalanceQuery(),
  );
  const [balanceTestStatus, setBalanceTestStatus] = useState<BalanceStatus | null>(null);
  const [routerDraft, setRouterDraft] = useState<RouterConfig>(() => defaultRouterConfig());
  const [secretVisible, setSecretVisible] = useState(false);
  const [balanceTokenVisible, setBalanceTokenVisible] = useState(false);
  const [newProviderCount, setNewProviderCount] = useState(1);
  const didInitialRefresh = useRef(false);

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

  async function refresh() {
    await refreshAppState();
    try {
      await Promise.all([refreshRouteUsage(), refreshRouteLogs()]);
    } catch {
      setRouteUsageStats(null);
      setRouteLogs(null);
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

  const activeProvider = useMemo(() => {
    if (!appState) return null;
    return (
      appState.providers.find((provider) => provider.id === appState.active_provider_id) ??
      appState.providers[0] ??
      null
    );
  }, [appState]);

  const requestCount = routeUsageStats?.success_count ?? routeUsageStats?.summary.request_count ?? 0;
  const officialCost = routeUsageStats?.summary.estimated_cost ?? 0;
  const uncachedInput = routeUsageStats?.summary.uncached_input_tokens ?? 0;
  const cachedInput = routeUsageStats?.summary.cached_input_tokens ?? 0;
  const outputTokens = routeUsageStats?.summary.output_tokens ?? 0;
  const totalTokens = routeUsageStats?.summary.total_tokens ?? 0;
  const modelRows = (routeUsageStats?.models ?? [])
    .map((row) => ({
      model: row.label,
      requests: row.request_count,
      tokens: row.total_tokens,
      cost: row.estimated_cost,
    }))
    .slice(0, 3);
  const failedCount = routeUsageStats?.failed_count ?? 0;
  const totalFinishedCount = requestCount + failedCount;
  const successRate = totalFinishedCount ? (requestCount / totalFinishedCount) * 100 : 0;

  function fillProviderEditor(targetFull: ProviderConfig, summary: ProviderSummary | null, tab: EditorTab) {
    const fields = providerFields(targetFull);
    setEditingId(summary?.id ?? targetFull.id);
    setProviderName(summary?.name ?? targetFull.name ?? "");
    setProviderBaseUrl(summary?.base_url || fields.baseUrl);
    setProviderApiKey(fields.apiKey);
    setProviderApiKeyDirty(false);
    setProviderEnabled(summary?.enabled ?? targetFull.enabled ?? true);
    setBalanceQuery(
      normalizeBalanceQuery(
        targetFull.balance_query,
        endpointFromBaseUrl(summary?.base_url || fields.baseUrl),
      ),
    );
    setEditorTab(tab);
    setSecretVisible(false);
    setBalanceTokenVisible(false);
    setConnectionTestResult(null);
    setBalanceTestStatus(targetFull.balance_status ?? null);
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
        enabled: providerEnabled,
        balance_query: nextBalance,
        balance_status: balanceTestStatus,
      };
      if (providerApiKeyDirty) {
        payload.api_key = providerApiKey;
      }
      const state = await callCommand<AppState>("save_provider", {
        payload,
      });
      setAppState(state);
      setEditorOpen(false);
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

  async function testConnection() {
    if (!editingId) return;
    await run(async () => {
      const payload: Record<string, unknown> = {
        provider_id: editingId,
        base_url: providerBaseUrl,
      };
      if (providerApiKeyDirty) {
        payload.api_key = providerApiKey;
      }
      const result = await callCommand<ProviderConnectionTestResult>("test_provider_connection", {
        payload,
      });
      setConnectionTestResult(result);
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

  async function toggleRouter(enabled: boolean) {
    const next = { ...(appState?.router ?? routerDraft), enabled };
    await saveRouter(next, true);
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
      }
      return next;
    });
  }

  if (!appState) {
    return (
      <main className="loading-screen">
        <div className="brand-logo">
          <span />
        </div>
        <strong>Codex Helper</strong>
        <p>{error || "正在加载本地网关状态"}</p>
        {error && <button onClick={() => refresh()}>重试</button>}
      </main>
    );
  }

  const routerOn = serviceOk(appState);

  return (
    <main className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-logo">
            <span />
          </div>
          <div>
            <strong>Codex Helper</strong>
            <small>LOCAL GATEWAY</small>
          </div>
        </div>

        <nav className="nav">
          {[
            ["dashboard", "总览"],
            ["route", "路由"],
            ["providers", "供应商"],
            ["usage", "使用统计"],
            ["requests", "请求记录"],
            ["settings", "设置"],
          ].map(([key, label]) => (
            <button
              className={screen === key ? "active" : ""}
              key={key}
              onClick={() => setScreen(key as Screen)}
              type="button"
            >
              <NavIcon type={key as Screen} />
              {label}
            </button>
          ))}
        </nav>
      </aside>

      <section className="shell">
        <header className="topbar">
          <div>
            <h1>
              {screen === "dashboard"
                ? "总览"
                : screen === "route"
                  ? "路由"
                : screen === "providers"
                  ? "供应商"
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
                  ? "管理 Codex 接管、本地代理与故障转移规则"
                : screen === "providers"
                  ? "管理上游连接、余额监控与路由顺序"
                : screen === "usage"
                    ? "分析本地路由后的 Token、费用与供应商使用情况"
                    : screen === "requests"
                      ? "经 Codex Helper 转发的请求"
                      : "本地网关运行参数"}
            </p>
          </div>
          <div className="top-actions">
            <StatusPill ok={routerOn} />
            <span className="muted">接管 Codex</span>
            <Toggle checked={appState.router.enabled} disabled={busy} onChange={toggleRouter} />
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
            officialCost={officialCost}
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
            routerDraft={routerDraft}
            routerOn={routerOn}
            setRouterDraft={setRouterDraft}
            onSaveRouter={saveRouter}
          />
        )}

        {screen === "providers" && (
          <ProvidersScreen
            busy={busy}
            onAdd={addProvider}
            onEdit={(provider, tab) => {
              void openProviderEditor(provider, tab);
            }}
            onToggle={toggleProvider}
            providers={appState.providers}
          />
        )}

        {screen === "usage" && (
          <UsageScreen
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
                  <p>通用偏好设置入口。路由接管与本地代理配置已移动到路由页。</p>
                </div>
              </div>
              <div className="settings-placeholder">
                <span>当前暂无额外设置</span>
                <button className="ghost" onClick={() => setScreen("route")} type="button">
                  打开路由配置
                </button>
              </div>
            </article>
          </section>
        )}
      </section>

      {editorOpen && (
        <ProviderEditor
          balanceQuery={balanceQuery}
          balanceTestStatus={balanceTestStatus}
          balanceTokenVisible={balanceTokenVisible}
          busy={busy}
          connectionTestResult={connectionTestResult}
          onBalanceTokenVisible={setBalanceTokenVisible}
          onClose={() => setEditorOpen(false)}
          onSave={saveProvider}
          onTab={setEditorTab}
          onTestBalance={testBalance}
          onTestConnection={testConnection}
          onUpdateBalance={updateBalanceQuery}
          providerApiKey={providerApiKey}
          providerBaseUrl={providerBaseUrl}
          providerEnabled={providerEnabled}
          providerName={providerName}
          secretVisible={secretVisible}
          setProviderApiKey={(value) => {
            setProviderApiKey(value);
            setBalanceTestStatus(null);
            setConnectionTestResult(null);
          }}
          setProviderApiKeyDirty={setProviderApiKeyDirty}
          setProviderBaseUrl={(value) => {
            setProviderBaseUrl(value);
            setBalanceTestStatus(null);
            setConnectionTestResult(null);
          }}
          setProviderEnabled={setProviderEnabled}
          setProviderName={setProviderName}
          setSecretVisible={setSecretVisible}
          tab={editorTab}
        />
      )}
    </main>
  );
}

function RouteScreen({
  appState,
  busy,
  onSaveRouter,
  routerDraft,
  routerOn,
  setRouterDraft,
}: {
  appState: AppState;
  busy: boolean;
  onSaveRouter: (nextRouter: RouterConfig, apply?: boolean) => Promise<void>;
  routerDraft: RouterConfig;
  routerOn: boolean;
  setRouterDraft: (router: RouterConfig) => void;
}) {
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
            <span className={`state-pill ${routerOn ? "ok" : "warn"}`}>
              <span />
              {routerOn ? "已接管" : "未接管"}
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
          <div className="route-diff-row">
            <span>openai_base_url → 本地代理地址</span>
            <button className="ghost small">查看变更</button>
            <button className="ghost small warn-text">恢复原配置</button>
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
        <button className="primary" disabled={busy} onClick={() => onSaveRouter(routerDraft, true)} type="button">
          保存修改
        </button>
      </div>
    </section>
  );
}

function UsageScreen({
  filter,
  onFilter,
  onRangeChange,
  onRefresh,
  onTrendMetricChange,
  stats,
  timeRange,
  trendMetric,
}: {
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
    estimated_cost: 0,
    currency: "USD",
  };
  const trendBuckets = stats?.buckets.length
    ? stats.buckets
    : [emptyTrendBucket("00:00")];
  const totalCost = summary.estimated_cost || 1;

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
          {(stats?.available_providers ?? []).map((provider) => (
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
        <Metric title="消费" value={formatMoney(summary.estimated_cost, summary.currency)} tone="purple" />
        <Metric title="非缓存输入" value={formatCompact(summary.uncached_input_tokens)} tone="cyan" sub={`${formatCompact(summary.input_tokens)} 输入`} />
        <Metric title="缓存输入" value={formatCompact(summary.cached_input_tokens)} tone="blue" sub="路由后统计" />
        <Metric title="输出 Token" value={formatCompact(summary.output_tokens)} tone="amber" sub={rangeLabel(timeRange)} />
        <Metric title="请求数" value={String(stats?.success_count ?? summary.request_count)} tone="green" />
      </div>

      <div className="usage-main-grid">
        <article className="card route-trend-card">
          <div className="card-head">
            <div>
              <h3>使用趋势</h3>
              <p>{rangeLabel(timeRange)} {bucketGranularityLabel(stats?.bucket_granularity)}的{trendMetricLabel(trendMetric)}</p>
            </div>
            <div className="mini-tabs">
              {(["cost", "tokens", "requests"] as TrendMetric[]).map((metric) => (
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

        <article className="card cost-distribution">
          <div className="card-head">
            <div>
              <h3>费用分布</h3>
              <p>按模型与供应商</p>
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
                <b>{formatMoney(row.estimated_cost)}</b>
              </div>
              <span><i style={{ width: `${Math.max(3, (row.estimated_cost / totalCost) * 100)}%` }} /></span>
              <small>{Math.round((row.estimated_cost / totalCost) * 100)}%</small>
            </div>
          ))}
        </article>
      </div>

      <article className="route-table-card usage-detail-card">
        <div className="panel-head">
          <div>
            <h2>用量明细</h2>
            <p>展示经过 Codex Helper 转发并成功记录用量的请求。</p>
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
            <span>消费</span>
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
              <b>{log.total_tokens ? formatMoney(log.estimated_cost, log.currency) : "-"}</b>
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
            <span>消费</span>
            <span>路由结果</span>
          </header>
          {rows.map((log) => {
            const status = statusMeta(log.status);
            return (
              <div className={log.route_attempts > 1 ? "selected" : ""} key={log.id}>
                <div className="status-cell">
                  <span className={`dot ${status.tone}`} />
                  <b className={`${status.tone}-text`}>{status.label}</b>
                </div>
                <div>
                  <strong>{formatLogTime(log.started_at_ms)}</strong>
                  <small>今天</small>
                </div>
                <strong>{log.model}</strong>
                <div>
                  <strong>{log.provider_name}</strong>
                  <small>{log.error ?? log.upstream_chain.join(" → ")}</small>
                </div>
                <span>{formatTokenTriplet(log)}</span>
                <b>{formatMs(log.first_byte_ms)}</b>
                <b>{formatDuration(log.total_ms)}</b>
                <b>{log.total_tokens ? formatMoney(log.estimated_cost, log.currency) : "-"}</b>
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
    </section>
  );
}

function Dashboard(props: {
  activeProvider: ProviderSummary | null;
  cachedInput: number;
  modelRows: Array<{ model: string; requests: number; tokens: number; cost: number }>;
  onRangeChange: (range: TimeRange) => Promise<void>;
  onTrendMetricChange: (metric: TrendMetric) => void;
  officialCost: number;
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
    officialCost,
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
  const healthyProviders = providers.filter((provider) => provider.enabled && !provider.balance_error).length;
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
        <Metric title="消费" value={formatMoney(officialCost)} tone="purple" />
        <Metric title="总 Token" value={formatCompact(totalTokens)} tone="cyan" tooltip={tokenTooltip} />
        <Metric title="请求数" value={String(Math.max(successCount, 0))} tone="green" />
      </div>

      <div className="dashboard-grid">
        <article className="card trend-card">
          <div className="card-head">
            <div>
              <h3>使用趋势</h3>
              <p>{rangeLabel(timeRange)} {bucketGranularityLabel(stats?.bucket_granularity)}的{trendMetricLabel(trendMetric)}</p>
            </div>
            <div className="mini-tabs">
              {(["cost", "tokens", "requests"] as TrendMetric[]).map((metric) => (
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
              return (
                <div className="provider-health" key={provider.id}>
                  <span className={`dot ${status.tone}`} />
                  <div>
                    <strong>{provider.name}</strong>
                    <small>{provider.provider_type}</small>
                  </div>
                  <b>{formatBalanceForCard(provider.balance_label)}</b>
                  <small className={status.tone}>{provider.latency_label}</small>
                </div>
              );
            })}
          </div>
          <div className="health-bar">
            <span style={{ width: `${providers.length ? (healthyProviders / providers.length) * 100 : 0}%` }} />
          </div>
          <footer>
            <span>健康供应商 {healthyProviders}/{providers.length}</span>
            <b>{providers.length ? Math.round((healthyProviders / providers.length) * 100) : 0}%</b>
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
              <span>估算</span>
            </header>
            {(modelRows.length ? modelRows : [{ model: "gpt-5.5", requests: 0, tokens: 0, cost: 0 }]).map((row, index) => (
              <div className="model-row" key={row.model}>
                <strong>{row.model}</strong>
                <span>{row.requests}</span>
                <span>{formatCompact(row.tokens)}</span>
                <b>{formatMoney(row.cost)}</b>
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
  const yMax = niceTrendMax(rawMax, metric);
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
            <stop offset="0%" stopColor="#6974ff" stopOpacity="0.22" />
            <stop offset="100%" stopColor="#6974ff" stopOpacity="0.02" />
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
          <span>消费 {formatMoney(hoveredPoint.bucket.estimated_cost)}</span>
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

function ProvidersScreen({
  busy,
  onAdd,
  onEdit,
  onToggle,
  providers,
}: {
  busy: boolean;
  onAdd: () => void;
  onEdit: (provider: ProviderSummary, tab: EditorTab) => void;
  onToggle: (provider: ProviderSummary, enabled: boolean) => void;
  providers: ProviderSummary[];
}) {
  return (
    <section className="providers-page">
      <div className="provider-toolbar">
        <div className="search-box">
          <span />
          <input placeholder="搜索名称或 Base URL" />
        </div>
        <button>全部状态</button>
        <button>全部类型</button>
        <button>按路由顺序排序</button>
        <div className="auto-check">
          <span>自动检查</span>
          <button>每 5 分钟</button>
        </div>
        <small>共 {providers.length} 个</small>
      </div>

      <div className="provider-actions">
        <button className="ghost">检查全部</button>
        <button className="primary" onClick={onAdd}>+ 添加供应商</button>
      </div>

      <article className="provider-table">
        <header>
          <span>状态</span>
          <span>供应商</span>
          <span>类型</span>
          <span>路由顺序</span>
          <span>余额</span>
          <span>延迟</span>
          <span>最近检查</span>
          <span>启用</span>
          <span>操作</span>
        </header>
        {providers.map((provider) => {
          const status = providerStatus(provider);
          return (
            <div
              className={`provider-line ${provider.enabled ? "selected" : ""}`}
              key={provider.id}
            >
              <div className="provider-status">
                <span className={`dot ${status.tone}`} />
                <b>{status.label}</b>
              </div>
              <div className="provider-name-cell">
                <strong>{provider.name}</strong>
                <small>{provider.base_url || "未配置 Base URL"}</small>
              </div>
              <span className="type-pill">{provider.provider_type}</span>
              <span className="order-pill">{provider.route_order}</span>
              <div className="balance-cell">
                <strong>{formatBalanceForCard(provider.balance_label)}</strong>
                <small className={provider.balance_error ? "warn" : "ok"}>
                  {provider.balance_error ? "查询异常" : "余额正常"}
                </small>
              </div>
              <b className={provider.balance_error ? "danger-text" : "ok-text"}>
                {provider.latency_label}
              </b>
              <span>{provider.last_checked_label}</span>
              <Toggle checked={provider.enabled} disabled={busy} onChange={(checked) => onToggle(provider, checked)} />
              <button className="ghost small" onClick={() => onEdit(provider, "base")}>编辑</button>
            </div>
          );
        })}
        <footer>拖动左侧图标可调整故障转移顺序；列表越靠上，路由优先级越高。</footer>
      </article>
    </section>
  );
}

function ProviderEditor(props: {
  balanceQuery: BalanceQueryConfig;
  balanceTestStatus: BalanceStatus | null;
  balanceTokenVisible: boolean;
  busy: boolean;
  connectionTestResult: ProviderConnectionTestResult | null;
  onBalanceTokenVisible: (visible: boolean) => void;
  onClose: () => void;
  onSave: () => void;
  onTab: (tab: EditorTab) => void;
  onTestBalance: () => void;
  onTestConnection: () => void;
  onUpdateBalance: (patch: Partial<BalanceQueryConfig>) => void;
  providerApiKey: string;
  providerBaseUrl: string;
  providerEnabled: boolean;
  providerName: string;
  secretVisible: boolean;
  setProviderApiKey: (value: string) => void;
  setProviderApiKeyDirty: (dirty: boolean) => void;
  setProviderBaseUrl: (value: string) => void;
  setProviderEnabled: (value: boolean) => void;
  setProviderName: (value: string) => void;
  setSecretVisible: (value: boolean) => void;
  tab: EditorTab;
}) {
  const {
    balanceQuery,
    balanceTestStatus,
    balanceTokenVisible,
    busy,
    connectionTestResult,
    onBalanceTokenVisible,
    onClose,
    onSave,
    onTab,
    onTestBalance,
    onTestConnection,
    onUpdateBalance,
    providerApiKey,
    providerBaseUrl,
    providerEnabled,
    providerName,
    secretVisible,
    setProviderApiKey,
    setProviderApiKeyDirty,
    setProviderBaseUrl,
    setProviderEnabled,
    setProviderName,
    setSecretVisible,
    tab,
  } = props;

  return (
    <div className="drawer-backdrop">
      <aside className="provider-drawer">
        <header>
          <div>
            <h2>编辑供应商</h2>
            <p>
              {providerName || "未命名供应商"}
              <span className="dot green" /> 连接正常
            </p>
          </div>
          <button className="close" onClick={onClose}>×</button>
        </header>

        <nav className="drawer-tabs">
          <button className={tab === "base" ? "active" : ""} onClick={() => onTab("base")}>基础配置</button>
          <button className={tab === "balance" ? "active" : ""} onClick={() => onTab("balance")}>余额查询</button>
          <button className={tab === "route" ? "active" : ""} onClick={() => onTab("route")}>路由设置</button>
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
                <small>请求将转发至 {providerBaseUrl ? `${providerBaseUrl.replace(/\/$/, "")}/responses` : "-"}</small>
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
              <div className="switch-row-card">
                <div>
                  <strong>启用供应商</strong>
                  <p>关闭后将停止接收新的转发请求</p>
                </div>
                <Toggle checked={providerEnabled} onChange={setProviderEnabled} />
              </div>
              <div className="test-box">
                <div>
                  <strong>连接测试</strong>
                  <p>保存前验证上游接口是否可用</p>
                </div>
                <button className="ghost" disabled={busy} onClick={onTestConnection} type="button">
                  {busy ? "测试中" : "重新测试"}
                </button>
                <ul>
                  {(connectionTestResult?.steps.length
                    ? connectionTestResult.steps
                    : [
                        {
                          key: "pending",
                          label: "等待测试",
                          status: "warn",
                          latency_ms: null,
                          message: "使用当前 Base URL 与 API Key 发起测试",
                        },
                      ]).map((step) => (
                    <li key={step.key}>
                      <span className={`dot ${step.status === "failed" ? "danger" : step.status === "warn" ? "amber" : "green"}`} />
                      <span>{step.label}</span>
                      <b className={step.status === "failed" ? "danger-text" : step.status === "warn" ? "warn-text" : "ok-text"}>
                        {step.status === "failed" ? "失败" : step.status === "warn" ? "注意" : "正常"}
                      </b>
                      <em>{step.latency_ms == null ? "-" : `${step.latency_ms} ms`}</em>
                      <small>{step.message}</small>
                    </li>
                  ))}
                </ul>
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
                <small>默认使用供应商 Base URL，并自动移除末尾的 /v1</small>
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
              <div className="quota-box">
                <strong>余额换算</strong>
                <span>500000 quota</span>
                <em>=</em>
                <b>1 USD</b>
                <button>编辑比例</button>
              </div>
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

          {tab === "route" && (
            <>
              <div className="switch-row-card">
                <div>
                  <strong>参与自动路由</strong>
                  <p>关闭后仅保留配置，不会被自动选择</p>
                </div>
                <Toggle checked={providerEnabled} onChange={setProviderEnabled} />
              </div>
              <div className="form-row">
                <label className="field">
                  <span>路由顺序</span>
                  <input readOnly value="1" />
                  <small>数字越小，优先级越高</small>
                </label>
                <label className="field">
                  <span>会话固定</span>
                  <select defaultValue="global">
                    <option value="global">跟随全局</option>
                  </select>
                </label>
              </div>
              <label className="field">
                <span>允许模型</span>
                <select defaultValue="all">
                  <option value="all">全部模型</option>
                </select>
              </label>
              <div className="route-box">
                <strong>故障处理</strong>
                <div className="form-row">
                  <label className="field">
                    <span>连续失败</span>
                    <input readOnly value="3 次" />
                  </label>
                  <label className="field">
                    <span>冷却时间</span>
                    <input readOnly value="60 秒" />
                  </label>
                </div>
                <div className="mini-toggle-line">
                  <span>连接异常时切换至下一供应商</span>
                  <Toggle checked onChange={() => undefined} />
                </div>
                <div className="mini-toggle-line">
                  <span>余额低于阈值时跳过此供应商（即将支持）</span>
                  <Toggle checked={false} disabled onChange={() => undefined} />
                </div>
              </div>
              <div className="form-row">
                <label className="field">
                  <span>低余额阈值</span>
                  <input defaultValue="$2.00" />
                </label>
                <label className="field">
                  <span>模型映射</span>
                  <select defaultValue="off">
                    <option value="off">未启用</option>
                  </select>
                </label>
              </div>
            </>
          )}
        </section>

        <footer>
          <button className="danger">删除</button>
          <div />
          <button className="ghost" onClick={onClose}>取消</button>
          <button className="primary" disabled={busy} onClick={onSave}>保存修改</button>
        </footer>
      </aside>
    </div>
  );
}

export default App;
