#!/usr/bin/env node
// chat-proxy.js — OpenAI Chat Completions passthrough + model routing
// Usage: node chat-proxy.js [--port 8690]
// Serves: Hermes, OpenCode, OpenClaw, Aider — any tool that speaks standard
// OpenAI Chat Completions API.
//
// No protocol translation — pure pass-through. Routes by model name,
// reading the same providers.json + .env as claude-proxy.js.

const http = require('http');
const https = require('https');
const fs = require('fs');
const path = require('path');
const os = require('os');

const PORT = parseInt(process.argv[process.argv.indexOf('--port') + 1]) || 8690;
const HOME = os.homedir();
const ENV_FILE = path.join(HOME, '.mimo2codex', '.env');
const PROVIDERS_FILE = path.join(HOME, '.mimo2codex', 'providers.json');

// ── Load API keys from .env ──────────────────────────────────
// Split on the first '=' instead of matching /^(\w+)=/ — \w is ASCII-only, so
// non-ASCII key names (e.g. RELAY_中转站_API_KEY) were silently dropped.
function loadEnv() {
  const env = {};
  if (fs.existsSync(ENV_FILE)) {
    fs.readFileSync(ENV_FILE, 'utf8').split('\n').forEach(line => {
      const t = line.trim();
      if (!t || t.startsWith('#')) return;
      const eq = t.indexOf('=');
      if (eq <= 0) return;
      env[t.slice(0, eq)] = t.slice(eq + 1).trim();
    });
  }
  return env;
}

// ── Load provider endpoints from providers.json ──────────────
function loadProviders(env) {
  const providers = {};
  if (fs.existsSync(PROVIDERS_FILE)) {
    const data = JSON.parse(fs.readFileSync(PROVIDERS_FILE, 'utf8'));
    for (const p of (data.providers || [])) {
      for (const m of (p.models || [])) {
        providers[m.id] = {
          baseUrl: p.baseUrl,
          apiKey: env[p.envKey] || '',
          defaultModel: m.id,
          displayName: m.displayName || m.id,
          contextWindow: m.contextWindow || 131072,
          maxOutputTokens: m.maxOutputTokens || 16384,
        };
      }
    }
  }
  // Built-in: DeepSeek
  if (!providers['deepseek-v4-pro']) {
    providers['deepseek-v4-pro'] = {
      baseUrl: 'https://api.deepseek.com/v1',
      apiKey: env.DS_API_KEY || env.DEEPSEEK_API_KEY || '',
      defaultModel: 'deepseek-v4-pro',
      displayName: 'DeepSeek V4 Pro',
      contextWindow: 1000000,
      maxOutputTokens: 393216,
    };
  }
  if (!providers['deepseek-v4-flash']) {
    providers['deepseek-v4-flash'] = {
      baseUrl: 'https://api.deepseek.com/v1',
      apiKey: env.DS_API_KEY || env.DEEPSEEK_API_KEY || '',
      defaultModel: 'deepseek-v4-flash',
      displayName: 'DeepSeek V4 Flash',
      contextWindow: 1000000,
      maxOutputTokens: 393216,
    };
  }
  return providers;
}

const env = loadEnv();
const PROVIDERS = loadProviders(env);

console.error(`[chat-proxy] Loaded ${Object.keys(PROVIDERS).length} models: ${Object.keys(PROVIDERS).join(', ')}`);

// ── Custom alias routes (别名页, written by CC-Gate) ─────────
// Bearer token `ccgate-<name>` → per-window upstream. Hot-reloaded via mtime
// so the app can add/remove aliases without restarting this proxy.
const ALIASES_FILE = path.join(HOME, '.mimo2codex', 'aliases.json');
let aliasCache = { aliasesMtime: 0, envMtime: 0, map: new Map(), freshEnv: {} };

