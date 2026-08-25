<script setup lang="ts">
import { computed, ref } from "vue";
import type { AppConfig } from "../types/models";
import { saveConfig, addRelay, updateRelay, deleteRelay, discoverRelayModels, setRelayEnabled, setRelayModelSelection, getRelayPresets, refreshRelayPresets } from "../ipc/api";
import type { RelayPreset } from "../ipc/api";
import { useToast } from "../composables/useToast";
import { useAppConfig } from "../composables/useAppConfig";

const props = defineProps<{ config: AppConfig | null }>();
const toast = useToast();
const { refresh } = useAppConfig();

const revealed = ref<Record<string, boolean>>({});
const userEdited = ref<Record<string, boolean>>({});

function getKey(envVar: string): string { return props.config?.api_keys?.[envVar] ?? ""; }
function setKey(envVar: string, value: string) {
  if (!props.config) return;
  props.config.api_keys = { ...props.config.api_keys, [envVar]: value };
  if (value) userEdited.value = { ...userEdited.value, [envVar]: true };
}
function clearKey(envVar: string) {
  setKey(envVar, "");
  revealed.value = { ...revealed.value, [envVar]: true };
  userEdited.value = { ...userEdited.value, [envVar]: true };
}
function wasDetected(envVar: string): boolean {
  return getKey(envVar).length > 0 && !userEdited.value[envVar];
}

async function saveApiKeys() {
  if (!props.config) return;
  try { await saveConfig(props.config); toast.ok("API 密钥已保存"); await refresh(); }
  catch (e: any) { toast.err(e?.message ?? String(e)); }
}

// 云端预设：打开弹窗时先秒显缓存，同时静默拉远端（3 秒上限，失败静默）
const relayPresets = ref<RelayPreset[]>([]);

async function loadPresets() {
  try { relayPresets.value = await getRelayPresets(); } catch { /* keep current */ }
  refreshRelayPresets().then(fresh => {
    if (fresh.length > 0) relayPresets.value = fresh;
  }).catch(() => { /* offline — cached list stays */ });
}

const editingRelay = ref<{ oldName: string; name: string; url: string; anthropicUrl: string; key: string; masked: boolean }>(
  { oldName: "", name: "", url: "", anthropicUrl: "", key: "", masked: true }
);
const relayBusy = ref(false);
const showRelayModal = ref(false);

function startNewRelay() { editingRelay.value = { oldName: "", name: "", url: "", anthropicUrl: "", key: "", masked: true }; showRelayModal.value = true; loadPresets(); }
function startEditRelay(r: { name: string; url: string; anthropic_url?: string; key: string }) { editingRelay.value = { oldName: r.name, name: r.name, url: r.url, anthropicUrl: r.anthropic_url ?? "", key: r.key, masked: true }; showRelayModal.value = true; }
function cancelRelay() { editingRelay.value = { oldName: "", name: "", url: "", anthropicUrl: "", key: "", masked: true }; showRelayModal.value = false; }
function pickPreset(p: RelayPreset) { editingRelay.value.name = p.name; editingRelay.value.url = p.url; editingRelay.value.anthropicUrl = p.anthropic_url ?? ""; }

async function onSaveRelay() {
  if (!props.config) return;
  const { oldName, name, url, key } = editingRelay.value;
  const anthropicUrl = editingRelay.value.anthropicUrl.trim();
  if (!name.trim() || !url.trim()) { toast.err("名称和 URL 不能为空"); return; }
  relayBusy.value = true;
  try {
    if (oldName) {
      await updateRelay(props.config, oldName, name, url, key, anthropicUrl || undefined);
    } else {
      if (props.config.relays.some(r => r.name === name)) { toast.err(`名称「${name}」已存在`); return; }
      await addRelay(props.config, name, url, key, anthropicUrl || undefined);
    }
    await refresh();
    toast.ok(`中转站「${name}」已保存`);
    cancelRelay();
  } catch (e: any) { toast.err(e?.message ?? String(e)); }
  finally { relayBusy.value = false; }
}

