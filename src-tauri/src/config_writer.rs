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
    ProviderMeta { id: "moonshot",  name: "月之暗面Kimi",   base_url: "https://api.moonshot.ai/v1",                                                     env_key: "MOONSHOT_API_KEY", env_key_aliases: &["KIMI_API_KEY"],   feature: None },
    // LongCat 官方 OpenAI 兼容端点: https://api.longcat.chat/openai/v1/chat/completions
    ProviderMeta { id: "longcat",   name: "美团LongCat",    base_url: "https://api.longcat.chat/openai/v1",                                             env_key: "LONGCAT_API_KEY",  env_key_aliases: &[],                 feature: None },
];

fn meta_by_id(id: &str) -> Option<&'static ProviderMeta> {
    PROVIDER_META.iter().find(|m| m.id == id)
}

/// Provider ids that have a known direct endpoint (usable for custom models).
pub fn known_provider_ids() -> Vec<&'static str> {
    PROVIDER_META.iter().map(|m| m.id).collect()
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

/// Prefixed display/route form for a discovered relay model:
/// "OpenRouter/anthropic/claude-opus-4". Codex catalog uses it as slug;
/// mimo2codex resolves it back to the raw id via models[].aliases.
fn discovered_route_id(relay: &str, raw: &str) -> String {
    format!("{}/{}", relay, raw)
}

pub fn write_providers(cfg: &AppConfig) -> Result<()> {
    // Collect enabled model slugs from all agents that write_providers
    // Unified-gateway rule: a model is available everywhere iff it's enabled
    // globally (模型管理页) — no more per-agent whitelists.
    let enabled_slugs: BTreeSet<String> = cfg.models.iter()
        .filter(|m| m.enabled)
        .map(|m| m.slug.clone())
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

    // Relay-discovered models: one entry per relay carrying everything its
    // /v1/models returned. Catalog slugs win on collision — a discovered id
    // equal to an existing model keeps the catalog's routing.
    for relay in &cfg.relays {
        // Disabled relay (e.g. subscription lapsed) — its discovered models
        // disappear from every picker until re-enabled.
        if relay.enabled == Some(false) { continue; }
        let discovered: Vec<&crate::types::RelayModelDef> = relay.models.iter()
            .filter(|dm| dm.selected)
            .collect();
        if discovered.is_empty() { continue; }
        entries.push(serde_json::json!({
            "id": sanitize_provider_id(&format!("relayx-{}-discovered", &relay.name)),
            "name": format!("{} 发现的模型", relay.name),
            "baseUrl": normalize_relay_base_url(&relay.url),
            "envKey": relay_env_key(&relay.name),
            "displayPrefix": format!("{}/", relay.name),
            "defaultModel": discovered[0].id.clone(),
            "models": discovered.iter().map(|dm| {
                let base = if dm.display_name.is_empty() { dm.id.as_str() } else { dm.display_name.as_str() };
                let mut mj = serde_json::json!({
                    // RAW upstream id — this is what gets sent to the relay.
                    // The relay rejects prefixed names (400 invalid model ID).
                    "id": dm.id,
                    // "OpenRouter - Claude Opus 4": relay name first so the same
                    // relay's models sort together in the picker.
                    "displayName": format!("{} - {}", relay.name, base),
                    // mimo2codex alias: Codex catalog uses the prefixed form as
                    // its slug; this maps it back to the raw upstream id.
                    "aliases": [discovered_route_id(&relay.name, &dm.id)],
                });
                if let Some(cw) = dm.context_window { mj["contextWindow"] = serde_json::json!(cw); }
                if let Some(mo) = dm.max_output_tokens { mj["maxOutputTokens"] = serde_json::json!(mo); }
                mj
            }).collect::<Vec<_>>(),
        }));
    }

    let content = serde_json::to_string_pretty(&serde_json::json!({ "providers": entries }))?;
    write_if_changed(&paths::providers_json(), &content)
}

// ── Custom aliases (别名页) ──────────────────────────────────

/// Validate a user-supplied alias name.
/// Rules: 2–32 chars, first char a letter, then [A-Za-z0-9_-]; must not collide
/// with any built-in generated alias ("claude"/"codex"/"aider" and their
/// per-model variants) nor another custom alias. `ignore_self` lets update keep
/// its own name.
pub fn validate_alias_name(name: &str, cfg: &AppConfig, ignore_self: Option<&str>) -> std::result::Result<(), String> {
    if name.len() < 2 || name.len() > 32 {
        return Err(format!("别名长度需在 2~32 之间（当前 {}）", name.len()));
    }
    let mut cs = name.chars();
    let first = cs.next().unwrap();
    if !first.is_ascii_alphabetic() {
        return Err("别名必须以字母开头".into());
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("别名只能包含字母、数字、下划线、中划线".into());
    }

    let mut reserved: std::collections::HashSet<String> =
        ["claude", "codex", "aider", "claude-cc-gate", "codex-cc-gate", "aider-cc-gate", "hermes-cc-gate", "pi-cc-gate"]
            .iter().map(|s| s.to_string()).collect();
    for m in crate::types::builtin_models() {
        for prefix in ["claude", "codex", "aider"] {
            reserved.insert(format!("{}-{}", prefix, short(&m.slug)));
        }
    }
    for a in &cfg.custom_aliases {
        if Some(a.name.as_str()) == ignore_self { continue; }
        reserved.insert(a.name.clone());
    }
    if reserved.contains(name) {
        return Err(format!("别名「{name}」与已有命令冲突，换一个"));
    }
    Ok(())
}

