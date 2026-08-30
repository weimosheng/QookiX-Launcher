<script setup lang="ts">
import { h, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { NButton, useDialog, useMessage } from "naive-ui";
import { peekUpdate, downloadAndInstall, relaunchApp } from "../updater";
import { useSettingsStore } from "../stores/settings";

const dialog = useDialog();
const message = useMessage();
const router = useRouter();
const settings = useSettingsStore();
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
    if (!settings.settings) {
      try {
        await settings.load();
      } catch {
        /* ignore */
      }
    }
    const update = await peekUpdate();
    if (!update) return;
    // 用户此前点击过「忽略此版本」且仍是同一版本：不再弹窗，避免每次启动都打扰。
    if (settings.settings?.dismissed_update_version === update.version) return;

    // 自动更新开启：检测到新版本直接下载安装，不弹窗询问
    if (settings.settings?.auto_update) {
      void doInstall();
      return;
    }

    // naive-ui 的对话框只内置 positive/negative 两个按钮，这里用 action 自定义
    // 三个按钮，额外提供「忽略此版本」。
    let dlg: { destroy: () => void } | null = null;
    const close = () => {
      dlg?.destroy();
      dlg = null;
    };
    dlg = dialog.warning({
      title: "发现新版本",
      content: `QookiX Launcher 有新版本 v${update.version}，是否下载并安装？`,
      action: () =>
        h("div", { style: "display:flex; gap:8px; justify-content:flex-end;" }, [
          h(
            NButton,
            {
              size: "small",
              quaternary: true,
              onClick: () => {
                close();
                void dismiss(update.version);
              },
            },
            { default: () => "忽略此版本" }
          ),
          h(NButton, { size: "small", ghost: true, onClick: close }, () => "以后再说"),
          h(
            NButton,
            {
              size: "small",
              type: "primary",
              onClick: () => {
                close();
                void doInstall();
              },
            },
            { default: () => "下载并更新" }
          ),
        ]),
    });
  } finally {
    checking.value = false;
  }
}

/** 点击「忽略此版本」：记录版本号，此后启动时不再提示该版本。 */
async function dismiss(version: string) {
  try {
    await settings.patch({ dismissed_update_version: version });
  } catch {
    /* 保存失败则下次继续提示，不影响使用 */
  }
}

async function doInstall() {
  // Jump to the Download Center so the user can watch the progress live.
  router.push("/downloads");
  try {
    const installed = await downloadAndInstall();
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
