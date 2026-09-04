<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import { openMenuId, bindMenuOutside } from "../composables/instanceMenu";
import { useRouter } from "vue-router";
import { useInstancesStore } from "../stores/instances";
import { usePinsStore, type PinTarget } from "../stores/pins";
import { useMessage, NModal, NButton } from "naive-ui";
import { api } from "../api";
import AppIcon from "./AppIcon.vue";
import {
  IconFolder,
  IconLayout,
  IconLayers,
  IconMapPin,
  IconMoreVertical,
  IconPlay,
  IconTrash,
} from "./icons";
import type { Instance } from "../types";

const props = defineProps<{ instance: Instance }>();
const emit = defineEmits<{ move: [instance: Instance] }>();
const instances = useInstancesStore();
const pins = usePinsStore();
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

/** 上次游玩的相对时间（unix 秒）：刚刚 / N 分钟前 / N 小时前 / 昨天 / N 天前 / 日期 */
function relTime(ts: number): string {
  const diff = Date.now() / 1000 - ts;
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  if (diff < 172800) return "昨天";
  if (diff < 259200) return "前天";
  if (diff < 86400 * 30) return `${Math.floor(diff / 86400)} 天前`;
  return new Date(ts * 1000).toLocaleDateString();
}

/** 累计游玩时长（秒）的简短人类可读格式 */
function fmtDuration(secs: number): string {
  if (secs < 60) return "不到 1 分钟";
  const totalMin = Math.floor(secs / 60);
  const h = Math.floor(totalMin / 60);
  const m = totalMin % 60;
  if (h >= 24) return `${Math.floor(h / 24)} 天 ${h % 24} 小时`;
  if (h > 0) return m > 0 ? `${h} 小时 ${m} 分钟` : `${h} 小时`;
  return `${m} 分钟`;
}

async function launch() {
  try {
    await instances.launch(props.instance.id);
    message.success("游戏已启动");
  } catch (e) {
    message.error(String(e));
  }
}

// 首页与侧边栏的固定互相独立，各自维护一条记录
const homePinId = computed(() =>
  pins.makeId("instance", props.instance.id, props.instance.id, "home")
);
const sidebarPinId = computed(() =>
  pins.makeId("instance", props.instance.id, props.instance.id, "sidebar")
);
function togglePin(target: PinTarget) {
  const i = props.instance;
  pins.toggle({
    id: pins.makeId("instance", i.id, i.id, target),
    type: "instance",
    target,
    instanceId: i.id,
    instanceName: i.name,
    instanceIcon: i.icon,
    mcVersion: i.mc_version,
    loader: i.loader,
    name: i.name,
    icon: null,
  });
}

// ——「更多」下拉菜单（全局单例：同一时刻只开一个）——
const menuOpen = computed(() => openMenuId.value === props.instance.id);
// 按钮离视口底部太近时改为向上弹出，避免菜单被截掉
const menuUp = ref(false);
const moreBtn = ref<HTMLElement | null>(null);

async function toggleMenu() {
  if (menuOpen.value) {
    openMenuId.value = null;
    return;
  }
  openMenuId.value = props.instance.id;
  bindMenuOutside();
  await nextTick();
  const r = moreBtn.value?.getBoundingClientRect();
  if (r) menuUp.value = r.bottom + 220 > window.innerHeight;
}
/** 菜单项：先收起菜单再执行动作 */
function runMenu(fn: () => void) {
  openMenuId.value = null;
  fn();
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
  <div class="inst-card glass clickable" :class="{ 'menu-open': menuOpen }" @click="router.push(`/instance/${instance.id}`)">
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
        <span v-if="instance.last_played" :title="`上次游玩 ${new Date(instance.last_played * 1000).toLocaleString()}`">
          上次 {{ relTime(instance.last_played) }}
        </span>
        <span v-if="instance.last_played && instance.total_play_time > 0">·</span>
        <span v-if="instance.total_play_time > 0" title="累计游玩时长">
          已玩 {{ fmtDuration(instance.total_play_time) }}
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
        <div class="more-wrap">
          <button
            ref="moreBtn"
            class="icon-btn"
            :class="{ active: menuOpen }"
            title="更多"
            @click="toggleMenu"
          >
            <IconMoreVertical />
          </button>
          <Transition name="menu-pop">
            <div v-if="menuOpen" class="more-menu" :class="{ up: menuUp }">
              <button
                class="more-item"
                :class="{ active: pins.isPinned(homePinId) }"
                @click="runMenu(() => togglePin('home'))"
              >
                <IconMapPin />
                <span>{{ pins.isPinned(homePinId) ? "取消固定到首页" : "固定到首页" }}</span>
              </button>
              <button
                class="more-item"
                :class="{ active: pins.isPinned(sidebarPinId) }"
                @click="runMenu(() => togglePin('sidebar'))"
              >
                <IconLayout />
                <span>{{ pins.isPinned(sidebarPinId) ? "取消固定到侧边栏" : "固定到侧边栏" }}</span>
              </button>
              <button class="more-item" @click="runMenu(() => emit('move', instance))">
                <IconLayers />
                <span>移动到分组</span>
              </button>
              <div class="more-divider"></div>
              <button class="more-item danger" @click="runMenu(confirmDelete)">
                <IconTrash />
                <span>删除实例</span>
              </button>
            </div>
          </Transition>
        </div>
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
  padding: 18px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  position: relative;
}
/* 菜单弹出时把整张卡片抬到最上层，避免被相邻卡片遮挡 */
.inst-card.menu-open {
  z-index: 50;
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
  background: var(--accent-16);
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
  border-color: var(--accent-04);
  background: var(--accent-soft);
}
.icon-btn.active {
  color: var(--accent);
  border-color: var(--accent-04);
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

/* ——「更多」下拉菜单 —— */
.more-wrap {
  position: relative;
}
.more-menu {
  position: absolute;
  right: 0;
  top: calc(100% + 8px);
  z-index: 60;
  min-width: 176px;
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  /* 明确背景：菜单嵌在卡片内，backdrop-filter 只能模糊卡片内部（近乎空白），
     故采用不依赖 backdrop-filter 的 var(--bg-2) 背景，保证任意主题下都可见、跟随主题 */
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 12px;
  box-shadow: 0 18px 40px -12px rgba(0, 0, 0, 0.55);
  -webkit-backdrop-filter: blur(var(--glass-blur, 8px));
  backdrop-filter: blur(var(--glass-blur, 8px));
}
/* 按钮离视口底部太近时向上弹 */
.more-menu.up {
  top: auto;
  bottom: calc(100% + 8px);
}
.more-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 8px 10px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-2);
  font-size: 13px;
  font-family: inherit;
  text-align: left;
  white-space: nowrap;
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}
.more-item svg {
  width: 15px;
  height: 15px;
  flex-shrink: 0;
}
.more-item:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-1);
}
.more-item.active {
  color: var(--accent);
}
.more-item.danger:hover {
  color: #e5534b;
  background: rgba(229, 83, 75, 0.12);
}
.more-divider {
  height: 1px;
  margin: 4px 6px;
  background: var(--border);
}
.menu-pop-enter-active,
.menu-pop-leave-active {
  transition: opacity 0.12s ease, transform 0.12s ease;
}
.menu-pop-enter-from,
.menu-pop-leave-to {
  opacity: 0;
  transform: translateY(-4px) scale(0.97);
}
.menu-pop-enter-to,
.menu-pop-leave-from {
  opacity: 1;
  transform: translateY(0) scale(1);
}
</style>
