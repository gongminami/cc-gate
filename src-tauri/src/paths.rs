use std::path::PathBuf;

use crate::error::Result;

pub fn home() -> PathBuf {
    dirs::home_dir().expect("home dir")
}

pub fn config_dir() -> PathBuf {
    home().join(".CC-Gate")
}

pub fn mimo2codex_dir() -> PathBuf {
    home().join(".mimo2codex")
}

pub fn logs_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let d = home().join("Library/Logs/CC-Gate");
    #[cfg(target_os = "windows")]
    let d = {
        let appdata = std::env::var("LOCALAPPDATA").map(PathBuf::from)
            .unwrap_or_else(|_| home().join("AppData").join("Local"));
        appdata.join("CC-Gate").join("logs")
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let d = home().join(".local/share/CC-Gate/logs");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn app_launchagent_plist() -> Result<PathBuf> {
    Ok(home().join("Library/LaunchAgents/com.CC-Gate.app.plist"))
}

/// Proxy launchd plist paths
pub fn proxy_launchagent_plist(name: &str) -> Result<PathBuf> {
    Ok(home().join(format!("Library/LaunchAgents/com.CC-Gate.{name}.plist")))
}

/// App config JSON stored in ~/.CC-Gate/config.json
pub fn app_config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// External tool config paths
pub fn codex_config_toml() -> PathBuf {
    home().join(".codex/config.toml")
}

pub fn codex_model_catalog_json() -> PathBuf {
    // Codex桌面端 /model 菜单只认 cc-switch-model-catalog.json 文件名
    home().join(".codex/cc-switch-model-catalog.json")
}

pub fn claude_settings_json() -> PathBuf {
    home().join(".claude/settings.json")
}

pub fn providers_json() -> PathBuf {
    mimo2codex_dir().join("providers.json")
}

/// Alias routing table for the local proxies: token `ccgate-<name>` → upstream.
/// Written by config_writer::write_alias_routes, hot-reloaded by the JS proxies.
pub fn aliases_json() -> PathBuf {
    mimo2codex_dir().join("aliases.json")
}

/// pi coding agent's custom provider definitions (~/.pi/agent/models.json).
/// CC-Gate merges a `ccgate` provider + per-alias `ccgate-<name>` providers
/// into it, preserving everything the user defined themselves.
pub fn pi_models_json() -> PathBuf {
    home().join(".pi/agent/models.json")
}

pub fn mimo_env() -> PathBuf {
    mimo2codex_dir().join(".env")
}

/// Returns the best shell config file(s) to inject aliases into.
/// macOS → ~/.zshrc
/// Linux → ~/.zshrc (if exists) else ~/.bashrc
/// Windows → writes both Git-Bash (~/.bashrc) + PowerShell ($PROFILE)
pub fn shell_configs() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        return vec![home().join(".zshrc")];
    }
    if cfg!(target_os = "linux") {
        let zshrc = home().join(".zshrc");
        if zshrc.exists() { return vec![zshrc]; }
        return vec![home().join(".bashrc")];
    }
    if cfg!(target_os = "windows") {
        let mut paths = vec![];
        // Git Bash / MSYS2
        paths.push(home().join(".bashrc"));
        // PowerShell 5.1
        let ps5 = home().join("Documents").join("WindowsPowerShell");
        let _ = std::fs::create_dir_all(&ps5);
        paths.push(ps5.join("Microsoft.PowerShell_profile.ps1"));
        // PowerShell 7+ (Core)
        let ps7 = home().join("Documents").join("PowerShell");
        if !ps7.exists() { let _ = std::fs::create_dir_all(&ps7); }
        paths.push(ps7.join("Microsoft.PowerShell_profile.ps1"));
        return paths;
    }
    vec![home().join(".zshrc")]
}

/// Short human-readable description of all shell configs being used.
pub fn shell_description() -> String {
    if cfg!(target_os = "macos") {
        "~/.zshrc".into()
    } else if cfg!(target_os = "linux") {
        "~/.zshrc（如存在）或 ~/.bashrc".into()
    } else if cfg!(target_os = "windows") {
        "Git-Bash（~/.bashrc）和 PowerShell（$PROFILE）".into()
    } else {
        "~/.zshrc".into()
    }
}

/// The reload command for the detected shell.
pub fn shell_reload_cmd() -> &'static str {
    if cfg!(target_os = "macos") {
        "source ~/.zshrc 或新开终端"
    } else if cfg!(target_os = "linux") {
        "source ~/.zshrc 或新开终端"
    } else if cfg!(target_os = "windows") {
        "新开 Git Bash / 新开 PowerShell 窗口"
    } else {
        "source ~/.zshrc 或新开终端"
    }
}

pub fn zshrc() -> PathBuf {
    home().join(".zshrc")
}

pub fn hermes_config_yaml() -> PathBuf {
    home().join(".hermes/config.yaml")
}

pub fn opencode_config_path() -> PathBuf {
    // opencode 实际读 ~/.config/opencode/opencode.jsonc（JSONC），不是 config.toml
    home().join(".config/opencode/opencode.jsonc")
}

pub fn openclaw_config_json() -> PathBuf {
    home().join(".openclaw/openclaw.json")
}

pub fn ensure_dirs() -> Result<()> {
    let dirs: &[PathBuf] = &[config_dir(), mimo2codex_dir()];
    for d in dirs {
        if !d.exists() {
            std::fs::create_dir_all(d)?;
        }
    }
    Ok(())
}
