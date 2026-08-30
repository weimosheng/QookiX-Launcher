<script setup lang="ts">
import { computed, markRaw, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useMessage } from "naive-ui";
import { api } from "../api";
import type { ContextMenuItem, FsEntry } from "../types";
import CodeEditor from "./CodeEditor.vue";
import ContextMenu from "./ContextMenu.vue";
import {
  IconChevronLeft,
  IconChevronRight,
  IconClose,
  IconCopy,
  IconCornerUpLeft,
  IconEdit,
  IconExternal,
  IconFile,
  IconFolder,
  IconRefresh,
  IconSave,
  IconSearch,
} from "./icons";

const props = defineProps<{ serverId: string }>();
const message = useMessage();

/** 内置编辑器支持打开的最大文件体积，与后端 MAX_EDIT_BYTES 保持一致 */
const MAX_EDIT = 4 * 1024 * 1024;

const TEXT_EXT = new Set([
  "txt", "json", "json5", "properties", "cfg", "conf", "config", "toml", "yaml", "yml",
  "ini", "xml", "lang", "mcmeta", "snbt", "js", "mjs", "cjs", "ts", "lua", "py",
  "md", "log", "csv", "html", "css", "sh", "bat", "cmd", "gitignore",
]);

function isEditable(e: FsEntry): boolean {
  if (e.is_dir || e.size > MAX_EDIT) return false;
  return TEXT_EXT.has(e.ext) || (e.ext === "" && e.size <= 256 * 1024);
}

function fmtSize(n: number): string {
  if (n >= 1024 * 1024 * 1024) return (n / 1024 / 1024 / 1024).toFixed(2) + " GB";
  if (n >= 1024 * 1024) return (n / 1024 / 1024).toFixed(1) + " MB";
  if (n >= 1024) return (n / 1024).toFixed(1) + " KB";
  return n + " B";
}

