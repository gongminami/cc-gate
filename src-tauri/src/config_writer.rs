//! Writes configuration to external tool config files.
//! Phase 4: per-model routing (direct | relay:<name>).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::PathBuf;
use std::io::Write;

use crate::error::Result;
use crate::paths;
use crate::types::{AppConfig, ModelDef, agent_list};

// ── Provider metadata (native direct endpoints) ─────────────

struct ProviderMeta {
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
    /// Primary env key name. We also check aliases when writing providers.json.
    env_key: &'static str,
    /// Alternative env key names to fall back to (checked in .env). Order = preference.
    env_key_aliases: &'static [&'static str],
    feature: Option<&'static str>,
}

const PROVIDER_META: &[ProviderMeta] = &[
    ProviderMeta { id: "deepseek",  name: "DeepSeek",       base_url: "https://api.deepseek.com/v1",                                                  env_key: "DEEPSEEK_API_KEY", env_key_aliases: &["DS_API_KEY"],     feature: None },
    ProviderMeta { id: "glm",       name: "智谱GLM",        base_url: "https://open.bigmodel.cn/api/paas/v4",                                         env_key: "GLM_API_KEY",      env_key_aliases: &["ZHIPU_API_KEY"],  feature: Some("forceParallelToolCalls") },
    ProviderMeta { id: "qwen",      name: "阿里Qwen-Max",   base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",                             env_key: "QWEN_API_KEY",     env_key_aliases: &[],                 feature: None },
    ProviderMeta { id: "qwen38",    name: "阿里Qwen3.8",    base_url: "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",          env_key: "QWEN38_API_KEY",   env_key_aliases: &[],                 feature: None },
    ProviderMeta { id: "xiaomi",    name: "小米MiMo",       base_url: "https://api.xiaomimimo.com/v1",                                                  env_key: "MIMO_API_KEY",     env_key_aliases: &["MINIMAX_API_KEY"], feature: None },
    ProviderMeta { id: "anthropic", name: "Anthropic Opus", base_url: "https://api.anthropic.com",                                                      env_key: "",                 env_key_aliases: &[],                 feature: None },
    ProviderMeta { id: "openai",    name: "OpenAI GPT",     base_url: "https://api.openai.com/v1",                                                      env_key: "",                 env_key_aliases: &[],                 feature: None },
    // Gemini 官方 OpenAI 兼容端点(原生 GenerateContent 协议由 Google 侧转换成 Chat Completions,
    // 本地三个代理的 baseUrl + "/chat/completions" 拼接恰好命中 .../v1beta/openai/chat/completions,无需 /v1)。
    ProviderMeta { id: "gemini",    name: "Google Gemini",  base_url: "https://generativelanguage.googleapis.com/v1beta/openai",                        env_key: "GEMINI_API_KEY",  env_key_aliases: &["GOOGLE_API_KEY"], feature: None },
];

fn meta_by_id(id: &str) -> Option<&'static ProviderMeta> {
    PROVIDER_META.iter().find(|m| m.id == id)
}

/// Pick the best env key name — the one that actually exists in .env wins.
/// If the primary key or any alias is present in .env, use that name.
/// Otherwise default to the primary key (so the user knows what to create).
fn resolve_env_key(meta: &ProviderMeta) -> String {
    if meta.env_key.is_empty() { return String::new(); }

    let env_path = paths::mimo_env();
    if let Ok(content) = std::fs::read_to_string(&env_path) {
        // Build a set of present key names
        let present: std::collections::HashSet<&str> = content.lines()
            .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim()))
            .collect();

        // Prefer the primary key if it exists
        if present.contains(meta.env_key) {
            return meta.env_key.to_string();
        }
        // Try each alias
        for alias in meta.env_key_aliases {
            if present.contains(alias) {
                return alias.to_string();
            }
        }
    }

    // Not in .env — return the primary (canonical) name
    meta.env_key.to_string()
}

// ── providers.json ────────────────��──────────────────────────

/// Sanitize a provider/routing identifier: keep only [a-z0-9_-].
/// Non-ASCII characters (e.g. Chinese) are removed to comply with
/// mimo2codex provider ID validation.
fn sanitize_provider_id(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect::<String>()
        .to_lowercase()
}

/// Env var name for a relay's API key. **Single source of truth** — every writer
/// (.env, providers.json `envKey`, key pruning) must call this, or the name written
/// to .env won't match the name providers.json tells the proxy to look up.
///
/// Non-ASCII names are transliterated to a stable `X<hex>` token rather than being
/// filtered out: dropping them collapsed every CJK-named relay onto the same
/// `RELAY__API_KEY`, so multiple relays silently overwrote each other's key.
pub fn relay_env_key(relay_name: &str) -> String {
    let mut out = String::new();
    for c in relay_name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_uppercase());
        } else if c == ' ' || c == '-' || c == '_' {
            out.push('_');
        } else {
            // Stable per-character transliteration keeps distinct names distinct.
            out.push_str(&format!("X{:X}", c as u32));
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    let stem = if trimmed.is_empty() { "UNNAMED".to_string() } else { trimmed };
    format!("RELAY_{stem}_API_KEY")
}

/// Relay URLs are sometimes configured with the FULL OpenAI endpoint path
/// (e.g. `https://…/v1/chat/completions`), but every local proxy (claude-proxy,
/// chat-proxy, mimo2codex) appends `/chat/completions` itself when forwarding.
/// Normalize to the base URL so a doubled path can't produce `404 page not found`
/// upstream (observed with deepseek-v4-flash → 商汤日日新 relay, 2026-08).
fn normalize_relay_base_url(raw: &str) -> String {
    let trimmed = raw.trim_end_matches('/');
    trimmed
        .strip_suffix("/chat/completions")
        .unwrap_or(trimmed)
        .to_string()
}

