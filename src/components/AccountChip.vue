<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { NButton, NInput, NModal, NPopover, useMessage } from "naive-ui";
import { useAccountsStore } from "../stores/accounts";
import MsLoginDialog from "./MsLoginDialog.vue";
import { IconCheck, IconChevronDown, IconTrash, IconUser, IconPlus } from "./icons";
import type { Account } from "../types";

const props = defineProps<{ collapsed?: boolean }>();

const accounts = useAccountsStore();
const message = useMessage();

const popoverShow = ref(false);
const showOfflineDialog = ref(false);
const offlineName = ref("");
const addingOffline = ref(false);

const offlineAvatarCache = reactive<Record<string, string>>({});

function skinToAvatar(skinDataUrl: string): Promise<string> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = 8;
      canvas.height = 8;
      const ctx = canvas.getContext("2d");
      if (!ctx) return resolve("");
      ctx.imageSmoothingEnabled = false;
      ctx.drawImage(img, 8, 8, 8, 8, 0, 0, 8, 8); // 头部基础层
      if (img.height >= 64) {
        ctx.drawImage(img, 40, 8, 8, 8, 0, 0, 8, 8); // 头部第二层（overlay）
      }
      resolve(canvas.toDataURL("image/png"));
    };
    img.onerror = () => resolve("");
    img.src = skinDataUrl;
  });
}

function getOfflineAvatar(uuid: string): string {
  return offlineAvatarCache[uuid] ?? "";
}

watch(
  () => accounts.accounts.map((a) => `${a.uuid}:${a.type}`).join(","),
  async () => {
    for (const acc of accounts.accounts) {
      if (acc.type === "offline" && !offlineAvatarCache[acc.uuid]) {
        const skin = localStorage.getItem(`qookix:offline_skin:${acc.uuid}`);
        if (skin) {
          const avatar = await skinToAvatar(skin);
          if (avatar) offlineAvatarCache[acc.uuid] = avatar;
        }
      }
    }
  },
  { immediate: true },
);

watch(
  () => accounts.avatarVersion,
  async () => {
    const cur = accounts.current;
    if (cur && cur.type === "offline") {
      const skin = localStorage.getItem(`qookix:offline_skin:${cur.uuid}`);
      if (skin) {
        const avatar = await skinToAvatar(skin);
        if (avatar) offlineAvatarCache[cur.uuid] = avatar;
      }
    }
  },
);

watch(
  () => accounts.msSuccess,
  (msg) => {
    if (msg) {
      message.success(msg);
      accounts.msSuccess = "";
      popoverShow.value = true;
    }
  }
);

watch(
  () => accounts.msFailed,
  (msg) => {
    if (msg) {
      message.error(msg, { duration: 8000 });
      accounts.msFailed = "";
    }
  }
);

watch(
  () => accounts.showManager,
  (v) => {
    if (v) popoverShow.value = true;
  }
);

watch(popoverShow, (v) => {
  if (!v) accounts.showManager = false;
});

const current = computed(() => accounts.current);

/** Avatar providers tried in order; falls back to the local user icon when all fail. */
const AVATAR_SOURCES: Array<(uuid: string) => string> = [
  (u) => `https://mc-heads.net/avatar/${u}/96`,
  (u) => `https://minotar.net/helm/${u}/96`,
  (u) => `https://crafatar.com/avatars/${u}?size=96&overlay`,
];

/** Number of failed providers per account uuid (drives the fallback chain). */
const avatarAttempt = reactive<Record<string, number>>({});

function avatar(uuid: string): string {
  const offline = getOfflineAvatar(uuid);
  if (offline) return offline;
  const i = avatarAttempt[uuid] ?? 0;
  if (i < AVATAR_SOURCES.length) {
    const base = AVATAR_SOURCES[i](uuid);
    const sep = base.includes("?") ? "&" : "?";
    return `${base}${sep}v=${accounts.avatarVersion}`;
  }
  return "";
}

function onAvatarError(uuid: string) {
  const next = (avatarAttempt[uuid] ?? 0) + 1;
  if (next <= AVATAR_SOURCES.length) avatarAttempt[uuid] = next;
}

async function select(acc: Account) {
  await accounts.select(acc.uuid);
  message.success(`当前游玩账号：${acc.username}`);
  popoverShow.value = false;
}

function openOfflineDialog() {
  offlineName.value = "";
  showOfflineDialog.value = true;
}

