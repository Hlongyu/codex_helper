import React from "react";
import ReactDOM from "react-dom/client";
import { isTauri } from "@tauri-apps/api/core";
import { AppearanceProvider } from "./components/Appearance";
import "@radix-ui/themes/styles.css";

async function start() {
  // This branch is removed from production builds. A desktop runtime must never
  // be replaced with preview IPC, even when launched with the preview mode.
  if (import.meta.env.DEV && import.meta.env.MODE === "ui-preview") {
    if (isTauri() || "__TAURI_INTERNALS__" in window)
      throw new Error("模拟预览只能在独立浏览器中运行。");
    const { installPreview } = await import("./preview/mockBackend");
    installPreview();
  }
  // App detects the runtime at module initialization, after preview installation.
  const tray = new URLSearchParams(location.search).get("view") === "tray";
  document.documentElement.classList.toggle("tray-window", tray);
  const { default: App } = tray
    ? await import("./TrayPanel")
    : await import("./App");
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <AppearanceProvider>
        <App />
      </AppearanceProvider>
    </React.StrictMode>,
  );
}

void start();
