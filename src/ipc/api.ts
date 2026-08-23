import { invoke } from "@tauri-apps/api/core";
import type { AgentMeta, RelayConfig, AppConfig, ApplyResult, ProxyStatus, CheckUpdateResult } from "../types/models";
import type { UsageSummary, DailyUsage, LogEntry, PerModelUsage, ToolStatus } from "../types/models";
export type { UsageSummary, DailyUsage, LogEntry, PerModelUsage, ToolStatus };

export async function getConfig(): Promise<AppConfig> { return invoke<AppConfig>("get_config"); }
export async function saveConfig(cfg: AppConfig): Promise<void> { return invoke<void>("save_config", { cfg }); }
export async function getAgentList(): Promise<AgentMeta[]> { return invoke<AgentMeta[]>("get_agent_list"); }
export async function applyAgentConfig(cfg: AppConfig): Promise<ApplyResult> { return invoke<ApplyResult>("apply_agent_config", { cfg }); }

// Backup / restore
export interface AgentStatus { agent_id: string; proxied: boolean; }
export interface RestoreResult { agent_id: string; restored: boolean; }
export async function checkAgentStatus(): Promise<AgentStatus[]> { return invoke<AgentStatus[]>("check_agent_status"); }
export async function restoreAgent(agentId: string): Promise<RestoreResult> { return invoke<RestoreResult>("restore_agent", { agentId }); }

export interface ShellInfo { config_file: string; reload_cmd: string; platform_os: string; }
export async function getShellInfo(): Promise<ShellInfo> { return invoke<ShellInfo>("get_shell_info"); }

export async function writeToolConfigs(cfg: AppConfig): Promise<string> { return invoke<string>("write_tool_configs", { cfg }); }

// Relay CRUD
export async function addRelay(cfg: AppConfig, name: string, url: string, key: string, anthropicUrl?: string): Promise<AppConfig> { return invoke<AppConfig>("add_relay", { cfg, name, url, key, anthropicUrl }); }
export async function updateRelay(cfg: AppConfig, oldName: string, name: string, url: string, key: string, anthropicUrl?: string): Promise<AppConfig> { return invoke<AppConfig>("update_relay", { cfg, oldName, name, url, key, anthropicUrl }); }
export async function deleteRelay(cfg: AppConfig, name: string): Promise<AppConfig> { return invoke<AppConfig>("delete_relay", { cfg, name }); }

// Custom alias CRUD (别名页)
export async function addAlias(cfg: AppConfig, name: string, tool: string, model: string, source: string): Promise<AppConfig> { return invoke<AppConfig>("add_alias", { cfg, name, tool, model, source }); }
export async function updateAlias(cfg: AppConfig, oldName: string, name: string, tool: string, model: string, source: string): Promise<AppConfig> { return invoke<AppConfig>("update_alias", { cfg, oldName, name, tool, model, source }); }
export async function deleteAlias(cfg: AppConfig, name: string): Promise<AppConfig> { return invoke<AppConfig>("delete_alias", { cfg, name }); }

// Custom model CRUD (模型管理页)
import type { ModelDef } from "../types/models";
export async function addCustomModel(cfg: AppConfig, model: ModelDef): Promise<AppConfig> { return invoke<AppConfig>("add_custom_model", { cfg, model }); }
export async function updateCustomModel(cfg: AppConfig, oldSlug: string, model: ModelDef): Promise<AppConfig> { return invoke<AppConfig>("update_custom_model", { cfg, oldSlug, model }); }
export async function deleteCustomModel(cfg: AppConfig, slug: string): Promise<AppConfig> { return invoke<AppConfig>("delete_custom_model", { cfg, slug }); }
export async function knownProviders(): Promise<string[]> { return invoke<string[]>("known_providers"); }

// Relay presets (快速填入, cloud-managed)
export interface RelayPreset { name: string; url: string; anthropic_url?: string; }
export async function getRelayPresets(): Promise<RelayPreset[]> { return invoke<RelayPreset[]>("get_relay_presets"); }
export async function refreshRelayPresets(): Promise<RelayPreset[]> { return invoke<RelayPreset[]>("refresh_relay_presets"); }

// Proxy
export async function getProxyStatus(): Promise<ProxyStatus[]> { return invoke<ProxyStatus[]>("get_proxy_status"); }
export async function startProxy(name: string): Promise<ProxyStatus> { return invoke<ProxyStatus>("start_proxy", { name }); }
export async function stopProxy(name: string): Promise<ProxyStatus> { return invoke<ProxyStatus>("stop_proxy", { name }); }
export async function restartProxy(name: string): Promise<ProxyStatus> { return invoke<ProxyStatus>("restart_proxy", { name }); }

export async function getAppAutostartStatus(): Promise<{ enabled: boolean }> { return invoke<{ enabled: boolean }>("get_app_autostart_status"); }
export async function setAppAutostart(enabled: boolean): Promise<{ enabled: boolean }> { return invoke<{ enabled: boolean }>("set_app_autostart", { enabled }); }
export async function quitApp(): Promise<void> { return invoke<void>("quit_app"); }
export async function hideMainWindow(): Promise<void> { return invoke<void>("hide_main_window"); }

// Usage
export async function getUsageSummary(): Promise<UsageSummary> { return invoke<UsageSummary>("get_usage_summary"); }
export async function getDailyUsage(days: number): Promise<DailyUsage[]> { return invoke<DailyUsage[]>("get_daily_usage", { days }); }
export async function getRecentLogs(limit: number): Promise<LogEntry[]> { return invoke<LogEntry[]>("get_recent_logs", { limit }); }
export async function getPerModelUsage(): Promise<PerModelUsage[]> { return invoke<PerModelUsage[]>("get_per_model_usage"); }
export async function importUsageData(): Promise<number> { return invoke<number>("import_usage_data"); }
export async function getAppLogTail(lines: number): Promise<string> { return invoke<string>("get_app_log_tail", { lines }); }
export async function getAppVersion(): Promise<string> { return invoke<string>("get_app_version"); }
export async function copyToClipboard(text: string): Promise<void> { return invoke<void>("copy_to_clipboard", { text }); }

// Tool check
export async function checkTools(force?: boolean): Promise<ToolStatus[]> { return invoke<ToolStatus[]>("check_tools", { force: force ?? false }); }
export async function checkOneTool(name: string): Promise<ToolStatus | null> { return invoke<ToolStatus | null>("check_one_tool", { name }); }
export async function saveToolCache(results: ToolStatus[]): Promise<void> { return invoke<void>("save_tool_cache", { results }); }

// Model catalog
export async function checkModelUpdates(): Promise<CheckUpdateResult> { return invoke<CheckUpdateResult>("check_model_updates"); }
