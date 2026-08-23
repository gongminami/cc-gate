use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Agent definitions ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AgentId {
    #[serde(rename = "codex_cli")]    CodexCli,
    #[serde(rename = "codex_desktop")] CodexDesktop,
    #[serde(rename = "claude_cli")]   ClaudeCli,
    #[serde(rename = "claude_desktop")] ClaudeDesktop,
    #[serde(rename = "hermes")]       Hermes,
    #[serde(rename = "opencode")]     OpenCode,
    #[serde(rename = "openclaw")]     OpenClaw,
    #[serde(rename = "aider")]        Aider,
    #[serde(rename = "cursor")]       Cursor,
    #[serde(rename = "reasonix")]     Reasonix,
    #[serde(rename = "pi")]           Pi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMeta {
    pub id: AgentId,
    pub name: String,
    #[serde(rename = "type")]
    pub agent_type: String,
    pub tool: String,
    pub proxy: String,
    pub writes_zshrc: bool,
    pub writes_providers: bool,
    pub writes_catalog: bool,
}

pub fn agent_list() -> Vec<AgentMeta> {
    vec![
        AgentMeta { id: AgentId::CodexCli,      name: "Codex CLI".into(),       agent_type: "cli".into(),     tool: "Codex".into(),     proxy: "mimo2codex".into(),   writes_zshrc: true,  writes_providers: true,  writes_catalog: true },
        AgentMeta { id: AgentId::CodexDesktop,   name: "Codex 桌面端".into(),    agent_type: "desktop".into(), tool: "Codex".into(),     proxy: "mimo2codex".into(),   writes_zshrc: false, writes_providers: true,  writes_catalog: true },
        AgentMeta { id: AgentId::ClaudeCli,      name: "Claude CLI".into(),      agent_type: "cli".into(),     tool: "Claude".into(),    proxy: "claude-proxy".into(), writes_zshrc: true,  writes_providers: true,  writes_catalog: false },
        AgentMeta { id: AgentId::ClaudeDesktop,   name: "Claude 桌面端".into(),   agent_type: "desktop".into(), tool: "Claude".into(),    proxy: "claude-proxy".into(), writes_zshrc: false, writes_providers: true,  writes_catalog: false },
        AgentMeta { id: AgentId::Hermes,          name: "Hermes".into(),          agent_type: "cli".into(),     tool: "Hermes".into(),    proxy: "chat-proxy".into(),   writes_zshrc: false, writes_providers: true,  writes_catalog: false },
        AgentMeta { id: AgentId::OpenCode,        name: "OpenCode".into(),        agent_type: "cli".into(),     tool: "OpenCode".into(),  proxy: "chat-proxy".into(),   writes_zshrc: false, writes_providers: true,  writes_catalog: false },
        AgentMeta { id: AgentId::OpenClaw,        name: "OpenClaw".into(),        agent_type: "cli".into(),     tool: "OpenClaw".into(),  proxy: "chat-proxy".into(),   writes_zshrc: false, writes_providers: true,  writes_catalog: false },
        AgentMeta { id: AgentId::Aider,           name: "Aider".into(),           agent_type: "cli".into(),     tool: "Aider".into(),     proxy: "chat-proxy".into(),   writes_zshrc: true,  writes_providers: true,  writes_catalog: false },
        AgentMeta { id: AgentId::Cursor,          name: "Cursor".into(),          agent_type: "cli".into(),     tool: "Cursor".into(),    proxy: "chat-proxy".into(),   writes_zshrc: false, writes_providers: true,  writes_catalog: false },
        AgentMeta { id: AgentId::Reasonix,       name: "Codex Reasonix".into(),  agent_type: "cli".into(),     tool: "Codex".into(),     proxy: "mimo2codex".into(),   writes_zshrc: false, writes_providers: true,  writes_catalog: true },
        AgentMeta { id: AgentId::Pi,             name: "PI".into(),              agent_type: "cli".into(),     tool: "PI".into(),        proxy: "chat-proxy".into(),   writes_zshrc: false, writes_providers: false, writes_catalog: false },
    ]
}

// ── Relay / transit station config ──────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    pub name: String,           // user-friendly label, e.g. "我的中转"
    pub url: String,            // OpenAI-compatible base URL, e.g. https://api.relay.com/v1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_url: Option<String>,  // Anthropic-native URL (optional), e.g. https://api.relay.com/anthropic
    pub key: String,            // API key (saved to .env as RELAY_<name>_API_KEY)
}

