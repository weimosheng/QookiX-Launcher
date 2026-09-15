<script setup lang="ts">
/**
 * 云存档同步弹窗（GitHub 私有仓库）。
 * 三种状态：未连接（设备流程授权）→ 初始化仓库 → 快照管理
 * （上传 / 恢复 / 删除 / 自动同步开关 / 每世界保留数）。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { listen } from "@tauri-apps/api/event";
import { NButton, NModal, NSelect, NSwitch, useMessage } from "naive-ui";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../api";
import { useInstancesStore } from "../stores/instances";
import type { CloudSnapshot } from "../types";
import { fmtSize } from "../utils/format";
import {
  IconCloud,
  IconDownloadCloud,
  IconGithub,
  IconTrash,
  IconUploadCloud,
} from "./icons";

const props = defineProps<{
  show: boolean;
  /** 恢复目标实例；浏览模式（world 为空）时可由用户在弹窗内选择 */
  instanceId: string;
  /** 传入空串即为"云端浏览"模式 */
  world: string;
  /** 实例的 MC 版本，随快照记录 */
  gameVersion: string;
}>();
const emit = defineEmits<{ (e: "update:show", v: boolean): void; (e: "restored"): void }>();

const message = useMessage();
const instances = useInstancesStore();

type Phase = "loading" | "connect" | "authing" | "ready";
const phase = ref<Phase>("loading");
const account = ref("");
const repoName = ref("");
const keepPerWorld = ref(5);
const autoSync = ref(false);
const snapshots = ref<CloudSnapshot[]>([]);
const worldId = ref("");

// 授权流程状态
const userCode = ref("");
const verificationUri = ref("");
const deviceCode = ref("");
const authError = ref("");
const codeCopied = ref(false);
const authCountdown = ref(0);
let pollTimer: number | null = null;

const busy = ref("");

// 浏览模式（world 为空）：从云端找回本地已丢失的世界。
// 严格按实例隔离：只显示/恢复所选实例自己的快照。
const browseMode = computed(() => !props.world.trim());
interface CloudWorldGroup {
  worldId: string;
  worldName: string;
  instanceId: string | null;
  instanceName: string | null;
  snapshots: CloudSnapshot[];
}
const cloudWorlds = ref<CloudWorldGroup[]>([]);
const expandedWorld = ref("");
const targetName = ref("");
/** 浏览/恢复目标实例：默认当前实例；从设置页进入时需先选择 */
const targetInstanceId = ref(props.instanceId);
const instanceOptions = computed(() =>
  instances.instances.map((i) => ({
    label: `${i.name}（${i.mc_version}）`,
    value: i.id,
  })),
);
/** 只显示目标实例自己的快照（上传时已打实例标签） */
const visibleWorlds = computed(() =>
  cloudWorlds.value.filter((g) => g.instanceId === targetInstanceId.value),
);

