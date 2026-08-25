<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type { AppConfig, ModelDef } from "../types/models";
import { addCustomModel, updateCustomModel, deleteCustomModel, knownProviders, checkModelUpdates, setModelRouting } from "../ipc/api";
import { useToast } from "../composables/useToast";
import { useAppConfig } from "../composables/useAppConfig";

const props = defineProps<{ config: AppConfig | null }>();
const toast = useToast();
const { refresh } = useAppConfig();

const providers = ref<string[]>([]);
const checking = ref(false);
onMounted(async () => {
  try { providers.value = await knownProviders(); } catch { /* fallback below */ }
});

function providerLabel(p: string): string {
  const m: Record<string, string> = {
    deepseek: "DeepSeek", glm: "智谱 GLM", qwen: "阿里 Qwen",
    qwen38: "阿里 Qwen3.8", xiaomi: "小米 MiMo",
    anthropic: "Anthropic", openai: "OpenAI", gemini: "Google Gemini",
    moonshot: "月之暗面 Kimi", longcat: "美团 LongCat",
  };
  return m[p] || p;
}

// ── 全局线路（对所有工具生效）─────────────────────────────
const routingBusy = ref<Set<string>>(new Set());
function routingFor(slug: string): string {
  return props.config?.model_routing?.[slug] ?? "direct";
}
async function onRoutingChange(slug: string, ev: Event) {
  if (!props.config) return;
  const routing = (ev.target as HTMLSelectElement).value;
  routingBusy.value = new Set([...routingBusy.value, slug]);
  try {
    await setModelRouting(props.config, slug, routing);
    await refresh();
    toast.ok(routing === "direct" ? `「${slug}」已切回直连` : `「${slug}」已改走 ${routing.slice(6)}——所有工具立即生效`);
  } catch (e: any) { toast.err(e?.message ?? String(e)); }
  finally {
    const next = new Set(routingBusy.value);
    next.delete(slug);
    routingBusy.value = next;
  }
}

// 内置模型不可删改（slug 对照后端 builtin_models）
const BUILTIN_SLUGS = new Set([
  "deepseek-v4-pro", "deepseek-v4-flash", "glm-5.2", "qwen3.8-max-preview", "qwen-max",
  "mimo-v2.5-pro", "mimo-v2.5", "claude-opus-5", "gpt-5.6",
  "gemini-3-flash-preview", "gemini-2.5-pro", "glm-5.3", "qwen3.7-max", "qwen3.8-max",
]);
function isBuiltin(slug: string): boolean { return BUILTIN_SLUGS.has(slug); }

function fmtTokens(n: number): string { return n >= 1_000_000 ? `${(n/1_000_000).toFixed(1)}M` : n >= 1_000 ? `${(n/1_000).toFixed(0)}K` : String(n); }

// 同厂商聚在一起展示：provider 字典序分组，组内按 priority 再 slug
function byProvider(a: ModelDef, b: ModelDef): number {
  if (a.provider !== b.provider) return a.provider.localeCompare(b.provider);
  if (a.priority !== b.priority) return a.priority - b.priority;
  return a.slug.localeCompare(b.slug);
}
const sortedModels = computed(() => [...(props.config?.models ?? [])].sort(byProvider));

// ── Modal ──────────────────────────────────────────────────

const emptyForm = (): ModelDef => ({
  slug: "", display_name: "", provider: "deepseek", enabled: true,
  context_window: 131072, max_output_tokens: 16384, priority: 500,
  default_reasoning_level: "high", supports_reasoning_summaries: true,
  input_price_per_1k: 0, output_price_per_1k: 0,
});
const form = ref<ModelDef>(emptyForm());
const oldSlug = ref("");
const showModelModal = ref(false);
const busy = ref(false);

function startNewModel() { form.value = emptyForm(); oldSlug.value = ""; showModelModal.value = true; }
function startEditModel(m: ModelDef) { form.value = { ...m }; oldSlug.value = m.slug; showModelModal.value = true; }
function cancelModel() { form.value = emptyForm(); oldSlug.value = ""; showModelModal.value = false; }