pub fn write_providers(cfg: &AppConfig) -> Result<()> {
    // Collect enabled model slugs from all agents that write_providers
    let enabled_slugs: BTreeSet<String> = agent_list().iter()
        .filter(|a| a.writes_providers)
        .flat_map(|a| cfg.agent_models.get(&crate::types::agent_id_key(&a.id)).cloned().unwrap_or_default())
        .collect();

    // Group models: key = (provider_id, routing)
    type ProviderKey = (String, String);
    let mut groups: BTreeMap<ProviderKey, Vec<&ModelDef>> = BTreeMap::new();

    for m in &cfg.models {
        if !enabled_slugs.contains(&m.slug) { continue; }

        let routing = cfg.model_routing.get(&m.slug)
            .map(|s| s.as_str())
            .unwrap_or("direct");

        let key = (m.provider.clone(), routing.to_string());
        groups.entry(key).or_default().push(m);
    }

    let relay_by_name: BTreeMap<&str, &crate::types::RelayConfig> = cfg.relays.iter()
        .map(|r| (r.name.as_str(), r))
        .collect();

    let mut entries: Vec<serde_json::Value> = Vec::new();

    for ((provider_id, routing), models) in &groups {
        if models.is_empty() { continue; }

        let (base_url, env_key, display_suffix) = if routing == "direct" {
            let meta = meta_by_id(provider_id);
            if meta.is_none() { continue; }
            let meta = meta.unwrap();
            if meta.env_key.is_empty() {
                // Provider with no env key (Anthropic/OpenAI) — skip from providers.json
                // (claude-proxy.js handles anthropic passthrough as a built-in)
                continue;
            }
            (meta.base_url.to_string(), resolve_env_key(meta), String::new())
        } else if routing.starts_with("relay:") {
            let relay_name = &routing[6..];
            let relay = relay_by_name.get(relay_name);
            if relay.is_none() { continue; }
            let relay = relay.unwrap();
            let env_key = relay_env_key(relay_name);
            // If relay has an anthropic_url and the provider is anthropic, use that URL
            // for native protocol passthrough (no translation). Otherwise use the OpenAI URL.
            // Either way, normalize away a trailing /chat/completions so the proxies'
            // own suffix append can't produce a doubled path (see normalize_relay_base_url).
            let url = if provider_id == "anthropic" {
                normalize_relay_base_url(&relay.anthropic_url.clone().unwrap_or_else(|| relay.url.clone()))
            } else {
                normalize_relay_base_url(&relay.url)
            };
            (url, env_key, format!(" via {}", relay_name))
        } else {
            continue;
        };

        let display_name = meta_by_id(provider_id)
            .map(|m| format!("{}{}", m.name, display_suffix))
            .unwrap_or_else(|| format!("{}{}", provider_id, display_suffix));

        let feature = meta_by_id(provider_id).and_then(|m| m.feature);

        let provider_entry = serde_json::json!({
            "id": format!("{}-{}",
                sanitize_provider_id(provider_id),
                sanitize_provider_id(&routing.replace(':', "-"))),
            "name": display_name,
            "baseUrl": base_url,
            "envKey": env_key,
            "defaultModel": models[0].slug,
            "models": models.iter().map(|m| serde_json::json!({
                "id": m.slug,
                "displayName": m.display_name,
                "contextWindow": m.context_window,
                "maxOutputTokens": m.max_output_tokens,
            })).collect::<Vec<_>>(),
        });

        let mut entry = provider_entry;
        if feature.is_some() {
            entry["features"] = serde_json::json!({"forceParallelToolCalls": true});
        }
        // Mark Anthropic-native endpoints so claude-proxy.js does native passthrough
        let is_anthropic_native = provider_id == "anthropic" && (
            routing == "direct" ||
            routing.starts_with("relay:")
        );
        if is_anthropic_native {
            entry["anthropicEndpoint"] = serde_json::json!(true);
        }
        entries.push(entry);
    }

    let content = serde_json::to_string_pretty(&serde_json::json!({ "providers": entries }))?;
    write_if_changed(&paths::providers_json(), &content)
}

// ── .env relay keys ─────────────────────────────────────────

pub fn write_env_relay_keys(cfg: &AppConfig) -> Result<()> {
    let env_path = paths::mimo_env();
    let existing = if env_path.exists() { fs::read_to_string(&env_path).unwrap_or_default() } else { String::new() };

    let mut lines: Vec<String> = existing.lines()
        .filter(|l| !l.trim().starts_with("RELAY_") || !l.contains("_API_KEY"))
        .map(|l| l.to_string())
        .collect();

    // Append relay keys
    for relay in &cfg.relays {
        let env_key = relay_env_key(&relay.name);
        lines.push(format!("{env_key}={}", relay.key));
    }

    let content = lines.join("\n").trim_end().to_string() + "\n";
    write_if_changed(&env_path, &content)
}

// ── Codex config.toml ────────────────────────────────────────

