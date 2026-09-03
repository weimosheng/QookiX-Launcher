<script setup lang="ts">
import { useRoute } from "vue-router";
import { computed, nextTick, ref, watch } from "vue";
import AccountChip from "./AccountChip.vue";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import { useTasksStore } from "../stores/tasks";
import { useInstancesStore } from "../stores/instances";
import { useSettingsStore } from "../stores/settings";
import { useMessage } from "naive-ui";
import {
  IconChevronsLeft,
  IconCompass,
  IconDownload,
  IconGrid,
  IconHome,
  IconList,
  IconNewspaper,
  IconSettings,
  IconSkin,
  IconStop,
  IconUsers,
} from "./icons";

const route = useRoute();
const tasks = useTasksStore();
const instances = useInstancesStore();
const settingsStore = useSettingsStore();
const message = useMessage();

const collapsed = ref(true);

const sidebarRef = ref<HTMLElement | null>(null);

const nav = [
  { name: "home", label: "首页", icon: IconHome, to: "/" },
  { name: "browse", label: "内容", icon: IconCompass, to: "/browse" },
  { name: "instances", label: "实例", icon: IconGrid, to: "/instances" },
  { name: "multiplayer", label: "多人", icon: IconUsers, to: "/multiplayer" },
  { name: "skins", label: "皮肤", icon: IconSkin, to: "/skins" },
  { name: "settings", label: "设置", icon: IconSettings, to: "/settings" },
  { name: "news", label: "新闻", icon: IconNewspaper, to: "/news" },
];

const downloadCount = computed(() => tasks.activeCount);

function isActive(n: { to: string }) {
  if (n.to === "/") return route.path === "/";
  return route.path.startsWith(n.to);
}

// Sliding active-highlight indicator
const navBox = ref<HTMLElement | null>(null);
const { indicatorStyle, refresh, snap } = useSlidingIndicator(
  sidebarRef,
  () => Array.from(sidebarRef.value?.querySelectorAll<HTMLElement>(".nav-item") ?? []),
  () => {
    const idx = nav.findIndex((n) => isActive(n));
    if (idx >= 0) return idx;
    if (route.path.startsWith("/downloads")) return nav.length;
    return -1;
  },
  { axis: "vertical" }
);
watch(
  () => route.path,
  () => nextTick(() => refresh())
);
watch(collapsed, () => nextTick(() => snap()));

async function stopAll() {
  try {
    await instances.stop();
    message.success("已关闭所有实例");
  } catch (e) {
    message.error(String(e));
  }
}
</script>

