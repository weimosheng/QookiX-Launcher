<script setup lang="ts">
import { onMounted, onBeforeUnmount, computed, watch, ref, provide } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { darkTheme, lightTheme, NConfigProvider, NDialogProvider, NLoadingBarProvider, NMessageProvider, NNotificationProvider } from "naive-ui";
import TitleBar from "./components/TitleBar.vue";
import SideBar from "./components/SideBar.vue";
import LoadingBarBridge from "./components/LoadingBarBridge.vue";
import LaunchProgress from "./components/LaunchProgress.vue";
import CrashDialog from "./components/CrashDialog.vue";
import UpdaterCheck from "./components/UpdaterCheck.vue";
import SplashScreen from "./components/SplashScreen.vue";
import { useSettingsStore } from "./stores/settings";
import { initDeepLink } from "./composables/deepLink";
import { MessageBridge } from "./composables/notify";

import { useAccountsStore } from "./stores/accounts";
import { useInstancesStore } from "./stores/instances";
import { useTasksStore } from "./stores/tasks";
import { usePinsStore } from "./stores/pins";
import {
  buildDarkOverrides,
  buildLightOverrides,
  DEFAULT_ACCENT,
  darken,
  lighten,
  rgba,
  ACCENT_ALPHAS,
} from "./theme";

const settings = useSettingsStore();
const accounts = useAccountsStore();
const instances = useInstancesStore();
const tasks = useTasksStore();
const pins = usePinsStore();

const isDark = computed(() => settings.settings?.theme !== "light");
const activeTheme = computed(() => (isDark.value ? darkTheme : lightTheme));

const accentColor = computed(() => settings.settings?.theme_color || DEFAULT_ACCENT);
const themeOverrides = computed(() =>
  isDark.value ? buildDarkOverrides(accentColor.value) : buildLightOverrides(accentColor.value),
);

// 首次应用主题不做过渡（否则页面加载时会闪一下颜色）
let themeReady = false;
let themeTransitionTimer: ReturnType<typeof setTimeout> | null = null;

watch(isDark, () => {
  const root = document.documentElement;
  if (themeReady) {
    // 切换瞬间临时开启全局颜色过渡，结束后移除，避免影响性能与 hover 动画
    root.classList.add("theme-transition");
    if (themeTransitionTimer) clearTimeout(themeTransitionTimer);
    themeTransitionTimer = setTimeout(() => {
      root.classList.remove("theme-transition");
      themeTransitionTimer = null;
    }, 320);
  }
  root.classList.toggle("light", !isDark.value);
  themeReady = true;
}, { immediate: true });

onBeforeUnmount(() => {
  if (themeTransitionTimer) clearTimeout(themeTransitionTimer);
});

// 将主题色应用到 CSS 变量上（accent / accent-deep / accent-soft / 各级 alpha）
watch(accentColor, (hex) => {
  const root = document.documentElement;
  root.style.setProperty("--accent", hex);
  root.style.setProperty("--accent-deep", darken(hex));
  root.style.setProperty("--accent-hover", lighten(hex));
  root.style.setProperty("--accent-soft", rgba(hex, 0.14));
  for (const a of ACCENT_ALPHAS) {
    root.style.setProperty(`--accent-${String(Math.round(a * 100)).padStart(2, "0")}`, rgba(hex, a));
  }
}, { immediate: true });

const bgStyle = computed(() => {
  const s = settings.settings;
  if (!s?.background_image) return {} as Record<string, string>;
  return {
    "--bg-image": `url("${convertFileSrc(s.background_image)}")`,
    "--bg-blur": `${s.background_blur}px`,
    "--bg-dim": String(s.background_dim / 100),
    "--bg-dim-light": String((s.background_dim / 100) * 0.45),
  } as Record<string, string>;
});

watch(() => settings.settings?.glass_blur, (v) => {
  if (v != null) document.documentElement.style.setProperty("--glass-blur", `${v}px`);
}, { immediate: true });

watch(() => settings.settings?.background_image, (p) => {
  document.documentElement.classList.toggle("has-bg", !!p);
}, { immediate: true });

// 供 TitleBar 的“新建分组”按钮触发实例页的分组对话框
const groupDialogRequest = ref(0);
provide("groupDialogRequest", groupDialogRequest);

// —— 启动加载流程 ——
// 在 splash 期间预加载核心数据，让首屏直接有数据可渲染；
// 各 store 的 load() 已是「已有数据则后台静默刷新」模式，这里首次加载会真正拉取。
const bootProgress = ref(0);
const bootStatus = ref("正在启动…");
const booted = ref(false);

async function boot() {
  bootStatus.value = "加载设置…";
  try {
    await settings.load();
  } catch {
    /* 设置加载失败不阻塞启动 */
  }
  bootProgress.value = 24;

  bootStatus.value = "读取实例与账号…";
  await Promise.all([
    instances.load().catch(() => {}),
    accounts.load().catch(() => {}),
  ]);
  bootProgress.value = 72;

  bootStatus.value = "初始化任务系统…";
  tasks.init();
  await pins.init().catch(() => {});
  void initDeepLink();
  bootProgress.value = 92;

  bootStatus.value = "即将就绪…";
  bootProgress.value = 100;
  // 让 100% 短暂展示后再淡出，避免进度条一闪而过
  setTimeout(() => {
    booted.value = true;
  }, 340);
}

onMounted(() => {
  void boot();
});
</script>

<template>
  <n-config-provider :theme="activeTheme" :theme-overrides="themeOverrides" :inline-theme-disabled="true">
    <n-loading-bar-provider>
      <LoadingBarBridge>
        <n-dialog-provider>
          <n-message-provider>
            <MessageBridge />
            <n-notification-provider>
                <div class="app app-bg" :class="{ light: !isDark }" :style="bgStyle">
                <TitleBar />
                <div class="body">
                  <SideBar />
                  <main class="content">
                    <router-view v-slot="{ Component, route }">
                      <Transition name="page-rise" mode="out-in">
                        <component :is="Component" :key="route.path" />
                      </Transition>
                    </router-view>
                  </main>
                </div>
                </div>
                <LaunchProgress />
                <CrashDialog />
                <UpdaterCheck />
                <SplashScreen :progress="bootProgress" :status="bootStatus" :done="booted" />
              </n-notification-provider>
          </n-message-provider>
        </n-dialog-provider>
      </LoadingBarBridge>
    </n-loading-bar-provider>
  </n-config-provider>
</template>

<style scoped>
.app {
  height: 100vh;
  display: flex;
  flex-direction: column;
}
.app.light {
  background: radial-gradient(900px 480px at 85% -10%, var(--accent-12), transparent 60%),
    linear-gradient(180deg, #f7f6f4, #eceef2);
}
.body {
  flex: 1;
  display: flex;
  min-height: 0;
}
.content {
  flex: 1;
  overflow-y: auto;
  padding: 22px 26px 30px;
}
</style>

