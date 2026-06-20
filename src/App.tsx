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

type Screen = "main" | "create" | "edit" | "current" | "settings";
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
    };
    mockProviderStore = mockProviderStore.map((provider) => ({
      id: provider.id,
      name:
        provider.id === payload.provider_id && payload.provider_name
          ? payload.provider_name
          : provider.name,
      enabled: provider.enabled,
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
    };
    const previewProviders = mockProviderStore.map((provider) => ({
      id: provider.id,
      name:
        provider.id === payload.provider_id && payload.provider_name
          ? payload.provider_name
          : provider.name,
      enabled: provider.enabled,
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

  async function refresh() {
    setError("");
    const state = await callCommand<AppState>("load_app_state");
    setAppState(state);
    setProviderName(state.active_provider?.name ?? "");
    setProviderText(state.active_provider_toml);
    const custom = parseCustomProviderFields(state.active_provider_toml);
    setCustomBaseUrl(custom.baseUrl);
    setCustomToken(custom.token);
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
    setCustomBaseUrl(baseUrl);
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
        },
      });
      const state = await callCommand<AppState>("apply_config");
      setAppState(state);
      setPreviewState(null);
      setScreen("main");
    });
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
                <span className={`status ${appState.marker_present ? "ok" : "warn"}`}>
                  {appState.marker_present ? "标记已接管" : "首次接管"}
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

            <section className="panel compact">
              <div className="apply-strip">
                <div>
                  <span>目标文件</span>
                  <strong>{appState.codex_config_path}</strong>
                </div>
                <div>
                  <span>基础模板</span>
                  <strong>{appState.base_template_name}</strong>
                </div>
                <div>
                  <span>校验</span>
                  <strong>本地 TOML 正常</strong>
                </div>
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
                  <span>接管标记</span>
                  <strong>{appState.marker_present ? "已接管" : "未接管"}</strong>
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
                      {previewState.marker_present ? "已有接管标记" : "将加入接管标记"}
                    </span>
                  </>
                ) : (
                  <>
                    <span className="status ok">
                      {previewState.current_config_exists ? "已读取" : "尚未创建"}
                    </span>
                    <span className="status neutral">
                      {previewState.marker_present ? "已有接管标记" : "无接管标记"}
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
