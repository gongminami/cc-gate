//! Remote model catalog — fetches latest model definitions from GitHub.
//! Local cache → remote fetch → merge into user config.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::paths;
use crate::types::ModelDef;

/// Remote catalog JSON URL (raw GitHub — updated by maintainers when vendors release new models).
const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/gongminami/cc-gate/main/models-catalog.json";

/// Deserialized from `models-catalog.json` hosted on GitHub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteCatalog {
    pub version: u32,
    pub updated_at: String,
    pub models: Vec<ModelDef>,
}

/// Path to local cache file: `~/.mimo2codex/models-cache.json`
pub fn catalog_cache_path() -> PathBuf {
    paths::mimo2codex_dir().join("models-cache.json")
}

/// Read the cached remote catalog (if it exists).
pub fn read_catalog_cache() -> Option<RemoteCatalog> {
    let path = catalog_cache_path();
    if !path.exists() {
        return None;
    }
    let data = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data)
        .inspect_err(|e| tracing::warn!("catalog cache parse failed: {e}"))
        .ok()
}

/// Fetch the remote catalog from GitHub.
pub async fn fetch_remote_catalog() -> Result<RemoteCatalog, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let resp = client
        .get(CATALOG_URL)
        .send()
        .await
        .map_err(|e| format!("网络请求失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("服务器返回 {}", resp.status()));
    }

    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取响应失败: {e}"))?;

    serde_json::from_str(&text).map_err(|e| format!("模型 JSON 解析失败: {e}"))
}

/// Write catalog to local cache.
pub fn save_catalog_cache(catalog: &RemoteCatalog) {
    let path = catalog_cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(catalog) {
        if let Err(e) = fs::write(&path, json) {
            tracing::warn!("catalog cache write failed: {e}");
        } else {
            tracing::info!("catalog cache saved (version {})", catalog.version);
        }
    }
}

/// Merge remote catalog models into the existing user config model list.
///
/// | Scenario | Handling |
/// |---|---|
/// | Remote has new models (not in existing) | Append with remote's `enabled` value |
/// | Remote has existing models | Update params (context_window, pricing, etc.); **keep** user's `enabled` state |
/// | Models only in local | Preserve as-is (no deletion) |
///
/// Returns (new_model_count, slugs_of_new_models).
pub fn merge_remote_models(existing: &mut Vec<ModelDef>, remote: &[ModelDef]) -> (u32, Vec<String>) {
    let mut new_slugs: Vec<String> = Vec::new();

    for r in remote {
        if let Some(emu) = existing.iter_mut().find(|e| e.slug == r.slug) {
            // Refresh parametric fields from remote (keep user's enabled + priority)
            emu.display_name = r.display_name.clone();
            emu.provider = r.provider.clone();
            emu.context_window = r.context_window;
            emu.max_output_tokens = r.max_output_tokens;
            emu.default_reasoning_level = r.default_reasoning_level.clone();
            emu.supports_reasoning_summaries = r.supports_reasoning_summaries;
            emu.input_price_per_1k = r.input_price_per_1k;
            emu.output_price_per_1k = r.output_price_per_1k;
        } else {
            // New model — add with remote's default enabled state
            new_slugs.push(r.slug.clone());
            existing.push(r.clone());
        }
    }

    let count = new_slugs.len() as u32;
    if count > 0 {
        tracing::info!("catalog merge: {} new models ({})", count, new_slugs.join(", "));
    }
    (count, new_slugs)
}

/// Result returned by the `check_model_updates` Tauri command.
#[derive(Debug, Clone, Serialize)]
pub struct CheckUpdateResult {
    /// How many brand-new models were discovered.
    pub new_models: u32,
    /// Slugs of those new models (for frontend badge display).
    pub new_slugs: Vec<String>,
    /// Remote catalog version number.
    pub version: u32,
    /// ISO8601 timestamp from the remote catalog.
    pub updated_at: String,
}

/// Result returned by the `check_app_update` Tauri command.
#[derive(Debug, Clone, Serialize)]
pub struct AppUpdateInfo {
    /// Whether the remote release is newer than the running app.
    pub has_update: bool,
    /// Version of the running app (from Cargo.toml).
    pub current_version: String,
    /// Latest version on GitHub Releases (no `v` prefix).
    pub latest_version: String,
    /// Releases page URL for the user to download from.
    pub release_url: String,
    /// Release notes (body of the GitHub release).
    pub notes: String,
}

const RELEASES_URL: &str = "https://api.github.com/repos/gongminami/cc-gate/releases/latest";

/// Minimal shape of the GitHub Releases "latest" API response.
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Fetch the latest published release from GitHub Releases.
/// `has_update` is filled by the caller (needs the local version).
pub async fn fetch_latest_release() -> Result<AppUpdateInfo, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("cc-gate/update-check")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let resp = client
        .get(RELEASES_URL)
        .send()
        .await
        .map_err(|e| format!("网络请求失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub 返回 {}", resp.status()));
    }

    let text = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
    let rel: GitHubRelease = serde_json::from_str(&text)
        .map_err(|e| format!("响应解析失败: {e}"))?;

    if rel.draft || rel.prerelease {
        return Err("当前没有正式发布版本".into());
    }

    Ok(AppUpdateInfo {
        has_update: false,
        current_version: String::new(),
        latest_version: rel.tag_name.trim_start_matches('v').to_string(),
        release_url: rel.html_url,
        notes: rel.body.unwrap_or_default(),
    })
}

// ── Relay presets (快速填入, cloud-managed) ──────────────────/// One "quick fill" preset for the relay-add dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPreset {
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RelayPresetsFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub presets: Vec<RelayPreset>,
}

const RELAY_PRESETS_URL: &str =
    "https://raw.githubusercontent.com/gongminami/cc-gate/main/relay-presets.json";

/// Shown when no cache exists yet (fresh install + offline).
fn builtin_relay_presets() -> Vec<RelayPreset> {
    vec![
        RelayPreset { name: "OpenRouter".into(), url: "https://openrouter.ai/api/v1".into(), anthropic_url: None },
        RelayPreset { name: "Gemini".into(), url: "https://generativelanguage.googleapis.com/v1beta/openai".into(), anthropic_url: None },
    ]
}

pub fn relay_presets_cache_path() -> PathBuf {
    paths::mimo2codex_dir().join("relay-presets-cache.json")
}

/// Cached presets from the last successful fetch; falls back to built-ins.
pub fn read_relay_presets() -> Vec<RelayPreset> {
    let path = relay_presets_cache_path();
    if path.exists() {
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<Vec<RelayPreset>>(&data) {
                if !v.is_empty() { return v; }
            }
        }
    }
    builtin_relay_presets()
}

fn save_relay_presets_cache(presets: &[RelayPreset]) {
    let path = relay_presets_cache_path();
    if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
    if let Ok(json) = serde_json::to_string_pretty(presets) {
        let _ = fs::write(&path, json);
    }
}

/// Fetch presets from GitHub. Caller enforces the timeout budget — the dialog
/// shows cached presets immediately and silently refreshes when this returns.
pub async fn fetch_relay_presets() -> Result<Vec<RelayPreset>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let resp = client
        .get(RELAY_PRESETS_URL)
        .send()
        .await
        .map_err(|e| format!("网络请求失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("服务器返回 {}", resp.status()));
    }

    let text = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
    let file: RelayPresetsFile = serde_json::from_str(&text)
        .map_err(|e| format!("预设 JSON 解析失败: {e}"))?;
    if file.presets.is_empty() {
        return Err("远端预设为空".into());
    }
    save_relay_presets_cache(&file.presets);
    Ok(file.presets)
}
