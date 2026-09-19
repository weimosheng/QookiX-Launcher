<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { useTasksStore, type TaskEntry } from "../stores/tasks";
import { fmtBytes, fmtSpeed, fmtTimeMs as fmtTime } from "../utils/format";
import { useInstancesStore } from "../stores/instances";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import {
  IconChevronDown,
  IconChevronRight,
  IconDownload,
} from "../components/icons";

const { t } = useI18n();
const tasks = useTasksStore();
const instances = useInstancesStore();
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
  manifest: "downloads.stage.manifest",
  client: "downloads.stage.client",
  libraries: "downloads.stage.libraries",
  natives: "downloads.stage.natives",
  assets: "downloads.stage.assets",
  logging: "downloads.stage.logging",
  loader: "downloads.stage.loader",
  content: "downloads.stage.content",
  modpack: "downloads.stage.modpack",
  "modpack-install": "downloads.stage.modpack-install",
  runtime: "downloads.stage.runtime",
  done: "downloads.stage.done",
  prepare: "downloads.stage.prepare",
  download: "downloads.stage.download",
  extract: "downloads.stage.extract",
  verify: "downloads.stage.verify",
  install: "downloads.stage.install",
  fetch: "downloads.stage.fetch",
  resolve: "downloads.stage.resolve",
  copy: "downloads.stage.copy",
  write: "downloads.stage.write",
};

function stageLabel(task: TaskEntry) {
  const key = STAGE_LABELS[task.stage];
  return key ? t(key) : task.stage;
}

function pct(done: number, total: number) {
  if (!total) return 0;
  return Math.min(100, Math.round((done / total) * 100));
}

function downloadPct(t: TaskEntry) {
  if (t.fraction != null && t.fraction >= 0) return Math.min(100, Math.round(t.fraction * 100));
  if (t.bytesTotal > 0) return pct(t.bytesDone, t.bytesTotal);
  return pct(t.fileDone, t.fileTotal);
}

function statusText(task: TaskEntry) {
  if (task.finished) return task.ok === false ? t("downloads.status.failed") : t("downloads.status.done");
  return t("downloads.status.running");
}

function toggle(t: TaskEntry) {
  const next = new Set(expanded.value);
  if (next.has(t.id)) next.delete(t.id);
  else next.add(t.id);
  expanded.value = next;
}

// 整合包会自动创建新实例，不是用户选择的目标实例，所以不显示"目标实例"
function isModpackTask(t: TaskEntry) {
  return (t.source ?? "").startsWith("整合包");
}

/**
 * 任务里的 `instanceId` 指向的实例是否已经真正可跳转。
 *
 * 实例详情页是从 instances store 里取数据的，而 store 只在启动时拉一次。
 * 整合包 / 导入这类"先下载、后建实例"的流程，任务一开始就带上了
 * instanceId，此时实例还没写进 instances.json（或者 store 还是旧快照），
 * 直接跳过去只会显示「实例不存在或已删除」。所以必须等到 store 里真的
 * 能查到这个实例才允许跳转。
 */
function instanceReady(t: TaskEntry) {
  return !!t.instanceId && !!instances.get(t.instanceId);
}

function gotoInstance(t: TaskEntry) {
  if (!instanceReady(t)) return;
  router.push(`/instance/${t.instanceId}`);
}

// 任务结束（成功或失败）时刷新实例列表：新建的实例这时才会进 store，
// 「目标实例」也随之从不可点击变成可跳转。
watch(
  () => tasks.taskList.filter((t) => t.finished).length,
  () => {
    void instances.load();
  }
);

onMounted(() => {
  if (!instances.instances.length) void instances.load();
});