// 后端推送的进度：{ kind, step, msg, sent, total }
const progress = ref<{ kind: string; step: string; msg?: string; sent?: number; total?: number } | null>(null);
const progressPercent = computed(() => {
  const p = progress.value;
  if (!p || !p.total || p.sent == null) return null;
  return Math.min(100, Math.round((p.sent / p.total) * 100));
});
// 速度与剩余时间：根据事件增量估算（指数平滑）
const speedState = { lastSent: -1, lastTs: 0, ema: 0 };
const speedBps = ref(0);
const etaText = ref("");
function updateSpeed(p: { sent?: number; total?: number }) {
  if (p.sent == null || !p.total) {
    speedBps.value = 0;
    etaText.value = "";
    speedState.lastSent = -1;
    return;
  }
  const now = Date.now();
  if (speedState.lastSent < 0 || p.sent < speedState.lastSent) {
    speedState.lastSent = p.sent;
    speedState.lastTs = now;
    return;
  }
  const dt = (now - speedState.lastTs) / 1000;
  if (dt < 0.3) return; // 采样太密不更新
  const inst = (p.sent - speedState.lastSent) / dt;
  speedState.ema = speedState.ema === 0 ? inst : speedState.ema * 0.7 + inst * 0.3;
  speedState.lastSent = p.sent;
  speedState.lastTs = now;
  speedBps.value = speedState.ema;
  const remain = Math.max(0, p.total - p.sent);
  etaText.value =
    speedState.ema > 1 ? fmtEta(remain / speedState.ema) : "";
}
function fmtEta(secs: number) {
  if (!Number.isFinite(secs) || secs <= 0) return "";
  if (secs < 60) return `剩余约 ${Math.ceil(secs)} 秒`;
  const m = Math.floor(secs / 60);
  const s = Math.round(secs % 60);
  return s > 0 ? `剩余约 ${m} 分 ${s} 秒` : `剩余约 ${m} 分钟`;
}
let unlistenProgress: (() => void) | null = null;
const showValue = computed({
  get: () => props.show,
  set: (v: boolean) => emit("update:show", v),
});

async function loadInfo() {
  phase.value = "loading";
  try {
    const st = await api.cloudSyncStatus();
    account.value = st.account;
    repoName.value = st.repoName;
    keepPerWorld.value = st.keepPerWorld;
    if (!st.connected) {
      phase.value = "connect";
      return;
    }
    await loadWorld();
  } catch (e) {
    message.error(String(e));
    phase.value = "connect";
  }
}

async function loadWorld() {
  if (browseMode.value) {
    await loadCloudWorlds();
    return;
  }
  const info = await api.cloudSyncWorldInfo(props.instanceId, props.world);
  worldId.value = info.worldId;
  autoSync.value = info.autoSync;
  snapshots.value = info.snapshots;
  if (!info.connected) {
    phase.value = "connect";
    return;
  }
  if (!repoName.value) {
    // 已授权但仓库还没建：静默初始化
    const r = await api.cloudSyncInitRepo();
    repoName.value = r.repo;
    // 仓库就绪后拉一次快照列表
    const info2 = await api.cloudSyncWorldInfo(props.instanceId, props.world);
    snapshots.value = info2.snapshots;
  }
  phase.value = "ready";
}

/** 浏览模式：拉取云端全部快照并按世界分组 */
async function loadCloudWorlds() {
  if (!repoName.value) {
    const r = await api.cloudSyncInitRepo();
    repoName.value = r.repo;
  }
  const all = await api.cloudSyncListAll();
  const groups = new Map<string, CloudWorldGroup>();
  for (const s of all.snapshots) {
    const wid = s.worldId || "unknown";
    let g = groups.get(wid);
    if (!g) {
      g = {
        worldId: wid,
        worldName: s.worldName || wid.slice(0, 8),
        instanceId: s.instanceId,
        instanceName: s.instanceName,
        snapshots: [],
      };
      groups.set(wid, g);
    }
    if (!g.worldName && s.worldName) g.worldName = s.worldName;
    if (!g.instanceId && s.instanceId) {
      g.instanceId = s.instanceId;
      g.instanceName = s.instanceName;
    }
    g.snapshots.push(s);
  }
  cloudWorlds.value = [...groups.values()].sort((a, b) => {
    const ta = a.snapshots[0]?.createdAt ?? "";
    const tb = b.snapshots[0]?.createdAt ?? "";
    return tb.localeCompare(ta);
  });
  phase.value = "ready";
}

function expandWorld(g: CloudWorldGroup) {
  expandedWorld.value = expandedWorld.value === g.worldId ? "" : g.worldId;
  targetName.value = g.worldName;
}