pub fn write_codex_config(cfg: &AppConfig) -> Result<()> {
    // Merge Codex Desktop + Reasonix models for default model (they share config.toml)
    let mut all_slugs: Vec<String> = cfg.agent_models
        .get("codex_desktop").cloned().unwrap_or_default();
    all_slugs.extend(
        cfg.agent_models.get("reasonix").cloned().unwrap_or_default()
    );
    all_slugs.sort();
    all_slugs.dedup();

    let default_model = all_slugs.first().cloned()
        .unwrap_or_else(|| "deepseek-v4-pro".into());

    let default_model_def = cfg.models.iter().find(|m| m.slug == default_model);
    let default_ctxt = default_model_def.map(|m| m.context_window).unwrap_or(1_000_000);
    let default_max_out = default_model_def.map(|m| m.max_output_tokens).unwrap_or(393_216);
    let base_url = format!("http://127.0.0.1:{}/v1", cfg.proxy_ports.mimo2codex);

    let content = format!(r#"model_provider = "custom"
model = "{default_model}"
model_reasoning_effort = "high"
model_context_window = {default_ctxt}
model_max_output_tokens = {default_max_out}
model_catalog_json = "cc-switch-model-catalog.json"

[model_providers.custom]
name = "CC-Gate"
base_url = "{base_url}"
wire_api = "responses"
requires_openai_auth = true"#);

    // Preserve user's [projects.*] (trusted dirs) and [mcp_servers.*] sections from the
    // existing config.toml — wholesale replacement would silently wipe them.
    let content = preserve_user_sections(&paths::codex_config_toml(), &content, &["projects", "mcp_servers"]);

    write_if_changed(&paths::codex_config_toml(), &content)
}

/// Append `[projects.*]` / `[mcp_servers.*]` sections from an existing TOML file to
/// newly generated content, so user-managed sections survive a config rewrite.
fn preserve_user_sections(existing_path: &std::path::Path, generated: &str, keys: &[&str]) -> String {
    let Ok(src) = std::fs::read_to_string(existing_path) else { return generated.to_string(); };
    let Ok(doc) = src.parse::<toml::Table>() else { return generated.to_string(); };

    let mut extra = String::new();
    for key in keys {
        if let Some(val) = doc.get(*key) {
            // Wrap in a temp table so the serializer emits the full header path
            // ([projects."/x"], not a bare ["/x"]) — toml::to_string(val) drops the key name.
            let mut wrapper = toml::Table::new();
            wrapper.insert((*key).to_string(), val.clone());
            if let Ok(serialized) = toml::to_string(&wrapper) {
                extra.push_str(&serialized);
                extra.push('\n');
            }
        }
    }
    if extra.is_empty() { generated.to_string() } else { format!("{generated}\n{extra}") }
}

// ── Model catalog ────────────────────────────────────────────

pub fn write_model_catalog(cfg: &AppConfig) -> Result<()> {
    // Merge models from both Codex CLI and Codex Desktop — both share the same proxy (mimo2codex)
    let mut codex_slugs: BTreeSet<String> = cfg.agent_models
        .get("codex_desktop").cloned().unwrap_or_default()
        .into_iter().collect();
    codex_slugs.extend(
        cfg.agent_models.get("codex_cli").cloned().unwrap_or_default()
    );
    let codex_set: BTreeSet<&str> = codex_slugs.iter().map(|s| s.as_str()).collect();

    let models: Vec<serde_json::Value> = cfg.models.iter()
        .filter(|m| codex_set.contains(m.slug.as_str()))
        .map(|m| serde_json::json!({
            "slug": m.slug, "display_name": m.display_name,
            "context_window": m.context_window, "max_context_window": m.context_window,
            "effective_context_window_percent": 95,
            "default_reasoning_level": m.default_reasoning_level,
            "default_reasoning_summary": "none", "input_modalities": ["text"],
            "supported_reasoning_levels": [
                {"effort":"none","description":"Disable Thinking"},
                {"effort":"low","description":"Low"},
                {"effort":"medium","description":"Medium"},
                {"effort":"high","description":"High"},
                {"effort":"xhigh","description":"Extra high"}
            ],
            "supports_reasoning_summaries": m.supports_reasoning_summaries,
            "supports_parallel_tool_calls": false, "supports_search_tool": false,
            "support_verbosity": false, "supported_in_api": true,
            "shell_type": "shell_command", "apply_patch_tool_type": "freeform",
            "visibility": "list", "priority": m.priority,
            "additional_speed_tiers": [], "service_tiers": [],
            "experimental_supported_tools": [],
            "truncation_policy": {"mode":"bytes","limit":10000},
            "base_instructions": format!("You are Codex, a coding agent powered by {}. You help the user with programming tasks. Read the codebase first, ask questions when needed, and implement solutions directly. Prefer existing patterns and keep changes minimal.", m.display_name),
            "description": format!("{} model via CC-Gate proxy", m.display_name),
            "default_verbosity": "low",
            "supports_image_detail_original": false,
            "upgrade": null
        }))
        .collect();

    let content = serde_json::to_string_pretty(&serde_json::json!({ "models": models }))?;
    write_if_changed(&paths::codex_model_catalog_json(), &content)
}

// ── Claude settings ──────────────────────────────────────────

pub fn write_claude_settings(cfg: &AppConfig) -> Result<()> {
    // Merge models from both Claude CLI and Claude Desktop
    let mut claude_slugs: BTreeSet<String> = cfg.agent_models
        .get("claude_desktop").cloned().unwrap_or_default()
        .into_iter().collect();
    claude_slugs.extend(
        cfg.agent_models.get("claude_cli").cloned().unwrap_or_default()
    );

    // Default model: first assigned (BTreeSet iterates sorted for stability), fallback to deepseek
    let _default_model = claude_slugs.iter().next()
        .map(|s| if s.starts_with("claude-") { s.clone() } else { format!("claude-{}", s) })
        .unwrap_or_else(|| "claude-deepseek-v4-pro".into());

    let base_url = format!("http://127.0.0.1:{}", cfg.proxy_ports.claude_proxy);

    // ── Deploy status-line script ──
    let status_line_script = paths::mimo2codex_dir().join("status-line.sh");
    let _ = fs::write(&status_line_script, include_str!("../../scripts/status-line.sh"));

    // StatusLine command: "bash ~/.mimo2codex/status-line.sh" — Claude Code pipes JSON via stdin
    let status_line_cmd = format!("bash {}", status_line_script.display());

    let settings = serde_json::json!({
        "env": {"ANTHROPIC_BASE_URL": base_url},
        // Do NOT set "model" to a full model name — the "model" key selects the tier
        // (opus/sonnet/haiku), and all four tier env vars are already set by shell aliases.
        // Writing a non-tier value like "claude-deepseek-v4-pro" would confuse Claude Code.
        "effortLevel": "xhigh",
        "statusLine": {
            "type": "command",
            "command": status_line_cmd,
        },
    });
    write_if_changed(&paths::claude_settings_json(), &serde_json::to_string_pretty(&settings)?)
}

// ── Deploy proxy scripts to ~/.mimo2codex/ ──────────────────

/// Copy built-in proxy scripts to ~/.mimo2codex/ so the proxy manager can launch them.
/// Scripts are embedded at compile time via `include_str!` — no runtime file dependency.
///
/// Uses `write_if_changed`: the shipped script is the source of truth, so an upgraded
/// CC-Gate always refreshes a stale runtime copy, while an already-current file costs
/// no disk write. The previous `if !exists` guard meant users who had ever launched an
/// older build kept its buggy proxy forever — fixes shipped in the binary never landed.
///
/// Note: this overwrites hand-patched copies of ~/.mimo2codex/*.js. Patch the repo-root
/// script and rebuild instead of editing the deployed copy.
pub fn deploy_proxy_scripts() -> Result<()> {
    let dest = paths::mimo2codex_dir();
    fs::create_dir_all(&dest)?;

    // claude-proxy.js — refresh when the shipped content differs
    write_if_changed(&dest.join("claude-proxy.js"), include_str!("../../claude-proxy.js"))?;

    // chat-proxy.js
    write_if_changed(&dest.join("chat-proxy.js"), include_str!("../../chat-proxy.js"))?;

    // status-line.sh
    write_if_changed(&dest.join("status-line.sh"), include_str!("../../scripts/status-line.sh"))?;

    // Install mimo2codex synchronously if missing.
    // Must block so start_enabled() doesn't race and fail to launch it.
    let mimo_bin = dest.join("mimo2codex");
    #[cfg(windows)] let mimo_bin = mimo_bin.with_extension("cmd");
    if !mimo_bin.exists() {
        tracing::info!("Installing mimo2codex via npm (blocking)...");
        let mut c = std::process::Command::new("npm");
        c.args(["install", "-g", "mimo2codex"]);
        #[cfg(windows)] {
            use std::os::windows::process::CommandExt as _;
            c.creation_flags(0x0800_0000);
        }
        match c.output() {
            Ok(o) if o.status.success() => tracing::info!("mimo2codex installed"),
            Ok(o) => tracing::warn!("mimo2codex install failed: {}", String::from_utf8_lossy(&o.stderr)),
            Err(e) => tracing::warn!("mimo2codex install error: {e}"),
        }
    }

    Ok(())
}

/// Run the platform installer script to ensure Node.js + npm + mimo2codex are available.
/// Called once on startup. Non-blocking — fires and forgets.
pub async fn ensure_environment() {
    #[cfg(target_os = "windows")]
    let (script, label) = (
        include_str!("../../scripts/setup-windows.ps1"),
        "setup-windows.ps1",
    );
    #[cfg(target_os = "macos")]
    let (script, label) = (
        include_str!("../../scripts/setup-mac.sh"),
        "setup-mac.sh",
    );
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    return;

    let dir = paths::mimo2codex_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join(label);
    // Write script to disk so user can inspect it
    if let Err(e) = fs::write(&path, script) {
        tracing::warn!("Failed to write {}: {e}", label);
        return;
    }

    let result = {
        #[cfg(target_os = "windows")]
        {
            // Run via powershell
            let mut cmd = tokio::process::Command::new("powershell");
            cmd.args(["-ExecutionPolicy", "Bypass", "-File"]);
            cmd.arg(path.to_string_lossy().to_string());
            cmd.kill_on_drop(true);
            crate::win_console::hide_console_async(&mut cmd);
            cmd.output().await
        }
        #[cfg(target_os = "macos")]
        {
            let mut cmd = tokio::process::Command::new("bash");
            cmd.arg(path.to_string_lossy().to_string());
            cmd.kill_on_drop(true);
            cmd.output().await
        }
    };

    match result {
        Ok(o) if o.status.success() => {
            tracing::info!("Environment setup OK");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            tracing::warn!("Environment setup exited non-zero: {stderr}");
        }
        Err(e) => {
            tracing::warn!("Environment setup failed: {e}");
        }
    }
}

