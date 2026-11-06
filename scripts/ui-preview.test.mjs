import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import test from "node:test";
import { transformWithEsbuild } from "vite";
import { invoke } from "@tauri-apps/api/core";
import { clearMocks } from "@tauri-apps/api/mocks";

test("UI preview exercises application commands in memory without network access", async () => {
  const source = await readFile(
    new URL("../src/preview/mockBackend.ts", import.meta.url),
    "utf8",
  );
  const compiled = await transformWithEsbuild(source, "mockBackend.ts", {
    loader: "ts",
    format: "esm",
  });
  const code = compiled.code.replaceAll(
    '"@tauri-apps/api/mocks"',
    JSON.stringify(import.meta.resolve("@tauri-apps/api/mocks")),
  );
  const previousWindow = globalThis.window;
  const previousFetch = globalThis.fetch;
  let networkCalls = 0;
  globalThis.window = { location: { search: "" } };
  globalThis.fetch = () => {
    networkCalls += 1;
    throw new Error("Preview attempted a real network request");
  };

  try {
    const { installPreview } = await import(
      `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`
    );
    installPreview();
    const original = await invoke("load_app_state");
    assert.equal(original.providers.length, 4);

    const logs = await invoke("load_route_logs", { payload: { filter: {} } });
    assert.equal(logs.total, 1284);
    assert.equal(logs.logs.length, 20);
    const filtered = await invoke("load_route_logs", {
      payload: { filter: { status: "failed", model: "gpt-5.4" } },
    });
    assert.ok(filtered.logs.length > 0);
    assert.ok(
      filtered.logs.every(
        (row) => row.status === "failed" && row.model === "gpt-5.4",
      ),
    );
    const secondPage = await invoke("load_route_logs", {
      payload: { filter: { page: 2 } },
    });
    assert.notEqual(secondPage.logs[0].id, logs.logs[0].id);

    const stats = await invoke("load_route_usage_stats", {
      payload: { filter: {} },
    });
    assert.equal(
      stats.summary.total_tokens,
      stats.summary.uncached_input_tokens +
        stats.summary.cached_input_tokens +
        stats.summary.output_tokens,
    );
    const ids = original.providers.map((row) => row.id).reverse();
    const reordered = await invoke("reorder_providers", {
      payload: { provider_ids: ids },
    });
    assert.deepEqual(
      reordered.providers.map((row) => row.id),
      ids,
    );
    const toggled = await invoke("save_provider", {
      payload: { provider_id: original.providers[0].id, enabled: false },
    });
    assert.equal(
      toggled.providers.find((row) => row.id === original.providers[0].id)
        .status,
      "disabled",
    );
    assert.equal(
      original.providers[0].status,
      "enabled",
      "Earlier IPC snapshots remain independent",
    );
    await invoke("save_provider", {
      payload: {
        provider_id: original.providers[0].id,
        auto_disable_reset_period: "weekly",
        auto_disable_reset_weekday: 5,
      },
    });
    const savedPolicy = await invoke("get_provider", {
      providerId: original.providers[0].id,
    });
    assert.equal(savedPolicy.auto_disable_reset_period, "weekly");
    assert.equal(savedPolicy.auto_disable_reset_weekday, 5);

    const skills = await invoke("set_skill_sharing_scope", {
      payload: { skill_identity: "code-review", sharing_scope: ["pi"] },
    });
    assert.deepEqual(skills.shared_skills[0].sharing_scope, ["pi"]);
    const promoted = await invoke("promote_client_skill", {
      payload: { client: "codex", skill_path: "~/.codex/skills/research" },
    });
    assert.ok(
      promoted.shared_skills.some((row) => row.identity === "research"),
    );
    const latency = await invoke("test_provider_latency_state");
    assert.equal(latency.ok, true);
    const capture = await invoke("load_route_debug_capture", {
      payload: { request_id: logs.logs[0].id },
    });
    assert.ok(
      capture.upstream_request.headers.every(
        (header) => !header.value.includes("sk-preview"),
      ),
    );

    const tray = await invoke("load_tray_snapshot");
    assert.equal(tray.panel_visible, true);
    assert.deepEqual(
      tray.clients,
      Object.fromEntries(
        Object.entries(original.clients).map(([client, config]) => [
          client,
          config.enabled,
        ]),
      ),
    );
    assert.doesNotMatch(
      JSON.stringify(tray),
      /api_key|experimental_bearer_token|sk-preview|local_token|skill_locations/,
    );
    const beforeTray = await invoke("load_app_state");
    const updatedTray = await invoke("update_tray_client", {
      payload: { client: "claude", enabled: !tray.clients.claude },
    });
    assert.equal(updatedTray.clients.claude, !tray.clients.claude);
    assert.equal(updatedTray.clients.codex, tray.clients.codex);
    assert.equal(updatedTray.clients.pi, tray.clients.pi);
    const afterTray = await invoke("load_app_state");
    assert.deepEqual(afterTray.router, beforeTray.router);
    assert.deepEqual(afterTray.providers, beforeTray.providers);
    await invoke("update_tray_client", {
      payload: { client: "codex", enabled: true },
    });
    const model = tray.codex_models.at(-1);
    assert.ok(model);
    assert.equal(
      (await invoke("update_tray_client", { payload: { codex_model: model } }))
        .codex_model,
      model,
    );
    await assert.rejects(
      invoke("update_tray_client", {
        payload: { codex_model: "unconfigured-model" },
      }),
      /所选模型不可用/,
    );
    await assert.rejects(
      invoke("update_tray_client", {
        payload: { client: "pi", enabled: true, codex_model: model },
      }),
      /单项客户端变更/,
    );
    await invoke("update_tray_client", {
      payload: { client: "codex", enabled: false },
    });
    await assert.rejects(
      invoke("update_tray_client", { payload: { codex_model: model } }),
      /先开启 Codex/,
    );
    assert.equal((await invoke("get_tray_preferences")).visible, true);
    assert.equal(
      (await invoke("set_tray_visibility", { visible: false })).visible,
      false,
    );
    assert.equal((await invoke("load_tray_snapshot")).panel_visible, false);
    assert.equal((await invoke("get_tray_preferences")).visible, false);
    await invoke("set_tray_visibility", { visible: true });
    await invoke("tray_action", { action: "show" });
    assert.equal((await invoke("load_tray_snapshot")).panel_visible, true);
    await invoke("tray_action", { action: "settings" });
    assert.equal((await invoke("load_tray_snapshot")).panel_visible, false);
    assert.equal(await invoke("take_tray_navigation"), "settings");
    assert.equal(await invoke("take_tray_navigation"), null);
    await invoke("tray_action", { action: "quit" });
    assert.equal((await invoke("load_tray_snapshot")).panel_visible, false);
    assert.ok(
      (await invoke("load_app_state")).providers.length,
      "Preview quit never exits the test process",
    );
    await assert.rejects(invoke("install_update"), /预览尚未模拟/);
    await assert.rejects(invoke("plugin:opener|open_path"), /模拟预览不打开/);

    installPreview();
    assert.equal(
      (await invoke("load_app_state")).providers[0].status,
      "enabled",
      "Reloading restores synthetic fixtures",
    );
    window.location.search = "?scenario=empty";
    installPreview();
    assert.equal((await invoke("load_app_state")).providers.length, 0);
    assert.equal((await invoke("load_route_logs")).total, 0);
    assert.deepEqual((await invoke("load_tray_snapshot")).codex_models, []);
    await invoke("update_tray_client", {
      payload: { client: "codex", enabled: false },
    });
    await assert.rejects(
      invoke("update_tray_client", {
        payload: { client: "codex", enabled: true },
      }),
      /至少一个已配置的路由模型/,
    );
    assert.equal((await invoke("load_tray_snapshot")).clients.codex, false);
    window.location.search = "?scenario=chart-large";
    installPreview();
    const largeChart = await invoke("load_route_usage_stats", {
      payload: { filter: {} },
    });
    assert.equal(largeChart.bucket_granularity, "month");
    assert.deepEqual(
      largeChart.buckets.map((bucket) => bucket.total_tokens),
      [1_234_000_000, 24_500_000],
    );
    assert.equal(largeChart.summary.total_tokens, 1_258_500_000);
    assert.equal(largeChart.today.total_tokens, 24_500_000);
    window.location.search = "?scenario=long-names";
    installPreview();
    const longNames = await invoke("load_route_usage_stats", {
      payload: { filter: {} },
    });
    assert.equal(longNames.models.length, 4);
    const sameBase = longNames.models.filter((row) =>
      row.label.endsWith("/gpt-6.1-sol"),
    );
    assert.equal(
      sameBase.length,
      2,
      "distinct routes for one base model keep separate counts",
    );
    assert.notEqual(sameBase[0].key, sameBase[1].key);
    assert.equal(
      longNames.models.reduce((count, row) => count + row.request_count, 0),
      longNames.summary.request_count,
    );
    const exactRoute = await invoke("load_route_usage_stats", {
      payload: { filter: { model: sameBase[1].key } },
    });
    assert.equal(exactRoute.models.length, 1);
    assert.equal(exactRoute.models[0].key, sameBase[1].key);
    window.location.search = "?scenario=offline";
    installPreview();
    assert.equal((await invoke("load_app_state")).router_status.running, false);
    window.location.search = "?scenario=import";
    installPreview();
    const pending = await invoke("peek_provider_import");
    assert.equal(pending.enabled, false);
    const imported = await invoke("confirm_provider_import", {
      id: pending.id,
      enabled: false,
    });
    assert.equal(imported.providers.length, 5);
    assert.equal(imported.providers.at(-1).status, "disabled");
    assert.equal(await invoke("peek_provider_import"), null);
    await assert.rejects(
      invoke("confirm_provider_import", { id: pending.id, enabled: true }),
      /已处理/,
    );
    installPreview();
    await invoke("dismiss_provider_import", { id: pending.id });
    assert.equal(await invoke("peek_provider_import"), null);
    assert.equal(networkCalls, 0);
  } finally {
    clearMocks();
    if (previousWindow === undefined) delete globalThis.window;
    else globalThis.window = previousWindow;
    globalThis.fetch = previousFetch;
  }
});