/** 浏览模式恢复：写入 targetName 指定的目录，并重建与云端世界的映射 */
async function restoreBrowse(s: CloudSnapshot, g: CloudWorldGroup) {
  const inst = targetInstanceId.value;
  if (!inst) {
    message.warning("请先选择要查看哪个实例的云存档");
    return;
  }
  const name = targetName.value.trim() || g.worldName;
  busy.value = "restore-" + s.releaseId;
  progress.value = { kind: "restore", step: "download", msg: "正在下载云端快照…" };
  try {
    const r = await api.cloudSyncRestore(inst, name, s.releaseId, g.worldId);
    message.success(
      (r.backupName ? `已恢复到「${name}」，原目录已备份为「${r.backupName}」` : `已恢复到「${name}」`) +
        "，世界列表已刷新",
    );
    emit("restored");
  } catch (e) {
    message.error(String(e));
  } finally {
    busy.value = "";
    progress.value = null;
  }
}

// ---- 设备流程授权 ----
// 点一下就全自动：申请设备码 → 自动复制验证码 → 自动打开授权页，用户只需 Ctrl+V + 点 Authorize
async function startAuth() {
  authError.value = "";
  try {
    const d = await api.cloudSyncStartAuth();
    userCode.value = d.userCode;
    verificationUri.value = d.verificationUri;
    deviceCode.value = d.deviceCode;
    phase.value = "authing";
    authCountdown.value = d.interval || 5;
    schedulePoll(d.interval || 5);
    // 自动复制验证码 + 自动打开授权页（失败不打断，界面上有手动入口）
    navigator.clipboard.writeText(d.userCode).then(
      () => (codeCopied.value = true),
      () => {},
    );
    openUrl(d.verificationUri).catch(() => {});
  } catch (e) {
    message.error(String(e));
  }
}

function schedulePoll(interval: number) {
  stopPoll();
  pollTimer = window.setTimeout(async () => {
    if (!deviceCode.value) return;
    try {
      const r = await api.cloudSyncPollAuth(deviceCode.value);
      if (r.status === "ok") {
        stopPoll();
        message.success("GitHub 授权成功");
        await loadInfo();
        return;
      }
    } catch (e) {
      // 用户码过期 / 被拒绝等：停止轮询并在界面提示
      authError.value = String(e);
      stopPoll();
      return;
    }
    schedulePoll(Math.max(interval, 3));
  }, interval * 1000);
}

function stopPoll() {
  if (pollTimer != null) {
    clearTimeout(pollTimer);
    pollTimer = null;
  }
}

function copyCode() {
  navigator.clipboard.writeText(userCode.value).then(
    () => {
      codeCopied.value = true;
      message.success("已复制，请到浏览器粘贴");
    },
    () => {},
  );
}

function openVerify() {
  openUrl(verificationUri.value).catch(() => message.error("打开浏览器失败，请手动访问 " + verificationUri.value));
}

async function disconnect() {
  try {
    await api.cloudSyncDisconnect();
    message.success("已断开连接（云端数据未删除）");
    phase.value = "connect";
  } catch (e) {
    message.error(String(e));
  }
}

// ---- 快照操作 ----
async function upload() {
  busy.value = "upload";
  progress.value = { kind: "upload", step: "pack", msg: "正在打包世界存档…" };
  try {
    const r = await api.cloudSyncUpload(
      props.instanceId,
      instances.instances.find((i) => i.id === props.instanceId)?.name ?? "",
      props.world,
      props.world,
      props.gameVersion,
    );
    message.success(
      `快照已上传（${fmtSize(r.sizeBytes)}）${r.cleaned ? `，已清理 ${r.cleaned} 个旧快照` : ""}`,
    );
    await loadWorld();
  } catch (e) {
    message.error(String(e));
  } finally {
    busy.value = "";
    progress.value = null;
  }
}

async function restore(s: CloudSnapshot) {
  busy.value = "restore-" + s.releaseId;
  progress.value = { kind: "restore", step: "download", msg: "正在下载云端快照…" };
  try {
    const r = await api.cloudSyncRestore(props.instanceId, props.world, s.releaseId);
    message.success(
      r.backupName
        ? `已恢复，原存档已保留为「${r.backupName}」`
        : "已恢复",
    );
    emit("restored");
  } catch (e) {
    message.error(String(e));
  } finally {
    busy.value = "";
    progress.value = null;
  }
}

