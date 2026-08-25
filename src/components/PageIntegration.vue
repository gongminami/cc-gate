<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { AppConfig } from "../types/models";
import { addAlias, updateAlias, deleteAlias, copyToClipboard, applyAgentConfig } from "../ipc/api";
import { useAppConfig } from "../composables/useAppConfig";
import { useToast } from "../composables/useToast";

const props = defineProps<{ config: AppConfig | null }>();
const toast = useToast();
const { refresh } = useAppConfig();

// ── Unified gateway commands ────────────────────────────────
const UNIFIED = [
  { tool: "Claude Code",  cmd: "claude-cc-gate", note: "/model 里切换全部模型（官方 + 厂商直连 + 中转站）" },
  { tool: "Codex CLI",    cmd: "codex-cc-gate",  note: "完整模型目录，Codex 内 /model 切换" },
  { tool: "Aider",        cmd: "aider-cc-gate",  note: "以第一个启用模型启动；换模型用高级别名" },
  { tool: "Hermes",       cmd: "hermes-cc-gate", note: "--provider ccgate，可用 -m 随时切模型" },
  { tool: "PI",           cmd: "pi-cc-gate",     note: "全部启用模型已自动写入 ~/.pi/agent/models.json" },
];

const FILE_BASED = [
  { tool: "Codex 桌面端 / Claude 桌面端 / Cursor", desc: "在「运行状态」页勾选并应用" },
  { tool: "OpenCode",      desc: "自动写入 opencode.jsonc 的 ccgate provider (:8690)" },
  { tool: "OpenClaw",      desc: "自动写入配置，模型走 chat-proxy (:8690)" },
  { tool: "Codex 桌面端 / Claude 桌面端 / Cursor", desc: "在「运行状态」页勾选并应用" },
];

async function onCopy(cmd: string) {
  try { await copyToClipboard(cmd); toast.ok(`已复制「${cmd}」——新开终端粘贴回车即用`); }
  catch (e: any) { toast.err(e?.message ?? String(e)); }
}

const applying = ref(false);
async function onApplyShell() {
  if (!props.config || applying.value) return;
  applying.value = true;
  try {
    const r = await applyAgentConfig(props.config);
    toast.ok(r.restarted_proxies?.length > 0 ? `已写入，重启：${r.restarted_proxies.join('、')}` : '已写入');
  } catch (e: any) { toast.err(e?.message ?? String(e)); }
  finally { applying.value = false; }
}

const showAdvanced = ref(false);

// ── Advanced: multi-window source pinning (原别名功能) ──
const TOOLS = [
  { id: "claude_cli", label: "Claude Code" },
  { id: "codex_cli",  label: "Codex CLI" },
  { id: "aider",      label: "Aider" },
  { id: "pi",         label: "PI" },
  { id: "opencode",   label: "OpenCode" },
  { id: "hermes",     label: "Hermes" },
];

const sources = computed(() => [
  { id: "direct", label: "直连" },
  ...(props.config?.relays ?? []).map(r => ({ id: `relay:${r.name}`, label: `中转:${r.name}` })),
]);

const modelsFor = computed(() => {
  const all = (props.config?.models ?? []).filter(m => m.enabled);
  const tool = form.value.tool;
  const source = form.value.source;
  return all.filter(m => {
    if (source !== "direct") return true;
    if (tool === "claude_cli") return m.provider !== "openai";
    if (tool === "codex_cli")  return !!m.native_responses;
    if (tool === "aider")      return m.provider !== "anthropic";
    if (tool === "pi")         return m.provider !== "anthropic";
    if (tool === "opencode")   return m.provider !== "anthropic";
    if (tool === "hermes")     return m.provider !== "anthropic";
    return true;
  }).sort((a, b) => {
    if (a.provider !== b.provider) return a.provider.localeCompare(b.provider);
    if (a.priority !== b.priority) return a.priority - b.priority;
    return a.slug.localeCompare(b.slug);
  });
});

function syncModelDefault() {
  const ok = modelsFor.value.some(m => m.slug === form.value.model);
  if (!ok) form.value.model = modelsFor.value[0]?.slug ?? "";
}
function modelLabel(slug: string): string {
  const m = props.config?.models.find(x => x.slug === slug);
  return m ? m.display_name : slug;
}
function sourceLabel(s: string): string {
  if (s === "direct") return "直连";
  return s.startsWith("relay:") ? `中转:${s.slice(6)}` : s;
}
function toolLabel(t: string): string {
  return TOOLS.find(x => x.id === t)?.label ?? t;
}

const emptyForm = (): { name: string; tool: string; model: string; source: string } =>
  ({ name: "", tool: "claude_cli", model: "", source: "direct" });