// ── Shell aliases ────────────────────────────────────────────

const CCGATE_BEGIN: &str = "# >>> CC-Gate aliases >>>";
const CCGATE_END:   &str = "# <<< CC-Gate aliases <<<";
const CCGATE_BEGIN_PS: &str = "# >>> CC-Gate functions >>>";
const CCGATE_END_PS:   &str = "# <<< CC-Gate functions <<<";

pub fn write_shell_aliases(cfg: &AppConfig) -> Result<()> {
    let bash = generate_bash_aliases(cfg);
    let ps   = generate_powershell_functions(cfg);

    for path in paths::shell_configs() {
        let is_ps = path.to_string_lossy().ends_with(".ps1");
        let (content, begin, end) = if is_ps {
            (&ps, CCGATE_BEGIN_PS, CCGATE_END_PS)
        } else {
            (&bash, CCGATE_BEGIN, CCGATE_END)
        };

        let existing = if path.exists() { fs::read_to_string(&path).unwrap_or_default() } else { String::new() };
        let new_content = if existing.contains(begin) {
            let before = &existing[..existing.find(begin).unwrap()].trim_end();
            let after_start = existing.find(end).unwrap() + end.len();
            let after = existing[after_start..].trim_start();
            format!("{}\n{}\n{}", before, content.trim_end(), after).trim_end().to_string() + "\n"
        } else {
            let t = existing.trim_end();
            if t.is_empty() { content.to_string() } else { format!("{}\n\n{}", t, content) }
        };

        let mut f = fs::File::create(&path)?;
        f.write_all(new_content.as_bytes())?;
        tracing::info!("Shell aliases written to {}", path.display());
    }
    Ok(())
}

fn generate_bash_aliases(cfg: &AppConfig) -> String {
    let mut out = String::from(CCGATE_BEGIN) + "\n";
    gen_aliases_impl(cfg, &mut out, false);
    out.push_str(CCGATE_END); out.push('\n');
    out
}

fn generate_powershell_functions(cfg: &AppConfig) -> String {
    let mut out = String::from(CCGATE_BEGIN_PS) + "\n";
    gen_aliases_impl(cfg, &mut out, true);
    out.push_str(CCGATE_END_PS); out.push('\n');
    out
}

fn gen_aliases_impl(cfg: &AppConfig, out: &mut String, powershell: bool) {
    // ── Codex ──────────────────────────────────────────────
    let codex_slugs = cfg.agent_models.get("codex_cli").cloned().unwrap_or_default();
    if !codex_slugs.is_empty() {
        let port = cfg.proxy_ports.mimo2codex;
        // Native alias: "codex" — official OpenAI (requires `codex login`), no CC-Gate env
        if powershell {
            out.push_str("function codex { & (Get-Command codex -CommandType Application) --dangerously-bypass-approvals-and-sandbox -c model_provider=\"openai\" -c model=\"gpt-5.5\" $args }\n");
        } else {
            out.push_str("alias codex='\\codex --dangerously-bypass-approvals-and-sandbox -c model_provider=\"openai\" -c model=\"gpt-5.5\"'\n");
        }
        // Per-model aliases: codex-{short}
        for slug in &codex_slugs {
            if let Some(m) = cfg.models.iter().find(|m| &m.slug == slug) {
                let aname = codex_alias(slug);
                out.push_str(&codex_alias_line(&aname, m, port, powershell, cfg));
            }
        }
    }

    // ── Claude CLI ──────────────────────────────────────────
    let claude_slugs = cfg.agent_models.get("claude_cli").cloned().unwrap_or_default();
    if !claude_slugs.is_empty() {
        // Native alias: "claude" — official Anthropic (user's own login/key), no CC-Gate env
        if powershell {
            out.push_str("function claude { $env:ANTHROPIC_BASE_URL='https://api.anthropic.com'; & (Get-Command claude -CommandType Application) --dangerously-skip-permissions --permission-mode bypassPermissions $args }\n");
        } else {
            out.push_str("alias claude='ANTHROPIC_BASE_URL=\"https://api.anthropic.com\" \\claude --dangerously-skip-permissions --permission-mode bypassPermissions'\n");
        }
        // Per-model aliases: claude-{short}
        for slug in &claude_slugs {
            let aname = claude_alias(slug);
            let cm = format!("claude-{}", slug);
            let port = cfg.proxy_ports.claude_proxy;
            if powershell {
                out.push_str(&format!(
                    "function {} {{ $env:CC_GATE_MODEL='{slug}'; $env:ANTHROPIC_BASE_URL='http://127.0.0.1:{port}'; $env:ANTHROPIC_AUTH_TOKEN='proxy'; $env:ANTHROPIC_MODEL='{cm}'; $env:ANTHROPIC_DEFAULT_OPUS_MODEL='{cm}'; $env:ANTHROPIC_DEFAULT_SONNET_MODEL='{cm}'; $env:ANTHROPIC_DEFAULT_HAIKU_MODEL='{cm}'; $env:ANTHROPIC_DEFAULT_FABLE_MODEL='{cm}'; $env:CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY='1'; claude --dangerously-skip-permissions --permission-mode bypassPermissions }}\n",
                    aname, slug=slug, port=port, cm=cm,
                ));
            } else {
                out.push_str(&format!(
                    "alias {aname}='CC_GATE_MODEL=\"{slug}\" \\\n  ANTHROPIC_BASE_URL=\"http://127.0.0.1:{port}\" \\\n  ANTHROPIC_AUTH_TOKEN=proxy \\\n  ANTHROPIC_MODEL=\"{cm}\" \\\n  ANTHROPIC_DEFAULT_OPUS_MODEL=\"{cm}\" \\\n  ANTHROPIC_DEFAULT_SONNET_MODEL=\"{cm}\" \\\n  ANTHROPIC_DEFAULT_HAIKU_MODEL=\"{cm}\" \\\n  ANTHROPIC_DEFAULT_FABLE_MODEL=\"{cm}\" \\\n  CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1 \\\n  \\claude --dangerously-skip-permissions --permission-mode bypassPermissions'\n",
                    aname=aname, slug=slug, port=port, cm=cm,
                ));
            }
        }
    }

    // ── Aider ──────────────────────────────────────────────
    let aider_slugs = cfg.agent_models.get("aider").cloned().unwrap_or_default();
    if !aider_slugs.is_empty() {
        // Native alias: "aider" — official default (user's own OpenAI key), no CC-Gate env
        if powershell {
            out.push_str("function aider { & (Get-Command aider -CommandType Application) $args }\n");
        } else {
            out.push_str("alias aider='\\aider'\n");
        }
        // Per-model: aider-{short}
        for slug in &aider_slugs {
            let aname = aider_alias(slug);
            let port = cfg.proxy_ports.chat_proxy;
            if powershell {
                out.push_str(&format!(
                    "function {} {{ $env:CC_GATE_MODEL='{}'; $env:OPENAI_API_BASE='http://127.0.0.1:{port}/v1'; $env:OPENAI_API_KEY='proxy'; aider --model openai/{} }}\n",
                    aname, slug, slug
                ));
            } else {
                out.push_str(&format!(
                    "alias {}='CC_GATE_MODEL=\"{}\" OPENAI_API_BASE=http://127.0.0.1:{}/v1 OPENAI_API_KEY=proxy \\aider --model openai/{}'\n",
                    aname, slug, port, slug
                ));
            }
        }
    }
}

