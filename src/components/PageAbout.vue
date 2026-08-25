<script setup lang="ts">
import { ref, onMounted } from "vue";
import { getAppLogTail, getAppVersion, copyToClipboard, openUrl } from "../ipc/api";
import { useAppUpdate } from "../composables/useAppUpdate";
import { useToast } from "../composables/useToast";

const toast = useToast();
const { info: updateInfo, checking: checkingUpdate, checkNow, dismiss: dismissUpdate } = useAppUpdate();

// Read the real version from the bundle instead of hardcoding it — a stale
// literal here is what makes "did my fix ship?" unanswerable.
const version = ref("…");
onMounted(async () => {
  try { version.value = await getAppVersion(); } catch { version.value = "unknown"; }
});

async function onCheckUpdate() {
  checkingUpdate.value = true;
  try {
    const r = await checkNow();
    if (!r) { toast.err("检查失败（网络或 GitHub 不可达）"); return; }
    if (r.has_update) { toast.ok(`发现新版本 v${r.latest_version}（当前 v${r.current_version}）`); }
    else { toast.ok("当前已是最新版本"); }
  } finally {
    checkingUpdate.value = false;
  }
}

const diag = ref("");
const loading = ref(false);
const copied = ref(false);

async function loadDiag() {
  loading.value = true;
  try { diag.value = await getAppLogTail(200); }
  catch (e) { diag.value = `读取失败: ${e}`; }
  finally { loading.value = false; }
}

async function copyDiag() {
  if (!diag.value) return;
  await copyToClipboard(diag.value);
  copied.value = true;
  setTimeout(() => (copied.value = false), 1500);
}
</script>

<template>
  <section class="page">
    <header class="page-header">
      <h2>关于 CC-Gate</h2>
      <span class="badge on">v{{ version }}</span>
      <button class="btn" style="margin-left:auto" :disabled="checkingUpdate" @click="onCheckUpdate">
        {{ checkingUpdate ? "检查中…" : "检查更新" }}
      </button>
    </header>

    <div v-if="updateInfo?.has_update" class="app-update-banner">
      <span>🚀 新版本 v{{ updateInfo.latest_version }} 已发布（当前 v{{ updateInfo.current_version }}）</span>
      <button class="btn primary" @click="openUrl(updateInfo.release_url)">去 GitHub 下载</button>
      <button class="btn ghost" @click="dismissUpdate()">忽略此版本</button>
    </div>

    <div class="about-content">
      <p class="about-desc">
        CC-Gate 是一个多模型 AI 工具配置管理器桌面应用，
        一键管理所有 AI Agent 的模型选择、中转站、API Key、Shell Alias 和用量统计。
      </p>

      <div class="feature-grid">
        <div class="feature-card">
          <div class="feature-icon">🔄</div>
          <div class="feature-title">Agent 模型分配</div>
          <div class="feature-detail">
            为 Codex、Claude Code、Hermes、OpenClaw、Aider 等 10 个 Agent
            独立分配可用模型，CLI 与桌面端分开管理
          </div>
        </div>

        <div class="feature-card">
          <div class="feature-icon">↗↘</div>
          <div class="feature-title">模型路由</div>
          <div class="feature-detail">
            每个模型可选择「直连」或任意中转站转发请求，
            一键切换无需改代码
          </div>
        </div>

        <div class="feature-card">
          <div class="feature-icon">🔑</div>
          <div class="feature-title">API Key 管理</div>
          <div class="feature-detail">
            22 个国内外大模型提供商统一管理 API Key，
            中转站密钥集中填入，自动检测环境变量
          </div>
        </div>

        <div class="feature-card">
          <div class="feature-icon">⚡</div>
          <div class="feature-title">Shell Alias</div>
          <div class="feature-detail">
            CLI Agent 模型自动生成 alias 写入 Shell 配置，
            跨平台支持 macOS、Linux、Windows (PowerShell)
          </div>
        </div>

        <div class="feature-card">
          <div class="feature-icon">📊</div>
          <div class="feature-title">用量统计</div>
          <div class="feature-detail">
            按今天/昨天/本周/本月等 8 个时段分桶，
            查看每个模型的 Token 消耗与费用
          </div>
        </div>

        <div class="feature-card">
          <div class="feature-icon">🚀</div>
          <div class="feature-title">自动启动</div>
          <div class="feature-detail">
            3 个代理进程（mimo2codex / claude-proxy / chat-proxy）
            随 App 启动自动拉起，统一管理
          </div>
        </div>

        <div class="feature-card">
          <div class="feature-icon">🔧</div>
          <div class="feature-title">工具检测</div>
          <div class="feature-detail">
            自动检测 Node.js、Python、Codex CLI、Claude Code、Aider 等
            依赖工具是否安装，提供安装指引
          </div>
        </div>

        <div class="feature-card">
          <div class="feature-icon">🌐</div>
          <div class="feature-title">中转站配置</div>
          <div class="feature-detail">
            内置 OpenRouter 预设，快速填入 API 地址
          </div>
        </div>
      </div>

      <div class="about-footer">
        <p>CC-Gate 替代原 CC Switch，统一管理多模型 AI 工具的配置分发。</p>
        <p class="dim" style="margin-top:8px">端口：mimo2codex :8688 · claude-proxy :8689 · chat-proxy :8690</p>
      </div>

      <div class="diag">
        <div class="diag-head">
          <div>
            <div class="diag-title">诊断信息</div>
            <div class="diag-hint">代理路由日志（模型走了哪个 provider、上游报什么错）。报问题时请复制这段发给开发者。</div>
          </div>
          <div class="diag-btns">
            <button class="btn" @click="loadDiag" :disabled="loading">{{ loading ? "读取中…" : "读取日志" }}</button>
            <button class="btn" @click="copyDiag" :disabled="!diag">{{ copied ? "已复制" : "复制" }}</button>
          </div>
        </div>
        <pre v-if="diag" class="diag-body">{{ diag }}</pre>
      </div>
    </div>
  </section>
