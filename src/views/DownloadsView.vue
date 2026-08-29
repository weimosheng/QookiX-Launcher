<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useTasksStore, type TaskEntry } from "../stores/tasks";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import {
  IconChevronDown,
  IconChevronRight,
  IconDownload,
} from "../components/icons";

const tasks = useTasksStore();
const router = useRouter();
const expanded = ref<Set<number>>(new Set());
const activeTab = ref<"active" | "finished">("active");

// 顶部 tab 的滑动高亮指示器
const tabBox = ref<HTMLElement | null>(null);
const { indicatorStyle: tabIndicatorStyle, refresh: refreshTabIndicator, snap: snapTabIndicator } = useSlidingIndicator(
  tabBox,
  () => Array.from(tabBox.value?.querySelectorAll<HTMLElement>(".tabs button") ?? []),
  () => (activeTab.value === "active" ? 0 : 1),
  { axis: "horizontal" }
);
const activeTasks = computed(() => tasks.taskList.filter((t) => !t.finished));
const finishedTasks = computed(() => tasks.taskList.filter((t) => t.finished));
const visibleTasks = computed(() => activeTab.value === "active" ? activeTasks.value : finishedTasks.value);

watch(activeTab, () => nextTick(() => refreshTabIndicator()));
watch([() => activeTasks.value.length, () => finishedTasks.value.length], () => nextTick(() => snapTabIndicator()));

const STAGE_LABELS: Record<string, string> = {
  manifest: "获取版本信息",
  client: "游戏客户端",
  libraries: "依赖库",
  natives: "解压运行库",
  assets: "资源文件",
  logging: "日志配置",
  loader: "加载器",
  content: "内容下载",
  modpack: "整合包下载",
  "modpack-install": "写入整合包",
  runtime: "Java 运行时",
  done: "完成",
  prepare: "准备中",
  download: "下载",
  extract: "解压",
  verify: "校验",
  install: "安装",
  fetch: "获取",
  resolve: "解析依赖",
  copy: "复制文件",
  write: "写入文件",
};

function stageLabel(t: TaskEntry) {
  return STAGE_LABELS[t.stage] ?? t.stage;
}

function fmtBytes(n: number) {
  if (n >= 1024 * 1024 * 1024) return (n / 1024 / 1024 / 1024).toFixed(2) + " GB";
  if (n >= 1024 * 1024) return (n / 1024 / 1024).toFixed(1) + " MB";
  if (n >= 1024) return (n / 1024).toFixed(1) + " KB";
  return n + " B";
}

function fmtSpeed(s: number) {
  if (s <= 0) return "—";
  if (s >= 1024 * 1024) return (s / 1024 / 1024).toFixed(1) + " MB/s";
  if (s >= 1024) return (s / 1024).toFixed(0) + " KB/s";
  return s.toFixed(0) + " B/s";
}