async function remove(s: CloudSnapshot) {
  busy.value = "del-" + s.releaseId;
  try {
    await api.cloudSyncDelete(s.releaseId);
    await loadWorld();
  } catch (e) {
    message.error(String(e));
  } finally {
    busy.value = "";
  }
}

async function toggleAuto(v: boolean) {
  autoSync.value = v;
  try {
    await api.cloudSyncSetAuto(props.instanceId, props.world, v);
  } catch (e) {
    autoSync.value = !v;
    message.error(String(e));
  }
}

async function changeKeep(v: number) {
  keepPerWorld.value = v;
  try {
    await api.cloudSyncSetKeep(v);
  } catch (e) {
    message.error(String(e));
  }
}

function fmtTime(iso: string) {
  const secs = Number(iso);
  const d = Number.isFinite(secs) && iso.length <= 12 ? new Date(secs * 1000) : new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

watch(
  () => props.show,
  (v) => {
    if (v) loadInfo();
    else stopPoll();
  },
);

onMounted(async () => {
  unlistenProgress = await listen<{
    kind: string;
    step: string;
    msg?: string;
    sent?: number;
    total?: number;
  }>("cloud_sync://progress", (e) => {
    progress.value = e.payload;
    updateSpeed(e.payload);
  });
  // 浏览模式需要实例下拉可选项
  if (!instances.instances.length) {
    await instances.load();
  }
});
onBeforeUnmount(() => {
  stopPoll();
  unlistenProgress?.();
  unlistenProgress = null;
});
</script>

<template>
  <n-modal
    v-model:show="showValue"
    preset="card"
    :title="browseMode ? '云存档：从云端恢复' : `云存档：${world}`"
    style="width: 600px; max-width: 94vw"
    :mask-closable="phase !== 'authing'"
    :close-on-esc="phase !== 'authing'"
  >
    <!-- 加载中 -->
    <div v-if="phase === 'loading'" class="center">加载中…</div>

    <!-- 未连接：介绍 + 授权入口 -->
    <div v-else-if="phase === 'connect'" class="connect">
      <div class="hero">
        <IconCloud class="hero-icon" />
        <div>
          <div class="hero-title">把存档同步到你的 GitHub</div>
          <div class="hero-sub">
            存档保存在你自己的 GitHub <b>私有仓库</b> 里，启动器只是帮你打包上传，
            不会经过任何第三方服务器。可在多台电脑间恢复进度。
          </div>
        </div>
      </div>
      <ul class="perk">
        <li>自动打包世界文件夹并上传，每个世界保留最近几个快照</li>
        <li>恢复前会自动备份本地现有存档，不怕覆盖丢档</li>
        <li>只申请仓库读写权限，随时可以断开</li>
      </ul>
      <NButton type="primary" size="large" block @click="startAuth">
        <IconGithub />&nbsp;使用 GitHub 登录
      </NButton>
    </div>

    <!-- 授权中：展示用户码 -->
    <div v-else-if="phase === 'authing'" class="authing">
      <div class="auth-step">
        <span class="step-num">1</span> 已自动打开授权页并复制好验证码
        <button class="link-btn" @click="openVerify">没弹出？重新打开</button>
      </div>
      <div class="auth-step">
        <span class="step-num">2</span> 在页面中粘贴（<b>Ctrl+V</b>）<template v-if="codeCopied">验证码</template>，然后点击 <b>Authorize</b>
      </div>
      <button class="code-box" :title="codeCopied ? '已自动复制，也可点击重新复制' : '点击复制'" @click="copyCode">
        {{ userCode || "…" }}
        <span class="code-hint">{{ codeCopied ? "已复制 ✓" : "点击复制" }}</span>
      </button>
      <div v-if="authError" class="auth-err">
        {{ authError }}
        <button class="link-btn" @click="startAuth">重新开始</button>
      </div>
      <div v-else class="auth-wait">等待你在 GitHub 完成授权，成功后这里会自动进入下一步…</div>
    </div>

    <!-- 已连接：快照管理 -->
    <div v-else class="ready">
      <div class="head-row">
        <span class="acct">
          <IconGithub /> {{ account }} / {{ repoName }}
        </span>
        <button class="link-btn" @click="disconnect">断开连接</button>
      </div>

      <!-- 进度条：上传 / 恢复期间显示具体阶段与百分比 -->
      <div v-if="progress" class="progress-box">
        <div class="progress-head">
          <span>{{ progress.msg || "处理中…" }}</span>
          <span v-if="progressPercent != null" class="progress-pct">{{ progressPercent }}%</span>
          <span v-else-if="progressPercent === null && (progress.step === 'pack' || progress.step === 'hash')" class="progress-pct">…</span>
        </div>
        <div class="progress-track">
          <div
            v-if="progressPercent != null"
            class="progress-fill"
            :style="{ width: progressPercent + '%' }"
          ></div>
          <div v-else class="progress-fill indeterminate"></div>
        </div>
        <div v-if="progress.total && progress.sent != null" class="progress-size">
          {{ fmtSize(progress.sent) }} / {{ fmtSize(progress.total) }}
          <template v-if="speedBps > 0">
            · {{ fmtSize(speedBps) }}/s
            <template v-if="etaText">· {{ etaText }}</template>
          </template>
        </div>
      </div>

      <!-- 浏览模式：只显示所选实例自己的云端快照 -->
      <template v-if="browseMode">
        <div class="cw-target">
          <span>实例</span>
          <NSelect
            v-model:value="targetInstanceId"
            :options="instanceOptions"
            placeholder="选择实例"
            size="small"
            filterable
          />
        </div>
        <div v-if="!targetInstanceId" class="center">请先选择实例</div>
        <div v-else-if="!visibleWorlds.length" class="center">
          该实例还没有云端快照
        </div>
        <div v-else class="snap-list">
          <div v-for="g in visibleWorlds" :key="g.worldId" class="cw-group">
            <button class="cw-head" @click="expandWorld(g)">
              <IconCloud class="snap-icon" />
              <div class="c-info">
                <div class="c-name">{{ g.worldName }}</div>
                <div class="c-meta">
                  <span>{{ g.snapshots.length }} 个快照</span>
                  <span>最近：{{ fmtTime(g.snapshots[0]?.createdAt ?? "") }}</span>
                </div>
              </div>
              <span class="cw-chevron">{{ expandedWorld === g.worldId ? "收起" : "展开" }}</span>
            </button>
            <div v-if="expandedWorld === g.worldId" class="cw-detail">
              <div class="cw-target">
                <span>恢复到目录</span>
                <input v-model="targetName" class="cw-input" spellcheck="false" />
              </div>
              <div v-for="s in g.snapshots" :key="s.releaseId" class="snap-row">
                <div class="c-info">
                  <div class="c-name">{{ fmtTime(s.createdAt) }}</div>
                  <div class="c-meta">
                    <span>{{ fmtSize(s.assetSize) }}</span>
                    <span v-if="(s.partCount ?? 1) > 1">{{ s.partCount }} 卷</span>
                    <span v-if="s.gameVersion">{{ s.gameVersion }}</span>
                  </div>
                </div>
                <NButton
                  size="small"
                  type="warning"
                  :loading="busy === 'restore-' + s.releaseId"
                  @click="restoreBrowse(s, g)"
                >
                  <IconDownloadCloud />&nbsp;恢复
                </NButton>
              </div>
            </div>
          </div>
        </div>
        <div class="foot-hint">
          恢复会写入左侧目录名（默认用云端记录的世界名）；如果本地已有同名世界，恢复前会自动备份
        </div>
      </template>

      <!-- 世界模式：当前世界的快照管理 -->
      <template v-else>
        <div class="toolbar">
          <NButton type="primary" :loading="busy === 'upload'" @click="upload">
            <IconUploadCloud />&nbsp;上传当前存档
          </NButton>
          <label class="auto-toggle">
            <NSwitch size="small" :value="autoSync" @update:value="toggleAuto" />
            <span>退出游戏后自动上传</span>
          </label>
        </div>

        <div class="quota-row">
          <span>每个世界保留快照数</span>
          <div class="quota-ctl">
            <button
              class="q-btn"
              :disabled="keepPerWorld <= 1"
              @click="changeKeep(Math.max(1, keepPerWorld - 1))"
            >−</button>
            <span class="q-num">{{ keepPerWorld }}</span>
            <button
              class="q-btn"
              :disabled="keepPerWorld >= 50"
              @click="changeKeep(Math.min(50, keepPerWorld + 1))"
            >+</button>
          </div>
        </div>

        <div class="snap-title">云端快照（{{ snapshots.length }}）</div>
        <div v-if="!snapshots.length" class="center">还没有云端快照，点上方「上传当前存档」开始</div>
        <div v-else class="snap-list">
          <div v-for="s in snapshots" :key="s.releaseId" class="snap-row">
            <IconCloud class="snap-icon" />
            <div class="c-info">
              <div class="c-name">{{ fmtTime(s.createdAt) }}</div>
              <div class="c-meta">
                <span>{{ fmtSize(s.assetSize) }}</span>
                <span v-if="s.gameVersion">{{ s.gameVersion }}</span>
              </div>
            </div>
            <div class="c-actions">
              <NButton
                size="small"
                type="warning"
                :loading="busy === 'restore-' + s.releaseId"
                @click="restore(s)"
              >
                <IconDownloadCloud />&nbsp;恢复
              </NButton>
              <button
                class="snap-del"
                title="删除此快照"
                :disabled="busy === 'del-' + s.releaseId"
                @click="remove(s)"
              >
                <IconTrash />
              </button>
            </div>
          </div>
        </div>
        <div class="foot-hint">恢复时会把云端快照下载并覆盖本地「{{ world }}」，覆盖前自动备份</div>
      </template>
    </div>
  </n-modal>
</template>

<style scoped>
.center {
  padding: 40px;
  text-align: center;
  color: var(--text-3);
}
.connect {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.hero {
  display: flex;
  gap: 14px;
  align-items: flex-start;
}
.hero-icon {
  font-size: 34px;
  color: var(--accent);
  flex-shrink: 0;
  margin-top: 2px;
}
.hero-title {
  font-size: 15px;
  font-weight: 700;
  margin-bottom: 4px;
}
.hero-sub {
  font-size: 12px;
  color: var(--text-2);
  line-height: 1.6;
}
.perk {
  margin: 0;
  padding-left: 18px;
  font-size: 12px;
  color: var(--text-3);
  line-height: 2;
}
.authing {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.auth-step {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
  display: flex;
  align-items: center;
  gap: 8px;
}
.step-num {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 12px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}
.auth-err {
  font-size: 12px;
  color: #e5534b;
  text-align: center;
  line-height: 1.8;
}
.auth-err .link-btn {
  color: var(--accent);
  margin-left: 6px;
}
.code-box {
  position: relative;
  padding: 18px;
  border-radius: 12px;
  border: 1px dashed var(--accent-04);
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 26px;
  font-weight: 800;
  letter-spacing: 6px;
  text-align: center;
  cursor: pointer;
  font-family: inherit;
}
.code-hint {
  position: absolute;
  right: 10px;
  bottom: 6px;
  font-size: 10px;
  letter-spacing: 0;
  font-weight: 400;
  color: var(--text-3);
}
.auth-wait {
  text-align: center;
  font-size: 12px;
  color: var(--text-3);
}
.ready {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.head-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.acct {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 600;
}
.link-btn {
  background: none;
  border: none;
  color: var(--text-3);
  font-size: 12px;
  cursor: pointer;
  text-decoration: underline;
  font-family: inherit;
  padding: 0;
}
.link-btn:hover {
  color: #e5534b;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 16px;
}
.auto-toggle {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-2);
  cursor: pointer;
}
.quota-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 12px;
  color: var(--text-2);
}
.quota-ctl {
  display: inline-flex;
  align-items: center;
  gap: 10px;
}
.q-btn {
  width: 24px;
  height: 24px;
  border-radius: 6px;
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text-1);
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
}
.q-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
.q-num {
  min-width: 20px;
  text-align: center;
  font-weight: 700;
}
.snap-title {
  font-size: 12px;
  font-weight: 700;
  color: var(--text-3);
  margin-top: 2px;
}
.progress-box {
  padding: 10px 12px;
  border-radius: 10px;
  background: var(--w-04);
  border: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.progress-head {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  color: var(--text-1);
}
.progress-pct {
  font-weight: 700;
  color: var(--accent);
}
.progress-track {
  height: 6px;
  border-radius: 3px;
  background: var(--w-08);
  overflow: hidden;
}
.progress-fill {
  height: 100%;
  border-radius: 3px;
  background: var(--accent);
  transition: width 0.25s ease;
}
.progress-fill.indeterminate {
  width: 40%;
  animation: indet 1.1s ease-in-out infinite;
}
@keyframes indet {
  0% { margin-left: -40%; }
  100% { margin-left: 100%; }
}
.progress-size {
  font-size: 11px;
  color: var(--text-3);
}
.snap-list {
  display: flex;
  flex-direction: column;
  max-height: 300px;
  overflow-y: auto;
}
.snap-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 6px;
  border-bottom: 1px solid var(--border);
}
.snap-row:last-child {
  border-bottom: none;
}
.snap-icon {
  font-size: 18px;
  color: var(--text-3);
  flex-shrink: 0;
}
.c-info {
  flex: 1;
  min-width: 0;
}
.c-name {
  font-size: 13px;
  font-weight: 600;
}
.c-meta {
  display: flex;
  gap: 8px;
  font-size: 11px;
  color: var(--text-3);
}
.c-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.snap-del {
  width: 30px;
  height: 30px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-2);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  font-size: 13px;
}
.snap-del:hover {
  color: #e5534b;
  border-color: var(--danger-50);
}
.cw-warn {
  font-size: 11px;
  color: #e0a000;
  background: rgba(224, 160, 0, 0.08);
  border: 1px solid rgba(224, 160, 0, 0.25);
  border-radius: 8px;
  padding: 6px 10px;
  margin-bottom: 6px;
}
.foot-hint {
  font-size: 11px;
  color: var(--text-3);
}
.cw-group {
  border: 1px solid var(--border);
  border-radius: 10px;
  margin-bottom: 8px;
  overflow: hidden;
}
.cw-head {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  background: var(--w-04);
  border: none;
  cursor: pointer;
  font-family: inherit;
  text-align: left;
  color: inherit;
}
.cw-head:hover {
  background: var(--w-08);
}
.cw-chevron {
  margin-left: auto;
  font-size: 11px;
  color: var(--text-3);
  flex-shrink: 0;
}
.cw-detail {
  padding: 8px 12px 10px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.cw-target {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  color: var(--text-2);
  padding: 4px 0 8px;
}
.cw-input {
  flex: 1;
  padding: 6px 10px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text-1);
  font-size: 13px;
  font-family: inherit;
  outline: none;
}
.cw-input:focus {
  border-color: var(--accent-04);
}
</style>