/// Resolve one alias into a proxy route entry. Returns None for combos that
/// don't go through the local proxies (claude_cli×anthropic-direct → native
/// line; codex_cli → direct injection), or whose source can't be resolved.
/// Pure so tests can run without touching the filesystem.
fn build_one_route(a: &crate::types::CustomAlias, cfg: &AppConfig) -> Option<serde_json::Value> {
    let model = cfg.models.iter().find(|m| m.slug == a.model)?;
    let relay_by_name = |n: &str| cfg.relays.iter().find(|r| r.name == n);

    // Models usable through this alias's source. The proxies honor any requested
    // model in this set and fall back to the alias's own model otherwise — that's
    // what makes /model switching work inside an alias window (方案B).
    // direct → only models of that provider (other slugs would 404 at the vendor);
    // relay → every enabled model (relays forward anything they carry).
    let enabled: Vec<&ModelDef> = cfg.models.iter().filter(|m| m.enabled).collect();
    let source_models: Vec<String> = if a.source == "direct" {
        enabled.iter().filter(|m| m.provider == model.provider).map(|m| m.slug.clone()).collect()
    } else {
        // Relay: every enabled model + everything discovered from that relay
        // (raw upstream ids — prefixed names would 400 at the relay).
        let relay_name = &a.source[6..];
        let mut v: Vec<String> = enabled.iter().map(|m| m.slug.clone()).collect();
        if let Some(r) = cfg.relays.iter().find(|r| r.name == relay_name) {
            for dm in &r.models {
                v.push(dm.id.clone());
                // prefixed display form (PI/Hermes list it this way)
                v.push(format!("{}/{}", r.name, dm.id));
            }
        }
        v
    };

    let (base_url, env_key, anthropic_native) = match (a.tool.as_str(), a.source.as_str()) {
        // Claude Code and pi both speak Anthropic protocol through claude-proxy.
        // Direct to any provider with a known base URL goes through claude-proxy
        // (translate), except Anthropic itself: claude_cli uses a pure native
        // shell line and pi already supports the official endpoint natively —
        // neither needs a proxy route.
        ("claude_cli" | "pi", "direct") => {
            if model.provider == "anthropic" { return None; }
            let meta = meta_by_id(&model.provider)?;
            (meta.base_url.to_string(), resolve_env_key(meta), false)
        }
        ("claude_cli" | "pi", s) if s.starts_with("relay:") => {
            let relay = relay_by_name(&s[6..])?;
            // If the relay offers a native Anthropic endpoint and the model is
            // Anthropic-flavored, passthrough there; otherwise translate.
            if model.provider == "anthropic" {
                if let Some(au) = &relay.anthropic_url {
                    (normalize_relay_base_url(au), relay_env_key(&relay.name), true)
                } else {
                    (normalize_relay_base_url(&relay.url), relay_env_key(&relay.name), false)
                }
            } else {
                (normalize_relay_base_url(&relay.url), relay_env_key(&relay.name), false)
            }
        }
        // Aider / OpenCode / Hermes speak OpenAI protocol — always proxied so
        // every combo works and usage stays visible (direct providers resolve
        // via PROVIDER_META).
        ("aider" | "opencode" | "hermes", "direct") => {
            let meta = meta_by_id(&model.provider)?;
            (meta.base_url.to_string(), resolve_env_key(meta), false)
        }
        ("aider" | "opencode" | "hermes", s) if s.starts_with("relay:") => {
            let relay = relay_by_name(&s[6..])?;
            (normalize_relay_base_url(&relay.url), relay_env_key(&relay.name), false)
        }
        // Codex CLI aliases inject the upstream directly on the command line —
        // never proxied, so no route entry.
        _ => return None,
    };

    Some(serde_json::json!({
        "name": a.name,
        "tool": a.tool,
        "token": format!("ccgate-{}", a.name),
        "model": a.model,
        // Models the proxies may honor for this token (方案B: /model switching
        // inside an alias window); anything else falls back to `model`.
        "models": source_models,
        "baseUrl": base_url,
        "envKey": env_key,
        "anthropicEndpoint": anthropic_native,
    }))
}

/// Pure builder for ~/.mimo2codex/aliases.json content (testable).
pub fn build_alias_routes(cfg: &AppConfig) -> Vec<serde_json::Value> {
    cfg.custom_aliases.iter()
        .filter_map(|a| build_one_route(a, cfg))
        .collect()
}

/// Write the alias routing table consumed by claude-proxy.js / chat-proxy.js.
pub fn write_alias_routes(cfg: &AppConfig) -> Result<()> {
    let content = serde_json::to_string_pretty(&serde_json::json!({ "aliases": build_alias_routes(cfg) }))?;
    write_if_changed(&paths::aliases_json(), &content)
}

// ── pi coding agent (~/.pi/agent/models.json) ────────────────

/// Merge CC-Gate providers into pi's models.json. Two layers:
/// ① `ccgate` — homepage-assigned pi models via chat-proxy (source = global routing);
/// ② `ccgate-<alias>` — one provider per custom alias, speaking anthropic-messages
///    through claude-proxy with the alias's ccgate token, so a pi window can pin a
///    specific source while /model-switching across that source's models.
/// The file hot-reloads on every /model open; user-defined providers are preserved.
/// A malformed existing file aborts the write instead of wiping it.
/// Pure merger: inject CC-Gate providers into an existing models.json document.
/// Returns Err(reason) when the doc shape is unusable (caller must not overwrite).
fn merge_pi_models(
    mut doc: serde_json::Value,
    cfg: &AppConfig,
) -> std::result::Result<serde_json::Value, String> {
    if !doc.is_object() {
        return Err("顶层必须是 JSON 对象".into());
    }
    let obj = doc.as_object_mut().unwrap();
    if !obj.contains_key("providers") {
        obj.insert("providers".into(), serde_json::json!({}));
    }
    let Some(providers) = obj.get_mut("providers").and_then(|p| p.as_object_mut()) else {
        return Err("providers 必须是对象".into());
    };

    // ① Base provider from the homepage matrix
    // Same-vendor models (direct or via a relay) must sort together — group by
    // provider, then priority, then slug (mirrors providers.json ordering).
    let mut enabled_models: Vec<&ModelDef> = cfg.models.iter().filter(|m| m.enabled).collect();
    enabled_models.sort_by(|a, b| {
        a.provider.cmp(&b.provider)
            .then(a.priority.cmp(&b.priority))
            .then(a.slug.cmp(&b.slug))
    });
    let pi_slugs: Vec<String> = enabled_models.iter().map(|m| m.slug.clone()).collect();
    let mut base_models: Vec<serde_json::Value> = pi_slugs.iter()
        .filter_map(|s| cfg.models.iter().find(|m| &m.slug == s && m.enabled))
        .map(|m| serde_json::json!({
            "id": m.slug,
            "name": m.display_name,
            "reasoning": true,
            "contextWindow": m.context_window,
            "maxTokens": m.max_output_tokens,
        }))
        .collect();
    // Relay-discovered models (picked, enabled relays): same routing via
    // chat-proxy, so PI's list covers them too. Named with the relay prefix.
    for relay in &cfg.relays {
        if relay.enabled == Some(false) { continue; }
        for dm in &relay.models {
            if !dm.selected || pi_slugs.contains(&dm.id)
                || base_models.iter().any(|mj| mj["id"] == serde_json::json!(dm.id)) { continue; }
            let base = if dm.display_name.is_empty() { dm.id.as_str() } else { dm.display_name.as_str() };
            let mut mj = serde_json::json!({
                // Prefixed id: PI shows ids verbatim; chat-proxy resolves the
                // prefix back to the raw upstream name when forwarding.
                "id": format!("{}/{}", relay.name, dm.id),
                "name": format!("{} - {}", relay.name, base),
                "reasoning": true,
            });
            if let Some(cw) = dm.context_window {
                mj["contextWindow"] = serde_json::json!(cw);
            }
            // Never reuse the context window here — relays reject
            // max_tokens above their output cap (SenseNova 65536).
            mj["maxTokens"] = serde_json::json!(dm.max_output_tokens.unwrap_or(16384));
            base_models.push(mj);
        }
    }
    if base_models.is_empty() {
        providers.remove("ccgate");
    } else {
        let port = cfg.proxy_ports.chat_proxy;
        providers.insert("ccgate".into(), serde_json::json!({
            "baseUrl": format!("http://127.0.0.1:{port}/v1"),
            "api": "openai-completions",
            "apiKey": "cc-gate-local",
            "name": "CC-Gate",
            "models": base_models,
        }));
    }

    // ② Per-alias providers (进阶层): token-pinned sources for pi windows
    let claude_port = cfg.proxy_ports.claude_proxy;
    for route in build_alias_routes(cfg) {
        let name = route["name"].as_str().unwrap_or_default().to_string();
        let token = format!("ccgate-{name}");
        let models: Vec<serde_json::Value> = route["models"].as_array()
            .map(|arr| arr.iter().filter_map(|m| m.as_str()).map(|slug| {
                let display = cfg.models.iter().find(|m| m.slug == slug)
                    .map(|m| m.display_name.clone()).unwrap_or_else(|| slug.to_string());
                serde_json::json!({ "id": slug, "name": display, "reasoning": true })
            }).collect())
            .unwrap_or_default();
        if models.is_empty() { continue; }
        providers.insert(format!("ccgate-{name}"), serde_json::json!({
            "baseUrl": format!("http://127.0.0.1:{claude_port}"),
            "api": "anthropic-messages",
            "name": format!("CC-Gate · {name}"),
            "headers": { "x-api-key": token },
            "models": models,
        }));
    }

    Ok(doc)
}

