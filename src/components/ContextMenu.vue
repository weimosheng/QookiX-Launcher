<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import type { ContextMenuItem } from "../types";

const props = defineProps<{
  show: boolean;
  x: number;
  y: number;
  items: ContextMenuItem[];
}>();

const emit = defineEmits<{ (e: "close"): void }>();

const el = ref<HTMLElement | null>(null);
const pos = ref({ left: 0, top: 0 });
const GAP = 6;

/** 先按鼠标位置放置，再夹回视口内，避免菜单被窗口边缘截断 */
function place() {
  const node = el.value;
  if (!node) return;
  const w = node.offsetWidth;
  const h = node.offsetHeight;
  let left = props.x;
  let top = props.y;
  if (left + w + GAP > window.innerWidth) left = Math.max(GAP, props.x - w);
  if (top + h + GAP > window.innerHeight) top = Math.max(GAP, window.innerHeight - h - GAP);
  pos.value = { left, top };
}

function onDocMouseDown(e: MouseEvent) {
  if (el.value && !el.value.contains(e.target as Node)) emit("close");
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.stopPropagation();
    emit("close");
  }
}

function dismiss() {
  emit("close");
}

function bind() {
  document.addEventListener("mousedown", onDocMouseDown, true);
  document.addEventListener("keydown", onKeydown, true);
  window.addEventListener("resize", dismiss);
  window.addEventListener("scroll", dismiss, true);
}

function unbind() {
  document.removeEventListener("mousedown", onDocMouseDown, true);
  document.removeEventListener("keydown", onKeydown, true);
  window.removeEventListener("resize", dismiss);
  window.removeEventListener("scroll", dismiss, true);
}

function pick(it: ContextMenuItem) {
  if (it.disabled || it.sep) return;
  emit("close");
  it.action?.();
}

watch(
  () => props.show,
  (v) => {
    if (v) {
      nextTick(place);
      bind();
    } else {
      unbind();
    }
  }
);

onBeforeUnmount(unbind);
</script>

<template>
  <Teleport to="body">
    <div
      v-if="show"
      ref="el"
      class="ctx"
      :style="{ left: pos.left + 'px', top: pos.top + 'px' }"
      @contextmenu.prevent
    >
      <template v-for="it in items" :key="it.key">
        <div v-if="it.sep" class="ctx-sep"></div>
        <button
          v-else
          class="ctx-item"
          :class="{ danger: it.danger }"
          :disabled="it.disabled"
          @click="pick(it)"
        >
          <span class="ctx-icon">
            <component :is="it.icon" v-if="it.icon" />
          </span>
          <span class="ctx-label">{{ it.label }}</span>
          <span v-if="it.shortcut" class="ctx-key">{{ it.shortcut }}</span>
        </button>
      </template>
    </div>
  </Teleport>
</template>

<style scoped>
.ctx {
  position: fixed;
  z-index: 1200;
  min-width: 176px;
  max-width: 260px;
  padding: 5px;
  border-radius: 11px;
  border: 1px solid var(--border);
  background: var(--bg-2);
  box-shadow: 0 12px 34px rgba(0, 0, 0, 0.42);
  backdrop-filter: blur(var(--glass-blur, 8px));
  -webkit-backdrop-filter: blur(var(--glass-blur, 8px));
  animation: ctx-in 0.1s ease;
}

@keyframes ctx-in {
  from {
    opacity: 0;
    transform: scale(0.97);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

.ctx-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 7px 9px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-1);
  font-size: 12.5px;
  font-family: inherit;
  text-align: left;
  cursor: pointer;
  white-space: nowrap;
}
.ctx-item:hover:not(:disabled) {
  background: var(--panel-hover);
}
.ctx-item:disabled {
  opacity: 0.4;
  cursor: default;
}
.ctx-item.danger {
  color: #e5534b;
}
.ctx-item.danger:hover:not(:disabled) {
  background: rgba(229, 83, 75, 0.14);
}

.ctx-icon {
  width: 15px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  font-size: 13px;
  color: var(--text-3);
}
.ctx-item.danger .ctx-icon {
  color: #e5534b;
}

.ctx-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.ctx-key {
  font-size: 11px;
  color: var(--text-3);
  opacity: 0.8;
  margin-left: 10px;
  flex-shrink: 0;
}

.ctx-sep {
  height: 1px;
  margin: 4px 6px;
  background: var(--border);
}
</style>
