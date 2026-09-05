<script setup lang="ts">
/**
 * 实例详情 · 内容管理 tab（mods / resourcepacks / shaders 共用）。
 * 从 InstanceDetailView 拆出，自行负责：内容列表加载/识别、更新检查与应用、
 * 切换版本、启用/禁用、移除、导入本地、"在内容中心搜索"跳转。
 * 父组件通过 ref 调用 checkUpdates / importLocal / reload，
 * 并可读取 updatesCount / checkingUpdates 驱动 tab 栏按钮。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { NButton, NModal, useMessage } from "naive-ui";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api } from "../../api";
import { useInstancesStore } from "../../stores/instances";
import {
  IconCheck,
  IconClose,
  IconDownload,
  IconFile,
  IconImage,
  IconPlus,
  IconRepeat,
  IconSearch,
  IconTrash,
} from "../icons";
import type { ContentItem, ProjectVersion, UpdateInfo } from "../../types";

const props = defineProps<{
  instanceId: string;
  /** 后端内容类型：mod / shader / resourcepack */
  kind: string;
}>();

const router = useRouter();
const message = useMessage();
const instances = useInstancesStore();

function sourceLabel(s: string) {
  return s === "modrinth" ? "Modrinth" : s === "curseforge" ? "CurseForge" : s === "modpack" ? "整合包" : "手动";
}

function iconUrl(icon: string | null): string | null {
  if (!icon) return null;
  if (icon.startsWith("http://") || icon.startsWith("https://") || icon.startsWith("data:")) return icon;
  return convertFileSrc(icon);
}

const contentItems = ref<ContentItem[]>([]);
const iconErrors = ref(new Set<string>());
const updates = ref<Record<string, UpdateInfo>>({});
const checkingUpdates = ref(false);
const loadingContent = ref(false);

let loadSeq = 0;
async function loadContent() {
  const seq = ++loadSeq;
  loadingContent.value = true;
  try {
    const res = await api.listContent(props.instanceId, props.kind);
    if (seq !== loadSeq) return;
    contentItems.value = res.items;
    api.identifyContent(props.instanceId, props.kind).catch(() => {});
  } catch (e) {
    if (seq !== loadSeq) return;
    message.error(String(e));
  } finally {
    if (seq === loadSeq) loadingContent.value = false;
  }
}

async function checkUpdates() {
  checkingUpdates.value = true;
  try {
    const list = await api.checkUpdates(props.instanceId, props.kind);
    const map: Record<string, UpdateInfo> = {};
    for (const u of list) map[u.filename] = u;
    updates.value = map;
    if (!list.length) message.success("所有内容都是最新版本");
    else message.info(`发现 ${list.length} 个可更新内容`);
  } catch (e) {
    message.error(String(e));
  } finally {
    checkingUpdates.value = false;
  }
}

async function applyUpdate(u: UpdateInfo) {
  try {
    await api.applyUpdate(props.instanceId, props.kind, u.filename, u.provider, u.projectId, u.latestVersionId);
    message.success("已加入下载队列：" + (u.projectTitle ?? u.filename));
    const next = { ...updates.value };
    delete next[u.filename];
    updates.value = next;
  } catch (e) {
    message.error(String(e));
  }
}

// ---- 确认弹窗（本组件内的移除操作使用） ----
const confirmState = ref<{ title: string; content: string; positiveText: string; onOk: () => void | Promise<void> } | null>(null);
const confirmLoading = ref(false);
async function handleConfirm() {
  if (!confirmState.value) return;
  confirmLoading.value = true;
  try {
    await confirmState.value.onOk();
    confirmState.value = null;
  } finally {
    confirmLoading.value = false;
  }
}

function removeContent(item: ContentItem) {
  confirmState.value = {
    title: "移除内容",
    content: `确定要移除「${item.record.filename}」吗？`,
    positiveText: "移除",
    onOk: async () => {
      try {
        await api.uninstallContent(props.instanceId, props.kind, item.record.filename);
        message.success("已移除");
        await loadContent();
      } catch (e) {
        message.error(String(e));
      }
    },
  };
}