async function addOffline() {
  const name = offlineName.value.trim();
  if (!name) {
    message.warning("请输入用户名");
    return;
  }
  addingOffline.value = true;
  try {
    await accounts.addOffline(name);
    showOfflineDialog.value = false;
    message.success("离线账号已添加");
  } catch (e) {
    message.error(String(e));
  } finally {
    addingOffline.value = false;
  }
}

async function startMs() {
  popoverShow.value = false;
  try {
    await accounts.startMs();
    if (accounts.msError) {
      message.error(accounts.msError);
      accounts.msError = "";
    }
  } catch (e) {
    message.error(String(e));
  }
}

function remove(acc: Account) {
  accounts
    .remove(acc.uuid)
    .then(() => message.success("账号已移除"))
    .catch((e) => message.error(String(e)));
}

function typeLabel(a: Account) {
  return a.type === "microsoft" ? "正版" : "离线";
}
</script>

<template>
  <n-popover
    v-model:show="popoverShow"
    trigger="click"
    placement="top-start"
    :width="320"
    :show-arrow="false"
    class="acctm-popover"
  >
    <template #trigger>
      <div class="acct-chip clickable" :class="{ empty: !accounts.accounts.length, collapsed: props.collapsed }">
        <div class="avatar">
          <template v-if="current">
            <img v-if="avatar(current.uuid)" :src="avatar(current.uuid)" alt="" @error="onAvatarError(current.uuid)" />
            <IconUser v-else />
          </template>
          <IconUser v-else />
        </div>
        <template v-if="!props.collapsed">
          <div class="acct-info">
            <div class="acct-name text-ellipsis">{{ current?.username ?? "未登录" }}</div>
            <div class="acct-type">
              {{
                current
                  ? current.type === "microsoft"
                    ? "正版账号"
                    : "离线账号"
                  : "点击添加账号"
              }}
            </div>
          </div>
          <IconChevronDown v-if="accounts.accounts.length" class="chev" />
          <IconUser v-else class="chev" />
        </template>
      </div>
    </template>

    <div class="acctm-body">
      <div class="acctm-title">当前游玩账号</div>

      <div v-if="!accounts.accounts.length" class="acctm-empty">还没有账号</div>
      <div v-else class="acctm-list">
        <div
          v-for="acc in accounts.accounts"
          :key="acc.uuid"
          class="acctm-row"
          :class="{ active: current?.uuid === acc.uuid }"
          @click="select(acc)"
        >
          <img v-if="avatar(acc.uuid)" :src="avatar(acc.uuid)" class="acctm-avatar" alt="" @error="onAvatarError(acc.uuid)" />
          <IconUser v-else class="acctm-avatar acctm-avatar-fallback" />
          <div class="acctm-info">
            <div class="acctm-name text-ellipsis">{{ acc.username }}</div>
            <span class="acctm-type" :class="acc.type">{{ typeLabel(acc) }}</span>
          </div>
          <IconCheck v-if="current?.uuid === acc.uuid" class="acctm-check" />
          <button class="acctm-remove" title="移除账号" @click.stop="remove(acc)">
            <IconTrash />
          </button>
        </div>
      </div>

      <div class="acctm-divider"></div>

      <div class="acctm-add">
        <button class="acctm-btn ms" @click="startMs">
          <IconPlus /> 添加 Microsoft 账户
        </button>
        <button class="acctm-btn" @click="openOfflineDialog">
          <IconPlus /> 添加离线账号
        </button>
      </div>
    </div>
  </n-popover>

  <!-- offline account name dialog -->
  <n-modal
    v-model:show="showOfflineDialog"
    preset="card"
    title="添加离线账号"
    style="width: 380px; max-width: 90vw"
  >
    <div class="acctm-offline-box">
      <n-input
        v-model:value="offlineName"
        placeholder="游戏内用户名（≤16 字符）"
        :maxlength="16"
        clearable
        @keyup.enter="addOffline"
      />
      <p class="acctm-offline-hint">离线账号的 UUID 由用户名确定，可与官方启动器互通。</p>
    </div>
    <template #footer>
      <div class="acctm-offline-footer">
        <n-button @click="showOfflineDialog = false">取消</n-button>
        <n-button type="primary" :loading="addingOffline" @click="addOffline">添加</n-button>
      </div>
    </template>
  </n-modal>

  <MsLoginDialog />
</template>