async function onDeleteRelay(name: string) {
  if (!props.config) return;
  try { await deleteRelay(props.config, name); await refresh(); toast.ok(`中转站「${name}」已删除`); }
  catch (e: any) { toast.err(e?.message ?? String(e)); }
}

// 中转站模型发现：GET {relay}/v1/models，全量导入并注入两个 proxy 的发现列表
const discovering = ref<Record<string, boolean>>({});

async function onDiscoverModels(name: string) {
  if (!props.config || discovering.value[name]) return;
  discovering.value = { ...discovering.value, [name]: true };
  try {
    const cfg = await discoverRelayModels(props.config, name);
    await refresh();
    const n = cfg.relays.find(r => r.name === name)?.models?.length ?? 0;
    toast.ok(`「${name}」发现 ${n} 个模型，已全部启用——新开终端即可在 /model 中切换`);
  } catch (e: any) { toast.err(e?.message ?? String(e)); }
  finally { discovering.value = { ...discovering.value, [name]: false }; }
}

function relayEnabled(r: { enabled?: boolean | null }): boolean { return r.enabled !== false; }

async function onToggleRelay(r: { name: string; enabled?: boolean | null }) {
  if (!props.config) return;
  try {
    await setRelayEnabled(props.config, r.name, !relayEnabled(r));
    await refresh();
    toast.ok(relayEnabled(r) ? `「${r.name}」已停用——其发现模型已从列表移除` : `「${r.name}」已启用`);
  } catch (e: any) { toast.err(e?.message ?? String(e)); }
}

// ── 挑选：pick which discovered models appear in the pickers ──
type RelayModel = { id: string; display_name?: string; context_window?: number; selected?: boolean };
const pickModal = ref<{ name: string; models: RelayModel[] } | null>(null);
const pickSearch = ref("");
const pickBusy = ref(false);
const pickedSet = ref<Set<string>>(new Set());
const pickShowSelectedOnly = ref(false);
const pickCollapsed = ref<Set<string>>(new Set());

function relayCounts(r: { models?: RelayModel[] }): { picked: number; total: number } {
  const all = r.models || [];
  return { picked: all.filter(m => m.selected !== false).length, total: all.length };
}

function openPickModal(r: { name: string; models?: RelayModel[] }) {
  pickModal.value = { name: r.name, models: [...(r.models || [])].sort((a, b) => a.id.localeCompare(b.id)) };
  pickedSet.value = new Set((r.models || []).filter(m => m.selected !== false).map(m => m.id));
  pickSearch.value = "";
  pickShowSelectedOnly.value = false;
  pickCollapsed.value = new Set();
}

// Overlay clicks deliberately do NOT close — only ×/取消/保存 do.
function cancelPick() { pickModal.value = null; pickSearch.value = ""; pickShowSelectedOnly.value = false; }

interface PickGroup { vendor: string; models: RelayModel[]; picked: number; }

const pickGroups = computed<PickGroup[]>(() => {
  if (!pickModal.value) return [];
  const q = pickSearch.value.trim().toLowerCase();
  let list = pickModal.value.models;
  if (pickShowSelectedOnly.value) list = list.filter(m => pickedSet.value.has(m.id));
  if (q) list = list.filter(m => m.id.toLowerCase().includes(q) || (m.display_name || "").toLowerCase().includes(q));
  const byVendor = new Map<string, RelayModel[]>();
  for (const m of list) {
    const slash = m.id.indexOf("/");
    const vendor = slash > 0 ? m.id.slice(0, slash) : "其他";
    if (!byVendor.has(vendor)) byVendor.set(vendor, []);
    byVendor.get(vendor)!.push(m);
  }
  return [...byVendor.entries()]
    .map(([vendor, models]) => ({ vendor, models, picked: models.filter(m => pickedSet.value.has(m.id)).length }))
    .sort((a, b) => a.vendor.localeCompare(b.vendor));
});

function togglePick(id: string) {
  const next = new Set(pickedSet.value);
  if (next.has(id)) next.delete(id); else next.add(id);
  pickedSet.value = next;
}

function toggleGroupCollapse(vendor: string) {
  const next = new Set(pickCollapsed.value);
  if (next.has(vendor)) next.delete(vendor); else next.add(vendor);
  pickCollapsed.value = next;
}

