import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

type SyncView = {
  url: string;
  username: string;
  directory: string;
  password_set: boolean;
  auto_upload: boolean;
  last_sync_at: string | null;
  message: string;
  error: boolean;
};
type Draft = { url: string; username: string; directory: string; password: string; auto_upload: boolean };
type Preview = {
  token: string;
  remote_exists: boolean;
  local_codex: number;
  local_claude: number;
  remote_codex: number;
  remote_claude: number;
  remote_created_at: string | null;
  same: boolean;
};

export function ProviderSync({ onDownloaded }: { onDownloaded: () => Promise<void> }) {
  const [saved, setSaved] = useState<SyncView | null>(null);
  const [draft, setDraft] = useState<Draft>({ url: "", username: "", password: "", directory: "xxswitch-sync", auto_upload: false });
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [direction, setDirection] = useState<"upload" | "download">("upload");
  const inFlight = useRef(false);
  const desktop = isTauri();

  useEffect(() => {
    const token = preview?.token;
    return () => { if (token) void invoke("cancel_provider_sync_preview", { token }).catch(() => {}); };
  }, [preview?.token]);

  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    let initialized = false;
    const refresh = async () => {
      if (inFlight.current) return;
      try {
        const view = await invoke<SyncView>("get_provider_sync");
        if (disposed || inFlight.current) return;
        setSaved(view);
        if (!initialized) {
          initialized = true;
          setDraft({ url: view.url, username: view.username, directory: view.directory, auto_upload: view.auto_upload, password: "" });
        }
      } catch (e) { if (!disposed) setError(String(e)); }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, [desktop]);

  const dirty = !saved || draft.url !== saved.url || draft.username !== saved.username || draft.directory !== saved.directory || draft.auto_upload !== saved.auto_upload || Boolean(draft.password);
  const configured = Boolean(saved?.url) && !dirty;
  function edit(patch: Partial<Draft>) { setDraft((current) => ({ ...current, ...patch })); setPreview(null); setMessage(""); }
  async function run(action: () => Promise<void>) {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true); setError(""); setMessage("");
    try { await action(); } catch (e) { setError(String(e)); }
    finally { inFlight.current = false; setBusy(false); }
  }
  async function prepare(next: "upload" | "download") {
    await run(async () => {
      setPreview(null);
      const nextPreview = await invoke<Preview>("preview_provider_sync");
      if (next === "download" && !nextPreview.remote_exists) {
        await invoke("cancel_provider_sync_preview", { token: nextPreview.token });
        setMessage("远端还没有供应商配置，请先从有配置的设备上传。"); return;
      }
      setDirection(next); setPreview(nextPreview);
    });
  }
  async function confirm() {
    if (!preview) return;
    await run(async () => {
      const view = await invoke<SyncView>(direction === "upload" ? "upload_provider_sync" : "download_provider_sync", { token: preview.token });
      setSaved(view); setPreview(null); setMessage(view.message);
      if (direction === "download") await onDownloaded();
    });
  }

  return <article className="page-panel provider-sync-panel">
    <div className="panel-head"><div>
      <h2>供应商云同步</h2>
      <p>通过 WebDAV 在设备间共享 Codex 和 Claude 供应商配置。</p>
    </div></div>
    <p className="provider-sync-note">同步名称、地址、API Key、排序、模型和供应商选项。路由设置、Skills、日志和运行状态保留在本机。</p>
    {!desktop && <p role="status">云同步需要在 XXSwitch 桌面应用中使用。</p>}
    <fieldset disabled={busy || !desktop || !saved} className="provider-sync-fields">
      <div className="route-form-grid">
        <label className="compact-field provider-sync-url"><span>WebDAV 服务地址</span>
          <input type="url" placeholder="https://dav.example.com/dav/" autoComplete="off" spellCheck={false} value={draft.url} onChange={(e) => edit({ url: e.target.value })} />
        </label>
        <label className="compact-field"><span>账号</span>
          <input autoComplete="off" value={draft.username} onChange={(e) => edit({ username: e.target.value })} />
        </label>
        <label className="compact-field"><span>密码 / 应用专用密码</span>
          <input type="password" autoComplete="new-password" placeholder={saved?.password_set ? "已保存，留空保持原密码" : "输入 WebDAV 密码"} value={draft.password} onChange={(e) => edit({ password: e.target.value })} />
        </label>
        <label className="compact-field provider-sync-url"><span>远程目录</span>
          <input autoComplete="off" spellCheck={false} value={draft.directory} onChange={(e) => edit({ directory: e.target.value })} />
          <small>所有设备填写相同地址和目录；例如坚果云地址为 https://dav.jianguoyun.com/dav/。</small>
        </label>
      </div>
      <label className="provider-sync-auto">
        <input type="checkbox" checked={draft.auto_upload} onChange={(e) => edit({ auto_upload: e.target.checked })} />
        <span>自动上传供应商变更<small>开启后约 15 秒内检查并上传。远端有不同修改时暂停，其他设备需手动下载。</small></span>
      </label>
      <p className="provider-sync-note">快照包含完整 API Key，未额外加密。请使用可信的私有存储和 HTTPS；WebDAV 密码仅保存在本机。</p>
      <div className="provider-sync-actions">
        <button className="primary" type="button" disabled={!dirty || !draft.url.trim() || !draft.directory.trim()} onClick={() => void run(async () => {
          const view = await invoke<SyncView>("save_provider_sync", { draft: { ...draft, password: draft.password || null } });
          setSaved(view); setDraft({ url: view.url, username: view.username, directory: view.directory, auto_upload: view.auto_upload, password: "" }); setPreview(null); setMessage("设置已保存");
        })}>保存设置</button>
        <button className="ghost" type="button" disabled={!configured} onClick={() => void run(async () => setMessage(await invoke<string>("test_provider_sync")))}>测试连接</button>
        <button className="ghost" type="button" disabled={!configured} onClick={() => void prepare("upload")}>上传本机供应商</button>
        <button className="ghost" type="button" disabled={!configured} onClick={() => void prepare("download")}>下载远端供应商</button>
      </div>
      {dirty && saved?.url && <p className="provider-sync-note">保存设置后再测试或同步。</p>}
    </fieldset>
    {preview && <div className="provider-sync-preview" role="region" aria-label="供应商同步确认">
      <strong>{direction === "upload" ? "确认上传供应商" : "确认下载供应商"}</strong>
      <p>本机：Codex {preview.local_codex} 个，Claude {preview.local_claude} 个。</p>
      <p>{preview.remote_exists ? `远端：Codex ${preview.remote_codex} 个，Claude ${preview.remote_claude} 个。` : "远端尚无快照，将创建供应商快照。"}</p>
      {preview.remote_created_at && <p>远端快照时间：{new Date(preview.remote_created_at).toLocaleString()}</p>}
      <p>{preview.same ? "两端供应商配置一致。" : direction === "upload" ? "将用本机列表完整替换远端供应商列表，包含 API Key。" : "将用远端列表完整替换本机供应商列表；覆盖前自动备份本机供应商。"}</p>
      <div className="provider-sync-actions">
        <button className="ghost" type="button" disabled={busy} onClick={() => setPreview(null)}>取消</button>
        <button className="primary" type="button" disabled={busy} onClick={() => void confirm()}>{busy ? "同步中…" : direction === "upload" ? "确认上传" : "确认下载"}</button>
      </div>
    </div>}
    {busy && <p role="status">正在处理…</p>}
    {error && <p className="provider-sync-error" role="alert">{error}</p>}
    {message && <p className="provider-sync-message" role="status">{message}</p>}
    {saved?.last_sync_at && <p className="provider-sync-note">最近同步：{new Date(saved.last_sync_at).toLocaleString()}</p>}
    {saved?.message && saved.message !== message && <p className={saved.error ? "provider-sync-error" : "provider-sync-note"} role={saved.error ? "alert" : "status"}>{saved.message}</p>}
  </article>;
}
