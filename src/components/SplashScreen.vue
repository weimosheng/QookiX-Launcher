<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

const props = withDefaults(
  defineProps<{
    /** 0..100 */
    progress?: number;
    status?: string;
    /** 是否已完成加载，触发淡出 */
    done?: boolean;
  }>(),
  { progress: 0, status: "", done: false },
);

const visible = ref(true);
const renderedProgress = ref(0);

const clamped = computed(() => Math.max(0, Math.min(100, props.progress)));

watch(clamped, (v) => {
  // 进度条平滑跟随，避免数字跳变
  renderedProgress.value = v;
});

onMounted(() => {
  renderedProgress.value = clamped.value;
});

function onLeave() {
  // 淡出动画结束后彻底卸载
  visible.value = false;
}
</script>

<template>
  <Transition name="splash-fade" @after-leave="onLeave">
    <div v-if="!done" class="splash">
      <div class="splash-bg" />
      <div class="splash-content">
        <div class="splash-logo">
          <img src="/app-icon.png" alt="QookiX Launcher" draggable="false" />
          <div class="splash-logo-glow" />
        </div>
        <h1 class="splash-title">QookiX Launcher</h1>
        <p class="splash-sub">{{ status || "正在启动…" }}</p>
        <div class="splash-bar" :aria-valuenow="renderedProgress">
          <div class="splash-bar-fill" :style="{ width: renderedProgress + '%' }" />
        </div>
        <div class="splash-percent">{{ Math.round(renderedProgress) }}%</div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.splash {
  position: fixed;
  inset: 0;
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}
.splash-bg {
  position: absolute;
  inset: 0;
  background:
    radial-gradient(620px 420px at 50% 38%, var(--accent-22), transparent 60%),
    linear-gradient(180deg, #0b0d12, #0f1218 60%, #0b0d12);
}
.splash-content {
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 14px;
  user-select: none;
}
.splash-logo {
  position: relative;
  width: 96px;
  height: 96px;
  border-radius: 22px;
  overflow: hidden;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.45);
}
.splash-logo img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.splash-logo-glow {
  position: absolute;
  inset: -40%;
  background: radial-gradient(circle, var(--accent-30), transparent 60%);
  pointer-events: none;
  animation: splash-pulse 2.4s ease-in-out infinite;
}
@keyframes splash-pulse {
  0%, 100% { opacity: 0.5; transform: scale(1); }
  50% { opacity: 1; transform: scale(1.08); }
}
.splash-title {
  margin: 6px 0 0;
  font-size: 26px;
  font-weight: 700;
  letter-spacing: 0.6px;
  color: #f2f3f7;
}
.splash-sub {
  margin: 0;
  font-size: 13px;
  color: var(--text-3, #8b8e9c);
  min-height: 18px;
  letter-spacing: 0.3px;
}
.splash-bar {
  width: 280px;
  max-width: 72vw;
  height: 4px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.08);
  overflow: hidden;
  margin-top: 4px;
}
.splash-bar-fill {
  height: 100%;
  border-radius: 999px;
  background: linear-gradient(90deg, var(--accent, #e89a4b), var(--accent-hover, #f2a860));
  box-shadow: 0 0 12px var(--accent-45, rgba(232, 154, 75, 0.45));
  transition: width 0.32s cubic-bezier(0.22, 1, 0.36, 1);
}
.splash-percent {
  font-size: 12px;
  color: var(--text-3, #8b8e9c);
  font-variant-numeric: tabular-nums;
}

.splash-fade-leave-active {
  transition: opacity 0.4s ease;
}
.splash-fade-leave-to {
  opacity: 0;
}
</style>