function fmtDate(sec: number): string {
  if (!sec) return "";
  const d = new Date(sec * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function badge(e: FsEntry): string {
  if (e.is_dir) return "DIR";
  return e.ext ? e.ext.slice(0, 4).toUpperCase() : "···";
}

// ---------------------------------------------------------------- 目录浏览

const cwd = ref("");
const entries = ref<FsEntry[]>([]);
const loading = ref(false);
const filter = ref("");
const backStack = ref<string[]>([]);
const fwdStack = ref<string[]>([]);

const crumbs = computed(() => {
  if (!cwd.value) return [];
  const parts = cwd.value.split("/");
  return parts.map((p, i) => ({ name: p, rel: parts.slice(0, i + 1).join("/") }));
});

const shown = computed(() => {
  const q = filter.value.trim().toLowerCase();
  if (!q) return entries.value;
  return entries.value.filter((e) => e.name.toLowerCase().includes(q));
});

async function reload() {
  loading.value = true;
  try {
    const r = await api.listHostedServerDir(props.serverId, cwd.value);
    if (r.rel !== cwd.value) return;
    entries.value = r.entries;
  } catch (e) {
    entries.value = [];
    message.error(String(e));
  } finally {
    loading.value = false;
  }
}

function navigate(rel: string) {
  if (rel === cwd.value) return;
  backStack.value.push(cwd.value);
  fwdStack.value = [];
  cwd.value = rel;
  filter.value = "";
  selected.value = null;
  reload();
}

function goBack() {
  const prev = backStack.value.pop();
  if (prev === undefined) return;
  fwdStack.value.push(cwd.value);
  cwd.value = prev;
  filter.value = "";
  reload();
}

function goForward() {
  const next = fwdStack.value.pop();
  if (next === undefined) return;
  backStack.value.push(cwd.value);
  cwd.value = next;
  filter.value = "";
  reload();
}

function goUp() {
  const i = cwd.value.lastIndexOf("/");
  navigate(i < 0 ? "" : cwd.value.slice(0, i));
}

async function reveal(rel: string) {
  try {
    await api.revealHostedServerPath(props.serverId, rel);
  } catch (e) {
    message.error(String(e));
  }
}

// ---------------------------------------------------------------- 编辑器标签页

interface OpenTab {
  rel: string;
  name: string;
  content: string;
  original: string;
  size: number;
  modified: number;
}

const openTabs = ref<OpenTab[]>([]);
const activeRel = ref<string | null>(null);
const saving = ref(false);

const active = computed(() => openTabs.value.find((t) => t.rel === activeRel.value) ?? null);
const isDirty = (t: OpenTab) => t.content !== t.original;

function onEdit(rel: string, value: string) {
  const t = openTabs.value.find((x) => x.rel === rel);
  if (t) t.content = value;
}

async function openEntry(e: FsEntry) {
  if (e.is_dir) {
    navigate(e.rel);
    return;
  }
  if (!isEditable(e)) {
    message.info(`「${e.name}」不是可编辑的文本文件，已为你打开所在文件夹`);
    reveal(e.rel);
    return;
  }
  if (openTabs.value.some((t) => t.rel === e.rel)) {
    activeRel.value = e.rel;
    return;
  }
  try {
    const r = await api.readHostedServerFile(props.serverId, e.rel);
    openTabs.value.push({
      rel: e.rel,
      name: e.name,
      content: r.content,
      original: r.content,
      size: r.size,
      modified: r.modified,
    });
    activeRel.value = e.rel;
  } catch (err) {
    message.error(String(err));
  }
}

async function saveTab(tab?: OpenTab | null) {
  const t = tab ?? active.value;
  if (!t) return;
  saving.value = true;
  try {
    const r = await api.writeHostedServerFile(props.serverId, t.rel, t.content);
    t.original = t.content;
    t.size = r.size;
    t.modified = r.modified;
    message.success("已保存 " + t.name);
    await reload();
  } catch (e) {
    message.error(String(e));
  } finally {
    saving.value = false;
  }
}

function closeTab(t: OpenTab) {
  const i = openTabs.value.findIndex((x) => x.rel === t.rel);
  if (i < 0) return;
  openTabs.value.splice(i, 1);
  if (activeRel.value === t.rel) {
    const next = openTabs.value[Math.min(i, openTabs.value.length - 1)];
    activeRel.value = next ? next.rel : null;
  }
}

function requestClose(t: OpenTab) {
  if (!isDirty(t)) {
    closeTab(t);
    return;
  }
  confirmState.value = {
    title: "未保存的修改",
    content: `「${t.name}」有尚未保存的修改，关闭后会丢失。确定关闭吗？`,
    positiveText: "放弃修改",
    onOk: () => closeTab(t),
  };
}

// ---------------------------------------------------------------- 确认弹窗

const confirmState = ref<{
  title: string;
  content: string;
  positiveText: string;
  onOk: () => void | Promise<void>;
} | null>(null);
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

// ---------------------------------------------------------------- 右键菜单

const menu = ref<{ show: boolean; x: number; y: number; items: ContextMenuItem[] }>({
  show: false,
  x: 0,
  y: 0,
  items: [],
});

function openMenu(x: number, y: number, items: ContextMenuItem[]) {
  menu.value = { show: true, x, y, items };
}
function closeMenu() {
  if (menu.value.show) menu.value = { ...menu.value, show: false };
}

const selected = ref<string | null>(null);

async function copyText(text: string, tip: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success(tip);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    if (ok) message.success(tip);
    else message.error("复制失败");
  }
}

function entryMenu(e: FsEntry): ContextMenuItem[] {
  const items: ContextMenuItem[] = [];
  if (e.is_dir) {
    items.push({
      key: "open",
      label: "打开",
      icon: markRaw(IconFolder),
      action: () => navigate(e.rel),
    });
  } else if (isEditable(e)) {
    items.push({
      key: "open",
      label: "在编辑器中打开",
      icon: markRaw(IconEdit),
      action: () => openEntry(e),
    });
  }
  items.push(
    {
      key: "reveal",
      label: e.is_dir ? "打开所在位置" : "在文件管理器中显示",
      icon: markRaw(IconExternal),
      action: () => reveal(e.rel),
    },
    { key: "s1", sep: true },
    {
      key: "copy-rel",
      label: "复制相对路径",
      icon: markRaw(IconCopy),
      action: () => copyText(e.rel, "已复制路径"),
    },
    {
      key: "copy-name",
      label: "复制文件名",
      icon: markRaw(IconCopy),
      action: () => copyText(e.name, "已复制文件名"),
    },
  );
  return items;
}

function onRowContext(ev: MouseEvent, e: FsEntry) {
  selected.value = e.rel;
  openMenu(ev.clientX, ev.clientY, entryMenu(e));
}

function onBlankContext(ev: MouseEvent) {
  selected.value = null;
  openMenu(ev.clientX, ev.clientY, [
    {
      key: "refresh",
      label: "刷新",
      icon: markRaw(IconRefresh),
      shortcut: "F5",
      action: () => reload(),
    },
    {
      key: "reveal",
      label: "在系统文件管理器中打开",
      icon: markRaw(IconExternal),
      action: () => reveal(cwd.value),
    },
  ]);
}

function onTabContext(ev: MouseEvent, t: OpenTab) {
  activeRel.value = t.rel;
  const others = openTabs.value.filter((x) => x.rel !== t.rel);
  openMenu(ev.clientX, ev.clientY, [
    {
      key: "save",
      label: "保存",
      icon: markRaw(IconSave),
      shortcut: "Ctrl+S",
      disabled: !isDirty(t),
      action: () => saveTab(t),
    },
    {
      key: "copy-rel",
      label: "复制相对路径",
      icon: markRaw(IconCopy),
      action: () => copyText(t.rel, "已复制路径"),
    },
    { key: "s1", sep: true },
    {
      key: "close",
      label: "关闭",
      icon: markRaw(IconClose),
      action: () => requestClose(t),
    },
    {
      key: "close-others",
      label: "关闭其他标签",
      disabled: !others.length,
      action: async () => {
        for (const o of others) {
          if (!isDirty(o)) closeTab(o);
        }
        const rest = openTabs.value.filter((x) => x.rel !== t.rel);
        if (rest.length) {
          message.info(`还有 ${rest.length} 个标签存在未保存的修改`);
        }
      },
    },
    {
      key: "close-all",
      label: "关闭全部标签",
      disabled: !openTabs.value.length,
      action: () => {
        const dirty = openTabs.value.filter(isDirty).length;
        if (!dirty) {
          openTabs.value = [];
          activeRel.value = null;
          return;
        }
        confirmState.value = {
          title: "未保存的修改",
          content: `有 ${dirty} 个文件存在尚未保存的修改，全部关闭会丢失这些改动。确定关闭吗？`,
          positiveText: "全部关闭",
          onOk: () => {
            openTabs.value = [];
            activeRel.value = null;
          },
        };
      },
    },
  ]);
}

// ---- 编辑器内右键 ----

const editorRef = ref<InstanceType<typeof CodeEditor> | null>(null);

function onEditorContext(p: { x: number; y: number }) {
  const t = active.value;
  const hasSel = editorRef.value?.hasSelection() ?? false;
  const items: ContextMenuItem[] = [
    {
      key: "cut",
      label: "剪切",
      shortcut: "Ctrl+X",
      disabled: !hasSel,
      action: () => editorRef.value?.cutSelection(),
    },
    {
      key: "copy",
      label: "复制",
      shortcut: "Ctrl+C",
      disabled: !hasSel,
      action: () => editorRef.value?.copySelection(),
    },
    {
      key: "paste",
      label: "粘贴",
      shortcut: "Ctrl+V",
      action: () => editorRef.value?.pasteClipboard(),
    },
    { key: "s1", sep: true },
    {
      key: "select-all",
      label: "全选",
      shortcut: "Ctrl+A",
      action: () => editorRef.value?.selectAll(),
    },
  ];
  if (t) {
    items.push(
      { key: "s2", sep: true },
      {
        key: "save",
        label: "保存",
        icon: markRaw(IconSave),
        shortcut: "Ctrl+S",
        disabled: !isDirty(t),
        action: () => saveTab(t),
      },
    );
  }
  openMenu(p.x, p.y, items);
}

function onGlobalKeydown(e: KeyboardEvent) {
  if (e.defaultPrevented) return;
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
    e.preventDefault();
    const t = active.value;
    if (t && isDirty(t)) saveTab(t);
    return;
  }
  const tag = (e.target as HTMLElement | null)?.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return;
  if (e.key === "F5") {
    e.preventDefault();
    reload();
    return;
  }
}