/** Group-level select/clear. With search or 只看已选 active it only touches VISIBLE rows. */
function groupSelect(vendor: string, on: boolean) {
  const g = pickGroups.value.find(x => x.vendor === vendor);
  if (!g) return;
  const next = new Set(pickedSet.value);
  for (const m of g.models) { if (on) next.add(m.id); else next.delete(m.id); }
  pickedSet.value = next;
}

function selectAllVisible() { const next = new Set(pickedSet.value); pickGroups.value.forEach(g => g.models.forEach(m => next.add(m.id))); pickedSet.value = next; }
function invertVisible() {
  const next = new Set(pickedSet.value);
  pickGroups.value.forEach(g => g.models.forEach(m => { if (next.has(m.id)) next.delete(m.id); else next.add(m.id); }));
  pickedSet.value = next;
}
function clearAll() { pickedSet.value = new Set(); }

async function onSavePick() {
  if (!props.config || !pickModal.value) return;
  pickBusy.value = true;
  try {
    await setRelayModelSelection(props.config, pickModal.value.name, [...pickedSet.value]);
    await refresh();
    toast.ok(`「${pickModal.value.name}」已挑选 ${pickedSet.value.size} 个模型`);
    cancelPick();
  } catch (e: any) { toast.err(e?.message ?? String(e)); }
  finally { pickBusy.value = false; }
}

const apiKeyGroups = [
  {
    label: "国内",
    keys: [
      { env: "DEEPSEEK_API_KEY", label: "DeepSeek", aliases: "也支持 DS_API_KEY", ph: "sk-…" },
      { env: "GLM_API_KEY",      label: "智谱 GLM", aliases: "也支持 ZHIPU_API_KEY", ph: "…" },
      { env: "QWEN_API_KEY",     label: "阿里 Qwen-Max", aliases: "", ph: "sk-…" },
      { env: "QWEN38_API_KEY",   label: "阿里 Qwen3.8", aliases: "", ph: "sk-…" },
      { env: "MIMO_API_KEY",     label: "小米 MiMo", aliases: "也支持 MINIMAX_API_KEY", ph: "…" },
      { env: "MOONSHOT_API_KEY", label: "Moonshot / Kimi", aliases: "也支持 KIMI_API_KEY", ph: "sk-…" },
      { env: "ERNIE_API_KEY",    label: "百度 文心一言", aliases: "", ph: "…" },
      { env: "DOUBAO_API_KEY",   label: "字节 豆包", aliases: "也支持 VOLCANO_API_KEY", ph: "…" },
      { env: "SPARK_API_KEY",    label: "讯飞 星火", aliases: "", ph: "…" },
      { env: "HUNYUAN_API_KEY",  label: "腾讯 混元", aliases: "", ph: "…" },
      { env: "MINIMAX_KEY",      label: "MiniMax", aliases: "也支持 HILAILI_API_KEY", ph: "…" },
      { env: "BAICHUAN_API_KEY", label: "百川 Baichuan", aliases: "", ph: "sk-…" },
      { env: "YI_API_KEY",       label: "零一万物 Yi", aliases: "", ph: "…" },
      { env: "STEP_API_KEY",     label: "阶跃星辰 Step", aliases: "", ph: "…" },
    ]
  },
  {
    label: "国际",
    keys: [
      { env: "OPENAI_API_KEY",    label: "OpenAI", aliases: "", ph: "sk-…" },
      { env: "ANTHROPIC_API_KEY", label: "Anthropic Claude", aliases: "", ph: "sk-ant-…" },
      { env: "GEMINI_API_KEY",    label: "Google Gemini", aliases: "也支持 GOOGLE_API_KEY", ph: "…" },
      { env: "XAI_API_KEY",       label: "xAI Grok", aliases: "", ph: "xai-…" },
      { env: "MISTRAL_API_KEY",   label: "Mistral", aliases: "", ph: "…" },
      { env: "COHERE_API_KEY",    label: "Cohere", aliases: "", ph: "…" },
      { env: "META_API_KEY",      label: "Meta Llama", aliases: "", ph: "…" },
      { env: "PERPLEXITY_API_KEY",label: "Perplexity", aliases: "", ph: "pplx-…" },
    ]
  },
];
</script>