function refreshAliases() {
  try {
    const am = fs.existsSync(ALIASES_FILE) ? fs.statSync(ALIASES_FILE).mtimeMs : 0;
    const em = fs.existsSync(ENV_FILE) ? fs.statSync(ENV_FILE).mtimeMs : 0;
    if (am === aliasCache.aliasesMtime && em === aliasCache.envMtime) return aliasCache;
    const map = new Map();
    if (am) {
      const data = JSON.parse(fs.readFileSync(ALIASES_FILE, 'utf8'));
      for (const a of (data.aliases || [])) map.set(a.token, a);
    }
    // Re-read .env so a just-saved relay/provider key is usable immediately.
    let freshEnv = {};
    if (em) {
      fs.readFileSync(ENV_FILE, 'utf8').split('\n').forEach(line => {
        const t = line.trim();
        if (!t || t.startsWith('#')) return;
        const eq = t.indexOf('=');
        if (eq <= 0) return;
        freshEnv[t.slice(0, eq)] = t.slice(eq + 1).trim();
      });
    }
    console.error(`[chat-proxy] [aliases] ${map.size} route(s) loaded`);
    aliasCache = { aliasesMtime: am, envMtime: em, map, freshEnv };
  } catch (e) {
    console.error(`[chat-proxy] [aliases] reload failed: ${e.message}`);
  }
  return aliasCache;
}

// Returns the alias route for a `ccgate-*` bearer token, or null.
function aliasFor(token) {
  if (!token || !String(token).startsWith('ccgate-')) return null;
  return refreshAliases().map.get(String(token)) || null;
}

function aliasToProvider(alias) {
  const key = alias.envKey ? (refreshAliases().freshEnv[alias.envKey] || '') : '';
  return {
    baseUrl: alias.baseUrl,
    apiKey: key,
    defaultModel: alias.model,
    displayName: `${alias.name} (alias)`,
  };
}

// ── Usage recording ──��─────────────────────────────────────
const USAGE_FILE = path.join(HOME, '.mimo2codex', 'usage.jsonl');

function modelToProvider(modelId) {
  if (modelId.startsWith('deepseek')) return 'deepseek';
  if (modelId.startsWith('glm')) return 'glm';
  if (modelId.startsWith('qwen3')) return 'qwen38';
  if (modelId.startsWith('qwen')) return 'qwen';
  if (modelId.startsWith('mimo')) return 'xiaomi';
  return 'unknown';
}

function recordUsage(modelId, usage, proxyName) {
  if (!usage || (!usage.prompt_tokens && !usage.completion_tokens && !usage.total_tokens)) return;
  const prompt = usage.prompt_tokens || 0;
  const completion = usage.completion_tokens || 0;
  const total = usage.total_tokens || (prompt + completion);
  const record = {
    request_id: `chat-${Date.now()}-${Math.random().toString(36).slice(2,8)}`,
    model: modelId,
    provider: modelToProvider(modelId),
    prompt_tokens: prompt,
    completion_tokens: completion,
    total_tokens: total,
    proxy: proxyName,
    timestamp: new Date().toISOString(),
  };
  try {
    fs.appendFileSync(USAGE_FILE, JSON.stringify(record) + '\n');
  } catch (e) {
    console.error(`[chat-proxy][usage] Failed to record: ${e.message}`);
  }
}

// ── /v1/models — OpenAI-format model list ──��────────────────
function handleModels(res, token) {
  console.error(`← GET /v1/models`);
  // Alias windows only see models their source carries.
  const alias = aliasFor(token);
  let providers = Object.values(PROVIDERS);
  if (alias) {
    const allowed = new Set(Array.isArray(alias.models) ? alias.models : [alias.model]);
    providers = providers.filter(p => allowed.has(p.defaultModel));
  }
  const models = providers.map(p => ({
    id: p.defaultModel,
    object: 'model',
    created: 1700000000,
    owned_by: 'CC-Gate',
  }));
  res.writeHead(200, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify({ object: 'list', data: models }));
}

