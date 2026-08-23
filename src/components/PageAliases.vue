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
];

const sources = computed(() => [
  { id: "direct", label: "直连（官方 API）" },
  ...(props.config?.relays ?? []).map(r => ({ id: `relay:${r.name}`, label: `中转站：${r.name}` })),
]);

// 联动过滤：直连时只列协议上可行的组合，避免配出必死的搭配；
// 中转站可承载任意模型（本地无每站目录），列全部。
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
    return true;
  });
});

function modelLabel(slug: string): string {
  const m = props.config?.models.find(x => x.slug === slug);
  return m ? m.display_name : slug;
}
function sourceLabel(s: string): string {
  if (s === "direct") return "直连";
  return s.startsWith("relay:") ? `中转：${s.slice(6)}` : s;
}
function toolLabel(t: string): string {
  return TOOLS.find(x => x.id === t)?.label ?? t;
}

// ── Modal state ────────────────────────────────────────────

const emptyForm = (): { oldName: string; name: string; tool: string; model: string; source: string } =>
  ({ oldName: "", name: "", tool: "claude_cli", model: "", source: "direct" });
const form = ref(emptyForm());
const showAliasModal = ref(false);
const busy = ref(false);

function syncModelDefault() {
  const ok = modelsFor.value.some(m => m.slug === form.value.model);
  if (!ok) form.value.model = modelsFor.value[0]?.slug ?? "";
}

function startNewAlias() {
  form.value = emptyForm();
  syncModelDefault();
  showAliasModal.value = true;
}

function startEditAlias(a: { name: string; tool: string; model: string; source: string }) {
  form.value = { oldName: a.name, name: a.name, tool: a.tool, model: a.model, source: a.source };
  showAliasModal.value = true;
}

function cancelAlias() {
  form.value = emptyForm();
  showAliasModal.value = false;
}

const NAME_RE = /^[A-Za-z][A-Za-z0-9_-]{1,31}$/;

async function onSaveAlias() {
  if (!props.config) return;
  const { oldName, name, tool, model, source } = form.value;
  const n = name.trim();
  if (!NAME_RE.test(n)) { toast.err("别名需以字母开头，2~32 位字母/数字/_/-"); return; }
  if (!model) { toast.err("请选择大模型"); return; }
  busy.value = true;
  try {
    if (oldName) await updateAlias(props.config, oldName, n, tool, model, source);
    else await addAlias(props.config, n, tool, model, source);
    await refresh();
    toast.ok(oldName ? `别名「${n}」已更新` : `别名「${n}」已生效——新开终端即可使用`);
    cancelAlias();
  } catch (e: any) { toast.err(e?.message ?? String(e)); }
  finally { busy.value = false; }
}

async function onDeleteAlias(name: string) {
  if (!props.config) return;
  try { await deleteAlias(props.config, name); await refresh(); toast.ok(`别名「${name}」已删除`); }
  catch (e: any) { toast.err(e?.message ?? String(e)); }
}

async function onCopyName(name: string) {
  try { await copyToClipboard(name); toast.ok(`已复制「${name}」（区分大小写）— 新开终端粘贴回车即用；已开着的终端请先执行 source ~/.zshrc`); }
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
    <header class="page-header"><h2>别名</h2></header>

    <div class="card">
      <div class="card-head">自定义别名</div>
      <div class="card-body">
        <p class="desc">
          一条别名 = 工具 × 大模型 × 来源。添加后自动写入 shell 配置（macOS: ~/.zshrc / Windows: PowerShell $PROFILE），
          点「复制」拿到短名字，打开终端粘贴回车即以该组合启动。<strong>同一工具开多个窗口、各走各的源</strong>，互不影响首页的全局分配。
        </p>
        <p class="desc dim">提示：别名区分大小写；添加前已开着的终端不认识新别名（执行 <code>source ~/.zshrc</code> 或新开终端即可）；用量统计照常记录。</p>

        <div v-if="aliases.length > 0" class="alias-list">
          <div v-for="a in aliases" :key="a.name" class="alias-row">
            <div class="alias-info">
              <span class="alias-name">{{ a.name }}</span>
              <span class="alias-meta dim">{{ toolLabel(a.tool) }} · {{ modelLabel(a.model) }} · {{ sourceLabel(a.source) }}</span>
            </div>
            <div class="alias-actions">
              <button class="btn primary" @click="onCopyName(a.name)">复制</button>
              <button class="btn ghost" @click="startEditAlias(a)">修改</button>
              <button class="btn ghost" style="color:var(--danger)" @click="onDeleteAlias(a.name)">删除</button>
            </div>
          </div>
        </div>
        <div v-else class="dim sec-empty">还没有别名，添加一条试试</div>

        <button class="btn" style="margin-top:8px" @click="startNewAlias">+ 添加别名</button>
      </div>
    </div>

    <!-- 别名弹窗 -->
    <div v-if="showAliasModal" class="modal-overlay" @click.self="cancelAlias">
      <div class="modal-dialog">
        <div class="modal-title">{{ form.oldName ? '修改别名' : '添加别名' }}</div>

        <div class="modal-field">
          <label>别名 <span class="dim" style="font-weight:400">(短名字，终端里敲的就是它)</span></label>
          <input v-model="form.name" type="text" placeholder="如 dsf" style="font-family:monospace" />
        </div>

        <div class="modal-field">
          <label>工具</label>
          <select v-model="form.tool" @change="syncModelDefault">
            <option v-for="t in TOOLS" :key="t.id" :value="t.id">{{ t.label }}</option>
          </select>
        </div>

        <div class="modal-field">
          <label>大模型</label>
          <select v-model="form.model">
            <option v-for="m in modelsFor" :key="m.slug" :value="m.slug">{{ m.display_name }}</option>
          </select>
        </div>

        <div class="modal-field">
          <label>来源</label>
          <select v-model="form.source" @change="syncModelDefault">
            <option v-for="s in sources" :key="s.id" :value="s.id">{{ s.label }}</option>
          </select>
        </div>


        <div class="modal-actions">
          <button class="btn" @click="cancelAlias">取消</button>
          <button class="btn primary" :disabled="busy || !form.name.trim() || !form.model" @click="onSaveAlias">保存并生效</button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.card-head { padding: 14px 18px 0; font-size: 16px; font-weight: 700; }
.card-body { padding: 10px 18px 16px; }
.desc { font-size: 14px; color: var(--fg-muted); line-height: 1.6; margin: 0 0 10px; }
.sec-empty { font-size: 14px; padding: 4px 0 12px; }

.alias-list { margin-bottom: 12px; }
.alias-row { display: flex; align-items: center; justify-content: space-between; padding: 8px; border-bottom: 1px solid var(--border); font-size: 14px; }
.alias-row:last-child { border-bottom: none; }
.alias-info { display: flex; flex-direction: column; gap: 2px; }
.alias-name { font-weight: 700; font-family: "SF Mono", "Menlo", monospace; font-size: 15px; }
.alias-meta { font-size: 13px; }

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
.modal-field input, .modal-field select {
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
.modal-field input:focus, .modal-field select:focus { border-color: var(--accent); box-shadow: var(--focus-ring); }
.modal-actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 18px; }
</style>
