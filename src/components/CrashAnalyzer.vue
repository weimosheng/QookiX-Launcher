<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { NButton, NSpin, useMessage } from "naive-ui";
import { api } from "../api";
import { fmtDateLocale as fmtTime, fmtSize } from "../utils/format";
import type { CrashDiagnosis } from "../types";
import {
  IconAlertCircle,
  IconBug,
  IconChevronDown,
  IconChevronRight,
  IconCopy,
  IconFile,
  IconRefresh,
  IconTrash,
} from "./icons";

const props = defineProps<{ instanceId: string }>();
const message = useMessage();

const loading = ref(false);
const analyzing = ref(false);
const logs = ref<{ filename: string; modified: number; size: number; kind: string }[]>([]);
const selected = ref<string>("");
const diagnosis = ref<CrashDiagnosis | null>(null);
const rawContent = ref("");
const showRaw = ref(false);

// Tauri 的 invoke 报错可能是字符串，也可能是带 message 的对象，
// 统一转成可读文本，避免界面上只弹出一个 [object Object] 或啥都没有。
function errText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}

// —— 分析结果缓存 ——
// 崩溃报告内容基本不会变，每次切走再回来都重新分析既慢又费（要读文件 + 跑诊断）。
// 按 实例 + 文件名 缓存诊断，TTL 1 小时。重新分析会强制刷新。
const DIAG_TTL = 7 * 24 * 60 * 60 * 1000;
interface DiagCache {
  d: CrashDiagnosis;
  ts: number;
}
function diagKey(instanceId: string, filename: string) {
  return `qookix:crash_diag:${instanceId}:${filename}`;
}
function readDiagCache(instanceId: string, filename: string): CrashDiagnosis | null {
  try {
    const raw = localStorage.getItem(diagKey(instanceId, filename));
    if (!raw) return null;
    const c = JSON.parse(raw) as DiagCache;
    if (Date.now() - c.ts > DIAG_TTL) return null;
    return c.d;
  } catch {
    return null;
  }
}
function writeDiagCache(instanceId: string, filename: string, d: CrashDiagnosis) {
  try {
    localStorage.setItem(diagKey(instanceId, filename), JSON.stringify({ d, ts: Date.now() }));
  } catch {
    /* 容量溢出等忽略，不影响主流程 */
  }
}

async function loadLogs() {
  loading.value = true;
  try {
    const r = await api.crashAnalysis(props.instanceId);
    logs.value = r;
    if (logs.value.length && !selected.value) {
      // 自动选中第一个文件时必须同时恢复其缓存诊断，
      // 否则切走再回来 diagnosis 为空，看起来像"缓存没生效"。
      selected.value = logs.value[0].filename;
      diagnosis.value = readDiagCache(props.instanceId, selected.value);
    }
  } catch (e) {
    console.error("[CrashAnalyzer] loadLogs failed:", e);
    message.error("加载崩溃报告失败：" + errText(e));
  } finally {
    loading.value = false;
  }
}

async function analyze(force = false) {
  if (!selected.value) {
    // 没选文件时给明确提示，而不是静默 return（否则看起来像"点了没反应"）
    if (!logs.value.length) {
      message.warning("该实例暂无崩溃报告");
    } else {
      message.warning("请先选择一个崩溃报告");
    }
    return;
  }
  // 命中有效缓存且非强制刷新：直接展示，不发请求
  if (!force) {
    const cached = readDiagCache(props.instanceId, selected.value);
    if (cached) {
      diagnosis.value = cached;
      return;
    }
  }
  analyzing.value = true;
  diagnosis.value = null;
  rawContent.value = "";
  try {
    const d = await api.analyzeCrash(props.instanceId, selected.value);
    diagnosis.value = d;
    writeDiagCache(props.instanceId, selected.value, d);
  } catch (e) {
    console.error("[CrashAnalyzer] analyze failed:", e);
    message.error("分析失败：" + errText(e));
  } finally {
    analyzing.value = false;
  }
}

async function loadRaw() {
  if (!selected.value) {
    message.warning("请先选择一个崩溃报告");
    return;
  }
  try {
    rawContent.value = await api.getCrashReportContent(props.instanceId, selected.value);
  } catch (e) {
    console.error("[CrashAnalyzer] loadRaw failed:", e);
    message.error("读取报告失败：" + errText(e));
  }
}