// ── Non-streaming request ─────────────────────────────────────
function httpRequest(url, headers, body) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const mod = u.protocol === 'https:' ? https : http;
    const req = mod.request(u, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...headers },
      timeout: 300000,
    }, res => {
      let data = '';
      res.on('data', c => data += c);
      res.on('end', () => {
        try { resolve({ status: res.statusCode, body: JSON.parse(data), headers: res.headers }); }
        catch { resolve({ status: res.statusCode, body: data, headers: res.headers }); }
      });
    });
    req.on('error', reject);
    req.on('timeout', () => { req.destroy(); reject(new Error('timeout')); });
    req.write(JSON.stringify(body));
    req.end();
  });
}

// ── Streaming (SSE) request — pipe upstream SSE directly ─────
function streamRequest(url, headers, body, clientRes) {
  const u = new URL(url);
  const mod = u.protocol === 'https:' ? https : http;
  const upstreamReq = mod.request(u, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...headers },
    timeout: 600000,
  }, upstreamRes => {
    if (upstreamRes.statusCode !== 200) {
      // Non-streaming error — collect and send as JSON
      let data = '';
      upstreamRes.on('data', c => data += c);
      upstreamRes.on('end', () => {
        clientRes.writeHead(upstreamRes.statusCode, { 'Content-Type': 'application/json' });
        clientRes.end(data);
      });
      return;
    }
    // Pipe SSE headers and stream
    clientRes.writeHead(200, {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      'Connection': 'keep-alive',
    });
    upstreamRes.pipe(clientRes);
  });

  upstreamReq.on('error', e => {
    console.error(`[chat-proxy] Stream error: ${e.message}`);
    if (!clientRes.headersSent) {
      clientRes.writeHead(502, { 'Content-Type': 'application/json' });
      clientRes.end(JSON.stringify({ error: { message: `Upstream error: ${e.message}`, type: 'proxy_error' } }));
    } else {
      clientRes.end();
    }
  });
  upstreamReq.on('timeout', () => {
    upstreamReq.destroy();
    if (!clientRes.headersSent) {
      clientRes.writeHead(504, { 'Content-Type': 'application/json' });
      clientRes.end(JSON.stringify({ error: { message: 'Upstream timeout', type: 'proxy_error' } }));
    } else {
      clientRes.end();
    }
  });
  upstreamReq.write(JSON.stringify(body));
  upstreamReq.end();
}

// ── Model name normalization ─────────────────────────────────
// Aider sends "openai/deepseek-v4-pro" — strip the prefix
// Hermes/OpenCode send plain model names
function normalizeModel(raw) {
  // Strip openai/ prefix (Aider convention)
  if (raw.startsWith('openai/')) return raw.slice(7);
  // Strip anthropic/ prefix (just in case)
  if (raw.startsWith('anthropic/')) return raw.slice(10);
  return raw;
}

// ── Error response ───────────────────────────────────────────
function errorResponse(status, message) {
  return { error: { message, type: 'api_error', code: status } };
}

function sendError(res, status, message) {
  console.error(`[chat-proxy] ERROR ${status}: ${message}`);
  res.writeHead(status, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify(errorResponse(status, message)));
}