async function importLocal() {
  const kind = props.kind;
  const filter = kind === "mod" ? [{ name: "JAR 文件", extensions: ["jar"] }] : [{ name: "ZIP 文件", extensions: ["zip"] }];
  const file = await open({ multiple: false, filters: filter });
  if (!file) return;
  try {
    await api.importLocalFile(props.instanceId, kind, file as string);
    message.success("已导入");
    await loadContent();
  } catch (e) {
    message.error(String(e));
  }
}

async function toggleContent(item: ContentItem) {
  try {
    await api.toggleContentEnabled(props.instanceId, props.kind, item.record.filename, !item.record.enabled);
    item.record.enabled = !item.record.enabled;
    message.success(item.record.enabled ? "已启用" : "已禁用");
  } catch (e) {
    message.error(String(e));
  }
}

// ---- 切换版本 ----
const switchState = ref<{
  show: boolean;
  loading: boolean;
  item: ContentItem | null;
  versions: ProjectVersion[];
  selected: string | null;
  provider: string;
  projectId: string;
}>({
  show: false,
  loading: false,
  item: null,
  versions: [],
  selected: null,
  provider: "",
  projectId: "",
});

function fmtIsoDate(s: string) {
  return s ? s.slice(0, 10) : "";
}

function modSearchTerm(item: ContentItem): string {
  const rec = item.record;
  // 优先用 slug 检索：内容中心按 slug 精确匹配，命中率最高
  if (rec.slug) return rec.slug as string;
  // 其次用项目标题（远程模组）
  if (rec.name && rec.name !== rec.filename) return rec.name;
  // 本地文件回退到文件名
  const base = rec.filename.replace(/\.(jar|zip|litemod|disabled)$/i, "");
  const loaders = ["fabric", "forge", "neoforge", "quilt", "rift", "optifine", "vanilla"];
  const parts = base
    .split(/[-_]/)
    .filter((p) => p && !loaders.includes(p.toLowerCase()) && !/\d/.test(p));
  return parts.length ? parts.join("-") : base;
}

/** 构建搜索跳转参数：带上来源 provider，让内容中心直接定位到对应平台 */
function buildModSearchQuery(item: ContentItem) {
  const q = modSearchTerm(item);
  const source = item.record.source;
  const provider = source === "modrinth" || source === "curseforge" ? source : null;
  return provider ? { q, provider } : { q };
}

async function openSwitchVersion(item: ContentItem) {
  const src = item.record.source;
  if (src !== "modrinth" && src !== "curseforge") {
    message.info("手动导入的内容无法切换版本");
    return;
  }
  const pid = item.record.project_id;
  if (!pid) {
    message.info("缺少项目信息，无法切换版本");
    return;
  }
  const inst = instances.get(props.instanceId);
  const mc = inst?.mc_version ?? "";
  const ld = inst && inst.loader !== "vanilla" ? inst.loader : "";
  switchState.value = {
    show: true,
    loading: true,
    item,
    versions: [],
    selected: null,
    provider: src,
    projectId: pid,
  };
  try {
    const res = await api.projectVersions(src, pid, mc, ld);
    switchState.value.versions = res.versions;
    const cur = res.versions.find((v) => v.id === item.record.version_id);
    switchState.value.selected = cur?.id ?? res.versions[0]?.id ?? null;
  } catch (e) {
    message.error(String(e));
  } finally {
    switchState.value.loading = false;
  }
}

async function doSwitchVersion() {
  const s = switchState.value;
  if (!s.item || !s.selected) {
    message.warning("请选择一个版本");
    return;
  }
  if (s.selected === s.item.record.version_id) {
    message.info("已选择当前安装的版本");
    switchState.value.show = false;
    return;
  }
  try {
    await api.applyUpdate(props.instanceId, props.kind, s.item.record.filename, s.provider, s.projectId, s.selected);
    message.success("已将切换版本任务添加到下载队列");
    switchState.value.show = false;
    await loadContent();
  } catch (e) {
    message.error(String(e));
  }
}