<style scoped>
/* chip (rendered in place, scoped styles are fine) */
.acct-chip {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 10px;
  height: 48px;
  box-sizing: border-box;
  border-radius: 12px;
  background: var(--panel);
  border: 1px solid var(--border);
}
.acct-chip.empty {
  opacity: 0.85;
}
.acct-chip.collapsed {
  justify-content: center;
  padding: 0;
}
.acct-chip.collapsed .avatar {
  width: 48px;
  height: 48px;
  border-radius: 11px;
}
.avatar {
  width: 34px;
  height: 34px;
  border-radius: 8px;
  overflow: hidden;
  background: rgba(255, 255, 255, 0.08);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  font-size: 17px;
  flex-shrink: 0;
  transition: width 0.2s ease, height 0.2s ease;
}
.avatar img {
  width: 100%;
  height: 100%;
  image-rendering: pixelated;
}
.acct-info {
  min-width: 0;
  flex: 1;
  animation: fade-in 0.2s ease;
}
.acct-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.acct-type {
  font-size: 11px;
  color: var(--text-3);
}
.chev {
  color: var(--text-3);
  font-size: 14px;
  flex-shrink: 0;
  animation: fade-in 0.2s ease;
}
@keyframes fade-in {
  from { opacity: 0; }
  to { opacity: 1; }
}
</style>

<!-- Global styles: popover + modal content is teleported to <body> by naive-ui,
     so scoped styles do not reliably apply there. -->
<style>
.acctm-popover {
  padding: 14px;
}
.acctm-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.acctm-title {
  font-size: 12px;
  font-weight: 700;
  color: #8b8e9c;
  letter-spacing: 0.5px;
}
.acctm-empty {
  font-size: 12px;
  color: #8b8e9c;
  padding: 8px 0;
}
.acctm-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 220px;
  overflow-y: auto;
}
.acctm-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 9px;
  border-radius: 9px;
  cursor: pointer;
  border: 1px solid transparent;
  transition: all 0.12s;
}
.acctm-row:hover {
  background: rgba(255, 255, 255, 0.06);
}
.acctm-row.active {
  background: rgba(232, 154, 75, 0.14);
  border-color: rgba(232, 154, 75, 0.35);
}
.acctm-avatar {
  width: 30px;
  height: 30px;
  border-radius: 8px;
  image-rendering: pixelated;
  background: rgba(255, 255, 255, 0.06);
  flex-shrink: 0;
}
.acctm-avatar-fallback {
  display: flex;
  align-items: center;
  justify-content: center;
  color: #8b8e9c;
  font-size: 14px;
}
.acctm-info {
  flex: 1;
  min-width: 0;
}
.acctm-name {
  font-size: 13px;
  font-weight: 600;
  color: #f2f3f7;
}
.acctm-type {
  font-size: 10px;
  padding: 1px 7px;
  border-radius: 6px;
  font-weight: 600;
}
.acctm-type.microsoft {
  background: rgba(90, 162, 240, 0.15);
  color: #7cb8f5;
}
.acctm-type.offline {
  background: rgba(255, 255, 255, 0.08);
  color: #c6c8d2;
}
.acctm-check {
  color: #e89a4b;
  font-size: 14px;
  flex-shrink: 0;
}
.acctm-remove {
  width: 26px;
  height: 26px;
  border: none;
  background: transparent;
  color: #8b8e9c;
  border-radius: 7px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  font-size: 12px;
  opacity: 0;
  transition: all 0.12s;
}
.acctm-row:hover .acctm-remove {
  opacity: 1;
}
.acctm-remove:hover {
  color: #e5534b;
  background: rgba(229, 83, 75, 0.12);
}
.acctm-divider {
  border-top: 1px solid rgba(255, 255, 255, 0.08);
  margin: 2px 0;
}
.acctm-add {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.acctm-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  width: 100%;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.05);
  color: #f2f3f7;
  border-radius: 9px;
  padding: 9px 12px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  white-space: nowrap;
  transition: all 0.12s;
}
.acctm-btn:hover {
  background: rgba(255, 255, 255, 0.1);
}
.acctm-btn.ms {
  background: rgba(232, 154, 75, 0.14);
  border-color: rgba(232, 154, 75, 0.45);
  color: #e89a4b;
}
.acctm-btn.ms:hover {
  background: rgba(232, 154, 75, 0.22);
}
.acctm-offline-box {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.acctm-offline-hint {
  margin: 0;
  font-size: 12px;
  color: #8b8e9c;
}
.acctm-offline-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
</style>
