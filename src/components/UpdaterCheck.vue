<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useDialog, useMessage } from "naive-ui";
import { peekUpdate, downloadAndInstall, relaunchApp } from "../updater";

const dialog = useDialog();
const message = useMessage();
const checking = ref(false);

onMounted(() => {
  // Check shortly after startup so the UI is ready and the home screen is shown.
  const t = setTimeout(() => runCheck(), 2000);
  // stop the check if the window is closed quickly
  window.addEventListener("beforeunload", () => clearTimeout(t));
});

async function runCheck() {
  if (checking.value) return;
  checking.value = true;
  try {
    const update = await peekUpdate();
    if (!update) return;
    dialog.warning({
      title: "发现新版本",
      content: `QookiX Launcher 有新版本 v${update.version}，是否下载并安装？`,
      positiveText: "下载并更新",
      negativeText: "以后再说",
      onPositiveClick: () => doInstall(),
    });
  } finally {
    checking.value = false;
  }
}

async function doInstall() {
  try {
    const installed = await downloadAndInstall((m) => message.info(m));
    if (!installed) return;
    dialog.success({
      title: "更新完成",
      content: "需要重启启动器才能生效，是否立即重启？",
      positiveText: "立即重启",
      negativeText: "稍后手动重启",
      onPositiveClick: () => relaunchApp(),
    });
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    message.error(detail || "更新失败，请稍后重试或手动下载");
    console.error("[updater] install error:", err);
  }
}
</script>

<template>
  <!-- Invisible host; runs the auto-update check against the providers above. -->
  <div style="display: none"></div>
</template>