// 点「查看原始报告」时：若还没加载过原始内容就顺手拉取，不必再单独点一次
async function ensureRaw() {
  if (rawContent.value) return;
  await loadRaw();
}

// 展开/收起原始报告：展开时若未加载则自动拉取
async function toggleRaw() {
  showRaw.value = !showRaw.value;
  if (showRaw.value) await ensureRaw();
}

async function deleteLog(filename: string) {
  if (!confirm(`确定删除 ${filename}？`)) return;
  try {
    await api.deleteInstancePath(props.instanceId, `crash-reports/${filename}`);
    // 顺手清掉对应诊断缓存，避免留下孤儿数据
    localStorage.removeItem(diagKey(props.instanceId, filename));
    message.success("已删除");
    await loadLogs();
    diagnosis.value = null;
    rawContent.value = "";
  } catch (e) {
    message.error(String(e));
  }
}

function copyExcerpt() {
  if (!diagnosis.value?.excerpt) return;
  try {
    navigator.clipboard.writeText(diagnosis.value.excerpt);
    message.success("已复制");
  } catch {
    message.error("复制失败");
  }
}

function copyRaw() {
  if (!rawContent.value) return;
  try {
    navigator.clipboard.writeText(rawContent.value);
    message.success("已复制");
  } catch {
    message.error("复制失败");
  }
}

const severityColor = computed(() => {
  if (!diagnosis.value) return "";
  switch (diagnosis.value.severity) {
    case "oom":
      return "#e5534b";
    case "jvm":
      return "#e0a030";
    case "gl":
      return "#5aa2f0";
    case "mod":
      return "#7ad08a";
    case "lwjgl":
      return "#c78aff";
    case "java_ver":
      return "#ff7a90";
    default:
      return "#8b8e9c";
  }
});

const severityBg = computed(() => {
  if (!diagnosis.value) return "";
  switch (diagnosis.value.severity) {
    case "oom":
      return "rgba(229,83,75,0.12)";
    case "jvm":
      return "rgba(224,160,48,0.12)";
    case "gl":
      return "rgba(90,162,240,0.12)";
    case "mod":
      return "rgba(122,208,138,0.12)";
    case "lwjgl":
      return "rgba(199,138,255,0.12)";
    case "java_ver":
      return "rgba(255,122,144,0.12)";
    default:
      return "rgba(139,142,156,0.12)";
  }
});

watch(
  () => props.instanceId,
  () => {
    logs.value = [];
    selected.value = "";
    diagnosis.value = null;
    rawContent.value = "";
    loadLogs();
  },
  { immediate: true }
);

watch(selected, () => {
  diagnosis.value = null;
  rawContent.value = "";
});

function handleSelect(filename: string) {
  selected.value = filename;
  // 切换文件：清掉上一次的原始内容；诊断优先走缓存（命中即直接展示）
  rawContent.value = "";
  showRaw.value = false;
  const cached = readDiagCache(props.instanceId, filename);
  diagnosis.value = cached;
}
</script>