async function onSaveModel() {
  if (!props.config) return;
  if (!form.value.slug.trim() || !form.value.display_name.trim()) { toast.err("slug 和显示名不能为空"); return; }
  busy.value = true;
  try {
    if (oldSlug.value) await updateCustomModel(props.config, oldSlug.value, form.value);
    else await addCustomModel(props.config, form.value);
    await refresh();
    toast.ok(oldSlug.value ? `模型「${form.value.slug}」已更新` : `模型「${form.value.slug}」已添加（默认直连，去首页勾选启用）`);
    cancelModel();
  } catch (e: any) { toast.err(e?.message ?? String(e)); }
  finally { busy.value = false; }
}

async function onDeleteModel(slug: string) {
  if (!props.config) return;
  try { await deleteCustomModel(props.config, slug); await refresh(); toast.ok(`模型「${slug}」已删除`); }
  catch (e: any) { toast.err(e?.message ?? String(e)); }
}

// ── Catalog update ─────────────────────────────────────────

async function onCheckUpdates() {
  checking.value = true;
  try {
    const r = await checkModelUpdates();
    await refresh();
    toast.ok(r.new_models > 0 ? `发现 ${r.new_models} 个新模型：${r.new_slugs.join(", ")}` : "已是最新目录");
  } catch (e: any) { toast.err(e?.message ?? String(e)); }
  finally { checking.value = false; }
}
</script>

<template>
  <section class="page">
    <header class="page-header">
      <h2>模型管理</h2>
      <span class="desc dim">
        目录版本 v{{ config?.model_catalog_version ?? 0 }} · 新模型由远端目录下发或自行添加
      </span>
      <button class="btn" style="margin-left:auto" :disabled="checking" @click="onCheckUpdates">{{ checking ? '检查中…' : '检查更新' }}</button>
    </header>

    <div class="card mt12" v-if="config">
      <div class="table-wrap">
        <table class="param-table">
          <thead>
            <tr>
              <th>厂商</th><th>模型</th><th>slug</th><th>上下文</th><th>最大输出</th>
              <th>入价 /1K</th><th>出价 /1K</th><th>Responses</th><th title="决定该模型走官方直连还是某个中转站；对所有工具生效">线路<span class="dim">(全局)</span></th><th style="width:130px"></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="m in sortedModels" :key="m.slug">
              <td class="provider-cell">{{ providerLabel(m.provider) }}</td>
              <td class="name-cell">{{ m.display_name }}<span v-if="!m.enabled" class="dim">（停用）</span></td>
              <td><code>{{ m.slug }}</code></td>
              <td>{{ fmtTokens(m.context_window) }}</td>
              <td>{{ fmtTokens(m.max_output_tokens) }}</td>
              <td>${{ m.input_price_per_1k.toFixed(4) }}</td>
              <td>${{ m.output_price_per_1k.toFixed(4) }}</td>
              <td>{{ m.native_responses ? '✓' : '—' }}</td>
              <td>
                <select class="route-select" :disabled="routingBusy.has(m.slug)" :value="routingFor(m.slug)" @change="onRoutingChange(m.slug, $event)">
                  <option value="direct">直连</option>
                  <option v-for="r in (config?.relays || [])" :key="r.name" :value="'relay:' + r.name">{{ r.name }}</option>
                </select>
              </td>
              <td class="actions-cell">
                <template v-if="!isBuiltin(m.slug)">
                  <button class="btn ghost" @click="startEditModel(m)">编辑</button>
                  <button class="btn ghost" style="color:var(--danger)" @click="onDeleteModel(m.slug)">删除</button>
                </template>
                <span v-else class="dim" style="font-size:12px">内置</span>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <div style="padding:10px 18px 16px">
        <button class="btn" @click="startNewModel">+ 添加自定义模型</button>
        <span class="desc dim" style="margin-left:10px">内置模型不可删除；中转站独有模型建议加在这里</span>
      </div>
    </div>
    <div v-else class="dim" style="padding:48px;text-align:center">Loading…</div>

    <!-- 模型弹窗 -->
    <div v-if="showModelModal" class="modal-overlay" @click.self="cancelModel">
      <div class="modal-dialog">
        <div class="modal-title">{{ oldSlug ? '编辑自定义模型' : '添加自定义模型' }}</div>

        <div class="modal-field"><label>显示名</label>
          <input v-model="form.display_name" type="text" placeholder="如 Qwen3.7 Plus" /></div>

        <div class="modal-field"><label>slug <span class="dim" style="font-weight:400">(调用时的模型 ID)</span></label>
          <input v-model="form.slug" type="text" placeholder="如 qwen3.7-plus" style="font-family:monospace" :disabled="!!oldSlug" /></div>

        <div class="modal-field"><label>厂商 <span class="dim" style="font-weight:400">(决定直连端点与 API Key 槽位)</span></label>
          <select v-model="form.provider">
            <option v-for="p in providers" :key="p" :value="p">{{ providerLabel(p) }}</option>
          </select></div>

        <div class="modal-row">
          <div class="modal-field"><label>上下文窗口 (tokens)</label>
            <input v-model.number="form.context_window" type="number" min="1024" /></div>
          <div class="modal-field"><label>最大输出 (tokens)</label>
            <input v-model.number="form.max_output_tokens" type="number" min="256" /></div>
        </div>

        <div class="modal-row">
          <div class="modal-field"><label>入价 $/1K <span class="dim" style="font-weight:400">(可选)</span></label>
            <input v-model.number="form.input_price_per_1k" type="number" step="0.0001" min="0" /></div>
          <div class="modal-field"><label>出价 $/1K <span class="dim" style="font-weight:400">(可选)</span></label>
            <input v-model.number="form.output_price_per_1k" type="number" step="0.0001" min="0" /></div>
        </div>

        <div class="modal-field checkbox-field">
          <label style="display:flex;align-items:center;gap:8px;font-weight:400">
            <input type="checkbox" v-model="form.native_responses" />
            原生支持 OpenAI Responses API<span class="dim">(Codex 直连所需；不确定就留空)</span>
          </label>
        </div>

        <div class="modal-actions">
          <button class="btn" @click="cancelModel">取消</button>
          <button class="btn primary" :disabled="busy" @click="onSaveModel">保存</button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.desc { font-size: 13px; margin-right: 10px; }