// 点击遮罩关闭弹窗（document 委托兜底，naive-ui mask 机制在此环境不可靠）
const switchCardRef = ref<HTMLElement | null>(null);
const confirmCardRef = ref<HTMLElement | null>(null);
function onDocMouseDown(e: MouseEvent) {
  const t = e.target as Element | null;
  if (!t) return;
  if (t.closest(".v-binder-follower-container, .n-base-select-menu, .n-popover, .n-dropdown")) return;
  if (switchState.value.show && switchCardRef.value && !switchCardRef.value.contains(t)) {
    switchState.value.show = false;
    return;
  }
  if (confirmState.value && confirmCardRef.value && !confirmCardRef.value.contains(t)) {
    confirmState.value = null;
  }
}

let unlistenUpdate: UnlistenFn | null = null;
let unlistenIdentify: UnlistenFn | null = null;
onMounted(async () => {
  document.addEventListener("mousedown", onDocMouseDown);
  await loadContent();
  try {
    unlistenUpdate = await listen<{ filename: string; ok: boolean; error?: string }>(
      "content://update-finished",
      (ev) => {
        const p = ev.payload;
        if (p.ok) {
          message.success((updates.value[p.filename]?.projectTitle ?? p.filename) + " 已更新");
        } else {
          message.error("更新失败 " + p.filename + (p.error ? "：" + p.error : ""));
        }
        loadContent();
      }
    );
  } catch {
    /* 事件监听不可用不影响主流程 */
  }
  try {
    unlistenIdentify = await listen<{
      instanceId: string; kind: string; filename: string;
      source: string; projectId: string; versionId: string;
      slug: string | null; name: string | null; description: string | null;
      icon: string | null; authors: string[] | null;
    }>("content::identified", (ev) => {
      const p = ev.payload;
      if (p.instanceId !== props.instanceId || p.kind !== props.kind) return;
      const idx = contentItems.value.findIndex((it) => it.record.filename === p.filename);
      if (idx < 0) return;
      const rec = contentItems.value[idx].record;
      rec.source = p.source;
      rec.project_id = p.projectId;
      rec.version_id = p.versionId;
      rec.slug = p.slug;
      if (p.name) rec.name = p.name;
      if (p.description) rec.description = p.description;
      if (p.icon) rec.icon = p.icon;
      if (p.authors) rec.authors = p.authors;
    });
  } catch {
    /* ignore */
  }
});
onBeforeUnmount(() => {
  document.removeEventListener("mousedown", onDocMouseDown);
  unlistenUpdate?.();
  unlistenUpdate = null;
  unlistenIdentify?.();
  unlistenIdentify = null;
});

// 实例安装完成后 / 实例对象刷新后重新加载内容列表
watch(
  () => instances.get(props.instanceId)?.installed,
  () => loadContent()
);
watch(
  () => instances.get(props.instanceId),
  () => loadContent()
);
watch(() => props.kind, () => loadContent());

const updatesCount = computed(() => Object.keys(updates.value).length);

defineExpose({
  reload: loadContent,
  checkUpdates,
  importLocal,
  updatesCount,
  checkingUpdates,
});
</script>