// —— 展开 / 收起动画 ——
//
// 结构：外层 .task-detail-wrap（overflow:hidden，高度被动画驱动）
//       └ 内层 .task-detail（高度 auto，用来实时量真实内容高度）
// 分开两层是为了能一边动画一边持续测量内容高度——直接量被动画改写过
// height 的元素是量不准的。
//
// 两个导致"结尾顿一下"的坑，都必须堵掉：
//
// 1) 不能用 setTimeout(duration) 当结束信号。CSS transition 是在我们改完
//    样式后的下一帧才真正开始跑的，而定时器从调用那一刻就开始计时，所以
//    定时器必然比动画早 1~2 帧触发。那一刻高度大约只走到 97%，我们却把它
//    一把改成 auto —— 剩下 3% 就是那个"跳"。改成监听 transitionend
//    （只认 propertyName === 'height' 且 target 是自己），另设一个稍长的
//    兜底定时器防止极端情况卡住。
//
// 2) 动画期间内容还在长。下载中 activeFiles 每 400ms 更新、完成的文件不断
//    追加，内容高度会变。若目标高度只在开头量一次，结尾就会跳到新的 auto
//    高度。用 ResizeObserver 盯着内层，内容一变就把动画目标同步过去——
//    CSS transition 会从当前值平滑改道到新目标，不会跳。
//
// 另外 .task-card 是 flex + gap，元素一插入就会多出这段间距，动画期间用负
// margin-top 抵消，避免开头 / 结尾抖一下。
const DETAIL_GAP = 10;
const DETAIL_DUR = 240;
const DETAIL_EASE = "cubic-bezier(0.22, 1, 0.36, 1)";
const DETAIL_TRANSITION = `height ${DETAIL_DUR}ms ${DETAIL_EASE}, margin-top ${DETAIL_DUR}ms ${DETAIL_EASE}, opacity ${DETAIL_DUR}ms ${DETAIL_EASE}`;

/** 挂在动画元素上的收尾函数，连续快速点击时先取消上一次，避免两个动画打架 */
type AnimEl = HTMLElement & { _dlCancel?: () => void };

function innerOf(e: HTMLElement): HTMLElement | null {
  return e.firstElementChild instanceof HTMLElement ? e.firstElementChild : null;
}
function contentHeight(e: HTMLElement) {
  const inner = innerOf(e);
  return inner ? inner.offsetHeight : e.scrollHeight;
}

/** 等 height 真正跑完；定时器只作为兜底，不作为正常结束信号 */
function whenHeightDone(e: AnimEl, done: () => void, after?: () => void) {
  let settled = false;
  const finish = () => {
    if (settled) return;
    settled = true;
    e.removeEventListener("transitionend", onEnd);
    clearTimeout(timer);
    e._dlCancel = undefined;
    after?.();
    done();
  };
  // 子元素也会冒泡出 transitionend，认准 target 是自己 + 属性是 height
  const onEnd = (ev: TransitionEvent) => {
    if (ev.target !== e || ev.propertyName !== "height") return;
    finish();
  };
  // 兜底：高度没变（无 transition 触发）、元素被隐藏等场景下也要放行
  const timer = window.setTimeout(finish, DETAIL_DUR + 120);
  e._dlCancel = () => {
    if (settled) return;
    settled = true;
    e.removeEventListener("transitionend", onEnd);
    clearTimeout(timer);
    e._dlCancel = undefined;
  };
  e.addEventListener("transitionend", onEnd);
}

function onExpandEnter(el: Element, done: () => void) {
  const e = el as AnimEl;
  e._dlCancel?.();

  e.style.transition = "none";
  e.style.overflow = "hidden";
  e.style.height = "0px";
  e.style.marginTop = `-${DETAIL_GAP}px`;
  e.style.opacity = "0";
  void e.offsetHeight; // 强制回流，让起始态生效

  const inner = innerOf(e);
  let ro: ResizeObserver | null = null;
  if (inner) {
    ro = new ResizeObserver(() => {
      e.style.height = `${contentHeight(e)}px`;
    });
    ro.observe(inner);
  }

  e.style.transition = DETAIL_TRANSITION;
  e.style.height = `${contentHeight(e)}px`;
  e.style.marginTop = "0px";
  e.style.opacity = "1";

  whenHeightDone(
    e,
    done,
    () => {
      ro?.disconnect();
      e.style.transition = "none";
      // 交还 auto，让后续动态内容能自由撑开
      e.style.height = "";
      e.style.marginTop = "";
      e.style.opacity = "";
      e.style.overflow = "";
      void e.offsetHeight;
      e.style.transition = "";
    }
  );
}

function onExpandLeave(el: Element, done: () => void) {
  const e = el as AnimEl;
  e._dlCancel?.();

  e.style.transition = "none";
  e.style.overflow = "hidden";
  e.style.height = `${e.offsetHeight}px`;
  e.style.marginTop = "0px";
  e.style.opacity = "1";
  void e.offsetHeight; // 强制回流，让起始态生效

  e.style.transition = DETAIL_TRANSITION;
  e.style.height = "0px";
  e.style.marginTop = `-${DETAIL_GAP}px`;
  e.style.opacity = "0";

  whenHeightDone(e, done, () => {
    e.style.transition = "";
    e.style.height = "";
    e.style.marginTop = "";
    e.style.opacity = "";
    e.style.overflow = "";
  });
}
</script>