.page-header { display:flex; align-items:center; }
.table-wrap { overflow-x: auto; padding: 6px 18px; }
.param-table { width: 100%; border-collapse: collapse; font-size: 13px; }
.param-table th { text-align: left; color: var(--fg-muted); font-weight: 600; padding: 8px 10px; border-bottom: 1px solid var(--border); white-space: nowrap; }
.param-table td { padding: 7px 10px; border-bottom: 1px solid var(--border); white-space: nowrap; }
.param-table tr:last-child td { border-bottom: none; }
.route-select { padding: 3px 7px; font-size: 12px; border: 1px solid var(--border-strong); border-radius: var(--radius-md); background: var(--surface); color: var(--fg); outline: none; cursor: pointer; }
.route-select:focus { border-color: var(--accent); box-shadow: var(--focus-ring); }
.provider-cell { font-weight: 600; }
.name-cell { font-weight: 500; }
.actions-cell { display: flex; gap: 4px; }

.modal-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 10000; }
.modal-dialog { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-lg); padding: 24px; max-width: 480px; width: 90%; box-shadow: var(--shadow-lg); }
.modal-title { font-size: 16px; font-weight: 700; margin-bottom: 18px; }
.modal-field { margin-bottom: 14px; flex: 1; }
.modal-row { display: flex; gap: 12px; }
.modal-field label { display: block; font-size: 14px; font-weight: 500; margin-bottom: 4px; }
.modal-field input, .modal-field select {
  width: 100%; box-sizing: border-box; padding: 6px 10px;
  border: 1px solid var(--border-strong); border-radius: var(--radius-md);
  background: var(--surface); color: var(--fg); font-size: 14px; outline: none; height: 34px;
}
.modal-field input:focus, .modal-field select:focus { border-color: var(--accent); box-shadow: var(--focus-ring); }
.checkbox-field input { width: auto; height: auto; }
.modal-actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 18px; }
</style>
