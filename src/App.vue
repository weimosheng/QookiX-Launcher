<script setup lang="ts">
import { onMounted, computed, watch } from "vue";
import { darkTheme, lightTheme, NConfigProvider, NDialogProvider, NLoadingBarProvider, NMessageProvider, NNotificationProvider } from "naive-ui";
import TitleBar from "./components/TitleBar.vue";
import SideBar from "./components/SideBar.vue";
import LoadingBarBridge from "./components/LoadingBarBridge.vue";
import LaunchProgress from "./components/LaunchProgress.vue";
import CrashDialog from "./components/CrashDialog.vue";
import UpdaterCheck from "./components/UpdaterCheck.vue";
import { useSettingsStore } from "./stores/settings";
import { useAccountsStore } from "./stores/accounts";
import { useInstancesStore } from "./stores/instances";
import { useTasksStore } from "./stores/tasks";
import { darkThemeOverrides, lightThemeOverrides } from "./theme";

const settings = useSettingsStore();
const accounts = useAccountsStore();
const instances = useInstancesStore();
const tasks = useTasksStore();

const isDark = computed(() => settings.settings?.theme !== "light");
const activeTheme = computed(() => (isDark.value ? darkTheme : lightTheme));
const themeOverrides = computed(() => (isDark.value ? darkThemeOverrides : lightThemeOverrides));

watch(isDark, () => {
  document.documentElement.classList.toggle("light", !isDark.value);
}, { immediate: true });

onMounted(() => {
  tasks.init();
  settings.load();
  accounts.load();
  instances.load();
});
</script>

<template>
  <n-config-provider :theme="activeTheme" :theme-overrides="themeOverrides" :inline-theme-disabled="true">
    <n-loading-bar-provider>
      <LoadingBarBridge>
        <n-dialog-provider>
          <n-message-provider>
            <n-notification-provider>
              <div class="app app-bg" :class="{ light: !isDark }">
                <TitleBar />
                <div class="body">
                  <SideBar />
                  <main class="content">
                    <router-view v-slot="{ Component }">
                      <component :is="Component" />
                    </router-view>
                  </main>
                </div>
                </div>
                <LaunchProgress />
                <CrashDialog />
                <UpdaterCheck />
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
  background: radial-gradient(900px 480px at 85% -10%, rgba(232, 154, 75, 0.12), transparent 60%),
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