<template>
  <div class="dl-view">
    <div ref="tabBox" class="tabs">
      <div class="indicator" :style="tabIndicatorStyle"></div>
      <button :class="{ active: activeTab === 'active' }" @click="activeTab = 'active'">
        {{ t('downloads.tab.active') }} <span v-if="activeTasks.length" class="tab-count">{{ activeTasks.length }}</span>
      </button>
      <button :class="{ active: activeTab === 'finished' }" @click="activeTab = 'finished'">
        {{ t('downloads.tab.finished') }} <span v-if="finishedTasks.length" class="tab-count">{{ finishedTasks.length }}</span>
      </button>
    </div>

    <div v-if="!visibleTasks.length" class="empty glass">
      <div class="empty-icon"><IconDownload /></div>
      <p>{{ activeTab === 'active' ? t('downloads.empty.noActive') : t('downloads.empty.noFinished') }}</p>
    </div>

    <div v-else class="task-list">
      <div v-for="task in visibleTasks" :key="task.id" class="task-card glass">
        <div class="task-top" @click="toggle(task)">
          <div class="task-main">
            <div class="task-title text-ellipsis">
              {{ task.source ?? task.message }}
              <span class="status" :class="task.finished ? (task.ok === false ? 'fail' : 'ok') : 'run'">
                {{ statusText(task) }}
              </span>
              <IconChevronDown class="caret" :class="{ open: expanded.has(task.id) }" />
            </div>
            <div class="task-meta">
              <span class="meta-item">{{ fmtTime(task.startedAt) }}</span>
              <span
                v-if="task.instanceName && !isModpackTask(task)"
                class="meta-item"
                :class="instanceReady(task) ? 'link' : 'pending'"
                :title="instanceReady(task) ? t('downloads.instanceLink') : t('downloads.instancePending')"
                @click.stop="gotoInstance(task)"
              >
                {{ t('downloads.targetInstance', { name: task.instanceName }) }}
                <IconChevronRight v-if="instanceReady(task)" />
                <span v-else class="pending-tag">{{ t('downloads.creating') }}</span>
              </span>
              <span class="meta-item">{{ stageLabel(task) }}</span>
            </div>
          </div>
          <div class="task-side">
            <template v-if="task.activity === 'download' && !task.finished">
              <div class="speed">{{ fmtSpeed(task.speed) }}</div>
              <div v-if="task.fraction != null" class="stage">{{ Math.round(task.fraction * 100) }}%</div>
              <div v-else class="stage">{{ task.fileDone }} / {{ task.fileTotal }} {{ t('downloads.fileUnit') }}</div>
            </template>
            <template v-else-if="!task.finished">
              <div class="stage install">{{ t('downloads.installStage') }}</div>
              <div v-if="task.stepTotal" class="stage">{{ task.stepDone }} / {{ task.stepTotal }}</div>
            </template>
          </div>
        </div>

        <!-- error message for failed tasks -->
        <div v-if="task.finished && task.ok === false" class="task-error">
          {{ task.message }}
        </div>

        <!-- download progress -->
        <div v-if="task.activity === 'download' || task.finished" class="task-progress">
          <div class="bar">
            <div
              class="fill"
              :style="{ width: downloadPct(task) + '%' }"
            ></div>
          </div>
          <div class="bar-info">
            <span v-if="task.fraction != null">{{ Math.round(task.fraction * 100) }}%</span>
            <span v-else>{{ task.fileDone }} / {{ task.fileTotal }} {{ t('downloads.fileUnit') }}</span>
            <span v-if="task.bytesTotal">
              {{ fmtBytes(task.bytesDone) }} / {{ fmtBytes(task.bytesTotal) }}
            </span>
            <span v-else-if="task.bytesDone">{{ fmtBytes(task.bytesDone) }}</span>
          </div>
        </div>

        <!-- install step progress -->
        <div v-else class="task-progress">
          <div class="bar">
            <div
              class="fill"
              :style="{ width: pct(task.stepDone, task.stepTotal) + '%' }"
            ></div>
          </div>
          <div class="bar-info">
            <span>{{ task.message }}</span>
            <span v-if="task.stepTotal">{{ task.stepDone }} / {{ task.stepTotal }}</span>
          </div>
        </div>

        <!-- details -->
        <Transition
          :css="false"
          @enter="onExpandEnter"
          @leave="onExpandLeave"
        >
          <div v-if="expanded.has(task.id)" class="task-detail-wrap">
            <div class="task-detail">
              <div class="detail-row">
                <span class="dl-label">{{ t('downloads.detail.downloading') }}</span>
                <span class="dl-value">{{ task.activeFiles.length }} {{ t('downloads.fileUnit') }}</span>
              </div>
              <div class="detail-row">
                <span class="dl-label">{{ t('downloads.detail.avgSpeed') }}</span>
                <span class="dl-value">{{ fmtSpeed(task.speed) }}</span>
              </div>
              <div v-if="task.files.length || task.activeFiles.length" class="detail-row files">
                <span class="dl-label">{{ t('downloads.detail.files') }}</span>
                <div class="dl-files">
                  <div v-for="(f, i) in task.activeFiles" :key="'a'+i" class="file-current">
                    <div class="file-current-row">
                      <span class="file-status">→</span>
                      <span class="file-name text-ellipsis">{{ f.name }}</span>
                      <span v-if="f.bytesTotal" class="file-progress">{{ pct(f.bytesDone, f.bytesTotal) }}%</span>
                    </div>
                    <div v-if="f.bytesTotal" class="file-mini-bar">
                      <div class="file-mini-fill" :style="{ width: pct(f.bytesDone, f.bytesTotal) + '%' }"></div>
                    </div>
                  </div>
                  <div
                    v-for="(f, i) in task.files.slice(-30).reverse()"
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
        </Transition>
      </div>
    </div>
  </div>
