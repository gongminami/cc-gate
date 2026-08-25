export type AgentId = "codex_cli" | "codex_desktop" | "claude_cli" | "claude_desktop" | "hermes" | "opencode" | "openclaw" | "aider" | "cursor" | "reasonix";

export interface AgentMeta {
  id: AgentId; name: string; type: "cli" | "desktop"; tool: string; proxy: string;
  writes_zshrc: boolean; writes_providers: boolean; writes_catalog: boolean;
}

export interface RelayModelDef { id: string; display_name?: string; context_window?: number; }
export interface RelayConfig {
  name: string;
  url: string;
  anthropic_url?: string;
  key: string;
  models?: RelayModelDef[];
  /** false = disabled: discovered models hidden from every picker */
  enabled?: boolean | null;
}

export interface CustomAlias {
  name: string;   // shell identifier
  tool: string;   // "claude_cli" | "codex_cli" | "aider" | "pi"
  model: string;  // model slug
  source: string; // "direct" | "relay:<name>"
}

export interface AppConfig {
  version: number; models: ModelDef[];
  agent_models: Record<AgentId, string[]>;
  relays: RelayConfig[];
  /** model_slug → "direct" | "relay:<name>" */
  model_routing: Record<string, string>;
  /** env_var_name → api_key_value */
  api_keys: Record<string, string>;
  proxy_ports: ProxyPorts;
  model_catalog_version: number;
  custom_aliases?: CustomAlias[];
  autostart_mimo2codex: boolean; autostart_claude_proxy: boolean; autostart_chat_proxy: boolean;
  autostart_app: boolean;
}

export interface ModelDef {
  slug: string; display_name: string; provider: string; enabled: boolean;
  context_window: number; max_output_tokens: number; priority: number;
  default_reasoning_level: string; supports_reasoning_summaries: boolean;
  /** Codex 直连需原生 Responses API（别名页联动过滤用） */
  native_responses?: boolean;
  input_price_per_1k: number; output_price_per_1k: number;
}

export interface ProxyPorts { mimo2codex: number; claude_proxy: number; chat_proxy: number; }
export interface ApplyResult { success: boolean; message: string; restarted_proxies: string[]; }
export interface ProxyStatus { name: string; port: number; running: boolean; pid: number | null; script: string; }

// Usage
export interface UsageSummary { today_cost_usd: number; month_cost_usd: number; total_requests: number; today_tokens: number; models: ModelUsage[]; }
export interface ModelUsage { model: string; display_name: string; total_tokens: number; total_cost_usd: number; request_count: number; }
export interface DailyUsage { date: string; total_tokens: number; total_cost_usd: number; request_count: number; }
export interface LogEntry { id: number; request_id: string; model: string; provider: string; proxy: string; prompt_tokens: number; completion_tokens: number; total_tokens: number; cost_usd: number; created_at: string; }

export interface PerModelSlot { model: string; display_name: string; tokens: number; cost_usd: number; requests: number; }
export interface PerModelUsage { label: string; models: PerModelSlot[]; }

export interface ToolStatus { name: string; command: string; installed: boolean; version: string | null; install_cmd: string; link: string; category: string; }

export interface CheckUpdateResult { new_models: number; new_slugs: string[]; version: number; updated_at: string; }

export interface AppUpdateInfo {
  has_update: boolean;
  current_version: string;
  latest_version: string;
  release_url: string;
  notes: string;
}
