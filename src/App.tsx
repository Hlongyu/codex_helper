import { type PointerEvent, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Tabs } from "@radix-ui/themes";
import { listen } from "@tauri-apps/api/event";
import { TrayPreferenceSetting } from "./components/TrayPreferenceSetting";
import { mergeTrayDraft } from "./trayState";
import {
  chartModelLabel,
  fusionModelLabel,
  requestModelLabel,
} from "./modelLabels";
import { UsageName } from "./components/UsageName";
import {
  Activity,
  ArrowUpRight,
  Blocks,
  ChartNoAxesCombined,
  CircleCheck,
  Clock3,
  Code2,
  FileText,
  LayoutDashboard,
  Network,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  Search,
  Server,
  Settings2,
  Wallet,
  Zap,
} from "lucide-react";
import {
  Button,
  Badge,
  Card,
  Checkbox,
  ConfirmDialog,
  Select,
  SelectOption,
  TextArea,
  TextField,
  Tooltip,
  CopyButton,
  DialogSurface,
  EmptyState,
  SegmentedControl,
  Toggle,
} from "./components/ui";
import { AppearanceControl } from "./components/Appearance";
import { UsageTrendChart } from "./components/UsageTrendChart";
import type {
  JsonValue,
  BalanceQueryType,
  NewApiBalanceTarget,
  ProviderWireApi,
  ProviderServiceTier,
  AutoDisableResetPeriod,
  SlowMode,
  BalanceQueryConfig,
  AiGateBalanceAccount,
  BalanceStatus,
  ProviderModelsResponse,
  ModelMapping,
  RouterConfig,
  SkillLocationConfig,
  ClientConfigs,
  SkillClientView,
  ClientSkillView,
  SharedSkillView,
  SkillManagementView,
  RouterStatus,
  ProviderConfig,
  ClaudeProviderConfig,
  ProviderSummary,
  ProviderLatencyDialogState,
  ProviderLatencyTestResponse,
  RouteRequestLog,
  RouteDebugCapture,
  RouteLogFilter,
  RouteLogFilterOption,
  RouteLogsResponse,
  RouteUsageBucket,
  RouteUsageStats,
  UpdateCheckInfo,
  UpdateInstallResult,
  AppState,
  Screen,
  EditorTab,
  AgentClientKind,
  ProviderKind,
  TimeRange,
  TrendMetric,
} from "./types";
import brandMark from "../output/icons/codex-helper-mark-transparent.png";
import "./App.css";
import { ProviderImport } from "./ProviderImport";
import { ProviderSync } from "./ProviderSync";

const isTauriRuntime = "__TAURI_INTERNALS__" in window;
const ROUTER_TIMEOUT_LIMITS = {
  connect_timeout_secs: 120,
  response_header_timeout_secs: 600,
  stream_idle_timeout_secs: 3600,
} as const;
const MAX_SLOW_DELAY_SECS = 3600;
const DEFAULT_GPT56_LONG_CONTEXT_WINDOW = 372_000;
const MAX_AUTO_DISABLE_FAILURE_THRESHOLD = 100;

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
    ai_gate_account_id: null,
  };
}

function defaultRouterConfig(): RouterConfig {
  return {
    enabled: false,
    debug_mode: false,
    force_disable_openai_auth: false,
    remote_compaction_enabled: false,
    token_budget_enabled: false,
    websockets_enabled: false,
    zstd_decompression_enabled: true,
    gpt56_long_context_enabled: false,
    gpt56_long_context_window: DEFAULT_GPT56_LONG_CONTEXT_WINDOW,
    auto_disable_enabled: true,
    auto_disable_failure_threshold: 3,
    model_provider: "custom",
    codex_model: "",
    host: "127.0.0.1",
    port: 18080,
    local_token: "",
    connect_timeout_secs: 10,
    response_header_timeout_secs: 180,
    stream_idle_timeout_secs: 180,
    slow_mode: "off",
    slow_delay_secs: 0,
    slow_delay_max_secs: 0,
  };
}

function routerConfigsEqual(left: RouterConfig, right: RouterConfig) {
  const leftKeys = Object.keys(left) as Array<keyof RouterConfig>;
  const rightKeys = Object.keys(right) as Array<keyof RouterConfig>;
  return (
    leftKeys.length === rightKeys.length &&
    leftKeys.every((key) => left[key] === right[key])
  );
}

function routerSelectedModelUnavailable(
  router: RouterConfig,
  models: string[],
) {
  return Boolean(
    router.codex_model.trim() &&
      !models.some(
        (model) =>
          model.toLowerCase() === router.codex_model.trim().toLowerCase(),
      ),
  );
}

