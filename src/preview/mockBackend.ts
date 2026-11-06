import { mockIPC } from "@tauri-apps/api/mocks";
import type { TraySnapshot } from "../trayTypes";
import type {
  AppState,
  BalanceQueryConfig,
  ClaudeProviderConfig,
  ProviderConfig,
  ProviderSummary,
  RouteLogFilter,
  RouteRequestLog,
  RouteUsageBucket,
  RouteUsageStats,
  SkillManagementView,
  UsageSummary,
} from "../types";

// Preview fixtures are synthetic. There is deliberately no fetch, file access,
// local gateway connection, persistent state, or native command in this module.
const dayKey = (date: Date) =>
  `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
const today = dayKey(new Date());
const models = ["gpt-5.4", "gpt-5.4-mini", "gpt-5.3-codex"];
const claudeModels = ["claude-sonnet-4-6", "claude-opus-4-6"];
const balanceQuery: BalanceQueryConfig = {
  enabled: true,
  query_type: "new_api",
  new_api_target: "token_quota",
  endpoint: "",
  path: "/api/usage/token/",
  auth_mode: "provider_token",
  query_token: "",
  new_api_user_id: "",
};

function provider(
  id: string,
  name: string,
  base_url: string,
  status: ProviderSummary["status"],
  latency_ms: number,
  balance_label: string,
): ProviderSummary {
  return {
    id,
    name,
    base_url,
    status,
    enabled: status === "enabled",
    provider_type: "custom",
    pending_changes: 0,
    auto_disable_override: false,
    auto_disable_enabled: true,
    auto_disable_failure_threshold: 3,
    consecutive_failure_count: status === "auto_disabled" ? 3 : 0,
    auto_disable_reset_period: "daily",
    auto_disable_reset_weekday: 1,
    auto_disable_reset_day: 1,
    auto_disable_reset_time: "00:00",
    balance_label,
    latency_ms,
    latency_label: `${latency_ms} ms`,
  };
}

export function installPreview() {
  const state: AppState = {
    app_version: "v1.2.46",
    capability_version: "2026.11.05",
    capability_updated_at: "2026-11-05 09:00",
    codex_config_path: "~/.codex/config.toml",
    claude_settings_path: "~/.claude/settings.json",
    pi_models_path: "~/.pi/agent/models.json",
    manager_dir: "~/.xxswitch",
    current_config_raw: 'model_provider = "xxswitch"',
    current_config_exists: true,
    active_provider_id: "preview-openai",
    active_claude_provider_id: "preview-anthropic",
    active_provider: null,
    active_claude_provider: null,
    active_provider_toml: "",
    final_preview_toml: "",
    diffs: [],
    providers: [
      provider(
        "preview-openai",
        "OpenAI 主线路",
        "https://api.example.com/v1",
        "enabled",
        286,
        "$ 128.50",
      ),
      provider(
        "preview-openrouter",
        "OpenRouter",
        "https://router.example.com/api/v1",
        "enabled",
        452,
        "$ 42.80",
      ),
      provider(
        "preview-local",
        "本地 Ollama",
        "http://127.0.0.1:11434/v1",
        "disabled",
        84,
        "未配置",
      ),
      {
        ...provider(
          "preview-backup",
          "备用网关",
          "https://backup.example.com/v1",
          "auto_disabled",
          0,
          "$ 16.20",
        ),
        latency_ms: null,
        latency_label: "未测试",
        last_failure_reason: "模拟上游超时，已自动停用",
      },
    ],
    claude_providers: [
      provider(
        "preview-anthropic",
        "Anthropic",
        "https://claude.example.com/v1",
        "enabled",
        318,
        "未配置",
      ),
      provider(
        "preview-claude-backup",
        "Claude 备用",
        "https://relay.example.com/v1",
        "disabled",
        521,
        "未配置",
      ),
    ],
    router: {
      enabled: true,
      debug_mode: true,
      force_disable_openai_auth: false,
      remote_compaction_enabled: false,
      token_budget_enabled: false,
      websockets_enabled: true,
      zstd_decompression_enabled: true,
      gpt56_long_context_enabled: false,
      gpt56_long_context_window: 372000,
      auto_disable_enabled: true,
      auto_disable_failure_threshold: 3,
      model_provider: "xxswitch",
      codex_model: models[0],
      host: "127.0.0.1",
      port: 18080,
      local_token: "preview-local-token",
      connect_timeout_secs: 30,
      response_header_timeout_secs: 120,
      stream_idle_timeout_secs: 120,
      slow_mode: "off",
      slow_delay_secs: 5,
      slow_delay_max_secs: 10,
    },
    codex_models: models,
    clients: {
      codex: {
        enabled: true,
        managed_skill_location: "~/.codex/skills",
        skill_locations: [{ path: "~/.codex/skills", writable: true }],
      },
      claude: {
        enabled: true,
        managed_skill_location: "~/.claude/skills",
        skill_locations: [{ path: "~/.claude/skills", writable: true }],
      },
      pi: {
        enabled: false,
        managed_skill_location: "~/.pi/agent/skills",
        skill_locations: [{ path: "~/.pi/agent/skills", writable: true }],
      },
    },
    multi_agent_enabled: false,
    router_status: { running: true, address: "127.0.0.1:18080" },
  };
  const providerDetails = new Map<string, ProviderConfig>();
  const claudeDetails = new Map<string, ClaudeProviderConfig>();
  function makeDetails(summary: ProviderSummary): ProviderConfig {
    return {
      ...summary,
      fusion_enabled: false,
      config: {
        model_providers: {
          custom: {
            base_url: summary.base_url,
            experimental_bearer_token: "sk-preview-not-a-real-key",
          },
        },
      },
      wire_api: "responses",
      image_detail_original_to_auto: false,
      supports_websockets: true,
      service_tier: "",
      connection_test_model: models[0],
      allowed_models: [...models],
      model_mappings: [],
      balance_query: { ...balanceQuery },
    };
  }
  for (const summary of state.providers)
    providerDetails.set(summary.id, makeDetails(summary));
  for (const summary of state.claude_providers)
    claudeDetails.set(summary.id, {
      ...summary,
      fusion_enabled: false,
      api_key: "sk-preview-not-a-real-key",
      connection_test_model: claudeModels[0],
      allowed_models: [...claudeModels],
      model_mappings: [],
    });

  const skills: SkillManagementView = {
    library_root: "~/.xxswitch/skills",
    clients: (
      Object.keys(state.clients) as Array<keyof typeof state.clients>
    ).map((client) => ({
      client,
      label:
        client === "codex"
          ? "Codex"
          : client === "claude"
            ? "Claude Code"
            : "Pi Coding Agent",
      managed_skill_location: state.clients[client].managed_skill_location,
      skill_locations: state.clients[client].skill_locations.map(
        (location) => ({ ...location, managed: true, exists: true }),
      ),
    })),
    shared_skills: [
      {
        identity: "code-review",
        description: "审查代码变更，检查正确性、可维护性与项目约定。",
        library_dir_name: "code-review",
        path: "~/.xxswitch/skills/code-review",
        sharing_scope: ["codex", "claude"],
        exposures: [
          {
            client: "codex",
            client_label: "Codex",
            path: "~/.codex/skills/code-review",
            health: "healthy",
            message: "共享正常",
          },
          {
            client: "claude",
            client_label: "Claude Code",
            path: "~/.claude/skills/code-review",
            health: "healthy",
            message: "共享正常",
          },
        ],
      },
      {
        identity: "frontend-design",
        description: "构建清晰、一致且易于使用的前端界面。",
        library_dir_name: "frontend-design",
        path: "~/.xxswitch/skills/frontend-design",
        sharing_scope: ["codex"],
        exposures: [
          {
            client: "codex",
            client_label: "Codex",
            path: "~/.codex/skills/frontend-design",
            health: "healthy",
            message: "共享正常",
          },
        ],
      },
    ],
    client_skills: [
      {
        client: "codex",
        client_label: "Codex",
        skill_location: "~/.codex/skills",
        path: "~/.codex/skills/research",
        dir_name: "research",
        identity: "research",
        description: "收集可靠资料，记录研究结论。",
        managed: false,
        shared: false,
      },
      {
        client: "claude",
        client_label: "Claude Code",
        skill_location: "~/.claude/skills",
        path: "~/.claude/skills/testing",
        dir_name: "testing",
        identity: "testing",
        description: "设计有意义的测试，验证关键用户流程。",
        managed: false,
        shared: false,
      },
    ],
    conflicts: [],
  };
  const midnight = new Date();
  midnight.setHours(0, 0, 0, 0);
  const elapsed = Math.max(Date.now() - midnight.getTime(), 1);
  const logs: RouteRequestLog[] = Array.from({ length: 1284 }, (_, index) => {
    const started_at_ms = Date.now() - Math.floor((index / 1284) * elapsed);
    const uncached_input_tokens = 320 + ((index * 197) % 1800);
    const cached_input_tokens =
      1800 +
      Math.round(2400 * (1 + Math.sin(index / 55))) +
      ((index * 71) % 1400);
    const output_tokens = 100 + ((index * 37) % 1800);
    const status =
      index === 0 ? "running" : index % 131 === 7 ? "failed" : "success";
    const summary = state.providers[index % 5 === 2 ? 1 : 0];
    return {
      id: `preview-request-${String(1284 - index).padStart(5, "0")}`,
      started_at_ms,
      day: today,
      hour: `${String(new Date(started_at_ms).getHours()).padStart(2, "0")}:00`,
      method: "POST",
      path: "/v1/responses",
      model: models[index % models.length],
      upstream_model: models[index % models.length],
      requested_service_tier: index % 3 ? "auto" : "priority",
      actual_service_tier: index % 3 ? "default" : "priority",
      provider_id: summary.id,
      provider_name: summary.name,
      provider_order: summary.id === "preview-openai" ? 1 : 2,
      upstream_chain: [summary.name],
      status,
      status_code: status === "failed" ? 429 : 200,
      error: status === "failed" ? "模拟上游限流，请稍后重试。" : null,
      route_result: index % 9 === 4 ? "故障切换 · 备用线路" : "直连",
      route_attempts: index % 9 === 4 ? 2 : 1,
      input_tokens: uncached_input_tokens + cached_input_tokens,
      uncached_input_tokens,
      cached_input_tokens,
      output_tokens,
      reasoning_output_tokens: Math.floor(output_tokens / 4),
      total_tokens: uncached_input_tokens + cached_input_tokens + output_tokens,
      first_byte_ms: 220 + ((index * 17) % 480),
      total_ms: 1200 + ((index * 227) % 18000),
      upstream_started_ms: 12,
      response_header_ms: 108 + (index % 90),
      debug_capture_available: index < 12,
      upstream_request_id: `preview-upstream-${index}`,
    };
  });
  if (
    new URLSearchParams(window.location.search).get("scenario") === "long-names"
  ) {
    const encode = (id: string) =>
      Array.from(new TextEncoder().encode(id), (byte) =>
        byte.toString(16).padStart(2, "0"),
      ).join("");
    const otherId = `preview-route-${"ab12".repeat(20)}`;
    state.providers[1].id = otherId;
    state.providers[1].name = "OpenRouter 备用线路";
    state.providers[2].name = `长供应商名称-${"provider".repeat(20)}`;
    const longModels = [
      `xxswitch-${encode(state.providers[0].id)}/gpt-6.1-sol`,
      `xxswitch-${encode(otherId)}/gpt-6.1-sol`,
      `xxswitch-${encode(state.providers[0].id)}/gpt-6-astra`,
      `unscoped-${"extended-model".repeat(20)}`,
    ];
    logs.forEach((log, index) => {
      const provider = state.providers[index % 3];
      log.model = longModels[index % longModels.length];
      log.provider_id = provider.id;
      log.provider_name = provider.name;
    });
  }
  if (
    new URLSearchParams(window.location.search).get("scenario") ===
    "chart-large"
  ) {
    const template = logs[1];
    const previousMonth = new Date(
      midnight.getFullYear(),
      midnight.getMonth() - 1,
      15,
      12,
    );
    const chartLogs = [1_234_000_000, 24_500_000].map((total_tokens, index) => {
      const date = index === 0 ? previousMonth : new Date();
      const uncached_input_tokens = total_tokens / 10;
      const cached_input_tokens = total_tokens * 0.8;
      const output_tokens = total_tokens / 10;
      return {
        ...template,
        id: `preview-chart-${index}`,
        started_at_ms: date.getTime(),
        day: dayKey(date),
        hour: `${String(date.getHours()).padStart(2, "0")}:00`,
        uncached_input_tokens,
        cached_input_tokens,
        output_tokens,
        input_tokens: uncached_input_tokens + cached_input_tokens,
        reasoning_output_tokens: 0,
        total_tokens,
      };
    });
    logs.splice(0, logs.length, ...chartLogs);
  }
  if (new URLSearchParams(window.location.search).get("scenario") === "empty") {
    state.providers = [];
    state.claude_providers = [];
    state.codex_models = [];
    state.active_provider_id = "";
    logs.splice(0);
    skills.shared_skills = [];
    skills.client_skills = [];
  }
  if (
    new URLSearchParams(window.location.search).get("scenario") === "offline"
  ) {
    state.router.enabled = false;
    state.router_status.running = false;
  }

  function filtered(filter: RouteLogFilter = {}) {
    return logs.filter(
      (log) =>
        (!filter.status || log.status === filter.status) &&
        (!filter.provider_id || log.provider_id === filter.provider_id) &&
        (!filter.model || log.model === filter.model) &&
        (!filter.start_day || log.day >= filter.start_day) &&
        (!filter.end_day || log.day <= filter.end_day) &&
        (!filter.query ||
          `${log.id} ${log.model} ${log.provider_name} ${log.error ?? ""}`
            .toLowerCase()
            .includes(filter.query.toLowerCase())),
    );
  }
  const emptySummary = (): UsageSummary => ({
    request_count: 0,
    input_tokens: 0,
    uncached_input_tokens: 0,
    cached_input_tokens: 0,
    output_tokens: 0,
    reasoning_output_tokens: 0,
    total_tokens: 0,
  });
  function summary(rows: RouteRequestLog[]): UsageSummary {
    return rows.reduce((sum, row) => {
      sum.request_count += 1;
      for (const key of [
        "input_tokens",
        "uncached_input_tokens",
        "cached_input_tokens",
        "output_tokens",
        "reasoning_output_tokens",
        "total_tokens",
      ] as const)
        sum[key] += row[key];
      return sum;
    }, emptySummary());
  }
  function logResponse(filter: RouteLogFilter = {}) {
    const rows = filtered(filter);
    const page = filter.page ?? 1,
      page_size = filter.page_size ?? 20;
    return {
      logs: rows.slice((page - 1) * page_size, page * page_size),
      total: rows.length,
      page,
      page_size,
      total_pages: Math.max(1, Math.ceil(rows.length / page_size)),
      available_providers: state.providers.slice(0, 2).map((item) => ({
        id: item.id,
        name: item.name,
        request_count: logs.filter((log) => log.provider_id === item.id).length,
      })),
      available_models: models,
      available_days: [today],
    };
  }
  function usage(filter: RouteLogFilter = {}): RouteUsageStats {
    const rows = filtered(filter),
      successful = rows.filter((row) => row.status === "success");
    const group = (key: "provider_name" | "model") =>
      [...new Set(successful.map((row) => row[key]))].map((label) => ({
        ...summary(successful.filter((row) => row[key] === label)),
        key: label,
        label,
      }));
    const hourly =
      filter.start_day === filter.end_day && Boolean(filter.start_day);
    const monthly = !filter.start_day && !filter.end_day;
    const bucketLabel = (row: RouteRequestLog) =>
      hourly ? row.hour : monthly ? row.day.slice(0, 7) : row.day;
    const buckets: RouteUsageBucket[] = [
      ...new Set(successful.map(bucketLabel)),
    ]
      .sort()
      .map((label) => ({
        ...summary(successful.filter((row) => bucketLabel(row) === label)),
        label,
      }));
    return {
      ...logResponse(filter),
      generated_at_ms: Date.now(),
      filters: filter,
      summary: summary(successful),
      today: summary(successful.filter((row) => row.day === today)),
      failed_count: rows.filter((row) => row.status === "failed").length,
      success_count: successful.length,
      running_count: rows.filter((row) => row.status === "running").length,
      average_first_byte_ms: successful.length
        ? Math.round(
            successful.reduce((n, row) => n + (row.first_byte_ms ?? 0), 0) /
              successful.length,
          )
        : null,
      average_total_ms: successful.length
        ? Math.round(
            successful.reduce((n, row) => n + row.total_ms, 0) /
              successful.length,
          )
        : null,
      bucket_granularity: hourly ? "hour" : monthly ? "month" : "day",
      buckets,
      providers: group("provider_name"),
      models: group("model"),
      details: successful.slice(0, 8),
    };
  }
  let autostart = false;
  let trayPanelVisible = true;
  let trayIconVisible = true;
  let trayNavigation: string | null = null;
  function traySnapshot(): TraySnapshot {
    return structuredClone({
      app_version: state.app_version,
      router_status: state.router_status,
      clients: {
        codex: state.clients.codex.enabled,
        claude: state.clients.claude.enabled,
        pi: state.clients.pi.enabled,
      },
      codex_model: state.router.codex_model,
      codex_models: state.codex_models,
      codex_provider_count: state.providers.filter(
        (provider) => provider.status === "enabled",
      ).length,
      claude_provider_count: state.claude_providers.filter(
        (provider) => provider.status === "enabled",
      ).length,
      today: summary(
        logs.filter((log) => log.day === today && log.status === "success"),
      ),
      usage_error: null,
      panel_visible: trayPanelVisible,
    });
  }
  let nextId = 0;
  let pendingImport =
    new URLSearchParams(window.location.search).get("scenario") === "import"
      ? {
          id: 1,
          app: "codex" as const,
          name: "网页导入预览",
          endpoint: "https://import.example.com/v1",
          api_key_masked: "sk-••••••••",
          api_format: "openai_responses",
          models: ["gpt-5.4"],
          enabled: false,
          error: null,
        }
      : null;
  let sync = {
    url: "https://dav.example.com/dav/",
    username: "preview-user",
    directory: "xxswitch-sync",
    password_set: true,
    auto_upload: false,
    last_sync_at: null as string | null,
    message: "",
    error: false,
  };

  mockIPC(
    (command, args) => {
      // The type narrowing is confined to this synthetic IPC boundary, matching
      // the application's existing command shapes; no payload is forwarded.
      const input = (args ?? {}) as Record<string, unknown>;
      const payload = (input.payload ?? {}) as Record<string, unknown>;
      const filter = (payload.filter ?? {}) as RouteLogFilter;
      if (command === "load_tray_snapshot") return traySnapshot();
      if (command === "get_tray_preferences")
        return { visible: trayIconVisible };
      if (command === "set_tray_visibility") {
        trayIconVisible = Boolean(input.visible);
        if (!trayIconVisible) trayPanelVisible = false;
        return { visible: trayIconVisible };
      }
      if (command === "take_tray_navigation") {
        const next = trayNavigation;
        trayNavigation = null;
        return next;
      }
      if (command === "tray_action") {
        if (input.action === "show") trayPanelVisible = true;
        else if (
          [
            "hide",
            "quit",
            "dashboard",
            "route",
            "providers",
            "usage",
            "settings",
          ].includes(String(input.action))
        ) {
          trayPanelVisible = false;
          if (input.action !== "hide" && input.action !== "quit")
            trayNavigation = String(input.action);
        } else throw new Error("未知快捷面板操作");
        return;
      }
      if (command === "update_tray_client") {
        if (
          typeof payload.codex_model === "string" &&
          payload.client === undefined &&
          payload.enabled === undefined
        ) {
          if (!state.clients.codex.enabled)
            throw new Error("请先开启 Codex 接管");
          if (!state.codex_models.includes(payload.codex_model))
            throw new Error("所选模型不可用");
          state.router.codex_model = payload.codex_model;
        } else if (
          ["codex", "claude", "pi"].includes(String(payload.client)) &&
          typeof payload.enabled === "boolean" &&
          payload.codex_model === undefined
        ) {
          const client = payload.client as "codex" | "claude" | "pi";
          if (
            payload.enabled &&
            client !== "claude" &&
            !state.codex_models.length
          )
            throw new Error(
              `${client === "codex" ? "Codex" : "Pi"} 接管需要至少一个已配置的路由模型`,
            );
          state.clients[client].enabled = payload.enabled;
          if (payload.enabled) {
            state.router.enabled = true;
            state.router_status.running = true;
          }
        } else throw new Error("快捷面板只接受单项客户端变更");
        return traySnapshot();
      }
      if (
        command === "load_app_state" ||
        command === "apply_config" ||
        command === "update_model_capabilities"
      )
        return structuredClone(state);
      if (command === "load_route_logs") return logResponse(filter);
      if (command === "load_route_usage_stats") return usage(filter);
      if (command === "load_skill_management") return structuredClone(skills);
      if (command === "peek_provider_import")
        return structuredClone(pendingImport);
      if (command === "dismiss_provider_import") {
        if (pendingImport?.id === input.id) pendingImport = null;
        return null;
      }
      if (command === "confirm_provider_import") {
        if (!pendingImport || pendingImport.id !== input.id)
          throw new Error("导入请求已处理。");
        const id = "preview-imported";
        const enabled = input.enabled === true;
        const item = provider(
          id,
          pendingImport.name,
          pendingImport.endpoint,
          enabled ? "enabled" : "disabled",
          0,
          "未配置",
        );
        state.providers.push(item);
        const template = providerDetails.get("preview-openai")!;
        providerDetails.set(id, {
          ...structuredClone(template),
          id,
          name: pendingImport.name,
          enabled,
          status: item.status,
          config: {
            model_providers: {
              custom: {
                base_url: pendingImport.endpoint,
                experimental_bearer_token: "sk-preview-not-a-real-key",
              },
            },
          },
        });
        pendingImport = null;
        return structuredClone(state);
      }
      if (command === "get_provider")
        return structuredClone(providerDetails.get(String(input.providerId)));
      if (command === "get_claude_provider")
        return structuredClone(claudeDetails.get(String(input.providerId)));
      if (
        command === "load_provider_models" ||
        command === "load_claude_provider_models"
      )
        return {
          models: command === "load_provider_models" ? models : claudeModels,
        };
      if (command === "get_autostart_enabled") return autostart;
      if (command === "set_autostart_enabled") {
        autostart = Boolean(input.enabled);
        return autostart;
      }
      if (command === "check_for_update")
        return {
          current_version: state.app_version,
          latest_version: state.app_version,
          available: false,
          installable: false,
          release_url: "https://example.com/preview",
        };
      if (command === "save_client_configs") {
        state.clients.codex.enabled = Boolean(payload.codex_enabled);
        state.clients.claude.enabled = Boolean(payload.claude_enabled);
        state.clients.pi.enabled = Boolean(payload.pi_enabled);
        return structuredClone(state);
      }
      if (command === "save_router_config") {
        Object.assign(state.router, payload);
        state.router_status.running = state.router.enabled;
        return structuredClone(state);
      }
      if (command === "save_multi_agent_enabled") {
        state.multi_agent_enabled = Boolean(payload.enabled);
        return structuredClone(state);
      }
      if (command === "add_provider" || command === "add_claude_provider") {
        const item = provider(
          `preview-new-${++nextId}`,
          String(input.name),
          "",
          "disabled",
          0,
          "未配置",
        );
        if (command === "add_provider") {
          state.providers.push(item);
          providerDetails.set(item.id, makeDetails(item));
        } else {
          state.claude_providers.push(item);
          claudeDetails.set(item.id, {
            ...item,
            fusion_enabled: false,
            api_key: "",
            connection_test_model: "",
            allowed_models: [],
            model_mappings: [],
          });
        }
        return structuredClone(state);
      }
      if (command === "save_provider" || command === "save_claude_provider") {
        const list =
          command === "save_provider"
            ? state.providers
            : state.claude_providers;
        const item = list.find((row) => row.id === payload.provider_id);
        if (item) {
          item.name = String(payload.provider_name ?? item.name);
          item.base_url = String(payload.base_url ?? item.base_url);
          for (const key of [
            "auto_disable_override",
            "auto_disable_enabled",
            "auto_disable_failure_threshold",
            "auto_disable_reset_period",
            "auto_disable_reset_weekday",
            "auto_disable_reset_day",
            "auto_disable_reset_time",
          ] as const) {
            if (payload[key] !== undefined)
              Object.assign(item, { [key]: payload[key] });
          }
          if (typeof payload.enabled === "boolean") {
            item.enabled = payload.enabled;
            item.status = payload.enabled ? "enabled" : "disabled";
          }
          if (command === "save_provider") {
            const details = providerDetails.get(item.id);
            if (details)
              Object.assign(details, payload, item, {
                config: {
                  model_providers: {
                    custom: {
                      base_url: item.base_url,
                      experimental_bearer_token:
                        payload.api_key ?? "sk-preview-not-a-real-key",
                    },
                  },
                },
              });
          } else {
            const details = claudeDetails.get(item.id);
            if (details) Object.assign(details, payload, item);
          }
        }
        return structuredClone(state);
      }
      if (
        command === "delete_provider" ||
        command === "delete_claude_provider"
      ) {
        if (command === "delete_provider")
          state.providers = state.providers.filter(
            (row) => row.id !== payload.provider_id,
          );
        else
          state.claude_providers = state.claude_providers.filter(
            (row) => row.id !== payload.provider_id,
          );
        return structuredClone(state);
      }
      if (
        command === "reorder_providers" ||
        command === "reorder_claude_providers"
      ) {
        const key =
          command === "reorder_providers" ? "providers" : "claude_providers";
        const ids = payload.provider_ids as string[];
        state[key].sort((a, b) => ids.indexOf(a.id) - ids.indexOf(b.id));
        return structuredClone(state);
      }
      if (command === "query_provider_balance") {
        const details = providerDetails.get(String(payload.provider_id));
        if (details)
          details.balance_status = {
            amount: "128.50",
            label: "$ 128.50",
            checked_at: Date.now(),
            accounts: [
              {
                account_id: 1,
                account_name: "预览账号",
                available_balance: 128.5,
                unit: "USD",
              },
            ],
          };
        return structuredClone(state);
      }
      if (command === "test_provider_latency_state")
        return {
          app_state: structuredClone(state),
          ok: true,
          latency_ms: 286,
          reply: "你好！这是由模拟数据生成的预览回复。没有请求真实模型。",
        };
      if (command === "load_route_debug_capture") {
        const body = {
          text: JSON.stringify(
            { model: models[0], input: "这是一条模拟请求。", stream: true },
            null,
            2,
          ),
          captured_bytes: 96,
          total_bytes: 96,
          truncated: false,
        };
        return {
          version: 1,
          request_id: String(payload.request_id),
          started_at_ms: Date.now(),
          provider_name: "OpenAI 主线路",
          inbound_request: {
            method: "POST",
            url: "http://127.0.0.1:18080/v1/responses",
            headers: [{ name: "content-type", value: "application/json" }],
            body,
          },
          upstream_request: {
            method: "POST",
            url: "https://api.example.com/v1/responses",
            headers: [{ name: "authorization", value: "Bearer [REDACTED]" }],
            body,
          },
          upstream_response: {
            status_code: 200,
            headers: [{ name: "content-type", value: "text/event-stream" }],
            body: {
              ...body,
              text: 'data: {"type":"response.completed"}\n\ndata: [DONE]',
            },
          },
        };
      }
      if (command === "get_provider_sync") return { ...sync };
      if (command === "save_provider_sync") {
        Object.assign(sync, input.draft);
        return { ...sync };
      }
      if (command === "test_provider_sync") return "模拟连接成功，未访问网络。";
      if (command === "preview_provider_sync")
        return {
          token: "preview-sync",
          remote_exists: true,
          local_codex: state.providers.length,
          local_claude: state.claude_providers.length,
          remote_codex: 3,
          remote_claude: 1,
          remote_created_at: new Date().toISOString(),
          same: false,
        };
      if (command === "cancel_provider_sync_preview") return null;
      if (
        command === "upload_provider_sync" ||
        command === "download_provider_sync"
      ) {
        sync = {
          ...sync,
          last_sync_at: new Date().toISOString(),
          message: "模拟同步完成，配置仅保存在预览内存中。",
        };
        return { ...sync };
      }
      if (command === "set_skill_sharing_scope") {
        const item = skills.shared_skills.find(
          (row) => row.identity === payload.skill_identity,
        );
        if (item) {
          item.sharing_scope =
            payload.sharing_scope as typeof item.sharing_scope;
          item.exposures = item.sharing_scope.map((client) => {
            const target = skills.clients.find(
              (entry) => entry.client === client,
            );
            return {
              client,
              client_label: target?.label ?? client,
              path: `${target?.managed_skill_location}/${item.identity}`,
              health: "healthy",
              message: "共享正常",
            };
          });
        }
        return structuredClone(skills);
      }
      if (command === "delete_shared_skill") {
        skills.shared_skills = skills.shared_skills.filter(
          (item) => item.identity !== payload.skill_identity,
        );
        return structuredClone(skills);
      }
      if (command === "save_skill_client_config") {
        const client = skills.clients.find(
          (item) => item.client === payload.client,
        );
        if (client)
          Object.assign(client, {
            skill_locations: payload.skill_locations,
            managed_skill_location: payload.managed_skill_location,
          });
        return structuredClone(skills);
      }
      if (
        command === "promote_client_skill" ||
        command === "replace_client_skill_with_shared"
      ) {
        const item = skills.client_skills.find(
          (skill) => skill.path === payload.skill_path,
        );
        if (item) {
          skills.shared_skills.push({
            ...item,
            library_dir_name: item.dir_name,
            sharing_scope: [item.client],
            exposures: [],
          });
          skills.client_skills = skills.client_skills.filter(
            (skill) => skill !== item,
          );
        }
        return structuredClone(skills);
      }
      if (command.startsWith("plugin:opener|"))
        throw new Error("模拟预览不打开真实文件或外部应用。");
      throw new Error(`预览尚未模拟此操作：${command}`);
    },
    { shouldMockEvents: true },
  );
}
