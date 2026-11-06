import type { AgentClientKind, RouterStatus, UsageSummary } from "./types";

export type TraySnapshot = {
  app_version: string;
  router_status: RouterStatus;
  clients: Record<AgentClientKind, boolean>;
  codex_model: string;
  codex_models: string[];
  codex_provider_count: number;
  claude_provider_count: number;
  today: UsageSummary | null;
  usage_error: string | null;
  panel_visible: boolean;
};

export type TrayClientPatch =
  | { client: AgentClientKind; enabled: boolean }
  | { codex_model: string };

export type TrayAction =
  | "show"
  | "hide"
  | "dashboard"
  | "route"
  | "providers"
  | "usage"
  | "settings"
  | "quit";