// ── Custom alias (别名页) ────────────────────────────────────

/// User-defined shortcut: tool × model × source.
/// `source`: "direct" | "relay:<relay_name>" (same vocabulary as model_routing).
/// Each alias becomes a shell alias whose token (`ccgate-<name>`) selects the
/// upstream per-window, so two terminals can run the same tool+model via
/// different sources simultaneously.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomAlias {
    pub name: String,   // shell identifier, e.g. "dsf"
    pub tool: String,   // "claude_cli" | "codex_cli" | "aider" | "pi"
    pub model: String,  // model slug
    pub source: String, // "direct" | "relay:<name>"
}

// ── App config ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub version: u32,
    pub models: Vec<ModelDef>,
    /// agent_id → list of enabled model slugs
    pub agent_models: HashMap<String, Vec<String>>,
    /// Named relay stations (set up in Settings)
    pub relays: Vec<RelayConfig>,
    /// model_slug → "direct" | "relay:<relay_name>"
    pub model_routing: HashMap<String, String>,
    /// env_var_name → api_key_value (saved to .env)
    #[serde(default)]
    pub api_keys: HashMap<String, String>,
    pub proxy_ports: ProxyPorts,
    #[serde(default)] pub model_catalog_version: u32,
    /// User-defined aliases from the 别名 page (newest appended; UI shows reversed).
    #[serde(default)] pub custom_aliases: Vec<CustomAlias>,
    #[serde(default = "default_true")] pub autostart_mimo2codex: bool,
    #[serde(default = "default_true")] pub autostart_claude_proxy: bool,
    #[serde(default = "default_true")] pub autostart_chat_proxy: bool,
    pub autostart_app: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        let all_slugs: Vec<String> = builtin_models().iter()
            .filter(|m| m.enabled).map(|m| m.slug.clone()).collect();

        let mut agent_models = HashMap::new();
        let mut model_routing = HashMap::new();
        for agent in agent_list() {
            agent_models.insert(agent_id_key(&agent.id), all_slugs.clone());
        }
        for m in builtin_models() {
            model_routing.insert(m.slug.clone(), "direct".into());
        }

        Self {
            version: 3, models: builtin_models(), agent_models, relays: vec![],
            model_routing, api_keys: HashMap::new(), proxy_ports: ProxyPorts::default(),
            model_catalog_version: 0,
            custom_aliases: vec![],
            autostart_mimo2codex: true, autostart_claude_proxy: true, autostart_chat_proxy: true,
            autostart_app: false,
        }
    }
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyPorts {
    pub mimo2codex: u16,
    pub claude_proxy: u16,
    pub chat_proxy: u16,
}
impl Default for ProxyPorts {
    fn default() -> Self { Self { mimo2codex: 8688, claude_proxy: 8689, chat_proxy: 8690 } }
}

// ── Model ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelDef {
    pub slug: String, pub display_name: String, pub provider: String,
    pub enabled: bool, pub context_window: u32, pub max_output_tokens: u32,
    pub priority: u32, pub default_reasoning_level: String,
    #[serde(default = "default_true")] pub supports_reasoning_summaries: bool,
    /// When true, Codex CLI aliases connect directly to the provider's Responses API
    /// (bypassing the local mimo2codex Chat Completions translation proxy).
    #[serde(default)] pub native_responses: bool,
    pub input_price_per_1k: f64, pub output_price_per_1k: f64,
}
impl Default for ModelDef {
    fn default() -> Self {
        Self { slug: String::new(), display_name: String::new(), provider: String::new(),
            enabled: true, context_window: 131072, max_output_tokens: 16384, priority: 100,
            default_reasoning_level: "high".into(), supports_reasoning_summaries: true,
            native_responses: false, input_price_per_1k: 0.0, output_price_per_1k: 0.0 }
    }
}