pub fn remove_shell_aliases() -> Result<()> {
    let begins = [CCGATE_BEGIN, CCGATE_BEGIN_PS];
    let ends   = [CCGATE_END,   CCGATE_END_PS];

    for path in paths::shell_configs() {
        if !path.exists() { continue; }
        let existing = fs::read_to_string(&path).unwrap_or_default();

        for (&begin, &end) in begins.iter().zip(ends.iter()) {
            if let (Some(b), Some(e)) = (existing.find(begin), existing.find(end)) {
                let content = format!("{}\n{}\n",
                    existing[..b].trim_end(),
                    existing[e + end.len()..].trim_start()
                ).trim_end().to_string() + "\n";
                fs::write(&path, &content)?;
                break;
            }
        }
    }
    Ok(())
}

// ── User API keys → .env ────────────────────────────────────

pub fn write_user_api_keys(cfg: &AppConfig) -> Result<()> {
    let env_path = paths::mimo_env();
    let existing = if env_path.exists() { fs::read_to_string(&env_path).unwrap_or_default() } else { String::new() };

    // Collect env var names that this function manages (user-entered keys + relay keys)
    let managed_keys: HashSet<&str> = cfg.api_keys.keys().map(|s| s.as_str()).collect();
    let _relay_keys: HashSet<String> = cfg.relays.iter()
        .map(|r| relay_env_key(&r.name))
        .collect();

    // Keep lines whose env var is NOT in managed_keys and NOT a RELAY_ key
    let other_env_keys: HashSet<&str> = ["DS_API_KEY", "GLM_API_KEY", "QWEN_API_KEY",
        "QWEN38_API_KEY", "MIMO_API_KEY", "DEEPSEEK_API_KEY"].iter().copied().collect();

    let mut preserved: Vec<String> = Vec::new();
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            preserved.push(line.to_string());
            continue;
        }
        if let Some((key, _)) = trimmed.split_once('=') {
            let key = key.trim();
            // Skip lines whose key is in managed_keys or is a RELAY_ key
            if managed_keys.contains(key) { continue; }
            if key.starts_with("RELAY_") && key.ends_with("_API_KEY") { continue; }
            if other_env_keys.contains(key) { continue; }
        }
        preserved.push(line.to_string());
    }

    // Append user-entered API keys
    for (key, val) in &cfg.api_keys {
        if !val.is_empty() {
            preserved.push(format!("{key}={val}"));
        }
    }

    // Append relay keys
    for relay in &cfg.relays {
        let env_key = relay_env_key(&relay.name);
        preserved.push(format!("{env_key}={}", relay.key));
    }

    let content = preserved.join("\n").trim_end().to_string() + "\n";
    write_if_changed(&env_path, &content)
}

// ── All-in-one ───────────────────────────────────────────────

