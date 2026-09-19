<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { NCheckbox, useMessage } from "naive-ui";
import { save } from "@tauri-apps/plugin-dialog";
import { useTasksStore } from "../stores/tasks";
import { api } from "../api";
import { IconClose, IconCopy, IconDownload } from "./icons";
import { useI18n } from "vue-i18n";

const props = defineProps<{ instanceId: string }>();
const tasks = useTasksStore();
const message = useMessage();
const { t } = useI18n();
const box = ref<HTMLDivElement | null>(null);
const autoScroll = ref(true);

const logs = computed(() => tasks.logs[props.instanceId] ?? []);

const logText = computed(() => logs.value.map((l) => l.line).join("\n"));

watch(() => logs.value.length, async () => {
  if (autoScroll.value) {
    await nextTick();
    if (box.value) box.value.scrollTop = box.value.scrollHeight;
  }
});

function clear() {
  tasks.clearLogs(props.instanceId);
}

function onScroll() {
  if (!box.value) return;
  const el = box.value;
  autoScroll.value = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
}

async function copyAll() {
  if (!logText.value) {
    message.info(t("logViewer.noLogContent"));
    return;
  }
  try {
    await navigator.clipboard.writeText(logText.value);
    message.success(t("logViewer.copiedAll"));
  } catch {
    // fallback for restricted contexts
    const ta = document.createElement("textarea");
    ta.value = logText.value;
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    if (ok) message.success(t("logViewer.copiedAll"));
    else message.error(t("logViewer.copyFailed"));
  }
}

async function exportLog() {
  if (!logText.value) {
    message.info(t("logViewer.noLogContent"));
    return;
  }
  const ts = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const defaultName = `${props.instanceId}-${ts.getFullYear()}${pad(ts.getMonth() + 1)}${pad(ts.getDate())}-${pad(ts.getHours())}${pad(ts.getMinutes())}${pad(ts.getSeconds())}.log`;
  const path = await save({
    defaultPath: defaultName,
    filters: [{ name: t("logViewer.logFile"), extensions: ["log", "txt"] }],
  });
  if (!path) return;
  try {
    await api.saveTextFile(path as string, logText.value);
    message.success(t("logViewer.exportedTo", { path }));
  } catch (e) {
    message.error(String(e));
  }
}
</script>

<template>
  <div class="log-panel glass">
    <div class="log-toolbar">
      <span class="log-title">{{ t("logViewer.title") }}</span>
      <div class="log-actions">
        <n-checkbox v-model:checked="autoScroll" size="small" class="auto">
          {{ t("logViewer.autoScroll") }}
        </n-checkbox>
        <button class="mini" :title="t('logViewer.copyAllLog')" @click="copyAll">
          <IconCopy /> {{ t("logViewer.copy") }}
        </button>
        <button class="mini" :title="t('logViewer.exportLogFile')" @click="exportLog">
          <IconDownload /> {{ t("logViewer.export") }}
        </button>
        <button class="mini" :title="t('logViewer.clearLog')" @click="clear">
          <IconClose /> {{ t("logViewer.clear") }}
        </button>
      </div>
    </div>
    <div ref="box" class="log-box mono" @scroll="onScroll">
      <div v-if="!logs.length" class="log-empty">
        {{ tasks.runningInstance === instanceId ? t("logViewer.gameStarting") : t("logViewer.empty") }}
      </div>
      <div
        v-for="(l, i) in logs"
        :key="i"
        class="log-line"
        :class="l.stream"
      >{{ l.line }}</div>
    </div>
    <div v-if="tasks.lastExit && tasks.lastExit.instanceId === instanceId && !tasks.gameRunning" class="exit-info">
      {{ t("logViewer.gameExited", { code: tasks.lastExit.code ?? t("logViewer.unknown") }) }}
    </div>
  </div>
</template>

<style scoped>
.log-panel {
  display: flex;
  flex-direction: column;
  height: calc(100vh - 320px);
  min-height: 320px;
  overflow: hidden;
}
.log-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
}
.log-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
}
.log-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.auto {
  font-size: 12px;
  color: var(--text-3);
  margin-right: 6px;
}
.auto :deep(.n-checkbox__label) {
  font-size: 12px;
  color: var(--text-3);
}
.mini {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-2);
  border-radius: 7px;
  padding: 4px 10px;
  font-size: 12px;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.mini:hover {
  color: var(--text-1);
  background: var(--w-06);
}
.log-box {
  flex: 1;
  overflow-y: auto;
  padding: 12px 14px;
  font-size: 12px;
  line-height: 1.55;
  background: var(--k-25);
  /* allow selecting / copying log text */
  user-select: text;
  -webkit-user-select: text;
  cursor: text;
}
.log-line.err {
  color: #f0907f;
}
.log-line.out {
  color: #c9ccd6;
}
.log-empty {
  color: var(--text-3);
  padding: 20px 0;
  text-align: center;
}
.exit-info {
  padding: 8px 14px;
  font-size: 12px;
  color: var(--text-3);
  border-top: 1px solid var(--border);
}
</style>
