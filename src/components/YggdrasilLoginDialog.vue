<script setup lang="ts">
import { computed, ref } from "vue";
import { NButton, NInput, NModal, NSelect, useMessage } from "naive-ui";
import { api } from "../api";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Account } from "../types";

const emit = defineEmits<{ added: [a: Account] }>();
const message = useMessage();

// LittleSkin 预置：官方主页与注册页（没有账号的用户可直接去注册）
const LITTLE_ROOT = "https://littleskin.cn/api/yggdrasil";
const LITTLE_LINKS = { homepage: "https://littleskin.cn", register: "https://littleskin.cn/auth/register" };

const show = ref(false);
const serverChoice = ref<"little" | "custom">("little");
const customRoot = ref("");
const username = ref("");
const password = ref("");
const loggingIn = ref(false);
const adding = ref(false);

interface LoginResult {
  accessToken: string;
  clientToken: string;
  server: string;
  serverName: string;
  links: { homepage: string; register: string };
  profiles: { id: string; name: string }[];
}
const result = ref<LoginResult | null>(null);
const chosenProfile = ref("");

const serverOptions = [
  { label: "LittleSkin", value: "little" },
  { label: "自定义皮肤站", value: "custom" },
];
const apiRoot = computed(() =>
  serverChoice.value === "custom" ? customRoot.value.trim() : LITTLE_ROOT
);
const serverLabel = computed(() =>
  serverChoice.value === "custom"
    ? result.value?.serverName || customRoot.value.trim() || "自定义皮肤站"
    : "LittleSkin"
);
const homepageUrl = computed(() =>
  serverChoice.value === "custom" ? result.value?.links.homepage || "" : LITTLE_LINKS.homepage
);
const registerUrl = computed(() =>
  serverChoice.value === "custom" ? result.value?.links.register || "" : LITTLE_LINKS.register
);

function openLink(url: string) {
  if (url) openUrl(url).catch((e: unknown) => message.error("打开失败：" + String(e)));
}

function open() {
  serverChoice.value = "little";
  customRoot.value = "";
  username.value = "";
  password.value = "";
  result.value = null;
  chosenProfile.value = "";
  show.value = true;
}
defineExpose({ open });

async function login() {
  if (!apiRoot.value) {
    message.warning("请填写皮肤站地址");
    return;
  }
  loggingIn.value = true;
  try {
    result.value = await api.yggdrasilLogin(apiRoot.value, username.value, password.value);
    const profiles = result.value.profiles;
    // 单角色直接进入添加，多角色让用户挑选
    chosenProfile.value = profiles.length === 1 ? profiles[0].id : "";
    if (profiles.length === 1) await add();
  } catch (e) {
    message.error(String(e));
  } finally {
    loggingIn.value = false;
  }
}

async function add() {
  const r = result.value;
  if (!r) return;
  const profile = r.profiles.find((p) => p.id === chosenProfile.value);
  if (!profile) {
    message.warning("请选择一个角色");
    return;
  }
  adding.value = true;
  try {
    const acc = await api.yggdrasilAddAccount({
      serverUrl: r.server,
      serverName: r.serverName,
      accessToken: r.accessToken,
      clientToken: r.clientToken,
      profileId: profile.id,
      profileName: profile.name,
    });
    // 成功提示由 AccountChip 的 onYggAdded 统一发，避免重复 toast
    show.value = false;
    emit("added", acc);
  } catch (e) {
    message.error(String(e));
  } finally {
    adding.value = false;
  }
}
</script>

<template>
  <n-modal
    v-model:show="show"
    preset="card"
    title="添加皮肤站账号（authlib-injector）"
    style="width: 440px; max-width: 92vw"
  >
    <div class="ygg-box">
      <template v-if="!result">
        <n-select
          v-model:value="serverChoice"
          :options="serverOptions"
          size="small"
          placeholder="选择皮肤站"
        />
        <n-input
          v-if="serverChoice === 'custom'"
          v-model:value="customRoot"
          class="ygg-input"
          placeholder="Yggdrasil API 地址，如 https://skin.example.com/api/yggdrasil"
        />
        <!-- 认证服务器信息行：站名 + 主页 / 注册（参考 HMCL 外置登录） -->
        <div class="ygg-server">
          <span class="ygg-server-label">认证服务器：</span>
          <span class="ygg-server-name">{{ serverLabel }}</span>
          <span class="ygg-server-links">
            <a v-if="homepageUrl" class="ygg-link" @click.prevent="openLink(homepageUrl)">主页</a>
            <a v-if="registerUrl" class="ygg-link" @click.prevent="openLink(registerUrl)">注册</a>
          </span>
        </div>
        <n-input
          v-model:value="username"
          class="ygg-input"
          placeholder="用户名 / 邮箱"
          @keyup.enter="login"
        />
        <n-input
          v-model:value="password"
          class="ygg-input"
          type="password"
          show-password-on="click"
          placeholder="密码"
          @keyup.enter="login"
        />
        <p class="ygg-hint">
          支持 Blessing Skin 系皮肤站（authlib-injector）。没有账号？点上面的「注册」去皮肤站创建。
          密码仅在登录瞬间使用，本地只保存令牌。
        </p>
        <n-button type="primary" block :loading="loggingIn" @click="login">
          {{ loggingIn ? "登录中…" : "登录" }}
        </n-button>
      </template>
      <template v-else>
        <p class="ygg-hint">登录成功（{{ result.serverName }}）。请选择要添加的角色：</p>
        <div class="ygg-profiles">
          <button
            v-for="p in result.profiles"
            :key="p.id"
            class="ygg-profile"
            :class="{ on: chosenProfile === p.id }"
            @click="chosenProfile = p.id"
          >
            <span class="ygg-pname">{{ p.name }}</span>
            <span class="ygg-pid">{{ p.id }}</span>
          </button>
        </div>
        <n-button type="primary" block :loading="adding" @click="add">添加该角色</n-button>
      </template>
    </div>
  </n-modal>
</template>

<style scoped>
.ygg-box {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.ygg-hint {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-3);
}
.ygg-server {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}
.ygg-server-label {
  color: var(--text-3);
}
.ygg-server-name {
  color: var(--text-1);
}
.ygg-server-links {
  margin-left: auto;
  display: inline-flex;
  gap: 12px;
}
.ygg-link {
  color: var(--accent);
  cursor: pointer;
  text-decoration: none;
}
.ygg-link:hover {
  text-decoration: underline;
}
.ygg-profiles {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.ygg-profile {
  display: flex;
  flex-direction: column;
  gap: 2px;
  text-align: left;
  padding: 9px 12px;
  border-radius: 10px;
  border: 1px solid var(--w-08);
  background: var(--w-04);
  cursor: pointer;
}
.ygg-profile:hover {
  border-color: var(--w-14);
}
.ygg-profile.on {
  border-color: var(--accent);
  background: var(--accent-08);
}
.ygg-pname {
  color: var(--text-1);
  font-size: 13px;
}
.ygg-pid {
  color: var(--text-3);
  font-size: 11px;
  font-family: "Cascadia Code", Consolas, monospace;
}
</style>