pub fn write_pi_models(cfg: &AppConfig) -> Result<()> {
    let path = paths::pi_models_json();
    let doc: serde_json::Value = if path.exists() {
        let src = fs::read_to_string(&path).unwrap_or_default();
        match serde_json::from_str(&src) {
            Ok(v) => v,
            Err(_) => return Err(crate::error::AppError::Config(format!(
                "{} 解析失败——请先修复或删除该文件再应用", path.display()))),
        }
    } else {
        serde_json::json!({})
    };

    let merged = merge_pi_models(doc, cfg).map_err(crate::error::AppError::Config)?;

    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let content = serde_json::to_string_pretty(&merged)? + "\n";
    write_if_changed(&path, &content)
}

// ── OpenCode alias config files ─────────────────────────────

/// Write one opencode.jsonc-shaped fragment per `opencode` custom alias into
/// `~/.mimo2codex/opencode-aliases/<name>.json`. The alias's shell line points
/// OPENCODE_CONFIG at it; the provider carries the ccgate token so chat-proxy
/// routes per-window (方案B). Directory is wiped first — stale alias configs
/// must never outlive their alias.
pub fn write_opencode_alias_configs(cfg: &AppConfig) -> Result<()> {
    let dir = paths::mimo2codex_dir().join("opencode-aliases");
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    let routes: Vec<serde_json::Value> = build_alias_routes(cfg)
        .into_iter()
        .filter(|r| r["tool"] == "opencode")
        .collect();
    if routes.is_empty() { return Ok(()); }

    fs::create_dir_all(&dir)?;
    for route in routes {
        let name = route["name"].as_str().unwrap_or_default().to_string();
        let token = route["token"].as_str().unwrap_or_default().to_string();
        if name.is_empty() || token.is_empty() { continue; }
        let port = cfg.proxy_ports.chat_proxy;

        // Models this source carries, in opencode's models-map shape.
        let mut models_map = serde_json::Map::new();
        if let Some(arr) = route["models"].as_array() {
            for slug in arr.iter().filter_map(|s| s.as_str()) {
                let display = cfg.models.iter().find(|m| m.slug == slug)
                    .map(|m| m.display_name.clone())
                    .unwrap_or_else(|| slug.to_string());
                models_map.insert(slug.to_string(), serde_json::json!({ "name": display }));
            }
        }

        let doc = serde_json::json!({
            "provider": {
                format!("ccgate-{name}"): {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": format!("CC-Gate · {name}"),
                    "options": {
                        "baseURL": format!("http://127.0.0.1:{port}/v1"),
                        "apiKey": token,
                    },
                    "models": models_map,
                }
            },
            "model": format!("ccgate-{name}/{}", route["model"].as_str().unwrap_or_default()),
        });

        let path = dir.join(format!("{name}.json"));
        fs::write(&path, serde_json::to_string_pretty(&doc)? + "\n")?;
        tracing::info!("opencode alias config written: {}", path.display());
    }
    Ok(())
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
    // Unified-gateway rule: the Codex catalog lists every globally-enabled model.
    let mut models: Vec<serde_json::Value> = cfg.models.iter()
        .filter(|m| m.enabled)
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

    // Relay-discovered models (enabled relay + picked in 挑选): appended so the
    // Codex /model list shows them too. slug = raw relay id — matches the
    // providers.json entry, so requests route to the relay unchanged.
    let known_slugs: BTreeSet<String> = cfg.models.iter().map(|m| m.slug.clone()).collect();
    for relay in &cfg.relays {
        if relay.enabled == Some(false) { continue; }
        for dm in &relay.models {
            if !dm.selected || known_slugs.contains(&dm.id)
                || models.iter().any(|mj| mj["slug"] == serde_json::json!(dm.id)) { continue; }
            models.push(serde_json::json!({
                // Prefixed slug → leads with the relay name in Codex's /model list;
                // resolved to the raw upstream id via providers.json aliases.
                "slug": discovered_route_id(&relay.name, &dm.id),
                "display_name": format!("{} - {}", relay.name,
                    if dm.display_name.is_empty() { dm.id.as_str() } else { dm.display_name.as_str() }),
                "context_window": dm.context_window.unwrap_or(200000),
                "max_context_window": dm.context_window.unwrap_or(200000),
                "effective_context_window_percent": 95,
                "default_reasoning_level": "medium",
                "default_reasoning_summary": "none", "input_modalities": ["text"],
                "supported_reasoning_levels": [
                    {"effort":"none","description":"Disable Thinking"},
                    {"effort":"low","description":"Low"},
                    {"effort":"medium","description":"Medium"},
                    {"effort":"high","description":"High"}
                ],
                "supports_reasoning_summaries": false,
                "supports_parallel_tool_calls": false, "supports_search_tool": false,
                "support_verbosity": false, "supported_in_api": true,
                "shell_type": "shell_command", "apply_patch_tool_type": "freeform",
                "visibility": "list", "priority": 9000,
                "additional_speed_tiers": [], "service_tiers": [],
                "experimental_supported_tools": [],
                "truncation_policy": {"mode":"bytes","limit":10000},
                "base_instructions": format!("You are Codex, a coding agent powered by {} (via {}). You help the user with programming tasks. Read the codebase first, ask questions when needed, and implement solutions directly. Prefer existing patterns and keep changes minimal.", dm.display_name, relay.name),
                "description": format!("{} via CC-Gate relay {}", dm.display_name, relay.name),
                "default_verbosity": "low",
                "supports_image_detail_original": false,
                "upgrade": null
            }));
        }
    }

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
    // Unified gateway commands — ONE per CLI tool. Opening one exposes every
    // usable model (official passthrough + direct vendors + relay-discovered);
    // switching happens inside the tool's own /model picker. This replaces the
    // old per-model alias farms (claude-ds, codex-glm, ...).
    let enabled: Vec<&ModelDef> = cfg.models.iter().filter(|m| m.enabled).collect();
    let cport = cfg.proxy_ports.claude_proxy;
    let mport = cfg.proxy_ports.mimo2codex;
    let hport = cfg.proxy_ports.chat_proxy;

    // Bare native aliases — official direct connections, untouched by CC-Gate.
    if powershell {
        out.push_str("function codex { & (Get-Command codex -CommandType Application) --dangerously-bypass-approvals-and-sandbox -c model_provider=\"openai\" -c model=\"gpt-5.5\" $args }\n");
        out.push_str("function claude { $env:ANTHROPIC_BASE_URL='https://api.anthropic.com'; & (Get-Command claude -CommandType Application) --dangerously-skip-permissions --permission-mode bypassPermissions $args }\n");
        out.push_str("function aider { & (Get-Command aider -CommandType Application) $args }\n");
    } else {
        out.push_str("alias codex='\\codex --dangerously-bypass-approvals-and-sandbox -c model_provider=\"openai\" -c model=\"gpt-5.5\"'\n");
        out.push_str("alias claude='ANTHROPIC_BASE_URL=\"https://api.anthropic.com\" \\claude --dangerously-skip-permissions --permission-mode bypassPermissions'\n");
        out.push_str("alias aider='\\aider'\n");
    }

    // claude-cc-gate — gateway discovery lists everything; the user picks the
    // main model in /model. Tier env vars are pinned to a SENTINEL name that no
    // provider carries: background jobs (permission classifier etc.) arrive at
    // claude-proxy under that name and are retargeted to the window's main model
    // ([background] tier-follow logic). Without it, a background job firing before
    // the first main-model request falls through to the Anthropic passthrough
    // with the placeholder "proxy" key → 401.
    let sentinel = "claude-haiku-follows-main";
    if powershell {
        out.push_str(&format!(
            "function claude-cc-gate {{ $env:ANTHROPIC_BASE_URL='http://127.0.0.1:{cport}'; $env:ANTHROPIC_AUTH_TOKEN='proxy'; $env:ANTHROPIC_DEFAULT_OPUS_MODEL='{sentinel}'; $env:ANTHROPIC_DEFAULT_SONNET_MODEL='{sentinel}'; $env:ANTHROPIC_DEFAULT_HAIKU_MODEL='{sentinel}'; $env:ANTHROPIC_DEFAULT_FABLE_MODEL='{sentinel}'; $env:CLAUDE_CODE_USE_GATEWAY='1'; $env:CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY='1'; claude --dangerously-skip-permissions --permission-mode bypassPermissions }}\n"
        ));
    } else {
        out.push_str(&format!(
            "alias claude-cc-gate='ANTHROPIC_BASE_URL=\"http://127.0.0.1:{cport}\" \\\n  ANTHROPIC_AUTH_TOKEN=proxy \\\n  ANTHROPIC_DEFAULT_OPUS_MODEL=\"{sentinel}\" \\\n  ANTHROPIC_DEFAULT_SONNET_MODEL=\"{sentinel}\" \\\n  ANTHROPIC_DEFAULT_HAIKU_MODEL=\"{sentinel}\" \\\n  ANTHROPIC_DEFAULT_FABLE_MODEL=\"{sentinel}\" \\\n  CLAUDE_CODE_USE_GATEWAY=1 \\\n  CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1 \\\n  \\claude --dangerously-skip-permissions --permission-mode bypassPermissions'\n"
        ));
    }

    // codex-cc-gate — full catalog via mimo2codex; /model switches inside Codex.
    if let Some(first) = enabled.first() {
        let slug = first.slug.as_str();
        if powershell {
            out.push_str(&format!(
                "function codex-cc-gate {{ $env:CC_GATE_MODEL='{slug}'; $env:OPENAI_API_KEY='proxy'; codex --dangerously-bypass-approvals-and-sandbox -c model_provider='custom' -c model='{slug}' -c base_url='http://127.0.0.1:{mport}/v1' -c requires_openai_auth='false' }}\n"
            ));
        } else {
            out.push_str(&format!(
                "alias codex-cc-gate='CC_GATE_MODEL=\"{slug}\" OPENAI_API_KEY=proxy \\codex --dangerously-bypass-approvals-and-sandbox -c model_provider=\"custom\" -c model=\"{slug}\" -c base_url=\"http://127.0.0.1:{mport}/v1\" -c requires_openai_auth=\"false\"'\n"
            ));
        }
    }

    // aider-cc-gate — Aider has no in-session switcher; starts on the first
    // enabled model (use the 高级 multi-window aliases to pin others).
    if let Some(first) = enabled.first() {
        let slug = first.slug.as_str();
        if powershell {
            out.push_str(&format!(
                "function aider-cc-gate {{ $env:CC_GATE_MODEL='{slug}'; $env:OPENAI_API_BASE='http://127.0.0.1:{hport}/v1'; $env:OPENAI_API_KEY='proxy'; aider --model openai/{slug} }}\n"
            ));
        } else {
            out.push_str(&format!(
                "alias aider-cc-gate='CC_GATE_MODEL=\"{slug}\" OPENAI_API_BASE=http://127.0.0.1:{hport}/v1 OPENAI_API_KEY=proxy \\aider --model openai/{slug}'\n"
            ));
        }
    }

    // hermes-cc-gate — Hermes takes --provider/--model flags natively.
    if let Some(first) = enabled.first() {
        let slug = first.slug.as_str();
        out.push_str(&format!(
            "alias hermes-cc-gate='\\hermes --provider ccgate -m {slug}'\n"
        ));
    }

    // pi-cc-gate — PI is config-driven (~/.pi/agent/models.json is auto-written
    // with every enabled model); the command is a named entry point.
    out.push_str("alias pi-cc-gate='\\pi'\n");

// ── Custom aliases (别名页) ─────────────────────────────
    // User-defined tool × model × source shortcuts. Token `ccgate-<name>` lets
    // the local proxies pick a per-window upstream, so two terminals can run
    // the same tool+model via different sources simultaneously.
    for a in &cfg.custom_aliases {
        let line = match a.tool.as_str() {
            "claude_cli" => custom_claude_line(a, cfg, powershell),
            "codex_cli"  => custom_codex_line(a, cfg, powershell),
            "aider"      => custom_aider_line(a, cfg, powershell),
            "opencode"   => custom_opencode_line(a, powershell),
            "hermes"     => custom_hermes_line(a, powershell),
            _ => continue,
        };
        out.push_str(&line);
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
    write_alias_routes(cfg)?;
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
    write_pi_models(cfg)?;
    write_opencode_alias_configs(cfg)?;
    tracing::info!("All tool configs written");
    Ok(())
}

// ── Hermes config.yaml ─────────────────────────────────────

pub fn write_hermes_config(cfg: &AppConfig) -> Result<()> {
    let slugs: Vec<String> = cfg.models.iter().filter(|m| m.enabled).map(|m| m.slug.clone()).collect();

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

    // Build CC-Gate base provider entry (only when homepage assigns models)
    let mut models_map = serde_yaml::Mapping::new();
    for slug in &slugs {
        if let Some(m) = cfg.models.iter().find(|d| &d.slug == slug) {
            let mut mm = serde_yaml::Mapping::new();
            mm.insert("context_length".into(), serde_yaml::Value::Number((m.context_window as i64).into()));
            mm.insert("name".into(), serde_yaml::Value::String(format!("{} (CC-Gate)", m.display_name)));
            models_map.insert(serde_yaml::Value::String(slug.clone()), serde_yaml::Value::Mapping(mm));
        }
    }
    // Relay-discovered models (picked ones, enabled relays only): chat-proxy
    // routes them the same way, so Hermes can switch to them via /model too.
    let known: std::collections::BTreeSet<String> = slugs.iter().cloned().collect();
    for relay in &cfg.relays {
        if relay.enabled == Some(false) { continue; }
        for dm in &relay.models {
            if !dm.selected || known.contains(&dm.id) { continue; }
            let base = if dm.display_name.is_empty() { dm.id.as_str() } else { dm.display_name.as_str() };
            let mut mm = serde_yaml::Mapping::new();
            if let Some(cw) = dm.context_window {
                mm.insert("context_length".into(), serde_yaml::Value::Number((cw as i64).into()));
            }
            mm.insert("name".into(), serde_yaml::Value::String(format!("{} - {}", relay.name, base)));
            // Prefixed key: Hermes lists keys verbatim; chat-proxy resolves.
            models_map.insert(
                serde_yaml::Value::String(format!("{}/{}", relay.name, dm.id)),
                serde_yaml::Value::Mapping(mm));
        }
    }

    // Filter existing custom_providers to keep non-CC-Gate ones.
    // "ccgate" AND every per-alias "ccgate-<name>" are ours; anything else survives.
    let is_ours = |name: &str| name == "ccgate" || name.starts_with("ccgate-");
    let mut new_providers: Vec<serde_yaml::Value> = Vec::new();
    if let serde_yaml::Value::Mapping(ref map) = doc {
        if let Some(serde_yaml::Value::Sequence(existing)) = map.get("custom_providers") {
            for entry in existing {
                if let Some(name) = entry.get("name").and_then(|v| v.as_str()) {
                    if is_ours(name) { continue; } // remove old CC-Gate entries
                }
                new_providers.push(entry.clone());
            }
        }
    }

    if !slugs.is_empty() {
        let default_model = slugs.first().cloned().unwrap();
        let mut provider = serde_yaml::Mapping::new();
        provider.insert("name".into(), "ccgate".into());
        provider.insert("base_url".into(), base_url.clone().into());
        provider.insert("api_key".into(), "proxy".into());
        provider.insert("api_mode".into(), "chat_completions".into());
        provider.insert("models".into(), serde_yaml::Value::Mapping(models_map));
        provider.insert("model".into(), default_model.into());
        new_providers.push(serde_yaml::Value::Mapping(provider));
    }

    // Per-alias providers (方案B token routing): each pins its own source via the
    // alias's ccgate-<name> bearer token, independent of homepage assignment.
    for route in build_alias_routes(cfg) {
        if route["tool"] != "hermes" { continue; }
        let name = route["name"].as_str().unwrap_or_default().to_string();
        if name.is_empty() { continue; }
        let token = format!("ccgate-{name}");
        let mut p = serde_yaml::Mapping::new();
        p.insert("name".into(), serde_yaml::Value::String(format!("ccgate-{name}")));
        p.insert("base_url".into(), base_url.clone().into());
        p.insert("api_key".into(), serde_yaml::Value::String(token));
        p.insert("api_mode".into(), "chat_completions".into());
        new_providers.push(serde_yaml::Value::Mapping(p));
    }

    if let serde_yaml::Value::Mapping(ref mut map) = doc {
        map.insert("custom_providers".into(), serde_yaml::Value::Sequence(new_providers));
    }

    let out = serde_yaml::to_string(&doc)?;
    write_if_changed(&path, &out)
}

// ── OpenClaw openclaw.json ──────────────────────────────────

pub fn write_openclaw_config(cfg: &AppConfig) -> Result<()> {
    let slugs: Vec<String> = cfg.models.iter().filter(|m| m.enabled).map(|m| m.slug.clone()).collect();
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
    let slugs: Vec<String> = cfg.models.iter().filter(|m| m.enabled).map(|m| m.slug.clone()).collect();
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

// ── Custom alias line builders (别名页) ──────────────────────

/// Claude Code: native Anthropic-direct → pure native line (no proxy);
/// everything else → claude-proxy with token ccgate-<name> so the proxy
/// resolves the upstream per-window.
fn custom_claude_line(a: &crate::types::CustomAlias, cfg: &AppConfig, powershell: bool) -> String {
    let cm = format!("claude-{}", a.model);
    let provider = cfg.models.iter().find(|m| m.slug == a.model).map(|m| m.provider.as_str());

    if a.source == "direct" && provider == Some("anthropic") {
        // Same shape as the bare `claude` alias — user's own login/key.
        if powershell {
            return format!(
                "function {name} {{ $env:ANTHROPIC_BASE_URL='https://api.anthropic.com'; & (Get-Command claude -CommandType Application) --dangerously-skip-permissions --permission-mode bypassPermissions $args }}\n",
                name = a.name
            );
        }
        return format!(
            "alias {name}='ANTHROPIC_BASE_URL=\"https://api.anthropic.com\" \\claude --dangerously-skip-permissions --permission-mode bypassPermissions'\n",
            name = a.name
        );
    }

    let port = cfg.proxy_ports.claude_proxy;
    let token = format!("ccgate-{}", a.name);
    if powershell {
        format!(
            "function {name} {{ $env:ANTHROPIC_BASE_URL='http://127.0.0.1:{port}'; $env:ANTHROPIC_AUTH_TOKEN='{token}'; $env:ANTHROPIC_MODEL='{cm}'; $env:ANTHROPIC_DEFAULT_OPUS_MODEL='{cm}'; $env:ANTHROPIC_DEFAULT_SONNET_MODEL='{cm}'; $env:ANTHROPIC_DEFAULT_HAIKU_MODEL='{cm}'; $env:ANTHROPIC_DEFAULT_FABLE_MODEL='{cm}'; $env:CLAUDE_CODE_USE_GATEWAY='1'; $env:CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY='1'; claude --dangerously-skip-permissions --permission-mode bypassPermissions }}\n",
            name = a.name, port = port, token = token, cm = cm,
        )
    } else {
        format!(
            "alias {name}='ANTHROPIC_BASE_URL=\"http://127.0.0.1:{port}\" \\\n  ANTHROPIC_AUTH_TOKEN={token} \\\n  ANTHROPIC_MODEL=\"{cm}\" \\\n  ANTHROPIC_DEFAULT_OPUS_MODEL=\"{cm}\" \\\n  ANTHROPIC_DEFAULT_SONNET_MODEL=\"{cm}\" \\\n  ANTHROPIC_DEFAULT_HAIKU_MODEL=\"{cm}\" \\\n  ANTHROPIC_DEFAULT_FABLE_MODEL=\"{cm}\" \\\n  CLAUDE_CODE_USE_GATEWAY=1 \\\n  CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1 \\\n  \\claude --dangerously-skip-permissions --permission-mode bypassPermissions'\n",
            name = a.name, port = port, token = token, cm = cm,
        )
    }
}

/// Aider: always via chat-proxy with token ccgate-<name> — every model×source
/// combo works and usage stays visible.
fn custom_aider_line(a: &crate::types::CustomAlias, cfg: &AppConfig, powershell: bool) -> String {
    let port = cfg.proxy_ports.chat_proxy;
    let token = format!("ccgate-{}", a.name);
    if powershell {
        format!(
            "function {name} {{ $env:OPENAI_API_BASE='http://127.0.0.1:{port}/v1'; $env:OPENAI_API_KEY='{token}'; aider --model openai/{model} }}\n",
            name = a.name, port = port, token = token, model = a.model,
        )
    } else {
        format!(
            "alias {name}='OPENAI_API_BASE=http://127.0.0.1:{port}/v1 OPENAI_API_KEY={token} \\aider --model openai/{model}'\n",
            name = a.name, port = port, token = token, model = a.model,
        )
    }
}

/// OpenCode: OPENCODE_CONFIG points at a per-alias config file (written by
/// write_opencode_alias_configs) whose provider carries the ccgate token.
fn custom_opencode_line(a: &crate::types::CustomAlias, powershell: bool) -> String {
    if powershell {
        format!(
            "function {name} {{ $env:OPENCODE_CONFIG=\"$env:USERPROFILE\\.mimo2codex\\opencode-aliases\\{name}.json\"; & (Get-Command opencode) $args }}\n",
            name = a.name,
        )
    } else {
        format!(
            "alias {name}='OPENCODE_CONFIG=\"$HOME/.mimo2codex/opencode-aliases/{name}.json\" \\opencode'\n",
            name = a.name,
        )
    }
}

/// Hermes: per-alias provider entries (ccgate-<name>) are written into
/// config.yaml by write_hermes_config; the alias just selects provider+model.
fn custom_hermes_line(a: &crate::types::CustomAlias, powershell: bool) -> String {
    let provider = format!("ccgate-{}", a.name);
    if powershell {
        format!(
            "function {name} {{ & (Get-Command hermes) --provider '{provider}' -m '{model}' $args }}\n",
            name = a.name, provider = provider, model = a.model,
        )
    } else {
        format!(
            "alias {name}='\\hermes --provider {provider} -m {model}'\n",
            name = a.name, provider = provider, model = a.model,
        )
    }
}

/// Codex CLI: injects the upstream directly on the command line (same pattern
/// as codex-ds native aliases). direct → PROVIDER_META url + key from api_keys;
/// relay:<n> → relay URL + embedded relay key.
fn custom_codex_line(a: &crate::types::CustomAlias, cfg: &AppConfig, powershell: bool) -> String {
    let m = cfg.models.iter().find(|m| m.slug == a.model);
    let ctx = m.map(|m| m.context_window).unwrap_or(131_072);
    let max = m.map(|m| m.max_output_tokens).unwrap_or(16_384);

    let (base_url, api_key) = if let Some(relay_name) = a.source.strip_prefix("relay:") {
        match cfg.relays.iter().find(|r| &r.name == relay_name) {
            Some(r) => (normalize_relay_base_url(&r.url), r.key.clone()),
            None => return String::new(), // stale relay — skip silently
        }
    } else {
        match m.as_ref().and_then(|m| meta_by_id(&m.provider)) {
            Some(meta) => (
                meta.base_url.to_string(),
                cfg.api_keys.get(meta.env_key).cloned().unwrap_or_else(|| "proxy".into()),
            ),
            None => return String::new(),
        }
    };

    if powershell {
        format!(
            "function {name} {{ $env:OPENAI_API_KEY='{key}'; codex --dangerously-bypass-approvals-and-sandbox -c model_provider='custom' -c model='{slug}' -c base_url='{url}' -c model_context_window={ctx} -c model_max_output_tokens={max} }}\n",
            name = a.name, key = api_key, slug = a.model, url = base_url, ctx = ctx, max = max,
        )
    } else {
        format!(
            "alias {name}='OPENAI_API_KEY={key} \\codex --dangerously-bypass-approvals-and-sandbox -c model_provider=\"custom\" -c model=\"{slug}\" -c base_url=\"{url}\" -c model_context_window={ctx} -c model_max_output_tokens={max}'\n",
            name = a.name, key = api_key, slug = a.model, url = base_url, ctx = ctx, max = max,
        )
    }
}

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

        // Unified gateway commands — one per tool, no per-model farms anymore.
        assert!(out.contains("alias claude-cc-gate="), "claude-cc-gate must exist:\n{out}");
        assert!(out.contains("CLAUDE_CODE_USE_GATEWAY=1"), "claude-cc-gate must enable gateway mode:\n{out}");
        assert!(out.contains("CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1"), "discovery must be on:\n{out}");
        let cc: String = {
            let lines = out.lines().collect::<Vec<_>>();
            let i = lines.iter().position(|l| l.starts_with("alias claude-cc-gate='")).unwrap();
            lines[i..].join("\n")
        };
        assert!(cc.contains("ANTHROPIC_AUTH_TOKEN=proxy"), "gateway alias must use proxy token:\n{cc}");
        assert!(cc.contains("\\claude"), "gateway alias must invoke claude:\n{cc}");

        // codex-cc-gate pins the first ENABLED model as default; routes via mimo2codex.
        let cg = out.lines().find(|l| l.starts_with("alias codex-cc-gate='")).unwrap();
        assert!(cg.contains("-c base_url=\"http://127.0.0.1:8688/v1\""), "codex gateway must route via mimo2codex:\n{cg}");
        assert!(cg.contains("requires_openai_auth=\"false\""), "codex gateway must be zero-auth:\n{cg}");
        // deepseek-v4-pro is enabled in this fixture → it is the pinned default.
        assert!(cg.contains("-c model=\"deepseek-v4-pro\""), "first enabled model becomes default:\n{cg}");

        // No per-model alias farms left behind.
        assert!(!out.contains("alias codex-ds='"), "per-model codex aliases are gone:\n{out}");
        assert!(!out.contains("alias claude-ds='CC_GATE_MODEL="), "per-model claude aliases are gone:\n{out}");
        assert!(!out.contains("alias aider-ds='CC_GATE_MODEL="), "per-model aider aliases are gone:\n{out}");

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


#[cfg(test)]
mod custom_alias_tests {
    use super::{build_alias_routes, gen_aliases_impl, validate_alias_name};
    use crate::types::{AppConfig, CustomAlias, RelayConfig};

    fn cfg_with_alias(tool: &str, model: &str, source: &str) -> AppConfig {
        let mut cfg = AppConfig::default();
        // Trim the default all-agents-all-models noise so generated output is readable.
        cfg.agent_models.clear();
        cfg.custom_aliases = vec![CustomAlias {
            name: "zz".into(), tool: tool.into(), model: model.into(), source: source.into(),
        }];
        cfg
    }

    #[test]
    fn claude_proxied_line_carries_token() {
        // deepseek-v4-flash direct (non-anthropic) → claude-proxy + ccgate token
        let cfg = cfg_with_alias("claude_cli", "deepseek-v4-flash", "direct");
        let mut out = String::new();
        gen_aliases_impl(&cfg, &mut out, false);
        assert!(out.contains("alias zz='ANTHROPIC_BASE_URL=\"http://127.0.0.1:8689\""), "{out}");
        assert!(out.contains("ANTHROPIC_AUTH_TOKEN=ccgate-zz"), "{out}");
        assert!(out.contains("ANTHROPIC_MODEL=\"claude-deepseek-v4-flash\""), "{out}");
    }

    #[test]
    fn claude_anthropic_direct_is_native_line() {
        let cfg = cfg_with_alias("claude_cli", "claude-opus-5", "direct");
        let mut out = String::new();
        gen_aliases_impl(&cfg, &mut out, false);
        assert!(out.contains("alias zz='ANTHROPIC_BASE_URL=\"https://api.anthropic.com\""), "{out}");
        assert!(!out.contains("ccgate-zz"), "native line must not carry a ccgate token:\n{out}");
    }

    #[test]
    fn aider_line_uses_chat_proxy_and_token() {
        let cfg = cfg_with_alias("aider", "glm-5.2", "direct");
        let mut out = String::new();
        gen_aliases_impl(&cfg, &mut out, false);
        assert!(out.contains("OPENAI_API_BASE=http://127.0.0.1:8690/v1"), "{out}");
        assert!(out.contains("OPENAI_API_KEY=ccgate-zz"), "{out}");
        assert!(out.contains("--model openai/glm-5.2"), "{out}");
    }

    #[test]
    fn codex_direct_injects_provider_url() {
        let cfg = cfg_with_alias("codex_cli", "deepseek-v4-pro", "direct");
        let mut out = String::new();
        gen_aliases_impl(&cfg, &mut out, false);
        assert!(out.contains("base_url=\"https://api.deepseek.com/v1\""), "{out}");
        assert!(out.contains("-c model=\"deepseek-v4-pro\""), "{out}");
        assert!(!out.contains("ccgate-zz"), "codex lines are direct-injected, no proxy token:\n{out}");
    }

    #[test]
    fn codex_relay_injects_relay_url() {
        let mut cfg = cfg_with_alias("codex_cli", "deepseek-v4-pro", "relay:商汤");
        cfg.relays.push(RelayConfig {
            name: "商汤".into(), url: "https://relay.example.com/v1/chat/completions".into(),
            anthropic_url: None, key: "sk-relay-test".into(),
            enabled: None,
        models: vec![],
        });
        let mut out = String::new();
        gen_aliases_impl(&cfg, &mut out, false);
        // Full endpoint path must be normalized away
        assert!(out.contains("base_url=\"https://relay.example.com/v1\""), "{out}");
        assert!(out.contains("OPENAI_API_KEY=sk-relay-test"), "{out}");
    }

    #[test]
    fn routes_only_for_proxied_combos() {
        let mut cfg = AppConfig::default();
        cfg.relays.push(RelayConfig {
            name: "r1".into(), url: "https://r1.example.com/v1/".into(),
            anthropic_url: Some("https://r1.example.com/anthropic".into()), key: "k1".into(),
            enabled: None,
        models: vec![],
        });
        cfg.custom_aliases = vec![
            CustomAlias { name: "ca".into(), tool: "claude_cli".into(), model: "deepseek-v4-flash".into(), source: "direct".into() },
            CustomAlias { name: "cb".into(), tool: "claude_cli".into(), model: "claude-opus-5".into(),  source: "relay:r1".into() },
            CustomAlias { name: "cc".into(), tool: "aider".into(),      model: "glm-5.2".into(),         source: "relay:r1".into(), },
            CustomAlias { name: "cd".into(), tool: "codex_cli".into(),  model: "deepseek-v4-pro".into(), source: "direct".into() },
            CustomAlias { name: "ce".into(), tool: "claude_cli".into(), model: "claude-opus-5".into(),   source: "direct".into(), },
        ];
        let routes = build_alias_routes(&cfg);
        let names: Vec<&str> = routes.iter().filter_map(|r| r["name"].as_str()).collect();
        // ca proxied ✓; cb anthropic via relay anthropic_url → passthrough route ✓;
        // cc aider relay ✓; cd codex → none; ce anthropic-direct native → none
        assert_eq!(names, vec!["ca", "cb", "cc"], "routes: {routes:?}");

        let cb = routes.iter().find(|r| r["name"] == "cb").unwrap();
        assert_eq!(cb["baseUrl"], "https://r1.example.com/anthropic");
        assert_eq!(cb["anthropicEndpoint"], serde_json::json!(true));
        assert_eq!(cb["token"], "ccgate-cb");

        let ca = routes.iter().find(|r| r["name"] == "ca").unwrap();
        assert_eq!(ca["envKey"], "DEEPSEEK_API_KEY");
        assert_eq!(ca["anthropicEndpoint"], serde_json::json!(false));
        // 方案B: direct source → only that provider's models are honored
        let ca_models = ca["models"].as_array().unwrap();
        assert!(ca_models.iter().all(|m| m.as_str().unwrap().starts_with("deepseek-")), "{ca_models:?}");
        // relay source → every enabled model is honored
        let cc = routes.iter().find(|r| r["name"] == "cc").unwrap();
        assert!(!cc["models"].as_array().unwrap().is_empty());
        assert_eq!(cc["baseUrl"], "https://r1.example.com/v1"); // trailing slash normalized
        assert_eq!(cc["envKey"], super::relay_env_key("r1"));
    }

    #[test]
    fn alias_name_validation() {
        let cfg = AppConfig::default();
        assert!(validate_alias_name("dsf", &cfg, None).is_ok());
        assert!(validate_alias_name("A-b_2", &cfg, None).is_ok());
        // builtin collisions
        assert!(validate_alias_name("claude", &cfg, None).is_err());
        assert!(validate_alias_name("claude-ds", &cfg, None).is_err());
        assert!(validate_alias_name("codex-glm", &cfg, None).is_err());
        // shape rules
        assert!(validate_alias_name("2fast", &cfg, None).is_err());
        assert!(validate_alias_name("a", &cfg, None).is_err());
        assert!(validate_alias_name("has space", &cfg, None).is_err());
        // uniqueness among customs, with ignore_self for updates
        let mut cfg2 = AppConfig::default();
        cfg2.custom_aliases.push(CustomAlias { name: "taken".into(), tool: "aider".into(), model: "glm-5.2".into(), source: "direct".into() });
        assert!(validate_alias_name("taken", &cfg2, None).is_err());
        assert!(validate_alias_name("taken", &cfg2, Some("taken")).is_ok());
    }
}

#[cfg(test)]
mod tier_pi_tests {
    use super::merge_pi_models;
    use crate::types::{AppConfig, CustomAlias};

    #[test]
    fn pi_merge_preserves_user_providers_and_adds_layers() {
        let mut cfg = AppConfig::default();
        cfg.agent_models.clear();
        cfg.agent_models.insert("pi".into(), vec!["deepseek-v4-flash".into()]);
        cfg.relays.push(crate::types::RelayConfig {
            name: "r1".into(), url: "https://r1.example.com/v1".into(),
            anthropic_url: None, key: "k".into(),
            enabled: None,
        models: vec![],
        });
        cfg.custom_aliases = vec![CustomAlias {
            name: "dsf".into(), tool: "pi".into(),
            model: "deepseek-v4-flash".into(), source: "relay:r1".into(),
        }];

        let user_doc = serde_json::json!({
            "providers": { "my-ollama": { "baseUrl": "http://localhost:11434/v1" } },
            "otherKey": true
        });
        let merged = merge_pi_models(user_doc, &cfg).expect("merge ok");

        // user content preserved
        assert!(merged["otherKey"].as_bool().unwrap());
        assert!(merged.pointer("/providers/my-ollama/baseUrl").is_some());

        // ① base ccgate provider with homepage-assigned models
        let base = merged.pointer("/providers/ccgate").expect("base provider");
        assert_eq!(base["api"], "openai-completions");
        let ids: Vec<&str> = base["models"].as_array().unwrap()
            .iter().map(|m| m["id"].as_str().unwrap()).collect();
        // Unified-gateway rule: pi sees every globally-enabled model (the
        // fixture's builtin catalog), not a per-agent whitelist slice.
        assert!(ids.contains(&"deepseek-v4-flash"), "enabled models must be present: {ids:?}");
        assert!(ids.len() > 1, "pi now lists the full enabled set: {ids:?}");

        // ② per-alias provider speaks anthropic-messages via :8689 with token header
        let al = merged.pointer("/providers/ccgate-dsf").expect("alias provider");
        assert_eq!(al["api"], "anthropic-messages");
        assert_eq!(al["baseUrl"], "http://127.0.0.1:8689");
        assert_eq!(al["headers"]["x-api-key"], "ccgate-dsf");

        // invalid docs are rejected so a broken file never gets wiped
        assert!(merge_pi_models(serde_json::json!("oops"), &cfg).is_err());
        assert!(merge_pi_models(serde_json::json!({"providers": []}), &cfg).is_err());
    }
}

#[cfg(test)]
mod opencode_hermes_tests {
    use super::{build_alias_routes, gen_aliases_impl};
    use crate::types::{AppConfig, CustomAlias};

    fn cfg_with(tool: &str) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.agent_models.clear();
        cfg.custom_aliases = vec![CustomAlias {
            name: "zz".into(), tool: tool.into(),
            model: "deepseek-v4-flash".into(), source: "relay:r1".into(),
        }];
        cfg.relays.push(crate::types::RelayConfig {
            name: "r1".into(), url: "https://r1.example.com/v1".into(),
            anthropic_url: None, key: "k".into(),
            enabled: None,
        models: vec![],
        });
        cfg
    }

    #[test]
    fn opencode_alias_line_points_at_config_file() {
        let cfg = cfg_with("opencode");
        let mut out = String::new();
        gen_aliases_impl(&cfg, &mut out, false);
        assert!(out.contains("alias zz='OPENCODE_CONFIG=\"$HOME/.mimo2codex/opencode-aliases/zz.json\" \\opencode'"), "{out}");
        // route exists so the token is recognized by chat-proxy
        assert!(build_alias_routes(&cfg).iter().any(|r| r["token"] == "ccgate-zz"));
    }

    #[test]
    fn hermes_alias_line_selects_provider_and_model() {
        let cfg = cfg_with("hermes");
        let mut out = String::new();
        gen_aliases_impl(&cfg, &mut out, false);
        assert!(out.contains("alias zz='\\hermes --provider ccgate-zz -m deepseek-v4-flash'"), "{out}");
    }
}
