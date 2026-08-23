use tauri::State;
use tauri::{Emitter, Manager};
use std::sync::Arc;

use crate::config_store;
use crate::config_writer;
use crate::error::{AppError, Result};
use crate::launchd;
use crate::model_catalog::{self, CheckUpdateResult};
use crate::proxy_manager::ProxyManager;
use crate::types::{AppConfig, AgentMeta, RelayConfig, ProxyStatus, agent_list};

#[tauri::command] pub fn get_config() -> Result<AppConfig> { config_store::load() }
#[tauri::command] pub fn save_config(cfg: AppConfig) -> Result<()> { config_store::save(&cfg) }
#[tauri::command] pub fn get_agent_list() -> Vec<AgentMeta> { agent_list() }

#[derive(serde::Serialize)]
pub struct ShellInfo {
    pub config_file: String,
    pub reload_cmd: String,
    pub platform_os: String,
}
#[tauri::command] pub fn get_shell_info() -> ShellInfo {
    ShellInfo {
        config_file: crate::paths::shell_description().to_string(),
        reload_cmd: crate::paths::shell_reload_cmd().to_string(),
        platform_os: std::env::consts::OS.to_string(),
    }
}

#[tauri::command]
pub async fn apply_agent_config(
    proxy_mgr: State<'_, Arc<ProxyManager>>,
    cfg: AppConfig,
) -> Result<ApplyResult> {
    config_store::save(&cfg)?;
    config_writer::write_all_tool_configs(&cfg)?;

    let mut restarted: Vec<String> = vec![];
    for name in &["mimo2codex", "claude-proxy", "chat-proxy"] {
        let (port, script) = proxy_mgr.proxy_script_for(name);
        if let Ok(s) = proxy_mgr.restart(name, port, &script).await {
            if s.running { restarted.push(name.to_string()); }
        }
    }

    Ok(ApplyResult { success: true, message: "配置已应用".into(), restarted_proxies: restarted })
}

#[derive(serde::Serialize, Clone)]
pub struct ApplyResult { pub success: bool, pub message: String, pub restarted_proxies: Vec<String> }

// ── Relay CRUD ─────────────────────────────────────────────

#[tauri::command]
pub fn add_relay(mut cfg: AppConfig, name: String, url: String, key: String, anthropic_url: Option<String>) -> Result<AppConfig> {
    if cfg.relays.iter().any(|r| r.name == name) {
        return Err(AppError::Config(format!("中转站 '{}' 已存在", name)));
    }
    cfg.relays.push(RelayConfig { name, url, anthropic_url, key });
    config_store::save(&cfg)?;
    Ok(cfg)
}

#[tauri::command]
pub fn update_relay(mut cfg: AppConfig, old_name: String, name: String, url: String, key: String, anthropic_url: Option<String>) -> Result<AppConfig> {
    if let Some(r) = cfg.relays.iter_mut().find(|r| r.name == old_name) {
        r.name = name; r.url = url; r.anthropic_url = anthropic_url; r.key = key;
    }
    config_store::save(&cfg)?;
    Ok(cfg)
}

#[tauri::command]
pub fn delete_relay(mut cfg: AppConfig, name: String) -> Result<AppConfig> {
    cfg.relays.retain(|r| r.name != name);
    // Also clean up model_routing entries pointing to this relay
    let target = format!("relay:{}", name);
    for (_, routing) in cfg.model_routing.iter_mut() {
        if *routing == target { *routing = "direct".into(); }
    }
    config_store::save(&cfg)?;
    Ok(cfg)
}

// ── Custom alias CRUD (别名页) ──────────────────────────────

fn persist_aliases(cfg: &AppConfig) -> Result<()> {
    config_writer::write_shell_aliases(cfg)?;
    config_writer::write_alias_routes(cfg)?;
    config_writer::write_pi_models(cfg)?;
    config_writer::write_opencode_alias_configs(cfg)?;
    Ok(())
}

