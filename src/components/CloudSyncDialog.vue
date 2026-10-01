<script setup lang="ts">
/**
 * 云存档同步弹窗（GitHub 私有仓库）。
 * 三种状态：未连接（设备流程授权）→ 初始化仓库 → 快照管理
 * （上传 / 恢复 / 删除 / 自动同步开关 / 每世界保留数）。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { listen } from "@tauri-apps/api/event";
import { NButton, NModal, NSelect, NSpin, NSwitch, useDialog, useMessage } from "naive-ui";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../api";
import { useInstancesStore } from "../stores/instances";
import { useHeightTransition } from "../composables/useHeightTransition";
import { useI18n } from "vue-i18n";
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
const dialog = useDialog();
const instances = useInstancesStore();
const { t } = useI18n();

type Phase = "loading" | "connect" | "authing" | "ready";
const phase = ref<Phase>("loading");
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
/** 展开的云端世界（可同时展开多个） */
const expandedWorlds = ref<string[]>([]);
const targetName = ref("");
/** 浏览模式的实例筛选；空 = 全部 */
const filterInstanceId = ref("");
const instanceOptions = computed(() => [
  { label: t("cloudSync.allInstances"), value: "" },
  ...instances.instances.map((i) => ({
    label: `${i.name}（${i.mc_version}）`,
    value: i.id,
  })),
]);
/** 筛选后的云端世界列表 */
const visibleWorlds = computed(() =>
  filterInstanceId.value
    ? cloudWorlds.value.filter((g) => g.instanceId === filterInstanceId.value)
    : cloudWorlds.value
);

/** 云存档记录对应的实例：本地还在就显示实例名，否则显示记录的实例名（红色标记） */
function instanceLabel(g: CloudWorldGroup): string {
  const local = instances.instances.find((i) => i.id === g.instanceId);
  return local?.name ?? g.instanceName ?? t("cloudSync.unknownInstance");
}
/** 记录里的实例本地是否还存在 */
function instanceExists(g: CloudWorldGroup): boolean {
  return !!g.instanceId && instances.instances.some((i) => i.id === g.instanceId);
}
/** 恢复目标实例：优先记录里的实例；老快照没有实例信息时用筛选里选中的实例 */
function restoreTarget(g: CloudWorldGroup): string {
  return instanceExists(g) ? (g.instanceId ?? "") : filterInstanceId.value;
}

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
  if (secs < 60) return t('cloudSync.etaSeconds', { secs: Math.ceil(secs) });
  const m = Math.floor(secs / 60);
  const s = Math.round(secs % 60);
  return s > 0 ? t('cloudSync.etaMinutesSeconds', { mins: m, secs: s }) : t('cloudSync.etaMinutes', { mins: m });
}
let unlistenProgress: (() => void) | null = null;
const authStep2Html = computed(() =>
  t('cloudSync.auth.step2', { code: codeCopied.value ? t('cloudSync.auth.code') : '' })
);
const showValue = computed({
  get: () => props.show,
  set: (v: boolean) => emit("update:show", v),
});

