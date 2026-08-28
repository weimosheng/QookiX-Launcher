<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";
import { useInstancesStore } from "../stores/instances";
import { useMessage, NModal, NButton } from "naive-ui";
import { api } from "../api";
import AppIcon from "./AppIcon.vue";
import { IconFolder, IconPlay, IconTrash } from "./icons";
import type { Instance } from "../types";

const props = defineProps<{ instance: Instance }>();
const instances = useInstancesStore();
const router = useRouter();
const message = useMessage();

const confirmState = ref<{ title: string; content: string; positiveText: string; onOk: () => void | Promise<void> } | null>(null);
const confirmLoading = ref(false);
async function handleConfirm() {
  if (!confirmState.value) return;
  confirmLoading.value = true;
  try {
    await confirmState.value.onOk();
    confirmState.value = null;
  } finally {
    confirmLoading.value = false;
  }
}

function loaderBadge() {
  return props.instance.loader === "vanilla" ? "原版" : props.instance.loader.charAt(0).toUpperCase() + props.instance.loader.slice(1);
}

async function launch() {
  try {
    await instances.launch(props.instance.id);
    message.success("游戏已启动");
  } catch (e) {
    message.error(String(e));
  }
}

async function apiOpen() {
  try {
    await api.openInstanceFolder(props.instance.id);
  } catch (e) {
    message.error(String(e));
  }
}

function confirmDelete() {
  confirmState.value = {
    title: "删除实例",
    content: `确定要删除「${props.instance.name}」吗？游戏目录与全部内容将被移除，此操作不可恢复。`,
    positiveText: "删除",
    onOk: async () => {
      try {
        await instances.remove(props.instance.id);
        message.success("实例已删除");
      } catch (e) {
        message.error(String(e));
      }
    },
  };
}
</script>

<template>
  <div class="inst-card glass clickable" @click="router.push(`/instance/${instance.id}`)">
    <div class="card-top">
      <div class="icon"><AppIcon :name="instance.icon" /></div>
      <div class="title-wrap">
        <div class="name text-ellipsis">{{ instance.name }}</div>
        <div class="meta">
          <span class="badge">{{ loaderBadge() }}</span>
          <span class="mc">{{ instance.mc_version }}</span>
          <span v-if="instance.loader_version" class="lv">{{ instance.loader_version }}</span>
        </div>
      </div>
    </div>
    <div class="card-foot">
      <div class="foot-info">
        <span>{{ instance.mods.length }} 模组</span>
        <span v-if="instance.last_played">
          · 最近 {{ new Date(instance.last_played * 1000).toLocaleDateString() }}
        </span>
      </div>
      <div class="actions" @click.stop>
        <button
          class="icon-btn play"
          title="启动游戏"
          @click="launch"
        >
          <IconPlay />
        </button>
        <button class="icon-btn" title="打开游戏目录" @click="apiOpen">
          <IconFolder />
        </button>
        <button class="icon-btn danger" title="删除实例" @click="confirmDelete">
          <IconTrash />
        </button>
      </div>
    </div>
  </div>
  <n-modal
    :show="confirmState !== null"
    preset="card"
    :title="confirmState?.title ?? ''"
    style="width: 420px; max-width: 92vw"
    @update:show="(v: boolean) => { if (!v) confirmState = null; }"
  >
    <div v-if="confirmState" style="display: flex; flex-direction: column; gap: 16px;">
      <div style="font-size: 14px; color: var(--text-2); line-height: 1.6;">{{ confirmState.content }}</div>
      <div style="display: flex; justify-content: flex-end; gap: 10px;">
        <n-button @click="confirmState = null">取消</n-button>
        <n-button type="error" :loading="confirmLoading" @click="handleConfirm">{{ confirmState.positiveText }}</n-button>
      </div>
    </div>
  </n-modal>
</template>

<style scoped>
.inst-card {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.card-top {
  display: flex;
  align-items: center;
  gap: 13px;
}
.icon {
  width: 46px;
  height: 46px;
  border-radius: 12px;
  overflow: hidden;
  background: transparent;
  position: relative;
  font-size: 21px;
  color: var(--accent);
  flex-shrink: 0;
  box-sizing: border-box;
}
.icon :deep(.app-icon) {
  position: absolute;
  inset: 0;
}
.title-wrap {
  flex: 1;
  min-width: 0;
}
.name {
  font-weight: 600;
  font-size: 15px;
  margin-bottom: 5px;
}
.meta {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 12px;
}
.badge {
  background: rgba(232, 154, 75, 0.16);
  color: var(--accent);
  border-radius: 6px;
  padding: 1px 7px;
  font-weight: 600;
}
.mc {
  color: var(--text-2);
  font-weight: 600;
}
.lv {
  color: var(--text-3);
}
.state {
  font-size: 12px;
  font-weight: 600;
  padding: 3px 9px;
  border-radius: 8px;
  flex-shrink: 0;
}
.state.ok {
  color: #4ec9a0;
  background: rgba(78, 201, 160, 0.12);
}
.state.warn {
  color: #e0a030;
  background: rgba(224, 160, 48, 0.12);
}
.card-foot {
  display: flex;
  justify-content: space-between;
  align-items: center;
  border-top: 1px solid var(--border);
  padding-top: 11px;
}
.foot-info {
  font-size: 12px;
  color: var(--text-3);
}
.actions {
  display: flex;
  gap: 6px;
}
.icon-btn {
  width: 32px;
  height: 32px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-2);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  font-size: 14px;
  transition: all 0.12s;
}
.icon-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-1);
}
.icon-btn.play {
  color: var(--accent);
  border-color: rgba(232, 154, 75, 0.4);
  background: var(--accent-soft);
}
.icon-btn.danger:hover {
  color: #e5534b;
  border-color: rgba(229, 83, 75, 0.5);
}
.icon-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
</style>
