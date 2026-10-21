import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type ImportPreview = {
  id: number;
  app: "codex" | "claude";
  name: string;
  endpoint: string;
  api_key_masked: string;
  api_format: string;
  models: string[];
  enabled: boolean;
  error: string | null;
};

export function ProviderImport({ onImported }: { onImported: () => Promise<void> }) {
  const [pending, setPending] = useState<ImportPreview | null>(null);
  const activeId = useRef<number | null>(null);
  const readVersion = useRef(0);
  const refreshPending = useRef<() => Promise<void>>(async () => {});

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const refresh = async () => {
      const version = ++readVersion.current;
      const next = await invoke<ImportPreview | null>("peek_provider_import");
      if (!disposed && version === readVersion.current && activeId.current === null && next) {
        activeId.current = next.id;
        setPending(next);
      }
    };
    refreshPending.current = refresh;
    // Subscribe first, then read: neither a cold-start link nor an event during mounting is lost.
    void listen("provider-import-pending", () => {
      void refresh().catch(() => {});
    }).then(async (stop) => {
      if (disposed) { stop(); return; }
      unlisten = stop;
      await refresh();
    }).catch(() => {
      // The inbox remains in Rust; retry reading even if event subscription failed.
      if (!disposed) void refresh().catch(() => {});
    });
    return () => { disposed = true; unlisten?.(); };
  }, []);

  if (!pending) return null;
  return <ImportDialog key={pending.id} pending={pending} onImported={onImported} onClose={async (consumed) => {
    if (!consumed) await invoke("dismiss_provider_import", { id: pending.id });
    readVersion.current += 1;
    activeId.current = null;
    setPending(null);
    await refreshPending.current();
  }} />;
}

function ImportDialog({ pending, onImported, onClose }: {
  pending: ImportPreview;
  onImported: () => Promise<void>;
  onClose: (consumed: boolean) => Promise<void>;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const inFlight = useRef(false);
  const [enabled, setEnabled] = useState(pending.enabled);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);
  const [error, setError] = useState("");
  const titleId = `provider-import-title-${pending.id}`;

  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  async function close() {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    try { await onClose(done); }
    catch { setError("无法关闭导入请求，请重试。"); }
    finally { inFlight.current = false; setBusy(false); }
  }

  async function confirm() {
    if (inFlight.current || done) return;
    inFlight.current = true;
    setBusy(true);
    setError("");
    try {
      await invoke("confirm_provider_import", { id: pending.id, enabled });
      setDone(true);
      try { await onImported(); }
      catch { setError("供应商已保存，但列表刷新失败，请关闭后重新进入供应商页面。"); }
    } catch (err) {
      setError(String(err));
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }

  return (
    <dialog ref={dialog} className="latency-dialog provider-import-dialog" aria-labelledby={titleId}
      onCancel={(event) => { event.preventDefault(); void close(); }}>
      <header>
        <div>
          <h2 id={titleId}>{done ? "供应商已导入" : pending.error ? "无法导入供应商" : "导入供应商"}</h2>
          <p>{done ? "可在供应商列表中继续编辑配置。" : "请核对从网页传入的供应商配置。"}</p>
        </div>
      </header>
      <div className="latency-dialog-body">
        {pending.error ? <p className="provider-import-error" role="alert">{pending.error}</p> : <>
          <dl className="provider-import-details">
            <dt>供应商</dt><dd>{pending.name}</dd>
            <dt>供应商类型</dt><dd>{pending.app === "claude" ? "Claude" : "Codex"}</dd>
            <dt>接口地址</dt><dd>{pending.endpoint}</dd>
            <dt>API Key</dt><dd><code>{pending.api_key_masked}</code></dd>
            <dt>接口协议</dt><dd>{{ openai_responses: "OpenAI Responses", openai_chat: "OpenAI Chat Completions", anthropic: "Anthropic Messages" }[pending.api_format] ?? pending.api_format}</dd>
            <dt>启用模型</dt><dd>{pending.models.length ? pending.models.join("、") : "不限制模型"}</dd>
          </dl>
          {!done && <>
            {pending.endpoint.startsWith("http:") && <p className="provider-import-warning">此地址使用 HTTP，发送请求时 API Key 将以明文传输。</p>}
            <label className="provider-import-enable">
              <input type="checkbox" checked={enabled} disabled={busy} onChange={(event) => setEnabled(event.target.checked)} />
              导入后启用供应商，允许参与路由
            </label>
            <p className="provider-import-hint">将新增一条供应商记录；已有供应商不会被覆盖。未启用的供应商可以稍后在列表中启用。</p>
          </>}
          {done && <p role="status">已保存为{enabled ? "启用" : "停用"}状态。</p>}
        </>}
        {error && <p className="provider-import-error" role="alert">{error}</p>}
      </div>
      <footer>
        <button autoFocus className="ghost" disabled={busy} onClick={() => void close()} type="button">{done || pending.error ? "关闭" : "取消"}</button>
        {!pending.error && !done && <button className="primary" disabled={busy} onClick={() => void confirm()} type="button">{busy ? "正在导入…" : "确认导入"}</button>}
      </footer>
    </dialog>
  );
}