</template>

<style scoped>
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
  background: var(--w-10);
  border-radius: 6px;
  padding: 1px 7px;
  font-weight: 700;
}

.btn {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: 1px solid var(--border);
  background: var(--w-05);
  color: var(--text-1);
  border-radius: 9px;
  padding: 8px 14px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.btn:hover:not(:disabled) {
  background: var(--w-10);
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
  display: grid;
  grid-template-columns: 1fr;
  gap: 12px;
  align-items: start;
}
/* 宽屏：任务卡片两列排布 */
@media (min-width: 1500px) {
  .task-list {
    grid-template-columns: 1fr 1fr;
  }
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
  /* 收起时指向右侧，展开时旋转 90° 指向下方；时长与详情区动画保持一致 */
  transform: rotate(-90deg);
  transition: transform 0.24s cubic-bezier(0.22, 1, 0.36, 1), color 0.15s;
}
.caret.open {
  transform: rotate(0deg);
  color: var(--accent);
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
  background: var(--success-12);
}
.status.fail {
  color: #e5534b;
  background: var(--danger-12);
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
/* 实例还没建好，跳过去只会看到「实例不存在或已删除」，先禁用 */
.meta-item.pending {
  cursor: not-allowed;
  opacity: 0.7;
}
/* 动画容器：只负责裁剪与高度过渡，内层 .task-detail 保持 auto 以便测量 */
.task-detail-wrap {
  overflow: hidden;
}

.pending-tag {
  font-size: 10px;
  font-weight: 600;
  padding: 0 5px;
  border-radius: 5px;
  color: var(--text-3);
  background: var(--w-08);
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
  background: var(--danger-10);
  border: 1px solid var(--danger-30);
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
  background: var(--w-08);
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
  background: var(--w-03);
  font-family: "Consolas", "Segoe UI Mono", monospace;
}
.file-row.ok {
  background: rgba(78, 201, 160, 0.06);
}
.file-row.fail {
  background: rgba(229, 83, 75, 0.06);
}
.file-row.active {
  background: var(--accent-08);
}
.file-current {
  padding: 5px 10px 6px;
  border-radius: 6px;
  background: var(--accent-08);
  font-size: 12px;
  font-family: "Consolas", "Segoe UI Mono", monospace;
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.file-current-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.file-mini-bar {
  height: 4px;
  border-radius: 2px;
  background: var(--w-10);
}
.file-mini-fill {
  height: 100%;
  border-radius: 2px;
  background: linear-gradient(90deg, var(--accent-deep, var(--accent)), var(--accent));
  transition: width 0.2s ease;
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
