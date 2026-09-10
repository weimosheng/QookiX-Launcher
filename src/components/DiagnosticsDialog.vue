<script setup lang="ts">
/**
 * 诊断中心：一键收集环境信息（系统 / Java / 显卡 / 网络 / 实例自检 /
 * 崩溃 / 日志），生成可复制的 Markdown 报告用于反馈与远程排障。
 * 报告已自动脱敏（系统用户名 / UUID / token）。
 */
import { ref, watch } from "vue";
import { NButton, NModal, NScrollbar, useMessage } from "naive-ui";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import type { DiagnosticReport, DiagnosticReportEntry } from "../types";
import { fmtSize, fmtTime } from "../utils/format";

const props = withDefaults(
  defineProps<{
    /** 指定实例时额外采集实例自检、崩溃、游戏日志 */
    instanceId?: string | null;
    instanceName?: string | null;
  }>(),
  { instanceId: null, instanceName: null },
);
const show = defineModel<boolean>("show", { required: true });

const message = useMessage();
const collecting = ref(false);
const report = ref<DiagnosticReport | null>(null);
/** 预览模式：结构化视图 / Markdown 全文 */
const previewMode = ref<"list" | "markdown">("list");

// 历史报告（自动持久化在数据目录 diagnostics/ 下）
const history = ref<DiagnosticReportEntry[]>([]);
const viewingHistory = ref<string | null>(null);

async function loadHistory() {
  try {
    history.value = await api.listDiagnosticsReports();
  } catch {
    history.value = [];
  }
}

async function openHistory(entry: DiagnosticReportEntry) {
  try {
    const md = await api.readDiagnosticsReport(entry.filename);
    report.value = {
      generated_at: entry.generated_at,
      sections: [],
      markdown: md,
      redactions: 0,
    };
    viewingHistory.value = entry.filename;
    previewMode.value = "markdown";
  } catch (e) {
    message.error(String(e));
  }
}

async function removeHistory(entry: DiagnosticReportEntry) {
  try {
    await api.deleteDiagnosticsReport(entry.filename);
    if (viewingHistory.value === entry.filename) viewingHistory.value = null;
    await loadHistory();
    message.success("已删除该报告");
  } catch (e) {
    message.error(String(e));
  }
}

async function collect() {
  collecting.value = true;
  try {
    report.value = await api.collectDiagnostics(props.instanceId);
    viewingHistory.value = null;
    previewMode.value = "list";
    await loadHistory();
  } catch (e) {
    message.error(String(e));
  } finally {
    collecting.value = false;
  }
}

watch(show, (v) => {
  if (v) {
    loadHistory();
    collect();
  }
});

async function copyAll() {
  if (!report.value) return;
  try {
    await navigator.clipboard.writeText(report.value.markdown);
    message.success("诊断报告已复制到剪贴板");
  } catch (e) {
    message.error(`复制失败：${String(e)}`);
  }
}

async function saveAs() {
  if (!report.value) return;
  const stamp = new Date().toISOString().slice(0, 19).replace(/[T:]/g, "-");
  const dest = await save({
    defaultPath: `qookix-diagnostics-${stamp}.md`,
    filters: [{ name: "诊断报告（Markdown）", extensions: ["md", "txt"] }],
  });
  if (!dest) return;
  try {
    await api.saveDiagnosticsReport(dest as string, report.value.markdown);
    message.success(`已保存到 ${dest}`);
  } catch (e) {
    message.error(String(e));
  }
}
</script>