function effectiveRouterCodexModelError(
  router: RouterConfig,
  models: string[],
  codexEnabled: boolean,
) {
  const selectionError = routerCodexModelError(router, models);
  return codexEnabled || routerSelectedModelUnavailable(router, models)
    ? selectionError
    : "";
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

function routerCodexModelError(router: RouterConfig, models: string[]) {
  const selected = router.codex_model.trim();
  if (!models.length) return "至少配置一个可路由模型后才能接管 Codex。";
  if (!selected) return "请选择 Codex 默认模型。";
  if (!models.some((model) => model.toLowerCase() === selected.toLowerCase())) {
    return "当前模型已不在可路由模型列表中。";
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
    router.response_header_timeout_secs >
      ROUTER_TIMEOUT_LIMITS.response_header_timeout_secs ||
    !Number.isInteger(router.stream_idle_timeout_secs) ||
    router.stream_idle_timeout_secs < 1 ||
    router.stream_idle_timeout_secs >
      ROUTER_TIMEOUT_LIMITS.stream_idle_timeout_secs
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

function autoDisableThresholdError(enabled: boolean, threshold: number) {
  if (!enabled) return "";
  if (
    !Number.isInteger(threshold) ||
    threshold < 1 ||
    threshold > MAX_AUTO_DISABLE_FAILURE_THRESHOLD
  ) {
    return `连续失败次数必须是 1 到 ${MAX_AUTO_DISABLE_FAILURE_THRESHOLD} 之间的整数。`;
  }
  return "";
}

function routerSlowModeError(router: RouterConfig) {
  if (router.slow_mode === "off") return "";
  if (
    !Number.isInteger(router.slow_delay_secs) ||
    router.slow_delay_secs < 1 ||
    router.slow_delay_secs > MAX_SLOW_DELAY_SECS
  ) {
    return `延迟必须是 1 到 ${MAX_SLOW_DELAY_SECS} 秒之间的整数。`;
  }
  if (router.slow_mode === "random") {
    if (
      !Number.isInteger(router.slow_delay_max_secs) ||
      router.slow_delay_max_secs < 1 ||
      router.slow_delay_max_secs > MAX_SLOW_DELAY_SECS
    ) {
      return `随机延迟上限必须是 1 到 ${MAX_SLOW_DELAY_SECS} 秒之间的整数。`;
    }
    if (router.slow_delay_max_secs < router.slow_delay_secs) {
      return "随机延迟上限不能小于下限。";
    }
  }
  return "";
}

function routerLongContextWindowError(router: RouterConfig) {
  if (!router.gpt56_long_context_enabled) return "";
  if (
    !Number.isSafeInteger(router.gpt56_long_context_window) ||
    router.gpt56_long_context_window < 1
  ) {
    return "上下文大小必须是大于 0 的整数。";
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
    if (!current || typeof current !== "object" || Array.isArray(current))
      return undefined;
    current = (current as Record<string, JsonValue>)[key];
  }
  return typeof current === "string" ? current : undefined;
}

function endpointFromBaseUrl(baseUrl: string) {
  return baseUrl
    .trim()
    .replace(/\/v1\/?$/, "")
    .replace(/\/$/, "");
}

function defaultBalancePath(
  queryType: BalanceQueryType,
  target: NewApiBalanceTarget,
) {
  if (queryType === "sub2_api") return "/v1/usage";
  if (queryType === "ai_gate") return "/api/me/upstreams/usage";
  if (queryType === "new_api" && target === "account_balance")
    return "/api/user/self";
  return "/api/usage/token/";
}

function normalizeBalanceQuery(
  config?: BalanceQueryConfig | null,
  endpoint = "",
) {
  const queryType = config?.query_type ?? "disabled";
  const target = config?.new_api_target ?? "token_quota";
  const next = {
    ...defaultBalanceQuery(endpoint),
    ...(config ?? {}),
    endpoint: config?.endpoint || endpoint,
    path: config?.path || defaultBalancePath(queryType, target),
  };
  if (
    next.query_type === "new_api" &&
    next.new_api_target === "account_balance"
  ) {
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
  if (abs >= 1_000_000)
    return `${(value / 1_000_000).toFixed(abs >= 10_000_000 ? 1 : 2)}M`;
  if (abs >= 1_000)
    return `${(value / 1_000).toFixed(abs >= 100_000 ? 0 : 1)}K`;
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

function filterForRange(
  range: TimeRange,
): Pick<RouteLogFilter, "start_day" | "end_day"> {
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
  const effectiveGranularity =
    granularity ??
    (range === "all" ? "month" : range === "today" ? "hour" : "day");
  const bucketByLabel = new Map(
    buckets.map((bucket) => [bucket.label, bucket]),
  );

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

  const sorted = [...buckets].sort((left, right) =>
    left.label.localeCompare(right.label),
  );
  return sorted.length ? sorted : [emptyTrendBucket("暂无")];
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
    steps.push(
      audit.compaction_response_forwarded
        ? "已返回 compaction"
        : "compaction 未透传",
    );
  }
  if (audit.compaction_item_reused) steps.push("已复用 compaction");
  return steps.join(" · ");
}

function normalizedServiceTier(value?: string | null) {
  const tier = value?.trim().toLowerCase() || "";
  return tier === "fast" ? "priority" : tier;
}

function serviceTierLabel(value?: string | null, emptyLabel = "未返回") {
  const tier = normalizedServiceTier(value);
  if (!tier) return emptyLabel;
  if (tier === "priority") return "Fast (priority)";
  if (tier === "ultrafast") return "Ultrafast";
  if (tier === "default") return "标准 (default)";
  if (tier === "flex") return "Flex";
  if (tier === "auto") return "自动 (auto)";
  return tier;
}

function serviceTierResult(log: RouteRequestLog) {
  const requested = normalizedServiceTier(log.requested_service_tier);
  const actual = normalizedServiceTier(log.actual_service_tier);
  if (!requested) return { label: "未强制", tone: "neutral" } as const;
  if (!actual) return { label: "未确认", tone: "amber" } as const;
  if (requested === actual) return { label: "已生效", tone: "ok" } as const;
  return { label: "未生效", tone: "danger" } as const;
}

function formatMs(value?: number | null) {
  if (value == null) return "-";
  if (value >= 1000)
    return `${(value / 1000).toFixed(value >= 10_000 ? 1 : 1)} s`;
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
  const slowDelay = log.slow_delay_ms;
  if (upstreamStarted == null) {
    return slowDelay == null
      ? []
      : [{ label: "Slow 模式延迟", durationMs: slowDelay }];
  }

  const phases: RouteTimingPhase[] = [];
  if (slowDelay != null) {
    phases.push({ label: "Slow 模式延迟", durationMs: slowDelay });
  }
  const beforeUpstream = Math.max(0, upstreamStarted - (slowDelay ?? 0));
  if (slowDelay == null || beforeUpstream > 0) {
    phases.push({
      label:
        log.route_attempts > 1 ? "发起最终上游前（含前置重试）" : "发起上游前",
      durationMs: beforeUpstream,
    });
  }
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
  phases.forEach((phase) =>
    lines.push(`${phase.label}：${formatDuration(phase.durationMs)}`),
  );
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

function formatDebugBodyText(text: string) {
  const trimmed = text.trim();
  if (!trimmed) return "";

  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    // Streaming Responses use SSE, with one JSON value per data line.
  }

  let formattedEvent = false;
  const lines = text.split(/\r?\n/).map((line) => {
    const match = line.match(/^(\s*data:\s*)(.+)$/);
    if (!match || match[2].trim() === "[DONE]") return line;
    try {
      const pretty = JSON.stringify(JSON.parse(match[2]), null, 2).split("\n");
      formattedEvent = true;
      return [
        `${match[1]}${pretty[0]}`,
        ...pretty
          .slice(1)
          .map((part) => `${" ".repeat(match[1].length)}${part}`),
      ].join("\n");
    } catch {
      return line;
    }
  });
  return formattedEvent ? lines.join("\n") : text;
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

function downloadTextFile(contents: string, type: string, filename: string) {
  const url = URL.createObjectURL(new Blob([contents], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}

function exportRouteLogsCsv(logs: RouteRequestLog[]) {
  const headers = [
    "时间",
    "状态",
    "模型",
    "供应商",
    "请求服务层级",
    "实际服务层级",
    "服务层级结果",
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
    requestModelLabel(log.model),
    log.provider_name,
    serviceTierLabel(log.requested_service_tier, "未强制"),
    serviceTierLabel(log.actual_service_tier),
    serviceTierResult(log).label,
    remoteCompactionV2AuditLabel(log),
    log.total_tokens,
    log.first_byte_ms ?? "",
    log.total_ms,
    log.route_result,
    log.error ?? "",
  ]);
  const csv = [headers, ...rows]
    .map((row) => row.map(csvEscape).join(","))
    .join("\n");
  downloadTextFile(
    `\uFEFF${csv}`,
    "text/csv;charset=utf-8",
    `xxswitch-route-logs-${Date.now()}.csv`,
  );
}

async function exportRouteDebugBundle(logs: RouteRequestLog[]) {
  const captures: Array<{
    route_log: RouteRequestLog;
    debug_capture: RouteDebugCapture;
  }> = [];
  const failures: Array<{ request_id: string; error: string }> = [];

  for (const log of logs) {
    try {
      const capture = await callCommand<RouteDebugCapture>(
        "load_route_debug_capture",
        {
          payload: { request_id: log.id },
        },
      );
      captures.push({ route_log: log, debug_capture: capture });
    } catch (err) {
      failures.push({ request_id: log.id, error: String(err) });
    }
  }

  const exportedAt = new Date();
  const bundle = {
    schema: "xxswitch.responses-debug-export",
    schema_version: 1,
    exported_at: exportedAt.toISOString(),
    requested_count: logs.length,
    capture_count: captures.length,
    failure_count: failures.length,
    captures,
    failures,
  };
  downloadTextFile(
    JSON.stringify(bundle, null, 2),
    "application/json;charset=utf-8",
    `xxswitch-responses-debug-${exportedAt.toISOString().replace(/[:.]/g, "-")}.json`,
  );
  return failures.length;
}

function formatBalanceForCard(label: string) {
  if (!label || label === "未配置") return "";
  return label
    .replace(/^账户余额\s*/, "")
    .replace(/^Key额度\s*/, "")
    .replace(/^余额\s*/, "");
}

function providerBalanceMeta(provider: ProviderSummary) {
  const label = formatBalanceForCard(provider.balance_label);
  const error = provider.balance_error?.trim() ?? "";
  if (
    provider.balance_label === "不适用" ||
    provider.provider_type === "Claude"
  ) {
    return { value: "不适用", detail: "无需余额查询", tone: "muted" };
  }
  if (!label || error === "未启用余额查询") {
    return { value: "未启用", detail: "余额查询", tone: "muted" };
  }
  if (error) {
    return {
      value: label || "查询异常",
      detail: "检查余额配置",
      tone: "danger",
    };
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
    value:
      provider.latency_label && provider.latency_label !== "-"
        ? provider.latency_label
        : `${provider.latency_ms} ms`,
    detail: provider.latency_ms > 500 ? "响应较慢" : "连接正常",
    tone: provider.latency_ms > 500 ? "warn" : "ok",
  };
}

function providerStatus(provider: ProviderSummary) {
  if (provider.status === "auto_disabled")
    return { label: "自动禁用", tone: "danger" };
  if (provider.status === "disabled" || !provider.enabled)
    return { label: "已停用", tone: "muted" };
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
  const baseUrl =
    jsonPath(provider?.config, ["model_providers", "custom", "base_url"]) ?? "";
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
  const icons = {
    dashboard: LayoutDashboard,
    route: Network,
    providers: Server,
    skills: Blocks,
    usage: ChartNoAxesCombined,
    requests: FileText,
    settings: Settings2,
  };
  const Icon = icons[type];
  return <Icon aria-hidden="true" className="nav-glyph" strokeWidth={1.8} />;
}

function StatusPill({ ok }: { ok: boolean }) {
  return (
    <Badge className={`service-pill ${ok ? "ok" : "warn"}`}>
      <span />
      {ok ? "服务正常" : "服务未接管"}
    </Badge>
  );
}

function ToolIcon({
  type,
}: {
  type: "settings" | "models" | "latency" | "balance";
}) {
  const icons = {
    settings: Settings2,
    models: Blocks,
    latency: Activity,
    balance: Wallet,
  };
  const Icon = icons[type];
  return <Icon aria-hidden="true" size={16} strokeWidth={1.8} />;
}

function App() {
  const [appState, setAppState] = useState<AppState | null>(null);
  const [skillManagement, setSkillManagement] =
    useState<SkillManagementView | null>(null);
  const [routeUsageStats, setRouteUsageStats] =
    useState<RouteUsageStats | null>(null);
  const [routeLogs, setRouteLogs] = useState<RouteLogsResponse | null>(null);
  const [usageRange, setUsageRange] = useState<TimeRange>("today");
  const [usageFilter, setUsageFilter] = useState<RouteLogFilter>(() => ({
    model: "",
    page_size: 20,
    ...filterForRange("today"),
  }));
  const [trendMetric, setTrendMetric] = useState<TrendMetric>("tokens");
  const [requestFilter, setRequestFilter] = useState<RouteLogFilter>({
    model: "",
    page_size: 20,
  });
  const [requestAutoRefresh, setRequestAutoRefresh] = useState(true);
  const [screen, setScreen] = useState<Screen>("dashboard");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [pendingScreen, setPendingScreen] = useState<Screen | null>(null);
  const [providerKind, setProviderKind] = useState<ProviderKind>("codex");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [updateCheck, setUpdateCheck] = useState<UpdateCheckInfo | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateConfirming, setUpdateConfirming] = useState(false);
  const [updateMessage, setUpdateMessage] = useState("");
  const [autostartEnabled, setAutostartEnabled] = useState<boolean | null>(
    null,
  );
  const [autostartBusy, setAutostartBusy] = useState(false);
  const [editorOpen, setEditorOpen] = useState(false);
  const [editorKind, setEditorKind] = useState<ProviderKind>("codex");
  const [editorTab, setEditorTab] = useState<EditorTab>("base");
  const [editingId, setEditingId] = useState("");
  const [providerName, setProviderName] = useState("");
  const [providerBaseUrl, setProviderBaseUrl] = useState("");
  const [providerApiKey, setProviderApiKey] = useState("");
  const [providerApiKeyDirty, setProviderApiKeyDirty] = useState(false);
  const [providerEnabled, setProviderEnabled] = useState(true);
  const [capabilityBusy, setCapabilityBusy] = useState(false);
  const [capabilityMessage, setCapabilityMessage] = useState("");
  const [fusionEnabled, setFusionEnabled] = useState(false);
  const [modelListCleared, setModelListCleared] = useState(false);
  const [providerEnabledDirty, setProviderEnabledDirty] = useState(false);
  const [providerAutoDisableResetPeriod, setProviderAutoDisableResetPeriod] =
    useState<AutoDisableResetPeriod>("daily");
  const [providerAutoDisableResetWeekday, setProviderAutoDisableResetWeekday] =
    useState(1);
  const [providerAutoDisableResetDay, setProviderAutoDisableResetDay] =
    useState(1);
  const [providerAutoDisableResetTime, setProviderAutoDisableResetTime] =
    useState("00:00");
  const [providerAutoDisableOverride, setProviderAutoDisableOverride] =
    useState(false);
  const [providerAutoDisableEnabled, setProviderAutoDisableEnabled] =
    useState(true);
  const [
    providerAutoDisableFailureThreshold,
    setProviderAutoDisableFailureThreshold,
  ] = useState(3);
  const [providerWireApi, setProviderWireApi] =
    useState<ProviderWireApi>("responses");
  const [providerSupportsWebsockets, setProviderSupportsWebsockets] =
    useState(false);
  const [
    providerImageDetailOriginalToAuto,
    setProviderImageDetailOriginalToAuto,
  ] = useState(false);
  const [providerServiceTier, setProviderServiceTier] =
    useState<ProviderServiceTier>("");
  const [providerTestModel, setProviderTestModel] = useState("");
  const [providerModels, setProviderModels] = useState<string[]>([]);
  const [allowedModels, setAllowedModels] = useState<string[]>([]);
  const [modelMappings, setModelMappings] = useState<ModelMapping[]>([]);
  const [balanceQuery, setBalanceQuery] = useState<BalanceQueryConfig>(() =>
    defaultBalanceQuery(),
  );
  const [balanceTestStatus, setBalanceTestStatus] =
    useState<BalanceStatus | null>(null);
  const [balanceAccountOptions, setBalanceAccountOptions] = useState<
    AiGateBalanceAccount[]
  >([]);
  const [routerDraft, setRouterDraft] = useState<RouterConfig>(() =>
    defaultRouterConfig(),
  );
  const [proxyDraft, setProxyDraft] = useState<RouterConfig>(() =>
    defaultRouterConfig(),
  );
  const [secretVisible, setSecretVisible] = useState(false);
  const [balanceTokenVisible, setBalanceTokenVisible] = useState(false);
  const [latencyDialog, setLatencyDialog] =
    useState<ProviderLatencyDialogState | null>(null);
  const [newProviderCount, setNewProviderCount] = useState(1);
  const [pendingSharedSkillDeletion, setPendingSharedSkillDeletion] =
    useState<SharedSkillView | null>(null);
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

  const logRequestSequence = useRef(0);
  const usageRequestSequence = useRef(0);
  const logRequestsInFlight = useRef(0);

  async function refreshRouteUsage(filter = usageFilter) {
    const sequence = ++usageRequestSequence.current;
    const usage = await callCommand<RouteUsageStats>("load_route_usage_stats", {
      payload: { filter },
    });
    if (sequence === usageRequestSequence.current) setRouteUsageStats(usage);
  }

  async function refreshRouteLogs(filter = requestFilter) {
    const sequence = ++logRequestSequence.current;
    logRequestsInFlight.current += 1;
    try {
      const logs = await callCommand<RouteLogsResponse>("load_route_logs", {
        payload: { filter },
      });
      if (sequence === logRequestSequence.current) setRouteLogs(logs);
    } finally {
      logRequestsInFlight.current -= 1;
    }
  }

  async function refreshAppState() {
    const state = await callCommand<AppState>("load_app_state");
    setAppState(state);
    setRouterDraft(state.router ?? defaultRouterConfig());
    setProxyDraft(state.router ?? defaultRouterConfig());
  }

  async function refreshSkillManagement() {
    const view = await callCommand<SkillManagementView>(
      "load_skill_management",
    );
    setSkillManagement(view);
  }

  async function refresh() {
    await refreshAppState();
    try {
      if (screen === "requests") await refreshRouteLogs();
      else if (screen === "dashboard" || screen === "usage")
        await refreshRouteUsage();
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
    if (!updateCheck?.available || !updateCheck.installable || updateBusy)
      return;
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

  async function refreshAutostart() {
    setAutostartEnabled(await callCommand<boolean>("get_autostart_enabled"));
  }

  async function updateAutostart(enabled: boolean) {
    if (autostartBusy) return;
    setAutostartBusy(true);
    setError("");
    try {
      setAutostartEnabled(
        await callCommand<boolean>("set_autostart_enabled", { enabled }),
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setAutostartBusy(false);
    }
  }

  useEffect(() => {
    refresh().catch((err) => setError(String(err)));
  }, []);

  const trayStateRef = useRef(appState);
  trayStateRef.current = appState;
  const trayNavigateRef = useRef(navigateToScreen);
  trayNavigateRef.current = navigateToScreen;
  useEffect(() => {
    if (
      !appState ||
      !isTauriRuntime ||
      (import.meta.env.DEV && import.meta.env.MODE === "ui-preview")
    )
      return;
    let cancelled = false;
    let sequence = 0;
    async function sync() {
      const request = ++sequence;
      const previous = trayStateRef.current;
      try {
        const next = await callCommand<AppState>("load_app_state");
        if (cancelled || request !== sequence) return;
        setAppState(next);
        setRouterDraft((draft) =>
          mergeTrayDraft(draft, previous?.router, next.router),
        );
        setProxyDraft((draft) =>
          mergeTrayDraft(draft, previous?.router, next.router),
        );
      } catch (err) {
        if (!cancelled) setError(String(err));
      }
    }
    async function navigate() {
      try {
        const next = await callCommand<Screen | null>("take_tray_navigation");
        if (
          !cancelled &&
          next &&
          ["dashboard", "route", "providers", "usage", "settings"].includes(
            next,
          )
        )
          trayNavigateRef.current(next);
      } catch (err) {
        if (!cancelled) setError(String(err));
      }
    }
    const subscriptions = Promise.all([
      listen("tray-state-changed", () => void sync()),
      listen("tray-navigate", () => void navigate()),
    ]);
    void subscriptions
      .then(() => {
        if (!cancelled) {
          void sync();
          void navigate();
        }
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
      void subscriptions
        .then((unlisten) => unlisten.forEach((fn) => fn()))
        .catch(() => {});
    };
  }, [appState !== null]);

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
      if (logRequestsInFlight.current > 0) return;
      refreshRouteLogs(requestFilter).catch((err) => setError(String(err)));
    }, 5000);
    return () => window.clearInterval(timer);
  }, [requestAutoRefresh, requestFilter, screen]);

  useEffect(() => {
    if (screen !== "settings") return;
    refreshAutostart().catch((err) => setError(String(err)));
  }, [screen]);

  useEffect(() => {
    if (shellRef.current) shellRef.current.scrollTop = 0;
  }, [screen]);

  const routeDirty = Boolean(
    appState && !routerConfigsEqual(routerDraft, appState.router),
  );

  useEffect(() => {
    if (!routeDirty) return;
    const handleBeforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", handleBeforeUnload);
    return () => window.removeEventListener("beforeunload", handleBeforeUnload);
  }, [routeDirty]);

  const activeProvider = useMemo(() => {
    if (!appState) return null;
    return (
      appState.providers.find(
        (provider) => provider.id === appState.active_provider_id,
      ) ??
      appState.providers[0] ??
      null
    );
  }, [appState]);
  const requestCount =
    routeUsageStats?.success_count ??
    routeUsageStats?.summary.request_count ??
    0;
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
  const successRate = totalFinishedCount
    ? (requestCount / totalFinishedCount) * 100
    : 0;

  function fillProviderEditor(
    targetFull: ProviderConfig,
    summary: ProviderSummary | null,
    tab: EditorTab,
  ) {
    const fields = providerFields(targetFull);
    setEditorKind("codex");
    setEditingId(summary?.id ?? targetFull.id);
    setProviderName(summary?.name ?? targetFull.name ?? "");
    setProviderBaseUrl(summary?.base_url || fields.baseUrl);
    setProviderApiKey(fields.apiKey);
    setProviderApiKeyDirty(false);
    setProviderEnabled((summary?.status ?? targetFull.status) === "enabled");
    setProviderEnabledDirty(false);
    setFusionEnabled(targetFull.fusion_enabled ?? false);
    setModelListCleared(false);
    setProviderAutoDisableResetPeriod(
      targetFull.auto_disable_reset_period ??
        summary?.auto_disable_reset_period ??
        "daily",
    );
    setProviderAutoDisableResetWeekday(
      targetFull.auto_disable_reset_weekday ??
        summary?.auto_disable_reset_weekday ??
        1,
    );
    setProviderAutoDisableResetDay(
      targetFull.auto_disable_reset_day ?? summary?.auto_disable_reset_day ?? 1,
    );
    setProviderAutoDisableResetTime(
      targetFull.auto_disable_reset_time ??
        summary?.auto_disable_reset_time ??
        "00:00",
    );
    setProviderAutoDisableOverride(targetFull.auto_disable_override ?? false);
    setProviderAutoDisableEnabled(
      targetFull.auto_disable_enabled ??
        appState?.router.auto_disable_enabled ??
        true,
    );
    setProviderAutoDisableFailureThreshold(
      targetFull.auto_disable_failure_threshold ??
        appState?.router.auto_disable_failure_threshold ??
        3,
    );
    setProviderWireApi(targetFull.wire_api ?? "responses");
    setProviderSupportsWebsockets(targetFull.supports_websockets ?? false);
    setProviderImageDetailOriginalToAuto(
      targetFull.image_detail_original_to_auto ?? false,
    );
    const serviceTier = targetFull.service_tier?.trim().toLowerCase();
    setProviderServiceTier(
      serviceTier === "priority" || serviceTier === "fast"
        ? "priority"
        : serviceTier === "ultrafast"
          ? "ultrafast"
          : "",
    );
    setProviderTestModel(targetFull.connection_test_model ?? "");
    setProviderModels(
      targetFull.allowed_models?.length
        ? targetFull.allowed_models
        : targetFull.connection_test_model
          ? [targetFull.connection_test_model]
          : [],
    );
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
    setBalanceAccountOptions(targetFull.balance_status?.accounts ?? []);
    setEditorOpen(true);
  }

  function fillClaudeProviderEditor(
    targetFull: ClaudeProviderConfig,
    summary: ProviderSummary | null,
  ) {
    setEditorKind("claude");
    setEditingId(summary?.id ?? targetFull.id);
    setProviderName(summary?.name ?? targetFull.name ?? "");
    setProviderBaseUrl(summary?.base_url || targetFull.base_url || "");
    setProviderApiKey(targetFull.api_key || "");
    setProviderApiKeyDirty(false);
    setProviderEnabled((summary?.status ?? targetFull.status) === "enabled");
    setProviderEnabledDirty(false);
    setFusionEnabled(targetFull.fusion_enabled ?? false);
    setModelListCleared(false);
    setProviderAutoDisableResetPeriod(
      targetFull.auto_disable_reset_period ??
        summary?.auto_disable_reset_period ??
        "daily",
    );
    setProviderAutoDisableResetWeekday(
      targetFull.auto_disable_reset_weekday ??
        summary?.auto_disable_reset_weekday ??
        1,
    );
    setProviderAutoDisableResetDay(
      targetFull.auto_disable_reset_day ?? summary?.auto_disable_reset_day ?? 1,
    );
    setProviderAutoDisableResetTime(
      targetFull.auto_disable_reset_time ??
        summary?.auto_disable_reset_time ??
        "00:00",
    );
    setProviderAutoDisableOverride(targetFull.auto_disable_override ?? false);
    setProviderAutoDisableEnabled(
      targetFull.auto_disable_enabled ??
        appState?.router.auto_disable_enabled ??
        true,
    );
    setProviderAutoDisableFailureThreshold(
      targetFull.auto_disable_failure_threshold ??
        appState?.router.auto_disable_failure_threshold ??
        3,
    );
    setProviderTestModel(targetFull.connection_test_model ?? "");
    setProviderModels(
      targetFull.allowed_models?.length
        ? targetFull.allowed_models
        : targetFull.connection_test_model
          ? [targetFull.connection_test_model]
          : [],
    );
    setAllowedModels(targetFull.allowed_models ?? []);
    setModelMappings(targetFull.model_mappings ?? []);
    setEditorTab("base");
    setSecretVisible(false);
    setEditorOpen(true);
  }

  async function openProviderEditor(
    provider?: ProviderSummary,
    tab: EditorTab = "base",
  ) {
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
      const targetFull = await callCommand<ClaudeProviderConfig>(
        "get_claude_provider",
        {
          providerId: targetSummary.id,
        },
      );
      fillClaudeProviderEditor(targetFull, targetSummary);
    });
  }

  async function addProvider() {
    await run(async () => {
      const name = `新供应商 ${newProviderCount}`;
      const state = await callCommand<AppState>("add_provider", { name });
      setNewProviderCount((value) => value + 1);
      setAppState(state);
      const created = state.providers.find(
        (provider) => provider.id === state.active_provider_id,
      );
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
      const state = await callCommand<AppState>("add_claude_provider", {
        name,
      });
      setNewProviderCount((value) => value + 1);
      setAppState(state);
      const created = state.claude_providers.find(
        (provider) => provider.id === state.active_claude_provider_id,
      );
      if (created) {
        const targetFull = await callCommand<ClaudeProviderConfig>(
          "get_claude_provider",
          {
            providerId: created.id,
          },
        );
        fillClaudeProviderEditor(targetFull, created);
      }
    });
  }

  async function updateModelCapabilities() {
    setCapabilityBusy(true);
    setCapabilityMessage("");
    try {
      const state = await callCommand<AppState>("update_model_capabilities");
      setAppState(state);
      setCapabilityMessage(
        `能力目录已更新为 ${state.capability_version}，请彻底重启 Codex 以刷新模型列表。`,
      );
    } catch (error) {
      setCapabilityMessage(
        `更新失败：${String(error)}。继续使用已有能力目录。`,
      );
    } finally {
      setCapabilityBusy(false);
    }
  }

  async function saveProvider() {
    if (!editingId) return;
    await run(async () => {
      const thresholdError = autoDisableThresholdError(
        providerAutoDisableOverride && providerAutoDisableEnabled,
        providerAutoDisableFailureThreshold,
      );
      if (thresholdError) throw new Error(thresholdError);
      if (modelListCleared && allowedModels.length === 0) {
        throw new Error(
          "模型列表已清空，请重新获取或手动添加模型后保存；如需停止使用供应商，请将其停用。",
        );
      }
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
        supports_websockets:
          providerWireApi === "responses" && providerSupportsWebsockets,
        image_detail_original_to_auto: providerImageDetailOriginalToAuto,
        service_tier: providerServiceTier,
        auto_disable_reset_period: providerAutoDisableResetPeriod,
        auto_disable_reset_weekday: providerAutoDisableResetWeekday,
        auto_disable_reset_day: providerAutoDisableResetDay,
        auto_disable_reset_time: providerAutoDisableResetTime,
        auto_disable_override: providerAutoDisableOverride,
        auto_disable_enabled: providerAutoDisableEnabled,
        auto_disable_failure_threshold: providerAutoDisableFailureThreshold,
        balance_query: nextBalance,
        balance_status: balanceTestStatus,
        connection_test_model: providerTestModel,
        allowed_models: allowedModels,
        fusion_enabled: fusionEnabled,
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
      const thresholdError = autoDisableThresholdError(
        providerAutoDisableOverride && providerAutoDisableEnabled,
        providerAutoDisableFailureThreshold,
      );
      if (thresholdError) throw new Error(thresholdError);
      if (modelListCleared && allowedModels.length === 0) {
        throw new Error(
          "模型列表已清空，请重新获取或手动添加模型后保存；如需停止使用供应商，请将其停用。",
        );
      }
      const payload: Record<string, unknown> = {
        provider_id: editingId,
        provider_name: providerName,
        auto_disable_reset_period: providerAutoDisableResetPeriod,
        auto_disable_reset_weekday: providerAutoDisableResetWeekday,
        auto_disable_reset_day: providerAutoDisableResetDay,
        auto_disable_reset_time: providerAutoDisableResetTime,
        auto_disable_override: providerAutoDisableOverride,
        auto_disable_enabled: providerAutoDisableEnabled,
        auto_disable_failure_threshold: providerAutoDisableFailureThreshold,
        base_url: providerBaseUrl,
        connection_test_model: providerTestModel,
        allowed_models: allowedModels,
        fusion_enabled: fusionEnabled,
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
      const queriedProvider = await callCommand<ProviderConfig>(
        "get_provider",
        {
          providerId: editingId,
        },
      );
      const status = queriedProvider.balance_status ?? null;
      setBalanceTestStatus(status);
      setBalanceAccountOptions(status?.accounts ?? []);
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
          endpoint:
            balanceQuery.endpoint || endpointFromBaseUrl(providerBaseUrl),
        },
      };
      if (providerApiKeyDirty) {
        payload.api_key = providerApiKey;
      }
      const result = await callCommand<ProviderModelsResponse>(
        "load_provider_models",
        {
          payload,
        },
      );
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
      const result = await callCommand<ProviderModelsResponse>(
        "load_claude_provider_models",
        {
          payload,
        },
      );
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

  async function toggleClaudeProvider(
    provider: ProviderSummary,
    enabled: boolean,
  ) {
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

  async function openProviderLatencyDialog(
    provider: ProviderSummary,
    kind: ProviderKind,
  ) {
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
      const config =
        kind === "claude"
          ? await callCommand<ClaudeProviderConfig>("get_claude_provider", {
              providerId: provider.id,
            })
          : await callCommand<ProviderConfig>("get_provider", {
              providerId: provider.id,
            });
      let models = normalizeModelNames([
        ...config.allowed_models,
        ...config.model_mappings.map((mapping) => mapping.target),
        config.connection_test_model,
      ]);
      try {
        const response = await callCommand<ProviderModelsResponse>(
          kind === "claude"
            ? "load_claude_provider_models"
            : "load_provider_models",
          { payload: { provider_id: provider.id } },
        );
        if (response.models.length > 0) models = response.models;
      } catch (err) {
        if (models.length === 0) throw err;
      }
      const preferredModel = config.connection_test_model.trim();
      const selectedModel =
        models.find(
          (model) => model.toLowerCase() === preferredModel.toLowerCase(),
        ) ??
        models[0] ??
        "";
      setLatencyDialog((current) =>
        current?.provider.id === provider.id
          ? { ...current, models, selectedModel, preparing: false }
          : current,
      );
    } catch (err) {
      setLatencyDialog((current) =>
        current?.provider.id === provider.id
          ? {
              ...current,
              preparing: false,
              result: { ok: false, message: String(err) },
            }
          : current,
      );
    }
  }

  async function testProviderLatency() {
    if (
      !latencyDialog ||
      !latencyDialog.selectedModel ||
      !latencyDialog.prompt.trim()
    )
      return;
    const target = latencyDialog;
    setLatencyDialog({ ...target, testing: true, result: null });
    try {
      const response = await callCommand<ProviderLatencyTestResponse>(
        "test_provider_latency_state",
        {
          payload: {
            provider_id: target.provider.id,
            provider_kind: target.providerKind,
            model: target.selectedModel,
            prompt: target.prompt.trim(),
            stream: target.streaming,
          },
        },
      );
      setAppState(response.app_state);
      setLatencyDialog((current) =>
        current?.provider.id === target.provider.id
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
          : current,
      );
    } catch (err) {
      setLatencyDialog((current) =>
        current?.provider.id === target.provider.id
          ? {
              ...current,
              testing: false,
              result: { ok: false, message: String(err) },
            }
          : current,
      );
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
        setProxyDraft(applied.router);
      } else {
        setAppState(saved);
        setRouterDraft(saved.router);
        setProxyDraft(saved.router);
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
      const view = await callCommand<SkillManagementView>(
        "save_skill_client_config",
        {
          payload: {
            client,
            skill_locations: skillLocations,
            managed_skill_location: managedSkillLocation,
          },
        },
      );
      setSkillManagement(view);
    });
  }

  async function promoteClientSkill(skill: ClientSkillView) {
    await run(async () => {
      const view = await callCommand<SkillManagementView>(
        "promote_client_skill",
        {
          payload: {
            client: skill.client,
            skill_path: skill.path,
            sharing_scope: [skill.client],
          },
        },
      );
      setSkillManagement(view);
    });
  }

  async function useExistingSharedSkill(skill: ClientSkillView) {
    await run(async () => {
      const view = await callCommand<SkillManagementView>(
        "replace_client_skill_with_shared",
        {
          payload: {
            client: skill.client,
            skill_path: skill.path,
          },
        },
      );
      setSkillManagement(view);
    });
  }

  async function setSkillSharingScope(
    skill: SharedSkillView,
    scope: AgentClientKind[],
  ) {
    await run(async () => {
      const view = await callCommand<SkillManagementView>(
        "set_skill_sharing_scope",
        {
          payload: {
            skill_identity: skill.identity,
            sharing_scope: scope,
          },
        },
      );
      setSkillManagement(view);
    });
  }

  async function deleteSharedSkill(skill: SharedSkillView) {
    await run(async () => {
      const view = await callCommand<SkillManagementView>(
        "delete_shared_skill",
        {
          payload: {
            skill_identity: skill.identity,
          },
        },
      );
      setSkillManagement(view);
      setPendingSharedSkillDeletion(null);
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
      if (
        next.query_type === "new_api" &&
        next.new_api_target === "account_balance"
      ) {
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
        {error && <Button onClick={() => refresh()}>重试</Button>}
      </main>
    );
  }

  const savedRouter = appState.router;
  const routeSaveDisabled =
    busy ||
    !routeDirty ||
    Boolean(routerModelProviderError(routerDraft)) ||
    Boolean(
      effectiveRouterCodexModelError(
        routerDraft,
        appState.codex_models,
        appState.clients.codex.enabled,
      ),
    );

  function navigateToScreen(nextScreen: Screen) {
    if (nextScreen === screen) return;
    if (screen === "route" && routeDirty) {
      setPendingScreen(nextScreen);
      return;
    }
    setScreen(nextScreen);
  }

  function discardRouteChangesAndNavigate() {
    if (!pendingScreen) return;
    setRouterDraft(savedRouter);
    setScreen(pendingScreen);
    setPendingScreen(null);
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
    <main className={`app ${sidebarCollapsed ? "sidebar-collapsed" : ""}`}>
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-logo">
            <img src={brandMark} alt="" />
          </div>
          <div>
            <strong>XXSwitch</strong>
            <small>你的本地 AI 工作区</small>
          </div>
        </div>

        <nav aria-label="主导航" className="nav">
          {navGroups.map((group) => (
            <div className="nav-section" key={group.label}>
              <span className="nav-section-label">{group.label}</span>
              {group.items.map(([key, label]) => (
                <Button
                  aria-current={screen === key ? "page" : undefined}
                  title={label}
                  className={screen === key ? "active" : ""}
                  key={key}
                  onClick={() => navigateToScreen(key as Screen)}
                  type="button"
                >
                  <NavIcon type={key as Screen} />
                  <span>{label}</span>
                </Button>
              ))}
            </div>
          ))}
        </nav>

        <div className="sidebar-footer">
          <div className="gateway-status">
            <span
              className={`gateway-status-icon ${routerOn ? "online" : "offline"}`}
            >
              <Network size={18} aria-hidden="true" />
            </span>
            <div>
              <strong>
                本地网关
                <span className={`dot ${routerOn ? "ok" : "muted"}`} />
              </strong>
              <code>
                {appState.router.host}:{appState.router.port}
              </code>
            </div>
          </div>
          <div className="sidebar-meta">
            <small>{appState.app_version.replace(/^v?/, "v")}</small>
            <AppearanceControl />
          </div>
        </div>
      </aside>

      <section className="shell" ref={shellRef}>
        <header className="topbar">
          <Button
            aria-label={sidebarCollapsed ? "展开侧栏" : "收起侧栏"}
            className="icon-button sidebar-toggle"
            onClick={() => setSidebarCollapsed(!sidebarCollapsed)}
          >
            {sidebarCollapsed ? (
              <PanelLeftOpen size={18} aria-hidden="true" />
            ) : (
              <PanelLeftClose size={18} aria-hidden="true" />
            )}
          </Button>
          <div className="topbar-heading">
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
                  ? "管理 Codex、Claude 与 Pi 的本地接管"
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
            {import.meta.env.DEV && import.meta.env.MODE === "ui-preview" && (
              <Badge className="preview-badge">模拟数据 · 独立预览</Badge>
            )}
            <StatusPill ok={routerOn} />
            <div className="client-switch">
              <Code2 size={14} aria-hidden="true" />
              <span>Codex</span>
              <Toggle
                checked={appState.clients.codex.enabled}
                label="接管 Codex"
                disabled={busy}
                onChange={(checked) => void saveClientConfig("codex", checked)}
              />
            </div>
            <div className="client-switch">
              <span>Claude</span>
              <Toggle
                checked={appState.clients.claude.enabled}
                label="接管 Claude Code"
                disabled={busy}
                onChange={(checked) => void saveClientConfig("claude", checked)}
              />
            </div>
            <div className="client-switch">
              <span>Pi</span>
              <Toggle
                checked={appState.clients.pi.enabled}
                label="接管 Pi"
                disabled={busy}
                onChange={(checked) => void saveClientConfig("pi", checked)}
              />
            </div>
            {screen === "route" && (
              <div className="topbar-route-actions">
                <Button
                  className="ghost"
                  disabled={busy || !routeDirty}
                  onClick={() => setRouterDraft(savedRouter)}
                  type="button"
                >
                  撤销修改
                </Button>
                <Button
                  className="primary"
                  disabled={routeSaveDisabled}
                  onClick={() => void saveRouter(routerDraft, true)}
                  type="button"
                >
                  保存修改
                </Button>
              </div>
            )}
          </div>
        </header>

        {error && !editorOpen && (
          <div className="error-banner" role="alert">
            {error}
            <Button
              aria-label="关闭错误提示"
              className="close"
              onClick={() => setError("")}
            >
              ×
            </Button>
          </div>
        )}

        {screen === "dashboard" && (
          <Dashboard
            onManageProviders={() => navigateToScreen("providers")}
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
            setRouterDraft={setRouterDraft}
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
            onRefreshBalance={
              providerKind === "claude"
                ? async (_provider) => undefined
                : refreshProviderBalance
            }
            onReorder={
              providerKind === "claude"
                ? reorderClaudeProviders
                : reorderProviders
            }
            onTestLatency={(provider) =>
              openProviderLatencyDialog(provider, providerKind)
            }
            onToggle={
              providerKind === "claude" ? toggleClaudeProvider : toggleProvider
            }
            providerKind={providerKind}
            providers={
              providerKind === "claude"
                ? appState.claude_providers
                : appState.providers
            }
          />
        )}

        {screen === "skills" && (
          <SkillManagementScreen
            busy={busy}
            onDeleteSharedSkill={async (skill) => {
              setError("");
              setPendingSharedSkillDeletion(skill);
            }}
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
            configuredProviders={[
              ...appState.providers,
              ...appState.claude_providers,
            ]}
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
            <ProviderSync
              onDownloaded={async () => {
                setAppState(await callCommand<AppState>("load_app_state"));
              }}
            />
            <LocalProxySettings
              appState={appState}
              busy={busy}
              onSaveRouter={saveRouter}
              routerDraft={proxyDraft}
              routerOn={routerOn}
              setRouterDraft={setProxyDraft}
            />
            <Card className="page-panel">
              <div className="panel-head">
                <div>
                  <h2>应用</h2>
                  <p>查看当前版本并获取最新稳定版本。</p>
                </div>
              </div>
              <div className="settings-list">
                <div className="settings-row application-setting-row">
                  <div className="settings-row-description">
                    <span>界面外观</span>
                    <small>选择浅色、深色，或自动跟随系统。</small>
                  </div>
                  <AppearanceControl />
                </div>
                <TrayPreferenceSetting />
                <div className="settings-row application-setting-row">
                  <div className="settings-row-description">
                    <span>开机自启</span>
                    <small>登录系统后在后台启动 XXSwitch</small>
                  </div>
                  <Toggle
                    checked={autostartEnabled ?? false}
                    disabled={autostartBusy || autostartEnabled === null}
                    label="开机自启"
                    onChange={(enabled) => void updateAutostart(enabled)}
                  />
                </div>
                <div className="settings-row">
                  <span>当前版本</span>
                  <strong>{appState.app_version}</strong>
                </div>
                <div className="settings-row update-check-row">
                  <div className="settings-row-description">
                    <span>模型能力目录</span>
                    <small>
                      OpenAI · DeepSeek · 默认模板 /{" "}
                      {appState.capability_version}
                    </small>
                    <small>
                      {appState.capability_updated_at
                        ? `上次获取：${appState.capability_updated_at}`
                        : "正在使用安装包目录快照"}
                    </small>
                    {capabilityMessage && (
                      <small role="status">{capabilityMessage}</small>
                    )}
                  </div>
                  <Button
                    className="ghost"
                    disabled={capabilityBusy || busy}
                    onClick={() => void updateModelCapabilities()}
                    type="button"
                  >
                    {capabilityBusy ? "正在获取…" : "更新模型能力"}
                  </Button>
                </div>
                <div className="settings-row update-check-row">
                  <div className="settings-row-description">
                    <span>远端更新</span>
                    <small>从 GitHub Release 获取最新稳定版本</small>
                  </div>
                  <Button
                    className="ghost"
                    disabled={updateBusy}
                    onClick={() => void checkForUpdate()}
                    type="button"
                  >
                    {updateBusy ? "正在检查…" : "检查更新"}
                  </Button>
                </div>
              </div>
              {updateCheck && (
                <div
                  className={`update-result ${updateCheck.available ? "available" : "latest"}`}
                >
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
                          {updateCheck.asset_name
                            ?.toLowerCase()
                            .endsWith(".dmg")
                            ? "确认后将自动替换当前应用并重启；没有写入权限时 macOS 会请求管理员授权。"
                            : "确认后将下载安装包并退出当前应用。"}
                        </span>
                      )}
                      {updateConfirming && !updateBusy && (
                        <Button
                          className="ghost"
                          onClick={() => setUpdateConfirming(false)}
                          type="button"
                        >
                          取消
                        </Button>
                      )}
                      <Button
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
                        {updateBusy
                          ? "正在更新…"
                          : updateConfirming
                            ? "确认更新"
                            : "一键更新"}
                      </Button>
                    </div>
                  )}
                </div>
              )}
              {updateMessage && (
                <div className="update-message">{updateMessage}</div>
              )}
            </Card>
          </section>
        )}
      </section>

      <ProviderImport
        onImported={async () => {
          // Preserve unsaved route and provider drafts while updating the saved list.
          setAppState(await callCommand<AppState>("load_app_state"));
        }}
      />

      {pendingSharedSkillDeletion && (
        <ConfirmDialog
          title="删除共享 Skill"
          busy={busy}
          error={error}
          onClose={() => setPendingSharedSkillDeletion(null)}
          onConfirm={() => deleteSharedSkill(pendingSharedSkillDeletion)}
        >
          删除 Shared Skill「{pendingSharedSkillDeletion.identity}」并移除所有
          exposure？
        </ConfirmDialog>
      )}

      {pendingScreen && (
        <DialogSurface
          title="未保存路由设置"
          className="unsaved-route-dialog"
          onClose={() => setPendingScreen(null)}
        >
          <header>
            <div>
              <h2 id="unsaved-route-dialog-title">未保存路由设置</h2>
              <p>离开当前页面将丢失这些修改。</p>
            </div>
          </header>
          <div className="unsaved-route-dialog-body">
            你对路由配置所做的修改尚未保存。可以继续编辑并使用右上角的保存按钮，或放弃本次修改。
          </div>
          <footer>
            <Button
              autoFocus
              className="ghost"
              onClick={() => setPendingScreen(null)}
              type="button"
            >
              继续编辑
            </Button>
            <Button
              className="danger"
              onClick={discardRouteChangesAndNavigate}
              type="button"
            >
              放弃修改
            </Button>
          </footer>
        </DialogSurface>
      )}

      {latencyDialog && (
        <ProviderLatencyDialog
          state={latencyDialog}
          onClose={() => setLatencyDialog(null)}
          onPromptChange={(prompt) =>
            setLatencyDialog((current) =>
              current ? { ...current, prompt, result: null } : current,
            )
          }
          onModelChange={(selectedModel) =>
            setLatencyDialog((current) =>
              current ? { ...current, selectedModel, result: null } : current,
            )
          }
          onStreamingChange={(streaming) =>
            setLatencyDialog((current) =>
              current ? { ...current, streaming, result: null } : current,
            )
          }
          onTest={() => void testProviderLatency()}
        />
      )}

      {editorOpen && editorKind === "codex" && (
        <ProviderEditor
          error={error}
          balanceAccountOptions={balanceAccountOptions}
          balanceQuery={balanceQuery}
          balanceTestStatus={balanceTestStatus}
          balanceTokenVisible={balanceTokenVisible}
          busy={busy}
          modelListCleared={modelListCleared}
          onClearModels={() => {
            setAllowedModels([]);
            setProviderModels([]);
            setModelListCleared(true);
          }}
          fusionEnabled={fusionEnabled}
          setFusionEnabled={setFusionEnabled}
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
          providerAutoDisableResetPeriod={providerAutoDisableResetPeriod}
          providerAutoDisableResetWeekday={providerAutoDisableResetWeekday}
          providerAutoDisableResetDay={providerAutoDisableResetDay}
          providerAutoDisableResetTime={providerAutoDisableResetTime}
          providerAutoDisableOverride={providerAutoDisableOverride}
          providerAutoDisableEnabled={providerAutoDisableEnabled}
          providerAutoDisableFailureThreshold={
            providerAutoDisableFailureThreshold
          }
          globalAutoDisableEnabled={appState.router.auto_disable_enabled}
          globalAutoDisableFailureThreshold={
            appState.router.auto_disable_failure_threshold
          }
          providerServiceTier={providerServiceTier}
          providerWireApi={providerWireApi}
          providerSupportsWebsockets={providerSupportsWebsockets}
          setProviderSupportsWebsockets={setProviderSupportsWebsockets}
          providerImageDetailOriginalToAuto={providerImageDetailOriginalToAuto}
          secretVisible={secretVisible}
          setProviderApiKey={(value) => {
            setProviderApiKey(value);
            setBalanceTestStatus(null);
            setBalanceAccountOptions([]);
            setProviderModels([]);
            setProviderTestModel("");
          }}
          setProviderApiKeyDirty={setProviderApiKeyDirty}
          setProviderBaseUrl={(value) => {
            setProviderBaseUrl(value);
            setBalanceTestStatus(null);
            setBalanceAccountOptions([]);
            setProviderModels([]);
            setProviderTestModel("");
          }}
          setAllowedModels={setAllowedModels}
          setModelMappings={setModelMappings}
          setProviderName={setProviderName}
          setProviderAutoDisableResetPeriod={setProviderAutoDisableResetPeriod}
          setProviderAutoDisableResetWeekday={
            setProviderAutoDisableResetWeekday
          }
          setProviderAutoDisableResetDay={setProviderAutoDisableResetDay}
          setProviderAutoDisableResetTime={setProviderAutoDisableResetTime}
          setProviderAutoDisableOverride={setProviderAutoDisableOverride}
          setProviderAutoDisableEnabled={setProviderAutoDisableEnabled}
          setProviderAutoDisableFailureThreshold={
            setProviderAutoDisableFailureThreshold
          }
          setProviderServiceTier={setProviderServiceTier}
          setProviderWireApi={setProviderWireApi}
          setProviderImageDetailOriginalToAuto={
            setProviderImageDetailOriginalToAuto
          }
          setSecretVisible={setSecretVisible}
          tab={editorTab}
        />
      )}
      {editorOpen && editorKind === "claude" && (
        <ClaudeProviderEditor
          error={error}
          modelListCleared={modelListCleared}
          onClearModels={() => {
            setAllowedModels([]);
            setProviderModels([]);
            setModelListCleared(true);
          }}
          fusionEnabled={fusionEnabled}
          setFusionEnabled={setFusionEnabled}
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
          providerAutoDisableResetPeriod={providerAutoDisableResetPeriod}
          providerAutoDisableResetWeekday={providerAutoDisableResetWeekday}
          providerAutoDisableResetDay={providerAutoDisableResetDay}
          providerAutoDisableResetTime={providerAutoDisableResetTime}
          providerAutoDisableOverride={providerAutoDisableOverride}
          providerAutoDisableEnabled={providerAutoDisableEnabled}
          providerAutoDisableFailureThreshold={
            providerAutoDisableFailureThreshold
          }
          globalAutoDisableEnabled={appState.router.auto_disable_enabled}
          globalAutoDisableFailureThreshold={
            appState.router.auto_disable_failure_threshold
          }
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
          setProviderAutoDisableResetPeriod={setProviderAutoDisableResetPeriod}
          setProviderAutoDisableResetWeekday={
            setProviderAutoDisableResetWeekday
          }
          setProviderAutoDisableResetDay={setProviderAutoDisableResetDay}
          setProviderAutoDisableResetTime={setProviderAutoDisableResetTime}
          setProviderAutoDisableOverride={setProviderAutoDisableOverride}
          setProviderAutoDisableEnabled={setProviderAutoDisableEnabled}
          setProviderAutoDisableFailureThreshold={
            setProviderAutoDisableFailureThreshold
          }
          setSecretVisible={setSecretVisible}
        />
      )}
    </main>
  );
}

function LocalProxySettings({
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
  const timeoutError = routerTimeoutError(routerDraft);
  const slowModeError = routerSlowModeError(routerDraft);
  const autoDisableError = autoDisableThresholdError(
    routerDraft.auto_disable_enabled,
    routerDraft.auto_disable_failure_threshold,
  );
  const endpointError =
    routerDraft.enabled && !routerDraft.host.trim()
      ? "监听地址不能为空。"
      : routerDraft.enabled &&
          (!Number.isInteger(routerDraft.port) ||
            routerDraft.port < 1 ||
            routerDraft.port > 65_535)
        ? "监听端口必须是 1 到 65535 之间的整数。"
        : "";
  const proxySettingsValid =
    !endpointError && !timeoutError && !slowModeError && !autoDisableError;
  const updateTimeout = (
    field:
      | "connect_timeout_secs"
      | "response_header_timeout_secs"
      | "stream_idle_timeout_secs",
    rawValue: string,
  ) => {
    const value = Number(rawValue);
    setRouterDraft({
      ...routerDraft,
      [field]: Number.isInteger(value) && value >= 0 ? value : 0,
    });
  };
  const updateSlowDelay = (
    field: "slow_delay_secs" | "slow_delay_max_secs",
    rawValue: string,
  ) => {
    const value = Number(rawValue);
    setRouterDraft({
      ...routerDraft,
      [field]: Number.isInteger(value) && value >= 0 ? value : 0,
    });
  };
  const selectSlowMode = (slow_mode: SlowMode) => {
    const slow_delay_secs = routerDraft.slow_delay_secs || 5;
    setRouterDraft({
      ...routerDraft,
      slow_mode,
      slow_delay_secs,
      slow_delay_max_secs:
        slow_mode === "random"
          ? Math.max(routerDraft.slow_delay_max_secs || 10, slow_delay_secs)
          : routerDraft.slow_delay_max_secs,
    });
  };

  return (
    <Card className="page-panel local-proxy-settings">
      <div className="panel-head">
        <div>
          <h2>本地代理</h2>
          <p>配置网关监听、上游超时与请求延迟。</p>
        </div>
        <Badge className={`state-pill ${routerOn ? "ok" : "warn"}`}>
          <span />
          {routerOn ? "运行中" : "未运行"}
        </Badge>
      </div>

      <div className="route-form-grid">
        <label className="compact-field">
          <span>监听地址</span>
          <TextField
            aria-invalid={Boolean(endpointError && !routerDraft.host.trim())}
            value={routerDraft.host}
            onChange={(event) =>
              setRouterDraft({
                ...routerDraft,
                host: event.currentTarget.value,
              })
            }
          />
        </label>
        <label className="compact-field">
          <span>监听端口</span>
          <TextField
            aria-invalid={Boolean(endpointError && routerDraft.host.trim())}
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
      {endpointError ? (
        <small className="settings-error">{endpointError}</small>
      ) : null}

      <div className="route-timeout-settings">
        <div>
          <strong>上游超时</strong>
          <p>每个供应商独立计时，超时后继续尝试下一供应商。</p>
        </div>
        <div className="route-timeout-grid">
          <label className="compact-field">
            <span>连接超时（秒）</span>
            <TextField
              aria-invalid={
                routerDraft.connect_timeout_secs < 1 ||
                routerDraft.connect_timeout_secs >
                  ROUTER_TIMEOUT_LIMITS.connect_timeout_secs
              }
              max={ROUTER_TIMEOUT_LIMITS.connect_timeout_secs}
              min={1}
              step={1}
              type="number"
              value={routerDraft.connect_timeout_secs}
              onChange={(event) =>
                updateTimeout("connect_timeout_secs", event.currentTarget.value)
              }
            />
          </label>
          <label className="compact-field">
            <span>响应头超时（秒）</span>
            <TextField
              aria-invalid={
                routerDraft.response_header_timeout_secs < 1 ||
                routerDraft.response_header_timeout_secs >
                  ROUTER_TIMEOUT_LIMITS.response_header_timeout_secs ||
                routerDraft.response_header_timeout_secs <
                  routerDraft.connect_timeout_secs
              }
              max={ROUTER_TIMEOUT_LIMITS.response_header_timeout_secs}
              min={1}
              step={1}
              type="number"
              value={routerDraft.response_header_timeout_secs}
              onChange={(event) =>
                updateTimeout(
                  "response_header_timeout_secs",
                  event.currentTarget.value,
                )
              }
            />
          </label>
          <label className="compact-field">
            <span>流空闲超时（秒）</span>
            <TextField
              aria-invalid={
                routerDraft.stream_idle_timeout_secs < 1 ||
                routerDraft.stream_idle_timeout_secs >
                  ROUTER_TIMEOUT_LIMITS.stream_idle_timeout_secs ||
                routerDraft.stream_idle_timeout_secs <
                  routerDraft.response_header_timeout_secs
              }
              max={ROUTER_TIMEOUT_LIMITS.stream_idle_timeout_secs}
              min={1}
              step={1}
              type="number"
              value={routerDraft.stream_idle_timeout_secs}
              onChange={(event) =>
                updateTimeout(
                  "stream_idle_timeout_secs",
                  event.currentTarget.value,
                )
              }
            />
          </label>
        </div>
        {timeoutError ? <small>{timeoutError}</small> : null}
      </div>

      <div className="route-slow-settings">
        <div className="route-slow-heading">
          <div>
            <strong>Slow 模式</strong>
            <p>流式请求等待期间保持连接，延迟结束后再连接上游。</p>
          </div>
          <SegmentedControl
            className="slow-mode-control"
            label="Slow 模式"
            value={routerDraft.slow_mode}
            onChange={selectSlowMode}
            options={[
              { value: "off", label: "关闭" },
              { value: "fixed", label: "固定" },
              { value: "random", label: "随机" },
            ]}
          />
        </div>
        {routerDraft.slow_mode !== "off" ? (
          <div className="route-slow-grid">
            <label className="compact-field">
              <span>
                {routerDraft.slow_mode === "fixed"
                  ? "延迟（秒）"
                  : "最小延迟（秒）"}
              </span>
              <TextField
                aria-invalid={
                  routerDraft.slow_delay_secs < 1 ||
                  routerDraft.slow_delay_secs > MAX_SLOW_DELAY_SECS
                }
                max={MAX_SLOW_DELAY_SECS}
                min={1}
                step={1}
                type="number"
                value={routerDraft.slow_delay_secs}
                onChange={(event) =>
                  updateSlowDelay("slow_delay_secs", event.currentTarget.value)
                }
              />
            </label>
            {routerDraft.slow_mode === "random" ? (
              <label className="compact-field">
                <span>最大延迟（秒）</span>
                <TextField
                  aria-invalid={
                    routerDraft.slow_delay_max_secs < 1 ||
                    routerDraft.slow_delay_max_secs > MAX_SLOW_DELAY_SECS ||
                    routerDraft.slow_delay_max_secs <
                      routerDraft.slow_delay_secs
                  }
                  max={MAX_SLOW_DELAY_SECS}
                  min={1}
                  step={1}
                  type="number"
                  value={routerDraft.slow_delay_max_secs}
                  onChange={(event) =>
                    updateSlowDelay(
                      "slow_delay_max_secs",
                      event.currentTarget.value,
                    )
                  }
                />
              </label>
            ) : null}
          </div>
        ) : null}
        {slowModeError ? <small>{slowModeError}</small> : null}
      </div>

      <div className="route-toggle-line">
        <div>
          <strong>自动禁用供应商</strong>
          <p>
            连续失败达到次数后停用供应商，并按供应商的刷新周期自动恢复。供应商可单独覆盖此策略。
          </p>
        </div>
        <Toggle
          label="自动停用故障供应商"
          checked={routerDraft.auto_disable_enabled}
          disabled={busy}
          onChange={(auto_disable_enabled) =>
            setRouterDraft({ ...routerDraft, auto_disable_enabled })
          }
        />
      </div>
      {routerDraft.auto_disable_enabled ? (
        <label className="compact-field">
          <span>连续失败次数</span>
          <TextField
            aria-invalid={Boolean(autoDisableError)}
            max={MAX_AUTO_DISABLE_FAILURE_THRESHOLD}
            min={1}
            step={1}
            type="number"
            value={routerDraft.auto_disable_failure_threshold}
            onChange={(event) =>
              setRouterDraft({
                ...routerDraft,
                auto_disable_failure_threshold: Number(
                  event.currentTarget.value,
                ),
              })
            }
          />
          {autoDisableError ? <small>{autoDisableError}</small> : null}
        </label>
      ) : null}

      <div className="route-toggle-line debug-mode-toggle">
        <div>
          <strong>以未压缩 JSON 转发</strong>
          <p>
            始终本地解压读取模型。关闭时保留 zstd
            压缩传输，改写请求后重新压缩；开启时向上游发送未压缩 JSON。
          </p>
        </div>
        <Toggle
          label="以未压缩 JSON 转发"
          checked={routerDraft.zstd_decompression_enabled}
          disabled={busy}
          onChange={(zstd_decompression_enabled) =>
            setRouterDraft({ ...routerDraft, zstd_decompression_enabled })
          }
        />
      </div>
      <div className="route-toggle-line debug-mode-toggle">
        <div>
          <strong>Debug 模式（Responses API）</strong>
          <p>保存入站请求、实际上游请求和上游原始响应；认证信息会脱敏。</p>
        </div>
        <Toggle
          label="启用 Debug 模式"
          checked={routerDraft.debug_mode}
          disabled={busy}
          onChange={(debug_mode) =>
            setRouterDraft({ ...routerDraft, debug_mode })
          }
        />
      </div>
      <div className="route-toggle-line">
        <div>
          <strong>启动程序后自动运行代理</strong>
        </div>
        <Toggle
          label="启动程序后自动运行代理"
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
            setRouterDraft({
              ...routerDraft,
              host: checked ? "0.0.0.0" : "127.0.0.1",
            })
          }
        />
      </div>

      <div className="route-footer-actions">
        <Button
          className="ghost"
          onClick={() => setRouterDraft(appState.router)}
          type="button"
        >
          撤销修改
        </Button>
        <Button
          className="primary"
          disabled={busy || !proxySettingsValid}
          onClick={() => onSaveRouter(routerDraft, true)}
          type="button"
        >
          保存代理设置
        </Button>
      </div>
    </Card>
  );
}

function RouteScreen({
  appState,
  busy,
  onSaveClientConfig,
  onSaveMultiAgentEnabled,
  routerDraft,
  setRouterDraft,
}: {
  appState: AppState;
  busy: boolean;
  onSaveClientConfig: (
    kind: AgentClientKind,
    enabled: boolean,
  ) => Promise<void>;
  onSaveMultiAgentEnabled: (enabled: boolean) => Promise<void>;
  routerDraft: RouterConfig;
  setRouterDraft: (router: RouterConfig) => void;
}) {
  const [showDiff, setShowDiff] = useState(false);
  const providerNameError = routerModelProviderError(routerDraft);
  const selectedModelUnavailable = routerSelectedModelUnavailable(
    routerDraft,
    appState.codex_models,
  );
  const longContextWindowError = routerLongContextWindowError(routerDraft);
  const codexModelError = effectiveRouterCodexModelError(
    routerDraft,
    appState.codex_models,
    appState.clients.codex.enabled,
  );

  return (
    <section className="route-page">
      <div className="section-title">
        <h2>路由配置</h2>
      </div>

      {showDiff && (
        <DialogSurface
          title="客户端配置变更"
          onClose={() => setShowDiff(false)}
        >
          <header>
            <div>
              <h2>客户端配置变更</h2>
              <p>由 XXSwitch 管理的配置字段。</p>
            </div>
            <Button className="close" onClick={() => setShowDiff(false)}>
              ×
            </Button>
          </header>
          <div className="latency-dialog-body">
            {appState.diffs.length ? (
              appState.diffs.map((diff, index) => (
                <div className="config-diff" key={`${diff.path}-${index}`}>
                  <code>{diff.path}</code>
                  <span>{diff.action}</span>
                </div>
              ))
            ) : (
              <EmptyState
                title="暂无待应用变更"
                description="当前客户端配置已与网关设置保持一致。"
              />
            )}
          </div>
          <footer>
            <Button className="primary" onClick={() => setShowDiff(false)}>
              完成
            </Button>
          </footer>
        </DialogSurface>
      )}
      <div className="route-config-grid">
        <Card className="route-card route-codex-card">
          <div className="route-card-head">
            <h3>Codex 接管</h3>
            <Badge
              className={`state-pill ${appState.clients.codex.enabled ? "ok" : "warn"}`}
            >
              <span />
              {appState.clients.codex.enabled ? "已接管" : "未接管"}
            </Badge>
          </div>
          <div className="compact-field">
            <span>配置文件</span>
            <div className="copy-field">
              <strong>{appState.codex_config_path}</strong>
              <CopyButton
                label="复制 Codex 配置路径"
                text={appState.codex_config_path}
              />
            </div>
          </div>
          <div className="compact-field">
            <span>当前接管地址</span>
            <div className="copy-field accent">
              <strong>{routeBaseUrl(routerDraft)}</strong>
              <CopyButton
                label="复制 Codex 接管地址"
                text={routeBaseUrl(routerDraft)}
              />
            </div>
          </div>
          <label className="compact-field route-provider-field">
            <span>Provider 名称</span>
            <TextField
              aria-invalid={Boolean(providerNameError)}
              maxLength={64}
              spellCheck={false}
              value={routerDraft.model_provider}
              onChange={(event) =>
                setRouterDraft({
                  ...routerDraft,
                  model_provider: event.currentTarget.value,
                })
              }
            />
            {providerNameError ? <small>{providerNameError}</small> : null}
          </label>
          <label className="compact-field route-model-field">
            <span>Codex 默认模型</span>
            <Select
              aria-invalid={Boolean(codexModelError)}
              disabled={busy || appState.codex_models.length === 0}
              value={routerDraft.codex_model}
              onValueChange={(value) =>
                setRouterDraft({
                  ...routerDraft,
                  codex_model: value,
                })
              }
            >
              <SelectOption value="">
                {appState.codex_models.length ? "选择模型" : "暂无可路由模型"}
              </SelectOption>
              {selectedModelUnavailable ? (
                <SelectOption value={routerDraft.codex_model}>
                  {routerDraft.codex_model}（当前不可路由）
                </SelectOption>
              ) : null}
              {appState.codex_models.map((model) => (
                <SelectOption key={model} value={model}>
                  {fusionModelLabel(model, appState.providers)}
                </SelectOption>
              ))}
            </Select>
            {codexModelError ? <small>{codexModelError}</small> : null}
          </label>
          <div className="route-diff-row">
            <span>openai_base_url → 本地代理地址</span>
            <Button className="ghost small" onClick={() => setShowDiff(true)}>
              查看变更
            </Button>
            <Toggle
              checked={appState.clients.codex.enabled}
              disabled={
                busy ||
                (!appState.clients.codex.enabled &&
                  (!appState.codex_models.length || selectedModelUnavailable))
              }
              label="接管 Codex"
              onChange={(checked) => void onSaveClientConfig("codex", checked)}
            />
          </div>
          <div className="route-toggle-line">
            <div>
              <strong>启用 WebSocket</strong>
              <p>
                允许 Codex 使用 WS 连接本地路由。请为支持 WS
                的供应商打开对应开关；其他供应商自动兼容 HTTP。保存后重启
                Codex。
              </p>
            </div>
            <Toggle
              checked={routerDraft.websockets_enabled}
              disabled={busy}
              label="启用 Codex WebSocket"
              onChange={(websockets_enabled) =>
                setRouterDraft({ ...routerDraft, websockets_enabled })
              }
            />
          </div>
          <div className="route-toggle-line">
            <div>
              <strong>启用远程压缩</strong>
              <p>
                开启时将 model_providers.
                {routerDraft.model_provider || "custom"}.name 设为
                OpenAI；关闭时与 Provider 名称一致。
              </p>
            </div>
            <Toggle
              label="启用远程压缩"
              checked={routerDraft.remote_compaction_enabled}
              disabled={busy}
              onChange={(remote_compaction_enabled) =>
                setRouterDraft({ ...routerDraft, remote_compaction_enabled })
              }
            />
          </div>
          <div className="route-toggle-line">
            <div>
              <strong>实验性上下文接力</strong>
              <p>
                启用 Token Budget，使用 Notes 保存进度并在新窗口通过 History
                恢复。
              </p>
            </div>
            <Toggle
              checked={routerDraft.token_budget_enabled}
              disabled={busy}
              label="启用实验性上下文接力"
              onChange={(token_budget_enabled) =>
                setRouterDraft({ ...routerDraft, token_budget_enabled })
              }
            />
          </div>
          <div className="route-toggle-line long-context-setting">
            <div>
              <strong>GPT-5.6 及以上长上下文</strong>
              <p>
                按模型版本号识别 GPT-5.6 及以上模型，包含模型后缀和融合条目。
              </p>
            </div>
            <div className="long-context-controls">
              <label className="compact-field">
                <span>上下文大小（Token）</span>
                <TextField
                  aria-invalid={Boolean(longContextWindowError)}
                  disabled={busy || !routerDraft.gpt56_long_context_enabled}
                  inputMode="numeric"
                  min={1}
                  step={1000}
                  type="number"
                  value={routerDraft.gpt56_long_context_window}
                  onChange={(event) =>
                    setRouterDraft({
                      ...routerDraft,
                      gpt56_long_context_window: Number(
                        event.currentTarget.value,
                      ),
                    })
                  }
                />
                {longContextWindowError ? (
                  <small>{longContextWindowError}</small>
                ) : null}
              </label>
              <Toggle
                checked={routerDraft.gpt56_long_context_enabled}
                disabled={busy}
                label="启用 GPT-5.6 及以上长上下文"
                onChange={(gpt56_long_context_enabled) =>
                  setRouterDraft({ ...routerDraft, gpt56_long_context_enabled })
                }
              />
            </div>
          </div>

          <div className="route-toggle-line">
            <div>
              <strong>强制不使用 OpenAI 登录态</strong>
              <p>
                开启时 requires_openai_auth 固定为 false；关闭时只检测是否配置
                ChatGPT 登录态，不校验过期。
              </p>
            </div>
            <Toggle
              label="强制不使用 OpenAI 登录态"
              checked={routerDraft.force_disable_openai_auth}
              disabled={busy}
              onChange={(force_disable_openai_auth) =>
                setRouterDraft({ ...routerDraft, force_disable_openai_auth })
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
        </Card>

        <Card className="route-card">
          <div className="route-card-head">
            <h3>Claude 接管</h3>
            <Badge
              className={`state-pill ${appState.clients.claude.enabled ? "ok" : "warn"}`}
            >
              <span />
              {appState.clients.claude.enabled ? "已接管" : "未接管"}
            </Badge>
          </div>
          <div className="compact-field">
            <span>设置文件</span>
            <div className="copy-field">
              <strong>{appState.claude_settings_path}</strong>
              <CopyButton
                label="复制 Claude 配置路径"
                text={appState.claude_settings_path}
              />
            </div>
          </div>
          <div className="compact-field">
            <span>Claude Base URL</span>
            <div className="copy-field accent">
              <strong>{claudeBaseUrl(routerDraft)}</strong>
              <CopyButton
                label="复制 Claude 接管地址"
                text={claudeBaseUrl(routerDraft)}
              />
            </div>
          </div>
          <div className="route-diff-row">
            <span>ANTHROPIC_BASE_URL → 本地代理地址</span>
            <Button className="ghost small" onClick={() => setShowDiff(true)}>
              查看变更
            </Button>
            <Toggle
              checked={appState.clients.claude.enabled}
              disabled={busy}
              label="接管 Claude Code"
              onChange={(checked) => void onSaveClientConfig("claude", checked)}
            />
          </div>
        </Card>

        <Card className="route-card">
          <div className="route-card-head">
            <h3>Pi 接管</h3>
            <Badge
              className={`state-pill ${appState.clients.pi.enabled ? "ok" : "warn"}`}
            >
              <span />
              {appState.clients.pi.enabled ? "已接管" : "未接管"}
            </Badge>
          </div>
          <div className="compact-field">
            <span>模型配置</span>
            <div className="copy-field">
              <strong>{appState.pi_models_path}</strong>
              <CopyButton
                label="复制 Pi 配置路径"
                text={appState.pi_models_path}
              />
            </div>
          </div>
          <div className="compact-field">
            <span>Responses Base URL</span>
            <div className="copy-field accent">
              <strong>{routeBaseUrl(routerDraft)}</strong>
              <CopyButton
                label="复制 Pi 接管地址"
                text={routeBaseUrl(routerDraft)}
              />
            </div>
          </div>
          <div className="route-diff-row">
            <span>models.json → xxswitch provider</span>
            <Badge className="skill-pill">自动同步</Badge>
            <Toggle
              checked={appState.clients.pi.enabled}
              disabled={busy}
              label="接管 Pi"
              onChange={(checked) => void onSaveClientConfig("pi", checked)}
            />
          </div>
        </Card>
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
  const [managedLocation, setManagedLocation] = useState(
    client.managed_skill_location,
  );

  useEffect(() => {
    setLocationsText(
      client.skill_locations.map((location) => location.path).join("\n"),
    );
    setManagedLocation(client.managed_skill_location);
  }, [client.client, client.managed_skill_location, client.skill_locations]);

  const parsedLocations = locationsText
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((path) => ({ path, writable: true }));

  return (
    <Card className="skill-card">
      <div className="card-head">
        <div>
          <h3>{client.label}</h3>
          <p>
            扫描 Skill Location 的直接子目录；新 exposure 只写入 Managed Skill
            Location。
          </p>
        </div>
        <Button
          className="ghost"
          disabled={busy}
          onClick={() =>
            void onSave(client.client, parsedLocations, managedLocation)
          }
          type="button"
        >
          保存路径
        </Button>
      </div>
      <label className="field skill-field">
        <span>扫描路径（每行一个）</span>
        <TextArea
          onChange={(event) => setLocationsText(event.currentTarget.value)}
          rows={4}
          value={locationsText}
        />
      </label>
      <label className="field skill-field">
        <span>共享写入路径</span>
        <TextField
          onChange={(event) => setManagedLocation(event.currentTarget.value)}
          value={managedLocation}
        />
      </label>
      <div className="skill-location-list">
        {client.skill_locations.map((location) => (
          <Badge
            className={location.exists ? "skill-pill ok" : "skill-pill warn"}
            key={location.path}
          >
            {location.managed ? "Managed · " : "Scan · "}
            {location.exists ? "存在" : "缺失"}
          </Badge>
        ))}
      </div>
    </Card>
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
  onSetSharingScope: (
    skill: SharedSkillView,
    scope: AgentClientKind[],
  ) => Promise<void>;
  onUseExistingSharedSkill: (skill: ClientSkillView) => Promise<void>;
  view: SkillManagementView | null;
}) {
  const clients: AgentClientKind[] = ["codex", "claude", "pi"];

  function nextScope(
    skill: SharedSkillView,
    client: AgentClientKind,
    checked: boolean,
  ) {
    const current = new Set(skill.sharing_scope);
    if (checked) current.add(client);
    else current.delete(client);
    return clients.filter((item) => current.has(item));
  }

  if (!view) {
    return (
      <section className="skills-page">
        <Card className="page-panel empty-state">
          <h2>Skill 管理</h2>
          <p>
            扫描本机已知 Agent Client 的 Skill Locations，发现可共享的 Skill。
          </p>
          <Button
            className="primary"
            disabled={busy}
            onClick={() => void onRefresh()}
            type="button"
          >
            扫描 Skills
          </Button>
        </Card>
      </section>
    );
  }

  return (
    <section className="skills-page">
      <div className="section-title">
        <div>
          <h2>Skills 工作区</h2>
          <p className="muted">共享库 · {view.library_root}</p>
        </div>
        <Button
          className="primary"
          disabled={busy}
          onClick={() => void onRefresh()}
          type="button"
        >
          重新扫描
        </Button>
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
        <Card className="page-panel skill-section">
          <div className="panel-head">
            <div>
              <h2>冲突</h2>
              <p>同名冲突不会自动覆盖、合并或重命名。</p>
            </div>
          </div>
          <div className="skill-list">
            {view.conflicts.map((conflict) => (
              <div
                className="skill-row conflict"
                key={`${conflict.kind}-${conflict.path}`}
              >
                <div>
                  <strong>{conflict.identity}</strong>
                  <small>
                    {conflict.client
                      ? agentClientLabel(conflict.client)
                      : "Library"}{" "}
                    · {conflict.path}
                  </small>
                </div>
                <Badge className="skill-pill warn">{conflict.kind}</Badge>
                <p>{conflict.message}</p>
              </div>
            ))}
          </div>
        </Card>
      )}

      <Card className="page-panel skill-section">
        <div className="panel-head">
          <div>
            <h2>共享 Skills</h2>
            <p>选择可使用这项 Skill 的客户端，更改后立即生效。</p>
          </div>
        </div>
        <div className="skill-list">
          {view.shared_skills.length === 0 && (
            <EmptyState
              title="暂无共享 Skill"
              description="将客户端中的 Skill 设为共享，即可在这里管理。"
            />
          )}
          {view.shared_skills.map((skill) => (
            <div className="shared-skill" key={skill.identity}>
              <div className="shared-skill-main">
                <div>
                  <strong>{skill.identity}</strong>
                  <small>{skill.description || skill.path}</small>
                  {skill.origin && (
                    <small>
                      来源：{agentClientLabel(skill.origin.client)} ·{" "}
                      {skill.origin.original_path}
                    </small>
                  )}
                </div>
                <Button
                  className="ghost danger"
                  disabled={busy}
                  onClick={() => void onDeleteSharedSkill(skill)}
                  type="button"
                >
                  删除
                </Button>
              </div>
              <div className="scope-row">
                {clients.map((client) => (
                  <label key={client}>
                    <Checkbox
                      checked={skill.sharing_scope.includes(client)}
                      disabled={busy}
                      onCheckedChange={(checked) =>
                        void onSetSharingScope(
                          skill,
                          nextScope(skill, client, checked),
                        )
                      }
                    />
                    {agentClientLabel(client)}
                  </label>
                ))}
              </div>
              <div className="exposure-list">
                {skill.exposures.map((exposure) => (
                  <div
                    className="exposure-row"
                    key={`${skill.identity}-${exposure.client}-${exposure.path}`}
                  >
                    <Badge
                      className={`skill-pill ${exposure.health === "healthy" ? "ok" : "warn"}`}
                    >
                      {exposure.client_label} · {exposure.health}
                    </Badge>
                    <code>{exposure.path || "未创建"}</code>
                    <small>{exposure.message}</small>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </Card>

      <Card className="page-panel skill-section">
        <div className="panel-head">
          <div>
            <h2>客户端 Skills</h2>
            <p>从各客户端发现的 Skills，设为共享后可供其他客户端使用。</p>
          </div>
        </div>
        <div className="skill-list">
          {view.client_skills.length === 0 && (
            <EmptyState
              title="暂无客户端 Skill"
              description="检查上方的扫描路径，或重新扫描客户端目录。"
            />
          )}
          {view.client_skills.map((skill) => (
            <div className="skill-row" key={`${skill.client}-${skill.path}`}>
              <div>
                <strong>{skill.identity}</strong>
                <small>
                  {skill.client_label} · {skill.path}
                </small>
                {skill.description && <small>{skill.description}</small>}
              </div>
              {skill.shared ? (
                <Button
                  className="ghost"
                  disabled={busy}
                  onClick={() => void onUseExistingSharedSkill(skill)}
                  type="button"
                >
                  使用已有共享 Skill
                </Button>
              ) : (
                <Button
                  className="primary"
                  disabled={busy}
                  onClick={() => void onPromote(skill)}
                  type="button"
                >
                  公用
                </Button>
              )}
            </div>
          ))}
        </div>
      </Card>
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
  const [distribution, setDistribution] = useState<"models" | "providers">(
    "models",
  );
  const distributionRows = stats?.[distribution] ?? [];
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
  const totalCalls =
    distributionRows.reduce((total, row) => total + row.request_count, 0) || 1;
  const availableProviders = usageProviderOptions(
    stats?.available_providers ?? [],
    configuredProviders,
  );

  return (
    <section className="usage-page">
      <div className="section-title">
        <h2>统计概览</h2>
        <Button className="ghost" onClick={onRefresh} type="button">
          重新统计
        </Button>
      </div>

      <div className="usage-filter-bar">
        <SegmentedControl
          className="range-tabs"
          label="统计时间范围"
          value={timeRange}
          onChange={onRangeChange}
          options={(["today", "week", "month", "all"] as TimeRange[]).map(
            (range) => ({ value: range, label: rangeLabel(range) }),
          )}
        />
        <Select
          aria-label="统计供应商"
          value={filter.provider_id ?? ""}
          onValueChange={(value) => onFilter({ provider_id: value })}
        >
          <SelectOption value="">全部供应商</SelectOption>
          {availableProviders.map((provider) => (
            <SelectOption key={provider.id} value={provider.id}>
              {provider.name}
            </SelectOption>
          ))}
        </Select>
        <Select
          aria-label="统计模型"
          value={filter.model ?? ""}
          onValueChange={(value) => onFilter({ model: value })}
        >
          <SelectOption value="">全部模型</SelectOption>
          {(stats?.available_models ?? []).map((model) => (
            <SelectOption key={model} value={model}>
              {model}
            </SelectOption>
          ))}
        </Select>
        <span className="filter-hint">
          {bucketGranularityLabel(stats?.bucket_granularity)}
        </span>
        <small>本程序路由日志</small>
      </div>

      <div className="metric-grid">
        <Metric
          title="调用次数"
          value={String(stats?.success_count ?? summary.request_count)}
          tone="green"
        />
        <Metric
          title="总 Token"
          value={formatCompact(summary.total_tokens)}
          tone="purple"
          sub={rangeLabel(timeRange)}
        />
        <Metric
          title="非缓存输入"
          value={formatCompact(summary.uncached_input_tokens)}
          tone="cyan"
          sub={`${formatCompact(summary.input_tokens)} 输入`}
        />
        <Metric
          title="输出 Token"
          value={formatCompact(summary.output_tokens)}
          tone="amber"
          sub="路由响应"
        />
      </div>

      <div className="usage-main-grid">
        <Card className="card route-trend-card">
          <div className="card-head">
            <div>
              <h3>使用趋势</h3>
              <p>
                {rangeLabel(timeRange)}{" "}
                {bucketGranularityLabel(stats?.bucket_granularity)}的
                {trendMetricLabel(trendMetric)}
              </p>
            </div>
            <SegmentedControl
              className="mini-tabs"
              label="趋势指标"
              value={trendMetric}
              onChange={onTrendMetricChange}
              options={(["tokens", "requests"] as TrendMetric[]).map(
                (metric) => ({
                  value: metric,
                  label: trendMetricLabel(metric),
                }),
              )}
            />
          </div>
          <TrendLineChart
            buckets={trendBuckets}
            granularity={stats?.bucket_granularity}
            metric={trendMetric}
            range={timeRange}
          />
        </Card>

        <Card className="card call-distribution">
          <div className="card-head">
            <div>
              <h3>调用分布</h3>
              <p>
                {distribution === "models" ? "按模型" : "按供应商"}统计成功调用
              </p>
            </div>
            <SegmentedControl
              className="mini-tabs"
              label="调用分布维度"
              value={distribution}
              onChange={setDistribution}
              options={[
                { value: "models", label: "按模型" },
                { value: "providers", label: "按供应商" },
              ]}
            />
          </div>
          {distributionRows.slice(0, 4).map((row) => (
            <div className="distribution-row" key={row.key}>
              <div>
                <UsageName
                  name={row.label}
                  label={
                    distribution === "models"
                      ? chartModelLabel(row.label, configuredProviders)
                      : row.label
                  }
                />
                <b>{row.request_count} 次</b>
              </div>
              <span>
                <i
                  style={{
                    width: `${Math.max(3, (row.request_count / totalCalls) * 100)}%`,
                  }}
                />
              </span>
              <small>
                {Math.round((row.request_count / totalCalls) * 100)}%
              </small>
            </div>
          ))}
          {distributionRows.length === 0 && (
            <EmptyState
              title="暂无调用分布"
              description="当前筛选范围内还没有成功调用。"
            />
          )}
        </Card>
      </div>

      <Card className="route-table-card usage-detail-card">
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
              <strong>{requestModelLabel(log.model)}</strong>
              <span>{log.provider_name}</span>
              <span>{formatTokenCount(log.uncached_input_tokens)}</span>
              <span>{formatTokenCount(log.cached_input_tokens)}</span>
              <span>{formatTokenCount(log.output_tokens)}</span>
              <b>{log.status === "success" ? 1 : 0}</b>
              <b>{formatTokenCount(log.total_tokens)}</b>
            </div>
          ))}
        </div>
        {!stats?.details.length && (
          <EmptyState
            title="暂无用量明细"
            description="经过网关的调用会显示在这里。"
          />
        )}
      </Card>
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
  const [selectedDebugLogs, setSelectedDebugLogs] = useState<
    Record<string, RouteRequestLog>
  >({});
  const [debugExporting, setDebugExporting] = useState(false);
  const [debugExportStatus, setDebugExportStatus] = useState("");
  const selectableRows = rows.filter((log) => log.debug_capture_available);
  const selectedDebugCount = Object.keys(selectedDebugLogs).length;
  const selectedPageDebugCount = selectableRows.filter((log) =>
    Boolean(selectedDebugLogs[log.id]),
  ).length;
  const allPageDebugSelected =
    selectableRows.length > 0 &&
    selectableRows.every((log) => Boolean(selectedDebugLogs[log.id]));

  const toggleDebugSelection = (log: RouteRequestLog, checked: boolean) => {
    setSelectedDebugLogs((current) => {
      const next = { ...current };
      if (checked) next[log.id] = log;
      else delete next[log.id];
      return next;
    });
    setDebugExportStatus("");
  };

  const toggleCurrentPageDebugSelection = (checked: boolean) => {
    setSelectedDebugLogs((current) => {
      const next = { ...current };
      selectableRows.forEach((log) => {
        if (checked) next[log.id] = log;
        else delete next[log.id];
      });
      return next;
    });
    setDebugExportStatus("");
  };

  const exportSelectedDebugLogs = async () => {
    const selected = Object.values(selectedDebugLogs);
    if (selected.length === 0 || debugExporting) return;
    setDebugExporting(true);
    setDebugExportStatus("");
    try {
      const failed = await exportRouteDebugBundle(selected);
      setDebugExportStatus(
        failed > 0
          ? `已导出 ${selected.length - failed} 条，${failed} 条读取失败`
          : `已导出 ${selected.length} 条 Debug 快照`,
      );
    } catch (err) {
      setDebugExportStatus(`导出失败：${String(err)}`);
    } finally {
      setDebugExporting(false);
    }
  };

  return (
    <section className="requests-page">
      <div className="section-title">
        <h2>请求列表</h2>
        <div className="request-export-actions">
          {debugExportStatus ? <small>{debugExportStatus}</small> : null}
          <Button
            className="ghost"
            onClick={() => exportRouteLogsCsv(rows)}
            type="button"
          >
            导出当前页 CSV
          </Button>
          {selectedDebugCount > 0 ? (
            <Button
              className="ghost"
              disabled={debugExporting}
              onClick={() => {
                setSelectedDebugLogs({});
                setDebugExportStatus("");
              }}
              type="button"
            >
              清空选择
            </Button>
          ) : null}
          <Button
            className="ghost"
            disabled={selectedDebugCount === 0 || debugExporting}
            onClick={() => void exportSelectedDebugLogs()}
            type="button"
          >
            {debugExporting
              ? "正在导出…"
              : `导出 Debug JSON (${selectedDebugCount})`}
          </Button>
        </div>
      </div>
      <div className="request-filter-bar">
        <div className="search-box">
          <Search size={15} aria-hidden="true" />
          <TextField
            aria-label="搜索请求"
            placeholder="搜索请求 ID 或错误信息"
            value={filter.query ?? ""}
            onChange={(event) => onFilter({ query: event.currentTarget.value })}
          />
        </div>
        <Select
          aria-label="请求状态"
          value={filter.status ?? ""}
          onValueChange={(value) => onFilter({ status: value })}
        >
          <SelectOption value="">全部状态</SelectOption>
          <SelectOption value="success">成功</SelectOption>
          <SelectOption value="failed">失败</SelectOption>
          <SelectOption value="cancelled">已取消</SelectOption>
        </Select>
        <Select
          aria-label="请求供应商"
          value={filter.provider_id ?? ""}
          onValueChange={(value) => onFilter({ provider_id: value })}
        >
          <SelectOption value="">全部供应商</SelectOption>
          {(logs?.available_providers ?? []).map((provider) => (
            <SelectOption key={provider.id} value={provider.id}>
              {provider.name}
            </SelectOption>
          ))}
        </Select>
        <Select
          aria-label="请求模型"
          value={filter.model ?? ""}
          onValueChange={(value) => onFilter({ model: value })}
        >
          <SelectOption value="">全部模型</SelectOption>
          {(logs?.available_models ?? []).map((model) => (
            <SelectOption key={model} value={model}>
              {model}
            </SelectOption>
          ))}
        </Select>
        <Button className="ghost" onClick={onToday} type="button">
          今日
        </Button>
        <span>实时刷新</span>
        <Toggle
          label="实时刷新请求记录"
          checked={autoRefresh}
          onChange={(checked) => {
            onSetAutoRefresh(checked);
            if (checked) onRefresh();
          }}
        />
        <small>共 {logs?.total ?? 0} 条</small>
      </div>

      <Card className="route-table-card request-log-card">
        <div className="request-log-table">
          <header>
            <Tooltip content="全选当前页 Debug 快照">
              <label className="request-debug-checkbox">
                <Checkbox
                  aria-label="全选当前页 Debug 快照"
                  checked={
                    selectedPageDebugCount > 0 && !allPageDebugSelected
                      ? "indeterminate"
                      : allPageDebugSelected
                  }
                  disabled={selectableRows.length === 0}
                  onCheckedChange={(checked) =>
                    toggleCurrentPageDebugSelection(checked)
                  }
                />
              </label>
            </Tooltip>
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
                ]
                  .filter(Boolean)
                  .join(" ")}
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
                <Tooltip
                  content={
                    log.debug_capture_available
                      ? "选择 Debug 快照"
                      : "该请求没有 Debug 快照"
                  }
                >
                  <div
                    className="request-debug-checkbox"
                    onClick={(event) => event.stopPropagation()}
                    onKeyDown={(event) => event.stopPropagation()}
                  >
                    <Checkbox
                      aria-label={`选择请求 ${log.id} 的 Debug 快照`}
                      checked={Boolean(selectedDebugLogs[log.id])}
                      disabled={!log.debug_capture_available}
                      onCheckedChange={(checked) =>
                        toggleDebugSelection(log, checked)
                      }
                    />
                  </div>
                </Tooltip>
                <div className="status-cell">
                  <span className={`dot ${status.tone}`} />
                  <b className={`${status.tone}-text`}>{status.label}</b>
                </div>
                <strong>{formatLogTime(log.started_at_ms)}</strong>
                <strong>{requestModelLabel(log.model)}</strong>
                <strong>{log.provider_name}</strong>
                <span>{formatTokenTriplet(log)}</span>
                <b>{formatMs(log.first_byte_ms)}</b>
                <Tooltip content={formatTimingDetails(log)}>
                  <b>{formatDuration(log.total_ms)}</b>
                </Tooltip>
                <span
                  className={`route-result ${routeResultTone(log.route_result)}`}
                >
                  {log.route_result}
                </span>
              </div>
            );
          })}
        </div>
        {rows.length === 0 && (
          <EmptyState
            title="暂无匹配的请求"
            description="调整筛选条件，或等待客户端通过网关发起调用。"
          />
        )}
        <footer className="table-pagination">
          <span>
            {selectedDebugCount > 0
              ? `已选择 ${selectedDebugCount} 条 Debug 快照`
              : "点击任意记录查看路由时间线、Token 明细和错误详情。"}
          </span>
          <div>
            <Button
              aria-label="上一页"
              className="ghost small"
              disabled={(logs?.page ?? 1) <= 1}
              onClick={() => onFilter({ page: (logs?.page ?? 1) - 1 })}
            >
              ‹
            </Button>
            <b>{logs?.page ?? 1}</b>
            <Button
              aria-label="下一页"
              className="ghost small"
              disabled={(logs?.page ?? 1) >= (logs?.total_pages ?? 1)}
              onClick={() => onFilter({ page: (logs?.page ?? 1) + 1 })}
            >
              ›
            </Button>
          </div>
        </footer>
      </Card>
      {selectedLog ? (
        <RequestLogDialog
          log={selectedLog}
          onClose={() => setSelectedLog(null)}
        />
      ) : null}
    </section>
  );
}

function RequestLogDialog({
  log,
  onClose,
}: {
  log: RouteRequestLog;
  onClose: () => void;
}) {
  const status = statusMeta(log.status);
  const tierResult = serviceTierResult(log);
  const phases = routeTimingPhases(log);
  const compactionAudit = remoteCompactionV2AuditLabel(log);
  const [debugCapture, setDebugCapture] = useState<RouteDebugCapture | null>(
    null,
  );
  const [debugError, setDebugError] = useState(log.debug_capture_error || "");
  const [debugLoading, setDebugLoading] = useState(log.debug_capture_available);
  const [debugView, setDebugView] = useState<
    "inbound" | "upstream" | "response"
  >("inbound");

  useEffect(() => {
    let cancelled = false;
    if (!log.debug_capture_available) {
      setDebugLoading(false);
      return () => {
        cancelled = true;
      };
    }
    setDebugLoading(true);
    callCommand<RouteDebugCapture>("load_route_debug_capture", {
      payload: { request_id: log.id },
    })
      .then((capture) => {
        if (!cancelled) setDebugCapture(capture);
      })
      .catch((err) => {
        if (!cancelled) setDebugError(String(err));
      })
      .finally(() => {
        if (!cancelled) setDebugLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [log.debug_capture_available, log.id]);

  const activeDebug = debugCapture
    ? debugView === "inbound"
      ? debugCapture.inbound_request
      : debugView === "upstream"
        ? debugCapture.upstream_request
        : debugCapture.upstream_response
    : null;
  const activeDebugBodyText = useMemo(
    () => formatDebugBodyText(activeDebug?.body.text || ""),
    [activeDebug],
  );
  const requestBody = log.request_body || debugCapture?.request_body || null;

  return (
    <DialogSurface
      title="请求详情"
      className="request-log-dialog"
      onClose={onClose}
    >
      <header>
        <div className="request-log-dialog-heading">
          <div>
            <h2 id="request-log-dialog-title">请求详情</h2>
            <Tooltip content={log.id}>
              <p>{log.id}</p>
            </Tooltip>
          </div>
          <span className={`request-detail-status ${status.tone}`}>
            <span className={`dot ${status.tone}`} />
            {status.label}
          </span>
        </div>
        <Button
          aria-label="关闭"
          className="close"
          onClick={onClose}
          title="关闭"
          type="button"
        >
          ×
        </Button>
      </header>

      <div className="latency-dialog-body request-log-dialog-body">
        <section className="request-detail-section">
          <header>
            <h3>请求概况</h3>
            <span>{formatLogDateTime(log.started_at_ms)}</span>
          </header>
          <div className="request-detail-grid">
            <div>
              <span>请求</span>
              <strong>
                {log.method} {log.path}
              </strong>
            </div>
            <div>
              <span>状态码</span>
              <strong>{log.status_code ?? "-"}</strong>
            </div>
            <div>
              <span>模型</span>
              <strong>{requestModelLabel(log.model) || "-"}</strong>
            </div>
            <div>
              <span>上游模型</span>
              <strong>
                {requestModelLabel(log.upstream_model || log.model) || "-"}
              </strong>
            </div>
            <div>
              <span>供应商</span>
              <strong>{log.provider_name}</strong>
            </div>
            <div>
              <span>路由尝试</span>
              <strong>{log.route_attempts} 次</strong>
            </div>
            <div>
              <span>请求服务层级</span>
              <strong>
                {serviceTierLabel(log.requested_service_tier, "未强制")}
              </strong>
            </div>
            <div>
              <span>实际服务层级</span>
              <strong>{serviceTierLabel(log.actual_service_tier)}</strong>
            </div>
            <div>
              <span>服务层级验证</span>
              <strong className={`service-tier-result ${tierResult.tone}`}>
                {tierResult.label}
              </strong>
            </div>
            {log.slow_delay_ms != null ? (
              <div>
                <span>Slow 延迟</span>
                <strong>{formatDuration(log.slow_delay_ms)}</strong>
              </div>
            ) : null}
          </div>
        </section>

        {requestBody ? (
          <section className="request-detail-section">
            <header>
              <h3>请求体处理</h3>
            </header>
            <div className="request-detail-grid">
              <div>
                <span>Content-Type</span>
                <strong>{requestBody.content_type || "未设置"}</strong>
              </div>
              <div>
                <span>Content-Encoding</span>
                <strong>{requestBody.content_encoding || "未设置"}</strong>
              </div>
              <div>
                <span>本地解析</span>
                <strong>
                  {requestBody.decompression_enabled ? "开启" : "关闭"}
                </strong>
              </div>
              <div>
                <span>zstd 识别</span>
                <strong>{requestBody.zstd_detected ? "是" : "否"}</strong>
              </div>
              <div>
                <span>处理结果</span>
                <strong>
                  {!requestBody.decompression_enabled
                    ? "未处理"
                    : requestBody.decompressed
                      ? "已解压"
                      : "无需解压"}
                </strong>
              </div>
              <div>
                <span>入站原始</span>
                <strong>
                  {requestBody.inbound_bytes.toLocaleString()} bytes
                </strong>
              </div>
              <div>
                <span>解压后</span>
                <strong>
                  {requestBody.decoded_bytes.toLocaleString()} bytes
                </strong>
              </div>
              <div>
                <span>上游转发</span>
                <strong>
                  {requestBody.upstream_bytes.toLocaleString()} bytes
                </strong>
              </div>
            </div>
          </section>
        ) : null}

        <section className="request-detail-section">
          <header>
            <h3>耗时阶段</h3>
            <strong>{formatDuration(log.total_ms)}</strong>
          </header>
          {phases.length > 0 ? (
            <div className="request-timing-list">
              {phases.map((phase) => {
                const width =
                  log.total_ms > 0
                    ? Math.max(
                        phase.durationMs > 0 ? 2 : 0,
                        (phase.durationMs / log.total_ms) * 100,
                      )
                    : 0;
                return (
                  <div className="request-timing-row" key={phase.label}>
                    <span>{phase.label}</span>
                    <div>
                      <i style={{ width: `${Math.min(100, width)}%` }} />
                    </div>
                    <strong>{formatDuration(phase.durationMs)}</strong>
                  </div>
                );
              })}
            </div>
          ) : (
            <p className="request-detail-empty">
              该记录创建时尚未采集分段耗时。
            </p>
          )}
        </section>

        <section className="request-detail-section">
          <header>
            <h3>Token 明细</h3>
          </header>
          <div className="request-token-grid">
            <div>
              <span>输入</span>
              <strong>{formatTokenCount(log.input_tokens)}</strong>
            </div>
            <div>
              <span>非缓存</span>
              <strong>{formatTokenCount(log.uncached_input_tokens)}</strong>
            </div>
            <div>
              <span>缓存</span>
              <strong>{formatTokenCount(log.cached_input_tokens)}</strong>
            </div>
            <div>
              <span>输出</span>
              <strong>{formatTokenCount(log.output_tokens)}</strong>
            </div>
            <div>
              <span>推理输出</span>
              <strong>{formatTokenCount(log.reasoning_output_tokens)}</strong>
            </div>
            <div>
              <span>总计</span>
              <strong>{formatTokenCount(log.total_tokens)}</strong>
            </div>
          </div>
        </section>

        <section className="request-detail-section">
          <header>
            <h3>路由详情</h3>
          </header>
          <dl className="request-route-details">
            <div>
              <dt>路由结果</dt>
              <dd>{log.route_result}</dd>
            </div>
            <div>
              <dt>上游链路</dt>
              <dd>{log.upstream_chain.join(" → ") || "-"}</dd>
            </div>
            <div>
              <dt>本地请求 ID</dt>
              <dd>
                <code>{log.id}</code>
              </dd>
            </div>
            <div>
              <dt>上游请求 ID</dt>
              <dd>
                <code>{log.upstream_request_id || "未返回"}</code>
              </dd>
            </div>
            {compactionAudit ? (
              <div>
                <dt>远程压缩</dt>
                <dd>{compactionAudit}</dd>
              </div>
            ) : null}
          </dl>
        </section>

        {debugLoading || debugCapture || debugError ? (
          <section className="request-detail-section request-debug-section">
            <header>
              <h3>Debug 快照</h3>
              {debugCapture ? <span>{debugCapture.provider_name}</span> : null}
            </header>
            {debugLoading ? (
              <p className="request-detail-empty">正在读取快照…</p>
            ) : null}
            {debugError ? (
              <pre className="request-debug-error">{debugError}</pre>
            ) : null}
            {debugCapture && activeDebug ? (
              <Tabs.Root
                value={debugView}
                onValueChange={(value) =>
                  setDebugView(value as typeof debugView)
                }
              >
                <Tabs.List
                  className="request-debug-tabs"
                  aria-label="Debug 快照视图"
                >
                  {(
                    [
                      ["inbound", "入站请求"],
                      ["upstream", "上游请求"],
                      ["response", "上游响应"],
                    ] as const
                  ).map(([value, label]) => (
                    <Tabs.Trigger key={value} value={value}>
                      {label}
                    </Tabs.Trigger>
                  ))}
                </Tabs.List>
                <Tabs.Content value={debugView}>
                  <div className="request-debug-meta">
                    {"method" in activeDebug ? (
                      <code>
                        {activeDebug.method} {activeDebug.url}
                      </code>
                    ) : (
                      <code>HTTP {activeDebug.status_code ?? "-"}</code>
                    )}
                    {activeDebug.body.truncated ? (
                      <span>
                        已截断 ·{" "}
                        {activeDebug.body.captured_bytes.toLocaleString()} /{" "}
                        {activeDebug.body.total_bytes.toLocaleString()} bytes
                      </span>
                    ) : (
                      <span>
                        {activeDebug.body.total_bytes.toLocaleString()} bytes
                      </span>
                    )}
                  </div>
                  <div className="request-debug-block">
                    <strong>Headers</strong>
                    <pre>
                      {activeDebug.headers
                        .map((header) => `${header.name}: ${header.value}`)
                        .join("\n") || "-"}
                    </pre>
                  </div>
                  <div className="request-debug-block">
                    <strong>Body</strong>
                    <pre>{activeDebugBodyText || "-"}</pre>
                  </div>
                </Tabs.Content>
              </Tabs.Root>
            ) : null}
          </section>
        ) : null}

        {log.error ? (
          <section className="request-detail-section request-error-detail">
            <header>
              <h3>错误详情</h3>
            </header>
            <pre>{log.error}</pre>
          </section>
        ) : null}
      </div>

      <footer>
        <Button className="primary" onClick={onClose} type="button">
          关闭
        </Button>
      </footer>
    </DialogSurface>
  );
}

function Dashboard(props: {
  activeProvider: ProviderSummary | null;
  cachedInput: number;
  modelRows: Array<{ model: string; requests: number; tokens: number }>;
  onRangeChange: (range: TimeRange) => Promise<void>;
  onManageProviders: () => void;
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
    onManageProviders,
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
  const availableProviders = providers.filter(
    (provider) => provider.enabled && provider.status === "enabled",
  ).length;
  const successCount =
    stats?.success_count ??
    Math.max(requestCount - (stats?.failed_count ?? 0), 0);
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
        <div>
          <span className="eyebrow">WORKSPACE OVERVIEW</span>
          <h2>使用概览</h2>
          <p>让每一次模型调用，都清晰可见。</p>
        </div>
        <SegmentedControl
          className="range-tabs"
          label="统计时间范围"
          value={timeRange}
          onChange={onRangeChange}
          options={(["today", "week", "month", "all"] as TimeRange[]).map(
            (range) => ({ value: range, label: rangeLabel(range) }),
          )}
        />
      </div>

      <div className="metric-grid">
        <Metric
          title="成功请求"
          value={Math.max(successCount, 0).toLocaleString()}
          tone="green"
          sub="通过本地网关完成"
        />
        <Metric
          title="总 Token"
          value={formatCompact(totalTokens)}
          tone="cyan"
          tooltip={tokenTooltip}
          sub="输入与输出合计"
        />
        <Metric
          title="成功率"
          value={`${successRate.toFixed(1)}%`}
          tone="purple"
          sub={`${stats?.failed_count ?? 0} 次失败请求`}
        />
        <Metric
          title="平均首字延迟"
          value={formatMs(avgFirstByte)}
          tone="amber"
          sub="从发起请求到首次输出"
        />
      </div>

      <div className="dashboard-grid">
        <Card className="card trend-card">
          <div className="card-head">
            <div>
              <h3>使用趋势</h3>
              <p>
                {rangeLabel(timeRange)}{" "}
                {bucketGranularityLabel(stats?.bucket_granularity)}的
                {trendMetricLabel(trendMetric)}
              </p>
            </div>
            <SegmentedControl
              className="mini-tabs"
              label="趋势指标"
              value={trendMetric}
              onChange={onTrendMetricChange}
              options={(["tokens", "requests"] as TrendMetric[]).map(
                (metric) => ({
                  value: metric,
                  label: trendMetricLabel(metric),
                }),
              )}
            />
          </div>
          <TrendLineChart
            buckets={trendBuckets}
            compact
            granularity={stats?.bucket_granularity}
            metric={trendMetric}
            range={timeRange}
          />
        </Card>

        <Card className="card provider-card">
          <div className="card-head">
            <div>
              <h3>供应商状态</h3>
              <p>
                {availableProviders} 个可用，共 {providers.length} 个连接
              </p>
            </div>
            <Button className="text-button" onClick={onManageProviders}>
              管理 <ArrowUpRight size={14} aria-hidden="true" />
            </Button>
          </div>
          <div className="provider-health-list">
            {providers.slice(0, 4).map((provider) => {
              const status = providerStatus(provider);
              const balance = providerBalanceMeta(provider);
              const latency = providerLatencyMeta(provider);
              return (
                <div className="provider-health" key={provider.id}>
                  <span className="provider-avatar" aria-hidden="true">
                    {provider.name.slice(0, 1).toUpperCase()}
                  </span>
                  <div className="provider-health-summary">
                    <strong>{provider.name}</strong>
                    <small>{status.label}</small>
                  </div>
                  <div className="provider-health-facts">
                    <Tooltip content={provider.balance_error ?? undefined}>
                      <span className={`provider-health-fact ${balance.tone}`}>
                        <small>余额</small>
                        <b>{balance.value}</b>
                      </span>
                    </Tooltip>
                    <Tooltip content={provider.latency_error ?? undefined}>
                      <span className={`provider-health-fact ${latency.tone}`}>
                        <small>延迟</small>
                        <b>{latency.value}</b>
                      </span>
                    </Tooltip>
                  </div>
                </div>
              );
            })}
            {providers.length === 0 && (
              <EmptyState
                title="尚未连接供应商"
                description="添加第一个供应商，开始使用本地网关。"
                action={
                  <Button className="primary" onClick={onManageProviders}>
                    <Plus size={14} />
                    添加供应商
                  </Button>
                }
              />
            )}
          </div>
          <div className="health-bar">
            <span
              style={{
                width: `${providers.length ? (availableProviders / providers.length) * 100 : 0}%`,
              }}
            />
          </div>
          <footer>
            <span>
              可用供应商 {availableProviders}/{providers.length}
            </span>
            <b>
              {providers.length
                ? Math.round((availableProviders / providers.length) * 100)
                : 0}
              %
            </b>
          </footer>
        </Card>

        <Card className="card donut-card">
          <h3>Token 构成</h3>
          <p>
            {rangeLabel(timeRange)}共计 {formatCompact(totalTokens)} Token
          </p>
          <div className="donut-area">
            <div
              className="donut"
              role="img"
              aria-label={`Token 构成：非缓存输入 ${uncachedInput}，缓存输入 ${cachedInput}，输出 ${outputTokens}`}
              style={{
                background: totalTokens
                  ? `conic-gradient(var(--chart-primary) 0 ${(uncachedInput / totalTokens) * 100}%, var(--chart-purple) ${(uncachedInput / totalTokens) * 100}% ${((uncachedInput + cachedInput) / totalTokens) * 100}%, var(--chart-amber) ${((uncachedInput + cachedInput) / totalTokens) * 100}% 100%)`
                  : "var(--surface-strong)",
              }}
            >
              <strong>{formatCompact(totalTokens)}</strong>
              <span>总 Token</span>
            </div>
            <div className="legend">
              <Legend
                color="cyan"
                label="非缓存输入"
                value={formatCompact(uncachedInput)}
              />
              <Legend
                color="blue"
                label="缓存输入"
                value={formatCompact(cachedInput)}
              />
              <Legend
                color="amber"
                label="输出"
                value={formatCompact(outputTokens)}
              />
            </div>
          </div>
        </Card>

        <Card className="card model-card">
          <h3>模型用量</h3>
          <div className="model-table">
            <header>
              <span>模型</span>
              <span>请求</span>
              <span>Token</span>
              <span>占比</span>
            </header>
            {modelRows.map((row) => (
              <div className="model-row" key={row.model}>
                <UsageName
                  name={row.model}
                  label={chartModelLabel(row.model, providers)}
                />
                <span>{row.requests}</span>
                <span>{formatCompact(row.tokens)}</span>
                <b>
                  {totalTokens
                    ? `${Math.round((row.tokens / totalTokens) * 100)}%`
                    : "0%"}
                </b>
                <i
                  style={{
                    width: `${totalTokens ? (row.tokens / totalTokens) * 100 : 0}%`,
                  }}
                />
              </div>
            ))}
          </div>
          {modelRows.length === 0 && (
            <EmptyState
              title="暂无模型用量"
              description="经过网关的调用会显示在这里。"
            />
          )}
        </Card>

        <Card className="card quality-card">
          <h3>请求质量</h3>
          <p>{rangeLabel(timeRange)}通过本地网关的请求</p>
          <Quality
            label="成功率"
            value={`${successRate.toFixed(1)}%`}
            tone="green"
          />
          <Quality
            label="平均首字延迟"
            value={formatMs(avgFirstByte)}
            tone="cyan"
          />
          <Quality
            label="平均总耗时"
            value={avgTotalMs == null ? "-" : formatDuration(avgTotalMs)}
            tone="purple"
          />
          <Quality
            label="当前活跃请求"
            value={String(activeRequests)}
            tone="amber"
          />
        </Card>
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
  const Icon =
    tone === "green"
      ? ArrowUpRight
      : tone === "cyan"
        ? Zap
        : tone === "amber"
          ? Clock3
          : CircleCheck;
  return (
    <Card className="metric" title={tooltip}>
      <div className="metric-heading">
        <p>{title}</p>
        <span className={`metric-icon ${tone}`}>
          <Icon size={16} aria-hidden="true" />
        </span>
      </div>
      <strong>{value}</strong>
      {sub && <small>{sub}</small>}
    </Card>
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
  return (
    <UsageTrendChart
      buckets={completeTrendBuckets(buckets, range, granularity)}
      compact={compact}
      granularity={
        granularity ??
        (range === "all" ? "month" : range === "today" ? "hour" : "day")
      }
      metric={metric}
    />
  );
}

function Legend({
  color,
  label,
  value,
}: {
  color: string;
  label: string;
  value: string;
}) {
  return (
    <div className="legend-row">
      <span className={`dot ${color}`} />
      <em>{label}</em>
      <strong>{value}</strong>
    </div>
  );
}

function Quality({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone: string;
}) {
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
  const canTest =
    !state.preparing &&
    !state.testing &&
    Boolean(state.selectedModel) &&
    Boolean(state.prompt.trim());
  return (
    <DialogSurface
      title="模型测速"
      onClose={onClose}
      closeDisabled={state.testing}
    >
      <header>
        <div className="latency-dialog-title">
          <span>
            <ToolIcon type="latency" />
          </span>
          <div>
            <h2 id="latency-dialog-title">模型测速</h2>
            <p>
              {state.provider.name} · {state.provider.base_url}
            </p>
          </div>
        </div>
        <Button
          aria-label="关闭"
          className="close"
          disabled={state.testing}
          onClick={onClose}
          title="关闭"
          type="button"
        >
          ×
        </Button>
      </header>

      <div className="latency-dialog-body">
        <div className="latency-mode-field">
          <span>请求方式</span>
          <SegmentedControl
            label="请求方式"
            className="latency-mode-control"
            value={state.streaming ? "streaming" : "sync"}
            disabled={state.testing}
            onChange={(value) => onStreamingChange(value === "streaming")}
            options={[
              { value: "sync", label: "同步" },
              { value: "streaming", label: "流式" },
            ]}
          />
        </div>

        <label className="field">
          <span>测试模型</span>
          <Select
            disabled={
              state.preparing || state.testing || state.models.length === 0
            }
            value={state.selectedModel}
            onValueChange={(value) => onModelChange(value)}
          >
            {state.preparing && (
              <SelectOption value="">正在读取模型…</SelectOption>
            )}
            {!state.preparing && state.models.length === 0 && (
              <SelectOption value="">没有可用模型</SelectOption>
            )}
            {state.models.map((model) => (
              <SelectOption key={model} value={model}>
                {model}
              </SelectOption>
            ))}
          </Select>
          {state.providerKind === "codex" && (
            <small>
              已开启 WebSocket 的供应商在流式测试中使用 WS；同步测试仍使用
              HTTP。
            </small>
          )}
        </label>

        <label className="field">
          <span>测试内容</span>
          <TextArea
            disabled={state.testing}
            maxLength={2000}
            rows={4}
            value={state.prompt}
            onChange={(event) => onPromptChange(event.currentTarget.value)}
          />
          <small>{state.prompt.length} / 2000</small>
        </label>

        {state.result && (
          <div
            aria-live="polite"
            className={`latency-test-result ${state.result.ok ? "ok" : "failed"}`}
          >
            <div className="latency-result-summary">
              <span className={`dot ${state.result.ok ? "green" : "danger"}`} />
              <div>
                <strong>
                  {state.result.ok
                    ? `${state.result.latencyMs} ms`
                    : "测速失败"}
                </strong>
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
        <Button
          className="ghost"
          disabled={state.testing}
          onClick={onClose}
          type="button"
        >
          取消
        </Button>
        <Button
          className="primary"
          disabled={!canTest}
          onClick={onTest}
          type="button"
        >
          {state.testing ? "正在测速…" : state.result ? "重新测速" : "开始测速"}
        </Button>
      </footer>
    </DialogSurface>
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
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState("all");
  const [refreshingBalances, setRefreshingBalances] = useState(false);
  const filtered = query.trim() !== "" || statusFilter !== "all";
  const visibleProviders = providers.filter(
    (provider) =>
      (!query.trim() ||
        `${provider.name} ${provider.base_url}`
          .toLowerCase()
          .includes(query.trim().toLowerCase())) &&
      (statusFilter === "all" || provider.status === statusFilter),
  );
  const [draggingProviderId, setDraggingProviderId] = useState<string | null>(
    null,
  );
  const [dragOverProviderId, setDragOverProviderId] = useState<string | null>(
    null,
  );
  const draggingProviderIdRef = useRef<string | null>(null);
  const dragStartY = useRef(0);

  function nextProviderOrder(draggedId: string, clientY: number) {
    const providerIds = providers.map((provider) => provider.id);
    const draggedIndex = providerIds.indexOf(draggedId);
    if (draggedIndex < 0 || providerIds.length < 2) return providerIds;

    const rows = Array.from(
      document.querySelectorAll<HTMLElement>("[data-provider-row-id]"),
    );
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
    nextIds.splice(
      Math.max(0, Math.min(insertIndex, nextIds.length)),
      0,
      movedId,
    );
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

  function handlePointerDown(
    event: PointerEvent<HTMLButtonElement>,
    providerId: string,
  ) {
    if (busy) return;
    event.preventDefault();
    dragStartY.current = event.clientY;
    draggingProviderIdRef.current = providerId;
    setDraggingProviderId(providerId);
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function handlePointerUp(
    event: PointerEvent<HTMLButtonElement>,
    providerId: string,
  ) {
    event.preventDefault();
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    finishPointerReorder(providerId, event.clientY);
  }

  function handlePointerMove(
    event: PointerEvent<HTMLButtonElement>,
    providerId: string,
  ) {
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
      <div className="section-title">
        <div>
          <span className="eyebrow">YOUR CONNECTIONS</span>
          <h2>供应商管理</h2>
          <p>集中管理上游连接，按优先级自动路由。</p>
        </div>
        <Button className="primary" onClick={onAdd} disabled={busy}>
          <Plus size={15} aria-hidden="true" />
          添加{providerKind === "claude" ? " Claude" : ""}供应商
        </Button>
      </div>
      <div className="provider-toolbar">
        <SegmentedControl
          className="protocol-switch"
          label="供应商类型"
          value={providerKind}
          onChange={(kind) => {
            setQuery("");
            setStatusFilter("all");
            onKindChange(kind);
          }}
          options={[
            {
              value: "codex",
              label: (
                <>
                  <Code2 size={15} aria-hidden="true" />
                  Codex
                </>
              ),
            },
            { value: "claude", label: "Claude" },
          ]}
        />
        <div className="provider-toolbar-filters">
          <label className="search-box">
            <Search size={15} aria-hidden="true" />
            <TextField
              aria-label="搜索供应商"
              placeholder="搜索名称或 Base URL"
              value={query}
              onChange={(event) => setQuery(event.currentTarget.value)}
            />
          </label>
          <Select
            aria-label="供应商状态"
            value={statusFilter}
            onValueChange={(value) => setStatusFilter(value)}
          >
            <SelectOption value="all">全部状态</SelectOption>
            <SelectOption value="enabled">已启用</SelectOption>
            <SelectOption value="disabled">已停用</SelectOption>
            <SelectOption value="auto_disabled">自动停用</SelectOption>
          </Select>
          {providerKind === "codex" && (
            <Button
              className="ghost"
              disabled={busy || refreshingBalances || providers.length === 0}
              onClick={async () => {
                setRefreshingBalances(true);
                try {
                  for (const provider of providers)
                    await onRefreshBalance(provider);
                } finally {
                  setRefreshingBalances(false);
                }
              }}
            >
              <Wallet size={14} aria-hidden="true" />
              {refreshingBalances ? "刷新中…" : "刷新余额"}
            </Button>
          )}
        </div>
      </div>
      <div className="list-caption">
        <span>
          共 {providers.length} 个供应商
          {filtered ? `，找到 ${visibleProviders.length} 个` : ""}
        </span>
        <span>
          <Network size={12} aria-hidden="true" />
          按列表顺序路由
        </span>
      </div>

      <Card className="provider-table">
        <header>
          <span />
          <span>状态</span>
          <span>供应商</span>
          <span>余额</span>
          <span>延迟</span>
          <span>启用</span>
          <span>操作</span>
        </header>
        {visibleProviders.map((provider) => {
          const status = providerStatus(provider);
          const balance = providerBalanceMeta(provider);
          const latency = providerLatencyMeta(provider);
          return (
            <div
              className={`provider-line ${provider.status === "enabled" ? "selected" : ""} ${draggingProviderId === provider.id ? "dragging" : ""} ${dragOverProviderId === provider.id ? "drop-target" : ""}`}
              data-provider-row-id={provider.id}
              key={provider.id}
            >
              <Button
                aria-label={`调整 ${provider.name} 的优先级`}
                className="drag-handle"
                disabled={busy || filtered}
                onKeyDown={(event) => {
                  if (event.key !== "ArrowUp" && event.key !== "ArrowDown")
                    return;
                  event.preventDefault();
                  const ids = providers.map((item) => item.id);
                  const from = ids.indexOf(provider.id),
                    to = from + (event.key === "ArrowUp" ? -1 : 1);
                  if (to < 0 || to >= ids.length) return;
                  [ids[from], ids[to]] = [ids[to], ids[from]];
                  void onReorder(ids);
                }}
                onPointerCancel={() => {
                  draggingProviderIdRef.current = null;
                  setDraggingProviderId(null);
                  setDragOverProviderId(null);
                }}
                onPointerDown={(event) => handlePointerDown(event, provider.id)}
                onPointerMove={(event) => handlePointerMove(event, provider.id)}
                onPointerUp={(event) => handlePointerUp(event, provider.id)}
                title={
                  filtered
                    ? "清除筛选后可调整优先级"
                    : "拖动或使用上下方向键调整优先级"
                }
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
              </Button>
              <div className={`provider-status ${status.tone}`}>
                <span className={`dot ${status.tone}`} />
                <b>{status.label}</b>
              </div>
              <div className="provider-name-cell">
                <span className="provider-avatar" aria-hidden="true">
                  {provider.name.slice(0, 1).toUpperCase()}
                </span>
                <div>
                  <strong>
                    {provider.name}
                    {providers.indexOf(provider) === 0 && (
                      <Badge className="priority-badge">优先</Badge>
                    )}
                  </strong>
                  <Tooltip content={provider.base_url}>
                    <small>{provider.base_url || "未配置 Base URL"}</small>
                  </Tooltip>
                </div>
              </div>
              <Tooltip content={provider.balance_error ?? undefined}>
                <div className={`balance-cell provider-fact ${balance.tone}`}>
                  <strong>{balance.value}</strong>
                  <small>{balance.detail}</small>
                </div>
              </Tooltip>
              <Tooltip content={provider.latency_error ?? undefined}>
                <div className={`latency-cell provider-fact ${latency.tone}`}>
                  <strong>{latency.value}</strong>
                  <small>{latency.detail}</small>
                </div>
              </Tooltip>
              <Toggle
                label={`启用 ${provider.name}`}
                checked={provider.status === "enabled"}
                disabled={busy}
                onChange={(checked) => onToggle(provider, checked)}
              />
              <div className="provider-toolbox">
                <Button
                  aria-label="设置"
                  className="provider-action-button main-action"
                  disabled={busy}
                  onClick={() => onEdit(provider, "base")}
                  title="设置"
                  type="button"
                >
                  <ToolIcon type="settings" />
                  <span>设置</span>
                </Button>
                <Button
                  aria-label="测试延迟"
                  className="provider-action-button"
                  disabled={busy}
                  onClick={() => void onTestLatency(provider)}
                  title="测试延迟"
                  type="button"
                >
                  <ToolIcon type="latency" />
                  <span>测速</span>
                </Button>
                <Button
                  aria-label="刷新余额"
                  className="provider-action-button"
                  disabled={busy || providerKind === "claude"}
                  onClick={() => void onRefreshBalance(provider)}
                  title="刷新余额"
                  type="button"
                >
                  <ToolIcon type="balance" />
                  <span>余额</span>
                </Button>
              </div>
            </div>
          );
        })}
        {visibleProviders.length === 0 && (
          <EmptyState
            title={
              providers.length ? "没有匹配的供应商" : "添加你的第一个供应商"
            }
            description={
              providers.length
                ? "试试其他名称、地址或状态。"
                : "连接模型服务后，即可为客户端提供统一路由。"
            }
            action={
              providers.length ? (
                <Button
                  className="ghost"
                  onClick={() => {
                    setQuery("");
                    setStatusFilter("all");
                  }}
                >
                  清除筛选
                </Button>
              ) : (
                <Button className="primary" onClick={onAdd}>
                  <Plus size={14} />
                  添加供应商
                </Button>
              )
            }
          />
        )}
        <footer>
          <span>拖动左侧图标调整优先级，越靠上越优先。</span>
          <span>也可使用 ↑ ↓ 方向键</span>
        </footer>
      </Card>
    </section>
  );
}

function AutoDisableResetFields(props: {
  period: AutoDisableResetPeriod;
  resetDay: number;
  resetTime: string;
  resetWeekday: number;
  setPeriod: (value: AutoDisableResetPeriod) => void;
  setResetDay: (value: number) => void;
  setResetTime: (value: string) => void;
  setResetWeekday: (value: number) => void;
}) {
  const {
    period,
    resetDay,
    resetTime,
    resetWeekday,
    setPeriod,
    setResetDay,
    setResetTime,
    setResetWeekday,
  } = props;
  return (
    <>
      <label className="field">
        <span>额度刷新周期</span>
        <Select
          value={period}
          onValueChange={(value) => setPeriod(value as AutoDisableResetPeriod)}
        >
          <SelectOption value="daily">每天</SelectOption>
          <SelectOption value="weekly">每周</SelectOption>
          <SelectOption value="monthly">每月</SelectOption>
          <SelectOption value="never">不刷新</SelectOption>
        </Select>
      </label>
      {period === "weekly" && (
        <label className="field">
          <span>每周刷新日</span>
          <Select
            value={resetWeekday}
            onValueChange={(value) => setResetWeekday(Number(value))}
          >
            <SelectOption value={1}>星期一</SelectOption>
            <SelectOption value={2}>星期二</SelectOption>
            <SelectOption value={3}>星期三</SelectOption>
            <SelectOption value={4}>星期四</SelectOption>
            <SelectOption value={5}>星期五</SelectOption>
            <SelectOption value={6}>星期六</SelectOption>
            <SelectOption value={7}>星期日</SelectOption>
          </Select>
        </label>
      )}
      {period === "monthly" && (
        <label className="field">
          <span>每月刷新日</span>
          <TextField
            max={31}
            min={1}
            type="number"
            value={resetDay}
            onChange={(event) =>
              setResetDay(
                Math.min(
                  31,
                  Math.max(1, Number(event.currentTarget.value) || 1),
                ),
              )
            }
          />
          <small>短月份按当月最后一天刷新</small>
        </label>
      )}
      {period !== "never" && (
        <label className="field">
          <span>刷新时间</span>
          <TextField
            type="time"
            value={resetTime}
            onChange={(event) => setResetTime(event.currentTarget.value)}
          />
        </label>
      )}
    </>
  );
}

function AutoDisablePolicyFields(props: {
  override: boolean;
  enabled: boolean;
  threshold: number;
  globalEnabled: boolean;
  globalThreshold: number;
  setOverride: (value: boolean) => void;
  setEnabled: (value: boolean) => void;
  setThreshold: (value: number) => void;
}) {
  const {
    override,
    enabled,
    threshold,
    globalEnabled,
    globalThreshold,
    setOverride,
    setEnabled,
    setThreshold,
  } = props;
  const thresholdError = autoDisableThresholdError(
    override && enabled,
    threshold,
  );
  return (
    <section className="auto-disable-policy">
      <label className="field">
        <span>自动禁用策略</span>
        <Select
          value={override ? "custom" : "global"}
          onValueChange={(value) => {
            const custom = value === "custom";
            setOverride(custom);
            if (custom && !override) {
              setEnabled(globalEnabled);
              setThreshold(globalThreshold);
            }
          }}
        >
          <SelectOption value="global">继承全局设置</SelectOption>
          <SelectOption value="custom">单独设置</SelectOption>
        </Select>
        <small>
          全局：
          {globalEnabled
            ? `连续失败 ${globalThreshold} 次后禁用`
            : "已关闭自动禁用"}
        </small>
      </label>
      {override ? (
        <>
          <div className="route-toggle-line">
            <div>
              <strong>启用自动禁用</strong>
              <p>连续失败达到次数后停用此供应商。</p>
            </div>
            <Toggle
              checked={enabled}
              onChange={setEnabled}
              label="启用此供应商的自动禁用"
            />
          </div>
          <label className="field">
            <span>连续失败次数</span>
            <TextField
              aria-invalid={Boolean(thresholdError)}
              disabled={!enabled}
              max={MAX_AUTO_DISABLE_FAILURE_THRESHOLD}
              min={1}
              step={1}
              type="number"
              value={threshold}
              onChange={(event) =>
                setThreshold(Number(event.currentTarget.value))
              }
            />
            {thresholdError ? (
              <small className="danger-text">{thresholdError}</small>
            ) : null}
          </label>
        </>
      ) : null}
    </section>
  );
}

function ProviderEditor(props: {
  error: string;
  modelListCleared: boolean;
  onClearModels: () => void;
  fusionEnabled: boolean;
  setFusionEnabled: (enabled: boolean) => void;
  allowedModels: string[];
  balanceAccountOptions: AiGateBalanceAccount[];
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
  providerAutoDisableResetPeriod: AutoDisableResetPeriod;
  providerAutoDisableResetWeekday: number;
  providerAutoDisableResetDay: number;
  providerAutoDisableResetTime: string;
  providerAutoDisableOverride: boolean;
  providerAutoDisableEnabled: boolean;
  providerAutoDisableFailureThreshold: number;
  globalAutoDisableEnabled: boolean;
  globalAutoDisableFailureThreshold: number;
  providerServiceTier: ProviderServiceTier;
  providerWireApi: ProviderWireApi;
  providerSupportsWebsockets: boolean;
  setProviderSupportsWebsockets: (value: boolean) => void;
  providerImageDetailOriginalToAuto: boolean;
  secretVisible: boolean;
  setAllowedModels: (models: string[]) => void;
  setProviderApiKey: (value: string) => void;
  setProviderApiKeyDirty: (dirty: boolean) => void;
  setProviderBaseUrl: (value: string) => void;
  setModelMappings: (mappings: ModelMapping[]) => void;
  setProviderName: (value: string) => void;
  setProviderAutoDisableResetPeriod: (value: AutoDisableResetPeriod) => void;
  setProviderAutoDisableResetWeekday: (value: number) => void;
  setProviderAutoDisableResetDay: (value: number) => void;
  setProviderAutoDisableResetTime: (value: string) => void;
  setProviderAutoDisableOverride: (value: boolean) => void;
  setProviderAutoDisableEnabled: (value: boolean) => void;
  setProviderAutoDisableFailureThreshold: (value: number) => void;
  setProviderServiceTier: (value: ProviderServiceTier) => void;
  setProviderWireApi: (value: ProviderWireApi) => void;
  setProviderImageDetailOriginalToAuto: (value: boolean) => void;
  setSecretVisible: (value: boolean) => void;
  tab: EditorTab;
}) {
  const {
    error,
    modelListCleared,
    onClearModels,
    fusionEnabled,
    setFusionEnabled,
    allowedModels,
    balanceAccountOptions,
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
    providerAutoDisableResetPeriod,
    providerAutoDisableResetWeekday,
    providerAutoDisableResetDay,
    providerAutoDisableResetTime,
    providerAutoDisableOverride,
    providerAutoDisableEnabled,
    providerAutoDisableFailureThreshold,
    globalAutoDisableEnabled,
    globalAutoDisableFailureThreshold,
    providerServiceTier,
    providerWireApi,
    providerSupportsWebsockets,
    setProviderSupportsWebsockets,
    providerImageDetailOriginalToAuto,
    secretVisible,
    setAllowedModels,
    setProviderApiKey,
    setProviderApiKeyDirty,
    setProviderBaseUrl,
    setModelMappings,
    setProviderName,
    setProviderAutoDisableResetPeriod,
    setProviderAutoDisableResetWeekday,
    setProviderAutoDisableResetDay,
    setProviderAutoDisableResetTime,
    setProviderAutoDisableOverride,
    setProviderAutoDisableEnabled,
    setProviderAutoDisableFailureThreshold,
    setProviderServiceTier,
    setProviderWireApi,
    setProviderImageDetailOriginalToAuto,
    setSecretVisible,
    tab,
  } = props;
  const [customAllowedModel, setCustomAllowedModel] = useState("");
  const [deleteConfirming, setDeleteConfirming] = useState(false);
  const [modelSearch, setModelSearch] = useState("");
  const allowedModelOptions = normalizeModelNames([
    ...providerModels,
    ...allowedModels,
  ]);
  const visibleModelOptions = allowedModelOptions.filter((model) =>
    model.toLowerCase().includes(modelSearch.trim().toLowerCase()),
  );
  const allModelsAllowed = allowedModels.length === 0 && !modelListCleared;
  const selectedAiGateAccountId = balanceQuery.ai_gate_account_id ?? null;
  const selectedAiGateAccountMissing =
    selectedAiGateAccountId !== null &&
    !balanceAccountOptions.some(
      (account) => account.account_id === selectedAiGateAccountId,
    );
  const upstreamPath =
    providerWireApi === "chat_completions" ? "chat/completions" : "responses";
  const updateAllowedModels = (models: string[]) =>
    setAllowedModels(normalizeModelNames(models));
  const toggleAllowedModel = (model: string, checked: boolean) => {
    const currentModels = allModelsAllowed
      ? allowedModelOptions
      : allowedModels;
    updateAllowedModels(
      checked
        ? [...currentModels, model]
        : currentModels.filter(
            (current) => current.toLowerCase() !== model.toLowerCase(),
          ),
    );
  };
  const addCustomAllowedModel = () => {
    const model = customAllowedModel.trim();
    if (!model) return;
    updateAllowedModels([
      ...(allModelsAllowed ? allowedModelOptions : allowedModels),
      model,
    ]);
    setCustomAllowedModel("");
  };

  return (
    <DialogSurface
      drawer
      title="编辑供应商"
      onClose={onClose}
      closeDisabled={busy}
    >
      <header>
        <div>
          <h2>编辑供应商</h2>
          <p>{providerName || "未命名供应商"}</p>
        </div>
        <Button className="close" disabled={busy} onClick={onClose}>
          ×
        </Button>
      </header>

      <SegmentedControl
        className="drawer-tabs"
        label="供应商配置分类"
        value={tab}
        onChange={onTab}
        options={[
          { value: "base", label: "基础配置" },
          { value: "models", label: "模型配置" },
          { value: "balance", label: "余额查询" },
        ]}
      />

      <section className="drawer-body">
        {error && (
          <div className="form-error" role="alert">
            {error}
          </div>
        )}
        {tab === "base" && (
          <>
            <label className="field">
              <span>供应商名称</span>
              <TextField
                value={providerName}
                onChange={(event) => setProviderName(event.currentTarget.value)}
              />
            </label>
            <label className="field">
              <span>Base URL</span>
              <TextField
                value={providerBaseUrl}
                onChange={(event) =>
                  setProviderBaseUrl(event.currentTarget.value)
                }
              />
              <small>
                请求将转发至{" "}
                {providerBaseUrl
                  ? `${providerBaseUrl.replace(/\/$/, "")}/${upstreamPath}`
                  : "-"}
              </small>
            </label>
            <label className="field">
              <span>转发 API Key</span>
              <div className="secret-field">
                <TextField
                  type={secretVisible ? "text" : "password"}
                  value={providerApiKey}
                  onChange={(event) => {
                    setProviderApiKey(event.currentTarget.value);
                    setProviderApiKeyDirty(true);
                  }}
                  placeholder="留空保持已保存的 API Key"
                />
                <Button
                  onClick={() => setSecretVisible(!secretVisible)}
                  type="button"
                >
                  {secretVisible ? "隐藏" : "显示"}
                </Button>
              </div>
            </label>
            <label className="field">
              <span>接口协议</span>
              <Select
                value={providerWireApi}
                onValueChange={(value) =>
                  setProviderWireApi(value as ProviderWireApi)
                }
              >
                <SelectOption value="responses">Responses API</SelectOption>
                <SelectOption value="chat_completions">
                  Chat Completions 兼容
                </SelectOption>
              </Select>
              <small>
                上游不支持 Responses API 时选择 Chat Completions 兼容模式
              </small>
            </label>
            <label className="field">
              <span className="fusion-toggle">
                <Checkbox
                  checked={
                    providerWireApi === "responses" &&
                    providerSupportsWebsockets
                  }
                  disabled={providerWireApi !== "responses"}
                  onCheckedChange={(checked) =>
                    setProviderSupportsWebsockets(checked)
                  }
                />{" "}
                上游支持 WebSocket
              </span>
              <small>
                仅适用于 Responses API。Codex 开启 WebSocket
                后，此供应商使用持久连接；未开启的供应商继续使用 HTTP。
              </small>
            </label>
            <label className="field">
              <span className="fusion-toggle">
                <Checkbox
                  checked={providerImageDetailOriginalToAuto}
                  onCheckedChange={(checked) =>
                    setProviderImageDetailOriginalToAuto(checked)
                  }
                />{" "}
                图片 detail 兼容：original → auto
              </span>
              <small>
                仅对当前供应商生效。上游不支持 original
                图片精度时开启，发送前将其转换为 auto；默认关闭。
              </small>
            </label>
            <label className="field">
              <span className="fusion-toggle">
                <Checkbox
                  checked={fusionEnabled}
                  onCheckedChange={(checked) => {
                    const enabled = checked;
                    setFusionEnabled(enabled);
                    if (enabled && allowedModels.length === 0)
                      setAllowedModels(providerModels);
                  }}
                />{" "}
                融合模式
              </span>
              <small>
                选定的模型将显示供应商名称，并仅发往此供应商。普通模型请求不会使用此供应商。请在模型配置中选择要发布的模型；Codex
                需重启以刷新列表。
              </small>
            </label>
            <AutoDisablePolicyFields
              override={providerAutoDisableOverride}
              enabled={providerAutoDisableEnabled}
              threshold={providerAutoDisableFailureThreshold}
              globalEnabled={globalAutoDisableEnabled}
              globalThreshold={globalAutoDisableFailureThreshold}
              setOverride={setProviderAutoDisableOverride}
              setEnabled={setProviderAutoDisableEnabled}
              setThreshold={setProviderAutoDisableFailureThreshold}
            />
            <AutoDisableResetFields
              period={providerAutoDisableResetPeriod}
              resetDay={providerAutoDisableResetDay}
              resetTime={providerAutoDisableResetTime}
              resetWeekday={providerAutoDisableResetWeekday}
              setPeriod={setProviderAutoDisableResetPeriod}
              setResetDay={setProviderAutoDisableResetDay}
              setResetTime={setProviderAutoDisableResetTime}
              setResetWeekday={setProviderAutoDisableResetWeekday}
            />
            <label className="field">
              <span>请求服务层级</span>
              <Select
                value={providerServiceTier}
                onValueChange={(value) =>
                  setProviderServiceTier(value as ProviderServiceTier)
                }
              >
                <SelectOption value="">不强制</SelectOption>
                <SelectOption value="priority">Fast</SelectOption>
                <SelectOption value="ultrafast">Ultrafast</SelectOption>
              </Select>
              <small>Ultrafast 需要上游授权，目前仅适用于 GPT-5.6 Sol</small>
            </label>
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
              <Button
                className="primary model-fetch-button"
                disabled={busy}
                onClick={onLoadProviderModels}
                type="button"
              >
                <ToolIcon type="models" />
                {busy
                  ? "正在获取"
                  : providerModels.length
                    ? "重新从上游获取"
                    : "从上游获取模型"}
              </Button>
            </section>

            <div className="model-config-stats">
              <div>
                <span>上游已发现</span>
                <strong>{providerModels.length}</strong>
                <small>个模型</small>
              </div>
              <div>
                <span>当前已启用</span>
                <strong>
                  {allModelsAllowed ? "全部" : allowedModels.length}
                </strong>
                <small>{allModelsAllowed ? "兼容任意模型" : "个模型"}</small>
              </div>
            </div>

            <div className="model-filter-box model-manager">
              <div className="mapping-box-head">
                <div>
                  <strong>启用模型</strong>
                  <p>取消不希望参与路由的模型；修改会在保存供应商后生效。</p>
                </div>
                <div className="model-list-actions">
                  <Button
                    className="ghost small"
                    disabled={allowedModelOptions.length === 0}
                    onClick={() => updateAllowedModels(allowedModelOptions)}
                    type="button"
                  >
                    全部启用
                  </Button>
                  <Button
                    className="ghost small"
                    disabled={busy || allowedModelOptions.length === 0}
                    onClick={() => {
                      onClearModels();
                      setModelSearch("");
                      setCustomAllowedModel("");
                    }}
                    type="button"
                  >
                    一键清空
                  </Button>
                </div>
              </div>
              <label className="model-search-field">
                <span aria-hidden="true" />
                <TextField
                  aria-label="搜索上游模型"
                  placeholder="搜索上游模型"
                  type="search"
                  value={modelSearch}
                  onChange={(event) =>
                    setModelSearch(event.currentTarget.value)
                  }
                />
              </label>
              <div className="model-option-grid">
                {allowedModelOptions.length === 0 && (
                  <div className="model-empty-state">
                    <strong>还没有上游模型</strong>
                    <p>确认 Base URL 和 API Key 后，从上游读取可用模型。</p>
                    <Button
                      className="ghost small"
                      disabled={busy}
                      onClick={onLoadProviderModels}
                      type="button"
                    >
                      从上游获取
                    </Button>
                  </div>
                )}
                {allowedModelOptions.length > 0 &&
                  visibleModelOptions.length === 0 && (
                    <p className="mapping-empty">
                      没有匹配“{modelSearch.trim()}”的模型。
                    </p>
                  )}
                {visibleModelOptions.map((model) => {
                  const checked =
                    allModelsAllowed ||
                    allowedModels.some(
                      (current) =>
                        current.toLowerCase() === model.toLowerCase(),
                    );
                  return (
                    <label
                      className={
                        checked ? "model-option selected" : "model-option"
                      }
                      key={model}
                    >
                      <Checkbox
                        checked={checked}
                        onCheckedChange={(checked) =>
                          toggleAllowedModel(model, checked)
                        }
                      />
                      <span>{model}</span>
                    </label>
                  );
                })}
              </div>
              <div className="custom-model-row">
                <TextField
                  aria-label="手动添加模型"
                  placeholder="手动添加未出现在上游列表中的模型"
                  value={customAllowedModel}
                  onChange={(event) =>
                    setCustomAllowedModel(event.currentTarget.value)
                  }
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      addCustomAllowedModel();
                    }
                  }}
                />
                <Button
                  className="ghost small"
                  disabled={!customAllowedModel.trim()}
                  onClick={addCustomAllowedModel}
                  type="button"
                >
                  添加模型
                </Button>
              </div>
            </div>

            <div className="mapping-box">
              <div className="mapping-box-head">
                <div>
                  <strong>模型映射</strong>
                  <p>客户端模型匹配后，转发为此供应商支持的上游模型。</p>
                </div>
                <Button
                  className="ghost small"
                  onClick={() =>
                    setModelMappings([
                      ...modelMappings,
                      { source: "", target: "" },
                    ])
                  }
                  type="button"
                >
                  添加映射
                </Button>
              </div>
              <div className="mapping-grid">
                <span>客户端模型</span>
                <span>上游模型</span>
                <span />
                {modelMappings.length === 0 && (
                  <p className="mapping-empty">
                    未配置映射时，请求模型将原样转发。
                  </p>
                )}
                {modelMappings.map((mapping, index) => (
                  <div className="mapping-row" key={index}>
                    <TextField
                      placeholder="gpt-5.5"
                      value={mapping.source}
                      onChange={(event) => {
                        const next = [...modelMappings];
                        next[index] = {
                          ...mapping,
                          source: event.currentTarget.value,
                        };
                        setModelMappings(next);
                      }}
                    />
                    <TextField
                      placeholder="deepseek-v4-pro"
                      value={mapping.target}
                      onChange={(event) => {
                        const next = [...modelMappings];
                        next[index] = {
                          ...mapping,
                          target: event.currentTarget.value,
                        };
                        setModelMappings(next);
                      }}
                    />
                    <Button
                      aria-label="删除映射"
                      className="icon-button"
                      onClick={() =>
                        setModelMappings(
                          modelMappings.filter(
                            (_, rowIndex) => rowIndex !== index,
                          ),
                        )
                      }
                      title="删除映射"
                      type="button"
                    >
                      ×
                    </Button>
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
              <Select
                value={balanceQuery.query_type}
                onValueChange={(value) =>
                  onUpdateBalance({
                    query_type: value as BalanceQueryType,
                  })
                }
              >
                <SelectOption value="new_api">New API</SelectOption>
                <SelectOption value="sub2_api">Sub2API</SelectOption>
                <SelectOption value="ai_gate">AI Gate</SelectOption>
                <SelectOption value="disabled">不查询</SelectOption>
              </Select>
            </label>
            {balanceQuery.query_type === "new_api" && (
              <label className="field">
                <span>查询目标</span>
                <Select
                  value={balanceQuery.new_api_target}
                  onValueChange={(value) =>
                    onUpdateBalance({
                      new_api_target: value as NewApiBalanceTarget,
                    })
                  }
                >
                  <SelectOption value="token_quota">API Key 额度</SelectOption>
                  <SelectOption value="account_balance">账户余额</SelectOption>
                </Select>
              </label>
            )}
            {balanceQuery.query_type === "ai_gate" && (
              <label className="field">
                <span>查询账号</span>
                <Select
                  value={
                    selectedAiGateAccountId === null
                      ? ""
                      : String(selectedAiGateAccountId)
                  }
                  onValueChange={(value) =>
                    onUpdateBalance({
                      ai_gate_account_id: value ? Number(value) : null,
                    })
                  }
                >
                  <SelectOption value="">聚合全部账号</SelectOption>
                  {selectedAiGateAccountMissing && (
                    <SelectOption value={selectedAiGateAccountId ?? ""}>
                      账号 #{selectedAiGateAccountId}（等待查询确认）
                    </SelectOption>
                  )}
                  {balanceAccountOptions.map((account) => (
                    <SelectOption
                      key={account.account_id}
                      value={account.account_id}
                    >
                      {account.account_name || `账号 #${account.account_id}`}
                    </SelectOption>
                  ))}
                </Select>
                <small>
                  {balanceAccountOptions.length
                    ? "可选择单个上游账号，或聚合全部账号；账号列表来自最近一次测试查询。"
                    : "先保持“聚合全部账号”并测试查询，成功后即可选择具体账号。"}
                </small>
              </label>
            )}
            <label className="field">
              <span>查询地址</span>
              <TextField
                value={balanceQuery.endpoint}
                onChange={(event) =>
                  onUpdateBalance({ endpoint: event.currentTarget.value })
                }
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
                <TextField
                  type={balanceTokenVisible ? "text" : "password"}
                  value={
                    balanceQuery.auth_mode === "provider_token"
                      ? savedApiKeyLabel(providerApiKey)
                      : balanceQuery.query_token
                  }
                  onChange={(event) =>
                    onUpdateBalance({
                      query_token: event.currentTarget.value,
                      auth_mode: "separate_token",
                    })
                  }
                  readOnly={balanceQuery.auth_mode === "provider_token"}
                />
                <Button
                  onClick={() => onBalanceTokenVisible(!balanceTokenVisible)}
                  type="button"
                >
                  {balanceTokenVisible ? "隐藏" : "显示"}
                </Button>
              </div>
            </label>
            {balanceQuery.query_type === "new_api" &&
              balanceQuery.new_api_target === "account_balance" && (
                <label className="field half">
                  <span>用户 ID</span>
                  <TextField
                    inputMode="numeric"
                    value={balanceQuery.new_api_user_id}
                    onChange={(event) =>
                      onUpdateBalance({
                        new_api_user_id: event.currentTarget.value.replace(
                          /\D/g,
                          "",
                        ),
                      })
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
                <span className="filter-hint">默认换算比例</span>
              </div>
            )}
            <div
              className={`balance-test-box ${balanceTestStatus?.error ? "failed" : balanceTestStatus ? "ok" : ""}`}
            >
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
              <Button disabled={busy} onClick={onTestBalance} type="button">
                {busy ? "查询中" : "测试查询"}
              </Button>
            </div>
          </>
        )}
      </section>

      <footer>
        <Button
          className="danger"
          disabled={busy}
          onClick={() =>
            deleteConfirming ? onDelete() : setDeleteConfirming(true)
          }
          type="button"
        >
          {deleteConfirming ? "确认删除" : "删除"}
        </Button>
        {deleteConfirming ? (
          <small className="danger-text">
            会保留历史请求日志；再次点击确认删除。
          </small>
        ) : (
          <div />
        )}
        <Button
          className="ghost"
          onClick={
            deleteConfirming ? () => setDeleteConfirming(false) : onClose
          }
        >
          {deleteConfirming ? "取消删除" : "取消"}
        </Button>
        <Button className="primary" disabled={busy} onClick={onSave}>
          保存修改
        </Button>
      </footer>
    </DialogSurface>
  );
}

function ClaudeProviderEditor(props: {
  error: string;
  modelListCleared: boolean;
  onClearModels: () => void;
  fusionEnabled: boolean;
  setFusionEnabled: (enabled: boolean) => void;
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
  providerAutoDisableResetPeriod: AutoDisableResetPeriod;
  providerAutoDisableResetWeekday: number;
  providerAutoDisableResetDay: number;
  providerAutoDisableResetTime: string;
  providerAutoDisableOverride: boolean;
  providerAutoDisableEnabled: boolean;
  providerAutoDisableFailureThreshold: number;
  globalAutoDisableEnabled: boolean;
  globalAutoDisableFailureThreshold: number;
  secretVisible: boolean;
  setAllowedModels: (models: string[]) => void;
  setModelMappings: (mappings: ModelMapping[]) => void;
  setProviderApiKey: (value: string) => void;
  setProviderApiKeyDirty: (dirty: boolean) => void;
  setProviderBaseUrl: (value: string) => void;
  setProviderName: (value: string) => void;
  setProviderAutoDisableResetPeriod: (value: AutoDisableResetPeriod) => void;
  setProviderAutoDisableResetWeekday: (value: number) => void;
  setProviderAutoDisableResetDay: (value: number) => void;
  setProviderAutoDisableResetTime: (value: string) => void;
  setProviderAutoDisableOverride: (value: boolean) => void;
  setProviderAutoDisableEnabled: (value: boolean) => void;
  setProviderAutoDisableFailureThreshold: (value: number) => void;
  setSecretVisible: (value: boolean) => void;
}) {
  const {
    error,
    modelListCleared,
    onClearModels,
    fusionEnabled,
    setFusionEnabled,
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
    providerAutoDisableResetPeriod,
    providerAutoDisableResetWeekday,
    providerAutoDisableResetDay,
    providerAutoDisableResetTime,
    providerAutoDisableOverride,
    providerAutoDisableEnabled,
    providerAutoDisableFailureThreshold,
    globalAutoDisableEnabled,
    globalAutoDisableFailureThreshold,
    secretVisible,
    setAllowedModels,
    setModelMappings,
    setProviderApiKey,
    setProviderApiKeyDirty,
    setProviderBaseUrl,
    setProviderName,
    setProviderAutoDisableResetPeriod,
    setProviderAutoDisableResetWeekday,
    setProviderAutoDisableResetDay,
    setProviderAutoDisableResetTime,
    setProviderAutoDisableOverride,
    setProviderAutoDisableEnabled,
    setProviderAutoDisableFailureThreshold,
    setSecretVisible,
  } = props;
  const [customAllowedModel, setCustomAllowedModel] = useState("");
  const [deleteConfirming, setDeleteConfirming] = useState(false);
  const [modelSearch, setModelSearch] = useState("");
  const updateAllowedModels = (models: string[]) =>
    setAllowedModels(normalizeModelNames(models));
  const allowedModelOptions = normalizeModelNames([
    ...providerModels,
    ...allowedModels,
  ]);
  const visibleModelOptions = allowedModelOptions.filter((model) =>
    model.toLowerCase().includes(modelSearch.trim().toLowerCase()),
  );
  const allModelsAllowed = allowedModels.length === 0 && !modelListCleared;
  const toggleAllowedModel = (model: string, checked: boolean) => {
    const currentModels = allModelsAllowed
      ? allowedModelOptions
      : allowedModels;
    updateAllowedModels(
      checked
        ? [...currentModels, model]
        : currentModels.filter(
            (current) => current.toLowerCase() !== model.toLowerCase(),
          ),
    );
  };
  const addCustomAllowedModel = () => {
    const model = customAllowedModel.trim();
    if (!model) return;
    updateAllowedModels([
      ...(allModelsAllowed ? allowedModelOptions : allowedModels),
      model,
    ]);
    setCustomAllowedModel("");
  };

  return (
    <DialogSurface
      drawer
      title="编辑 Claude 供应商"
      onClose={onClose}
      closeDisabled={busy}
    >
      <header>
        <div>
          <h2>编辑 Claude 供应商</h2>
          <p>{providerName || "未命名 Claude 供应商"}</p>
        </div>
        <Button className="close" disabled={busy} onClick={onClose}>
          ×
        </Button>
      </header>

      <section className="drawer-body">
        {error && (
          <div className="form-error" role="alert">
            {error}
          </div>
        )}
        <label className="field">
          <span>供应商名称</span>
          <TextField
            value={providerName}
            onChange={(event) => setProviderName(event.currentTarget.value)}
          />
        </label>
        <label className="field">
          <span>Base URL</span>
          <TextField
            placeholder="https://api.anthropic.com/v1"
            value={providerBaseUrl}
            onChange={(event) => setProviderBaseUrl(event.currentTarget.value)}
          />
          <small>
            按填写值原样拼接 /messages 与 /models；需要 /v1 的供应商请写到 Base
            URL 中。
          </small>
        </label>
        <label className="field">
          <span>API Key</span>
          <div className="secret-field">
            <TextField
              type={secretVisible ? "text" : "password"}
              value={providerApiKey}
              onChange={(event) => {
                setProviderApiKey(event.currentTarget.value);
                setProviderApiKeyDirty(true);
              }}
              placeholder="留空保持已保存的 API Key"
            />
            <Button
              onClick={() => setSecretVisible(!secretVisible)}
              type="button"
            >
              {secretVisible ? "隐藏" : "显示"}
            </Button>
          </div>
        </label>
        <label className="field">
          <span className="fusion-toggle">
            <Checkbox
              checked={fusionEnabled}
              onCheckedChange={(checked) => {
                const enabled = checked;
                setFusionEnabled(enabled);
                if (enabled && allowedModels.length === 0)
                  setAllowedModels(providerModels);
              }}
            />{" "}
            融合模式
          </span>
          <small>
            选定的模型将显示供应商名称，并仅发往此供应商。普通模型请求不会使用此供应商。请在模型配置中选择要发布的模型。
          </small>
        </label>
        <AutoDisablePolicyFields
          override={providerAutoDisableOverride}
          enabled={providerAutoDisableEnabled}
          threshold={providerAutoDisableFailureThreshold}
          globalEnabled={globalAutoDisableEnabled}
          globalThreshold={globalAutoDisableFailureThreshold}
          setOverride={setProviderAutoDisableOverride}
          setEnabled={setProviderAutoDisableEnabled}
          setThreshold={setProviderAutoDisableFailureThreshold}
        />
        <AutoDisableResetFields
          period={providerAutoDisableResetPeriod}
          resetDay={providerAutoDisableResetDay}
          resetTime={providerAutoDisableResetTime}
          resetWeekday={providerAutoDisableResetWeekday}
          setPeriod={setProviderAutoDisableResetPeriod}
          setResetDay={setProviderAutoDisableResetDay}
          setResetTime={setProviderAutoDisableResetTime}
          setResetWeekday={setProviderAutoDisableResetWeekday}
        />
        <section className="model-config-intro compact">
          <div>
            <span className="section-kicker">模型来源</span>
            <h3>模型配置</h3>
            <p>{providerBaseUrl || "尚未配置 Base URL"}</p>
          </div>
          <Button
            className="primary model-fetch-button"
            disabled={busy}
            onClick={onLoadProviderModels}
            type="button"
          >
            <ToolIcon type="models" />
            {busy
              ? "正在获取"
              : providerModels.length
                ? "重新获取"
                : "从上游获取模型"}
          </Button>
        </section>
        <div className="model-filter-box model-manager">
          <div className="mapping-box-head">
            <div>
              <strong>启用模型</strong>
              <p>
                {allowedModels.length
                  ? `已启用 ${allowedModels.length} 个 Claude 模型`
                  : modelListCleared
                    ? "已清空，请重新获取或添加模型"
                    : "当前兼容任意 Claude 模型"}
              </p>
            </div>
            <div className="model-list-actions">
              <Button
                className="ghost small"
                disabled={allowedModelOptions.length === 0}
                onClick={() => updateAllowedModels(allowedModelOptions)}
                type="button"
              >
                全部启用
              </Button>
              <Button
                className="ghost small"
                disabled={busy || allowedModelOptions.length === 0}
                onClick={() => {
                  onClearModels();
                  setModelSearch("");
                  setCustomAllowedModel("");
                }}
                type="button"
              >
                一键清空
              </Button>
            </div>
          </div>
          <label className="model-search-field">
            <span aria-hidden="true" />
            <TextField
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
                <Button
                  className="ghost small"
                  disabled={busy}
                  onClick={onLoadProviderModels}
                  type="button"
                >
                  从上游获取
                </Button>
              </div>
            )}
            {allowedModelOptions.length > 0 &&
              visibleModelOptions.length === 0 && (
                <p className="mapping-empty">
                  没有匹配“{modelSearch.trim()}”的模型。
                </p>
              )}
            {visibleModelOptions.map((model) => {
              const checked =
                allModelsAllowed ||
                allowedModels.some(
                  (current) => current.toLowerCase() === model.toLowerCase(),
                );
              return (
                <label
                  className={checked ? "model-option selected" : "model-option"}
                  key={model}
                >
                  <Checkbox
                    checked={checked}
                    onCheckedChange={(checked) =>
                      toggleAllowedModel(model, checked)
                    }
                  />
                  <span>{model}</span>
                </label>
              );
            })}
          </div>
          <div className="custom-model-row">
            <TextField
              placeholder="手动输入模型，例如 claude-sonnet-4-5"
              value={customAllowedModel}
              onChange={(event) =>
                setCustomAllowedModel(event.currentTarget.value)
              }
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  addCustomAllowedModel();
                }
              }}
            />
            <Button
              className="ghost small"
              disabled={!customAllowedModel.trim()}
              onClick={addCustomAllowedModel}
              type="button"
            >
              添加模型
            </Button>
          </div>
        </div>
        <div className="mapping-box">
          <div className="mapping-box-head">
            <div>
              <strong>模型映射</strong>
              <p>只替换 Anthropic 请求体顶层 model 字段</p>
            </div>
            <Button
              className="ghost small"
              onClick={() =>
                setModelMappings([...modelMappings, { source: "", target: "" }])
              }
              type="button"
            >
              添加映射
            </Button>
          </div>
          <div className="mapping-grid">
            <span>客户端模型</span>
            <span>上游模型</span>
            <span />
            {modelMappings.length === 0 && (
              <p className="mapping-empty">
                未配置映射时，请求模型将原样转发。
              </p>
            )}
            {modelMappings.map((mapping, index) => (
              <div className="mapping-row" key={index}>
                <TextField
                  placeholder="claude-sonnet-4-5"
                  value={mapping.source}
                  onChange={(event) => {
                    const next = [...modelMappings];
                    next[index] = {
                      ...mapping,
                      source: event.currentTarget.value,
                    };
                    setModelMappings(next);
                  }}
                />
                <TextField
                  placeholder="claude-3-5-sonnet-latest"
                  value={mapping.target}
                  onChange={(event) => {
                    const next = [...modelMappings];
                    next[index] = {
                      ...mapping,
                      target: event.currentTarget.value,
                    };
                    setModelMappings(next);
                  }}
                />
                <Button
                  aria-label="删除映射"
                  className="icon-button"
                  onClick={() =>
                    setModelMappings(
                      modelMappings.filter((_, rowIndex) => rowIndex !== index),
                    )
                  }
                  title="删除映射"
                  type="button"
                >
                  ×
                </Button>
              </div>
            ))}
          </div>
        </div>
      </section>

      <footer>
        <Button
          className="danger"
          disabled={busy}
          onClick={() =>
            deleteConfirming ? onDelete() : setDeleteConfirming(true)
          }
          type="button"
        >
          {deleteConfirming ? "确认删除" : "删除"}
        </Button>
        {deleteConfirming ? (
          <small className="danger-text">
            会保留历史请求日志；再次点击确认删除。
          </small>
        ) : (
          <div />
        )}
        <Button
          className="ghost"
          onClick={
            deleteConfirming ? () => setDeleteConfirming(false) : onClose
          }
        >
          {deleteConfirming ? "取消删除" : "取消"}
        </Button>
        <Button className="primary" disabled={busy} onClick={onSave}>
          保存修改
        </Button>
      </footer>
    </DialogSurface>
  );
}

export default App;