pub fn write_all_tool_configs(cfg: &AppConfig) -> Result<()> {
    // Back up original configs before first modification (idempotent)
    crate::backup::ensure_all_backups();

    // Deploy proxy scripts to ~/.mimo2codex/ so CC-Gate can launch them
    deploy_proxy_scripts()?;

    write_codex_config(cfg)?;
    write_model_catalog(cfg)?;
    write_claude_settings(cfg)?;
    write_providers(cfg)?;
    write_user_api_keys(cfg)?;
    write_shell_aliases(cfg)?;
    clean_stale_bat_files();

/// Delete old .bat / .cmd launcher files that may shadow CC-Gate's shell aliases.
/// These are typically created by earlier manual setups and can cause
/// "claude-ds → hermes" or "codex-ds" misrouting.
fn clean_stale_bat_files() {
    let home = std::env::var("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    if home.as_os_str().is_empty() { return; }
    let candidates: &[&str] = &[
        "claude.bat", "claude.cmd",
        "codex.bat", "codex.cmd",
        "claude-ds.bat", "claude-ds.cmd", "claude-mimo.bat", "claude-mimo.cmd",
        "claude-glm.bat", "claude-glm.cmd", "claude-qwen.bat", "claude-qwen.cmd",
        "codex-ds.bat", "codex-ds.cmd", "codex-ds-flash.bat", "codex-ds-flash.cmd",
        "codex-mimo.bat", "codex-mimo.cmd",
        "codex-glm.bat", "codex-glm.cmd", "codex-qwen.bat", "codex-qwen.cmd",
    ];
    for name in candidates {
        let p = home.join(name);
        if p.exists() {
            let _ = std::fs::remove_file(&p);
            tracing::info!("Removed stale launcher: {}", p.display());
        }
    }
}
    write_hermes_config(cfg)?;
    write_openclaw_config(cfg)?;
    write_opencode_config(cfg)?;
    tracing::info!("All tool configs written");
    Ok(())
}

// ── Hermes config.yaml ─────────────────────────────────────

pub fn write_hermes_config(cfg: &AppConfig) -> Result<()> {
    let slugs: Vec<String> = cfg.agent_models
        .get("hermes").cloned().unwrap_or_default();
    if slugs.is_empty() { return Ok(()); }

    let path = paths::hermes_config_yaml();
    let src = if path.exists() { fs::read_to_string(&path).unwrap_or_default() } else { String::new() };

    // Parse as generic YAML value (preserve non-CC-Gate keys)
    let mut doc: serde_yaml::Value = if src.trim().is_empty() {
        serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
    } else {
        serde_yaml::from_str(&src).unwrap_or_else(|_| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
    };

    let port = cfg.proxy_ports.chat_proxy;
    let base_url = format!("http://127.0.0.1:{port}/v1");
    let default_model = slugs.first().cloned().unwrap();

    // Build CC-Gate provider entry
    let mut models_map = serde_yaml::Mapping::new();
    for slug in &slugs {
        if let Some(m) = cfg.models.iter().find(|d| &d.slug == slug) {
            let mut mm = serde_yaml::Mapping::new();
            mm.insert("context_length".into(), serde_yaml::Value::Number((m.context_window as i64).into()));
            mm.insert("name".into(), serde_yaml::Value::String(format!("{} (CC-Gate)", m.display_name)));
            models_map.insert(serde_yaml::Value::String(slug.clone()), serde_yaml::Value::Mapping(mm));
        }
    }

    let mut provider = serde_yaml::Mapping::new();
    provider.insert("name".into(), "ccgate".into());
    provider.insert("base_url".into(), base_url.into());
    provider.insert("api_key".into(), "proxy".into());
    provider.insert("api_mode".into(), "chat_completions".into());
    provider.insert("models".into(), serde_yaml::Value::Mapping(models_map));
    provider.insert("model".into(), default_model.into());

    // Filter existing custom_providers to keep non-CC-Gate ones
    let mut new_providers: Vec<serde_yaml::Value> = Vec::new();
    if let serde_yaml::Value::Mapping(ref map) = doc {
        if let Some(serde_yaml::Value::Sequence(existing)) = map.get("custom_providers") {
            for entry in existing {
                if let Some(name) = entry.get("name").and_then(|v| v.as_str()) {
                    if name == "ccgate" { continue; } // remove old CC-Gate entry
                }
                new_providers.push(entry.clone());
            }
        }
    }
    new_providers.push(serde_yaml::Value::Mapping(provider));

    if let serde_yaml::Value::Mapping(ref mut map) = doc {
        map.insert("custom_providers".into(), serde_yaml::Value::Sequence(new_providers));
    }

    let out = serde_yaml::to_string(&doc)?;
    write_if_changed(&path, &out)
}

// ── OpenClaw openclaw.json ──────────────────────────────────

pub fn write_openclaw_config(cfg: &AppConfig) -> Result<()> {
    let slugs: Vec<String> = cfg.agent_models
        .get("openclaw").cloned().unwrap_or_default();
    if slugs.is_empty() { return Ok(()); }

    let path = paths::openclaw_config_json();
    let src = if path.exists() { fs::read_to_string(&path).unwrap_or_default() } else { String::new() };

    // Parse existing config as JSON5-lenient: try serde_json first
    let doc: serde_json::Value = if src.trim().is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        crate::backup::parse_jsonc_lenient(&src)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
    };

    let port = cfg.proxy_ports.chat_proxy;
    let default_model = slugs.first().cloned().unwrap();

    let models_json: Vec<serde_json::Value> = slugs.iter()
        .filter_map(|slug| cfg.models.iter().find(|d| &d.slug == slug))
        .map(|m| serde_json::json!({
            "id": m.slug,
            "name": m.display_name,
            "reasoning": false,
            "input": ["text"],
            "cost": {"input": m.input_price_per_1k / 1000.0, "output": m.output_price_per_1k / 1000.0, "cacheRead": 0, "cacheWrite": 0},
            "contextWindow": m.context_window,
            "maxTokens": m.max_output_tokens,
        }))
        .collect();

    // Merge into existing config
    let mut map = if let serde_json::Value::Object(m) = doc { m } else { serde_json::Map::new() };

    // agents.defaults.model.primary
    let primary = serde_json::json!({"primary": format!("ccgate/{default_model}")});
    if let serde_json::Value::Object(ref mut agents) = map.entry("agents".to_string()).or_insert(serde_json::json!({})) {
        if let serde_json::Value::Object(ref mut defaults) = agents.entry("defaults".to_string()).or_insert(serde_json::json!({})) {
            defaults.insert("model".to_string(), primary);
        }
    }

    // models.providers.ccgate
    let ccgate_provider = serde_json::json!({
        "baseUrl": format!("http://127.0.0.1:{port}/v1"),
        "apiKey": "proxy",
        "api": "openai-completions",
        "models": models_json,
    });
    if let serde_json::Value::Object(ref mut models) = map.entry("models".to_string()).or_insert(serde_json::json!({})) {
        if let serde_json::Value::Object(ref mut providers) = models.entry("providers".to_string()).or_insert(serde_json::json!({})) {
            providers.insert("ccgate".to_string(), ccgate_provider);
        }
    }

    let out = serde_json::to_string_pretty(&map)? + "\n";
    write_if_changed(&path, &out)
}

/// Write opencode config (~/.config/opencode/opencode.jsonc): merge a `ccgate`
/// provider (chat-proxy port) into the existing JSONC doc and point the default
/// model at it. Preserves any other providers (e.g. built-in zhipuai).
pub fn write_opencode_config(cfg: &AppConfig) -> Result<()> {
    let slugs: Vec<String> = cfg.agent_models
        .get("opencode").cloned().unwrap_or_default();
    if slugs.is_empty() { return Ok(()); }

    let path = paths::opencode_config_path();
    let src = if path.exists() { fs::read_to_string(&path).unwrap_or_default() } else { String::new() };

    let doc: serde_json::Value = if src.trim().is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        crate::backup::parse_jsonc_lenient(&src)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
    };

    let port = cfg.proxy_ports.chat_proxy;
    let default_model = slugs.first().cloned().unwrap();

    // provider.ccgate = { npm, name, options{baseURL, apiKey}, models{slug:{name}} }
    let mut models_map = serde_json::Map::new();
    for slug in &slugs {
        let name = cfg.models.iter()
            .find(|d| &d.slug == slug)
            .map(|m| m.display_name.clone())
            .unwrap_or_else(|| slug.clone());
        models_map.insert(slug.clone(), serde_json::json!({ "name": name }));
    }
    let ccgate_provider = serde_json::json!({
        "npm": "@ai-sdk/openai-compatible",
        "name": "CC-Gate",
        "options": {
            "baseURL": format!("http://127.0.0.1:{port}/v1"),
            "apiKey": "cc-gate-local",
        },
        "models": models_map,
    });

    let mut map = if let serde_json::Value::Object(m) = doc { m } else { serde_json::Map::new() };
    // provider (opencode uses singular "provider")
    if let serde_json::Value::Object(ref mut providers) = map.entry("provider".to_string()).or_insert(serde_json::json!({})) {
        providers.insert("ccgate".to_string(), ccgate_provider);
    }
    // model = "ccgate/<default>"
    map.insert("model".to_string(), serde_json::json!(format!("ccgate/{default_model}")));

    let out = serde_json::to_string_pretty(&map)? + "\n";
    write_if_changed(&path, &out)
}

// ── Helpers ─────────────────────────────────────────────────

fn write_if_changed(path: &std::path::Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let current = if path.exists() { fs::read_to_string(path).unwrap_or_default() } else { String::new() };
    if current == content { tracing::info!("{} unchanged, skip", path.display()); return Ok(()); }
    let mut f = fs::File::create(path)?;
    f.write_all(content.as_bytes())?;
    tracing::info!("{} written", path.display());
    Ok(())
}

fn short(slug: &str) -> &str { match slug {
    "deepseek-v4-pro" => "ds", "deepseek-v4-flash" => "ds-flash",
    "glm-5.2" => "glm", "qwen3.8-max-preview" => "qwen", "qwen-max" => "qwen-max",
    "mimo-v2.5-pro" => "mimo", "mimo-v2.5" => "mimo-v2.5",
    "claude-opus-5" => "opus", "gpt-5.6" => "gpt",
    "gemini-3-flash-preview" => "gemini-flash", "gemini-2.5-pro" => "gemini-pro",
    _ => slug,
}}
fn codex_alias(s: &str) -> String { format!("codex-{}", short(s)) }