async function loadInfo() {
  phase.value = "loading";
  try {
    const st = await api.cloudSyncStatus();
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

function isExpanded(worldId: string): boolean {
  return expandedWorlds.value.includes(worldId);
}

// 展开 / 收起动画：与下载页同一套（高度过渡 + 内容变化跟随），gap 为 0（条目内没有 flex 间距）
const { onEnter: onExpandEnter, onLeave: onExpandLeave } = useHeightTransition({ gap: 0 });

/** 展开 / 收起一个云端世界（可同时展开多个）；展开后滚进视口，避免内容在折叠线以下 */

/** 展开 / 收起一个云端世界（可同时展开多个）；展开后滚进视口，避免内容在折叠线以下 */
function toggleWorld(g: CloudWorldGroup, ev: MouseEvent) {
  if (isExpanded(g.worldId)) {
    expandedWorlds.value = expandedWorlds.value.filter((x) => x !== g.worldId);
    return;
  }
  expandedWorlds.value = [...expandedWorlds.value, g.worldId];
  targetName.value = g.worldName;
  const group = (ev.currentTarget as HTMLElement | null)?.closest(".cw-group");
  // 等展开动画跑完再滚，否则按展开前的高度算位置、滚不到位
  window.setTimeout(() => group?.scrollIntoView({ block: "nearest", behavior: "smooth" }), 260);
}

/** 浏览模式恢复：写入 targetInstance 的 targetName 目录，并重建与云端世界的映射 */
async function restoreBrowse(s: CloudSnapshot, g: CloudWorldGroup) {
  const inst = restoreTarget(g);
  if (!inst) {
    message.warning(t('cloudSync.pickInstanceToRestore'));
    return;
  }
  const name = targetName.value.trim() || g.worldName;
  busy.value = "restore-" + s.releaseId;
  progress.value = { kind: "restore", step: "download", msg: t('cloudSync.downloadingSnapshot') };
  try {
    const r = await api.cloudSyncRestore(inst, name, s.releaseId, g.worldId);
    message.success(
      (r.backupName ? t('cloudSync.restoreOkWithBackup', { name, backup: r.backupName }) : t('cloudSync.restoreOk', { name })) +
        t('cloudSync.worldListRefreshed'),
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
        message.success(t('cloudSync.githubAuthOk'));
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
      message.success(t('cloudSync.copiedToPaste'));
    },
    () => {},
  );
}

function openVerify() {
  openUrl(verificationUri.value).catch(() => message.error(t('cloudSync.openBrowserFailed', { url: verificationUri.value })));
}

// ---- 快照操作 ----
async function upload() {
  busy.value = "upload";
  progress.value = { kind: "upload", step: "pack", msg: t('cloudSync.packingWorld') };
  try {
    const r = await api.cloudSyncUpload(
      props.instanceId,
      instances.instances.find((i) => i.id === props.instanceId)?.name ?? "",
      props.world,
      props.world,
      props.gameVersion,
    );
    message.success(
      r.cleaned ? t('cloudSync.uploadOkCleaned', { size: fmtSize(r.sizeBytes), count: r.cleaned }) : t('cloudSync.uploadOk', { size: fmtSize(r.sizeBytes) }),
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
  progress.value = { kind: "restore", step: "download", msg: t('cloudSync.downloadingSnapshot') };
  try {
    const r = await api.cloudSyncRestore(props.instanceId, props.world, s.releaseId);
    message.success(
      r.backupName
        ? t('cloudSync.restoreDoneWithBackup', { backup: r.backupName })
        : t('cloudSync.restoreDone'),
    );
    emit("restored");
  } catch (e) {
    message.error(String(e));
  } finally {
    busy.value = "";
    progress.value = null;
  }
}

/** 删除前二次确认：删掉的是 GitHub 上的云端文件，删了找不回来 */
function confirmRemove(s: CloudSnapshot) {
  dialog.warning({
    title: t('cloudSync.deleteSnapshot'),
    content: t('cloudSync.confirmDeleteSnapshot'),
    positiveText: t('common.delete'),
    negativeText: t('common.cancel'),
    onPositiveClick: () => remove(s),
  });
}

async function remove(s: CloudSnapshot) {
  busy.value = "del-" + s.releaseId;
  try {
    await api.cloudSyncDelete(s.releaseId);
    await loadWorld();
    message.success(t('cloudSync.snapshotDeleted'));
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
    :title="browseMode ? t('cloudSync.titleBrowse') : t('cloudSync.titleWorld', { world })"
    style="width: 600px; max-width: 94vw"
    :mask-closable="phase !== 'authing'"
    :close-on-esc="phase !== 'authing'"
  >
    <!-- 加载中 -->
    <div v-if="phase === 'loading'" class="center">{{ t('cloudSync.loading') }}</div>

    <!-- 未连接：介绍 + 授权入口 -->
    <div v-else-if="phase === 'connect'" class="connect">
      <div class="hero">
        <IconCloud class="hero-icon" />
        <div>
          <div class="hero-title">{{ t('cloudSync.hero.title') }}</div>
          <div class="hero-sub" v-html="t('cloudSync.hero.sub')"></div>
        </div>
      </div>
      <ul class="perk">
        <li>{{ t('cloudSync.perk.auto') }}</li>
        <li>{{ t('cloudSync.perk.backup') }}</li>
        <li>{{ t('cloudSync.perk.scope') }}</li>
      </ul>
      <NButton type="primary" size="large" block @click="startAuth">
        <IconGithub />&nbsp;{{ t('cloudSync.loginWithGithub') }}
      </NButton>
    </div>

    <!-- 授权中：展示用户码 -->
    <div v-else-if="phase === 'authing'" class="authing">
      <div class="auth-step">
        <span class="step-num">1</span> {{ t('cloudSync.auth.step1') }}
        <button class="link-btn" @click="openVerify">{{ t('cloudSync.auth.reopen') }}</button>
      </div>
      <div class="auth-step" v-html="authStep2Html"></div>
      <button class="code-box" :title="codeCopied ? t('cloudSync.auth.copiedTitle') : t('cloudSync.auth.copyTitle')" @click="copyCode">
        {{ userCode || "…" }}
        <span class="code-hint">{{ codeCopied ? t('cloudSync.auth.copied') : t('cloudSync.auth.copy') }}</span>
      </button>
      <div v-if="authError" class="auth-err">
        {{ authError }}
        <button class="link-btn" @click="startAuth">{{ t('cloudSync.auth.restart') }}</button>
      </div>
      <div v-else class="auth-wait">{{ t('cloudSync.auth.waiting') }}</div>
    </div>

    <!-- 已连接：快照管理 -->
    <div v-else class="ready">
      <!-- 进度条：上传 / 恢复期间显示具体阶段与百分比 -->
      <div v-if="progress" class="progress-box">
        <div class="progress-head">
          <span>{{ progress.msg || t('cloudSync.processing') }}</span>
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

      <!-- 浏览模式：打开即列出云端全部存档，并标注所属实例 -->
      <template v-if="browseMode">
        <div class="cw-target">
          <span>{{ t('cloudSync.instance') }}</span>
          <NSelect
            v-model:value="filterInstanceId"
            :options="instanceOptions"
            size="small"
            filterable
          />
        </div>
        <div v-if="!visibleWorlds.length" class="center">
          {{ t('cloudSync.noSnapshots') }}
        </div>
        <div v-else class="snap-list">
          <div v-for="g in visibleWorlds" :key="g.worldId" class="cw-group">
            <button class="cw-head" @click="toggleWorld(g, $event)">
              <IconCloud class="snap-icon" />
              <div class="c-info">
                <div class="c-name">{{ g.worldName }}</div>
                <div class="c-meta">
                  <span>{{ t('cloudSync.snapshotCount', { count: g.snapshots.length }) }}</span>
                  <span>{{ t('cloudSync.recent', { time: fmtTime(g.snapshots[0]?.createdAt ?? "") }) }}</span>
                </div>
              </div>
              <span class="cw-inst" :class="{ missing: !instanceExists(g) }">{{ instanceLabel(g) }}</span>
              <span class="cw-chevron">{{ isExpanded(g.worldId) ? t('cloudSync.collapse') : t('cloudSync.expand') }}</span>
            </button>
            <Transition :css="false" @enter="onExpandEnter" @leave="onExpandLeave">
              <div v-if="isExpanded(g.worldId)" class="cw-detail-wrap">
                <div class="cw-detail">
                  <div class="cw-target">
                    <span>{{ t('cloudSync.restoreTo') }}</span>
                    <input v-model="targetName" class="cw-input" spellcheck="false" />
                  </div>
                  <div v-for="s in g.snapshots" :key="s.releaseId" class="snap-row">
                    <div class="c-info">
                      <div class="c-name">{{ fmtTime(s.createdAt) }}</div>
                      <div class="c-meta">
                        <span>{{ fmtSize(s.assetSize) }}</span>
                        <span v-if="(s.partCount ?? 1) > 1">{{ t('cloudSync.parts', { count: s.partCount }) }}</span>
                        <span v-if="s.gameVersion">{{ s.gameVersion }}</span>
                      </div>
                    </div>
                    <NButton
                      size="small"
                      type="warning"
                      :disabled="!restoreTarget(g)"
                      :title="restoreTarget(g) ? '' : t('cloudSync.pickInstanceToRestore')"
                      :loading="busy === 'restore-' + s.releaseId"
                      @click="restoreBrowse(s, g)"
                    >
                      <IconDownloadCloud />&nbsp;{{ t('cloudSync.restore') }}
                    </NButton>
                    <button
                      class="snap-del"
                      :title="t('cloudSync.deleteSnapshot')"
                      :disabled="busy === 'del-' + s.releaseId"
                      @click="confirmRemove(s)"
                    >
                      <NSpin v-if="busy === 'del-' + s.releaseId" :size="14" />
                      <IconTrash v-else />
                    </button>
                  </div>
                </div>
              </div>
            </Transition>
          </div>
        </div>
      </template>

      <!-- 世界模式：当前世界的快照管理 -->
      <template v-else>
        <div class="toolbar">
          <NButton type="primary" :loading="busy === 'upload'" @click="upload">
            <IconUploadCloud />&nbsp;{{ t('cloudSync.uploadCurrent') }}
          </NButton>
          <label class="auto-toggle">
            <NSwitch size="small" :value="autoSync" @update:value="toggleAuto" />
            <span>{{ t('cloudSync.autoUpload') }}</span>
          </label>
        </div>

        <div class="quota-row">
          <span>{{ t('cloudSync.keepPerWorld') }}</span>
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

        <div class="snap-title">{{ t('cloudSync.cloudSnapshots', { count: snapshots.length }) }}</div>
        <div v-if="!snapshots.length" class="center">{{ t('cloudSync.emptyHint') }}</div>
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
                <IconDownloadCloud />&nbsp;{{ t('cloudSync.restore') }}
              </NButton>
              <button
                class="snap-del"
                :title="t('cloudSync.deleteSnapshot')"
                :disabled="busy === 'del-' + s.releaseId"
                @click="confirmRemove(s)"
              >
                <NSpin v-if="busy === 'del-' + s.releaseId" :size="14" />
                <IconTrash v-else />
              </button>
            </div>
          </div>
        </div>
        <div class="foot-hint">{{ t('cloudSync.restoreHint', { world }) }}</div>
      </template>
    </div>
  </n-modal>
</template>

<style scoped>
.center {
  padding: 26px 16px;
  text-align: center;
  color: var(--text-3);
  font-size: 12px;
  border: 1px dashed var(--border);
  border-radius: 10px;
}
/* 云存档条目上标注的所属实例 */
.cw-inst {
  flex-shrink: 0;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--w-06);
  color: var(--text-2);
  white-space: nowrap;
}
.cw-inst.missing {
  color: #e5534b;
  background: rgba(229, 83, 75, 0.14);
}
/* 行内标签不要被挤成竖排 */
.cw-target > span {
  white-space: nowrap;
  flex-shrink: 0;
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
  /* 给足高度：展开的条目要能完整看到（超出时整列滚动，不再从中间裁掉） */
  max-height: 56vh;
  overflow-y: auto;
  padding-right: 4px;
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
/* 全局 button:active 有 scale(0.96)，整行标题一按就缩、两侧漏出卡片底色 */
.cw-head:active {
  transform: none;
}
/* 展开动画的外层（高度由 useHeightTransition 驱动） */
.cw-detail-wrap {
  overflow: hidden;
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
