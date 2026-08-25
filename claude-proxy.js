#!/usr/bin/env node
// claude-proxy.js — Anthropic Messages API → OpenAI Chat Completions
// Usage: node claude-proxy.js [--port 8689]
// Auto-discovers providers from ~/.mimo2codex/.env + providers.json
//
// cc-gate-script-version: 2

const http = require('http');
const https = require('https');
const fs = require('fs');
const path = require('path');
const os = require('os');

// Default must match proxy_manager.rs, which always passes --port 8689 explicitly.
// A mismatched default silently listens on the wrong port when run by hand.
const PORT = parseInt(process.argv[process.argv.indexOf('--port') + 1]) || 8689;
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
          displayPrefix: p.displayPrefix || undefined,
          displayName: m.displayName || m.id,
          contextWindow: m.contextWindow || 131072,
          maxOutputTokens: m.maxOutputTokens || 16384,
          anthropicEndpoint: p.anthropicEndpoint || false,
          anthropicModel: p.anthropicModel || null,
          anthropicVersion: p.anthropicVersion || null,   // per-provider override
          timeoutMs: p.timeoutMs || null,                 // per-provider override
        };
      }
    }
  }
  // Built-in: DeepSeek (always available via default provider)
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

// ── Anthropic official models (gateway discovery supplement) ──
// The /model picker should also list Claude's own models even though they have
// no providers.json entry (they go through the native passthrough instead).
// Live list comes from models-cache.json — the remote model catalog CC-Gate
// refreshes from GitHub — so a new Claude model needs no proxy change. The
// static table is only the offline fallback.
const CATALOG_CACHE_FILE = path.join(HOME, '.mimo2codex', 'models-cache.json');

const OFFICIAL_MODELS_FALLBACK = [
  { id: 'claude-opus-5',   display_name: 'Claude Opus 5',   context_window: 1000000, max_output_tokens: 32768 },
  { id: 'claude-opus-4-8', display_name: 'Claude Opus 4.8', context_window: 1000000, max_output_tokens: 128000 },
  { id: 'claude-sonnet-5', display_name: 'Claude Sonnet 5', context_window: 1000000, max_output_tokens: 128000 },
  { id: 'claude-fable-5',  display_name: 'Claude Fable 5',  context_window: 1000000, max_output_tokens: 128000 },
];

function loadOfficialModels() {
  try {
    if (fs.existsSync(CATALOG_CACHE_FILE)) {
      const cat = JSON.parse(fs.readFileSync(CATALOG_CACHE_FILE, 'utf8'));
      const list = (cat.models || [])
        .filter(m => m.provider === 'anthropic' && m.slug)
        .map(m => ({
          id: m.slug,
          display_name: m.display_name || m.slug,
          context_window: m.context_window || 200000,
          max_output_tokens: m.max_output_tokens || 16384,
        }));
      if (list.length > 0) return list;
    }
  } catch (e) {
    console.error(`[official-models] catalog cache read failed: ${e.message}`);
  }
  return OFFICIAL_MODELS_FALLBACK;
}

// Native-passthrough upstream. Overridable so the test harness can point it at
// a fake upstream; production default is unchanged.
const ANTHROPIC_NATIVE_BASE = process.env.CCGATE_ANTHROPIC_BASE_URL || 'https://api.anthropic.com';

// ── Custom alias routes (别名页, written by CC-Gate) ─────────
// Token `ccgate-<name>` → per-window upstream. Hot-reloaded via mtime so the
// app can add/remove aliases without restarting this proxy.
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
    // Re-read .env so a just-saved relay/provider key is usable immediately
    // without restarting the proxy (PROVIDERS keep their startup snapshot).
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
    console.error(`[aliases] ${map.size} route(s) loaded`);
    aliasCache = { aliasesMtime: am, envMtime: em, map, freshEnv };
  } catch (e) {
    console.error(`[aliases] reload failed: ${e.message}`);
  }
  return aliasCache;
}

// Returns the alias route for a `ccgate-*` token, or null.
function aliasFor(token) {
  if (!token || !String(token).startsWith('ccgate-')) return null;
  return refreshAliases().map.get(String(token)) || null;
}

// ── providers.json hot reload ────────────────────────────────
// PROVIDERS used to be a startup snapshot, so "discover relay models" (or any
// provider edit) needed a proxy restart. Now: same mtime trick as aliases —
// reloaded lazily on the next request after CC-Gate rewrites the file.
const PROVIDERS = new Proxy({}, {
  get(_t, prop) { refreshProviders(); return Reflect.get(providersState.map, prop); },
  ownKeys() { refreshProviders(); return Reflect.ownKeys(providersState.map); },
  has(_t, prop) { refreshProviders(); return prop in providersState.map; },
  getOwnPropertyDescriptor(_t, prop) {
    refreshProviders();
    const d = Object.getOwnPropertyDescriptor(providersState.map, prop);
    return d ? { ...d, configurable: true } : undefined;
  },
});
let providersState = { mtime: -1, envMtime: -1, map: {} };

