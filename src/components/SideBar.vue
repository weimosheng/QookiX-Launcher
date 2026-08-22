<script setup lang="ts">
import { useRoute } from "vue-router";
import { nextTick, ref, watch } from "vue";
import AccountChip from "./AccountChip.vue";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import {
  IconChevronsLeft,
  IconCompass,
  IconDownload,
  IconGrid,
  IconHome,
  IconList,
  IconSettings,
} from "./icons";

const route = useRoute();

const collapsed = ref(true);

const nav = [
  { name: "home", label: "首页", icon: IconHome, to: "/" },
  { name: "browse", label: "内容", icon: IconCompass, to: "/browse" },
  { name: "downloads", label: "下载", icon: IconDownload, to: "/downloads" },
  { name: "instances", label: "实例", icon: IconGrid, to: "/instances" },
  { name: "settings", label: "设置", icon: IconSettings, to: "/settings" },
];

function isActive(n: { to: string }) {
  if (n.to === "/") return route.path === "/";
  return route.path.startsWith(n.to);
}

// Sliding active-highlight indicator
const navBox = ref<HTMLElement | null>(null);
const { indicatorStyle, refresh, snap } = useSlidingIndicator(
  navBox,
  () => Array.from(navBox.value?.querySelectorAll<HTMLElement>(".nav-item") ?? []),
  () => nav.findIndex((n) => isActive(n)),
  { axis: "vertical" }
);
watch(
  () => route.path,
  () => nextTick(() => refresh())
);
watch(collapsed, () => nextTick(() => snap()));
</script>

<template>
  <aside class="sidebar" :class="{ collapsed }">
    <nav ref="navBox" class="nav">
      <div class="indicator" :style="indicatorStyle"></div>
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
      </router-link>
    </nav>

    <div class="side-foot">
      <AccountChip :collapsed="collapsed" />
      <button class="collapse-btn" @click="collapsed = !collapsed">
        <IconChevronsLeft v-if="!collapsed" />
        <IconList v-else />
      </button>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  width: 216px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  padding: 18px 12px 12px;
  border-right: 1px solid var(--border);
  background: color-mix(in srgb, var(--bg-1) 72%, transparent);
  transition: width 0.2s ease, padding 0.2s ease;
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
  border-radius: 10px;
  background: var(--accent-soft);
  border: 1px solid rgba(232, 154, 75, 0.3);
  transition:
    top 0.28s cubic-bezier(0.22, 1, 0.36, 1),
    height 0.28s cubic-bezier(0.22, 1, 0.36, 1),
    opacity 0.18s;
  pointer-events: none;
}
.nav-icon {
  font-size: 18px;
}
.nav-label {
  animation: fade-in 0.2s ease;
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
</style>
