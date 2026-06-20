import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

type ProviderConfig = {
  id: string;
  name: string;
  enabled: boolean;
  config: JsonValue;
  balance_query: BalanceQueryConfig;
  balance_status?: BalanceStatus | null;
};

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

type ProviderSummary = {
  id: string;
  name: string;
  enabled: boolean;
  pending_changes: number;
};

type ConfigRow = {
  path: string;
  value: JsonValue;
  source: string;
  changed: boolean;
};

type DiffEntry = {
  path: string;
  current?: JsonValue | null;
  desired?: JsonValue | null;
  action: string;
  source: string;
};

type AppState = {
  codex_config_path: string;
  manager_dir: string;
  current_config_raw: string;
  current_config_exists: boolean;
  active_provider_id: string;
  base_template_name: string;
  base_toml: string;
  base: JsonValue;
  providers: ProviderSummary[];
  active_provider: ProviderConfig | null;
  active_provider_toml: string;
  desired: JsonValue;
  final_preview_toml: string;
  summary: ConfigRow[];
  diffs: DiffEntry[];
  marker_present: boolean;
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

type UsageProviderPoint = {
  provider_key: string;
  provider_name: string;
  request_count: number;
  input_tokens: number;
  uncached_input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  reasoning_output_tokens: number;
  total_tokens: number;
  estimated_cost: number;
  known: boolean;
};

type UsageDailyPoint = {
  day: string;
  request_count: number;
  total_tokens: number;
  estimated_cost: number;
  providers: UsageProviderPoint[];
};

type UsageMonthlyPoint = {
  month: string;
  request_count: number;
  total_tokens: number;
  estimated_cost: number;
};

type UsageDetailRow = {
  timestamp: string;
  day: string;
  session_id: string;
  provider_key: string;
  provider_name: string;
  model: string;
  input_tokens: number;
  uncached_input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  reasoning_output_tokens: number;
  total_tokens: number;
  estimated_cost: number;
  cost_breakdown: string;
  pricing_model_match: string;
  pricing_source: string;
  currency: string;
  source: string;
};

type PricingRule = {
  id: string;
  provider_match: string;
  model_match: string;
  input_per_million: number;
  cached_input_per_million: number;
  output_per_million: number;
  reasoning_output_per_million: number;
  currency: string;
  source: string;
};

type UsageStatsFilter = {
  start_day?: string | null;
  end_day?: string | null;
  provider_key?: string | null;
  provider_name?: string | null;
  model?: string | null;
  page?: number | null;
  page_size?: number | null;
};

type UsageFilterOption = {
  provider_key: string;
  provider_name: string;
  request_count: number;
  known: boolean;
};

type UsageStats = {
  generated_at_ms: number;
  source_dir: string;
  filters: UsageStatsFilter;
  summary: UsageSummary;
  today: UsageSummary;
  this_month: UsageSummary;
  daily: UsageDailyPoint[];
  monthly: UsageMonthlyPoint[];
  providers: UsageProviderPoint[];
  details: UsageDetailRow[];
  pricing: PricingRule[];
  available_providers: UsageFilterOption[];
  available_models: string[];
  available_days: string[];
  unknown_provider_count: number;
  parsed_files: number;
  parsed_events: number;
  filtered_events: number;
  detail_page: number;
  detail_page_size: number;
  detail_total_pages: number;
};

type Screen = "main" | "create" | "edit" | "current" | "settings" | "usage";
type PreviewMode = "provider" | "current";

function valueToTomlScalar(value: JsonValue): string {
  if (typeof value === "string") return JSON.stringify(value);
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (Array.isArray(value)) return `[${value.map(valueToTomlScalar).join(", ")}]`;
  return JSON.stringify(value);
}

function objectToToml(value: JsonValue, prefix = ""): string {
  if (!isObject(value)) return "";
  const scalars: string[] = [];
  const tables: string[] = [];

  for (const [key, child] of Object.entries(value)) {
    if (isObject(child)) {
      const tableName = prefix ? `${prefix}.${key}` : key;
      const body = objectToToml(child, tableName);
      tables.push(`[${tableName}]\n${body}`);
    } else {
      scalars.push(`${key} = ${valueToTomlScalar(child)}`);
    }
  }

  return [...scalars, ...tables].filter(Boolean).join("\n") + (scalars.length || tables.length ? "\n" : "");
}

function parseTomlScalar(raw: string): JsonValue {
  const value = raw.trim();
  if (value === "true") return true;
  if (value === "false") return false;
  if (/^-?\d+(\.\d+)?$/.test(value)) return Number(value);
  if (value.startsWith('"') && value.endsWith('"')) {
    try {
      return JSON.parse(value);
    } catch {
      return value.slice(1, -1);
    }
  }
  return value;
}

function mockTomlToObject(tomlText: string): JsonValue {
  const root: Record<string, JsonValue> = {};
  let current: Record<string, JsonValue> = root;

  for (const line of tomlText.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;

    const table = trimmed.match(/^\[([^\]]+)\]$/);
    if (table) {
      current = root;
      for (const part of table[1].split(".")) {
        const next = current[part];
        if (!isObject(next)) current[part] = {};
        current = current[part] as Record<string, JsonValue>;
      }
      continue;
    }

    const pair = trimmed.match(/^([A-Za-z0-9_-]+)\s*=\s*(.*)$/);
    if (pair) {
      current[pair[1]] = parseTomlScalar(pair[2]);
    }
  }

  return root;
}

function parseCustomProviderFields(tomlText: string) {
  const sectionMatch = tomlText.match(/\[model_providers\.custom\]([\s\S]*?)(?=\n\[|$)/);
  const section = sectionMatch?.[1] ?? "";
  const baseUrl = section.match(/^\s*base_url\s*=\s*"([^"]*)"/m)?.[1] ?? "";
  const token =
    section.match(/^\s*experimental_bearer_token\s*=\s*"([^"]*)"/m)?.[1] ?? "";
  return { baseUrl, token };
}

function upsertTomlScalar(tomlText: string, key: string, value: string) {
  const line = `${key} = ${JSON.stringify(value)}`;
  const pattern = new RegExp(`^\\s*${key}\\s*=.*$`, "m");
  if (pattern.test(tomlText)) {
    return tomlText.replace(pattern, line);
  }
  return `${line}\n${tomlText}`.trimEnd() + "\n";
}

function upsertTomlSectionValue(
  tomlText: string,
  sectionName: string,
  key: string,
  value: string,
) {
  const line = `${key} = ${JSON.stringify(value)}`;
  const sectionPattern = new RegExp(`(\\[${sectionName.replace(".", "\\.")}\\]\\n)([\\s\\S]*?)(?=\\n\\[|$)`);
  const match = tomlText.match(sectionPattern);

  if (!match) {
    return `${tomlText.trimEnd()}\n\n[${sectionName}]\n${line}\n`;
  }

  const body = match[2];
  const keyPattern = new RegExp(`^\\s*${key}\\s*=.*$`, "m");
  const nextBody = keyPattern.test(body)
    ? body.replace(keyPattern, line)
    : `${body.trimEnd()}\n${line}\n`;

  return tomlText.replace(sectionPattern, `${match[1]}${nextBody}`);
}

function buildCustomProviderToml(baseUrl: string, token: string) {
  return [
    'model_provider = "custom"',
    "",
    "[model_providers.custom]",
    `base_url = ${JSON.stringify(baseUrl)}`,
    `experimental_bearer_token = ${JSON.stringify(token)}`,
    "",
  ].join("\n");
}

function buildVisibleCustomProviderToml(baseUrl: string, token: string, tokenVisible: boolean) {
  return buildCustomProviderToml(baseUrl, tokenVisible || !token ? token : "********");
}

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

function defaultBalancePath(
  queryType: BalanceQueryType,
  newApiTarget: NewApiBalanceTarget = "token_quota",
) {
  if (queryType === "sub2_api") return "/v1/usage";
  if (queryType === "new_api" && newApiTarget === "account_balance") {
    return "/api/user/self";
  }
  return "/api/usage/token/";
}

function endpointFromBaseUrl(baseUrl: string) {
  return baseUrl.trim().replace(/\/v1\/?$/, "").replace(/\/$/, "");
}

function normalizeBalanceQuery(config?: BalanceQueryConfig | null, endpoint = "") {
  const queryType = config?.query_type ?? "disabled";
  const newApiTarget = config?.new_api_target ?? "token_quota";
  const isLegacyNewApiPath =
    queryType === "new_api" &&
    newApiTarget === "token_quota" &&
    config?.path === "/api/user/self" &&
    !config?.new_api_user_id;
  const normalized = {
    ...defaultBalanceQuery(endpoint),
    ...(config ?? {}),
    endpoint: config?.endpoint || endpoint,
    path: isLegacyNewApiPath
      ? defaultBalancePath(queryType, newApiTarget)
      : config?.path || defaultBalancePath(queryType, newApiTarget),
  };

  if (normalized.query_type === "new_api" && normalized.new_api_target === "account_balance") {
    normalized.auth_mode = "separate_token";
  }

  return normalized;
}

function balanceChipLabel(provider: ProviderConfig | null | undefined) {
  const query = provider?.balance_query;
  if (!provider) return "未选择";
  if (!query?.enabled || query.query_type === "disabled") return "未配置";
  return provider.balance_status?.label ?? "未查询";
}

function balanceChipActionLabel(provider: ProviderConfig | null | undefined) {
  const query = provider?.balance_query;
  if (!provider) return "设置";
  if (!query?.enabled || query.query_type === "disabled") return "设置";
  if (provider.balance_status?.error) return "重试";
  if (!provider.balance_status) return "查询";
  return "刷新";
}

