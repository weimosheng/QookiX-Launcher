<script setup lang="ts">
import { h, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { NButton, useDialog, useMessage } from "naive-ui";
import { peekUpdate, downloadUpdate, updateReady } from "../updater";
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
  // 已经下载好、只等重启：别再重复下载同一版本
  if (updateReady.value) return;
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

    // 自动更新开启：后台静默下载安装，不弹窗、不打断当前页面。
    // 下载完也只是点亮标题栏的「重启以更新」，绝不自动重启。
    if (settings.settings?.auto_update) {
      void doInstall(true);
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

/**
 * @param auto 自动更新路径：后台下载，不跳转页面、不弹窗询问。
 */
async function doInstall(auto = false) {
  // 手动触发时才跳转到下载中心看进度；自动更新在后台跑，不打断用户。
  if (!auto) router.push("/downloads");
  try {
    const downloaded = await downloadUpdate();
    if (!downloaded) return;
    // 只下载不安装：安装与重启由标题栏「重启以更新」按钮触发。
    message.success("更新已下载，点击标题栏的「重启以更新」安装");
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