watch(
  () => props.serverId,
  () => {
    cwd.value = "";
    backStack.value = [];
    fwdStack.value = [];
    filter.value = "";
    entries.value = [];
    openTabs.value = [];
    activeRel.value = null;
    selected.value = null;
    closeMenu();
    reload();
  }
);

onMounted(() => {
  window.addEventListener("keydown", onGlobalKeydown);
  reload();
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onGlobalKeydown);
  closeMenu();
});
</script>

<template>
  <div class="fm">
    <!-- 左侧：文件浏览 -->
    <aside class="fm-side glass">
      <div class="fm-path">
        <button class="nav" :disabled="!backStack.length" title="后退" @click="goBack">
          <IconChevronLeft />
        </button>
        <button class="nav" :disabled="!fwdStack.length" title="前进" @click="goForward">
          <IconChevronRight />
        </button>
        <button class="nav" :disabled="!cwd" title="上一级" @click="goUp">
          <IconCornerUpLeft />
        </button>
        <div class="fm-crumbs">
          <button class="crumb root" @click="navigate('')">服务器目录</button>
          <template v-for="c in crumbs" :key="c.rel">
            <span class="sep">/</span>
            <button class="crumb" :title="c.name" @click="navigate(c.rel)">{{ c.name }}</button>
          </template>
        </div>
        <button class="nav" title="刷新" @click="reload"><IconRefresh /></button>
      </div>

      <div class="fm-tools">
        <label class="fm-search">
          <IconSearch />
          <input v-model="filter" placeholder="筛选当前目录" spellcheck="false" />
        </label>
        <button class="tool" title="在系统文件管理器中打开" @click="reveal(cwd)">
          <IconExternal />
        </button>
      </div>

      <div class="fm-list" @contextmenu.prevent="onBlankContext">
        <div v-if="loading && !entries.length" class="fm-empty">加载中…</div>
        <div v-else-if="!shown.length" class="fm-empty">
          {{ filter ? "没有匹配的文件" : "这个文件夹是空的" }}
        </div>

        <template v-else>
          <div
            v-for="e in shown"
            :key="e.rel"
            class="fm-row"
            :class="{ active: activeRel === e.rel, selected: selected === e.rel }"
            @click="
              selected = e.rel;
              openEntry(e);
            "
            @contextmenu.prevent.stop="onRowContext($event, e)"
          >
            <span class="fm-badge" :class="e.is_dir ? 'dir' : 'code'">
              <IconFolder v-if="e.is_dir" />
              <template v-else>{{ badge(e) }}</template>
            </span>

            <div class="fm-meta">
              <div class="fm-name" :title="e.name">{{ e.name }}</div>
              <div class="fm-sub">
                {{ e.is_dir ? "文件夹" : fmtSize(e.size)
                }}<template v-if="e.modified"> · {{ fmtDate(e.modified) }}</template>
              </div>
            </div>
            <div class="fm-acts">
              <button
                v-if="isEditable(e)"
                class="act"
                title="在编辑器中打开"
                @click.stop="openEntry(e)"
              >
                <IconEdit />
              </button>
            </div>
          </div>
        </template>
      </div>
    </aside>

    <!-- 右侧：编辑器 -->
    <section class="fm-main glass">
      <div class="fm-tabs">
        <div
          v-for="t in openTabs"
          :key="t.rel"
          class="fm-tab"
          :class="{ active: t.rel === activeRel }"
          :title="t.rel"
          @click="activeRel = t.rel"
          @contextmenu.prevent.stop="onTabContext($event, t)"
        >
          <span class="dot" v-if="isDirty(t)"></span>
          <span class="fm-tab-name">{{ t.name }}</span>
          <button class="x" title="关闭" @click.stop="requestClose(t)"><IconClose /></button>
        </div>
        <div class="fm-tabs-right">
          <button
            class="save-btn"
            :disabled="!active || saving || !isDirty(active)"
            @click="saveTab()"
          >
            <IconSave /> {{ saving ? "保存中…" : "保存" }}
          </button>
        </div>
      </div>

      <div v-if="active" class="fm-editor">
        <CodeEditor
          ref="editorRef"
          :model-value="active.content"
          :filename="active.name"
          @update:model-value="onEdit(active.rel, $event)"
          @save="saveTab(active)"
          @contextmenu="onEditorContext"
        />
      </div>
      <div v-else class="fm-placeholder">
        <IconFile />
        <p>从左侧选择一个文本文件开始编辑</p>
        <span>支持 server.properties、ops.json、config/*.toml、日志等常见文本文件</span>
      </div>
    </section>

    <!-- 确认弹窗 -->
    <div v-if="confirmState" class="fm-mask" @click.self="confirmState = null">
      <div class="fm-dialog glass">
        <h4>{{ confirmState.title }}</h4>
        <p>{{ confirmState.content }}</p>
        <div class="fm-dialog-actions">
          <button class="btn ghost" :disabled="confirmLoading" @click="confirmState = null">
            取消
          </button>
          <button class="btn danger" :disabled="confirmLoading" @click="handleConfirm">
            {{ confirmState.positiveText }}
          </button>
        </div>
      </div>
    </div>

    <!-- 右键菜单 -->
    <ContextMenu
      :show="menu.show"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      @close="closeMenu"
    />
  </div>
</template>

<style scoped>
.fm {
  display: flex;
  gap: 14px;
  height: 480px;
  min-height: 420px;
}

.fm-side {
  width: 400px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.fm-path {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
}
.nav {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid transparent;
  border-radius: 7px;
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
  font-size: 13px;
  flex-shrink: 0;
}
.nav:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.07);
  color: var(--text-1);
}
.nav:disabled {
  opacity: 0.3;
  cursor: default;
}
.fm-crumbs {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  overflow-x: auto;
  scrollbar-width: none;
  white-space: nowrap;
}
.fm-crumbs::-webkit-scrollbar {
  display: none;
}
.crumb {
  border: none;
  background: transparent;
  color: var(--text-2);
  font-size: 12px;
  font-family: inherit;
  padding: 3px 5px;
  border-radius: 6px;
  cursor: pointer;
  max-width: 130px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.crumb:hover {
  background: rgba(255, 255, 255, 0.07);
  color: var(--text-1);
}
.crumb.root {
  color: var(--accent);
  font-weight: 600;
}
.sep {
  color: var(--text-3);
  font-size: 11px;
  opacity: 0.6;
}

.fm-tools {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
}
.fm-search {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 9px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.18);
  color: var(--text-3);
  font-size: 12px;
}
.fm-search input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  color: var(--text-1);
  font-size: 12px;
  font-family: inherit;
  outline: none;
  user-select: text;
}
.tool {
  position: relative;
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
  font-size: 13px;
  flex-shrink: 0;
}
.tool:hover {
  background: rgba(255, 255, 255, 0.07);
  color: var(--text-1);
}

.fm-list {
  flex: 1;
  overflow-y: auto;
  padding: 4px;
}
.fm-empty {
  padding: 40px 12px;
  text-align: center;
  color: var(--text-3);
  font-size: 13px;
}

.fm-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 8px;
  border-radius: 9px;
  cursor: pointer;
  transition: background 0.1s;
  user-select: none;
}
.fm-row:hover {
  background: var(--panel-hover);
}
.fm-row.active {
  background: var(--accent-12);
}
.fm-row.selected {
  box-shadow: inset 0 0 0 1px var(--accent-40);
}
.fm-row.selected:hover {
  background: var(--accent-16);
}

.fm-badge {
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.2px;
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-3);
}
.fm-badge.dir {
  color: var(--accent);
  background: var(--accent-14);
  font-size: 15px;
}
.fm-badge.code {
  color: #7cb8f5;
  background: rgba(90, 162, 240, 0.14);
}

.fm-meta {
  flex: 1;
  min-width: 0;
}
.fm-name {
  font-size: 12.5px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.fm-sub {
  font-size: 11px;
  color: var(--text-3);
  margin-top: 2px;
}

.fm-acts {
  display: flex;
  gap: 4px;
  opacity: 0;
  flex-shrink: 0;
  transition: opacity 0.12s;
}
.fm-row:hover .fm-acts {
  opacity: 1;
}
.act {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
  font-size: 12px;
}
.act:hover {
  color: var(--text-1);
  background: rgba(255, 255, 255, 0.08);
}

/* ---- 编辑器 ---- */
.fm-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.fm-tabs {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 5px 8px;
  border-bottom: 1px solid var(--border);
  overflow-x: auto;
  scrollbar-width: none;
}
.fm-tabs::-webkit-scrollbar {
  display: none;
}
.fm-tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px 5px 10px;
  border-radius: 8px;
  border: 1px solid transparent;
  color: var(--text-3);
  font-size: 12px;
  cursor: pointer;
  max-width: 180px;
  flex-shrink: 0;
  transition: background 0.1s;
}
.fm-tab:hover {
  background: rgba(255, 255, 255, 0.06);
}
.fm-tab.active {
  background: var(--accent-12);
  border-color: var(--accent-25);
  color: var(--text-1);
}
.fm-tab-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
  flex-shrink: 0;
}
.x {
  width: 16px;
  height: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  font-size: 11px;
  flex-shrink: 0;
}
.x:hover {
  background: rgba(255, 255, 255, 0.12);
  color: var(--text-1);
}
.fm-tabs-right {
  margin-left: auto;
  padding-left: 8px;
  flex-shrink: 0;
}
.save-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--accent-35);
  border-radius: 8px;
  background: var(--accent-12);
  color: var(--accent);
  font-size: 12px;
  font-weight: 600;
  font-family: inherit;
  padding: 5px 12px;
  cursor: pointer;
}
.save-btn:hover:not(:disabled) {
  background: var(--accent-20);
}
.save-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.fm-editor {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.fm-placeholder {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--text-3);
  font-size: 34px;
  padding: 20px;
}
.fm-placeholder p {
  margin: 6px 0 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-2);
}
.fm-placeholder span {
  font-size: 12px;
  opacity: 0.75;
  text-align: center;
}

/* ---- 确认弹窗 ---- */
.fm-mask {
  position: fixed;
  inset: 0;
  z-index: 900;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
}
.fm-dialog {
  width: min(420px, 92vw);
  padding: 20px;
}
.fm-dialog h4 {
  margin: 0 0 10px;
  font-size: 15px;
}
.fm-dialog p {
  margin: 0 0 18px;
  font-size: 13px;
  color: var(--text-2);
  line-height: 1.6;
}
.fm-dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.btn {
  border-radius: 9px;
  padding: 7px 16px;
  font-size: 13px;
  font-weight: 600;
  font-family: inherit;
  cursor: pointer;
  border: 1px solid var(--border);
}
.btn.ghost {
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-1);
}
.btn.danger {
  background: rgba(229, 83, 75, 0.14);
  color: #e5534b;
  border-color: rgba(229, 83, 75, 0.45);
}
.btn:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
