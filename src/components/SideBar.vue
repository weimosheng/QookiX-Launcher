<script setup lang="ts">
import { useRoute } from "vue-router";
import { computed, nextTick, ref, watch } from "vue";
import AccountChip from "./AccountChip.vue";
import AppIcon from "./AppIcon.vue";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import { useTasksStore } from "../stores/tasks";
import { useInstancesStore } from "../stores/instances";
import { useSettingsStore } from "../stores/settings";
import { usePinsStore } from "../stores/pins";
import { useMessage } from "naive-ui";
import {
  IconChevronsLeft,
  IconCompass,
  IconDownload,
  IconGrid,
  IconHome,
  IconList,
  IconClose,
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

// 新闻可隐藏：settings.show_news 为 false 时不显示该导航项（默认显示）
const nav = computed(() => {
  const list = [
    { name: "home", label: "首页", icon: IconHome, to: "/" },
    { name: "browse", label: "内容", icon: IconCompass, to: "/browse" },
    { name: "instances", label: "实例", icon: IconGrid, to: "/instances" },
    { name: "multiplayer", label: "多人", icon: IconUsers, to: "/multiplayer" },
    { name: "skins", label: "皮肤", icon: IconSkin, to: "/skins" },
    { name: "settings", label: "设置", icon: IconSettings, to: "/settings" },
  ];
  if (settingsStore.settings?.show_news ?? true) {
    list.push({ name: "news", label: "新闻", icon: IconNewspaper, to: "/news" });
  }
  return list;
});

// —— 侧边栏固定实例 ——
// 与「固定到首页」共用 pins store，但只取 target === "sidebar" 的实例项，
// 两者互不干扰；名称/图标跟随实例列表实时更新。
const pins = usePinsStore();
const pinnedInstances = computed(() => {
  const byId = new Map(instances.instances.map((i) => [i.id, i]));
  return pins.items
    .filter((p) => p.type === "instance" && p.target === "sidebar")
    .map((p) => {
      const inst = byId.get(p.instanceId);
      return {
        id: p.id,
        instanceId: p.instanceId,
        // 实例可能已被删除或改名/换图标，实时从实例列表取，缺失时回退 pin 快照
        name: inst?.name ?? p.instanceName ?? p.name,
        icon: inst?.icon ?? p.instanceIcon ?? null,
        exists: !!inst,
      };
    })
    // 已被删除的实例不再显示
    .filter((p) => p.exists);
});

const downloadCount = computed(() => tasks.activeCount);

function isActive(n: { to: string }) {
  if (n.to === "/") return route.path === "/";
  // 实例详情/创建页是「实例」的子页面，进入后保持「实例」高亮，
  // 否则滑动指示器会停留在上一个页面（没有任何 nav 项匹配 → 不更新）
  if (n.to === "/instances") {
    return (
      route.path.startsWith("/instances") ||
      route.path.startsWith("/instance/") ||
      route.path === "/create"
    );
  }
  return route.path.startsWith(n.to);
}

// Sliding active-highlight indicator
const navBox = ref<HTMLElement | null>(null);
const { indicatorStyle, refresh, snap } = useSlidingIndicator(
  sidebarRef,
  // 排除 .pin-item（固定实例）与底部导航，索引顺序与 nav.value 一一对应
  () => Array.from(sidebarRef.value?.querySelectorAll<HTMLElement>(".nav-item:not(.pin-item)") ?? []),
  () => {
    // 用 nav.value（现在是 computed）。固定实例已被选择器排除，不会被计入，
    // 索引顺序保持稳定：0..nav.length-1 为主导航，nav.length 为底部「下载」。
    const idx = nav.value.findIndex((n) => isActive(n));
    if (idx >= 0) return idx;
    if (route.path.startsWith("/downloads")) return nav.value.length;
    return -1;
  },
  { axis: "vertical" }
);
watch(
  () => route.path,
  () => nextTick(() => refresh())
);
watch(collapsed, () => nextTick(() => snap()));
// 固定实例增删会改变侧边栏高度/布局，需重算指示器位置
watch(
  () => [pinnedInstances.value.length, nav.value.length],
  () => nextTick(() => refresh())
);

function unpin(id: string) {
  pins.remove(id);
}

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

    <!-- 固定实例：独立类名 .pin-item，避免被滑动指示器计入 .nav-item -->
    <div v-if="pinnedInstances.length" class="pin-section">
      <!-- 与上方导航之间只有这一条分割线（.pin-section 不再自带 border-top） -->
      <div class="pin-divider"></div>
      <div v-if="!collapsed" class="pin-section-title">固定</div>
      <div class="pin-list">
        <router-link
          v-for="p in pinnedInstances"
          :key="p.id"
          :to="`/instance/${p.instanceId}`"
          class="nav-item pin-item"
          :class="{ active: route.path.startsWith(`/instance/${p.instanceId}`) }"
          :title="collapsed ? p.name : p.name"
        >
          <div class="pin-icon-wrap">
            <AppIcon :name="p.icon" />
          </div>
          <span v-if="!collapsed" class="nav-label pin-label">{{ p.name }}</span>
          <button
            v-if="!collapsed"
            class="pin-unpin"
            title="取消固定"
            @click.prevent.stop="unpin(p.id)"
          >
            <IconClose />
          </button>
        </router-link>
      </div>
    </div>

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
/* —— 固定实例区域 —— */
.pin-section {
  margin-top: 14px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  overflow-y: auto;
  max-height: 40vh;
  /* 隐藏滚动条但保留滚轮滚动 */
  scrollbar-width: none;
  -ms-overflow-style: none;
}
.pin-section::-webkit-scrollbar {
  display: none;
}
.pin-section-title {
  padding: 0 14px 2px;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.05em;
  color: var(--text-3);
}
.pin-divider {
  height: 1px;
  margin: 0 10px 8px;
  background: var(--border);
  flex-shrink: 0;
}
.pin-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
/* 注意：这里必须用 .pin-list 提高优先级，否则会被下方同权重的
   .nav-item { padding: 10px 14px } 覆盖（CSS 后写胜出） */
.pin-list .pin-item {
  padding: 4px 6px 4px 4px;
  gap: 10px;
}
.pin-icon-wrap {
  width: 40px;
  height: 40px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
  overflow: hidden;
}
/* 折叠态按钮是 48×48，图标直接铺满 */
.sidebar.collapsed .pin-icon-wrap {
  width: 48px;
  height: 48px;
}
/* 铺满后背景高亮被图标挡住，改用内描边表示选中、亮度表示悬停 */
.sidebar.collapsed .pin-list .pin-item.active .pin-icon-wrap {
  box-shadow: inset 0 0 0 2px var(--accent);
}
.sidebar.collapsed .pin-list .pin-item:hover .pin-icon-wrap {
  filter: brightness(1.12);
}
.pin-icon-wrap :deep(.app-icon) {
  width: 100%;
  height: 100%;
}
/* 头像式展示，用 cover 保证任何比例的图片都能铺满方框（contain 会留白边） */
.pin-icon-wrap :deep(img),
.pin-icon-wrap :deep(svg) {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.pin-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}
.pin-unpin {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--text-3);
  opacity: 0;
  cursor: pointer;
  transition: opacity 0.12s, color 0.12s, background 0.12s;
}
.pin-item:hover .pin-unpin {
  opacity: 1;
}
.pin-unpin:hover {
  color: var(--accent);
  background: var(--accent-soft);
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