function refreshProviders() {
  try {
    const pm = fs.existsSync(PROVIDERS_FILE) ? fs.statSync(PROVIDERS_FILE).mtimeMs : 0;
    const em = fs.existsSync(ENV_FILE) ? fs.statSync(ENV_FILE).mtimeMs : 0;
    if (pm === providersState.mtime && em === providersState.envMtime) return;
    // Latest .env wins over the startup snapshot so a just-saved relay key is
    // picked up together with its new models.
    const env2 = { ...env, ...refreshAliases().freshEnv };
    providersState = { mtime: pm, envMtime: em, map: loadProviders(env2) };
    console.error(`[providers] ${Object.keys(providersState.map).length} entries loaded`);
  } catch (e) {
    console.error(`[providers] reload failed: ${e.message}`);
  }
}

function aliasToProvider(alias) {
  const key = alias.envKey ? (refreshAliases().freshEnv[alias.envKey] || '') : '';
  return {
    baseUrl: alias.baseUrl,
    apiKey: key,
    defaultModel: alias.model,
    displayName: `${alias.name} (alias)`,
    anthropicEndpoint: !!alias.anthropicEndpoint,
    anthropicModel: null,
    anthropicVersion: null,
    timeoutMs: null,
  };
}

// Request timeouts. Callers may override per-provider via opts.timeout (providers.json timeoutMs).
const TIMEOUT_UNARY = 120000;   // non-streaming request
const TIMEOUT_STREAM = 300000;  // streaming request — first byte may lag on slow relays

// ── HTTP request helper ──────────────────────────────────────
function httpRequest(url, opts, body) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const mod = u.protocol === 'https:' ? https : http;
    const req = mod.request(u, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...opts.headers },
      timeout: opts.timeout || TIMEOUT_UNARY,
    }, res => {
      let data = '';
      res.on('data', c => data += c);
      res.on('end', () => {
        try { resolve({ status: res.statusCode, body: JSON.parse(data) }); }
        catch { resolve({ status: res.statusCode, body: data }); }
      });
    });
    req.on('error', reject);
    req.on('timeout', () => { req.destroy(); reject(new Error('timeout')); });
    req.write(JSON.stringify(body));
    req.end();
  });
}

// ── Streaming HTTP request (pass-through, no transform) ──────
function streamPassthrough(url, opts, body, clientRes) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const mod = u.protocol === 'https:' ? https : http;
    const upstreamReq = mod.request(u, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...opts.headers },
      timeout: opts.timeout || TIMEOUT_STREAM,
    }, upstreamRes => {
      if (upstreamRes.statusCode !== 200) {
        let data = '';
        upstreamRes.on('data', c => data += c);
        upstreamRes.on('end', () => {
          try {
            const err = JSON.parse(data);
            clientRes.writeHead(upstreamRes.statusCode, { 'Content-Type': 'application/json' });
            clientRes.end(JSON.stringify(errorResponse(upstreamRes.statusCode,
              err?.error?.message || data)));
          } catch {
            clientRes.writeHead(upstreamRes.statusCode, { 'Content-Type': 'application/json' });
            clientRes.end(JSON.stringify(errorResponse(upstreamRes.statusCode, data)));
          }
          resolve();
        });
        return;
      }
      // Streaming success — pipe through
      clientRes.writeHead(200, {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
        'Connection': 'keep-alive',
      });
      upstreamRes.pipe(clientRes);
      upstreamRes.on('end', () => resolve());
      upstreamRes.on('error', reject);
    });
    upstreamReq.on('error', (e) => {
      if (!clientRes.headersSent) {
        clientRes.writeHead(502, { 'Content-Type': 'application/json' });
        clientRes.end(JSON.stringify(errorResponse(502, `Upstream error: ${e.message}`)));
      }
      reject(e);
    });
    upstreamReq.on('timeout', () => {
      upstreamReq.destroy();
      if (!clientRes.headersSent) {
        clientRes.writeHead(504, { 'Content-Type': 'application/json' });
        clientRes.end(JSON.stringify(errorResponse(504, 'Upstream timeout')));
      }
      reject(new Error('timeout'));
    });
    upstreamReq.write(JSON.stringify(body));
    upstreamReq.end();
  });
}

