<script setup lang="ts">
import { ref, watch } from "vue";
import { NButton, NModal } from "naive-ui";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAccountsStore } from "../stores/accounts";

const accounts = useAccountsStore();
const show = ref(false);

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    document.body.removeChild(ta);
  }
}

watch(
  () => accounts.msFlow,
  (f) => {
    show.value = !!f;
    if (f) {
      if (f.userCode) copyText(f.userCode).catch(() => {});
      if (f.verificationUri) openUrl(f.verificationUri).catch(() => {});
    }
  },
  { immediate: true }
);

async function copyCode() {
  if (!accounts.msFlow?.userCode) return;
  await copyText(accounts.msFlow.userCode);
}

function close() {
  show.value = false;
  accounts.msFlow = null;
  accounts.msError = "";
}

async function retry() {
  accounts.msError = "";
  await accounts.startMs();
}
</script>

<template>
  <n-modal
    :show="show || !!accounts.msError"
    preset="card"
    title="登录 Microsoft 账户"
    style="width: 460px; max-width: 92vw"
    :mask-closable="false"
  >
    <div class="qkms-box">
      <template v-if="accounts.msError">
        <div class="qkms-error-box">{{ accounts.msError }}</div>
      </template>
      <template v-else>
        <p>已自动复制代码并打开浏览器，在浏览器中粘贴并授权即可</p>
        <a class="qkms-link" @click="accounts.msFlow && openUrl(accounts.msFlow.verificationUri)">{{ accounts.msFlow?.verificationUri || "…" }}</a>
        <div class="qkms-code mono">{{ accounts.msFlow?.userCode || "等待中…" }}</div>
        <p class="qkms-hint">等待你在浏览器中完成授权，本窗口会自动继续…</p>
        <div v-if="accounts.msPolling" class="qkms-polling">正在等待授权…</div>
      </template>
    </div>
    <template #footer>
      <div class="qkms-footer">
        <n-button @click="close">关闭</n-button>
        <template v-if="!accounts.msError">
          <n-button @click="copyCode">复制代码</n-button>
          <n-button type="primary" @click="accounts.manualCheck">我已登录</n-button>
        </template>
        <n-button v-if="accounts.msError" type="primary" @click="retry">重试</n-button>
      </div>
    </template>
  </n-modal>
</template>

<style>
.qkms-box {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
}
.qkms-box p {
  margin: 0;
  font-size: 13px;
  color: #c6c8d2;
  text-align: center;
}
.qkms-link {
  color: #e89a4b;
  font-size: 14px;
  word-break: break-all;
  cursor: pointer;
  text-decoration: underline;
}
.qkms-code {
  font-size: 30px;
  font-weight: 800;
  letter-spacing: 10px;
  text-align: center;
  color: #e89a4b;
  background: rgba(232, 154, 75, 0.08);
  border: 1px dashed rgba(232, 154, 75, 0.4);
  border-radius: 12px;
  padding: 14px 0;
  width: 100%;
}
.qkms-copy {
  border: 1px solid rgba(232, 154, 75, 0.4);
  background: rgba(232, 154, 75, 0.1);
  color: #e89a4b;
  border-radius: 8px;
  padding: 6px 18px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.qkms-copy:hover {
  background: rgba(232, 154, 75, 0.2);
}
.qkms-hint {
  font-size: 12px;
  color: #8b8e9c;
}
.qkms-polling {
  font-size: 12px;
  color: #e89a4b;
}
.qkms-error-box {
  color: #e5534b;
  font-size: 13px;
  line-height: 1.6;
  word-break: break-all;
  user-select: text;
  -webkit-user-select: text;
  cursor: text;
  background: rgba(229, 83, 75, 0.08);
  border: 1px solid rgba(229, 83, 75, 0.3);
  border-radius: 8px;
  padding: 12px;
  width: 100%;
}
.qkms-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
</style>
