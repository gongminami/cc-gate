//! Backup original tool configs before CC-Gate modifies them.
//! Restore = copy original back verbatim.
//!
//! Backups live at ~/.mimo2codex/backups/<name>.orig
//! Each file is backed up ONCE — the first time CC-Gate needs to write it.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::error::Result;
use crate::paths;
use crate::types::agent_list;

fn backup_dir() -> PathBuf {
    paths::mimo2codex_dir().join("backups")
}

fn ensure_backup_dir() -> Result<()> {
    let d = backup_dir();
    fs::create_dir_all(&d)?;
    Ok(())
}

/// Back up `src` to `backup_dir()/<name>.orig` — only if the backup doesn't already exist.
/// Returns true if a backup was created, false if it already existed or src doesn't exist.
pub fn backup_once(src: &PathBuf, name: &str) -> Result<bool> {
    if !src.exists() {
        // Nothing to back up — record this fact by creating an empty marker
        let marker = backup_dir().join(format!("{name}.absent"));
        if !marker.exists() {
            ensure_backup_dir()?;
            fs::write(&marker, "")?;
        }
        return Ok(false);
    }
    let dst = backup_dir().join(format!("{name}.orig"));
    if dst.exists() {
        return Ok(false); // already backed up
    }
    ensure_backup_dir()?;
    fs::copy(src, &dst)?;
    tracing::info!("Backup created: {} → {}", src.display(), dst.display());
    Ok(true)
}

/// Restore `target` from `backup_dir()/<name>.orig`.
/// If the backup doesn't exist (file was absent originally), delete the target.
pub fn restore_from_backup(target: &PathBuf, name: &str) -> Result<bool> {
    let bak = backup_dir().join(format!("{name}.orig"));
    let absent = backup_dir().join(format!("{name}.absent"));

    if bak.exists() {
        // Ensure parent dir exists (e.g. ~/.codex/ might not exist yet)
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&bak, target)?;
        tracing::info!("Restored: {} ← {}", target.display(), bak.display());
        Ok(true)
    } else if absent.exists() {
        // File didn't exist originally — remove CC-Gate's version
        if target.exists() {
            fs::remove_file(target)?;
            tracing::info!("Removed CC-Gate created file: {}", target.display());
        }
        Ok(true)
    } else {
        tracing::warn!("No backup found for {}", name);
        Ok(false)
    }
}

// ── Agent → config file mapping ──────────────────────────

/// For a given agent, return the path that CC-Gate modifies and the backup name.
fn agent_config_target(agent: &crate::types::AgentMeta) -> Option<(PathBuf, &'static str)> {
    use crate::types::AgentId::*;
    match agent.id {
        CodexCli | CodexDesktop | Reasonix => Some((paths::codex_config_toml(), "codex_config.toml")),
        ClaudeCli | ClaudeDesktop => Some((paths::claude_settings_json(), "claude_settings.json")),
        Hermes => Some((paths::hermes_config_yaml(), "hermes_config.yaml")),
        OpenClaw => Some((paths::openclaw_config_json(), "openclaw_config.json")),
        OpenCode => Some((paths::opencode_config_path(), "opencode_config.jsonc")),
        Aider | Cursor => Some((paths::zshrc(), "zshrc")),
        // pi 的 models.json 是合并式写入（只动自己的 provider 键），无需备份
        Pi => None,
    }
}

/// Ensure every agent that WILL be written to has a backup.
/// Call once before write_all_tool_configs.
pub fn ensure_all_backups() {
    for agent in agent_list() {
        if let Some((path, name)) = agent_config_target(&agent) {
            if path.exists() {
                let _ = backup_once(&path, name);
            } else {
                // Record that the file was absent
                let _ = backup_once(&PathBuf::new(), name);
            }
        }
    }
}

/// JSONC/JSON5-lenient parse via the `json5` crate: handles `//` comments and
/// trailing commas (a proper parser — a naive line-based stripper can't tell a
/// trailing comma from a required separator comma like `"a": {...},`).
pub(crate) fn parse_jsonc_lenient(src: &str) -> Option<serde_json::Value> {
    json5::from_str::<serde_json::Value>(src).ok()
}

