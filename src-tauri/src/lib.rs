use std::sync::{Arc, Mutex, OnceLock};
use tauri::{Emitter, Manager as _};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{fmt, EnvFilter, Registry};

static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub mod error;
pub mod paths;
pub mod types;
pub mod config_store;
pub mod config_writer;
pub mod proxy_manager;
pub mod launchd;
pub mod tray;
pub mod commands;
pub mod usage_db;
pub mod tool_check;
pub mod model_catalog;
pub mod backup;
#[cfg(windows)]
mod win_console;
#[cfg(not(windows))]
mod win_console {
    // stub module — hide_console_async is a no-op on non-Windows
    pub fn hide_console_async(_cmd: &mut tokio::process::Command) {}
    pub fn hide_console(_cmd: &mut std::process::Command) {}
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();
    if let Err(e) = paths::ensure_dirs() {
        eprintln!("ensure_dirs failed: {e}");
    }

    let proxy_mgr = Arc::new(proxy_manager::ProxyManager::new());

    let proxy_mgr_for_setup = proxy_mgr.clone();
    let proxy_mgr_for_exit = proxy_mgr.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .manage(proxy_mgr)
        .setup(move |app| {
            if let Err(e) = tray::setup(&app.handle()) {
                tracing::warn!(error = %e, "tray setup failed");
            }

            if let Some(win) = app.get_webview_window("main") {
                let win_for_close = win.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win_for_close.hide();
                    }
                });
            }

            let m = proxy_mgr_for_setup.clone();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Deploy proxy scripts first, then start proxies
                if let Err(e) = crate::config_writer::deploy_proxy_scripts() {
                    tracing::warn!("deploy_proxy_scripts failed: {e}");
                }
                let _ = m.start_enabled().await;
                tray::force_refresh(&app_handle).await;
            });

            // Background: try to refresh model catalog from GitHub
            let app_handle2 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match crate::model_catalog::fetch_remote_catalog().await {
                    Ok(remote) => {
                        crate::model_catalog::save_catalog_cache(&remote);
                        // If new models found, merge into config silently
                        if let Ok(mut cfg) = crate::config_store::load() {
                            if cfg.model_catalog_version < remote.version {
                                let (count, _) = crate::model_catalog::merge_remote_models(
                                    &mut cfg.models, &remote.models,
                                );
                                cfg.model_catalog_version = remote.version;
                                if let Err(e) = crate::config_store::save(&cfg) {
                                    tracing::warn!("catalog background save failed: {e}");
                                } else if count > 0 {
                                    let _ = app_handle2.emit("config-changed", ());
                                    tracing::info!("catalog background refresh: {count} new models");
                                }
                            }
                        }
                    }
                    Err(e) => tracing::info!("catalog background fetch skipped: {e}"),
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            commands::config::get_config,
            commands::config::save_config,
            commands::config::get_agent_list,
            commands::config::get_shell_info,
            commands::config::apply_agent_config,
            commands::config::add_relay,
            commands::config::update_relay,
            commands::config::delete_relay,
            commands::config::add_alias,
            commands::config::update_alias,
            commands::config::delete_alias,
            commands::config::add_custom_model,
            commands::config::update_custom_model,
            commands::config::delete_custom_model,
            commands::config::known_providers,
            commands::config::get_relay_presets,
            commands::config::refresh_relay_presets,
            commands::config::write_tool_configs,
            commands::proxy::start_proxy,
            commands::proxy::stop_proxy,
            commands::proxy::restart_proxy,
            commands::config::get_proxy_status,
            commands::config::get_app_autostart_status,
            commands::config::set_app_autostart,
            commands::config::quit_app,
            commands::config::hide_main_window,
            commands::usage::get_usage_summary,
            commands::usage::get_daily_usage,
            commands::usage::get_recent_logs,
            commands::usage::get_per_model_usage,
            commands::usage::import_usage_data,
            commands::usage::get_app_log_tail,
            commands::usage::get_app_version,
            commands::usage::copy_to_clipboard,
            commands::config::check_model_updates,
            commands::config::discover_relay_models,
            commands::config::set_relay_enabled,
            commands::config::set_relay_model_selection,
            commands::config::set_model_routing,
            commands::config::check_app_update,
            commands::config::open_url,
            check_tools,
            check_one_tool,
            save_tool_cache,
            crate::backup::check_agent_status,
            crate::backup::restore_agent,
        ])
        .build(tauri::generate_context!())
        .expect("error while building CC-Gate");

    app.run(move |_handle, event| {
        #[allow(unused_variables)]
        let handle = &_handle;
        // Exit handler (all platforms)
        if matches!(event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit) {
            let m = proxy_mgr_for_exit.clone();
            tauri::async_runtime::block_on(async move {
                m.shutdown_all().await;
            });
            return;
        }
        // Dock click re-open (macOS only)
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { has_visible_windows: false, .. } = event {
            if let Some(w) = handle.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }
    });
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let stdout_layer = fmt::layer().with_target(false);

    let file_layer = paths::logs_dir().ok().and_then(|dir| {
        let _ = std::fs::create_dir_all(&dir);
        let appender = tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("app")
            .filename_suffix("log")
            .max_log_files(7)
            .build(&dir)
            .ok()?;
        let (writer, guard) = tracing_appender::non_blocking(appender);
        let _ = LOG_GUARD.set(guard);
        Some(
            fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_target(false),
        )
    });

    let _ = Registry::default()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer)
        .try_init();

    let _ = std::any::type_name::<Mutex<()>>();
}

#[tauri::command]
fn ping() -> &'static str { "pong" }

#[tauri::command]
fn check_tools(force: Option<bool>) -> Vec<crate::tool_check::ToolStatus> {
    if force.unwrap_or(false) {
        crate::tool_check::refresh()
    } else {
        crate::tool_check::check_all()
    }
}

/// 逐条工具检测：前端逐个调用，每调用一次检测一个工具、返回一个结果
#[tauri::command]
fn check_one_tool(name: String) -> Option<crate::tool_check::ToolStatus> {
    crate::tool_check::check_one(&name)
}

/// 渐进式检测完成后保存结果到缓存
#[tauri::command]
fn save_tool_cache(results: Vec<crate::tool_check::ToolStatus>) {
    crate::tool_check::save_to_cache(results);
}