</template>

<style scoped>
.app-update-banner {
  display: flex; align-items: center; gap: 12px; flex-wrap: wrap;
  padding: 10px 16px; margin-bottom: 16px; max-width: 720px;
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
  border-radius: var(--radius-md); font-size: 13px; color: var(--fg);
}
.about-content { padding: 0; max-width: 720px; }
.about-desc { font-size: 14px; color: var(--fg-dim); line-height: 1.6; margin: 0 0 24px; }

.feature-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
.feature-card {
  padding: 16px; border-radius: var(--radius-lg);
  background: var(--surface); border: 1px solid var(--border);
}
.feature-icon { font-size: 22px; margin-bottom: 6px; }
.feature-title { font-size: 14px; font-weight: 700; margin-bottom: 4px; }
.feature-detail { font-size: 12px; color: var(--fg-dim); line-height: 1.5; }

.about-footer { margin-top: 28px; padding-top: 16px; border-top: 1px solid var(--border); font-size: 13px; color: var(--fg-muted); }

.diag { margin-top: 20px; padding: 16px; border-radius: var(--radius-lg); background: var(--surface); border: 1px solid var(--border); }
.diag-head { display: flex; justify-content: space-between; align-items: flex-start; gap: 12px; }
.diag-title { font-size: 14px; font-weight: 700; margin-bottom: 4px; }
.diag-hint { font-size: 12px; color: var(--fg-dim); line-height: 1.5; }
.diag-btns { display: flex; gap: 8px; flex-shrink: 0; }
.diag-body {
  margin: 12px 0 0; padding: 10px; max-height: 280px; overflow: auto;
  font-size: 11px; line-height: 1.5; white-space: pre-wrap; word-break: break-all;
  background: var(--bg); border: 1px solid var(--border); border-radius: var(--radius-sm);
  color: var(--fg-dim);
}
</style>