/// Check whether a given agent's config currently has CC-Gate proxy settings.
pub fn is_agent_proxied(agent: &crate::types::AgentMeta) -> bool {
    use crate::types::AgentId::*;
    match agent.id {
        ClaudeCli | ClaudeDesktop => {
            let path = paths::claude_settings_json();
            if let Ok(content) = fs::read_to_string(&path) {
                content.contains("ANTHROPIC_BASE_URL") && content.contains("8689") && content.contains("127.0.0.1")
            } else { false }
        }
        CodexCli | CodexDesktop | Reasonix => {
            let path = paths::codex_config_toml();
            if let Ok(content) = fs::read_to_string(&path) {
                content.contains("model_provider = \"custom\"") && content.contains("8688") && content.contains("127.0.0.1")
            } else { false }
        }
        Hermes => {
            let path = paths::hermes_config_yaml();
            if let Ok(content) = fs::read_to_string(&path) {
                content.contains("name: ccgate")
            } else { false }
        }
        OpenClaw => {
            let path = paths::openclaw_config_json();
            if let Ok(content) = fs::read_to_string(&path) {
                // 实际写入格式: models.providers.ccgate (map key)，不是 "id":"ccgate"
                serde_json::from_str::<serde_json::Value>(&content)
                    .map(|v| v.pointer("/models/providers/ccgate").is_some())
                    .unwrap_or(false)
            } else { false }
        }
        OpenCode => {
            let path = paths::opencode_config_path();
            if let Ok(content) = fs::read_to_string(&path) {
                // opencode.jsonc 可能是 JSONC(带注释)，lenient 清理后解析
                parse_jsonc_lenient(&content)
                    .map(|v| v.pointer("/provider/ccgate").is_some())
                    .unwrap_or(false)
            } else { false }
        }
        Aider | Cursor => {
            let path = paths::zshrc();
            if let Ok(content) = fs::read_to_string(&path) {
                content.contains("# >>> CC-Gate aliases >>>")
            } else { false }
        }
        Pi => {
            let path = paths::pi_models_json();
            if let Ok(content) = fs::read_to_string(&path) {
                serde_json::from_str::<serde_json::Value>(&content)
                    .map(|v| v.pointer("/providers/ccgate").is_some())
                    .unwrap_or(false)
            } else { false }
        }
    }
}

/// Restore an agent's config to original (copy backup → target file).
/// For zshrc (shared by Aider/Cursor), only removes the CC-Gate aliases block.
pub fn restore_agent_config(agent: &crate::types::AgentMeta) -> Result<bool> {
    use crate::types::AgentId::*;
    match agent.id {
        Aider | Cursor => restore_zshrc_block(),
        _ => {
            if let Some((target, name)) = agent_config_target(agent) {
                restore_from_backup(&target, name)
            } else {
                Ok(false)
            }
        }
    }
}

/// Remove CC-Gate aliases block from .zshrc (does NOT restore the whole file).
fn restore_zshrc_block() -> Result<bool> {
    let path = paths::zshrc();
    if !path.exists() {
        return Ok(true);
    }
    let content = fs::read_to_string(&path).unwrap_or_default();

    const BEGIN: &str = "# >>> CC-Gate aliases >>>";
    const END: &str = "# <<< CC-Gate aliases <<<";

    let start = match content.find(BEGIN) {
        Some(i) => i,
        None => return Ok(false), // no block to remove
    };
    let end = match content.find(END) {
        Some(i) => i + END.len(),
        None => return Ok(false),
    };

    // If line before BEGIN is blank, also remove one blank line
    let before_block = content[..start].trim_end();
    let after_block = content[end..].trim_start();

    let new_content = if before_block.is_empty() {
        after_block.to_string()
    } else {
        format!("{}\n{}", before_block, after_block)
    };

    fs::write(&path, new_content)?;
    tracing::info!("Removed CC-Gate aliases from {}", path.display());
    Ok(true)
}

/// Return per-agent proxied status for the frontend.
pub fn check_all_agent_status() -> HashMap<String, bool> {
    let mut map = HashMap::new();
    for agent in agent_list() {
        let key = crate::types::agent_id_key(&agent.id);
        map.insert(key, is_agent_proxied(&agent));
    }
    map
}

// ── Tauri commands ────────────────────────────���─────────

#[derive(serde::Serialize, Clone)]
pub struct AgentStatus {
    pub agent_id: String,
    pub proxied: bool,
}

#[tauri::command]
pub fn check_agent_status() -> Vec<AgentStatus> {
    agent_list()
        .iter()
        .map(|a| AgentStatus {
            agent_id: crate::types::agent_id_key(&a.id),
            proxied: is_agent_proxied(a),
        })
        .collect()
}

#[derive(serde::Serialize, Clone)]
pub struct RestoreResult {
    pub agent_id: String,
    pub restored: bool,
}

#[tauri::command]
pub fn restore_agent(agent_id: String) -> Result<RestoreResult> {
    let agent = agent_list()
        .into_iter()
        .find(|a| crate::types::agent_id_key(&a.id) == agent_id);
    match agent {
        Some(a) => {
            let restored = restore_agent_config(&a)?;
            Ok(RestoreResult { agent_id, restored })
        }
        None => Ok(RestoreResult { agent_id, restored: false }),
    }
}