// ── Main server ──────────────────────────────────────────────
const server = http.createServer(async (req, res) => {
  console.error(`[chat-proxy] ${req.method} ${req.url}`);

  // ── GET /v1/models ─────────────────────────────────────
  if (req.method === 'GET' && req.url === '/v1/models') {
    handleModels(res, (req.headers['authorization'] || '').replace(/^Bearer\s+/i, '').trim());
    return;
  }

  // ── GET /health ────────────────────────────────────────
  if (req.method === 'GET' && req.url === '/health') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ status: 'ok', models: Object.keys(PROVIDERS).length }));
    return;
  }

  // ── POST /v1/chat/completions ──────────────────────────
  if (req.method !== 'POST' || !req.url.startsWith('/v1/chat/completions')) {
    res.writeHead(404, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(errorResponse(404, 'Not found. Use POST /v1/chat/completions or GET /v1/models')));
    return;
  }

  let body = '';
  req.on('data', c => body += c);
  req.on('end', async () => {
    let chatReq;
    try { chatReq = JSON.parse(body); }
    catch { sendError(res, 400, 'Invalid JSON'); return; }

    if (!chatReq.model) {
      sendError(res, 400, 'Missing model field');
      return;
    }

    const modelId = normalizeModel(chatReq.model);

    // ── Resolve provider ──
    // Priority: custom alias bearer token (per-window upstream, 别名页) >
    // providers.json model route. The alias must win or two windows couldn't
    // run the same model via different sources.
    const bearer = (req.headers['authorization'] || '').replace(/^Bearer\s+/i, '').trim();
    let provider;
    if (bearer.startsWith('ccgate-')) {
      const alias = aliasFor(bearer);
      provider = alias ? aliasToProvider(alias) : null;
      if (!provider) {
        sendError(res, 400, `Unknown alias token: ${bearer}`);
        return;
      }
      // 方案B: honor the requested model when this alias's source carries it,
      // so /model switching works inside an alias window.
      const allowed = Array.isArray(alias.models) ? alias.models : [];
      if (allowed.includes(modelId)) {
        provider.defaultModel = modelId;
      }
    } else {
      provider = PROVIDERS[modelId];
    }

    if (!provider) {
      const known = Object.keys(PROVIDERS).join(', ');
      sendError(res, 400, `Unknown model: ${modelId}. Available: ${known}`);
      return;
    }

    if (!provider.apiKey) {
      sendError(res, 500, `No API key configured for ${modelId}. Set the env var in ~/.mimo2codex/.env`);
      return;
    }

    // Build upstream request — keep original model name or use provider default
    const upstreamBody = { ...chatReq };
    upstreamBody.model = provider.defaultModel;

    // Some relay configs store the FULL endpoint path (e.g. https://…/v1/chat/completions),
    // but this proxy appends "/chat/completions" itself. Strip any existing suffix to
    // avoid a doubled path, which relays answer with "404 page not found".
    const base = String(provider.baseUrl || '').replace(/\/+$/, '');
    const upstreamUrl = base.endsWith('/chat/completions') ? base : `${base}/chat/completions`;
    const authHeaders = { 'Authorization': `Bearer ${provider.apiKey}` };

    const isStream = chatReq.stream === true;
    console.error(`→ ${modelId} → ${provider.displayName} (${isStream ? 'stream' : 'batch'}: ${upstreamUrl})`);

    if (isStream) {
      // Streaming: skip usage recording for now (SSE buffering complex)
      streamRequest(upstreamUrl, authHeaders, upstreamBody, res);
    } else {
      try {
        const result = await httpRequest(upstreamUrl, authHeaders, upstreamBody);
        // Record usage on success (disabled — 模型参数未校准, 暂不统计)
        // if (result.status === 200 && result.body?.usage) {
        //   recordUsage(modelId, result.body.usage, 'chat-proxy');
        // }
        // Pass through as-is — no translation needed
        res.writeHead(result.status, { 'Content-Type': 'application/json' });
        res.end(typeof result.body === 'string' ? result.body : JSON.stringify(result.body));
        if (result.status !== 200) {
          const errMsg = result.body?.error?.message || JSON.stringify(result.body);
          console.error(`← ${modelId} ERROR ${result.status}: ${errMsg}`);
        }
      } catch (e) {
        console.error(`← ${modelId} FAIL: ${e.message}`);
        sendError(res, 502, `Upstream error: ${e.message}`);
      }
    }
  });
});

server.listen(PORT, '127.0.0.1', () => {
  console.error(`[chat-proxy] Listening on http://127.0.0.1:${PORT}`);
  console.error(`[chat-proxy] Models: ${Object.keys(PROVIDERS).join(', ')}`);
});