fn validate_alias_combo(cfg: &AppConfig, tool: &str, model: &str, source: &str) -> Result<()> {
    const TOOLS: &[&str] = &["claude_cli", "codex_cli", "aider", "pi", "opencode", "hermes"];
    if !TOOLS.contains(&tool) {
        return Err(AppError::Config(format!("不支持的工具类型: {tool}")));
    }
    if !cfg.models.iter().any(|m| m.slug == model && m.enabled) {
        return Err(AppError::Config(format!("未知或未启用的模型: {model}")));
    }
    if source == "direct" {
        return Ok(());
    }
    let Some(relay_name) = source.strip_prefix("relay:") else {
        return Err(AppError::Config(format!("无效来源: {source}")));
    };
    if !cfg.relays.iter().any(|r| r.name == relay_name) {
        return Err(AppError::Config(format!("中转站不存在: {relay_name}")));
    }
    Ok(())
}

#[tauri::command]
pub fn add_alias(mut cfg: AppConfig, name: String, tool: String, model: String, source: String) -> Result<AppConfig> {
    let name = name.trim().to_string();
    config_writer::validate_alias_name(&name, &cfg, None).map_err(AppError::Config)?;
    validate_alias_combo(&cfg, &tool, &model, &source)?;
    let alias = crate::types::CustomAlias { name: name.clone(), tool: tool.clone(), model: model.clone(), source: source.clone() };
    cfg.custom_aliases.push(alias);
    config_store::save(&cfg)?;
    persist_aliases(&cfg)?;
    Ok(cfg)
}

#[tauri::command]
pub fn update_alias(mut cfg: AppConfig, old_name: String, name: String, tool: String, model: String, source: String) -> Result<AppConfig> {
    let name = name.trim().to_string();
    config_writer::validate_alias_name(&name, &cfg, Some(old_name.as_str())).map_err(AppError::Config)?;
    validate_alias_combo(&cfg, &tool, &model, &source)?;
    let Some(a) = cfg.custom_aliases.iter_mut().find(|a| a.name == old_name) else {
        return Err(AppError::Config(format!("别名不存在: {old_name}")));
    };
    a.name = name.clone(); a.tool = tool; a.model = model; a.source = source;
    config_store::save(&cfg)?;
    persist_aliases(&cfg)?;
    Ok(cfg)
}

#[tauri::command]
pub fn delete_alias(mut cfg: AppConfig, name: String) -> Result<AppConfig> {
    cfg.custom_aliases.retain(|a| a.name != name);
    config_store::save(&cfg)?;
    persist_aliases(&cfg)?;
    Ok(cfg)
}

// ── Custom model CRUD (模型管理页) ──────────────────────────

fn validate_custom_model(cfg: &AppConfig, model: &crate::types::ModelDef, ignore_self: Option<&str>) -> Result<()> {
    use crate::config_writer::known_provider_ids;
    let slug = model.slug.trim();
    if slug.is_empty() || slug.len() > 64 {
        return Err(AppError::Config("slug 长度需在 1~64 之间".into()));
    }
    if !slug.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false)
        || !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.' || c == '_') {
        return Err(AppError::Config("slug 只能小写字母开头，含小写字母/数字/-/./_".into()));
    }
    if cfg.models.iter().any(|m| m.slug == slug && Some(m.slug.as_str()) != ignore_self) {
        return Err(AppError::Config(format!("模型 '{slug}' 已存在")));
    }
    if !known_provider_ids().contains(&model.provider.as_str()) {
        return Err(AppError::Config(format!(
            "未知厂商 '{}'——自定义模型的厂商必须支持直连端点（可选: {}）",
            model.provider,
            known_provider_ids().join(", ")
        )));
    }
    if model.context_window == 0 || model.max_output_tokens == 0 {
        return Err(AppError::Config("上下文窗口和最大输出必须大于 0".into()));
    }
    Ok(())
}

fn is_builtin_slug(slug: &str) -> bool {
    crate::types::builtin_models().iter().any(|m| m.slug == slug)
}

#[tauri::command]
pub fn add_custom_model(mut cfg: AppConfig, mut model: crate::types::ModelDef) -> Result<AppConfig> {
    model.slug = model.slug.trim().to_string();
    validate_custom_model(&cfg, &model, None)?;
    model.enabled = true;
    cfg.models.push(model.clone());
    // Route direct by default so the new model is usable immediately
    cfg.model_routing.insert(model.slug.clone(), "direct".into());
    config_store::save(&cfg)?;
    config_writer::write_all_tool_configs(&cfg)?;
    Ok(cfg)
}