pub fn builtin_models() -> Vec<ModelDef> {
    vec![
        ModelDef { slug: "deepseek-v4-pro".into(),     display_name: "DeepSeek V4 Pro".into(),     provider: "deepseek".into(), enabled: true,  context_window: 1_000_000, max_output_tokens: 393_216, priority: 100,  default_reasoning_level: "high".into(),   input_price_per_1k: 0.0003, output_price_per_1k: 0.002,  native_responses: true, ..Default::default() },
        ModelDef { slug: "deepseek-v4-flash".into(),    display_name: "DeepSeek V4 Flash".into(),    provider: "deepseek".into(), enabled: true,  context_window: 1_000_000, max_output_tokens: 393_216, priority: 101,  default_reasoning_level: "medium".into(), input_price_per_1k: 0.0001, output_price_per_1k: 0.0005, native_responses: true, ..Default::default() },
        ModelDef { slug: "glm-5.2".into(),              display_name: "GLM-5.2".into(),              provider: "glm".into(),      enabled: true,  context_window: 1_000_000, max_output_tokens: 16_384,  priority: 200,  default_reasoning_level: "high".into(),   input_price_per_1k: 0.0014, output_price_per_1k: 0.0014, ..Default::default() },
        ModelDef { slug: "qwen3.8-max-preview".into(),  display_name: "Qwen3.8 Max Preview".into(),  provider: "qwen38".into(),    enabled: true,  context_window: 1_048_576, max_output_tokens: 65_536,  priority: 300,  default_reasoning_level: "high".into(),   input_price_per_1k: 0.0013, output_price_per_1k: 0.0052, ..Default::default() },
        ModelDef { slug: "qwen-max".into(),             display_name: "Qwen-Max".into(),             provider: "qwen".into(),      enabled: false, context_window: 131_072,   max_output_tokens: 16_384,  priority: 301,  default_reasoning_level: "high".into(),   input_price_per_1k: 0.003,  output_price_per_1k: 0.012,  ..Default::default() },
        ModelDef { slug: "mimo-v2.5-pro".into(),        display_name: "MiMo V2.5 Pro".into(),        provider: "xiaomi".into(),    enabled: true,  context_window: 131_072,   max_output_tokens: 16_384,  priority: 1000, default_reasoning_level: "high".into(),   input_price_per_1k: 0.0005, output_price_per_1k: 0.001,  ..Default::default() },
        ModelDef { slug: "mimo-v2.5".into(),            display_name: "MiMo V2.5".into(),            provider: "xiaomi".into(),    enabled: false, context_window: 1_000_000, max_output_tokens: 16_384,  priority: 1001, default_reasoning_level: "high".into(),   input_price_per_1k: 0.0005, output_price_per_1k: 0.001,  ..Default::default() },
        ModelDef { slug: "claude-opus-5".into(),         display_name: "Claude Opus 5".into(),         provider: "anthropic".into(),  enabled: true,  context_window: 1_000_000, max_output_tokens: 32_768,  priority: 50,   default_reasoning_level: "xhigh".into(),  input_price_per_1k: 0.015,  output_price_per_1k: 0.075,  ..Default::default() },
        ModelDef { slug: "gpt-5.6".into(),               display_name: "GPT-5.6".into(),               provider: "openai".into(),     enabled: true,  context_window: 1_000_000, max_output_tokens: 128_000, priority: 60,   default_reasoning_level: "xhigh".into(),  input_price_per_1k: 0.00125,output_price_per_1k: 0.01,   ..Default::default() },
        // Gemini 走官方 OpenAI 兼容端点(直连),native_responses=false → 一律经本地代理转 Chat Completions。
        ModelDef { slug: "gemini-3-flash-preview".into(), display_name: "Gemini 3 Flash Preview".into(), provider: "gemini".into(),     enabled: true,  context_window: 1_048_576, max_output_tokens: 65_536,  priority: 400, default_reasoning_level: "medium".into(), input_price_per_1k: 0.0005, output_price_per_1k: 0.003,  ..Default::default() },
        ModelDef { slug: "gemini-2.5-pro".into(),        display_name: "Gemini 2.5 Pro".into(),        provider: "gemini".into(),     enabled: true,  context_window: 1_048_576, max_output_tokens: 65_536,  priority: 401, default_reasoning_level: "high".into(),   input_price_per_1k: 0.00125,output_price_per_1k: 0.01,   ..Default::default() },
        // 2026-08 catalog sync (v2): GLM-5.3 / Qwen3.7-Max / Qwen3.8-Max 正式版
        // GLM-5.3 官方虽有 Responses 端点(open.bigmodel.cn/api/v1),仍走翻译代理保持一致行为。
        ModelDef { slug: "glm-5.3".into(),               display_name: "GLM-5.3".into(),               provider: "glm".into(),        enabled: true,  context_window: 1_000_000, max_output_tokens: 65_536,  priority: 201, default_reasoning_level: "high".into(),   input_price_per_1k: 0.0011, output_price_per_1k: 0.0039, ..Default::default() },
        ModelDef { slug: "qwen3.7-max".into(),           display_name: "Qwen3.7 Max".into(),           provider: "qwen".into(),       enabled: true,  context_window: 262_144,   max_output_tokens: 65_536,  priority: 302, default_reasoning_level: "high".into(),   input_price_per_1k: 0.0017, output_price_per_1k: 0.005,  ..Default::default() },
        ModelDef { slug: "qwen3.8-max".into(),           display_name: "Qwen3.8 Max".into(),           provider: "qwen".into(),       enabled: true,  context_window: 262_144,   max_output_tokens: 65_536,  priority: 303, default_reasoning_level: "high".into(),   input_price_per_1k: 0.0013, output_price_per_1k: 0.0052, ..Default::default() },
    ]
}

