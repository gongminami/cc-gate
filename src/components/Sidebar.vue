<script setup lang="ts">
import { onMounted, ref } from "vue";
import { getAppVersion } from "../ipc/api";

defineProps<{ current: string }>();
const emit = defineEmits<(e: "navigate", page: string) => void>();

const version = ref("…");
onMounted(async () => {
  try { version.value = await getAppVersion(); } catch { version.value = "unknown"; }
});

const nav = [
  { id: "home",       label: "首页",          icon: "⌂" },
  { id: "relay",      label: "中转与API_Key",  icon: "◎" },
  { id: "models",     label: "模型管理",        icon: "◆" },
  { id: "shell",      label: "Shell 集成",      icon: ">" },
  { id: "aliases",    label: "别名",           icon: "@" },
  // { id: "usage",      label: "用量统计",        icon: "▤" },
  // { id: "models",     label: "模型参数",        icon: "◆" },
  { id: "appearance", label: "外观",            icon: "◐" },
  { id: "tools",      label: "工具检测",        icon: "🔧" },
  { id: "startup",    label: "启动项",          icon: "⏻" },
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
      <div class="nav-item" @click="emit('navigate', 'settings')">v{{ version }}</div>
    </div>
  </aside>
</template>