// ── OpenAI streaming chunks → Anthropic SSE converter ─────────
function openaiStreamToAnthropicSSE(url, opts, body, clientRes, modelId) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const mod = u.protocol === 'https:' ? https : http;
    const upstreamReq = mod.request(u, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...opts.headers },
      timeout: opts.timeout || TIMEOUT_STREAM,
    }, upstreamRes => {
      if (upstreamRes.statusCode !== 200) {
        let data = '';
        upstreamRes.on('data', c => data += c);
        upstreamRes.on('end', () => {
          try {
            const err = JSON.parse(data);
            clientRes.writeHead(upstreamRes.statusCode, { 'Content-Type': 'application/json' });
            clientRes.end(JSON.stringify(errorResponse(upstreamRes.statusCode,
              err?.error?.message || data)));
          } catch {
            clientRes.writeHead(upstreamRes.statusCode, { 'Content-Type': 'application/json' });
            clientRes.end(JSON.stringify(errorResponse(upstreamRes.statusCode, data)));
          }
          resolve();
        });
        return;
      }

      clientRes.writeHead(200, {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
        'Connection': 'keep-alive',
      });

      let msgStarted = false;
      let blockIdx = -1;
      let blockKind = null;
      let inputTokens = 0;
      let outputTokens = 0;
      let finalStopReason = 'end_turn';
      let finished = false;
      const msgId = `msg_${Date.now()}`;
      const tcMap = new Map();           // tool call index → {id, name}
      const pending = [];                // buffer chunks before message_start

      // ── helpers ──────────────────────────────────────────
      function closeBlock() {
        if (blockIdx >= 0) {
          clientRes.write(`event: content_block_stop\ndata: ${
            JSON.stringify({type:'content_block_stop',index:blockIdx})
          }\n\n`);
          blockIdx = -1; blockKind = null;
        }
      }

      function flushMsgStart() {
        if (msgStarted) return;
        clientRes.write(`event: message_start\ndata: ${
          JSON.stringify({type:'message_start',message:{
            id:msgId,type:'message',role:'assistant',content:[],
            model:modelId,stop_reason:null,stop_sequence:null,
            usage:{input_tokens:inputTokens}
          }})
        }\n\n`);
        msgStarted = true;
        for (const fn of pending) fn();
        pending.length = 0;
      }

      function emitFinal() {
        if (finished) return; finished = true;
        flushMsgStart();
        closeBlock();
        clientRes.write(`event: message_delta\ndata: ${
          JSON.stringify({type:'message_delta',delta:{
            stop_reason:finalStopReason,stop_sequence:null
          },usage:{output_tokens:outputTokens}})
        }\n\n`);
        clientRes.write(`event: message_stop\ndata: ${
          JSON.stringify({type:'message_stop'})
        }\n\n`);
      }

      // ── data handler ──────────────────────────────────────
      let buffer = '';
      upstreamRes.on('data', chunk => {
        buffer += chunk.toString();
        const lines = buffer.split('\n');
        buffer = lines.pop() || '';

        for (const line of lines) {
          const s = line.trim();
          if (!s.startsWith('data: ')) continue;
          const p = s.slice(6).trim();
          if (p === '[DONE]') { emitFinal(); clientRes.end(); resolve(); return; }

          let obj;
          try { obj = JSON.parse(p); } catch { continue; }
          const ch = (obj.choices || [])[0] || {};
          const d = ch.delta || {};
          const fr = ch.finish_reason;

          // input tokens — DeepSeek sends this in the LAST chunk
          if (obj.usage?.prompt_tokens) {
            inputTokens = obj.usage.prompt_tokens;
            flushMsgStart();         // now we have the real value, flush everything
          }
          // output tokens + stop reason from the finish_reason chunk
          if (fr) {
            if (obj.usage?.completion_tokens) outputTokens = obj.usage.completion_tokens;
            if (fr === 'tool_calls') finalStopReason = 'tool_use';
            else if (fr === 'length' || fr === 'max_tokens') finalStopReason = 'max_tokens';
            else finalStopReason = 'end_turn';
          }

          // ── text content ──────────────────────────────
          const doText = () => {
            if (d.content != null && d.content !== '') {
              if (blockKind !== 'text') closeBlock();
              if (blockIdx < 0) {
                blockIdx = 0; blockKind = 'text';
                clientRes.write(`event: content_block_start\ndata: ${
                  JSON.stringify({type:'content_block_start',index:0,
                    content_block:{type:'text',text:''}})
                }\n\n`);
              }
              clientRes.write(`event: content_block_delta\ndata: ${
                JSON.stringify({type:'content_block_delta',index:0,
                  delta:{type:'text_delta',text:d.content}})
              }\n\n`);
            }
          };

          // ── tool calls ────────────────────────────────
          const doTools = () => {
            if (!d.tool_calls) return;
            for (const tc of d.tool_calls) {
              const i = tc.index;
              if (!tcMap.has(i)) tcMap.set(i, { id: tc.id || '', name: '' });
              const e = tcMap.get(i);
              if (tc.id) e.id = tc.id;
              if (tc.function?.name) e.name = tc.function.name;

              if (blockKind !== 'tool_use' || blockIdx !== i) closeBlock();
              if (blockIdx < 0) {
                blockIdx = i; blockKind = 'tool_use';
                clientRes.write(`event: content_block_start\ndata: ${
                  JSON.stringify({type:'content_block_start',index:i,
                    content_block:{type:'tool_use',id:e.id,name:e.name,input:{}}})
                }\n\n`);
              }
              if (tc.function?.arguments) {
                clientRes.write(`event: content_block_delta\ndata: ${
                  JSON.stringify({type:'content_block_delta',index:i,
                    delta:{type:'input_json_delta',partial_json:tc.function.arguments}})
                }\n\n`);
              }
            }
          };

          // ── dispatch ──────────────────────────────────
          if (!msgStarted && !inputTokens) {
            // Defer: message_start hasn't been sent yet, buffer content
            pending.push(doText);
            pending.push(doTools);
          } else {
            flushMsgStart();
            doText();
            doTools();
          }
        }
      });

      // ── end handler ────────────────────────────────────────
      upstreamRes.on('end', () => {
        if (!finished && buffer.trim().startsWith('data: ') && buffer.trim().slice(6).trim() !== '[DONE]') {
          try {
            const obj = JSON.parse(buffer.trim().slice(6).trim());
            const d = (obj.choices || [])[0]?.delta || {};
            if (d.content != null) {
              if (blockKind !== 'text') closeBlock();
              if (blockIdx < 0) { blockIdx = 0; blockKind = 'text';
                clientRes.write(`event: content_block_start\ndata: ${
                  JSON.stringify({type:'content_block_start',index:0,
                    content_block:{type:'text',text:''}})
                }\n\n`);
              }
              clientRes.write(`event: content_block_delta\ndata: ${
                JSON.stringify({type:'content_block_delta',index:0,
                  delta:{type:'text_delta',text:d.content}})
              }\n\n`);
            }
          } catch {}
        }
        emitFinal();
        if (!clientRes.writableEnded) clientRes.end();
        resolve();
      });

      upstreamRes.on('error', reject);
    });

    upstreamReq.on('error', (e) => {
      console.error(`[stream] Upstream error: ${e.message}`);
      if (!clientRes.headersSent) {
        clientRes.writeHead(502, { 'Content-Type': 'application/json' });
        clientRes.end(JSON.stringify(errorResponse(502, `Upstream error: ${e.message}`)));
      } else if (!clientRes.writableEnded) {
        clientRes.end();
      }
      reject(e);
    });
    upstreamReq.on('timeout', () => {
      upstreamReq.destroy();
      if (!clientRes.headersSent) {
        clientRes.writeHead(504, { 'Content-Type': 'application/json' });
        clientRes.end(JSON.stringify(errorResponse(504, 'Upstream timeout')));
      }
      reject(new Error('timeout'));
    });
    upstreamReq.write(JSON.stringify(body));
    upstreamReq.end();
  });
}

