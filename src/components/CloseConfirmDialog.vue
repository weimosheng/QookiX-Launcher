<script setup lang="ts">
/**
 * 关闭窗口确认弹窗。
 *
 * 「关闭窗口时」设置为「每次询问」（默认）时，后端会拦截窗口关闭并广播
 * `app://close-requested`，这里弹出两个选项（最小化到后台 / 退出程序），
 * 以及「保持我的选择」勾选框：勾选后把该选择写回 `close_behavior`，之后
 * 不再询问。
 */
import { onBeforeUnmount, onMounted, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { NButton, NCheckbox, NModal } from "naive-ui";
import { api } from "../api";
import { notifyError } from "../composables/notify";
import { IconMinus, IconPower } from "./icons";

const show = ref(false);
const remember = ref(false);
const busy = ref(false);
let unlisten: (() => void) | null = null;

function open() {
  // 连点标题栏关闭按钮时不要重复叠加弹窗
  if (show.value) return;
  remember.value = false;
  show.value = true;
}

async function choose(action: "minimize" | "quit") {
  if (busy.value) return;
  busy.value = true;
  show.value = false;
  try {
    await api.resolveCloseRequest(action, remember.value);
  } catch (e) {
    // 执行失败（如写盘失败）时把窗口还原，避免用户以为已经生效
    show.value = true;
    notifyError(String(e));
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  try {
    unlisten = await listen("app://close-requested", () => open());
  } catch {
    /* 监听不可用不影响其他功能 */
  }
});

onBeforeUnmount(() => {
  unlisten?.();
  unlisten = null;
});
</script>

<template>
  <NModal
    :show="show"
    preset="card"
    :closable="false"
    :close-on-esc="false"
    :style="{ width: 'min(520px, 92vw)' }"
    @update:show="(v: boolean) => (show = v)"
  >
    <template #header>
      <div class="cc-head">
        <span class="cc-title">关闭 QookiX Launcher</span>
        <span class="cc-sub">请选择关闭窗口时的行为</span>
      </div>
    </template>

    <div class="cc-options">
      <button class="cc-option" :disabled="busy" @click="choose('minimize')">
        <span class="cc-icon"><IconMinus /></span>
        <span class="cc-option-name">最小化到后台</span>
        <span class="cc-option-hint">保持托盘运行，游戏与下载任务不中断</span>
      </button>
      <button class="cc-option is-danger" :disabled="busy" @click="choose('quit')">
        <span class="cc-icon"><IconPower /></span>
        <span class="cc-option-name">退出程序</span>
        <span class="cc-option-hint">完全关闭启动器，已启动的游戏不受影响</span>
      </button>
    </div>

    <template #footer>
      <div class="cc-footer">
        <NCheckbox v-model:checked="remember" :disabled="busy" class="cc-remember">
          保持我的选择（下次不再询问）
        </NCheckbox>
        <NButton :disabled="busy" @click="show = false">取消</NButton>
      </div>
    </template>
  </NModal>
</template>

<style scoped>
.cc-head {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.cc-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-1);
}
.cc-sub {
  font-size: 12px;
  font-weight: 400;
  color: var(--text-3);
}
.cc-options {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}
.cc-option {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  gap: 7px;
  padding: 16px 14px 14px;
  border-radius: 12px;
  border: 1px solid var(--w-09);
  background: var(--w-04);
  cursor: pointer;
  font-family: inherit;
  transition: background 0.15s, border-color 0.15s, transform 0.15s;
}
.cc-option:hover:not(:disabled) {
  background: var(--w-07);
  border-color: var(--accent);
  transform: translateY(-1px);
}
.cc-option:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
.cc-option:disabled {
  opacity: 0.6;
  cursor: default;
}
.cc-icon {
  display: grid;
  place-items: center;
  width: 38px;
  height: 38px;
  border-radius: 11px;
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 18px;
}
.cc-option-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.cc-option-hint {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-3);
}
.cc-option.is-danger .cc-icon {
  background: var(--danger-12);
  color: #e5534b;
}
.cc-option.is-danger:hover:not(:disabled) {
  border-color: rgba(229, 83, 75, 0.65);
  background: var(--danger-08);
}
.cc-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.cc-remember {
  font-size: 12px;
}
@media (max-width: 480px) {
  .cc-options {
    grid-template-columns: 1fr;
  }
}
</style>