<template>
  <section class="page">
    <header class="page-header"><h2>中转与 API Key</h2></header>

    <!-- 中转站 -->
    <div class="card">
      <div class="card-head">中转站</div>
      <div class="card-body">
        <p class="desc">添加中转站（OpenRouter 等），首页即可为模型选择直连还是走中转。同一 URL 可用不同 Key 添加多次——改名字区分即可。</p>

        <div v-if="config && config.relays.length > 0" class="relay-list">
          <div v-for="r in config.relays" :key="r.name" class="relay-row" :class="{ disabled: r.enabled === false }">
            <div class="relay-info">
              <span class="relay-name">{{ r.name }} <span v-if="r.enabled === false" class="off-badge">已停用</span></span>
              <span class="relay-url dim">OpenAI: {{ r.url }}</span>
              <span v-if="r.anthropic_url" class="relay-url dim">Anthropic: {{ r.anthropic_url }}</span>
              <span v-if="(r.models || []).length > 0" class="relay-found dim">已发现 {{ relayCounts(r).total }} 个，挑选 {{ relayCounts(r).picked }} 个</span>
            </div>
            <div class="relay-actions">
              <button v-if="(r.models || []).length > 0" class="btn ghost" :disabled="discovering[r.name] === true" @click="onDiscoverModels(r.name)">
                {{ discovering[r.name] ? "发现中…" : "刷新模型" }}
              </button>
              <button v-else class="btn ghost" :disabled="discovering[r.name] === true || !r.key" @click="onDiscoverModels(r.name)">
                {{ discovering[r.name] ? "发现中…" : "发现模型" }}
              </button>
              <button v-if="(r.models || []).length > 0" class="btn ghost" @click="openPickModal(r)">挑选</button>
              <button v-if="r.enabled !== false" class="btn ghost toggle-btn" @click="onToggleRelay(r)">
                停用
              </button>
              <button v-else class="btn ghost toggle-btn" @click="onToggleRelay(r)">启用</button>
              <button class="btn ghost" @click="startEditRelay(r)">编辑</button>
              <button class="btn ghost" style="color:var(--danger)" @click="onDeleteRelay(r.name)">删除</button>
            </div>
          </div>
        </div>
        <div v-else class="dim sec-empty">还没有添加中转站</div>

        <button class="btn" style="margin-top:8px" @click="startNewRelay">+ 添加中转站</button>
      </div>
    </div>

    <!-- 中转站弹窗 -->
    <div v-if="showRelayModal" class="modal-overlay" @click.self="cancelRelay">
      <div class="modal-dialog">
        <div class="modal-title">{{ editingRelay.oldName ? '编辑中转站' : '添加中转站' }}</div>

        <div class="modal-field">
          <label>名称 <span class="dim" style="font-weight:400">(必填)</span></label>
          <input v-model="editingRelay.name" type="text" placeholder="我的中转" />
        </div>

        <div class="modal-field">
          <label>OpenAI URL <span class="dim" style="font-weight:400">(必填)</span></label>
          <input v-model="editingRelay.url" type="text" placeholder="https://api.xxx.com/v1" style="font-family:monospace" />
        </div>

        <div class="modal-field">
          <label>API Key</label>
          <div class="key-input-wrap">
            <input :type="editingRelay.masked ? 'password' : 'text'" placeholder="sk-…" :value="editingRelay.key" @input="editingRelay.key = ($event.target as HTMLInputElement).value" @focus="editingRelay.masked = false" @blur="editingRelay.masked = true" />
            <span v-if="editingRelay.key" class="key-clear" @mousedown.prevent="editingRelay.key = ''">×</span>
          </div>
        </div>

        <div class="modal-field">
          <label>Anthropic URL <span class="dim" style="font-weight:400">(选填，Claude Code 原生协议)</span></label>
          <input v-model="editingRelay.anthropicUrl" type="text" placeholder="留空则使用上方 OpenAI URL" style="font-family:monospace" />
        </div>

        <div class="modal-presets">
          <span class="dim" style="font-size:12px">快速填入：</span>
          <button v-for="p in relayPresets" :key="p.name" class="btn ghost" style="font-size:12px;padding:2px 8px" @click="pickPreset(p)">{{ p.name }}</button>
        </div>

        <div class="modal-actions">
          <button class="btn" @click="cancelRelay">取消</button>
          <button class="btn primary" :disabled="relayBusy || !editingRelay.name.trim() || !editingRelay.url.trim()" @click="onSaveRelay">保存</button>
        </div>
      </div>
    </div>

    <!-- 挑选模型弹窗（overlay click intentionally does not close） -->
    <div v-if="pickModal" class="modal-overlay">
      <div class="modal-dialog pick-dialog">
        <div class="pick-head">
          <div class="modal-title">挑选「{{ pickModal.name }}」的模型</div>
          <button class="pick-close" title="关闭" @click="cancelPick">×</button>
        </div>

        <div class="pick-toolbar">
          <input v-model="pickSearch" type="text" placeholder="搜索 id 或名称…" class="pick-search" />
          <button class="btn ghost pick-tool" :class="{ on: pickShowSelectedOnly }" @click="pickShowSelectedOnly = !pickShowSelectedOnly">只看已选</button>
          <button class="btn ghost pick-tool" @click="selectAllVisible()">全选</button>
          <button class="btn ghost pick-tool" @click="invertVisible()">反选</button>
          <button class="btn ghost pick-tool" @click="clearAll()">清空</button>
        </div>

        <div class="pick-list">
          <div v-for="g in pickGroups" :key="g.vendor" class="pick-group">
            <div class="pick-group-head" :class="{ collapsed: pickCollapsed.has(g.vendor) }" @click="toggleGroupCollapse(g.vendor)">
              <span class="pg-caret">{{ pickCollapsed.has(g.vendor) ? "▸" : "▾" }}</span>
              <span class="pg-name">{{ g.vendor }}</span>
              <span class="pg-count dim">{{ g.picked }}/{{ g.models.length }}</span>
              <span class="pg-actions" @click.stop>
                <button class="pg-btn" title="全选该厂商" @click="groupSelect(g.vendor, true)">全选</button>
                <button class="pg-btn" title="清空该厂商" @click="groupSelect(g.vendor, false)">清空</button>
              </span>
            </div>
            <template v-if="!pickCollapsed.has(g.vendor)">
              <label v-for="m in g.models" :key="m.id" class="pick-row">
                <input type="checkbox" :checked="pickedSet.has(m.id)" @change="togglePick(m.id)" />
                <span class="pick-id">{{ m.display_name || m.id }}</span>
                <span class="pick-meta dim">{{ m.id }}<template v-if="m.context_window"> · {{ (m.context_window / 1000).toFixed(0) }}K</template></span>
              </label>
            </template>
          </div>
          <div v-if="pickGroups.length === 0" class="dim sec-empty" style="text-align:center;padding:20px 0">无匹配模型</div>
        </div>

        <div class="pick-foot">
          <span class="dim">已勾选 <b>{{ pickedSet.size }}</b> / {{ pickModal.models.length }} · 未勾选的不会出现在任何工具的模型列表里</span>
          <div class="modal-actions" style="margin:0">
            <button class="btn" @click="cancelPick">取消</button>
            <button class="btn primary" :disabled="pickBusy" @click="onSavePick">{{ pickBusy ? "保存中…" : "保存挑选" }}</button>
          </div>
        </div>
      </div>
    </div>

    <!-- API 密钥 -->
    <div class="card mt12">
      <div class="card-head">API 密钥（直连）</div>
      <div class="card-body">
        <p class="desc">直连厂商密钥。已填值显示为密码，聚焦可编辑，右侧 × 清空。自动检测 .env 已有值。<strong>如该厂商走中转则无需填写。</strong></p>

        <div class="sticky-save"><button class="btn primary" @click="saveApiKeys">保存密钥</button></div>

        <div v-for="group in apiKeyGroups" :key="group.label" class="key-group">
          <div class="key-group-label">{{ group.label }}</div>
          <div class="key-grid">
            <div v-for="k in group.keys" :key="k.env" class="field">
              <label>{{ k.label }} <span class="key-env dim">{{ k.env }}</span><span v-if="wasDetected(k.env)" class="detected-badge">env</span></label>
              <div class="key-input-wrap">
                <input :type="revealed[k.env] ? 'text' : 'password'" :placeholder="k.ph" :value="getKey(k.env)" @input="setKey(k.env, ($event.target as HTMLInputElement).value)" @focus="revealed = { ...revealed, [k.env]: true }" @blur="revealed = { ...revealed, [k.env]: false }" style="font-size:14px" />
                <span v-if="getKey(k.env)" class="key-clear" @mousedown.prevent="clearKey(k.env)">×</span>
              </div>
              <span v-if="k.aliases" class="help">{{ k.aliases }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.card-head { padding: 14px 18px 0; font-size: 16px; font-weight: 700; }
.card-body { padding: 10px 18px 16px; }
.desc { font-size: 14px; color: var(--fg-muted); line-height: 1.6; margin: 0 0 14px; }
.sec-empty { font-size: 14px; padding: 4px 0 12px; }

.relay-list { margin-bottom: 12px; }
.relay-row { display: flex; align-items: center; justify-content: space-between; padding: 6px 8px; border-bottom: 1px solid var(--border); font-size: 14px; }
.relay-row:last-child { border-bottom: none; }
.relay-info { display: flex; flex-direction: column; gap: 2px; }
.relay-name { font-weight: 600; font-size: 14px; }
.relay-url { font-family: "SF Mono", "Menlo", monospace; font-size: 13px; }
.relay-found { font-size: 12px; color: var(--accent); }
.relay-row.disabled .relay-info { opacity: 0.45; }
.off-badge { font-size: 10px; padding: 1px 6px; border-radius: 8px; background: var(--danger-soft); color: var(--danger); vertical-align: middle; margin-left: 4px; }
.toggle-btn { min-width: 44px; }
.relay-actions { display: flex; gap: 4px; }

/* modal */
.modal-overlay {
  position: fixed; inset: 0;
  background: rgba(0,0,0,0.5);
  display: flex; align-items: center; justify-content: center;
  z-index: 10000;
}
.modal-dialog {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  padding: 24px;
  max-width: 460px;
  width: 90%;
  box-shadow: var(--shadow-lg);
}
.modal-title { font-size: 16px; font-weight: 700; margin-bottom: 18px; }
.modal-field { margin-bottom: 14px; }
.modal-field label { display: block; font-size: 14px; font-weight: 500; margin-bottom: 4px; }
.modal-field input {
  width: 100%; box-sizing: border-box;
  padding: 6px 10px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  background: var(--surface);
  color: var(--fg);
  font-size: 14px;
  outline: none;
  transition: border-color 0.15s, box-shadow 0.15s;
  height: 34px;
}
.modal-field input:focus { border-color: var(--accent); box-shadow: var(--focus-ring); }
.modal-presets { display: flex; align-items: center; flex-wrap: wrap; gap: 4px; margin-bottom: 18px; }
.modal-actions { display: flex; gap: 8px; justify-content: flex-end; }

/* 挑选弹窗：大尺寸 + 分厂商分组 */
.pick-dialog {
  width: min(960px, 92vw); max-width: none; height: 85vh;
  display: flex; flex-direction: column;
}
.pick-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 12px; }
.pick-head .modal-title { margin-bottom: 0; }
.pick-close {
  width: 30px; height: 30px; border: none; border-radius: var(--radius-md);
  background: transparent; color: var(--fg-dim); font-size: 20px; line-height: 1;
  cursor: pointer; transition: background 0.1s, color 0.1s;
}
.pick-close:hover { background: var(--danger-soft); color: var(--danger); }
.pick-toolbar { display: flex; gap: 6px; align-items: center; margin-bottom: 10px; flex-shrink: 0; }
.pick-tool { font-size: 12px; padding: 3px 10px; white-space: nowrap; }
.pick-tool.on { background: color-mix(in srgb, var(--accent) 18%, transparent); color: var(--accent); border-color: var(--accent); }
.pick-search { flex: 1; box-sizing: border-box; padding: 5px 10px; height: 30px; border: 1px solid var(--border-strong); border-radius: var(--radius-md); background: var(--surface); color: var(--fg); font-size: 13px; outline: none; }
.pick-search:focus { border-color: var(--accent); box-shadow: var(--focus-ring); }
.pick-list { overflow-y: auto; border: 1px solid var(--border); border-radius: var(--radius-md); flex: 1; min-height: 0; }
.pick-group { border-bottom: 1px solid var(--border); }
.pick-group:last-child { border-bottom: none; }
.pick-group-head {
  position: sticky; top: 0; z-index: 2;
  display: flex; align-items: center; gap: 8px;
  padding: 7px 12px; cursor: pointer; user-select: none;
  background: var(--surface-soft); border-bottom: 1px solid var(--border);
}
.pg-caret { font-size: 11px; width: 14px; color: var(--fg-dim); }
.pg-name { font-weight: 700; font-size: 13px; text-transform: capitalize; }
.pg-count { font-size: 12px; }
.pg-actions { margin-left: auto; display: flex; gap: 4px; }
.pg-btn {
  white-space: nowrap;
  padding: 1px 9px; font-size: 11px; border-radius: var(--radius-sm);
  border: 1px solid var(--border-strong); background: var(--surface); color: var(--fg);
  cursor: pointer; transition: all 0.1s;
}
.pg-btn:hover { border-color: var(--accent); color: var(--accent); }
.pick-row { display: flex; align-items: center; gap: 8px; padding: 6px 12px 6px 34px; border-bottom: 1px solid var(--border); cursor: pointer; font-size: 13px; }
.pick-row:last-child { border-bottom: none; }
.pick-row:hover { background: var(--surface-soft); }
.pick-row input[type="checkbox"] { width: 15px; height: 15px; accent-color: var(--accent); flex-shrink: 0; }
.pick-id { font-weight: 600; white-space: nowrap; }
.pick-meta { font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: "SF Mono", "Menlo", monospace; }
.pick-foot {
  display: flex; align-items: center; justify-content: space-between; gap: 12px;
  padding-top: 12px; flex-shrink: 0; font-size: 13px;
}