<template>
  <aside ref="sidebarRef" class="sidebar" :class="{ collapsed }">
    <div class="indicator" :style="indicatorStyle"></div>
    <nav ref="navBox" class="nav">
      <router-link
        v-for="n in nav"
        :key="n.name"
        :to="n.to"
        class="nav-item"
        :class="{ active: isActive(n) }"
        :title="collapsed ? n.label : undefined"
      >
        <component :is="n.icon" class="nav-icon" />
        <span v-if="!collapsed" class="nav-label">{{ n.label }}</span>
        <span
          v-if="n.name === 'downloads' && downloadCount > 0"
          class="nav-badge"
          :title="`正在下载 ${downloadCount} 项`"
        >{{ downloadCount }}</span>
      </router-link>
    </nav>

    <div class="side-foot">
      <button v-if="tasks.gameRunning" class="stop-all-btn" @click="stopAll">
        <IconStop />
        <span v-if="!collapsed">关闭所有实例</span>
      </button>
      <router-link
        to="/downloads"
        class="nav-item foot-nav"
        :class="{ active: route.path.startsWith('/downloads') }"
        :title="collapsed ? '下载' : undefined"
      >
        <IconDownload class="nav-icon" />
        <span v-if="!collapsed" class="nav-label">下载</span>
        <span
          v-if="downloadCount > 0"
          class="nav-badge"
          :title="`正在下载 ${downloadCount} 项`"
        >{{ downloadCount }}</span>
      </router-link>
      <AccountChip :collapsed="collapsed" />
      <button
        v-if="settingsStore.settings?.show_sidebar_collapse_btn ?? true"
        class="collapse-btn"
        @click="collapsed = !collapsed"
      >
        <IconChevronsLeft v-if="!collapsed" />
        <IconList v-else />
      </button>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  position: relative;
  width: 216px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  padding: 18px 12px 12px;
  border-right: 1px solid var(--border);
  background: color-mix(in srgb, var(--bg-1) 84%, transparent);
  backdrop-filter: blur(var(--glass-blur, 8px));
  -webkit-backdrop-filter: blur(var(--glass-blur, 8px));
  transition: width 0.2s ease, padding 0.2s ease;
}
/* 背景图片模式下与卡片一致：用半透明面板色 + 模糊，透出背景图 */
:global(.has-bg) .sidebar {
  background: var(--panel);
}
.sidebar.collapsed {
  width: 60px;
  padding: 18px 6px 12px;
}
.nav {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.nav-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  border-radius: 10px;
  color: var(--text-2);
  font-size: 14px;
  font-weight: 500;
  text-decoration: none;
  transition: color 0.14s, background 0.14s, border-color 0.14s, transform 0.1s ease;
  border: 1px solid transparent;
  white-space: nowrap;
  overflow: hidden;
  height: 48px;
  box-sizing: border-box;
}
.nav-item:active {
  transform: scale(0.96);
}
.sidebar.collapsed .nav-item {
  justify-content: center;
  padding: 0;
}
.nav-item:hover {
  background: var(--panel-hover);
  color: var(--text-1);
}
.nav-item.active {
  color: var(--accent);
}
.indicator {
  position: absolute;
  left: 0;
  width: 100%;
  z-index: 1;
  border-radius: 10px;
  background: var(--accent-soft);
  border: 1px solid var(--accent-03);
  transition:
    top 0.28s cubic-bezier(0.22, 1, 0.36, 1),
    height 0.28s cubic-bezier(0.22, 1, 0.36, 1),
    opacity 0.18s;
  pointer-events: none;
}
.nav-icon,
.nav-label,
.nav-badge {
  position: relative;
  z-index: 2;
}
.nav-icon {
  font-size: 18px;
}
.nav-label {
  animation: fade-in 0.2s ease;
}
.nav-badge {
  position: absolute;
  top: 6px;
  right: 8px;
  min-width: 16px;
  height: 16px;
  padding: 0 4px;
  border-radius: 8px;
  background: var(--accent);
  color: #fff;
  font-size: 11px;
  font-weight: 600;
  line-height: 16px;
  text-align: center;
  box-sizing: border-box;
  pointer-events: none;
  animation: fade-in 0.18s ease;
}
@keyframes fade-in {
  from { opacity: 0; }
  to { opacity: 1; }
}
.side-foot {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.collapse-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 48px;
  box-sizing: border-box;
  padding: 0 8px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--panel);
  color: var(--text-3);
  font-size: 16px;
  cursor: pointer;
  transition: color 0.14s, background 0.14s, border-color 0.14s;
}
.collapse-btn:hover {
  background: var(--panel-hover);
  color: var(--text-1);
}
.sidebar.collapsed .collapse-btn {
  border: none;
  background: transparent;
  padding: 0;
}
.sidebar.collapsed .collapse-btn:hover {
  background: transparent;
}
.stop-all-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  height: 40px;
  box-sizing: border-box;
  padding: 0 14px;
  border: 1px solid rgba(229, 83, 75, 0.4);
  border-radius: 10px;
  background: rgba(229, 83, 75, 0.12);
  color: #f0907f;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.14s;
}
.stop-all-btn:hover {
  background: rgba(229, 83, 75, 0.2);
}
.sidebar.collapsed .stop-all-btn {
  justify-content: center;
  padding: 0;
}
</style>
