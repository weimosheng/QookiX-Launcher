<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, computed } from "vue";
import { useRouter } from "vue-router";
import { listen } from "@tauri-apps/api/event";
import { NButton, NModal } from "naive-ui";
import { useI18n } from "vue-i18n";
import DiagnosticsDialog from "./DiagnosticsDialog.vue";

interface CrashInfo {
  instanceId: string;
  exit_code: number | null;
  crash_report: string | null;
  severity: string;
  title: string;
  reason: string;
  advice: string;
  excerpt: string;
  affected_mods?: string[];
}

const show = ref(false);
const showDiag = ref(false);
const info = ref<CrashInfo | null>(null);
let unlisten: (() => void) | null = null;
const router = useRouter();
const { t } = useI18n();

// 「查看日志」应跳到真正的游戏日志页（launch://log 流 + 持久化日志），
// 而不是崩溃分析页；崩溃分析需要 crash-reports/*.txt 文件，很多错误（如
// 模组加载失败、OOM）根本不会生成该文件，跳过去只会看到「暂无崩溃报告」。
function openLogs() {
  if (info.value?.instanceId) {
    show.value = false;
    router.push(`/instance/${info.value.instanceId}?tab=logs`);
  }
}

// 仅当本次确实检测到崩溃报告文件时才提供「崩溃分析」入口
function openCrash() {
  if (info.value?.instanceId) {
    show.value = false;
    router.push(`/instance/${info.value.instanceId}?tab=crash`);
  }
}

const sevMeta = computed(() => {
  const META: Record<string, { label: string; cls: string }> = {
    jvm: { label: t("crashDialog.severity.jvm"), cls: "sev-jvm" },
    oom: { label: t("crashDialog.severity.oom"), cls: "sev-oom" },
    gl: { label: t("crashDialog.severity.gl"), cls: "sev-gl" },
    java_ver: { label: t("crashDialog.severity.javaVer"), cls: "sev-java" },
    lwjgl: { label: t("crashDialog.severity.lwjgl"), cls: "sev-lwjgl" },
    mod: { label: t("crashDialog.severity.mod"), cls: "sev-mod" },
    unknown: { label: t("crashDialog.severity.unknown"), cls: "sev-unknown" },
  };
  return META[info.value?.severity ?? "unknown"] ?? META.unknown;
});

onMounted(async () => {
  unlisten = await listen<CrashInfo>("launch://crash", (e) => {
    info.value = e.payload;
    show.value = true;
  });
});

onBeforeUnmount(() => {
  unlisten?.();
});
</script>

<template>
  <NModal
    :show="show"
    preset="card"
    :style="{ width: 'min(560px, 92vw)' }"
    :closable="true"
    @update:show="(v: boolean) => (show = v)"
  >
    <template #header>
      <div class="crash-header">
        <span class="crash-badge" :class="sevMeta.cls">{{ sevMeta.label }}</span>
        <span class="crash-title">{{ info?.title ?? t("crashDialog.defaultTitle") }}</span>
      </div>
    </template>

    <div v-if="info" class="crash-body">
      <p class="crash-reason">{{ info.reason }}</p>
      <div v-if="info.affected_mods && info.affected_mods.length" class="crash-mods">
        <span class="mods-label">{{ t("crashDialog.relatedMods") }}</span>
        <div class="mods-list">
          <span v-for="m in info.affected_mods" :key="m" class="mod-chip">{{ m }}</span>
        </div>
      </div>
      <div v-if="info.exit_code !== null && info.exit_code !== undefined" class="crash-code">
        {{ t("crashDialog.exitCodeLabel") }}<code>{{ info.exit_code }}</code>
      </div>
      <p class="crash-advice">{{ info.advice }}</p>
      <div v-if="info.excerpt" class="crash-excerpt">
        <div class="excerpt-head">{{ t("crashDialog.excerptHead") }}</div>
        <pre>{{ info.excerpt }}</pre>
      </div>
    </div>

    <template #footer>
      <div class="crash-footer">
        <span v-if="info?.crash_report" class="crash-path">{{ info.crash_report }}</span>
        <div class="footer-btns">
          <NButton @click="openLogs">{{ t("crashDialog.viewLogs") }}</NButton>
          <NButton v-if="info?.crash_report" @click="openCrash">{{ t("crashDialog.crashAnalysis") }}</NButton>
          <NButton @click="showDiag = true">{{ t("crashDialog.diagnosticsReport") }}</NButton>
          <NButton type="primary" @click="show = false">{{ t("crashDialog.gotIt") }}</NButton>
        </div>
      </div>
    </template>
  </NModal>

  <DiagnosticsDialog v-model:show="showDiag" :instance-id="info?.instanceId ?? null" />
</template>

<style scoped>
.crash-header {
  display: flex;
  align-items: center;
  gap: 10px;
}
.crash-badge {
  font-size: 12px;
  font-weight: 600;
  padding: 2px 10px;
  border-radius: 999px;
  white-space: nowrap;
}
.crash-title {
  font-size: 15px;
  font-weight: 700;
  color: var(--text-1, #e8eaf2);
}
.sev-jvm { background: rgba(224, 72, 72, 0.16); color: #f0716f; }
.sev-oom { background: rgba(230, 162, 60, 0.16); color: #f0b46a; }
.sev-gl  { background: rgba(120, 140, 220, 0.16); color: #9fafe8; }
.sev-java{ background: rgba(96, 180, 160, 0.16); color: #6dcfae; }
.sev-lwjgl { background: rgba(224, 72, 72, 0.16); color: #f0716f; }
.sev-mod { background: rgba(230, 162, 60, 0.16); color: #f0b46a; }
.sev-unknown { background: rgba(128, 132, 150, 0.16); color: #aeb2c4; }

.crash-body { display: flex; flex-direction: column; gap: 10px; }
.crash-reason {
  margin: 0;
  font-size: 13.5px;
  color: var(--text-2, #c6c8d2);
}
.crash-mods {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}
.mods-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-3, #8b8e9c);
  line-height: 24px;
  flex-shrink: 0;
}
.mods-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.mod-chip {
  font-size: 12px;
  font-weight: 600;
  padding: 3px 10px;
  border-radius: 999px;
  background: rgba(150, 181, 225, 0.14);
  border: 1px solid rgba(150, 181, 225, 0.35);
  color: var(--accent-light, #a8c4ea);
}
.crash-code {
  font-size: 12px;
  color: var(--text-3, #8b8e9c);
}
.crash-advice {
  margin: 0;
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--panel-2, var(--w-04));
  border-left: 3px solid var(--accent, #e89a4b);
  font-size: 13px;
  color: var(--text-main, #e6e8f0);
  line-height: 1.6;
}
.crash-excerpt {
  border: 1px solid var(--border, var(--w-08));
  border-radius: 8px;
  overflow: hidden;
}
.excerpt-head {
  font-size: 12px;
  font-weight: 600;
  padding: 6px 12px;
  color: var(--text-3, #8b8e9c);
  background: var(--w-03);
  border-bottom: 1px solid var(--border, var(--w-08));
}
.crash-excerpt pre {
  margin: 0;
  padding: 10px 12px;
  max-height: 180px;
  overflow: auto;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-2, #c6c8d2);
  white-space: pre-wrap;
  word-break: break-all;
}
.crash-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}
.footer-btns {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.crash-path {
  font-size: 11px;
  color: var(--text-3, #8b8e9c);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>