<template>
  <n-modal
    v-model:show="show"
    preset="card"
    title="诊断报告"
    style="width: 720px; max-width: 95vw"
    :mask-closable="true"
    :close-on-esc="true"
  >
    <div class="dg-body">
      <div class="dg-head">
        <p class="dg-hint">
          一键收集运行环境与实例状态，用于反馈问题或远程排障。
          报告已自动脱敏（系统用户名 / 账号 UUID / access token），发送前建议自行过目。
          <template v-if="props.instanceName">
            当前包含实例：<b>{{ props.instanceName }}</b>
          </template>
        </p>
        <div class="dg-modes">
          <button
            :class="{ active: previewMode === 'list' }"
            :disabled="!report?.sections.length"
            @click="previewMode = 'list'"
          >
            分类查看
          </button>
          <button :class="{ active: previewMode === 'markdown' }" @click="previewMode = 'markdown'">Markdown 全文</button>
        </div>
      </div>

      <div v-if="collecting" class="dg-loading">
        正在采集（含网络探测，约几秒）…
      </div>

      <template v-else-if="report">
        <n-scrollbar style="max-height: 50vh" trigger="none">
          <div v-if="previewMode === 'list'" class="dg-sections">
            <section v-for="s in report.sections" :key="s.key" class="dg-section">
              <h4 class="dg-title">{{ s.title }}</h4>
              <ul class="dg-lines">
                <li v-for="(l, i) in s.lines" :key="i">{{ l }}</li>
              </ul>
              <details v-if="s.block" class="dg-block">
                <summary>查看详情</summary>
                <pre>{{ s.block }}</pre>
              </details>
            </section>
          </div>
          <pre v-else class="dg-markdown">{{ report.markdown }}</pre>
        </n-scrollbar>

        <p class="dg-meta">
          <template v-if="viewingHistory">正在查看历史报告（{{ viewingHistory }}）</template>
          <template v-else>
            已脱敏 <b>{{ report.redactions }}</b> 处 · 已自动保存到数据目录
            <span v-if="report.sections.length">· {{ report.sections.length }} 个分类</span>
          </template>
        </p>

        <div class="dg-history">
          <div class="dg-history-head">
            历史报告
            <span class="dg-history-note">自动保留最近 20 份</span>
          </div>
          <div v-if="!history.length" class="dg-history-empty">暂无</div>
          <n-scrollbar v-else style="max-height: 120px" trigger="none">
            <div
              v-for="h in history"
              :key="h.filename"
              class="dg-history-row"
              :class="{ active: viewingHistory === h.filename }"
            >
              <button class="dg-history-open" @click="openHistory(h)">
                {{ fmtTime(h.generated_at) }} · {{ fmtSize(h.size) }}
              </button>
              <button class="dg-history-del" title="删除" @click="removeHistory(h)">✕</button>
            </div>
          </n-scrollbar>
        </div>
      </template>

      <div class="dg-actions">
        <n-button :loading="collecting" @click="collect">重新采集</n-button>
        <n-button :disabled="!report || collecting" @click="copyAll">复制报告</n-button>
        <n-button type="primary" :disabled="!report || collecting" @click="saveAs">另存为文件</n-button>
      </div>
    </div>
  </n-modal>
</template>

<style scoped>
.dg-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.dg-head {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.dg-hint {
  margin: 0;
  font-size: 12px;
  line-height: 1.7;
  color: var(--text-3);
}
.dg-hint b {
  color: var(--text-1);
}
.dg-modes {
  display: flex;
  gap: 6px;
}
.dg-modes button {
  background: none;
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 3px 10px;
  font-size: 11px;
  color: var(--text-3);
  cursor: pointer;
}
.dg-modes button.active {
  color: var(--accent);
  border-color: var(--accent);
}
.dg-loading {
  padding: 32px 0;
  text-align: center;
  font-size: 13px;
  color: var(--text-3);
}
.dg-sections {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding-right: 8px;
}
.dg-section {
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 8px 12px;
  background: var(--panel-sub, rgba(255, 255, 255, 0.02));
}
.dg-title {
  margin: 0 0 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.dg-lines {
  margin: 0;
  padding-left: 16px;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.dg-lines li {
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-2);
  word-break: break-all;
}
.dg-block {
  margin-top: 6px;
}
.dg-block summary {
  font-size: 11px;
  color: var(--accent);
  cursor: pointer;
}
.dg-block pre,
.dg-markdown {
  margin: 6px 0 0;
  padding: 8px;
  background: rgba(0, 0, 0, 0.22);
  border-radius: 6px;
  font-size: 11px;
  line-height: 1.6;
  color: var(--text-2);
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 260px;
  overflow: auto;
}
.dg-meta {
  margin: 0;
  font-size: 11px;
  color: var(--text-3);
}
.dg-meta b {
  color: var(--accent);
}
.dg-history {
  border-top: 1px solid var(--border);
  padding-top: 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.dg-history-head {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-2);
  display: flex;
  align-items: baseline;
  gap: 8px;
}
.dg-history-note {
  font-size: 10px;
  font-weight: 400;
  color: var(--text-3);
}
.dg-history-empty {
  font-size: 11px;
  color: var(--text-3);
  padding: 4px 0;
}
.dg-history-row {
  display: flex;
  align-items: center;
  gap: 6px;
  border-radius: 6px;
}
.dg-history-row:hover {
  background: rgba(255, 255, 255, 0.04);
}
.dg-history-row.active {
  background: rgba(255, 255, 255, 0.06);
}
.dg-history-open {
  flex: 1;
  text-align: left;
  background: none;
  border: none;
  padding: 3px 6px;
  font-size: 11px;
  color: var(--text-2);
  cursor: pointer;
  font-variant-numeric: tabular-nums;
}
.dg-history-open:hover {
  color: var(--accent);
}
.dg-history-del {
  background: none;
  border: none;
  color: var(--text-3);
  font-size: 11px;
  padding: 3px 8px;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s;
}
.dg-history-row:hover .dg-history-del {
  opacity: 1;
}
.dg-history-del:hover {
  color: #e06c6c;
}
.dg-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
</style>
