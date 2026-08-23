<script setup lang="ts">
import { computed, ref } from "vue";
import type { AppConfig } from "../types/models";
import { addAlias, updateAlias, deleteAlias, copyToClipboard } from "../ipc/api";
import { useToast } from "../composables/useToast";
import { useAppConfig } from "../composables/useAppConfig";

const props = defineProps<{ config: AppConfig | null }>();
const toast = useToast();
const { refresh } = useAppConfig();

// ── Options ────────────────────────────────────────────────

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

// 联动过滤：直连时只列协议上可行的组合；中转站可承载任意模型，列全部。
// 联动过滤：直连时只列协议上可行的组合；中转站可承载任意模型，列全部。
// 同厂商聚在一起：provider 分组 + priority 排序，避免新合并的模型散落列表。
const modelsFor = computed(() => {
  const all = (props.config?.models ?? []).filter(m => m.enabled);
  const tool = form.value.tool;
  const source = form.value.source;
  return all.filter(m => {
    if (source !== "direct") return true;
    if (tool === "claude_cli") return m.provider !== "openai";
    if (tool === "codex_cli")  return !!m.native_responses;
    if (tool === "aider")      return m.provider !== "anthropic";
    if (tool === "pi")         return m.provider !== "anthropic"; // pi 官方 anthropic 无需别名
    if (tool === "opencode")   return m.provider !== "anthropic";
    if (tool === "hermes")     return m.provider !== "anthropic";
    return true;
  }).sort((a, b) => {
    if (a.provider !== b.provider) return a.provider.localeCompare(b.provider);
    if (a.priority !== b.priority) return a.priority - b.priority;
    return a.slug.localeCompare(b.slug);
  });
});

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

// ── Inline form state ──────────────────────────────────────

const emptyForm = (): { name: string; tool: string; model: string; source: string } =>
  ({ name: "", tool: "claude_cli", model: "", source: "direct" });
const form = ref(emptyForm());
const editingName = ref(""); // 非空 = 修改模式
const busy = ref(false);

function syncModelDefault() {
  const ok = modelsFor.value.some(m => m.slug === form.value.model);
  if (!ok) form.value.model = modelsFor.value[0]?.slug ?? "";
}

function startEditAlias(a: { name: string; tool: string; model: string; source: string }) {
  form.value = { name: a.name, tool: a.tool, model: a.model, source: a.source };
  editingName.value = a.name;
}

function cancelEdit() {
  form.value = emptyForm();
  editingName.value = "";
}

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
  if (editingName.value === name) cancelEdit(); // 正在编辑的行被删 → 回到添加模式
  try { await deleteAlias(props.config, name); await refresh(); toast.ok(`别名「${name}」已删除`); }
  catch (e: any) { toast.err(e?.message ?? String(e)); }
}

async function onCopyName(name: string) {
  try { await copyToClipboard(name); toast.ok(`已复制「${name}」（区分大小写）— 新开终端粘贴回车即用`); }
  catch (e: any) { toast.err(e?.message ?? String(e)); }
}

// ── List ───────────────────────────────────────────────────

const aliases = computed(() => {
  const list = [...(props.config?.custom_aliases ?? [])];
  list.reverse(); // 最新在最上
  return list;
});
</script>

<template>
  <section class="page">
    <header class="page-header">
      <h2>别名</h2>
      <span class="desc dim">Shell 集成的补充：那里是首页分配自动生成的标准命令，这里是自定义快捷方式——自由组合 工具×模型×来源，支持同一工具多窗口各走各的源</span>
    </header>

    <!-- 内联添加/编辑表单 -->
    <div class="card">
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
      <div class="form-hint dim">
        区分大小写 · 已开着的终端需执行 <code>source ~/.zshrc</code> 或新开终端 · 用量统计照常记录
      </div>
    </div>

    <!-- 单行列表 -->
    <div class="card mt12">
      <div v-if="aliases.length > 0" class="alias-list">
        <div v-for="a in aliases" :key="a.name" class="alias-row" :class="{ editing: editingName === a.name }">
          <span class="alias-name">{{ a.name }}</span>
          <span class="alias-desc dim">{{ toolLabel(a.tool) }} · {{ modelLabel(a.model) }} · {{ sourceLabel(a.source) }}</span>
          <span class="alias-actions">
            <button class="btn primary" @click="onCopyName(a.name)">复制</button>
            <button class="btn ghost" @click="startEditAlias(a)">修改</button>
            <button class="btn ghost" style="color:var(--danger)" @click="onDeleteAlias(a.name)">删除</button>
          </span>
        </div>
      </div>
      <div v-else class="dim empty-hint">还没有别名——在上面填一个，点「+ 添加」试试</div>
    </div>
  </section>
</template>

<style scoped>
.inline-form { display: flex; gap: 12px; align-items: flex-end; padding: 14px 18px 4px; flex-wrap: wrap; }
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
.form-hint { padding: 2px 18px 12px; font-size: 12px; line-height: 1.6; }
.form-hint code { background: var(--surface-soft); padding: 1px 5px; border-radius: 3px; }

.alias-list {}
.alias-row { display: flex; align-items: center; gap: 14px; padding: 9px 18px; border-bottom: 1px solid var(--border); font-size: 14px; }
.alias-row:last-child { border-bottom: none; }
.alias-row.editing { background: var(--surface-soft); }
.alias-name { font-weight: 700; font-family: "SF Mono", "Menlo", monospace; font-size: 15px; min-width: 60px; }
.alias-desc { flex: 1; font-size: 13px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.alias-actions { display: flex; gap: 4px; flex-shrink: 0; }
.empty-hint { padding: 24px 18px; font-size: 14px; text-align: center; }
.mt12 { margin-top: 12px; }
</style>