function defaultPricingRules(): PricingRule[] {
  return [
    {
      id: "gpt-5-5",
      provider_match: "*",
      model_match: "gpt-5.5*",
      input_per_million: 5,
      cached_input_per_million: 0.5,
      output_per_million: 30,
      reasoning_output_per_million: 0,
      currency: "USD",
      source: "OpenAI API pricing, standard GPT models, USD per 1M tokens",
    },
    {
      id: "gpt-5-4",
      provider_match: "*",
      model_match: "gpt-5.4*",
      input_per_million: 2.5,
      cached_input_per_million: 0.25,
      output_per_million: 15,
      reasoning_output_per_million: 0,
      currency: "USD",
      source: "OpenAI API pricing, standard GPT models, USD per 1M tokens",
    },
    {
      id: "gpt-5",
      provider_match: "*",
      model_match: "gpt-5*",
      input_per_million: 1.25,
      cached_input_per_million: 0.125,
      output_per_million: 10,
      reasoning_output_per_million: 0,
      currency: "USD",
      source: "OpenAI API pricing, standard GPT models, USD per 1M tokens",
    },
  ];
}

function formatInteger(value: number) {
  return Math.round(value || 0).toLocaleString("zh-CN");
}

function formatCompactNumber(value: number) {
  const abs = Math.abs(value || 0);
  if (abs >= 1_000_000) return `${(value / 1_000_000).toFixed(abs >= 10_000_000 ? 1 : 2)}M`;
  if (abs >= 1_000) return `${(value / 1_000).toFixed(abs >= 100_000 ? 0 : 1)}K`;
  return formatInteger(value);
}

function formatMoney(value: number, currency = "USD") {
  const prefix = currency.toUpperCase() === "USD" ? "$" : `${currency} `;
  if (Math.abs(value) < 0.0001) return `${prefix}0.0000`;
  if (Math.abs(value) < 0.01) return `${prefix}${value.toFixed(4)}`;
  return `${prefix}${value.toFixed(2)}`;
}