/// All known API key env var names (for the Settings page key grid).
/// (env_var_name, display_label, aliases)
pub fn all_api_key_names() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("DEEPSEEK_API_KEY",   "DeepSeek",         "也支持 DS_API_KEY"),
        ("GLM_API_KEY",        "智谱 GLM",          "也支持 ZHIPU_API_KEY"),
        ("QWEN_API_KEY",       "阿里 Qwen-Max",     ""),
        ("QWEN38_API_KEY",     "阿里 Qwen3.8",      ""),
        ("MIMO_API_KEY",       "小米 MiMo",          "也支持 MINIMAX_API_KEY"),
        ("MOONSHOT_API_KEY",   "Moonshot / Kimi",   "也支持 KIMI_API_KEY"),
        ("ERNIE_API_KEY",      "百度 文心一言",       ""),
        ("DOUBAO_API_KEY",     "字节 豆包",          "也支持 VOLCANO_API_KEY"),
        ("SPARK_API_KEY",      "讯飞 星火",          ""),
        ("HUNYUAN_API_KEY",    "腾讯 混元",          ""),
        ("MINIMAX_KEY",        "MiniMax",           "也支持 HILAILI_API_KEY"),
        ("BAICHUAN_API_KEY",   "百川 Baichuan",      ""),
        ("YI_API_KEY",         "零一万物 Yi",        ""),
        ("STEP_API_KEY",       "阶跃星辰 Step",      ""),
        ("OPENAI_API_KEY",     "OpenAI",            ""),
        ("ANTHROPIC_API_KEY",  "Anthropic Claude",  ""),
        ("GEMINI_API_KEY",     "Google Gemini",     "也支持 GOOGLE_API_KEY"),
        ("XAI_API_KEY",        "xAI Grok",          ""),
        ("MISTRAL_API_KEY",    "Mistral",           ""),
        ("COHERE_API_KEY",     "Cohere",             ""),
        ("META_API_KEY",       "Meta Llama",        ""),
        ("PERPLEXITY_API_KEY", "Perplexity",         ""),
        ("GROQ_API_KEY",       "Groq",              ""),
        ("TOGETHER_API_KEY",   "Together AI",       ""),
        ("DEEPINFRA_API_KEY",  "DeepInfra",         ""),
        ("REPLICATE_API_KEY",  "Replicate",         ""),
        ("OPENROUTER_API_KEY", "OpenRouter",         ""),
        ("CLOUDFLARE_AI_KEY",  "Cloudflare AI",     ""),
    ]
}

// ── Helpers ─────────────────────────────────────────────────

pub fn agent_id_key(id: &AgentId) -> String {
    serde_json::to_string(id).unwrap().trim_matches('"').to_string()
}

/// Routing for a model slug: "direct" or "relay:<name>"
pub fn routable_relays(relays: &[RelayConfig]) -> Vec<(&str, &str)> {
    // returns (routing_key, display_label)
    let mut v = vec![("direct", "直连")];
    for r in relays {
        v.push((&r.name, &r.name)); // will be displayed as-is in dropdown
    }
    v
}

// ── Proxy status ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyStatus {
    pub name: String, pub port: u16, pub running: bool, pub pid: Option<u32>, pub script: String,
}
