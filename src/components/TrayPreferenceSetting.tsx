import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Toggle } from "./ui";

export function TrayPreferenceSetting() {
  const [visible, setVisible] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let cancelled = false;
    void invoke<{ visible: boolean }>("get_tray_preferences")
      .then((preferences) => {
        if (!cancelled) setVisible(preferences.visible);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);
  async function update(next: boolean) {
    setBusy(true);
    setError("");
    try {
      setVisible(
        (
          await invoke<{ visible: boolean }>("set_tray_visibility", {
            visible: next,
          })
        ).visible,
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="settings-row application-setting-row">
      <div className="settings-row-description">
        <span>显示状态栏图标</span>
        <small>
          显示菜单栏／系统托盘快捷入口；关闭后仍可重新打开 XXSwitch 进入主窗口。
        </small>
        {error && <small className="danger-text">{error}</small>}
      </div>
      <Toggle
        label="显示状态栏图标"
        checked={visible ?? true}
        disabled={busy || visible === null}
        onChange={(next) => void update(next)}
      />
    </div>
  );
}
