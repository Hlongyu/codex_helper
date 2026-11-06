import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  ArrowRightLeft,
  Blocks,
  Code2,
  ExternalLink,
  RefreshCw,
  Settings2,
  Terminal,
  X,
} from "lucide-react";
import {
  Badge,
  Button,
  Card,
  CopyButton,
  Select,
  SelectOption,
  Toggle,
} from "./components/ui";
import type { TrayAction, TrayClientPatch, TraySnapshot } from "./trayTypes";
import { AppearanceControl } from "./components/Appearance";
import "./App.css";
import "./styles/tray.css";

const preview = import.meta.env.DEV && import.meta.env.MODE === "ui-preview";
const compactNumber = new Intl.NumberFormat("zh-CN", {
  notation: "compact",
  maximumFractionDigits: 1,
});
const clients = [
  { id: "codex", name: "Codex", Icon: Code2 },
  { id: "claude", name: "Claude Code", Icon: Terminal },
  { id: "pi", name: "Pi", Icon: Blocks },
] as const;

export default function TrayPanel() {
  const [snapshot, setSnapshot] = useState<TraySnapshot | null>(null);
  const [open, setOpen] = useState(true);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const mutation = useRef(false);
  const sequence = useRef(0);

  const refresh = useCallback(async () => {
    if (mutation.current) return;
    const request = ++sequence.current;
    setLoading(true);
    try {
      const state = await invoke<TraySnapshot>("load_tray_snapshot");
      if (request !== sequence.current) return;
      setSnapshot(state);
      setOpen(state.panel_visible);
      setError("");
    } catch (err) {
      if (request === sequence.current)
        setError(`读取状态失败：${String(err)}`);
    } finally {
      if (request === sequence.current) setLoading(false);
    }
  }, []);

  const action = useCallback(
    async (next: TrayAction) => {
      try {
        await invoke("tray_action", { action: next });
        if (next !== "show") {
          sequence.current += 1;
          setLoading(false);
        }
        setOpen(next === "show");
        if (next === "show") await refresh();
      } catch (err) {
        setError(String(err));
      }
    },
    [refresh],
  );

  useEffect(() => {
    void refresh();
    if (preview) return;
    const subscription = listen<boolean>("tray-visibility", ({ payload }) => {
      if (!payload) {
        sequence.current += 1;
        setLoading(false);
      }
      setOpen(payload);
      if (payload) void refresh();
    });
    return () => {
      void subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [refresh]);

  useEffect(() => {
    if (!open) return;
    const interval = window.setInterval(() => void refresh(), 8_000);
    return () => window.clearInterval(interval);
  }, [open, refresh]);

  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const menuOpen = document.querySelector(
        '[role="listbox"][data-state="open"], [role="dialog"][data-state="open"]',
      );
      if (menuOpen) return;
      event.preventDefault();
      void action("hide");
    };
    // Read open menu state before Radix dismisses its layer. Tooltip dismissal
    // must not consume the panel's Escape shortcut.
    document.addEventListener("keydown", escape, true);
    return () => document.removeEventListener("keydown", escape, true);
  }, [action]);

  async function update(patch: TrayClientPatch) {
    if (mutation.current) return;
    mutation.current = true;
    sequence.current += 1;
    setBusy(true);
    setError("");
    try {
      setSnapshot(
        await invoke<TraySnapshot>("update_tray_client", { payload: patch }),
      );
    } catch (err) {
      setError(String(err));
      // Applying a client configuration can fail after persistence. Read the
      // current state again so the switches still describe the saved settings.
      try {
        setSnapshot(await invoke<TraySnapshot>("load_tray_snapshot"));
      } catch {
        // Keep the last snapshot and the actionable mutation error.
      }
    } finally {
      mutation.current = false;
      setBusy(false);
    }
  }

  const disabled = busy || Boolean(error) || !snapshot;
  const running = snapshot?.router_status.running ?? false;
  const panel = (
    <section
      className="tray-panel"
      aria-label="XXSwitch 快捷面板"
      aria-busy={loading || busy}
    >
      <header className="tray-panel-header">
        <span className="tray-brand-mark">
          <ArrowRightLeft size={18} aria-hidden="true" />
        </span>
        <div className="tray-brand">
          <strong>XXSwitch</strong>
          <small>{snapshot?.app_version ?? "本地 AI 工作区"}</small>
        </div>
        <div className="tray-header-actions">
          <Button
            className="icon-button"
            title="刷新状态"
            aria-label="刷新状态"
            disabled={loading || busy}
            onClick={() => void refresh()}
          >
            <RefreshCw size={15} />
          </Button>
          <Button
            className="icon-button"
            title="设置"
            aria-label="设置"
            onClick={() => void action("settings")}
          >
            <Settings2 size={16} />
          </Button>
          <Button
            className="icon-button"
            title="收起面板"
            aria-label="收起面板"
            onClick={() => void action("hide")}
          >
            <X size={16} />
          </Button>
        </div>
      </header>
      <div className="tray-panel-content">
        {error && (
          <div className="tray-error" role="alert">
            {error}
            <Button className="text-button" onClick={() => void refresh()}>
              重试
            </Button>
          </div>
        )}
        <Card className="tray-gateway">
          <div className="tray-gateway-heading">
            <strong>本地网关</strong>
            <Badge className={`state-pill ${running ? "ok" : "warn"}`}>
              <span />
              {snapshot
                ? running
                  ? "运行中"
                  : snapshot.router_status.error
                    ? "异常"
                    : "未运行"
                : "正在读取"}
            </Badge>
          </div>
          <div className="tray-gateway-address">
            <code>{snapshot?.router_status.address || "—"}</code>
            {snapshot?.router_status.address && (
              <CopyButton
                text={snapshot.router_status.address}
                label="复制地址"
              />
            )}
          </div>
          {snapshot?.router_status.error && (
            <small className="danger-text">
              {snapshot.router_status.error}
            </small>
          )}
        </Card>
        <div className="tray-section-heading">
          <span>客户端接管</span>
          <Button className="text-button" onClick={() => void action("route")}>
            路由设置 <ExternalLink size={12} />
          </Button>
        </div>
        <Card className="tray-clients">
          {clients.map(({ id, name, Icon }) => {
            const enabled = snapshot?.clients[id] ?? false;
            const count =
              id === "claude"
                ? snapshot?.claude_provider_count
                : snapshot?.codex_provider_count;
            return (
              <div className="tray-client" key={id}>
                <span
                  className={`tray-client-icon ${enabled ? "enabled" : ""}`}
                >
                  <Icon size={17} aria-hidden="true" />
                </span>
                <div className="tray-client-name">
                  <strong>{name}</strong>
                  <small>
                    {enabled
                      ? `已接管 · ${count ?? 0} 个已启用供应商`
                      : "使用客户端自身配置"}
                  </small>
                </div>
                <Toggle
                  label={`接管 ${name}`}
                  checked={enabled}
                  disabled={disabled}
                  onChange={(checked) =>
                    void update({ client: id, enabled: checked })
                  }
                />
                {id === "codex" && (
                  <Select
                    className="tray-model-select"
                    aria-label="Codex 默认模型"
                    value={
                      snapshot?.codex_models.length ? snapshot.codex_model : ""
                    }
                    disabled={
                      disabled || !enabled || !snapshot?.codex_models.length
                    }
                    onValueChange={(model) => {
                      if (model !== snapshot?.codex_model)
                        void update({ codex_model: model });
                    }}
                  >
                    {!snapshot?.codex_models.length && (
                      <SelectOption value="">暂无可用模型</SelectOption>
                    )}
                    {Boolean(snapshot?.codex_models.length) &&
                      !snapshot?.codex_models.includes(
                        snapshot.codex_model,
                      ) && (
                        <SelectOption
                          value={snapshot?.codex_model ?? ""}
                          disabled
                        >
                          {snapshot?.codex_model
                            ? `${snapshot.codex_model}（暂不可用）`
                            : "请选择默认模型"}
                        </SelectOption>
                      )}
                    {snapshot?.codex_models.map((model) => (
                      <SelectOption value={model} key={model}>
                        {model}
                      </SelectOption>
                    ))}
                  </Select>
                )}
              </div>
            );
          })}
        </Card>
        <div className="tray-section-heading">
          <span>今日用量</span>
          <Button className="text-button" onClick={() => void action("usage")}>
            查看统计 <ExternalLink size={12} />
          </Button>
        </div>
        <Card className="tray-usage">
          {snapshot?.today ? (
            <>
              <div>
                <small>成功调用</small>
                <strong>
                  {compactNumber.format(snapshot.today.request_count)}
                </strong>
              </div>
              <div>
                <small>总 Token</small>
                <strong>
                  {compactNumber.format(snapshot.today.total_tokens)}
                </strong>
              </div>
              <div>
                <small>输出 Token</small>
                <strong>
                  {compactNumber.format(snapshot.today.output_tokens)}
                </strong>
              </div>
            </>
          ) : (
            <span className="tray-usage-empty">
              {snapshot?.usage_error
                ? "用量暂时无法读取，请刷新重试。"
                : loading
                  ? "正在读取今日用量…"
                  : "暂无用量数据"}
            </span>
          )}
        </Card>
      </div>
      <footer className="tray-panel-footer">
        <Button className="primary" onClick={() => void action("dashboard")}>
          打开主窗口 <ExternalLink size={13} />
        </Button>
        <Button className="text-button" onClick={() => void action("quit")}>
          退出
        </Button>
      </footer>
    </section>
  );

  if (preview)
    return (
      <main className="tray-preview-page">
        <div className="tray-preview-bar">
          <Badge className="preview-badge">模拟数据 · 独立预览</Badge>
          <AppearanceControl />
          <Button
            className="ghost"
            aria-label="模拟 macOS 状态栏图标"
            onClick={() => void action(open ? "hide" : "show")}
          >
            <img
              className="tray-template-preview"
              src={
                new URL(
                  "../src-tauri/icons/tray/template@2x.png",
                  import.meta.url,
                ).href
              }
              width="18"
              height="18"
              alt=""
            />
          </Button>
          <Button
            className="ghost"
            aria-label="模拟 Windows / Linux 托盘图标"
            onClick={() => void action(open ? "hide" : "show")}
          >
            <img
              src={
                new URL("../src-tauri/icons/tray/color@2x.png", import.meta.url)
                  .href
              }
              width="20"
              height="20"
              alt=""
            />
          </Button>
        </div>
        {open ? (
          <div className="tray-preview-window">{panel}</div>
        ) : (
          <p className="tray-preview-hint">点击托盘图标打开快捷面板</p>
        )}
      </main>
    );
  return <main className="tray-panel-page">{panel}</main>;
}