#[tauri::command]
pub fn update_custom_model(mut cfg: AppConfig, old_slug: String, mut model: crate::types::ModelDef) -> Result<AppConfig> {
    model.slug = model.slug.trim().to_string();
    validate_custom_model(&cfg, &model, Some(old_slug.as_str()))?;
    let Some(m) = cfg.models.iter_mut().find(|m| m.slug == old_slug) else {
        return Err(AppError::Config(format!("模型不存在: {old_slug}")));
    };
    *m = model;
    config_store::save(&cfg)?;
    config_writer::write_all_tool_configs(&cfg)?;
    Ok(cfg)
}

#[tauri::command]
pub fn delete_custom_model(mut cfg: AppConfig, slug: String) -> Result<AppConfig> {
    if is_builtin_slug(&slug) {
        return Err(AppError::Config("内置模型不可删除（可在首页取消勾选停用）".into()));
    }
    if !cfg.models.iter().any(|m| m.slug == slug) {
        return Err(AppError::Config(format!("模型不存在: {slug}")));
    }
    cfg.models.retain(|m| m.slug != slug);
    cfg.model_routing.remove(&slug);
    for v in cfg.agent_models.values_mut() {
        v.retain(|s| s != &slug);
    }
    cfg.custom_aliases.retain(|a| a.model != slug);
    config_store::save(&cfg)?;
    config_writer::write_all_tool_configs(&cfg)?;
    Ok(cfg)
}

#[tauri::command]
pub fn known_providers() -> Vec<&'static str> {
    crate::config_writer::known_provider_ids()
}

// ── Relay presets (快速填入, cloud-managed) ──────────────────

/// Cached presets for immediate display when the relay dialog opens.
#[tauri::command]
pub fn get_relay_presets() -> Vec<crate::model_catalog::RelayPreset> {
    crate::model_catalog::read_relay_presets()
}

/// Silent refresh: fetch remote presets with a hard 3s budget. Never errors to
/// the UI — on failure the caller just keeps showing what it already has.
#[tauri::command]
pub async fn refresh_relay_presets() -> Vec<crate::model_catalog::RelayPreset> {
    match crate::model_catalog::fetch_relay_presets().await {
        Ok(p) => p,
        Err(e) => {
            tracing::info!("relay presets refresh skipped: {e}");
            crate::model_catalog::read_relay_presets()
        }
    }
}

// ── Legacy / proxy ─────────────────────────────────────────

#[tauri::command] pub fn write_tool_configs(cfg: AppConfig) -> Result<String> {
    config_store::save(&cfg)?; config_writer::write_all_tool_configs(&cfg)?;
    Ok("All configs written".into())
}
#[tauri::command] pub fn get_proxy_status(proxy_mgr: State<'_, Arc<ProxyManager>>) -> Vec<ProxyStatus> {
    let mgr = proxy_mgr.inner().clone();
    let (tx, rx) = std::sync::mpsc::channel();
    tauri::async_runtime::spawn(async move { let _ = tx.send(mgr.status_all().await); });
    rx.recv().unwrap_or_default()
}
#[tauri::command] pub fn get_app_autostart_status() -> serde_json::Value { serde_json::json!({ "enabled": launchd::autostart_status() }) }
#[tauri::command] pub fn set_app_autostart(enabled: bool) -> Result<serde_json::Value> {
    if enabled { launchd::enable_autostart()?; } else { launchd::disable_autostart()?; }
    Ok(serde_json::json!({ "enabled": launchd::autostart_status() }))
}
#[tauri::command] pub fn quit_app(app: tauri::AppHandle) { app.exit(0); }
#[tauri::command] pub fn hide_main_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") { let _ = w.hide(); }
}

/// Fetch latest model catalog from GitHub, merge into user config.
#[tauri::command]
pub async fn check_model_updates(app: tauri::AppHandle) -> Result<CheckUpdateResult> {
    let remote = model_catalog::fetch_remote_catalog().await?;
    model_catalog::save_catalog_cache(&remote);

    let mut cfg = config_store::load()?;
    let (new_count, new_slugs) =
        model_catalog::merge_remote_models(&mut cfg.models, &remote.models);
    cfg.model_catalog_version = remote.version;
    config_store::save(&cfg)?;

    // Notify frontend to refresh
    let _ = app.emit("config-changed", ());

    Ok(CheckUpdateResult {
        new_models: new_count,
        new_slugs,
        version: remote.version,
        updated_at: remote.updated_at,
    })
}