function fmtTime(ms: number) {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function pct(done: number, total: number) {
  if (!total) return 0;
  return Math.min(100, Math.round((done / total) * 100));
}

function downloadPct(t: TaskEntry) {
  if (t.bytesTotal > 0) return pct(t.bytesDone, t.bytesTotal);
  return pct(t.fileDone, t.fileTotal);
}

function statusText(t: TaskEntry) {
  if (t.finished) return t.ok === false ? "失败" : "完成";
  return "进行中";
}

function toggle(t: TaskEntry) {
  const next = new Set(expanded.value);
  if (next.has(t.id)) next.delete(t.id);
  else next.add(t.id);
  expanded.value = next;
}

function gotoInstance(t: TaskEntry) {
  if (t.instanceId) router.push(`/instance/${t.instanceId}`);
}

// 整合包会自动创建新实例，不是用户选择的目标实例，所以不显示“目标实例”
function isModpackTask(t: TaskEntry) {
  return (t.source ?? "").startsWith("整合包");
}
</script>

<template>
  <div class="dl-view">
    <div ref="tabBox" class="tabs">
      <div class="indicator" :style="tabIndicatorStyle"></div>
      <button :class="{ active: activeTab === 'active' }" @click="activeTab = 'active'">
        进行中 <span v-if="activeTasks.length" class="tab-count">{{ activeTasks.length }}</span>
      </button>
      <button :class="{ active: activeTab === 'finished' }" @click="activeTab = 'finished'">
        已完成 <span v-if="finishedTasks.length" class="tab-count">{{ finishedTasks.length }}</span>
      </button>
    </div>

    <div v-if="!visibleTasks.length" class="empty glass">
      <div class="empty-icon"><IconDownload /></div>
      <p>{{ activeTab === 'active' ? '暂无进行中的任务' : '没有已完成的任务' }}</p>
    </div>

    <div v-else class="task-list">
      <div v-for="t in visibleTasks" :key="t.id" class="task-card glass">
        <div class="task-top" @click="toggle(t)">
          <div class="task-main">
            <div class="task-title text-ellipsis">
              {{ t.source ?? t.message }}
              <span class="status" :class="t.finished ? (t.ok === false ? 'fail' : 'ok') : 'run'">
                {{ statusText(t) }}
              </span>
              <IconChevronDown v-if="expanded.has(t.id)" class="caret" />
              <IconChevronRight v-else class="caret" />
            </div>
            <div class="task-meta">
              <span class="meta-item">{{ fmtTime(t.startedAt) }}</span>
              <span
                v-if="t.instanceName && !isModpackTask(t)"
                class="meta-item link"
                @click.stop="gotoInstance(t)"
              >
                目标实例：{{ t.instanceName }} <IconChevronRight />
              </span>
              <span class="meta-item">{{ stageLabel(t) }}</span>
            </div>
          </div>
          <div class="task-side">
            <template v-if="t.activity === 'download' && !t.finished">
              <div class="speed">{{ fmtSpeed(t.speed) }}</div>
              <div class="stage">{{ t.fileDone }} / {{ t.fileTotal }} 个文件</div>
            </template>
            <template v-else-if="!t.finished">
              <div class="stage install">安装阶段</div>
              <div v-if="t.stepTotal" class="stage">{{ t.stepDone }} / {{ t.stepTotal }}</div>
            </template>
          </div>
        </div>

        <!-- error message for failed tasks -->
        <div v-if="t.finished && t.ok === false" class="task-error">
          {{ t.message }}
        </div>

        <!-- download progress -->
        <div v-if="t.activity === 'download' || t.finished" class="task-progress">
          <div class="bar">
            <div
              class="fill"
              :style="{ width: downloadPct(t) + '%' }"
            ></div>
          </div>
          <div class="bar-info">
            <span>{{ t.fileDone }} / {{ t.fileTotal }} 个文件</span>
            <span v-if="t.bytesTotal">
              {{ fmtBytes(t.bytesDone) }} / {{ fmtBytes(t.bytesTotal) }}
            </span>
            <span v-else-if="t.bytesDone">{{ fmtBytes(t.bytesDone) }}</span>
          </div>
        </div>

        <!-- install step progress -->
        <div v-else class="task-progress">
          <div class="bar">
            <div
              class="fill"
              :style="{ width: pct(t.stepDone, t.stepTotal) + '%' }"
            ></div>
          </div>
          <div class="bar-info">
            <span>{{ t.message }}</span>
            <span v-if="t.stepTotal">{{ t.stepDone }} / {{ t.stepTotal }}</span>
          </div>
        </div>

        <!-- details -->
        <div v-if="expanded.has(t.id)" class="task-detail">
          <div class="detail-row">
            <span class="dl-label">当前文件</span>
            <span class="dl-value mono text-ellipsis">{{ t.current || t.message || "—" }}</span>
          </div>
          <div class="detail-row">
            <span class="dl-label">平均速度</span>
            <span class="dl-value">{{ fmtSpeed(t.speed) }}</span>
          </div>
          <div v-if="t.files.length || t.current" class="detail-row files">
            <span class="dl-label">文件明细</span>
            <div class="dl-files">
              <div v-if="t.current && !t.finished" class="file-row active">
                <span class="file-status">→</span>
                <span class="file-name text-ellipsis">{{ t.current }}</span>
              </div>
              <div
                v-for="(f, i) in t.files.slice(-30).reverse()"
                :key="i"
                class="file-row"
                :class="f.ok ? 'ok' : 'fail'"
              >
                <span class="file-status">{{ f.ok ? '✓' : '✗' }}</span>
                <span class="file-name text-ellipsis">{{ f.name }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dl-view {
  max-width: 980px;
  margin: 0 auto;
}
.tabs {
  position: relative;
  display: flex;
  gap: 4px;
  margin-bottom: 16px;
  border-bottom: 1px solid var(--border);
  padding-bottom: 0;
}
.tabs .indicator {
  position: absolute;
  top: 2px;
  bottom: 2px;
  border-radius: 8px;
  background: var(--accent-soft);
  pointer-events: none;
}
.tabs button {
  border: none;
  background: transparent;
  color: var(--text-3);
  padding: 8px 18px;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.15s;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.tabs button:hover {
  color: var(--text-1);
}
.tabs button.active {
  color: var(--accent);
}
.tab-count {
  font-size: 11px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 6px;
  padding: 1px 7px;
  font-weight: 700;
}

.btn {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.05);
  color: var(--text-1);
  border-radius: 9px;
  padding: 8px 14px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
}
.btn:disabled {
  opacity: 0.4;
  cursor: default;
}
.empty {
  padding: 60px;
  text-align: center;
  color: var(--text-3);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
}
.empty-icon {
  font-size: 34px;
  color: var(--text-3);
  opacity: 0.6;
}
.task-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.task-card {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.task-top {
  display: flex;
  justify-content: space-between;
  gap: 14px;
  cursor: pointer;
  transition: transform 0.1s ease;
}
.task-top:active {
  transform: scale(0.98);
}
.task-main {
  min-width: 0;
  flex: 1;
}
.task-title {
  font-size: 14px;
  font-weight: 700;
  margin-bottom: 6px;
  display: flex;
  align-items: center;
  gap: 8px;
}
.caret {
  font-size: 13px;
  color: var(--text-3);
  margin-left: auto;
  flex-shrink: 0;
}
.status {
  font-size: 11px;
  font-weight: 600;
  padding: 1px 8px;
  border-radius: 7px;
  flex-shrink: 0;
}
.status.run {
  color: var(--accent);
  background: var(--accent-soft);
}
.status.ok {
  color: #4ec9a0;
  background: rgba(78, 201, 160, 0.12);
}
.status.fail {
  color: #e5534b;
  background: rgba(229, 83, 75, 0.12);
}
.task-meta {
  display: flex;
  align-items: center;
  gap: 14px;
  font-size: 12px;
  color: var(--text-3);
  flex-wrap: wrap;
}
.meta-item {
  display: inline-flex;
  align-items: center;
  gap: 3px;
}
.meta-item.link {
  color: var(--accent);
  cursor: pointer;
}
.task-side {
  text-align: right;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  justify-content: center;
}
.speed {
  font-size: 16px;
  font-weight: 700;
  color: var(--accent);
  font-variant-numeric: tabular-nums;
}
.stage {
  font-size: 11px;
  color: var(--text-3);
}
.stage.install {
  color: var(--accent);
  font-weight: 600;
}
.task-error {
  background: rgba(229, 83, 75, 0.1);
  border: 1px solid rgba(229, 83, 75, 0.3);
  border-radius: 8px;
  padding: 8px 12px;
  font-size: 13px;
  color: #e5534b;
  word-break: break-all;
  user-select: text;
  -webkit-user-select: text;
  cursor: text;
}
.task-progress {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.bar {
  height: 6px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.08);
  overflow: hidden;
}
.fill {
  height: 100%;
  border-radius: 4px;
  background: linear-gradient(90deg, var(--accent-deep), var(--accent));
  transition: width 0.3s ease;
}
.bar-info {
  display: flex;
  justify-content: space-between;
  font-size: 11px;
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
  gap: 12px;
}
.task-detail {
  border-top: 1px solid var(--border);
  padding-top: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  font-size: 12px;
}
.detail-row {
  display: flex;
  gap: 12px;
  align-items: baseline;
}
.dl-label {
  color: var(--text-3);
  flex-shrink: 0;
  width: 70px;
}
.dl-value {
  color: var(--text-2);
  min-width: 0;
}
.detail-row.files {
  flex-direction: column;
  gap: 6px;
  align-items: flex-start;
}
.dl-files {
  display: flex;
  flex-direction: column;
  gap: 3px;
  width: 100%;
  max-height: 320px;
  overflow-y: auto;
}
.file-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  padding: 4px 10px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.03);
  font-family: "Consolas", "Segoe UI Mono", monospace;
}
.file-row.ok {
  background: rgba(78, 201, 160, 0.06);
}
.file-row.fail {
  background: rgba(229, 83, 75, 0.06);
}
.file-row.active {
  background: rgba(232, 154, 75, 0.08);
}
.file-status {
  font-weight: 700;
  flex-shrink: 0;
  width: 16px;
  text-align: center;
}
.file-row.ok .file-status {
  color: #4ec9a0;
}
.file-row.fail .file-status {
  color: #e5534b;
}
.file-row.active .file-status {
  color: var(--accent, #e89a4b);
}
.file-name {
  color: var(--text-2);
  min-width: 0;
  flex: 1;
}
.file-progress {
  color: var(--accent, #e89a4b);
  font-weight: 600;
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
}
</style>