// ── Anthropic Messages → OpenAI Chat Completions ─────────────
function anthropicToOpenAI(anthropicReq) {
  const messages = [];
  // System → system message
  if (anthropicReq.system) {
    if (typeof anthropicReq.system === 'string') {
      messages.push({ role: 'system', content: anthropicReq.system });
    } else if (Array.isArray(anthropicReq.system)) {
      for (const block of anthropicReq.system) {
        if (block.type === 'text') messages.push({ role: 'system', content: block.text });
      }
    }
  }
  // Messages — handle mixed content blocks (text, tool_use, tool_result)
  for (const msg of (anthropicReq.messages || [])) {
    if (typeof msg.content === 'string') {
      messages.push({ role: msg.role, content: msg.content });
    } else if (Array.isArray(msg.content)) {
      // Build OpenAI-format message from Anthropic content blocks
      let textParts = [];
      let toolCalls = [];
      for (const block of msg.content) {
        if (block.type === 'text') {
          textParts.push(block.text);
        } else if (block.type === 'tool_use') {
          toolCalls.push({
            id: block.id,
            type: 'function',
            function: {
              name: block.name,
              arguments: typeof block.input === 'string' ? block.input : JSON.stringify(block.input),
            },
          });
        } else if (block.type === 'tool_result') {
          // Tool result → tool message in OpenAI format
          messages.push({
            role: 'tool',
            tool_call_id: block.tool_use_id,
            content: typeof block.content === 'string' ? block.content : JSON.stringify(block.content),
          });
        }
      }
      const openaiMsg = { role: msg.role };
      if (textParts.length > 0) openaiMsg.content = textParts.join('\n');
      if (toolCalls.length > 0) openaiMsg.tool_calls = toolCalls;
      if (textParts.length > 0 || toolCalls.length > 0) {
        messages.push(openaiMsg);
      }
    }
  }
  // Translate Anthropic tools → OpenAI tools (function calling)
  const openaiReq = {
    model: anthropicReq.model,
    messages,
    max_tokens: anthropicReq.max_tokens || 4096,
    temperature: anthropicReq.temperature,
    top_p: anthropicReq.top_p,
    stop: anthropicReq.stop_sequences,
    stream: false,
  };
  if (anthropicReq.tools && anthropicReq.tools.length > 0) {
    openaiReq.tools = anthropicReq.tools.map(t => ({
      type: 'function',
      function: {
        name: t.name,
        description: t.description || '',
        parameters: t.input_schema || { type: 'object', properties: {} },
      },
    }));
    // Default to 'auto' — let model decide when to call tools
    openaiReq.tool_choice = 'auto';
  }
  return openaiReq;
}

