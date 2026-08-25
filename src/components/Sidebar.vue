<script setup lang="ts">
import { onMounted, ref } from "vue";
import { getAppVersion } from "../ipc/api";
import { useAppUpdate } from "../composables/useAppUpdate";

defineProps<{ current: string }>();
const emit = defineEmits<(e: "navigate", page: string) => void>();

const { info: updateInfo } = useAppUpdate();

const version = ref("…");
onMounted(async () => {
  try { version.value = await getAppVersion(); } catch { version.value = "unknown"; }
});

const nav = [
  { id: "home",         label: "桌面端接入",   icon: "⌂" },
  { id: "integration",  label: "CLI 接入",     icon: ">" },
  { id: "relay",        label: "中转与API_Key", icon: "◎" },
  { id: "models",       label: "模型管理",     icon: "◆" },
  { id: "appearance",   label: "外观",         icon: "◐" },
  { id: "tools",        label: "工具检测",     icon: "🔧" },
  { id: "startup",      label: "启动项",       icon: "⏻" },
];
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar-brand">CC-Gate</div>
    <nav class="sidebar-nav">
      <div
        v-for="item in nav"
        :key="item.id"
        class="nav-item"
        :class="{ active: current === item.id }"
        @click="emit('navigate', item.id)"
      >
        <span class="nav-icon">{{ item.icon }}</span>
        {{ item.label }}
      </div>
    </nav>
    <div class="sidebar-footer">
      <div class="nav-item" @click="emit('navigate', 'settings')">
        v{{ version }}
        <span v-if="updateInfo?.has_update" class="upd-dot" title="有新版本 {{ updateInfo.latest_version }}"></span>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.upd-dot {
  display: inline-block; width: 8px; height: 8px; border-radius: 50%;
  background: var(--accent); margin-left: 6px;
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 25%, transparent);
}
</style>