test("Production assets exclude preview IPC, synthetic credentials, and fixtures", async () => {
  const directory = new URL("../dist/assets/", import.meta.url);
  const files = (await readdir(directory)).filter((file) =>
    file.endsWith(".js"),
  );
  assert.ok(files.length > 0, "Run the production build first");
  for (const file of files) {
    const code = await readFile(new URL(file, directory), "utf8");
    assert.doesNotMatch(
      code,
      /preview-request-|preview-chart-|chart-large|long-names|unscoped-|sk-preview-not-a-real-key|网页导入预览|模拟数据 · 独立预览|模拟 macOS 状态栏图标|模拟 Windows \/ Linux 托盘图标|function mockIPC/,
    );
  }
});

test("Usage model labels hide only managed route prefixes and retain provider context", async () => {
  const source = await readFile(
    new URL("../src/modelLabels.ts", import.meta.url),
    "utf8",
  );
  const compiled = await transformWithEsbuild(source, "modelLabels.ts", {
    loader: "ts",
    format: "esm",
  });
  const { requestModelLabel, fusionModelLabel, chartModelLabel } = await import(
    `data:text/javascript;base64,${Buffer.from(compiled.code).toString("base64")}`
  );
  const providers = [
    { id: "provider-a", name: "主线路" },
    { id: "provider-b", name: "备用线路" },
  ];
  const key = (id) => Buffer.from(id).toString("hex");
  const first = `xxswitch-${key(providers[0].id)}/gpt-6.1-sol`;
  const second = `xxswitch-${key(providers[1].id)}/gpt-6.1-sol`;
  assert.equal(chartModelLabel(first, providers), "gpt-6.1-sol（主线路）");
  assert.equal(chartModelLabel(second, providers), "gpt-6.1-sol（备用线路）");
  assert.notEqual(
    first,
    second,
    "display labels do not merge the original identities",
  );
  assert.equal(chartModelLabel(first, []), "gpt-6.1-sol");
  assert.equal(
    fusionModelLabel(first, []),
    "gpt-6.1-sol（供应商已移除）",
    "route picker retains its missing-provider diagnostic",
  );
  assert.equal(
    requestModelLabel(`xxswitch-${key("路由中文")}/vendor/model`),
    "vendor/model",
  );
  assert.equal(
    chartModelLabel(
      `xxswitch-claude-${key(providers[0].id)}--claude-sonnet`,
      providers,
    ),
    "claude-sonnet（主线路）",
  );
  for (const model of [
    "openai/gpt-6.1-sol",
    "gpt-6-astra",
    "xxswitch-custom/model",
    "xxswitch-/model",
    "xxswitch-31/",
    "",
  ]) {
    assert.equal(chartModelLabel(model, providers), model);
  }
});