// ── OpenAI Chat Completion → Anthropic Messages ─────────────
function openAIToAnthropic(openAIResp, model) {
  const choice = (openAIResp.choices || [])[0] || {};
  const msg = choice.message || {};
  const content = [];

  // Text content
  if (msg.content) {
    content.push({ type: 'text', text: msg.content });
  }

  // Tool calls → tool_use blocks
  if (msg.tool_calls && msg.tool_calls.length > 0) {
    for (const tc of msg.tool_calls) {
      let input;
      try {
        input = typeof tc.function.arguments === 'string'
          ? JSON.parse(tc.function.arguments)
          : tc.function.arguments;
      } catch {
        input = {};
      }
      content.push({
        type: 'tool_use',
        id: tc.id,
        name: tc.function.name,
        input,
      });
    }
  }

  // Fallback: if somehow no content at all, add empty text
  if (content.length === 0) {
    content.push({ type: 'text', text: '' });
  }

  // Determine stop_reason
  let stopReason = 'end_turn';
  if (choice.finish_reason === 'tool_calls') {
    stopReason = 'tool_use';
  } else if (choice.finish_reason === 'length' || choice.finish_reason === 'max_tokens') {
    stopReason = 'max_tokens';
  }

  return {
    id: `msg_${Date.now()}`,
    type: 'message',
    role: 'assistant',
    content,
    model,
    stop_reason: stopReason,
    stop_sequence: null,
    usage: {
      input_tokens: openAIResp.usage?.prompt_tokens || 0,
      output_tokens: openAIResp.usage?.completion_tokens || 0,
    },
  };
}

// ── Error response ───────────────────────────────────────────
// Translate cryptic relay/vendor errors into actionable hints (kept in English+
// Chinese so the message survives any client). Matched on stable substrings.
// Clamp client max_tokens to the model's declared output cap. PI sends huge
// defaults; relays like SenseNova reject anything above their limit.
function clampMaxTokens(provider, body) {
  let cap = provider && provider.maxOutputTokens;
  // Alias windows build their provider from the alias route (no output cap on
  // it) — fall back to the same model's entry in the shared catalog.
  if (!cap) {
    const ref = PROVIDERS[body.model];
    if (ref) cap = ref.maxOutputTokens;
  }
  if (cap && body.max_tokens > cap) {
    console.error(`[clamp] max_tokens ${body.max_tokens} -> ${cap}`);
    body.max_tokens = cap;
  }
}

function humanizeUpstreamError(msg) {
  const m = String(msg || '');
  if (/prohibited due to a violation/i.test(m)) {
    return m + ' 【提示】该模型被中转站平台政策拦截（大厂闭源模型通常禁止聚合调用）。在 CC-Gate 中转站「挑选」里取消勾选即可不再出现；或在平台上绑定自己的官方 Key(BYOK)。';
  }
  if (/is not a valid model ID/i.test(m)) {
    return m + ' 【提示】该模型 ID 已失效——中转站可能已下架，请在 CC-Gate 中转站「刷新模型」后重新挑选。';
  }
  return m;
}

function errorResponse(status, message) {
  return {
    type: 'error',
    error: { type: 'api_error', message, code: status },
  };
}

// ── Token-based routing ──────────────────────────────────────
const TOKEN_MAP = {
  'ds': 'deepseek-v4-pro',
  'qwen': 'qwen3.8-max-preview',
  'glm': 'glm-5.2',
  'mimo': 'mimo-v2.5-pro',
};