<template>
  <div>
    <div v-if="!loadingContent && !contentItems.length" class="empty glass">
      <p>这里还是空的</p>
      <div class="empty-actions">
        <button class="btn ghost" @click="importLocal"><IconPlus /> 导入本地文件</button>
        <button class="btn ghost" @click="router.push('/browse')">从内容中心安装</button>
      </div>
    </div>
    <div v-else class="content-list glass">
      <div v-for="item in contentItems" :key="item.record.filename" class="c-row">
        <div class="c-icon">
          <img
            v-if="item.record.icon && !iconErrors.has(item.record.filename)"
            :src="iconUrl(item.record.icon) ?? ''"
            class="c-thumb"
            alt=""
            loading="lazy"
            @error="iconErrors.add(item.record.filename)"
          />
          <IconFile v-else-if="item.record.filename.endsWith('.jar')" />
          <IconImage v-else />
        </div>
        <div class="c-info">
          <div class="c-name text-ellipsis">
            {{ item.record.cn_name ?? item.record.name ?? item.record.filename }}
            <span v-if="item.record.cn_name && item.record.name" class="c-en">{{ item.record.name }}</span>
          </div>
          <div v-if="(item.record.name && item.record.name !== item.record.filename) || item.record.cn_name" class="c-file text-ellipsis">{{ item.record.filename }}</div>
          <div class="c-meta">
            <span v-if="item.record.source !== 'manual'" class="src" :class="item.record.source">{{ sourceLabel(item.record.source) }}</span>
            <span v-if="item.record.version" class="ver">{{ item.record.version }}</span>
            <span v-if="item.record.authors && item.record.authors.length" class="author">作者：{{ item.record.authors.join("、") }}</span>
            <span v-if="!item.exists" class="missing">文件缺失</span>
          </div>
        </div>
        <div class="c-actions">
          <button
            v-if="updates[item.record.filename]"
            class="icon-btn ok"
            :title="`更新到 ${updates[item.record.filename].latestVersion}`"
            @click="applyUpdate(updates[item.record.filename])"
          >
            <IconDownload />
          </button>
          <button
            v-if="(item.record.source === 'modrinth' || item.record.source === 'curseforge') && item.record.project_id"
            class="icon-btn"
            title="切换版本"
            @click="openSwitchVersion(item)"
          >
            <IconRepeat />
          </button>
          <button
            class="icon-btn"
            title="在内容中心搜索"
            @click="router.push({ name: 'browse', query: buildModSearchQuery(item) })"
          >
            <IconSearch />
          </button>
          <button
            v-if="item.record.enabled"
            class="icon-btn warn"
            title="禁用"
            @click="toggleContent(item)"
          >
            <IconClose />
          </button>
          <button
            v-else
            class="icon-btn ok"
            title="启用"
            @click="toggleContent(item)"
          >
            <IconCheck />
          </button>
          <button class="icon-btn danger" title="移除" @click="removeContent(item)">
            <IconTrash />
          </button>
        </div>
      </div>
    </div>

    <!-- confirm dialog -->
    <n-modal
      :show="confirmState !== null"
      preset="card"
      :title="confirmState?.title ?? ''"
      style="width: 420px; max-width: 92vw"
      :mask-closable="true"
      :close-on-esc="true"
      @update:show="(v: boolean) => { if (!v) confirmState = null; }"
      @mask-click="confirmState = null"
    >
      <div v-if="confirmState" ref="confirmCardRef" style="display: flex; flex-direction: column; gap: 16px;">
        <div style="font-size: 14px; color: var(--text-2); line-height: 1.6;">{{ confirmState.content }}</div>
        <div style="display: flex; justify-content: flex-end; gap: 10px;">
          <n-button @click="confirmState = null">取消</n-button>
          <n-button type="error" :loading="confirmLoading" @click="handleConfirm">{{ confirmState.positiveText }}</n-button>
        </div>
      </div>
    </n-modal>

    <!-- switch version -->
    <n-modal
      v-model:show="switchState.show"
      preset="card"
      :title="`切换版本：${switchState.item?.record.name ?? switchState.item?.record.filename ?? ''}`"
      style="width: 520px; max-width: 94vw"
      :mask-closable="true"
      :close-on-esc="true"
      @mask-click="switchState.show = false"
    >
      <div ref="switchCardRef" class="sv-body">
        <div v-if="switchState.loading" class="center">加载中…</div>
        <div v-else-if="!switchState.versions.length" class="center">没有可用的版本</div>
        <div v-else class="sv-list">
          <button
            v-for="v in switchState.versions"
            :key="v.id"
            class="sv-item"
            :class="{ active: switchState.selected === v.id }"
            @click="switchState.selected = v.id"
          >
            <span class="sv-num">{{ v.version_number ?? v.name }}</span>
            <span v-if="v.date_published" class="sv-date">{{ fmtIsoDate(v.date_published) }}</span>
          </button>
        </div>
        <div class="sv-actions">
          <n-button size="small" @click="switchState.show = false">取消</n-button>
          <n-button
            size="small"
            type="primary"
            :disabled="!switchState.selected || switchState.loading"
            @click="doSwitchVersion"
          >
            切换
          </n-button>
        </div>
      </div>
    </n-modal>
  </div>