.key-input-wrap { position: relative; display: flex; align-items: center; }
.key-input-wrap input { flex: 1; padding-right: 28px; border: 1px solid var(--border-strong); border-radius: var(--radius-md); background: var(--surface); color: var(--fg); outline: none; font-size: 14px; font-family: "SF Mono", "Menlo", monospace; height: 34px; padding: 4px 28px 4px 10px; transition: border-color 0.15s, box-shadow 0.15s; }
.key-input-wrap input:focus { border-color: var(--accent); box-shadow: var(--focus-ring); }
.key-clear { position: absolute; right: 8px; top: 50%; transform: translateY(-50%); cursor: pointer; font-size: 17px; color: var(--fg-dim); width: 20px; height: 20px; display: flex; align-items: center; justify-content: center; border-radius: 50%; transition: background 0.1s, color 0.1s; user-select: none; }
.key-clear:hover { background: var(--danger-soft); color: var(--danger); }
.detected-badge { font-size: 10px; padding: 0 5px; border-radius: 3px; background: var(--success-soft); color: var(--success-fg); font-weight: 500; }
.key-env { font-size: 12px; margin-left: 4px; }

.sticky-save { position: sticky; top: 0; z-index: 5; display: flex; justify-content: flex-end; margin-bottom: 12px; padding: 10px 14px; background: var(--surface); border-bottom: 1px solid var(--border); margin-left: -18px; margin-right: -18px; }

.key-group { margin-bottom: 24px; }
.key-group:last-child { margin-bottom: 0; }
.key-group-label { font-size: 14px; font-weight: 700; color: var(--fg-muted); margin-bottom: 10px; padding-bottom: 6px; border-bottom: 1px solid var(--border); }
.key-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(230px, 1fr)); gap: 10px 16px; }
.key-grid .field { margin-bottom: 4px; }
.key-grid .field label { font-size: 14px; display: flex; align-items: baseline; gap: 6px; margin-bottom: 3px; }
.key-grid .field .help { font-size: 12px; display: block; margin-top: 2px; }
</style>
