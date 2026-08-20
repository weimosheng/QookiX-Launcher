<script setup lang="ts">
import { useRoute } from "vue-router";
import AccountChip from "./AccountChip.vue";
import { IconCompass, IconDownload, IconGrid, IconHome, IconSettings } from "./icons";

const route = useRoute();

const nav = [
  { name: "home", label: "首页", icon: IconHome, to: "/" },
  { name: "browse", label: "内容中心", icon: IconCompass, to: "/browse" },
  { name: "downloads", label: "下载中心", icon: IconDownload, to: "/downloads" },
  { name: "instances", label: "游戏实例", icon: IconGrid, to: "/instances" },
  { name: "settings", label: "设置", icon: IconSettings, to: "/settings" },
];

function isActive(n: { to: string }) {
  if (n.to === "/") return route.path === "/";
  return route.path.startsWith(n.to);
}
</script>

<template>
  <aside class="sidebar">
    <nav class="nav">
      <router-link
        v-for="n in nav"
        :key="n.name"
        :to="n.to"
        class="nav-item"
        :class="{ active: isActive(n) }"
      >
        <component :is="n.icon" class="nav-icon" />
        <span>{{ n.label }}</span>
      </router-link>
    </nav>

    <div class="side-foot">
      <AccountChip />
      <div class="ver">QookiX Launcher v0.1.0 · 纯净无广告</div>
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
  background: rgba(12, 14, 20, 0.5);
}
.nav {
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
  transition: all 0.14s;
  border: 1px solid transparent;
}
.nav-item:hover {
  background: rgba(255, 255, 255, 0.05);
  color: var(--text-1);
}
.nav-item.active {
  background: var(--accent-soft);
  color: var(--accent);
  border-color: rgba(232, 154, 75, 0.3);
}
.nav-icon {
  font-size: 18px;
}
.side-foot {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.ver {
  font-size: 11px;
  color: var(--text-3);
  text-align: center;
  opacity: 0.8;
}
</style>