</template>

<style scoped>
.btn {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: none;
  border-radius: 10px;
  padding: 9px 16px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.14s;
}
.btn.ghost {
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-1);
  border: 1px solid var(--border);
}
.btn.ghost:hover {
  background: rgba(255, 255, 255, 0.1);
}
.content-list {
  padding: 18px;
}
.c-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 8px;
  border-bottom: 1px solid var(--border);
}
.c-row:last-child {
  border-bottom: none;
}
.c-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.c-icon {
  width: 34px;
  height: 34px;
  border-radius: 9px;
  background: rgba(255, 255, 255, 0.05);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  font-size: 16px;
  flex-shrink: 0;
  overflow: hidden;
}
.c-thumb {
  width: 100%;
  height: 100%;
  object-fit: cover;
  image-rendering: pixelated;
}
.c-info {
  flex: 1;
  min-width: 0;
}
.c-name {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 3px;
}
.c-en {
  font-size: 11px;
  font-weight: 400;
  color: var(--text-3);
  margin-left: 6px;
}
.c-file {
  font-size: 11px;
  color: var(--text-3);
  margin-bottom: 3px;
}
.c-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
}
.src {
  padding: 1px 7px;
  border-radius: 6px;
  font-weight: 600;
}
.src.modrinth {
  background: rgba(90, 162, 240, 0.15);
  color: #7cb8f5;
}
.src.curseforge {
  background: rgba(240, 101, 67, 0.15);
  color: #f08a67;
}
.src.manual {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-3);
}
.src.modpack {
  background: rgba(150, 181, 225, 0.18);
  color: #96b5e1;
}
.ver {
  color: var(--text-3);
}
.author {
  color: var(--text-3);
  opacity: 0.85;
}
.missing {
  color: #e5534b;
  font-weight: 600;
}
.icon-btn {
  width: 30px;
  height: 30px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-2);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  font-size: 13px;
  transition: all 0.12s;
}
.icon-btn.danger:hover {
  color: #e5534b;
  border-color: rgba(229, 83, 75, 0.5);
}
.icon-btn.warn {
  color: #e5534b;
  border-color: rgba(229, 83, 75, 0.35);
}
.icon-btn.warn:hover {
  background: rgba(229, 83, 75, 0.15);
  border-color: #e5534b;
}
.icon-btn.ok {
  color: #4ec9a0;
  border-color: rgba(78, 201, 160, 0.35);
}
.icon-btn.ok:hover {
  background: rgba(78, 201, 160, 0.15);
  border-color: #4ec9a0;
}
.center {
  padding: 60px;
  text-align: center;
  color: var(--text-3);
}
.empty {
  padding: 40px;
  text-align: center;
  color: var(--text-3);
  display: flex;
  flex-direction: column;
  gap: 14px;
  align-items: center;
}
.empty-actions {
  display: flex;
  gap: 10px;
}
.sv-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.sv-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 320px;
  overflow-y: auto;
}
.sv-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  border-radius: 9px;
  padding: 8px 14px;
  font-size: 13px;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.sv-item:hover {
  background: rgba(255, 255, 255, 0.08);
}
.sv-item.active {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-soft);
}
.sv-num {
  font-weight: 600;
}
.sv-date {
  font-size: 11px;
  color: var(--text-3);
}
.sv-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
</style>
