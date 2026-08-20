<script setup lang="ts">
import { ref, onMounted } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { IconClose, IconMinus, IconSquare } from "./icons";

const win = getCurrentWindow();
const maximized = ref(false);

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
</template>

<style scoped>
.titlebar {
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px 0 14px;
  background: rgba(10, 12, 17, 0.6);
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
  background: rgba(255, 255, 255, 0.08);
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