/// Build a Codex alias line. When native_responses is set, the alias connects
/// directly to the provider's Responses API (bypassing mimo2codex).
/// Otherwise it routes through the local proxy.
fn codex_alias_line(aname: &str, m: &ModelDef, port: u16, powershell: bool, cfg: &AppConfig) -> String {
    if m.native_responses {
        if let Some(meta) = meta_by_id(&m.provider) {
            let api_key = cfg.api_keys.get(meta.env_key).map(|k| k.as_str()).unwrap_or("proxy");
            if powershell {
                format!(
                    "function {aname} {{ $env:CC_GATE_MODEL='{slug}'; $env:OPENAI_API_KEY='{key}'; codex --dangerously-bypass-approvals-and-sandbox -c model_provider='custom' -c model='{slug}' -c base_url='{url}' -c model_context_window={ctx} -c model_max_output_tokens={max} }}\n",
                    aname=aname, slug=m.slug, key=api_key, url=meta.base_url, ctx=m.context_window, max=m.max_output_tokens
                )
            } else {
                format!(
                    "alias {aname}='CC_GATE_MODEL=\"{slug}\" OPENAI_API_KEY={key} \\codex --dangerously-bypass-approvals-and-sandbox -c model_provider=\"custom\" -c model=\"{slug}\" -c base_url=\"{url}\" -c model_context_window={ctx} -c model_max_output_tokens={max}'\n",
                    aname=aname, slug=m.slug, key=api_key, url=meta.base_url, ctx=m.context_window, max=m.max_output_tokens
                )
            }
        } else {
            codex_alias_line_proxy(aname, m, port, powershell)
        }
    } else {
        codex_alias_line_proxy(aname, m, port, powershell)
    }
}

fn codex_alias_line_proxy(aname: &str, m: &ModelDef, port: u16, powershell: bool) -> String {
    if powershell {
        format!(
            "function {aname} {{ $env:CC_GATE_MODEL='{slug}'; $env:OPENAI_API_KEY='proxy'; codex --dangerously-bypass-approvals-and-sandbox -c model_provider='custom' -c model='{slug}' -c base_url='http://127.0.0.1:{port}/v1' -c model_context_window={ctx} -c model_max_output_tokens={max} -c requires_openai_auth='false' }}\n",
            aname=aname, slug=m.slug, port=port, ctx=m.context_window, max=m.max_output_tokens
        )
    } else {
        format!(
            "alias {aname}='CC_GATE_MODEL=\"{slug}\" OPENAI_API_KEY=proxy \\codex --dangerously-bypass-approvals-and-sandbox -c model_provider=\"custom\" -c model=\"{slug}\" -c base_url=\"http://127.0.0.1:{port}/v1\" -c model_context_window={ctx} -c model_max_output_tokens={max} -c requires_openai_auth=\"false\"'\n",
            aname=aname, slug=m.slug, port=port, ctx=m.context_window, max=m.max_output_tokens
        )
    }
}
fn claude_alias(s: &str) -> String { format!("claude-{}", short(s)) }
fn aider_alias(s: &str) -> String { format!("aider-{}", short(s)) }

#[cfg(test)]
mod tests {
    use super::relay_env_key;
    use super::gen_aliases_impl;

    #[test]
    fn ascii_names_keep_their_shape() {
        assert_eq!(relay_env_key("NL"), "RELAY_NL_API_KEY");
        assert_eq!(relay_env_key("my relay"), "RELAY_MY_RELAY_API_KEY");
        assert_eq!(relay_env_key("open-router"), "RELAY_OPEN_ROUTER_API_KEY");
    }

    #[test]
    fn non_ascii_names_stay_distinct() {
        // Previously sanitize_provider_id() stripped these to "", collapsing every
        // CJK-named relay onto RELAY__API_KEY so they overwrote each other's key.
        let a = relay_env_key("中转站A");
        let b = relay_env_key("中转站B");
        assert_ne!(a, b, "distinct names must not collide");
        for k in [&a, &b] {
            assert!(k.starts_with("RELAY_") && k.ends_with("_API_KEY"));
            assert!(k.is_ascii(), "{k} must be ASCII — the proxy reads it from .env");
        }
    }

    #[test]
    fn mixed_and_degenerate_names() {
        assert_eq!(relay_env_key("中转站-1"), "RELAY_X4E2DX8F6CX7AD9_1_API_KEY");
        // A name with nothing usable must still yield a valid, stable var name.
        assert_eq!(relay_env_key("-"), "RELAY_UNNAMED_API_KEY");
        assert_eq!(relay_env_key(""), "RELAY_UNNAMED_API_KEY");
    }

    #[test]
    fn bare_aliases_are_native_suffixed_stay_proxy() {
        use crate::types::AppConfig;
        let mut cfg = AppConfig::default();
        // deepseek-v4-pro has native_responses=true → codex-ds connects directly.
        // glm-5.2 has native_responses=false → codex-glm still routes via mimo2codex :8688.
        cfg.agent_models.insert("codex_cli".into(), vec!["deepseek-v4-pro".into(), "glm-5.2".into()]);
        cfg.agent_models.insert("claude_cli".into(), vec!["claude-opus-5".into(), "deepseek-v4-pro".into()]);
        cfg.agent_models.insert("aider".into(), vec!["deepseek-v4-pro".into()]);
        let mut out = String::new();
        gen_aliases_impl(&cfg, &mut out, false);

        // Bare (no suffix) aliases must be official/native — no CC-Gate env, no proxy port.
        assert!(out.contains("alias codex='\\codex"), "bare codex must be native, got:\n{out}");
        let bare_codex = out.lines().find(|l| l.starts_with("alias codex='")).unwrap();
        assert!(!bare_codex.contains("model_provider=\"custom\""),
            "bare codex must not route via custom provider: {bare_codex}");
        assert!(out.contains("alias claude='ANTHROPIC_BASE_URL=\"https://api.anthropic.com\""),
            "bare claude must point at official Anthropic:\n{out}");
        assert!(!out.contains("alias claude='CC_GATE_MODEL="),
            "bare claude must not inject CC_GATE_MODEL:\n{out}");
        assert!(out.contains("alias aider='\\aider'"), "bare aider must be native:\n{out}");
        assert!(!out.contains("alias aider='CC_GATE_MODEL="),
            "bare aider must not inject CC_GATE_MODEL:\n{out}");

        // native_responses codex model → direct to provider Responses API, not via :8688.
        let codex_ds = out.lines().find(|l| l.starts_with("alias codex-ds='")).unwrap();
        assert!(codex_ds.contains("base_url=\"https://api.deepseek.com/v1\""),
            "codex-ds must connect directly to DeepSeek:\n{codex_ds}");
        assert!(!codex_ds.contains("127.0.0.1:8688"),
            "codex-ds must NOT route via mimo2codex :8688:\n{codex_ds}");

        // Non-native codex model → still routed through mimo2codex :8688.
        let codex_glm = out.lines().find(|l| l.starts_with("alias codex-glm='")).unwrap();
        assert!(codex_glm.contains("base_url=\"http://127.0.0.1:8688/v1\""),
            "codex-glm must stay proxied via :8688:\n{codex_glm}");
        assert!(codex_glm.contains("requires_openai_auth=\"false\""),
            "codex-glm proxy alias must be zero-auth:\n{codex_glm}");

        // Claude/aider suffixed aliases still route through their own proxies (unaffected by native_responses).
        assert!(out.contains("alias claude-ds='CC_GATE_MODEL="), "claude-ds must stay proxied:\n{out}");
        assert!(out.contains("alias aider-ds='CC_GATE_MODEL="), "aider-ds must stay proxied:\n{out}");

        // Helpful when eyeballing generated output.
        println!("\n=== generated aliases ===\n{out}");
    }