const form = ref(emptyForm());
const editingName = ref("");
const busy = ref(false);
// Default the model picker once, then keep it valid whenever tool/source changes
// (the selects in the template @change into these setters).
watch([() => form.value.tool, () => form.value.source], () => syncModelDefault());
syncModelDefault();

function startEditAlias(a: { name: string; tool: string; model: string; source: string }) {
  form.value = { name: a.name, tool: a.tool, model: a.model, source: a.source };
  editingName.value = a.name;
}
function cancelEdit() { form.value = emptyForm(); editingName.value = ""; }

const NAME_RE = /^[A-Za-z][A-Za-z0-9_-]{1,31}$/;

async function onSave() {
  if (!props.config) return;
  const { name, tool, model, source } = form.value;
  const n = name.trim();
  if (!NAME_RE.test(n)) { toast.err("别名需以字母开头，2~32 位字母/数字/_/-"); return; }
  if (!model) { toast.err("请选择大模型"); return; }
  busy.value = true;
  try {
    if (editingName.value) await updateAlias(props.config, editingName.value, n, tool, model, source);
    else await addAlias(props.config, n, tool, model, source);
    await refresh();
    toast.ok(editingName.value ? `别名「${n}」已更新` : `别名「${n}」已生效——新开终端粘贴回车即用`);
    cancelEdit();
  } catch (e: any) { toast.err(e?.message ?? String(e)); }
  finally { busy.value = false; }
}

async function onDeleteAlias(name: string) {
  if (!props.config) return;
  if (editingName.value === name) cancelEdit();
  try { await deleteAlias(props.config, name); await refresh(); toast.ok(`别名「${name}」已删除`); }
  catch (e: any) { toast.err(e?.message ?? String(e)); }
}
</script>

<template>
  <section class="page">
    <header class="page-header">
      <h2>CLI 接入</h2>
      <span class="desc dim">每个工具一条统一命令：打开就是全量模型列表（官方 + 厂商直连 + 中转站），切模型在工具内完成。添加/修改中转站或挑选后立即生效。</span>
    </header>

    <div class="card">
      <div class="card-head">统一命令</div>
      <div class="card-body">
        <div v-for="u in UNIFIED" :key="u.cmd" class="uc-row">
          <span class="uc-tool">{{ u.tool }}</span>
          <code class="uc-cmd">{{ u.cmd }}</code>
          <button class="btn primary uc-copy" @click="onCopy(u.cmd)">复制</button>
          <span class="uc-note dim">{{ u.note }}</span>
        </div>
        <p class="dim uc-hint">裸命令（claude / codex / aider）保持官方原生直连，不受 CC-Gate 影响。新开终端即可使用。</p>
        <div style="margin-top:10px">
          <button class="btn primary" :disabled="applying" @click="onApplyShell">{{ applying ? "写入中…" : "写入 Shell 配置并应用" }}</button>
          <span class="dim" style="font-size:12px;margin-left:8px">把统一命令写入 ~/.zshrc 并重启三个代理</span>
        </div>
      </div>
    </div>

    <div class="card mt12">
      <div class="card-head">配置文件接入（无需命令行）</div>
      <div class="card-body">
        <div v-for="f in FILE_BASED" :key="f.tool" class="fb-row">
          <span class="fb-tool">{{ f.tool }}</span>
          <span class="dim">{{ f.desc }}</span>
        </div>
      </div>
    </div>

    <div class="card mt12">
      <div class="adv-head" @click="showAdvanced = !showAdvanced">
        <span>{{ showAdvanced ? "▾" : "▸" }} 高级：多窗口钉源（别名）</span>
        <span class="dim adv-hint">把某个窗口锁死在一个来源上 / 同一模型多来源并行 —— 普通使用不需要</span>
      </div>
      <div v-if="showAdvanced" class="card-body">
        <div class="inline-form">
          <div class="field">
            <label>别名</label>
            <input v-model="form.name" class="alias-input" type="text" placeholder="如 dsf" />
          </div>
          <div class="field">
            <label>工具</label>
            <select v-model="form.tool" @change="syncModelDefault">
              <option v-for="t in TOOLS" :key="t.id" :value="t.id">{{ t.label }}</option>
            </select>
          </div>
          <div class="field">
            <label>大模型</label>
            <select v-model="form.model">
              <option v-for="m in modelsFor" :key="m.slug" :value="m.slug">{{ m.display_name }}</option>
            </select>
          </div>
          <div class="field">
            <label>来源</label>
            <select v-model="form.source" @change="syncModelDefault">
              <option v-for="s in sources" :key="s.id" :value="s.id">{{ s.label }}</option>
            </select>
          </div>
          <div class="field btn-field">
            <label>&nbsp;</label>
            <button class="btn primary" :disabled="busy || !form.name.trim() || !form.model" @click="onSave">
              {{ editingName ? '保存修改' : '+ 添加' }}
            </button>
            <button v-if="editingName" class="btn ghost cancel-btn" :disabled="busy" @click="cancelEdit">取消</button>
          </div>
        </div>

        <div v-if="(config?.custom_aliases || []).length > 0" class="alias-list">
          <div v-for="a in [...(config?.custom_aliases || [])].reverse()" :key="a.name" class="alias-row" :class="{ editing: editingName === a.name }">
            <span class="alias-name">{{ a.name }}</span>
            <span class="alias-desc dim">{{ toolLabel(a.tool) }} · {{ modelLabel(a.model) }} · {{ sourceLabel(a.source) }}</span>
            <span class="alias-actions">
              <button class="btn primary" @click="onCopy(`ccgate-${a.name}`)">复制命令名</button>
              <button class="btn ghost" @click="startEditAlias(a)">修改</button>
              <button class="btn ghost" style="color:var(--danger)" @click="onDeleteAlias(a.name)">删除</button>
            </span>
          </div>
        </div>
        <div v-else-if="config" class="dim empty-hint">还没有自定义别名</div>

        <p class="dim uc-hint">别名窗口的 /model 只显示其来源携带的模型；区分大小写。已开着的终端需 source ~/.zshrc 或新开终端。</p>
      </div>
    </div>
  </section>
