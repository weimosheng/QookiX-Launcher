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
import { useI18n } from "vue-i18n";

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
const { t } = useI18n();
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
    message.success(t("diagnostics.deletedReport"));
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
    message.success(t("diagnostics.copiedToClipboard"));
  } catch (e) {
    message.error(t("diagnostics.copyFailed", { error: String(e) }));
  }
}

async function saveAs() {
  if (!report.value) return;
  const stamp = new Date().toISOString().slice(0, 19).replace(/[T:]/g, "-");
  const dest = await save({
    defaultPath: `qookix-diagnostics-${stamp}.md`,
    filters: [{ name: t("diagnostics.fileFilterName"), extensions: ["md", "txt"] }],
  });
  if (!dest) return;
  try {
    await api.saveDiagnosticsReport(dest as string, report.value.markdown);
    message.success(t("diagnostics.savedTo", { dest }));
  } catch (e) {
    message.error(String(e));
  }
}
</script>

<template>
  <n-modal
    v-model:show="show"
    preset="card"
    :title="t('diagnostics.title')"
    style="width: 720px; max-width: 95vw"
    :mask-closable="true"
    :close-on-esc="true"
  >
    <div class="dg-body">
      <div class="dg-head">
        <p class="dg-hint">
          {{ t("diagnostics.hint") }}
          <template v-if="props.instanceName">
            {{ t("diagnostics.currentInstance") }}<b>{{ props.instanceName }}</b>
          </template>
        </p>
        <div class="dg-modes">
          <button
            :class="{ active: previewMode === 'list' }"
            :disabled="!report?.sections.length"
            @click="previewMode = 'list'"
          >
            {{ t("diagnostics.modeList") }}
          </button>
          <button :class="{ active: previewMode === 'markdown' }" @click="previewMode = 'markdown'">{{ t("diagnostics.modeMarkdown") }}</button>
        </div>
      </div>

      <div v-if="collecting" class="dg-loading">
        {{ t("diagnostics.collecting") }}
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
                <summary>{{ t("diagnostics.viewDetails") }}</summary>
                <pre>{{ s.block }}</pre>
              </details>
            </section>
          </div>
          <pre v-else class="dg-markdown">{{ report.markdown }}</pre>
        </n-scrollbar>

        <p class="dg-meta">
          <template v-if="viewingHistory">{{ t("diagnostics.viewingHistory", { filename: viewingHistory }) }}</template>
          <template v-else>
            {{ t("diagnostics.redactedSummary", { count: report.redactions }) }}
            <span v-if="report.sections.length">{{ t("diagnostics.sectionCount", { count: report.sections.length }) }}</span>
          </template>
        </p>

        <div class="dg-history">
          <div class="dg-history-head">
            {{ t("diagnostics.historyTitle") }}
            <span class="dg-history-note">{{ t("diagnostics.historyNote") }}</span>
          </div>
          <div v-if="!history.length" class="dg-history-empty">{{ t("diagnostics.historyEmpty") }}</div>
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
              <button class="dg-history-del" :title="t('diagnostics.deleteTitle')" @click="removeHistory(h)">✕</button>
            </div>
          </n-scrollbar>
        </div>
      </template>

      <div class="dg-actions">
        <n-button :loading="collecting" @click="collect">{{ t("diagnostics.recollect") }}</n-button>
        <n-button :disabled="!report || collecting" @click="copyAll">{{ t("diagnostics.copyReport") }}</n-button>
        <n-button type="primary" :disabled="!report || collecting" @click="saveAs">{{ t("diagnostics.saveAsFile") }}</n-button>
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
  background: var(--panel-sub, var(--w-02));
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
  background: var(--k-22);
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
  background: var(--w-04);
}
.dg-history-row.active {
  background: var(--w-06);
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