// ── Usage recording ────────────────────────────────────────
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
  if (!usage || (!usage.input_tokens && !usage.prompt_tokens && !usage.total_tokens)) return;
  const prompt = usage.input_tokens || usage.prompt_tokens || 0;
  const completion = usage.output_tokens || usage.completion_tokens || 0;
  const total = usage.total_tokens || (prompt + completion);
  const record = {
    request_id: `claude-${Date.now()}-${Math.random().toString(36).slice(2,8)}`,
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
    console.error(`[usage] Failed to record: ${e.message}`);
  }
}

// ── /v1/models — gateway model discovery ────��───────────────
function handleModels(res, token) {
  console.error(`← GET /v1/models (gateway discovery)`);
  // Alias windows only see models their source carries — a /model list that
  // offers unsupported names would just produce upstream errors.
  const alias = aliasFor(token);
  let providers = Object.values(PROVIDERS);
  if (alias) {
    const allowed = new Set(Array.isArray(alias.models) ? alias.models : [alias.model]);
    providers = providers.filter(p => allowed.has(p.defaultModel));
  }
  const routed = providers.map(p => ({
    id: 'claude-' + p.defaultModel,       // claude- prefix required by CC
    type: 'model',
    display_name: p.displayName,
    created_at: '2025-01-01T00:00:00Z',
    context_window: p.contextWindow || 200000,
    max_output_tokens: p.maxOutputTokens || 16384,
  }));
  // Official Claude models head the list — non-alias windows only. An alias
  // window pins one source whose token can't authenticate to api.anthropic.com
  // (placeholder "proxy"/"ccgate-*"), and relay-carried claude-* models already
  // appear above via their providers.json entries.
  const official = alias ? [] : loadOfficialModels().map(m => ({
    id: m.id,
    type: 'model',
    display_name: m.display_name,
    created_at: '2025-01-01T00:00:00Z',
    context_window: m.context_window,
    max_output_tokens: m.max_output_tokens,
  }));
  res.writeHead(200, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify({ data: [...official, ...routed] }));
}