</template>

<style scoped>
.page-header { display: block; }
.page-header h2 { margin: 0; }
.page-desc { font-size: 13px; line-height: 1.7; margin: 8px 0 0; text-align: left; }
.uc-row {
  display: grid;
  grid-template-columns: 130px 180px 72px 1fr;
  gap: 12px; align-items: center;
  padding: 9px 0; border-bottom: 1px solid var(--border);
}
.uc-row:last-of-type { border-bottom: none; }
.uc-tool { font-weight: 600; font-size: 14px; white-space: nowrap; }
.uc-cmd, .uc-copy { white-space: nowrap; justify-self: start; }
.uc-cmd { font-family: "SF Mono", "Menlo", monospace; font-weight: 700; color: var(--accent); background: var(--surface-soft); padding: 3px 10px; border-radius: var(--radius-md); }
.uc-copy { padding: 3px 14px; font-size: 12px; }
.uc-note { font-size: 12px; flex: 1; min-width: 200px; }
.uc-hint { font-size: 12px; margin: 10px 0 0; line-height: 1.6; }

.fb-row { display: flex; align-items: baseline; gap: 12px; padding: 7px 0; border-bottom: 1px solid var(--border); font-size: 14px; flex-wrap: wrap; }
.fb-row:last-child { border-bottom: none; }
.fb-tool { font-weight: 600; min-width: 260px; white-space: nowrap; }

.adv-head { display: flex; align-items: baseline; gap: 10px; padding: 14px 18px; cursor: pointer; user-select: none; font-weight: 700; font-size: 15px; }
.adv-head:hover { color: var(--accent); }
.adv-hint { font-size: 12px; font-weight: 400; }

.inline-form { display: flex; gap: 12px; align-items: flex-end; padding: 4px 0 10px; flex-wrap: wrap; }
.field { display: flex; flex-direction: column; gap: 4px; }
.field label { font-size: 12px; color: var(--fg-muted); font-weight: 600; }
.alias-input { width: 130px; font-family: "SF Mono", "Menlo", monospace; font-weight: 600; }
.inline-form input, .inline-form select {
  box-sizing: border-box; padding: 6px 10px; height: 34px;
  border: 1px solid var(--border-strong); border-radius: var(--radius-md);
  background: var(--surface); color: var(--fg); font-size: 14px; outline: none;
  transition: border-color 0.15s, box-shadow 0.15s;
}
.inline-form input:focus, .inline-form select:focus { border-color: var(--accent); box-shadow: var(--focus-ring); }
.btn-field { flex-direction: row; gap: 6px; align-items: flex-end; }
.cancel-btn { height: 34px; }

.alias-list { margin-top: 4px; }
.alias-row { display: flex; align-items: center; gap: 14px; padding: 9px 4px; border-bottom: 1px solid var(--border); font-size: 14px; }
.alias-row:last-child { border-bottom: none; }
.alias-row.editing { background: var(--surface-soft); }
.alias-name { font-weight: 700; font-family: "SF Mono", "Menlo", monospace; font-size: 15px; min-width: 60px; }
.alias-desc { flex: 1; font-size: 13px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.alias-actions { display: flex; gap: 4px; flex-shrink: 0; }
.empty-hint { padding: 16px 0; font-size: 13px; text-align: center; }
.mt12 { margin-top: 12px; }
.card-head { padding: 14px 18px 0; font-size: 16px; font-weight: 700; }
.card-body { padding: 8px 18px 16px; }
</style>
