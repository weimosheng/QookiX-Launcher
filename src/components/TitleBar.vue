<script setup lang="ts">
import { computed, ref, onMounted, inject } from "vue";
import { useRoute, useRouter } from "vue-router";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  IconClose,
  IconMinus,
  IconSquare,
  IconHome,
  IconCompass,
  IconDownload,
  IconGrid,
  IconNewspaper,
  IconPlus,
  IconSettings,
  IconUser,
  IconUsers,
  IconTrash,
  IconRefresh,
} from "./icons";
import { useTasksStore } from "../stores/tasks";
import { useServersStore } from "../stores/servers";
import {
  updateReady,
  updateReadyVersion,
  updateInstalling,
  applyUpdateNow,
} from "../updater";

const route = useRoute();
const router = useRouter();
const tasks = useTasksStore();
const servers = useServersStore();

const pageIcons: Record<string, any> = {
  home: IconHome,
  compass: IconCompass,
  download: IconDownload,
  grid: IconGrid,
  newspaper: IconNewspaper,
  plus: IconPlus,
  settings: IconSettings,
  user: IconUser,
  users: IconUsers,
};
const pageIcon = computed(() => pageIcons[(route.meta.icon as string) ?? ""] ?? IconHome);

const actionIcons: Record<string, any> = {
  plus: IconPlus,
  trash: IconTrash,
};
// 页面路由 meta 中配置的导航按钮（如“新建实例”）
const pageAction = computed(() => (route.meta.action as { text?: string; icon?: string; to?: string }) ?? null);
// 供实例页触发的“新建分组”信号（由 App 提供）
const groupDialogRequest = inject<{ value: number }>("groupDialogRequest", { value: 0 });
function requestCreateGroup() {
  groupDialogRequest.value++;
}
// 下载中心特殊的“清除已完成”操作按钮
const finishedCount = computed(() => tasks.taskList.filter((t) => t.finished).length);

const win = getCurrentWindow();
const maximized = ref(false);

/**
 * 安装已下载的更新包并重启。Windows 上 NSIS 会强制结束当前进程并自行拉起
 * 新进程，所以调用后通常不会再返回。
 */
function doApplyUpdate() {
  void applyUpdateNow().catch((err) => {
    console.error("[updater] apply failed:", err);
  });
}

async function toggleMax() {
  if (await win.isMaximized()) {
    await win.unmaximize();
  } else {
    await win.maximize();
  }
  maximized.value = await win.isMaximized();
}

onMounted(async () => {
  maximized.value = await win.isMaximized();
  win.onResized(() => {
    win.isMaximized().then((m) => (maximized.value = m));
  });
});
</script>

<template>
  <div class="titlebar" data-tauri-drag-region>
    <div class="tb-left" data-tauri-drag-region>
      <img src="/app-icon.png" class="tb-logo" draggable="false" alt="" />
      <span class="tb-title">QookiX Launcher</span>
      <span class="tb-divider">/</span>
      <component :is="pageIcon" class="tb-page-icon" />
      <span class="tb-page">{{ (route.meta.title as string) ?? "" }}</span>
    </div>
    <div class="tb-right" data-tauri-drag-region>
      <div class="tb-actions">
        <button
          v-if="updateReady"
          class="tb-action primary"
          :disabled="updateInstalling"
          :title="updateReadyVersion ? `已下载 v${updateReadyVersion}，点击安装并重启` : '已下载更新，点击安装并重启'"
          @click="doApplyUpdate()"
        >
          <IconRefresh class="tb-action-icon" />
          {{ updateInstalling ? "正在安装…" : "重启以更新" }}
        </button>
        <button
          v-if="route.name === 'instances'"
          class="tb-action"
          @click="requestCreateGroup"
        >
          <IconPlus class="tb-action-icon" /> 新建分组
        </button>
        <button
          v-if="route.name === 'multiplayer' && servers.canCreate"
          class="tb-action primary"
          @click="servers.requestCreate()"
        >
          <IconPlus class="tb-action-icon" /> 创建服务器
        </button>
        <button
          v-if="pageAction?.to"
          class="tb-action primary"
          @click="router.push(pageAction.to)"
        >
          <component :is="actionIcons[pageAction.icon ?? '']" class="tb-action-icon" />
          {{ pageAction.text }}
        </button>
        <button
          v-if="route.name === 'downloads'"
          class="tb-action"
          :disabled="!finishedCount"
          @click="tasks.clearFinished()"
        >
          <IconTrash class="tb-action-icon" /> 清除已完成
        </button>
      </div>
      <div class="tb-controls">
        <button class="tb-btn" title="最小化" @click="win.minimize()">
        <IconMinus />
      </button>
      <button class="tb-btn" title="最大化" @click="toggleMax">
        <IconSquare v-if="!maximized" />
        <IconMinus v-else class="restore-icon" />
      </button>
      <button class="tb-btn tb-close" title="关闭" @click="win.close()">
        <IconClose />
      </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.titlebar {
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px 0 14px;
  background: color-mix(in srgb, var(--bg-0) 86%, transparent);
  backdrop-filter: blur(var(--glass-blur, 8px));
  -webkit-backdrop-filter: blur(var(--glass-blur, 8px));
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  -webkit-app-region: drag;
}
.tb-left {
  display: flex;
  align-items: center;
  gap: 9px;
}
.tb-logo {
  width: 20px;
  height: 20px;
  border-radius: 6px;
}
.tb-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
  letter-spacing: 0.2px;
}
.tb-divider {
  color: var(--text-3);
  font-size: 13px;
  margin: 0 1px;
}
.tb-page-icon {
  width: 14px;
  height: 14px;
  color: var(--text-3);
}
.tb-page {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-1);
  letter-spacing: 0.2px;
}
.tb-right {
  display: flex;
  align-items: center;
  gap: 10px;
}
.tb-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  -webkit-app-region: no-drag;
}
.tb-action {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.05);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.15s;
}
.tb-action:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
  color: var(--text-1);
}
.tb-action:disabled {
  opacity: 0.4;
  cursor: default;
}
.tb-action.primary {
  border: none;
  background: linear-gradient(135deg, var(--accent), var(--accent-deep));
  color: #1a1208;
  box-shadow: 0 4px 16px var(--accent-03);
}
.tb-action.primary:hover:not(:disabled) {
  filter: brightness(1.08);
}

.tb-action-icon {
  width: 14px;
  height: 14px;
}
.tb-controls {
  display: flex;
  gap: 2px;
  -webkit-app-region: no-drag;
}
.tb-btn {
  width: 38px;
  height: 30px;
  border: none;
  background: transparent;
  color: var(--text-3);
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 15px;
  cursor: pointer;
  transition: all 0.12s;
}
.tb-btn:hover {
  background: var(--panel-hover);
  color: var(--text-1);
}
.tb-close:hover {
  background: #e5534b;
  color: #fff;
}
.restore-icon {
  transform: scale(0.75);
  display: block;
}
</style>