// ── Main server ──────────────────────────────────────────────
const server = http.createServer(async (req, res) => {
  console.error(`${req.method} ${req.url}`);
  // Gateway model discovery — match on pathname only: Claude Code appends a
  // query string (?limit=1000), and strict equality 404'd it, silently emptying
  // the /model picker's gateway options.
  const reqPath = req.url.split('?')[0];
  if (req.method === 'GET' && reqPath === '/v1/models') {
    handleModels(res, (req.headers['x-api-key'] || (req.headers['authorization'] || '').replace('Bearer ', '')) || '');
    return;
  }

  // Count tokens — Claude Code uses this for context window management.
  // Return a rough estimate (char count / 2) so Claude Code doesn't hang.
  if (req.method === 'POST' && req.url.startsWith('/v1/messages/count_tokens')) {
    let body = '';
    req.on('data', c => body += c);
    req.on('end', () => {
      try {
        const parsed = JSON.parse(body);
        const text = JSON.stringify(parsed.messages || []) + (parsed.system || '');
        // Rough estimate: ~2 chars per token for Chinese/English mixed text
        const estimated = Math.max(1, Math.ceil(text.length / 2));
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ input_tokens: estimated }));
      } catch {
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ input_tokens: 100 }));
      }
    });
    return;
  }

  if (req.method !== 'POST' || !req.url.startsWith('/v1/messages')) {
    res.writeHead(404, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(errorResponse(404, 'Not found')));
    return;
  }

  // Extract token from x-api-key header (or Authorization Bearer)
  const authHeader = req.headers['x-api-key'] || '';
  const bearer = (req.headers['authorization'] || '').replace('Bearer ', '');
  const token = authHeader || bearer || 'ds';

  let body = '';
  req.on('data', c => body += c);
  req.on('end', async () => {
    let anthropicReq;
    try { anthropicReq = JSON.parse(body); }
    catch { res.writeHead(400); res.end(JSON.stringify(errorResponse(400, 'Invalid JSON'))); return; }

    const modelId = anthropicReq.model || '';

    // ── Resolve the requested model name ──
    // Gateway discovery prefixes every model with "claude-" (see handleModels), so
    // "deepseek-v4-pro" arrives as "claude-deepseek-v4-pro". But a provider model
    // literally named "claude-opus-5" arrives as "claude-claude-opus-5" via discovery
    // and as plain "claude-opus-5" when discovery is bypassed. Match the name as-is
    // first so a blind slice(7) can't mangle it into "opus-5".
    let realModelId = modelId;
    if (!PROVIDERS[realModelId] && modelId.startsWith('claude-')) {
      realModelId = modelId.slice(7);
    }

    // ── Resolve provider ──
    // Priority: custom alias token (per-window upstream, 别名页) > providers.json
    // model route > legacy token shorthand. Alias must win over everything or
    // two windows couldn't run the same model via different sources.
    const alias = aliasFor(token);
    let resolvedModel;
    let provider;
    if (alias) {
      provider = aliasToProvider(alias);
      // 方案B: honor the requested model when this alias's source carries it,
      // so /model switching works inside an alias window. Unknown names fall
      // back to the alias's own model instead of erroring at an upstream that
      // doesn't carry them.
      const allowed = Array.isArray(alias.models) ? alias.models : [];
      if (allowed.includes(realModelId) || realModelId === alias.model) {
        provider.defaultModel = realModelId;
      } else {
        provider.defaultModel = alias.model;
      }
      resolvedModel = provider.defaultModel;
    } else {
      // Token routing (x-api-key: ds|qwen|glm|mimo) is a legacy shorthand. It must not
      // outrank an explicitly requested model, or the model field is silently ignored.
      resolvedModel = PROVIDERS[realModelId] ? realModelId : (TOKEN_MAP[token] || realModelId);
      provider = PROVIDERS[resolvedModel];
    }

    // ── Anthropic-native passthrough (built-in) — only when providers.json has no route ──
    // Claude's own models go directly to api.anthropic.com with the client's OAuth token.
    // Gated on !provider: any provider (incl. a third-party relay) that configures a
    // claude-* model must win over this built-in, or its key is sent to Anthropic → 401.
    const isAnthropicNative = !provider && /^claude-(opus|sonnet|haiku|fable)-/.test(modelId);
    if (isAnthropicNative) {
      // Alias windows inject placeholder tokens ("proxy", "ccgate-*") that can never
      // authenticate to Anthropic — fall back to a real key from .env/process when present.
      let clientKey = authHeader || 'no-key';
      if (!clientKey || clientKey === 'proxy' || clientKey.startsWith('ccgate-')) {
        clientKey = process.env.ANTHROPIC_API_KEY || env.ANTHROPIC_API_KEY || clientKey;
      }
      const nativeHeaders = { 'x-api-key': clientKey, 'anthropic-version': '2023-06-01', 'Content-Type': 'application/json' };
      console.error(`→ ${modelId} → Anthropic passthrough (${ANTHROPIC_NATIVE_BASE})${anthropicReq.stream ? ' [stream]' : ''}`);
      const reqBody = { ...anthropicReq };
      // Keep the FULL "claude-*" name: discovery prefixed it, and Anthropic's API
      // requires it verbatim — the stripped realModelId here caused 400 model-not-found.
      reqBody.model = modelId;
      clampMaxTokens(null, reqBody); // no declared cap -> no-op
      try {
        if (anthropicReq.stream) {
          await streamPassthrough(`${ANTHROPIC_NATIVE_BASE}/v1/messages`, {
            headers: nativeHeaders
          }, reqBody, res);
        } else {
          const result = await httpRequest(`${ANTHROPIC_NATIVE_BASE}/v1/messages`, {
            headers: nativeHeaders
          }, reqBody);
          if (result.status === 200) {
            res.writeHead(200, { 'Content-Type': 'application/json' });
            res.end(JSON.stringify(result.body));
          } else {
            const errMsg = humanizeUpstreamError(result.body?.error?.message || JSON.stringify(result.body));
            console.error(`← ${realModelId} ERROR ${result.status}: ${errMsg}`);
            res.writeHead(result.status, { 'Content-Type': 'application/json' });
            res.end(JSON.stringify(errorResponse(result.status, errMsg)));
          }
        }
      } catch (e) {
        console.error(`← ${realModelId} FAIL: ${e.message}`);
        if (!res.headersSent) {
          res.writeHead(502, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify(errorResponse(502, `Upstream error: ${e.message}`)));
        }
      }
      return;
    }

    if (!provider) {
      // Distinguish "no route for this model" from "bad token shorthand" — conflating
      // them sent past debugging sessions chasing keys when the model name was at fault.
      const known = Object.keys(PROVIDERS).join(', ') || '(none)';
      const msg = `No provider configured for model "${modelId}" (resolved: "${resolvedModel}"). `
        + `Known models: ${known}. Token shorthands: ${Object.keys(TOKEN_MAP).join(', ')}.`;
      console.error(msg);
      res.writeHead(400, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify(errorResponse(400, msg)));
      return;
    }

    if (!provider.apiKey) {
      console.error(`No API key for ${modelId}`);
      res.writeHead(500, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify(errorResponse(500, `No API key configured for ${modelId}`)));
      return;
    }

    if (provider.anthropicEndpoint) {
      // ── Anthropic passthrough (provider speaks Anthropic natively) ──
      // Use provider.apiKey (from providers.json envKey), not the client's token —
      // the client sends a placeholder like "proxy", which upstream rejects with 401.
      const upstreamHeaders = {
        'x-api-key': provider.apiKey || authHeader || 'no-key',
        'anthropic-version': provider.anthropicVersion || '2023-06-01',
        'Content-Type': 'application/json',
      };
      console.error(`→ ${modelId} → ${provider.displayName} (passthrough: ${provider.baseUrl})${anthropicReq.stream ? ' [stream]' : ''}`);
      const reqBody = { ...anthropicReq };
      reqBody.model = provider.anthropicModel || provider.defaultModel || modelId;
      clampMaxTokens(provider, reqBody);
      try {
        if (anthropicReq.stream) {
          await streamPassthrough(provider.baseUrl + '/v1/messages', {
            headers: upstreamHeaders, timeout: provider.timeoutMs
          }, reqBody, res);
        } else {
          const result = await httpRequest(provider.baseUrl + '/v1/messages', {
            headers: upstreamHeaders, timeout: provider.timeoutMs
          }, reqBody);
          if (result.status === 200) {
            res.writeHead(200, { 'Content-Type': 'application/json' });
            res.end(JSON.stringify(result.body));
          } else {
            const errMsg = humanizeUpstreamError(result.body?.error?.message || JSON.stringify(result.body));
            console.error(`← ${modelId} ERROR ${result.status}: ${errMsg}`);
            res.writeHead(result.status, { 'Content-Type': 'application/json' });
            res.end(JSON.stringify(errorResponse(result.status, errMsg)));
          }
        }
      } catch (e) {
        console.error(`← ${modelId} FAIL: ${e.message}`);
        if (!res.headersSent) {
          res.writeHead(502, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify(errorResponse(502, `Upstream error: ${e.message}`)));
        }
      }
    } else {
      // ── OpenAI Chat Completions translation ──
      const openaiReq = anthropicToOpenAI(anthropicReq);
      openaiReq.model = provider.defaultModel;
      clampMaxTokens(provider, openaiReq);
      // Some relay configs store the FULL endpoint path (e.g. https://…/v1/chat/completions),
      // but this proxy appends "/chat/completions" itself. Strip any existing suffix to
      // avoid a doubled path, which relays answer with "404 page not found".
      const base = String(provider.baseUrl || '').replace(/\/+$/, '');
      const upstreamUrl = base.endsWith('/chat/completions') ? base : `${base}/chat/completions`;
      console.error(`→ ${modelId} → ${provider.displayName} (translate: ${upstreamUrl})${anthropicReq.stream ? ' [stream]' : ''}`);
      try {
        if (anthropicReq.stream) {
          openaiReq.stream = true;
          // DeepSeek V4 with thinking enabled puts tool_calls in reasoning_content
          // instead of delta.tool_calls, breaking Claude Code's tool parsing.
          // Qwen 3.8 requires enable_thinking=true; other models are fine without.
          if (resolvedModel.startsWith('deepseek')) {
            openaiReq.thinking = { type: 'disabled' };
          }
          await openaiStreamToAnthropicSSE(upstreamUrl, {
            headers: { 'Authorization': `Bearer ${provider.apiKey}` }, timeout: provider.timeoutMs
          }, openaiReq, res, modelId);
        } else {
          const result = await httpRequest(upstreamUrl, {
            headers: { 'Authorization': `Bearer ${provider.apiKey}` }, timeout: provider.timeoutMs
          }, openaiReq);
          if (result.status === 200) {
            const anthropicResp = openAIToAnthropic(result.body, modelId);
            res.writeHead(200, { 'Content-Type': 'application/json' });
            res.end(JSON.stringify(anthropicResp));
          } else {
            const errMsg = humanizeUpstreamError(result.body?.error?.message || JSON.stringify(result.body));
            console.error(`← ${modelId} ERROR ${result.status}: ${errMsg}`);
            res.writeHead(result.status, { 'Content-Type': 'application/json' });
            res.end(JSON.stringify(errorResponse(result.status, errMsg)));
          }
        }
      } catch (e) {
        console.error(`← ${modelId} FAIL: ${e.message}`);
        if (!res.headersSent) {
          res.writeHead(502, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify(errorResponse(502, `Upstream error: ${e.message}`)));
        }
      }
    }
  });
});

server.listen(PORT, '127.0.0.1', () => {
  console.error(`Claude Proxy listening on http://127.0.0.1:${PORT}`);
  console.error(`Providers: ${Object.keys(PROVIDERS).join(', ')}`);
});