test("Tray updates refresh clean router drafts and preserve unsaved edits", async () => {
  const source = await readFile(
    new URL("../src/trayState.ts", import.meta.url),
    "utf8",
  );
  const compiled = await transformWithEsbuild(source, "trayState.ts", {
    loader: "ts",
    format: "esm",
  });
  const { mergeTrayDraft } = await import(
    `data:text/javascript;base64,${Buffer.from(compiled.code).toString("base64")}`
  );
  const saved = { codex_model: "model-a", port: 8080, debug_mode: false };
  const next = { ...saved, codex_model: "model-b" };
  assert.deepEqual(mergeTrayDraft({ ...saved }, saved, next), next);
  const dirty = { ...saved, port: 9090 };
  assert.equal(mergeTrayDraft(dirty, saved, next), dirty);
  assert.deepEqual(dirty, {
    codex_model: "model-a",
    port: 9090,
    debug_mode: false,
  });
  assert.equal(mergeTrayDraft(dirty, undefined, next), dirty);
});

test("Trend charts keep large-value axes and sparse data within responsive bounds", async () => {
  const source = await readFile(
    new URL("../src/components/trendChart.ts", import.meta.url),
    "utf8",
  );
  const compiled = await transformWithEsbuild(source, "trendChart.ts", {
    loader: "ts",
    format: "esm",
  });
  const { buildTrendGeometry, formatTrendAxisValue, formatTrendXAxisLabel } =
    await import(
      `data:text/javascript;base64,${Buffer.from(compiled.code).toString("base64")}`
    );
  assert.equal(formatTrendAxisValue(2_000_000_000), "20亿");
  assert.equal(formatTrendAxisValue(500_000_000), "5亿");
  assert.equal(formatTrendAxisValue(0), "0");
  assert.equal(formatTrendXAxisLabel("2026-10", "month"), "2026-10");
  assert.equal(formatTrendXAxisLabel("2026-11-06", "day"), "11-06");
  for (const width of [200, 320, 640, 1100]) {
    for (const values of [
      [],
      [0],
      [1],
      [7],
      [1_234_000_000, 24_500_000],
      Array.from({ length: 31 }, (_, i) => i * 150_000),
    ]) {
      const chart = buildTrendGeometry(values, width, 220);
      assert.deepEqual(
        chart.points.map((point) => point.value),
        values,
      );
      assert.ok(chart.left < chart.right, `usable plot at ${width}px`);
      for (const point of chart.points) {
        assert.ok(point.x >= chart.left && point.x <= chart.right);
        assert.ok(point.y >= chart.top && point.y <= chart.bottom);
      }
      assert.ok(chart.ticks.every((tick) => tick.label.length <= 6));
      assert.ok(chart.xTickIndexes.length <= 6);
      if (values.length) {
        assert.equal(chart.xTickIndexes[0], 0);
        assert.equal(chart.xTickIndexes.at(-1), values.length - 1);
      }
    }
  }
  const single = buildTrendGeometry([10], 320, 176);
  assert.equal(single.points.length, 1);
  assert.equal(single.points[0].x, (single.left + single.right) / 2);
});