<template>
  <div class="crash-analyzer">
    <div v-if="loading" class="crash-loading">
      <NSpin size="medium" />
      <span>正在扫描崩溃报告…</span>
    </div>

    <div v-else-if="logs.length === 0" class="crash-empty">
      <IconBug />
      <p>暂无崩溃报告</p>
      <span class="crash-empty-hint">游戏正常退出时不会生成崩溃报告。</span>
    </div>

    <div v-else class="crash-body">
      <!-- 日志列表 -->
      <div class="crash-log-list">
        <div class="crash-log-header">
          <span class="crash-log-title">崩溃报告</span>
          <span class="crash-log-count">{{ logs.length }} 个</span>
        </div>
        <div class="crash-log-items">
          <div
            v-for="l in logs"
            :key="l.filename"
            class="crash-log-item"
            :class="{ active: selected === l.filename }"
          >
            <button class="crash-log-select" @click="handleSelect(l.filename)">
              <div class="crash-log-icon">
                <IconFile v-if="l.kind === 'crash'" />
                <IconAlertCircle v-else />
              </div>
              <div class="crash-log-info">
                <div class="crash-log-name text-ellipsis">{{ l.filename }}</div>
                <div class="crash-log-meta">
                  <span>{{ l.kind === "crash" ? "崩溃报告" : "JVM 日志" }}</span>
                  <span>·</span>
                  <span>{{ fmtSize(l.size) }}</span>
                  <span>·</span>
                  <span>{{ fmtTime(l.modified) }}</span>
                </div>
              </div>
            </button>
            <button
              class="crash-log-del"
              title="删除"
              @click="deleteLog(l.filename)"
            >
              <IconTrash />
            </button>
          </div>
        </div>
      </div>

      <!-- 分析结果 -->
      <div class="crash-result">
        <div v-if="analyzing" class="crash-analyzing">
          <NSpin size="medium" />
          <span>正在分析崩溃原因…</span>
        </div>

        <!-- 已出诊断：优先于「分析」按钮展示，否则点完按钮结果永远不显示 -->
        <div v-else-if="diagnosis" class="crash-diagnosis">
          <!-- 严重度标签 -->
          <div class="crash-severity" :style="{ background: severityBg, color: severityColor }">
            <span class="crash-severity-dot" :style="{ background: severityColor }" />
            <span class="crash-severity-text">
              {{ diagnosis.severity === 'oom' ? '内存不足' : diagnosis.severity === 'jvm' ? 'JVM 崩溃' : diagnosis.severity === 'gl' ? '显卡问题' : diagnosis.severity === 'mod' ? '模组问题' : diagnosis.severity === 'lwjgl' ? '依赖缺失' : diagnosis.severity === 'java_ver' ? '版本问题' : '未知' }}
            </span>
          </div>

          <!-- 标题 -->
          <h3 class="crash-title">{{ diagnosis.title }}</h3>

          <!-- 原因 -->
          <div class="crash-section">
            <div class="crash-section-label">原因</div>
            <p class="crash-reason">{{ diagnosis.reason }}</p>
          </div>

          <!-- 摘录 -->
          <div v-if="diagnosis.excerpt" class="crash-section">
            <div class="crash-section-label">
              关键信息
              <NButton quaternary size="tiny" @click="copyExcerpt">
                <IconCopy />
              </NButton>
            </div>
            <p class="crash-excerpt">{{ diagnosis.excerpt }}</p>
          </div>

          <!-- 受影响模组 -->
          <div v-if="diagnosis.affected_mods.length" class="crash-section">
            <div class="crash-section-label">受影响模组</div>
            <div class="crash-mods">
              <span v-for="m in diagnosis.affected_mods" :key="m" class="crash-mod">{{ m }}</span>
            </div>
          </div>

          <!-- 建议 -->
          <div class="crash-section">
            <div class="crash-section-label">修复建议</div>
            <p class="crash-advice">{{ diagnosis.advice }}</p>
          </div>

          <!-- 操作 -->
          <div class="crash-actions">
            <NButton quaternary size="small" @click="analyze(true)">
              <IconRefresh />
              重新分析
            </NButton>
            <NButton quaternary size="small" @click="toggleRaw">
              <IconChevronRight v-if="!showRaw" />
              <IconChevronDown v-else />
              查看原始报告
            </NButton>
          </div>

          <!-- 原始内容：点「查看原始报告」即自动加载，无需再单独点一次 -->
          <div v-if="showRaw" class="crash-raw">
            <NSpin v-if="!rawContent" size="small" />
            <template v-else>
              <pre>{{ rawContent }}</pre>
              <NButton quaternary size="small" @click="copyRaw">
                <IconCopy />
                复制全部
              </NButton>
            </template>
          </div>
        </div>

        <!-- 尚未分析：显示「分析此崩溃报告」按钮（selected 为真但还没出结果） -->
        <div v-else class="crash-prompt">
          <NButton type="primary" size="small" :disabled="analyzing || !selected" @click="analyze()">
            分析此崩溃报告
          </NButton>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.crash-analyzer {
  display: flex;
  flex-direction: column;
  height: 100%;
  gap: 16px;
}

/* 磨砂玻璃适配：卡片与卡片式按钮统一加上背景模糊，跟随主题 --glass-blur */
.crash-result,
.crash-log-item,
.crash-excerpt,
.crash-raw,
.crash-advice {
  backdrop-filter: blur(var(--glass-blur, 8px));
  -webkit-backdrop-filter: blur(var(--glass-blur, 8px));
}