function formatDateInput(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function usageRangeFilter(range: "today" | "7d" | "30d" | "month" | "all"): UsageStatsFilter {
  const now = new Date();
  if (range === "all") return {};
  if (range === "today") {
    const today = formatDateInput(now);
    return { start_day: today, end_day: today };
  }
  if (range === "month") {
    const start = new Date(now.getFullYear(), now.getMonth(), 1);
    return { start_day: formatDateInput(start), end_day: formatDateInput(now) };
  }

  const days = range === "7d" ? 6 : 29;
  const start = new Date(now);
  start.setDate(now.getDate() - days);
  return { start_day: formatDateInput(start), end_day: formatDateInput(now) };
}

function usageFilterPayload(filter: UsageStatsFilter): UsageStatsFilter {
  const payload: UsageStatsFilter = {};
  if (filter.start_day) payload.start_day = filter.start_day;
  if (filter.end_day) payload.end_day = filter.end_day;
  if (filter.provider_key) payload.provider_key = filter.provider_key;
  if (filter.provider_name) payload.provider_name = filter.provider_name;
  if (filter.model) payload.model = filter.model;
  if (filter.page) payload.page = filter.page;
  if (filter.page_size) payload.page_size = filter.page_size;
  return payload;
}

function usageRangeLabel(filter: UsageStatsFilter) {
  if (filter.start_day && filter.end_day) {
    return filter.start_day === filter.end_day
      ? filter.start_day
      : `${filter.start_day} 至 ${filter.end_day}`;
  }
  if (filter.start_day) return `${filter.start_day} 之后`;
  if (filter.end_day) return `${filter.end_day} 之前`;
  return "全部时间";
}

function providerFilterValue(filter: UsageStatsFilter) {
  if (!filter.provider_key) return "";
  return `${filter.provider_key}\u0000${filter.provider_name ?? ""}`;
}

function parseProviderFilterValue(value: string): Pick<UsageStatsFilter, "provider_key" | "provider_name"> {
  if (!value) {
    return { provider_key: null, provider_name: null };
  }
  const [providerKey, providerName = ""] = value.split("\u0000");
  return {
    provider_key: providerKey || null,
    provider_name: providerName || null,
  };
}

function usagePageNumbers(page: number, totalPages: number) {
  const pages = new Set<number>([1, totalPages, page - 1, page, page + 1]);
  return Array.from(pages)
    .filter((value) => value >= 1 && value <= totalPages)
    .sort((left, right) => left - right);
}

function costTooltipForSummary(summary: UsageSummary | null | undefined, label: string) {
  if (!summary) return "";
  return `${label}\n输入（未缓存）: ${formatInteger(
    summary.uncached_input_tokens,
  )}\n输入（已缓存）: ${formatInteger(
    summary.cached_input_tokens,
  )}\n输出: ${formatInteger(summary.output_tokens)}\n推理输出: ${formatInteger(
    summary.reasoning_output_tokens,
  )}\n金额: ${formatMoney(summary.estimated_cost, summary.currency)}`;
}

function syncCustomProviderToml(tomlText: string, baseUrl: string, token: string) {
  let next = tomlText;
  next = upsertTomlScalar(next, "model_provider", "custom");
  next = upsertTomlSectionValue(next, "model_providers.custom", "base_url", baseUrl);
  next = upsertTomlSectionValue(
    next,
    "model_providers.custom",
    "experimental_bearer_token",
    token,
  );
  return next;
}

function formatValue(value: JsonValue | undefined | null) {
  if (value === undefined || value === null) {
    return "-";
  }
  if (typeof value === "string") {
    return value;
  }
  return JSON.stringify(value);
}

function isSecretPath(path: string) {
  const normalized = path.toLowerCase();
  return (
    normalized.includes("token") ||
    normalized.includes("api_key") ||
    normalized.endsWith("_key") ||
    normalized.includes("secret") ||
    normalized.includes("password")
  );
}

function pathLabel(path: string) {
  if (path.length <= 42) {
    return path;
  }
  return `${path.slice(0, 20)}...${path.slice(-18)}`;
}

function sourceClass(source: string) {
  if (source === "供应商") return "source provider";
  if (source === "基础模板") return "source base";
  return "source";
}

function highlightToml(tomlText: string, diffs: DiffEntry[]) {
  const changedKeys = new Set(
    diffs.map((diff) => {
      const parts = diff.path.split(".");
      return parts[parts.length - 1];
    }),
  );
  return tomlText.split("\n").map((line, index) => {
    const key = line.match(/^\s*([A-Za-z0-9_-]+)\s*=/)?.[1];
    const changed = key ? changedKeys.has(key) : false;
    return (
      <span className={changed ? "line-highlight" : undefined} key={`${line}-${index}`}>
        {line || " "}
        {"\n"}
      </span>
    );
  });
}

const isTauriRuntime = "__TAURI_INTERNALS__" in window;

const mockBase: JsonValue = {};

let mockProviderStore: ProviderConfig[] = [];
const mockDetails = Array.from({ length: 26 }, (_, index): UsageDetailRow => {
  const inputTokens = 180_000 + index * 9_300;
  const cachedInputTokens = Math.round(inputTokens * 0.32);
  const outputTokens = 18_000 + index * 1_120;
  const reasoningOutputTokens = Math.round(outputTokens * 0.42);
  const totalTokens = inputTokens + outputTokens;
  const estimatedCost =
    ((inputTokens - cachedInputTokens) / 1_000_000) * 1.25 +
    (cachedInputTokens / 1_000_000) * 0.125 +
    (outputTokens / 1_000_000) * 10;
  return {
    timestamp: `2026-06-${String(10 + Math.floor(index / 3)).padStart(2, "0")} ${String(
      9 + (index % 9),
    ).padStart(2, "0")}:24:16`,
    day: `2026-06-${String(10 + Math.floor(index / 3)).padStart(2, "0")}`,
    session_id: `mock-session-${index + 1}`,
    provider_key: index % 3 === 0 ? "openai" : "custom",
    provider_name: index % 3 === 0 ? "OpenAI" : "Hlongyu API",
    model: index % 4 === 0 ? "gpt-5.3-codex" : "gpt-5",
    input_tokens: inputTokens,
    uncached_input_tokens: inputTokens - cachedInputTokens,
    cached_input_tokens: cachedInputTokens,
    output_tokens: outputTokens,
    reasoning_output_tokens: reasoningOutputTokens,
    total_tokens: totalTokens,
    estimated_cost: estimatedCost,
    cost_breakdown: `模型匹配: gpt-5*\n输入: ${
      inputTokens - cachedInputTokens
    } tokens × $1.2500/1M\n缓存输入: ${cachedInputTokens} tokens × $0.1250/1M\n输出: ${outputTokens} tokens × $10.0000/1M\n推理输出: ${reasoningOutputTokens} tokens，作为输出细分展示，不重复计费\n合计: ${formatMoney(
      estimatedCost,
      "USD",
    )}`,
    pricing_model_match: "gpt-5*",
    pricing_source: "OpenAI API pricing, standard GPT models, USD per 1M tokens",
    currency: "USD",
    source: "~/.codex/sessions/mock.jsonl",
  };
});

let mockUsageStats: UsageStats = {
  generated_at_ms: Date.now(),
  source_dir: "~/.codex/sessions",
  filters: {},
  summary: {
    request_count: 654,
    input_tokens: 183_420_000,
    uncached_input_tokens: 121_320_000,
    cached_input_tokens: 62_100_000,
    output_tokens: 14_560_000,
    reasoning_output_tokens: 7_880_000,
    total_tokens: 197_980_000,
    estimated_cost: 0,
    currency: "USD",
  },
  today: {
    request_count: 179,
    input_tokens: 48_200_000,
    uncached_input_tokens: 29_780_000,
    cached_input_tokens: 18_420_000,
    output_tokens: 3_900_000,
    reasoning_output_tokens: 1_900_000,
    total_tokens: 52_100_000,
    estimated_cost: 0,
    currency: "USD",
  },
  this_month: {
    request_count: 654,
    input_tokens: 183_420_000,
    uncached_input_tokens: 121_320_000,
    cached_input_tokens: 62_100_000,
    output_tokens: 14_560_000,
    reasoning_output_tokens: 7_880_000,
    total_tokens: 197_980_000,
    estimated_cost: 0,
    currency: "USD",
  },
  daily: Array.from({ length: 10 }, (_, index) => {
    const day = `2026-06-${String(index + 12).padStart(2, "0")}`;
    const requestCount = [42, 58, 51, 83, 76, 92, 69, 110, 88, 179][index];
    const totalTokens = requestCount * 290_000;
    return {
      day,
      request_count: requestCount,
      total_tokens: totalTokens,
      estimated_cost: 0,
      providers: [
        {
          provider_key: "custom",
          provider_name: "Hlongyu API",
          request_count: Math.round(requestCount * 0.72),
          input_tokens: Math.round(totalTokens * 0.66),
          uncached_input_tokens: Math.round(totalTokens * 0.48),
          cached_input_tokens: Math.round(totalTokens * 0.18),
          output_tokens: Math.round(totalTokens * 0.06),
          reasoning_output_tokens: Math.round(totalTokens * 0.03),
          total_tokens: Math.round(totalTokens * 0.72),
          estimated_cost: 0,
          known: false,
        },
        {
          provider_key: "openai",
          provider_name: "OpenAI",
          request_count: Math.round(requestCount * 0.28),
          input_tokens: Math.round(totalTokens * 0.25),
          uncached_input_tokens: Math.round(totalTokens * 0.19),
          cached_input_tokens: Math.round(totalTokens * 0.06),
          output_tokens: Math.round(totalTokens * 0.03),
          reasoning_output_tokens: Math.round(totalTokens * 0.012),
          total_tokens: Math.round(totalTokens * 0.28),
          estimated_cost: 0,
          known: true,
        },
      ],
    };
  }),
  monthly: [
    { month: "2026-04", request_count: 1721, total_tokens: 42_000_000, estimated_cost: 0 },
    { month: "2026-05", request_count: 2680, total_tokens: 91_000_000, estimated_cost: 0 },
    { month: "2026-06", request_count: 654, total_tokens: 197_980_000, estimated_cost: 0 },
  ],
  providers: [
    {
      provider_key: "custom",
      provider_name: "Hlongyu API",
      request_count: 471,
      input_tokens: 122_000_000,
      uncached_input_tokens: 79_000_000,
      cached_input_tokens: 43_000_000,
      output_tokens: 20_000_000,
      reasoning_output_tokens: 8_600_000,
      total_tokens: 142_000_000,
      estimated_cost: 0,
      known: false,
    },
    {
      provider_key: "openai",
      provider_name: "OpenAI",
      request_count: 183,
      input_tokens: 48_000_000,
      uncached_input_tokens: 34_000_000,
      cached_input_tokens: 14_000_000,
      output_tokens: 7_980_000,
      reasoning_output_tokens: 3_200_000,
      total_tokens: 55_980_000,
      estimated_cost: 0,
      known: true,
    },
  ],
  details: mockDetails,
  pricing: defaultPricingRules(),
  available_providers: [
    { provider_key: "custom", provider_name: "Hlongyu API", request_count: 471, known: true },
    { provider_key: "openai", provider_name: "OpenAI", request_count: 183, known: true },
  ],
  available_models: ["gpt-5", "gpt-5.3-codex"],
  available_days: ["2026-06-10", "2026-06-11", "2026-06-12", "2026-06-13", "2026-06-14"],
  unknown_provider_count: 1,
  parsed_files: 12,
  parsed_events: 654,
  filtered_events: 654,
  detail_page: 1,
  detail_page_size: 20,
  detail_total_pages: 2,
};

let mockState = buildMockState({
  activeProviderId: "",
  base: mockBase,
  baseTemplateName: "未设置",
  providers: mockProviderStore,
  markerPresent: false,
});

function isObject(value: JsonValue): value is Record<string, JsonValue> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function mergeJson(base: JsonValue, overlay: JsonValue): JsonValue {
  if (!isObject(base) || !isObject(overlay)) {
    return overlay;
  }

  const output: Record<string, JsonValue> = { ...base };
  for (const [key, value] of Object.entries(overlay)) {
    output[key] = key in output ? mergeJson(output[key], value) : value;
  }
  return output;
}

function flattenJson(value: JsonValue, prefix = ""): Record<string, JsonValue> {
  if (!isObject(value)) {
    return prefix ? { [prefix]: value } : {};
  }

  const out: Record<string, JsonValue> = {};
  for (const [key, child] of Object.entries(value)) {
    Object.assign(out, flattenJson(child, prefix ? `${prefix}.${key}` : key));
  }
  return out;
}

function sourceForPath(base: JsonValue, provider: ProviderConfig | null, path: string) {
  if (provider && path in flattenJson(provider.config)) return "供应商";
  if (path in flattenJson(base)) return "基础模板";
  return "当前配置";
}

function buildMockState(input: {
  activeProviderId: string;
  baseTemplateName: string;
  base: JsonValue;
  providers: ProviderConfig[];
  markerPresent: boolean;
}): AppState {
  const activeProvider =
    input.providers.find((provider) => provider.id === input.activeProviderId) ??
    input.providers[0] ??
    null;
  const desired = activeProvider
    ? mergeJson(input.base, activeProvider.config)
    : input.base;
  const flat = flattenJson(desired);
  const diffs: DiffEntry[] = Object.entries(flat).slice(0, 4).map(([path, desired]) => ({
    path,
    current: path === "model" ? "gpt-4.1-codex" : null,
    desired,
    action: path === "model" ? "更新" : "新增",
    source: sourceForPath(input.base, activeProvider, path),
  }));

  return {
    codex_config_path: "~/.codex/config.toml",
    manager_dir: "~/.codex/config-manager",
    current_config_raw:
      'approval_policy = "on-request"\nsandbox_mode = "read-only"\nmodel_provider = "openai"\nmodel = "gpt-4.1-codex"\n',
    current_config_exists: true,
    active_provider_id: activeProvider?.id ?? "",
    base_template_name: input.baseTemplateName,
    base_toml: objectToToml(input.base),
    base: input.base,
    providers: input.providers.map((provider) => ({
      id: provider.id,
      name: provider.name,
      enabled: provider.enabled,
      pending_changes: provider.id === activeProvider?.id ? diffs.length : 0,
    })),
    active_provider: activeProvider,
    active_provider_toml: activeProvider ? objectToToml(activeProvider.config) : "",
    desired,
    final_preview_toml: objectToToml(desired),
    summary: Object.entries(flat).map(([path, value]) => ({
      path,
      value,
      source: sourceForPath(input.base, activeProvider, path),
      changed: diffs.some((diff) => diff.path === path),
    })),
    diffs,
    marker_present: input.markerPresent,
  };
}

async function callCommand<T>(command: string, args?: Record<string, unknown>) {
  if (isTauriRuntime) {
    return invoke<T>(command, args);
  }

  if (command === "load_app_state") {
    return mockState as T;
  }

  if (command === "select_provider") {
    const providerId = String(args?.providerId);
    mockState = buildMockState({
      activeProviderId: providerId,
      base: mockState.base,
      baseTemplateName: mockState.base_template_name,
      providers: mockProviderStore.map((provider) => ({
        id: provider.id,
        name: provider.name,
        enabled: provider.enabled,
        balance_query: provider.balance_query,
        balance_status: provider.balance_status,
        config: provider.config,
      })),
      markerPresent: mockState.marker_present,
    });
    return mockState as T;
  }

  if (command === "save_provider") {
    const payload = args?.payload as {
      provider_id: string;
      provider_name?: string;
      config_toml: string;
      balance_query?: BalanceQueryConfig;
    };
    mockProviderStore = mockProviderStore.map((provider) => ({
      id: provider.id,
      name:
        provider.id === payload.provider_id && payload.provider_name
          ? payload.provider_name
          : provider.name,
      enabled: provider.enabled,
      balance_query:
        provider.id === payload.provider_id && payload.balance_query
          ? payload.balance_query
          : provider.balance_query,
      balance_status: provider.balance_status,
      config:
        provider.id === payload.provider_id
          ? mockTomlToObject(payload.config_toml)
          : provider.config,
    }));
    mockState = buildMockState({
      activeProviderId: payload.provider_id,
      base: mockState.base,
      baseTemplateName: mockState.base_template_name,
      providers: mockProviderStore,
      markerPresent: mockState.marker_present,
    });
    return mockState as T;
  }

  if (command === "preview_provider") {
    const payload = args?.payload as {
      provider_id: string;
      provider_name?: string;
      config_toml: string;
      balance_query?: BalanceQueryConfig;
    };
    const previewProviders = mockProviderStore.map((provider) => ({
      id: provider.id,
      name:
        provider.id === payload.provider_id && payload.provider_name
          ? payload.provider_name
          : provider.name,
      enabled: provider.enabled,
      balance_query:
        provider.id === payload.provider_id && payload.balance_query
          ? payload.balance_query
          : provider.balance_query,
      balance_status: provider.balance_status,
      config:
        provider.id === payload.provider_id
          ? mockTomlToObject(payload.config_toml)
          : provider.config,
    }));
    return buildMockState({
      activeProviderId: payload.provider_id,
      base: mockState.base,
      baseTemplateName: mockState.base_template_name,
      providers: previewProviders,
      markerPresent: mockState.marker_present,
    }) as T;
  }

  if (command === "add_provider") {
    const name = String(args?.name ?? "新供应商");
    const id = name.toLowerCase().replace(/[^a-z0-9]+/g, "-") || "provider";
    mockProviderStore = [
      ...mockProviderStore,
      {
        id,
        name,
        enabled: false,
        balance_query: defaultBalanceQuery(),
        balance_status: null,
        config: mockTomlToObject(
          'model_provider = "custom"\n\n[model_providers.custom]\nbase_url = ""\nexperimental_bearer_token = ""\n',
        ),
      },
    ];
    mockState = buildMockState({
      activeProviderId: id,
      base: mockState.base,
      baseTemplateName: mockState.base_template_name,
      providers: mockProviderStore,
      markerPresent: mockState.marker_present,
    });
    return mockState as T;
  }

  if (command === "save_base_template") {
    const payload = args?.payload as { base_template_name: string; base_toml: string };
    mockState = buildMockState({
      activeProviderId: mockState.active_provider?.id ?? "",
      base: mockTomlToObject(payload.base_toml),
      baseTemplateName: payload.base_template_name,
      providers: mockProviderStore,
      markerPresent: mockState.marker_present,
    });
    return mockState as T;
  }

  if (command === "apply_config") {
    const appliedId = mockState.active_provider?.id ?? "";
    mockProviderStore = mockProviderStore.map((provider) => ({
      ...provider,
      enabled: provider.id === appliedId,
    }));
    mockState = buildMockState({
      activeProviderId: appliedId,
      base: mockState.base,
      baseTemplateName: mockState.base_template_name,
      providers: mockProviderStore,
      markerPresent: true,
    });
    mockState = { ...mockState, diffs: [] };
    return mockState as T;
  }

  if (command === "query_provider_balance") {
    const providerId = String((args?.payload as { provider_id?: string })?.provider_id ?? "");
    mockProviderStore = mockProviderStore.map((provider) => ({
      ...provider,
      balance_status:
        provider.id === providerId
          ? {
              amount: "23.41",
              label: "余额 ¥ 23.41",
              checked_at: Math.floor(Date.now() / 1000),
              error: null,
            }
          : provider.balance_status,
    }));
    mockState = buildMockState({
      activeProviderId: mockState.active_provider?.id ?? providerId,
      base: mockState.base,
      baseTemplateName: mockState.base_template_name,
      providers: mockProviderStore,
      markerPresent: mockState.marker_present,
    });
    return mockState as T;
  }

  if (command === "load_usage_stats") {
    const payload = (args?.payload as
      | { filter?: UsageStatsFilter; force_refresh?: boolean }
      | undefined) ?? {};
    const filter = (payload.filter ?? {}) as UsageStatsFilter;
    const filteredDetails = mockDetails.filter((row) => {
      if (filter.start_day && row.day < filter.start_day) return false;
      if (filter.end_day && row.day > filter.end_day) return false;
      if (filter.provider_key && row.provider_key !== filter.provider_key) return false;
      if (filter.provider_name && row.provider_name !== filter.provider_name) return false;
      if (filter.model && row.model !== filter.model) return false;
      return true;
    });
    const summary = filteredDetails.reduce<UsageSummary>(
      (total, row) => ({
        ...total,
        request_count: total.request_count + 1,
        input_tokens: total.input_tokens + row.input_tokens,
        uncached_input_tokens: total.uncached_input_tokens + row.uncached_input_tokens,
        cached_input_tokens: total.cached_input_tokens + row.cached_input_tokens,
        output_tokens: total.output_tokens + row.output_tokens,
        reasoning_output_tokens: total.reasoning_output_tokens + row.reasoning_output_tokens,
        total_tokens: total.total_tokens + row.total_tokens,
        estimated_cost: total.estimated_cost + row.estimated_cost,
      }),
      {
        request_count: 0,
        input_tokens: 0,
        uncached_input_tokens: 0,
        cached_input_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
        total_tokens: 0,
        estimated_cost: 0,
        currency: "USD",
      },
    );
    const pageSize = Number(filter.page_size) || 20;
    const totalPages = Math.max(1, Math.ceil(filteredDetails.length / pageSize));
    const page = Math.min(Math.max(Number(filter.page) || 1, 1), totalPages);
    const start = (page - 1) * pageSize;
    const details = filteredDetails.slice(start, start + pageSize);
    return {
      ...mockUsageStats,
      filters: { ...filter, page, page_size: pageSize },
      summary,
      details,
      filtered_events: filteredDetails.length,
      detail_page: page,
      detail_page_size: pageSize,
      detail_total_pages: totalPages,
      generated_at_ms: Date.now(),
    } as T;
  }

  throw new Error(`未实现的模拟命令: ${command}`);
}

function App() {
  const [appState, setAppState] = useState<AppState | null>(null);
  const [screen, setScreen] = useState<Screen>(() => {
    if (window.location.hash === "#edit") return "edit";
    if (window.location.hash === "#current") return "current";
    if (window.location.hash === "#settings") return "settings";
    return "main";
  });
  const [providerName, setProviderName] = useState("");
  const [providerText, setProviderText] = useState("");
  const [customBaseUrl, setCustomBaseUrl] = useState("");
  const [customToken, setCustomToken] = useState("");
  const [customTokenVisible, setCustomTokenVisible] = useState(false);
  const [balanceQuery, setBalanceQuery] = useState<BalanceQueryConfig>(defaultBalanceQuery());
  const [balanceTokenVisible, setBalanceTokenVisible] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [previewState, setPreviewState] = useState<AppState | null>(null);
  const [previewMode, setPreviewMode] = useState<PreviewMode>("provider");
  const [previewExpanded, setPreviewExpanded] = useState(false);
  const [visibleSummarySecrets, setVisibleSummarySecrets] = useState<Record<string, boolean>>({});
  const [newProviderName, setNewProviderName] = useState("");
  const [newBaseUrl, setNewBaseUrl] = useState("https://code.xxcd.top/v1");
  const [newToken, setNewToken] = useState("");
  const [newTokenVisible, setNewTokenVisible] = useState(false);
  const [selectAfterCreate, setSelectAfterCreate] = useState(true);
  const [baseText, setBaseText] = useState("");
  const [baseName, setBaseName] = useState("默认模板");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [balanceBusy, setBalanceBusy] = useState(false);
  const [usageStats, setUsageStats] = useState<UsageStats | null>(null);
  const [usageBusy, setUsageBusy] = useState(false);
  const [usageRefreshing, setUsageRefreshing] = useState(false);
  const [usageRange, setUsageRange] = useState<"today" | "7d" | "30d" | "month" | "all" | "custom">("30d");
  const [usageFilter, setUsageFilter] = useState<UsageStatsFilter>(() => ({
    ...usageRangeFilter("30d"),
    page: 1,
    page_size: 20,
  }));

  async function refresh() {
    setError("");
    const state = await callCommand<AppState>("load_app_state");
    setAppState(state);
    setProviderName(state.active_provider?.name ?? "");
    setProviderText(state.active_provider_toml);
    const custom = parseCustomProviderFields(state.active_provider_toml);
    setCustomBaseUrl(custom.baseUrl);
    setCustomToken(custom.token);
    setBalanceQuery(
      normalizeBalanceQuery(
        state.active_provider?.balance_query,
        endpointFromBaseUrl(custom.baseUrl),
      ),
    );
    setBaseName(state.base_template_name);
    setVisibleSummarySecrets({});
    setPreviewState(null);
  }

  useEffect(() => {
    refresh().catch((err) => setError(String(err)));
  }, []);

  useEffect(() => {
    if (!appState) return;
    setProviderName(appState.active_provider?.name ?? "");
    setProviderText(appState.active_provider_toml);
    const custom = parseCustomProviderFields(appState.active_provider_toml);
    setCustomBaseUrl(custom.baseUrl);
    setCustomToken(custom.token);
    setCustomTokenVisible(false);
    setBalanceTokenVisible(false);
    setBalanceQuery(
      normalizeBalanceQuery(
        appState.active_provider?.balance_query,
        endpointFromBaseUrl(custom.baseUrl),
      ),
    );
    setVisibleSummarySecrets({});
    setPreviewState(null);
  }, [appState?.active_provider?.id]);

  const importantRows = useMemo(() => {
    if (!appState) return [];
    const preferred = [
      "model_provider",
      "model",
      "approval_policy",
      "sandbox_mode",
      "model_reasoning_effort",
      "model_providers.custom.base_url",
      "model_providers.custom.experimental_bearer_token",
    ];

    return [
      ...appState.summary.filter((row) => preferred.includes(row.path)),
      ...appState.summary.filter((row) => !preferred.includes(row.path)),
    ].slice(0, 8);
  }, [appState]);

  async function run<T>(action: () => Promise<T>) {
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

  async function selectProvider(providerId: string) {
    await run(async () => {
      const state = await callCommand<AppState>("select_provider", { providerId });
      setAppState(state);
      setPreviewState(null);
    });
  }

  function closePreview() {
    setPreviewState(null);
    setPreviewExpanded(false);
  }

  function updateProviderName(name: string) {
    setProviderName(name);
    closePreview();
  }

  function updateCustomBaseUrl(baseUrl: string) {
    const previousEndpoint = endpointFromBaseUrl(customBaseUrl);
    const nextEndpoint = endpointFromBaseUrl(baseUrl);
    setCustomBaseUrl(baseUrl);
    setBalanceQuery((current) => ({
      ...current,
      endpoint:
        !current.endpoint || current.endpoint === previousEndpoint
          ? nextEndpoint
          : current.endpoint,
    }));
    setProviderText((current) => syncCustomProviderToml(current, baseUrl, customToken));
    closePreview();
  }

  function updateCustomToken(token: string) {
    setCustomToken(token);
    setProviderText((current) => syncCustomProviderToml(current, customBaseUrl, token));
    closePreview();
  }

  function updateProviderToml(tomlText: string) {
    setProviderText(tomlText);
    const custom = parseCustomProviderFields(tomlText);
    setCustomBaseUrl(custom.baseUrl);
    setCustomToken(custom.token);
    closePreview();
  }

  function toggleSummarySecret(path: string) {
    setVisibleSummarySecrets((current) => ({
      ...current,
      [path]: !current[path],
    }));
  }

  function updateBalanceQuery(patch: Partial<BalanceQueryConfig>) {
    setBalanceQuery((current) => {
      const next = { ...current, ...patch };
      const previousDefault = defaultBalancePath(
        current.query_type,
        current.new_api_target,
      );
      if (patch.query_type && patch.query_type !== "disabled") {
        next.enabled = true;
      }
      if (patch.query_type === "disabled") {
        next.enabled = false;
      }
      if (patch.query_type || patch.new_api_target) {
        const nextDefault = defaultBalancePath(next.query_type, next.new_api_target);
        if (!current.path || current.path === previousDefault) {
          next.path = nextDefault;
        }
      }
      if (!next.path) {
        next.path = defaultBalancePath(next.query_type, next.new_api_target);
      }
      if (next.query_type === "new_api" && next.new_api_target === "account_balance") {
        next.enabled = true;
        next.auth_mode = "separate_token";
      }
      return next;
    });
    closePreview();
  }

  async function addProvider() {
    setNewProviderName("");
    setNewBaseUrl("https://code.xxcd.top/v1");
    setNewToken("");
    setNewTokenVisible(false);
    setSelectAfterCreate(true);
    setScreen("create");
  }

  async function createProvider() {
    const name = newProviderName.trim();
    if (!name) {
      setError("供应商名称不能为空");
      return;
    }

    await run(async () => {
      const state = await callCommand<AppState>("add_provider", { name });
      const activeId = state.active_provider?.id;
      if (!activeId) {
        setAppState(state);
        setScreen("main");
        return;
      }

      const saved = await callCommand<AppState>("save_provider", {
        payload: {
          provider_id: activeId,
          config_toml: buildCustomProviderToml(newBaseUrl, newToken),
          balance_query: defaultBalanceQuery(endpointFromBaseUrl(newBaseUrl)),
        },
      });

      if (!selectAfterCreate) {
        setAppState(saved);
        setScreen("main");
        return;
      }

      setAppState(saved);
      setScreen("edit");
    });
  }

  async function saveProvider() {
    await run(async () => {
      const state = await callCommand<AppState>("save_provider", {
        payload: {
          provider_id: appState?.active_provider?.id,
          provider_name: providerName,
          config_toml: providerText,
          balance_query: balanceQuery,
        },
      });
      setAppState(state);
      setPreviewState(null);
    });
  }

  async function previewProvider() {
    if (!appState?.active_provider) return;

    await run(async () => {
      const state = await callCommand<AppState>("preview_provider", {
        payload: {
          provider_id: appState.active_provider?.id,
          provider_name: providerName,
          config_toml: providerText,
          balance_query: balanceQuery,
        },
      });
      setPreviewState(state);
      setPreviewMode("provider");
      setPreviewExpanded(false);
    });
  }

  async function previewCurrentConfig() {
    await run(async () => {
      const state = await callCommand<AppState>("load_app_state");
      setAppState(state);
      setPreviewState(state);
      setPreviewMode("current");
      setPreviewExpanded(false);
    });
  }

  async function copyPreviewConfig() {
    if (!previewState) return;

    try {
      await navigator.clipboard.writeText(previewState.final_preview_toml);
    } catch {
      setError("复制失败，请手动选择预览内容复制。");
    }
  }

  async function saveBase() {
    await run(async () => {
      const state = await callCommand<AppState>("save_base_template", {
        payload: {
          base_template_name: baseName,
          base_toml: baseText,
        },
      });
      setAppState(state);
      setScreen("main");
    });
  }

  async function openSettings() {
    await run(async () => {
      const state = await callCommand<AppState>("load_app_state");
      setBaseName(state.base_template_name);
      setBaseText(state.base_toml);
      setAppState(state);
      setScreen("settings");
    });
  }

  async function applyConfig() {
    if (!appState) return;

    const conflictCount = appState.diffs.filter((diff) => diff.action === "冲突").length;
    if (
      conflictCount > 0 &&
      !window.confirm(`发现 ${conflictCount} 个外部修改冲突。确认后会使用当前预览值写入配置。`)
    ) {
      return;
    }

    await run(async () => {
      const state = await callCommand<AppState>("apply_config");
      setAppState(state);
      setScreen("main");
    });
  }

  async function applyEditedProvider() {
    if (!appState?.active_provider) return;

    const diffs = previewState?.diffs ?? appState.diffs;
    const conflictCount = diffs.filter((diff) => diff.action === "冲突").length;
    if (
      conflictCount > 0 &&
      !window.confirm(`发现 ${conflictCount} 个外部修改冲突。确认后会使用当前预览值写入配置。`)
    ) {
      return;
    }

    await run(async () => {
      await callCommand<AppState>("save_provider", {
        payload: {
          provider_id: appState.active_provider?.id,
          provider_name: providerName,
          config_toml: providerText,
          balance_query: balanceQuery,
        },
      });
      const state = await callCommand<AppState>("apply_config");
      setAppState(state);
      setPreviewState(null);
      setScreen("main");
    });
  }

  async function queryBalance() {
    if (!appState?.active_provider) return;

    setBalanceBusy(true);
    setError("");
    try {
      const state = await callCommand<AppState>("query_provider_balance", {
        payload: {
          provider_id: appState.active_provider?.id,
        },
      });
      setAppState(state);
    } catch (err) {
      setError(String(err));
    } finally {
      setBalanceBusy(false);
    }
  }

  async function openUsageStats() {
    setScreen("usage");
    await loadUsageStats(usageFilter, !usageStats);
  }

  async function loadUsageStats(nextFilter = usageFilter, forceRefresh = false) {
    if (forceRefresh) {
      setUsageRefreshing(true);
    }
    setUsageBusy(true);
    setError("");
    try {
      const stats = await callCommand<UsageStats>("load_usage_stats", {
        payload: {
          filter: usageFilterPayload(nextFilter),
          force_refresh: forceRefresh,
        },
      });
      setUsageStats(stats);
      setUsageFilter(stats.filters ?? nextFilter);
    } catch (err) {
      setError(String(err));
    } finally {
      setUsageBusy(false);
      setUsageRefreshing(false);
    }
  }

  function applyUsageRange(range: "today" | "7d" | "30d" | "month" | "all") {
    const nextFilter = {
      ...usageRangeFilter(range),
      provider_key: usageFilter.provider_key,
      provider_name: usageFilter.provider_name,
      model: usageFilter.model,
      page: 1,
      page_size: usageFilter.page_size ?? 20,
    };
    setUsageRange(range);
    setUsageFilter(nextFilter);
    void loadUsageStats(nextFilter);
  }

  function updateUsageFilter(patch: UsageStatsFilter, range: typeof usageRange = "custom") {
    const nextFilter = { ...usageFilter, ...patch };
    setUsageRange(range);
    setUsageFilter(nextFilter);
    void loadUsageStats(nextFilter);
  }

  function updateUsagePage(page: number) {
    const nextFilter = { ...usageFilter, page };
    setUsageFilter(nextFilter);
    void loadUsageStats(nextFilter);
  }

  function updateUsagePageSize(pageSize: number) {
    const nextFilter = { ...usageFilter, page: 1, page_size: pageSize };
    setUsageFilter(nextFilter);
    void loadUsageStats(nextFilter);
  }

  if (!appState) {
    return (
      <main className="loading-shell">
        <div className="loading-card">
          <strong>正在加载 Codex 配置</strong>
          <p>{error || "读取本地状态与 ~/.codex/config.toml"}</p>
          {error && <button onClick={() => refresh()}>重试</button>}
        </div>
      </main>
    );
  }

  const modalToml =
    previewMode === "current"
      ? previewState?.current_config_exists
        ? previewState.current_config_raw
        : "# 当前还没有 ~/.codex/config.toml"
      : previewState?.final_preview_toml ?? "";
  const modalDiffs = previewMode === "provider" ? previewState?.diffs ?? [] : [];
  const appliedProvider = appState.providers.find((provider) => provider.enabled);

  if (screen === "usage") {
    const stats = usageStats;
    const details = stats?.details ?? [];
    const currency = stats?.summary.currency ?? "USD";
    const detailPage = stats?.detail_page ?? usageFilter.page ?? 1;
    const detailPageSize = stats?.detail_page_size ?? usageFilter.page_size ?? 20;
    const detailTotalPages = stats?.detail_total_pages ?? 1;
    const pageNumbers = usagePageNumbers(detailPage, detailTotalPages);

    return (
      <main className="usage-shell">
        <header className="usage-topbar">
          <div className="usage-brand">
            <div className="brand-mark">C</div>
            <div>
              <strong>Codex 配置</strong>
              <span>使用统计</span>
            </div>
          </div>
          <div className="top-actions">
            <button className="secondary" onClick={() => setScreen("main")}>
              返回配置
            </button>
            <button
              className="primary"
              onClick={() => loadUsageStats(usageFilter, true)}
              disabled={usageRefreshing}
            >
              {usageRefreshing ? "刷新中" : "刷新统计"}
            </button>
          </div>
        </header>

        <section className="usage-workspace">
          <div className="usage-title-row">
            <div>
              <p className="eyebrow">本机使用统计</p>
              <h2>使用情况概览</h2>
              <p>只读取本机 Codex 会话 token_count 元数据，不展示 prompt 或 response 内容。</p>
            </div>
            <div className="usage-meta">
              <span>来源：{stats?.source_dir ?? "~/.codex/sessions"}</span>
              <span>
                {stats
                  ? `${formatInteger(stats.filtered_events)} / ${formatInteger(
                      stats.parsed_events,
                    )} 条记录 · ${formatInteger(stats.parsed_files)} 个文件`
                  : "尚未读取"}
              </span>
            </div>
          </div>

          {error && <div className="error-banner">{error}</div>}

          <section className="usage-filter-panel">
            <div className="usage-filter-group">
              <span>时间</span>
              <div className="usage-range-buttons">
                {[
                  ["today", "今天"],
                  ["7d", "7 天"],
                  ["30d", "30 天"],
                  ["month", "本月"],
                  ["all", "全部"],
                ].map(([value, label]) => (
                  <button
                    className={usageRange === value ? "selected" : ""}
                    key={value}
                    onClick={() =>
                      applyUsageRange(value as "today" | "7d" | "30d" | "month" | "all")
                    }
                    type="button"
                  >
                    {label}
                  </button>
                ))}
              </div>
              <input
                aria-label="开始日期"
                type="date"
                value={usageFilter.start_day ?? ""}
                onChange={(event) =>
                  updateUsageFilter({
                    start_day: event.currentTarget.value || null,
                    page: 1,
                  })
                }
              />
              <input
                aria-label="结束日期"
                type="date"
                value={usageFilter.end_day ?? ""}
                onChange={(event) =>
                  updateUsageFilter({
                    end_day: event.currentTarget.value || null,
                    page: 1,
                  })
                }
              />
            </div>

            <div className="usage-filter-group usage-filter-selects">
              <span>筛选</span>
              <select
                value={providerFilterValue(usageFilter)}
                onChange={(event) =>
                  updateUsageFilter({
                    ...parseProviderFilterValue(event.currentTarget.value),
                    page: 1,
                  })
                }
              >
                <option value="">全部供应商</option>
                {(stats?.available_providers ?? []).map((provider) => (
                  <option
                    key={`${provider.provider_key}-${provider.provider_name}`}
                    value={`${provider.provider_key}\u0000${provider.provider_name}`}
                  >
                    {provider.provider_name}
                  </option>
                ))}
              </select>
              <select
                value={usageFilter.model ?? ""}
                onChange={(event) =>
                  updateUsageFilter({ model: event.currentTarget.value || null, page: 1 })
                }
              >
                <option value="">全部模型</option>
                {(stats?.available_models ?? []).map((model) => (
                  <option key={model} value={model}>
                    {model}
                  </option>
                ))}
              </select>
            </div>

            <div className="usage-note">
              <strong>{stats?.unknown_provider_count ?? 0}</strong>
              <span>个未识别供应商</span>
            </div>
          </section>

          <section className={`usage-kpi-grid ${usageBusy && !usageRefreshing ? "is-loading" : ""}`}>
            <article className="usage-kpi-card accent">
              <span>输入（未缓存）</span>
              <strong>{formatCompactNumber(stats?.summary.uncached_input_tokens ?? 0)}</strong>
              <p>筛选范围：{usageRangeLabel(usageFilter)}</p>
            </article>
            <article className="usage-kpi-card">
              <span>输入（已缓存）</span>
              <strong>{formatCompactNumber(stats?.summary.cached_input_tokens ?? 0)}</strong>
              <p>按缓存输入单价计费</p>
            </article>
            <article className="usage-kpi-card">
              <span>输出</span>
              <strong>{formatCompactNumber(stats?.summary.output_tokens ?? 0)}</strong>
              <p>推理输出 {formatCompactNumber(stats?.summary.reasoning_output_tokens ?? 0)}</p>
            </article>
            <article
              className="usage-kpi-card warning"
              title={costTooltipForSummary(stats?.summary, "筛选范围金额")}
            >
              <span>金额</span>
              <strong>{formatMoney(stats?.summary.estimated_cost ?? 0, currency)}</strong>
              <p>悬停查看计算详情</p>
            </article>
          </section>

          <section className={`usage-bottom-grid single ${usageBusy && !usageRefreshing ? "is-loading" : ""}`}>
            <article className="usage-panel usage-details-panel">
              <div className="panel-header">
                <div>
                  <h3>明细记录</h3>
                  <p>按页查看筛选范围内的 token_count 记录。</p>
                </div>
                <span className="detail-count">
                  第 {formatInteger(detailPage)} / {formatInteger(detailTotalPages)} 页 · 共{" "}
                  {formatInteger(stats?.filtered_events ?? 0)} 条
                </span>
              </div>
              <div className="usage-detail-wrap">
                <div className="usage-detail-table">
                  <span>时间</span>
                  <span>供应商</span>
                  <span>模型</span>
                  <span>输入（未缓存）</span>
                  <span>输入（已缓存）</span>
                  <span>输出</span>
                  <span>推理</span>
                  <span>金额</span>
                  {details.map((row) => (
                    <div className="usage-detail-row" key={`${row.timestamp}-${row.session_id}-${row.total_tokens}`}>
                      <span title={row.source}>{row.timestamp}</span>
                      <strong title={row.provider_key}>{row.provider_name}</strong>
                      <em title={row.pricing_model_match}>{row.model}</em>
                      <b>{formatCompactNumber(row.uncached_input_tokens)}</b>
                      <b>{formatCompactNumber(row.cached_input_tokens)}</b>
                      <b>{formatCompactNumber(row.output_tokens)}</b>
                      <b>{formatCompactNumber(row.reasoning_output_tokens)}</b>
                      <i className="cost-cell" title={row.cost_breakdown}>
                        {formatMoney(row.estimated_cost, row.currency || currency)}
                      </i>
                    </div>
                  ))}
                </div>
                {!details.length && <div className="usage-empty">当前筛选范围没有明细。</div>}
              </div>
              <div className="usage-pagination">
                <button
                  className="secondary"
                  disabled={usageBusy || detailPage <= 1}
                  onClick={() => updateUsagePage(detailPage - 1)}
                  type="button"
                >
                  上一页
                </button>
                {pageNumbers.map((page, index) => (
                  <span className="pagination-page" key={page}>
                    {index > 0 && pageNumbers[index - 1] !== page - 1 && <em>...</em>}
                    <button
                      className={page === detailPage ? "selected" : ""}
                      onClick={() => updateUsagePage(page)}
                      type="button"
                    >
                      {page}
                    </button>
                  </span>
                ))}
                <button
                  className="secondary"
                  disabled={usageBusy || detailPage >= detailTotalPages}
                  onClick={() => updateUsagePage(detailPage + 1)}
                  type="button"
                >
                  下一页
                </button>
                <select
                  value={detailPageSize}
                  onChange={(event) => updateUsagePageSize(Number(event.currentTarget.value))}
                >
                  {[20, 50, 100].map((size) => (
                    <option key={size} value={size}>
                      {size} 条/页
                    </option>
                  ))}
                </select>
              </div>
            </article>
          </section>
        </section>
      </main>
    );
  }

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">C</div>
          <div>
            <h1>Codex 配置</h1>
            <p>本地配置管理器</p>
          </div>
        </div>

        <button className="add-button" onClick={addProvider} disabled={busy}>
          +
        </button>

        <nav className="provider-list">
          {appState.providers.map((provider) => (
            <button
              className={`provider-item ${
                provider.id === appState.active_provider_id ? "active" : ""
              }`}
              key={provider.id}
              onClick={() => selectProvider(provider.id)}
              disabled={busy}
            >
              <span className="provider-name">
                <span>{provider.name}</span>
                {provider.enabled && <b className="enabled-badge">启用中</b>}
              </span>
              <small>
                {provider.id === appState.active_provider_id && screen === "edit"
                  ? "正在编辑"
                  : provider.pending_changes
                    ? `${provider.pending_changes} 项变更`
                    : "已配置"}
              </small>
            </button>
          ))}
        </nav>

        <div className="global-actions">
          <button className="settings-button primary-nav" onClick={openUsageStats} disabled={busy}>
            使用统计
          </button>
          <button className="settings-button" onClick={previewCurrentConfig} disabled={busy}>
            查看实际配置
          </button>
          <button className="settings-button" onClick={openSettings} disabled={busy}>
            基础配置
          </button>
        </div>
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div>
            <p className="eyebrow">
              {screen === "edit"
                ? "编辑供应商"
                : screen === "create"
                  ? "新增供应商"
                : screen === "current"
                  ? "当前配置"
                : screen === "settings"
                  ? "基础配置"
                  : "当前供应商"}
            </p>
            <h2>
              {screen === "settings"
                ? "基础模板"
                : screen === "create"
                  ? "自定义接口"
                : screen === "current"
                  ? "config.toml"
                : appState.active_provider?.name ?? "未选择供应商"}
            </h2>
          </div>
          <div className="top-actions">
            {screen === "main" && (
              <>
                <div
                  className={`balance-chip ${
                    appState.active_provider?.balance_status?.error ? "error" : ""
                  }`}
                  title={appState.active_provider?.balance_status?.error ?? undefined}
                >
                  <strong>
                    {balanceBusy ? "查询中" : balanceChipLabel(appState.active_provider)}
                  </strong>
                  <button
                    type="button"
                    onClick={
                      appState.active_provider?.balance_query.enabled
                        ? queryBalance
                        : () => setScreen("edit")
                    }
                    disabled={balanceBusy || !appState.active_provider}
                  >
                    {balanceBusy ? "..." : balanceChipActionLabel(appState.active_provider)}
                  </button>
                </div>
                <button className="secondary" onClick={() => setScreen("edit")}>
                  编辑配置
                </button>
                <button
                  className="secondary"
                  onClick={previewProvider}
                  disabled={busy || !appState.active_provider}
                >
                  预览选择配置
                </button>
                <button
                  className="primary"
                  onClick={applyConfig}
                  disabled={busy || !appState.active_provider}
                >
                  应用
                </button>
              </>
            )}
            {screen === "create" && (
              <>
                <button className="secondary" onClick={() => setScreen("main")}>
                  取消
                </button>
                <button className="primary" onClick={createProvider} disabled={busy}>
                  创建供应商
                </button>
              </>
            )}
            {screen === "edit" && (
              <>
                <button className="secondary" onClick={() => setScreen("main")}>
                  返回
                </button>
                <button className="secondary" onClick={saveProvider} disabled={busy}>
                  保存模板
                </button>
                <button className="primary" onClick={previewProvider} disabled={busy}>
                  预览选择配置
                </button>
              </>
            )}
            {screen === "current" && (
              <>
                <button className="secondary" onClick={() => setScreen("main")}>
                  返回
                </button>
                <button className="primary" onClick={refresh} disabled={busy}>
                  重新读取
                </button>
              </>
            )}
            {screen === "settings" && (
              <>
                <button className="secondary" onClick={() => setScreen("main")}>
                  取消
                </button>
                <button className="primary" onClick={saveBase} disabled={busy}>
                  保存基础模板
                </button>
              </>
            )}
          </div>
        </header>

        {error && <div className="error-banner">{error}</div>}

        {screen === "main" && (
          <>
            {appState.active_provider ? (
              <section className="summary-grid">
                <article className="metric-card accent">
                  <span>模型</span>
                  <strong>
                    {formatValue(
                      appState.summary.find((row) => row.path === "model")?.value,
                    )}
                  </strong>
                  <p>来自供应商配置</p>
                </article>
                <article className="metric-card">
                  <span>审批策略</span>
                  <strong>
                    {formatValue(
                      appState.summary.find((row) => row.path === "approval_policy")
                        ?.value,
                    )}
                  </strong>
                  <p>来自基础模板</p>
                </article>
                <article className="metric-card">
                  <span>沙盒模式</span>
                  <strong>
                    {formatValue(
                      appState.summary.find((row) => row.path === "sandbox_mode")?.value,
                    )}
                  </strong>
                  <p>来自基础模板</p>
                </article>
                <article className="metric-card warning">
                  <span>待应用</span>
                  <strong>{appState.diffs.length} 项变更</strong>
                  <p>应用前由后端重新检查</p>
                </article>
              </section>
            ) : (
              <section className="empty-state">
                <div>
                  <h3>还没有供应商模板</h3>
                  <p>首次启动不会预设任何供应商。你可以先查看当前 config.toml，再按需新增供应商模板。</p>
                </div>
                <div className="empty-actions">
                  <button className="secondary" onClick={previewCurrentConfig}>
                    查看实际配置
                  </button>
                  <button className="primary" onClick={addProvider}>
                    新增供应商
                  </button>
                </div>
              </section>
            )}

            <section className="panel">
              <div className="panel-header">
                <div>
                  <h3>现有配置摘要</h3>
                  <p>主界面只展示最终会影响 Codex 的关键项。</p>
                </div>
                <span className={`status ${appliedProvider ? "ok" : "warn"}`}>
                  {appliedProvider ? `已应用：${appliedProvider.name}` : "未应用供应商"}
                </span>
              </div>

              <div className="summary-list">
                {importantRows.length ? (
                  importantRows.map((row) => (
                    <div
                      className={`summary-row ${row.changed ? "changed" : ""}`}
                      key={row.path}
                    >
                      <span title={row.path}>{pathLabel(row.path)}</span>
                      <div className="summary-value">
                        <strong>
                          {isSecretPath(row.path) && !visibleSummarySecrets[row.path]
                            ? "********"
                            : formatValue(row.value)}
                        </strong>
                        {isSecretPath(row.path) && (
                          <button
                            type="button"
                            onClick={() => toggleSummarySecret(row.path)}
                          >
                            {visibleSummarySecrets[row.path] ? "隐藏" : "显示"}
                          </button>
                        )}
                      </div>
                      <em className={sourceClass(row.source)}>{row.source}</em>
                    </div>
                  ))
                ) : (
                  <div className="summary-empty">当前还没有由模板生成的摘要。</div>
                )}
              </div>
            </section>
          </>
        )}

        {screen === "create" && (
          <>
            <section className="create-layout">
              <article className="panel create-form">
                <div className="panel-header">
                  <div>
                    <h3>连接信息</h3>
                    <p>创建一个写入 [model_providers.custom] 的供应商模板。</p>
                  </div>
                  <span className="status ok">Custom</span>
                </div>

                <label className="field">
                  <span>供应商名称</span>
                  <input
                    value={newProviderName}
                    onChange={(event) => setNewProviderName(event.currentTarget.value)}
                    placeholder="例如：xxcd"
                  />
                </label>

                <label className="field">
                  <span>base_url</span>
                  <input
                    value={newBaseUrl}
                    onChange={(event) => setNewBaseUrl(event.currentTarget.value)}
                    placeholder="https://code.xxcd.top/v1"
                  />
                </label>

                <label className="field">
                  <span>experimental_bearer_token</span>
                  <div className="secret-input">
                    <input
                      autoComplete="off"
                      type={newTokenVisible ? "text" : "password"}
                      value={newToken}
                      onChange={(event) => setNewToken(event.currentTarget.value)}
                      placeholder="粘贴 token"
                    />
                    <button
                      type="button"
                      onClick={() => setNewTokenVisible((visible) => !visible)}
                    >
                      {newTokenVisible ? "隐藏" : "显示"}
                    </button>
                  </div>
                </label>

                <label className="check-row">
                  <input
                    checked={selectAfterCreate}
                    onChange={(event) => setSelectAfterCreate(event.currentTarget.checked)}
                    type="checkbox"
                  />
                  <span>创建后进入编辑</span>
                </label>
              </article>

              <article className="editor-panel result">
                <div className="panel-header">
                  <div>
                    <h3>将生成的 TOML</h3>
                    <p>不包含 model，后续可手动添加。</p>
                  </div>
                  <span className="status ok">TOML</span>
                </div>
                <pre className="code-preview">
                  <code>
                    {buildVisibleCustomProviderToml(newBaseUrl, newToken, newTokenVisible)}
                  </code>
                </pre>
              </article>
            </section>

            <section className="panel compact">
              <div className="choice-strip">
                <div className="choice-item selected">
                  <strong>只创建模板</strong>
                  <p>保存供应商配置，暂不写入 config.toml。</p>
                </div>
                <div className="choice-item">
                  <strong>创建后立即应用</strong>
                  <p>后续再接入应用确认流程。</p>
                </div>
              </div>
            </section>
          </>
        )}

        {screen === "edit" && (
          appState.active_provider ? (
            <section className="edit-focus">
              <article className="panel provider-editor">
                <div className="panel-header">
                  <div>
                    <h3>供应商字段</h3>
                    <p>这里只编辑供应商模板，基础模板仍在设置中管理。</p>
                  </div>
                  <span className="status ok">Custom</span>
                </div>

                <div className="form-grid">
                  <label className="field">
                    <span>供应商名称</span>
                    <input
                      value={providerName}
                      onChange={(event) => updateProviderName(event.currentTarget.value)}
                      placeholder="例如：xxcd"
                    />
                  </label>

                  <label className="field">
                    <span>model_provider</span>
                    <input readOnly value="custom" />
                  </label>
                </div>

                <div className="connection-panel">
                  <div className="connection-heading">
                    <div>
                      <strong>自定义接口</strong>
                      <p>写入到 [model_providers.custom]</p>
                    </div>
                  </div>

                  <label className="field">
                    <span>base_url</span>
                    <input
                      value={customBaseUrl}
                      onChange={(event) => updateCustomBaseUrl(event.currentTarget.value)}
                      placeholder="https://code.xxcd.top/v1"
                    />
                  </label>

                  <label className="field">
                    <span>experimental_bearer_token</span>
                    <div className="secret-input">
                      <input
                        autoComplete="off"
                        type={customTokenVisible ? "text" : "password"}
                        value={customToken}
                        onChange={(event) => updateCustomToken(event.currentTarget.value)}
                        placeholder="xxx"
                      />
                      <button
                        type="button"
                        onClick={() => setCustomTokenVisible((visible) => !visible)}
                      >
                        {customTokenVisible ? "隐藏" : "显示"}
                      </button>
                    </div>
                  </label>
                </div>

                <div className="balance-config">
                  <div className="balance-config-head">
                    <div>
                      <strong>余额查询</strong>
                      <p>仅保存到本工具状态，不写入 Codex config.toml。</p>
                    </div>
                    <label className="switch-row">
                      <input
                        checked={balanceQuery.enabled}
                        onChange={(event) =>
                          updateBalanceQuery({
                            enabled: event.currentTarget.checked,
                            query_type: event.currentTarget.checked
                              ? balanceQuery.query_type === "disabled"
                                ? "new_api"
                                : balanceQuery.query_type
                              : "disabled",
                          })
                        }
                        type="checkbox"
                      />
                      <span />
                    </label>
                  </div>

                  <div className="segmented">
                    {[
                      ["disabled", "不查询"],
                      ["new_api", "NewAPI"],
                      ["sub2_api", "Sub2API"],
                    ].map(([value, label]) => (
                      <button
                        className={balanceQuery.query_type === value ? "selected" : ""}
                        key={value}
                        onClick={() =>
                          updateBalanceQuery({
                            query_type: value as BalanceQueryType,
                            enabled: value !== "disabled",
                          })
                        }
                        type="button"
                      >
                        {label}
                      </button>
                    ))}
                  </div>

                  {balanceQuery.enabled && balanceQuery.query_type !== "disabled" && (
                    <>
                      {balanceQuery.query_type === "new_api" && (
                        <div className="field">
                          <span>查询内容</span>
                          <div className="radio-row">
                            <button
                              className={`radio-pill ${
                                balanceQuery.new_api_target === "token_quota"
                                  ? "selected"
                                  : ""
                              }`}
                              onClick={() =>
                                updateBalanceQuery({ new_api_target: "token_quota" })
                              }
                              type="button"
                            >
                              API Key 额度
                            </button>
                            <button
                              className={`radio-pill ${
                                balanceQuery.new_api_target === "account_balance"
                                  ? "selected"
                                  : ""
                              }`}
                              onClick={() =>
                                updateBalanceQuery({
                                  new_api_target: "account_balance",
                                  auth_mode: "separate_token",
                                })
                              }
                              type="button"
                            >
                              账户余额
                            </button>
                          </div>
                        </div>
                      )}

                      <div className="balance-config-grid">
                        <label className="field">
                          <span>查询地址</span>
                          <input
                            value={balanceQuery.endpoint}
                            onChange={(event) =>
                              updateBalanceQuery({ endpoint: event.currentTarget.value })
                            }
                            placeholder={endpointFromBaseUrl(customBaseUrl)}
                          />
                        </label>
                        <label className="field">
                          <span>余额路径</span>
                          <input
                            value={balanceQuery.path}
                            onChange={(event) =>
                              updateBalanceQuery({ path: event.currentTarget.value })
                            }
                            placeholder={defaultBalancePath(
                              balanceQuery.query_type,
                              balanceQuery.new_api_target,
                            )}
                          />
                        </label>
                      </div>

                      <div className="field">
                        <span>认证方式</span>
                        <div className="radio-row">
                          {!(
                            balanceQuery.query_type === "new_api" &&
                            balanceQuery.new_api_target === "account_balance"
                          ) && (
                            <button
                              className={`radio-pill ${
                                balanceQuery.auth_mode === "provider_token" ? "selected" : ""
                              }`}
                              onClick={() =>
                                updateBalanceQuery({ auth_mode: "provider_token" })
                              }
                              type="button"
                            >
                              使用供应商 Token
                            </button>
                          )}
                          <button
                            className={`radio-pill ${
                              balanceQuery.auth_mode === "separate_token" ? "selected" : ""
                            }`}
                            onClick={() =>
                              updateBalanceQuery({ auth_mode: "separate_token" })
                            }
                            type="button"
                          >
                            {balanceQuery.query_type === "new_api" &&
                            balanceQuery.new_api_target === "account_balance"
                              ? "用户访问令牌"
                              : "单独填写查询 Token"}
                          </button>
                        </div>
                      </div>

                      {balanceQuery.auth_mode === "separate_token" && (
                        <label className="field">
                          <span>
                            {balanceQuery.query_type === "new_api" &&
                            balanceQuery.new_api_target === "account_balance"
                              ? "用户访问令牌"
                              : "查询 Token"}
                          </span>
                          <div className="secret-input">
                            <input
                              autoComplete="off"
                              type={balanceTokenVisible ? "text" : "password"}
                              value={balanceQuery.query_token}
                              onChange={(event) =>
                                updateBalanceQuery({
                                  query_token: event.currentTarget.value,
                                })
                              }
                              placeholder={
                                balanceQuery.query_type === "new_api" &&
                                balanceQuery.new_api_target === "account_balance"
                                  ? "粘贴 New API 用户访问令牌"
                                  : "粘贴查询 token"
                              }
                            />
                            <button
                              type="button"
                              onClick={() => setBalanceTokenVisible((visible) => !visible)}
                            >
                              {balanceTokenVisible ? "隐藏" : "显示"}
                            </button>
                          </div>
                        </label>
                      )}

                      {balanceQuery.query_type === "new_api" &&
                        balanceQuery.new_api_target === "account_balance" && (
                          <label className="field compact-field">
                            <span>New-Api-User</span>
                            <input
                              inputMode="numeric"
                              value={balanceQuery.new_api_user_id}
                              onChange={(event) =>
                                updateBalanceQuery({
                                  new_api_user_id: event.currentTarget.value.replace(
                                    /\D/g,
                                    "",
                                  ),
                                })
                              }
                              placeholder="数字用户 ID"
                            />
                          </label>
                        )}
                    </>
                  )}
                </div>

                <div className="advanced-block">
                  <button
                    className="advanced-toggle"
                    onClick={() => setAdvancedOpen((open) => !open)}
                  >
                    <span>高级 TOML</span>
                    <strong>{advancedOpen ? "收起" : "展开"}</strong>
                  </button>

                  {advancedOpen && (
                    <div className="advanced-editor-shell">
                      <textarea
                        className="code-input advanced"
                        value={providerText}
                        onChange={(event) => updateProviderToml(event.currentTarget.value)}
                        spellCheck={false}
                      />
                    </div>
                  )}
                </div>
              </article>
            </section>
          ) : (
            <section className="empty-state">
              <div>
                <h3>没有可编辑的供应商</h3>
                <p>请先新增一个供应商模板，然后再编辑它的 TOML 配置。</p>
              </div>
              <div className="empty-actions">
                <button className="secondary" onClick={() => setScreen("main")}>
                  返回
                </button>
                <button className="primary" onClick={addProvider}>
                  新增供应商
                </button>
              </div>
            </section>
          )
        )}

        {screen === "current" && (
          <>
            <section className="panel compact">
              <div className="apply-strip">
                <div>
                  <span>目标文件</span>
                  <strong>{appState.codex_config_path}</strong>
                </div>
                <div>
                  <span>文件状态</span>
                  <strong>{appState.current_config_exists ? "已读取" : "尚未创建"}</strong>
                </div>
                <div>
                  <span>管理标记</span>
                  <strong>{appState.marker_present ? "存在" : "缺失"}</strong>
                </div>
              </div>
            </section>

            <section className="settings-layout single current-view">
              <article className="editor-panel current-file-panel">
                <div className="panel-header">
                  <div>
                    <h3>实际生效配置</h3>
                    <p>只读显示磁盘上的真实 config.toml。</p>
                  </div>
                  <span className="status ok">只读</span>
                </div>
                <pre className="code-preview">
                  <code>
                    {appState.current_config_exists
                      ? appState.current_config_raw
                      : "# 当前还没有 ~/.codex/config.toml"}
                  </code>
                </pre>
              </article>
            </section>
          </>
        )}

        {screen === "settings" && (
          <section className="settings-layout single">
            <article className="panel">
              <div className="panel-header">
                <div>
                  <h3>基础模板</h3>
                  <p>基础模板只在设置中编辑，主界面不会混入模板细节。</p>
                </div>
                <span className="status ok">Base</span>
              </div>

              <label className="field">
                <span>模板名称</span>
                <input
                  value={baseName}
                  onChange={(event) => setBaseName(event.currentTarget.value)}
                />
              </label>

              <label className="field grow">
                <span>配置 TOML</span>
                <textarea
                  className="code-input base"
                  value={baseText}
                  onChange={(event) => setBaseText(event.currentTarget.value)}
                  spellCheck={false}
                />
              </label>
            </article>
          </section>
        )}
      </section>

      {previewState && (
        <div className="modal-backdrop" role="dialog" aria-modal="true">
          <section className={previewExpanded ? "preview-modal expanded" : "preview-modal"}>
            <header className="preview-modal-header">
              <div>
                <h3>{previewMode === "current" ? "实际生效配置" : "选择配置预览"}</h3>
                <p>{previewState.codex_config_path}</p>
              </div>
              <div className="preview-badges">
                {previewMode === "provider" ? (
                  <>
                    <span className={previewState.diffs.length ? "status warn" : "status ok"}>
                      {previewState.diffs.length ? `${previewState.diffs.length} 处变化` : "无变化"}
                    </span>
                    <span className="status neutral">
                      {previewState.marker_present ? "已有管理标记" : "将写入管理标记"}
                    </span>
                  </>
                ) : (
                  <>
                    <span className="status ok">
                      {previewState.current_config_exists ? "已读取" : "尚未创建"}
                    </span>
                    <span className="status neutral">
                      {previewState.marker_present ? "已有管理标记" : "无管理标记"}
                    </span>
                  </>
                )}
              </div>
            </header>

            <pre className="modal-code-preview">
              <code>{highlightToml(modalToml, modalDiffs)}</code>
            </pre>

            <footer className="preview-modal-footer">
              <div className="modal-toolbox">
                <button className="modal-icon-button" onClick={copyPreviewConfig} title="复制">
                  复制
                </button>
                <button
                  className="modal-icon-button"
                  onClick={() => setPreviewExpanded((expanded) => !expanded)}
                  title="展开"
                >
                  {previewExpanded ? "还原" : "展开"}
                </button>
              </div>
              <div className="modal-actions">
                <button className="secondary dark" onClick={closePreview} disabled={busy}>
                  取消
                </button>
                {previewMode === "provider" && (
                  <button className="primary" onClick={applyEditedProvider} disabled={busy}>
                    应用到磁盘
                  </button>
                )}
              </div>
            </footer>
          </section>
        </div>
      )}
    </main>
  );
}

export default App;