    #[test]
    fn gemini_provider_meta_models_and_aliases() {
        // 1. PROVIDER_META 直连定义
        let meta = super::meta_by_id("gemini").expect("gemini must have direct provider meta");
        assert_eq!(meta.base_url, "https://generativelanguage.googleapis.com/v1beta/openai",
            "代理拼接 baseUrl + /chat/completions 必须命中 Gemini OpenAI 兼容端点,不能带 /v1");
        assert_eq!(meta.env_key, "GEMINI_API_KEY");
        assert!(meta.env_key_aliases.contains(&"GOOGLE_API_KEY"),
            "GOOGLE_API_KEY must remain a valid alias");

        // 2. 内置模型:两个 gemini 模型,均走代理(无 Responses API)
        let models = crate::types::builtin_models();
        let g3 = models.iter().find(|m| m.slug == "gemini-3-flash-preview")
            .expect("gemini-3-flash-preview must be builtin");
        assert_eq!(g3.provider, "gemini");
        assert!(!g3.native_responses, "gemini has no Responses API — must route via proxy");
        let g25 = models.iter().find(|m| m.slug == "gemini-2.5-pro")
            .expect("gemini-2.5-pro must be builtin");
        assert_eq!(g25.provider, "gemini");
        assert!(!g25.native_responses, "gemini has no Responses API — must route via proxy");

        // 3. 别名后缀映射(决定 codex-gemini-flash / claude-gemini-pro 等别名)
        assert_eq!(super::short("gemini-3-flash-preview"), "gemini-flash");
        assert_eq!(super::short("gemini-2.5-pro"), "gemini-pro");
        assert_eq!(super::codex_alias("gemini-3-flash-preview"), "codex-gemini-flash");
        assert_eq!(super::codex_alias("gemini-2.5-pro"), "codex-gemini-pro");

        // 4. 拼接出的上游端点必须与实测存在的 Gemini OpenAI 兼容端点一致
        assert_eq!(
            format!("{}/chat/completions", meta.base_url),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
    }

    /// Manual "Apply" without the GUI: rewrites providers.json + shell aliases from
    /// the real ~/.CC-Gate/config.json. Gated — only runs when CCGATE_MANUAL_APPLY=1.
    #[test]
    fn manual_apply_when_requested() {
        if std::env::var("CCGATE_MANUAL_APPLY").is_err() { return; }
        let cfg = crate::config_store::load().expect("load ~/.CC-Gate/config.json");
        super::write_providers(&cfg).expect("write providers.json");
        super::write_shell_aliases(&cfg).expect("write shell aliases");
        eprintln!("MANUAL-APPLY-OK");
    }

    #[test]
    fn preserve_user_sections_keeps_projects_and_mcp() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("ccgate-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        // Existing config with user-managed sections that must survive a rewrite.
        write!(f, "model = \"old\"\n\n[projects.\"/Users/ami/code\"]\ntrust_level = \"trusted\"\n\n[mcp_servers.node_repl]\ntype = \"stdio\"\ncommand = \"/x/node_repl\"\n\n[mcp_servers.node_repl.env]\nA = \"1\"\n").unwrap();
        drop(f);

        let generated = "model_provider = \"custom\"\nmodel = \"new\"\n[model_providers.custom]\nname = \"CC-Gate\"\n";
        let merged = super::preserve_user_sections(&path, generated, &["projects", "mcp_servers"]);

        assert!(merged.contains("[projects.\"/Users/ami/code\"]"), "projects must survive:\n{merged}");
        assert!(merged.contains("trust_level = \"trusted\""));
        assert!(merged.contains("[mcp_servers.node_repl]"), "mcp_servers must survive:\n{merged}");
        assert!(merged.contains("command = \"/x/node_repl\""));
        assert!(merged.contains("[mcp_servers.node_repl.env]"), "nested mcp env table must survive:\n{merged}");
        assert!(merged.contains("A = \"1\""));
        assert!(merged.contains("model_provider = \"custom\""), "generated core must stay:\n{merged}");
        // Parse round-trip must stay valid TOML.
        let doc = merged.parse::<toml::Table>().expect("merged output must be valid TOML");
        assert!(doc.contains_key("projects") && doc.contains_key("mcp_servers"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn jsonc_lenient_parses_comments_and_trailing_commas() {
        // Realistic JSONC: // comments + trailing commas at line ends (not inline).
        let src = "{\n  // opencode config\n  \"model\": \"ccgate/x\",\n  \"provider\": {\n    \"ccgate\": {\n      \"name\": \"CC-Gate\"\n    },\n  },\n}\n";
        let v = crate::backup::parse_jsonc_lenient(src).expect("JSONC must parse");
        assert_eq!(v.pointer("/provider/ccgate/name").and_then(|x| x.as_str()), Some("CC-Gate"));
        // Detection path: /provider/ccgate must be found for opencode.
        assert!(v.pointer("/provider/ccgate").is_some());
    }

    /// Relay base URLs that already carry the full OpenAI endpoint path must be
    /// normalized (trailing /chat/completions stripped) so the local proxies'
    /// own suffix append can't produce a doubled path → upstream 404.
    #[test]
    fn normalize_relay_base_url_strips_full_endpoint_path() {
        use super::normalize_relay_base_url;
        assert_eq!(
            normalize_relay_base_url("https://token.sensenova.cn/v1/chat/completions"),
            "https://token.sensenova.cn/v1"
        );
        assert_eq!(
            normalize_relay_base_url("https://token.sensenova.cn/v1/chat/completions/"),
            "https://token.sensenova.cn/v1"
        );
        // Base URL without the endpoint suffix must be left untouched.
        assert_eq!(
            normalize_relay_base_url("https://openrouter.ai/api/v1"),
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(normalize_relay_base_url("https://api.deepseek.com/v1/"), "https://api.deepseek.com/v1");
    }

    /// Full headless "Apply" regression: write ALL tool configs, then every agent
    /// must report proxied (including OpenClaw and OpenCode, the previously
    /// false-negative pair). Gated — only runs when CCGATE_FULL_TEST=1.
    #[test]
    fn full_apply_all_agents_proxied() {
        if std::env::var("CCGATE_FULL_TEST").is_err() { return; }
        let cfg = crate::config_store::load().expect("load ~/.CC-Gate/config.json");
        super::write_all_tool_configs(&cfg).expect("write all tool configs");
        let agents = crate::types::agent_list();
        let status: Vec<String> = agents.iter()
            .map(|a| format!("{}={}", a.name, crate::backup::is_agent_proxied(a)))
            .collect();
        eprintln!("APPLIED: {}", status.join(" | "));
        for a in &agents {
            assert!(crate::backup::is_agent_proxied(a), "agent {} must be proxied after apply", a.name);
        }
        eprintln!("FULL-APPLY-ALL-PROXIED-OK");
    }
}

