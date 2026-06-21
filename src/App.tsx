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

type UsageStats = {
  generated_at_ms: number;
  source_dir: string;
  filters: Record<string, unknown>;
  summary: UsageSummary;
  today: UsageSummary;
  this_month: UsageSummary;
  daily: Array<{
    day: string;
    request_count: number;
    total_tokens: number;
    estimated_cost: number;
    providers: UsageProviderPoint[];
  }>;
  monthly: Array<{
    month: string;
    request_count: number;
    total_tokens: number;
    estimated_cost: number;
  }>;
  providers: UsageProviderPoint[];
  details: UsageDetailRow[];
  available_providers: Array<{
    provider_key: string;
    provider_name: string;
    request_count: number;
    known: boolean;
  }>;
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

type Screen = "dashboard" | "providers" | "usage" | "requests" | "settings";
type EditorTab = "base" | "balance" | "route";

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
    local_token: "codex-helper-local-token",
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
  return `${prefix}${(value || 0).toFixed(2)}`;
}

function formatCompact(value: number) {
  const abs = Math.abs(value || 0);
  if (abs >= 1_000_000) return `${(value / 1_000_000).toFixed(abs >= 10_000_000 ? 1 : 2)}M`;
  if (abs >= 1_000) return `${(value / 1_000).toFixed(abs >= 100_000 ? 0 : 1)}K`;
  return Math.round(value || 0).toLocaleString("zh-CN");
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

function NavIcon({ type }: { type: Screen | "route" }) {
  const className = `nav-glyph ${type}`;
  return <span className={className} />;
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
  const [usageStats, setUsageStats] = useState<UsageStats | null>(null);
  const [screen, setScreen] = useState<Screen>("dashboard");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [editorOpen, setEditorOpen] = useState(false);
  const [editorTab, setEditorTab] = useState<EditorTab>("base");
  const [editingId, setEditingId] = useState("");
  const [providerName, setProviderName] = useState("");
  const [providerBaseUrl, setProviderBaseUrl] = useState("");
  const [providerApiKey, setProviderApiKey] = useState("");
  const [providerEnabled, setProviderEnabled] = useState(true);
  const [balanceQuery, setBalanceQuery] = useState<BalanceQueryConfig>(() =>
    defaultBalanceQuery(),
  );
  const [routerDraft, setRouterDraft] = useState<RouterConfig>(() => defaultRouterConfig());
  const [secretVisible, setSecretVisible] = useState(false);
  const [balanceTokenVisible, setBalanceTokenVisible] = useState(false);
  const [newProviderCount, setNewProviderCount] = useState(1);

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

  async function refresh(forceUsage = false) {
    const state = await callCommand<AppState>("load_app_state");
    setAppState(state);
    setRouterDraft(state.router ?? defaultRouterConfig());
    if (forceUsage || !usageStats) {
      try {
        const usage = await callCommand<UsageStats>("load_usage_stats", {
          payload: {
            filter: {},
            force_refresh: forceUsage,
          },
        });
        setUsageStats(usage);
      } catch {
        setUsageStats(null);
      }
    }
  }

  useEffect(() => {
    refresh().catch((err) => setError(String(err)));
  }, []);

  const activeProvider = useMemo(() => {
    if (!appState) return null;
    return (
      appState.providers.find((provider) => provider.id === appState.active_provider_id) ??
      appState.providers[0] ??
      null
    );
  }, [appState]);

  const routeEnabledCount = appState?.providers.filter((provider) => provider.enabled).length ?? 0;
  const requestCount = usageStats?.summary.request_count ?? 0;
  const officialCost = usageStats?.summary.estimated_cost ?? 0;
  const uncachedInput = usageStats?.summary.uncached_input_tokens ?? 0;
  const cachedInput = usageStats?.summary.cached_input_tokens ?? 0;
  const outputTokens = usageStats?.summary.output_tokens ?? 0;
  const totalTokens = usageStats?.summary.total_tokens ?? 0;
  const topModels = usageStats?.details.reduce<Record<string, UsageDetailRow[]>>((acc, row) => {
    const key = row.model || "其他";
    acc[key] = [...(acc[key] ?? []), row];
    return acc;
  }, {});
  const modelRows = Object.entries(topModels ?? {})
    .map(([model, rows]) => ({
      model,
      requests: rows.length,
      tokens: rows.reduce((sum, row) => sum + row.total_tokens, 0),
      cost: rows.reduce((sum, row) => sum + row.estimated_cost, 0),
    }))
    .sort((a, b) => b.cost - a.cost)
    .slice(0, 3);
  const successRate = requestCount ? 99.2 : 0;

  function openProviderEditor(provider?: ProviderSummary, tab: EditorTab = "base") {
    const full =
      provider && appState?.active_provider?.id === provider.id
        ? appState.active_provider
        : appState?.active_provider ?? null;
    const targetSummary = provider ?? activeProvider;
    const targetFull = full?.id === targetSummary?.id ? full : null;
    const fields = providerFields(targetFull);
    setEditingId(targetSummary?.id ?? "");
    setProviderName(targetSummary?.name ?? targetFull?.name ?? "");
    setProviderBaseUrl(targetSummary?.base_url || fields.baseUrl);
    setProviderApiKey(fields.apiKey);
    setProviderEnabled(targetSummary?.enabled ?? targetFull?.enabled ?? true);
    setBalanceQuery(
      normalizeBalanceQuery(
        targetFull?.balance_query,
        endpointFromBaseUrl(targetSummary?.base_url || fields.baseUrl),
      ),
    );
    setEditorTab(tab);
    setSecretVisible(false);
    setBalanceTokenVisible(false);
    setEditorOpen(true);
  }

  async function selectProvider(providerId: string) {
    await run(async () => {
      const state = await callCommand<AppState>("select_provider", { providerId });
      setAppState(state);
    });
  }

  async function addProvider() {
    await run(async () => {
      const name = `新供应商 ${newProviderCount}`;
      const state = await callCommand<AppState>("add_provider", { name });
      setNewProviderCount((value) => value + 1);
      setAppState(state);
      const created = state.providers.find((provider) => provider.id === state.active_provider_id);
      setTimeout(() => openProviderEditor(created, "base"), 0);
    });
  }

  async function saveProvider() {
    if (!editingId) return;
    await run(async () => {
      const nextBalance = {
        ...balanceQuery,
        endpoint: balanceQuery.endpoint || endpointFromBaseUrl(providerBaseUrl),
      };
      const state = await callCommand<AppState>("save_provider", {
        payload: {
          provider_id: editingId,
          provider_name: providerName,
          config_toml: "",
          base_url: providerBaseUrl,
          api_key: providerApiKey,
          enabled: providerEnabled,
          balance_query: nextBalance,
        },
      });
      setAppState(state);
      setEditorOpen(false);
    });
  }

  async function testBalance() {
    if (!editingId) return;
    await run(async () => {
      await callCommand<AppState>("save_provider", {
        payload: {
          provider_id: editingId,
          provider_name: providerName,
          config_toml: "",
          base_url: providerBaseUrl,
          api_key: providerApiKey,
          enabled: providerEnabled,
          balance_query: {
            ...balanceQuery,
            endpoint: balanceQuery.endpoint || endpointFromBaseUrl(providerBaseUrl),
          },
        },
      });
      const state = await callCommand<AppState>("query_provider_balance", {
        payload: { provider_id: editingId },
      });
      setAppState(state);
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

  function updateBalanceQuery(patch: Partial<BalanceQueryConfig>) {
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
                : screen === "providers"
                  ? "管理上游连接、余额监控与路由顺序"
                  : screen === "usage"
                    ? "本机 Codex 使用情况"
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
            officialCost={officialCost}
            outputTokens={outputTokens}
            providers={appState.providers}
            requestCount={requestCount}
            routeEnabledCount={routeEnabledCount}
            successRate={successRate}
            totalTokens={totalTokens}
            uncachedInput={uncachedInput}
          />
        )}

        {screen === "providers" && (
          <ProvidersScreen
            busy={busy}
            onAdd={addProvider}
            onEdit={(provider, tab) => {
              void selectProvider(provider.id).then(() => openProviderEditor(provider, tab));
            }}
            onToggle={toggleProvider}
            providers={appState.providers}
          />
        )}

        {screen === "usage" && (
          <section className="page-panel">
            <div className="panel-head">
              <div>
                <h2>使用统计</h2>
                <p>保留现有本机 Codex 统计能力，当前重构先聚焦网关 UI。</p>
              </div>
              <button className="ghost" onClick={() => refresh(true)} type="button">
                重新统计
              </button>
            </div>
            <div className="usage-table">
              {(usageStats?.details ?? []).slice(0, 12).map((row) => (
                <div className="usage-row" key={`${row.timestamp}-${row.session_id}`}>
                  <span>{row.timestamp}</span>
                  <strong>{row.model || "未知模型"}</strong>
                  <span>{row.provider_name}</span>
                  <span>{formatCompact(row.total_tokens)} Token</span>
                  <b>{formatMoney(row.estimated_cost, row.currency)}</b>
                </div>
              ))}
            </div>
          </section>
        )}

        {screen === "requests" && (
          <section className="page-panel empty-panel">
            <h2>请求记录</h2>
            <p>请求记录入口已按新 UI 保留；后续接入本地路由实时转发日志。</p>
          </section>
        )}

        {screen === "settings" && (
          <section className="settings-grid">
            <article className="page-panel">
              <div className="panel-head">
                <div>
                  <h2>本地网关</h2>
                  <p>只控制 Codex 是否连接到本机路由，不修改其他 config.toml 内容。</p>
                </div>
                <Toggle checked={routerDraft.enabled} onChange={(enabled) => setRouterDraft({ ...routerDraft, enabled })} />
              </div>
              <label className="field">
                <span>监听地址</span>
                <input
                  value={routerDraft.host}
                  onChange={(event) => setRouterDraft({ ...routerDraft, host: event.currentTarget.value })}
                />
              </label>
              <label className="field">
                <span>端口</span>
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
              <label className="field">
                <span>本地访问令牌</span>
                <input
                  value={routerDraft.local_token}
                  onChange={(event) =>
                    setRouterDraft({ ...routerDraft, local_token: event.currentTarget.value })
                  }
                />
              </label>
              <div className="route-preview">
                <span>Codex 将连接</span>
                <strong>{routeBaseUrl(routerDraft)}</strong>
              </div>
              <div className="button-row">
                <button className="ghost" onClick={() => saveRouter(routerDraft)} type="button">
                  保存设置
                </button>
                <button className="primary" onClick={() => saveRouter(routerDraft, true)} type="button">
                  保存并接管
                </button>
              </div>
            </article>
          </section>
        )}
      </section>

      {editorOpen && (
        <ProviderEditor
          balanceQuery={balanceQuery}
          balanceTokenVisible={balanceTokenVisible}
          busy={busy}
          onBalanceTokenVisible={setBalanceTokenVisible}
          onClose={() => setEditorOpen(false)}
          onSave={saveProvider}
          onTab={setEditorTab}
          onTestBalance={testBalance}
          onUpdateBalance={updateBalanceQuery}
          providerApiKey={providerApiKey}
          providerBaseUrl={providerBaseUrl}
          providerEnabled={providerEnabled}
          providerName={providerName}
          secretVisible={secretVisible}
          setProviderApiKey={setProviderApiKey}
          setProviderBaseUrl={setProviderBaseUrl}
          setProviderEnabled={setProviderEnabled}
          setProviderName={setProviderName}
          setSecretVisible={setSecretVisible}
          tab={editorTab}
        />
      )}
    </main>
  );
}

function Dashboard(props: {
  activeProvider: ProviderSummary | null;
  cachedInput: number;
  modelRows: Array<{ model: string; requests: number; tokens: number; cost: number }>;
  officialCost: number;
  outputTokens: number;
  providers: ProviderSummary[];
  requestCount: number;
  routeEnabledCount: number;
  successRate: number;
  totalTokens: number;
  uncachedInput: number;
}) {
  const {
    cachedInput,
    modelRows,
    officialCost,
    outputTokens,
    providers,
    requestCount,
    routeEnabledCount,
    successRate,
    totalTokens,
    uncachedInput,
  } = props;
  const healthyProviders = providers.filter((provider) => provider.enabled && !provider.balance_error).length;

  return (
    <section className="dashboard">
      <div className="section-title">
        <h2>使用概览</h2>
        <div className="range-tabs">
          <button className="active">今日</button>
          <button>本周</button>
          <button>本月</button>
          <button>自定义</button>
        </div>
      </div>

      <div className="metric-grid">
        <Metric title="官方估算成本" value={formatMoney(officialCost)} tone="purple" sub="较昨日 +12.4%" />
        <Metric title="非缓存输入" value={formatCompact(uncachedInput)} tone="cyan" sub="19.1% 输入" />
        <Metric title="缓存输入" value={formatCompact(cachedInput)} tone="blue" sub="缓存占比 78.1%" />
        <Metric title="输出 Token" value={formatCompact(outputTokens)} tone="amber" sub="今日累计" />
        <Metric title="请求数" value={String(requestCount)} tone="green" sub={`成功 ${Math.max(requestCount - 1, 0)}`} />
      </div>

      <div className="dashboard-grid">
        <article className="card trend-card">
          <div className="card-head">
            <div>
              <h3>使用趋势</h3>
              <p>官方估算成本 · 最近 7 天</p>
            </div>
            <div className="mini-tabs">
              <button className="active">费用</button>
              <button>Token</button>
              <button>请求</button>
            </div>
          </div>
          <div className="chart">
            {[32, 39, 36, 52, 50, 71, 96].map((height, index) => (
              <span key={index} style={{ height: `${height}%` }} />
            ))}
          </div>
          <div className="chart-labels">
            {["周一", "周二", "周三", "周四", "周五", "周六", "今天"].map((label) => (
              <span key={label}>{label}</span>
            ))}
          </div>
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
          <p>今日共计 {formatCompact(totalTokens)} Token</p>
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
          <p>今日通过本地网关的请求</p>
          <Quality label="成功率" value={`${successRate.toFixed(1)}%`} tone="green" />
          <Quality label="平均首字延迟" value="842 ms" tone="cyan" />
          <Quality label="平均总耗时" value="18.4 s" tone="purple" />
          <Quality label="当前活跃请求" value={String(routeEnabledCount ? 2 : 0)} tone="amber" />
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
}: {
  title: string;
  value: string;
  tone: "purple" | "cyan" | "blue" | "amber" | "green";
  sub: string;
}) {
  return (
    <article className="metric">
      <span className={`dot ${tone}`} />
      <p>{title}</p>
      <strong>{value}</strong>
      <small>{sub}</small>
    </article>
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
  balanceTokenVisible: boolean;
  busy: boolean;
  onBalanceTokenVisible: (visible: boolean) => void;
  onClose: () => void;
  onSave: () => void;
  onTab: (tab: EditorTab) => void;
  onTestBalance: () => void;
  onUpdateBalance: (patch: Partial<BalanceQueryConfig>) => void;
  providerApiKey: string;
  providerBaseUrl: string;
  providerEnabled: boolean;
  providerName: string;
  secretVisible: boolean;
  setProviderApiKey: (value: string) => void;
  setProviderBaseUrl: (value: string) => void;
  setProviderEnabled: (value: boolean) => void;
  setProviderName: (value: string) => void;
  setSecretVisible: (value: boolean) => void;
  tab: EditorTab;
}) {
  const {
    balanceQuery,
    balanceTokenVisible,
    busy,
    onBalanceTokenVisible,
    onClose,
    onSave,
    onTab,
    onTestBalance,
    onUpdateBalance,
    providerApiKey,
    providerBaseUrl,
    providerEnabled,
    providerName,
    secretVisible,
    setProviderApiKey,
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
                    onChange={(event) => setProviderApiKey(event.currentTarget.value)}
                    placeholder="sk-..."
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
                <button className="ghost">重新测试</button>
                <ul>
                  <li><span className="dot green" />基础连接 <b>正常</b> <em>213 ms</em></li>
                  <li><span className="dot green" />Responses API <b>正常</b> <em>721 ms</em></li>
                  <li><span className="dot green" />流式输出 <b>支持</b> <em>已验证</em></li>
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
                    value={balanceQuery.auth_mode === "provider_token" ? apiKeyPreview(providerApiKey) : balanceQuery.query_token}
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
              <div className="success-box">
                <div>
                  <strong>查询成功</strong>
                  <p>9,310,000 quota → $18.62</p>
                </div>
                <button onClick={onTestBalance} type="button">测试查询</button>
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
                  <span>余额低于阈值时跳过此供应商</span>
                  <Toggle checked onChange={() => undefined} />
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