.crash-loading,
.crash-analyzing,
.crash-prompt {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 40px 0;
  color: var(--text-3);
  font-size: 14px;
}

.crash-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 60px 0;
  color: var(--text-3);
  font-size: 14px;
}
.crash-empty p {
  margin: 0;
  font-size: 15px;
  color: var(--text-2);
}
.crash-empty-hint {
  font-size: 12px;
  color: var(--text-3);
}

.crash-body {
  display: grid;
  grid-template-columns: 280px 1fr;
  gap: 16px;
  height: 100%;
  min-height: 0;
}

/* 日志列表 */
.crash-log-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
}
.crash-log-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.crash-log-title {
  font-size: 13px;
  font-weight: 700;
  color: var(--text-1);
}
.crash-log-count {
  font-size: 11px;
  color: var(--text-3);
  background: var(--panel);
  padding: 2px 8px;
  border-radius: 20px;
}
.crash-log-items {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 360px;
  overflow-y: auto;
}
.crash-log-item {
  display: flex;
  align-items: center;
  gap: 10px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: var(--panel);
  transition: all 0.12s;
}
/* 选择区：撑满条目并承载 padding，使条目内除删除键外的区域都可点击 */
.crash-log-select {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border: none;
  background: transparent;
  color: inherit;
  font-family: inherit;
  text-align: left;
  cursor: pointer;
}
.crash-log-item:hover {
  background: var(--panel-hover);
}
.crash-log-item.active {
  border-color: var(--accent);
  background: var(--accent-soft);
}
.crash-log-icon {
  flex-shrink: 0;
  color: var(--text-3);
  font-size: 16px;
}
.crash-log-item.active .crash-log-icon {
  color: var(--accent);
}
.crash-log-info {
  flex: 1;
  min-width: 0;
}
.crash-log-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.crash-log-meta {
  font-size: 11px;
  color: var(--text-3);
  display: flex;
  gap: 4px;
  margin-top: 2px;
}
.crash-log-del {
  flex-shrink: 0;
  margin-right: 12px;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-3);
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  font-size: 13px;
  opacity: 0;
  transition: all 0.12s;
}
.crash-log-item:hover .crash-log-del {
  opacity: 1;
}
.crash-log-del:hover {
  color: #e5534b;
  background: rgba(229, 83, 75, 0.1);
}

/* 分析结果 */
/* 分析结果卡片 */
.crash-result {
  display: flex;
  flex-direction: column;
  gap: 14px;
  overflow-y: auto;
  min-width: 0;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 16px;
}

.crash-severity {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  border-radius: 20px;
  font-size: 12px;
  font-weight: 600;
  width: fit-content;
}
.crash-severity-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
}

.crash-title {
  font-size: 20px;
  font-weight: 700;
  color: var(--text-1);
  margin: 0;
  line-height: 1.3;
}

.crash-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.crash-section-label {
  font-size: 11px;
  font-weight: 700;
  color: var(--text-3);
  letter-spacing: 0.5px;
  display: flex;
  align-items: center;
  gap: 6px;
}
.crash-reason {
  margin: 0;
  font-size: 14px;
  color: var(--text-2);
  line-height: 1.6;
}
.crash-excerpt {
  margin: 0;
  font-size: 13px;
  color: var(--text-1);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 12px;
  font-family: "Cascadia Code", Consolas, monospace;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
}
.crash-mods {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.crash-mod {
  font-size: 12px;
  padding: 3px 8px;
  border-radius: 6px;
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}
.crash-advice {
  margin: 0;
  font-size: 14px;
  color: var(--text-1);
  line-height: 1.7;
  background: rgba(122, 208, 138, 0.08);
  border: 1px solid rgba(122, 208, 138, 0.18);
  border-radius: 10px;
  padding: 12px 14px;
}

.crash-actions {
  display: flex;
  gap: 8px;
  padding-top: 4px;
}

.crash-raw {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 12px;
  overflow: auto;
  max-height: 300px;
}
.crash-raw pre {
  margin: 0;
  font-size: 12px;
  font-family: "Cascadia Code", Consolas, monospace;
  line-height: 1.5;
  color: var(--text-2);
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
