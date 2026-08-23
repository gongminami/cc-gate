<script setup lang="ts">
import { onMounted, ref } from "vue";
import Sidebar from "./components/Sidebar.vue";
import Toaster from "./components/Toaster.vue";
import CloseToTrayDialog from "./components/CloseToTrayDialog.vue";
import PageHome from "./components/PageHome.vue";
import PageModels from "./components/PageModels.vue";
import PageShell from "./components/PageShell.vue";
import PageUsage from "./components/PageUsage.vue";
import PageRelayKeys from "./components/PageRelayKeys.vue";
import PageAliases from "./components/PageAliases.vue";
import PageTools from "./components/PageTools.vue";
import PageStartup from "./components/PageStartup.vue";
import PageAppearance from "./components/PageAppearance.vue";
import PageAbout from "./components/PageAbout.vue";
import { useAppConfig } from "./composables/useAppConfig";
import { useTheme } from "./composables/useTheme";
import { useAppUpdate } from "./composables/useAppUpdate";

useTheme();
const { config, loading, error, refresh } = useAppConfig();
const { refresh: checkUpdate } = useAppUpdate();
const currentPage = ref<string>(localStorage.getItem("ccgate.page") ?? "home");

function navigate(page: string) {
  currentPage.value = page;
  localStorage.setItem("ccgate.page", page);
}

onMounted(async () => {
  await refresh();
  // Startup silent app-update check — failures are suppressed inside the composable.
  setTimeout(() => { checkUpdate(); }, 2500);
});
</script>

<template>
  <div class="shell">
    <Sidebar :current="currentPage" @navigate="navigate" />
    <main class="main">
      <div v-if="loading && !config" class="loading">Loading config…</div>
      <div v-else-if="error" class="fatal">
        <h2>启动失败</h2>
        <pre>{{ error }}</pre>
        <button class="btn" @click="refresh">重试</button>
      </div>
      <template v-else>
        <PageHome      v-if="currentPage === 'home'"       :config="config" />
        <PageModels    v-else-if="currentPage === 'models'"   :config="config" />
        <PageShell     v-else-if="currentPage === 'shell'"    :config="config" />
        <PageUsage     v-else-if="currentPage === 'usage'" />
        <PageRelayKeys v-else-if="currentPage === 'relay'"    :config="config" />
        <PageAliases   v-else-if="currentPage === 'aliases'"  :config="config" />
        <PageTools     v-else-if="currentPage === 'tools'" />
        <PageStartup   v-else-if="currentPage === 'startup'"  :config="config" />
        <PageAppearance v-else-if="currentPage === 'appearance'" />
        <PageAbout      v-else-if="currentPage === 'settings'" />
      </template>
    </main>
    <Toaster />
    <CloseToTrayDialog />
  </div>
</template>

<style scoped>
.shell { display: flex; height: 100%; }
.main { flex: 1; overflow: auto; background: var(--surface-soft); }
.loading, .fatal { padding: 48px; color: var(--fg-muted); }
.fatal pre { background: var(--surface); padding: 12px; border-radius: 6px; border: 1px solid var(--border); white-space: pre-wrap; font-size: 12px; }
</style>
