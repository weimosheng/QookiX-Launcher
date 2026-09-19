<script setup lang="ts">
// 新手引导条：三步开始玩（账号 → 实例 → 启动），非阻塞、可关闭、完成即消失。
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { useAccountsStore } from "../stores/accounts";
import { useInstancesStore } from "../stores/instances";
import { useSettingsStore } from "../stores/settings";
import { IconClose, IconPlay } from "./icons";

const { t } = useI18n();

const DONE_KEY = "qookix:onboarding:done";
const LAUNCHED_KEY = "qookix:onboarding:launched";

const emit = defineEmits<{ launch: [] }>();

const router = useRouter();
const accounts = useAccountsStore();
const instances = useInstancesStore();
const settingsStore = useSettingsStore();

const dismissed = ref(localStorage.getItem(DONE_KEY) === "1");
// 启动成功由 HomeView 写标记，这里只读（用 ref 存初值即可，无需响应式追踪 localStorage）
const launched = ref(localStorage.getItem(LAUNCHED_KEY) === "1");

// 预览模式：localStorage 设 qookix:onboarding:preview=1 可强制查看全空视角（测试用）
const preview = localStorage.getItem("qookix:onboarding:preview") === "1";
const step1 = computed(() => !preview && accounts.accounts.length > 0);
const step2 = computed(() => !preview && instances.instances.length > 0);
const allDone = computed(() => step1.value && step2.value && launched.value);

const show = computed(
  () =>
    !dismissed.value &&
    !allDone.value &&
    // 与新手向导（driver.js tour）错开：tour 播完/跳过（写入 onboarding_completed）
    // 之前不显示本条，避免首次启动双重引导；tour 结束后由本条接管三步进度
    !!settingsStore.settings?.onboarding_completed,
);

function dismiss() {
  dismissed.value = true;
  localStorage.setItem(DONE_KEY, "1");
}

watch(allDone, (v) => {
  if (v) dismiss();
});

function openAccounts() {
  if (!step1.value) accounts.showManager = true;
}

function openBrowse() {
  if (!step2.value) router.push("/browse");
}
</script>

<template>
  <section v-if="show" class="onboarding glass">
    <div class="ob-head">
      <IconPlay class="ob-title-icon" />
      <span class="ob-title">{{ t("onboardingBar.title") }}</span>
    </div>
    <div class="ob-steps">
      <button
        class="ob-step"
        :class="{ done: step1, current: !step1 }"
        @click="openAccounts"
      >
        <span class="ob-num">{{ step1 ? "✓" : "1" }}</span>
        <span class="ob-text">
          <span class="ob-name">{{ step1 ? t("onboardingBar.step1.done") : t("onboardingBar.step1.todo") }}</span>
          <span class="ob-sub">{{ t("onboardingBar.step1.sub") }}</span>
        </span>
      </button>
      <span class="ob-arrow">→</span>
      <button
        class="ob-step"
        :class="{ done: step2, current: step1 && !step2 }"
        @click="openBrowse"
      >
        <span class="ob-num">{{ step2 ? "✓" : "2" }}</span>
        <span class="ob-text">
          <span class="ob-name">{{ step2 ? t("onboardingBar.step2.done") : t("onboardingBar.step2.todo") }}</span>
          <span class="ob-sub">{{ t("onboardingBar.step2.sub") }}</span>
        </span>
      </button>
      <span class="ob-arrow">→</span>
      <button
        class="ob-step"
        :class="{ done: launched, current: step1 && step2 && !launched }"
        @click="emit('launch')"
      >
        <span class="ob-num">{{ launched ? "✓" : "3" }}</span>
        <span class="ob-text">
          <span class="ob-name">{{ launched ? t("onboardingBar.step3.done") : t("onboardingBar.step3.todo") }}</span>
          <span class="ob-sub">{{ launched ? t("onboardingBar.step3.subDone") : t("onboardingBar.step3.subTodo") }}</span>
        </span>
      </button>
    </div>
    <button class="ob-close" :title="t('onboardingBar.closeTitle')" @click="dismiss">
      <IconClose />
    </button>
  </section>
</template>

<style scoped>
.onboarding {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 12px 16px;
  border-radius: var(--radius-lg, 14px);
}
.ob-head {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--accent);
  flex-shrink: 0;
}
.ob-title-icon {
  width: 18px;
  height: 18px;
}
.ob-title {
  font-weight: 600;
  color: var(--text-1);
  white-space: nowrap;
}
.ob-steps {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.ob-step {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  border: 1px solid transparent;
  border-radius: 10px;
  background: transparent;
  cursor: pointer;
  text-align: left;
  transition: background 0.15s, border-color 0.15s;
  min-width: 0;
}
.ob-step:hover {
  background: var(--bg-3);
}
.ob-step.done {
  cursor: default;
  opacity: 0.65;
}
.ob-step.done:hover {
  background: transparent;
}
.ob-step.current {
  border-color: var(--accent);
}
.ob-num {
  width: 24px;
  height: 24px;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  font-weight: 700;
  flex-shrink: 0;
  background: var(--bg-3);
  color: var(--text-2);
}
.ob-step.done .ob-num {
  background: var(--accent);
  color: var(--bg-1);
}
.ob-step.current .ob-num {
  background: var(--accent);
  color: var(--bg-1);
}
.ob-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
}
.ob-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.ob-sub {
  font-size: 11px;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ob-arrow {
  color: var(--text-3);
  flex-shrink: 0;
}
.ob-close {
  flex-shrink: 0;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.ob-close:hover {
  background: var(--bg-3);
  color: var(--text-1);
}
.ob-close svg {
  width: 14px;
  height: 14px;
}
</style>
