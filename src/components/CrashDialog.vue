<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, computed } from "vue";
import { useRouter } from "vue-router";
import { listen } from "@tauri-apps/api/event";
import { NButton, NModal } from "naive-ui";

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
const info = ref<CrashInfo | null>(null);
let unlisten: (() => void) | null = null;
const router = useRouter();

function openLogs() {
  if (info.value?.instanceId) {
    show.value = false;
    router.push(`/instance/${info.value.instanceId}?tab=crash`);
  }
}

const SEV_META: Record<string, { label: string; cls: string }> = {
  jvm: { label: "JVM 崩溃", cls: "sev-jvm" },
  oom: { label: "内存不足", cls: "sev-oom" },
  gl: { label: "显卡问题", cls: "sev-gl" },
  java_ver: { label: "Java 版本", cls: "sev-java" },
  lwjgl: { label: "依赖缺失", cls: "sev-lwjgl" },
  mod: { label: "模组冲突", cls: "sev-mod" },
  unknown: { label: "未知原因", cls: "sev-unknown" },
};

const sevMeta = computed(
  () => SEV_META[info.value?.severity ?? "unknown"] ?? SEV_META.unknown
);

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
        <span class="crash-title">{{ info?.title ?? "游戏崩溃" }}</span>
      </div>
    </template>

    <div v-if="info" class="crash-body">
      <p class="crash-reason">{{ info.reason }}</p>
      <div v-if="info.affected_mods && info.affected_mods.length" class="crash-mods">
        <span class="mods-label">相关模组</span>
        <div class="mods-list">
          <span v-for="m in info.affected_mods" :key="m" class="mod-chip">{{ m }}</span>
        </div>
      </div>
      <div v-if="info.exit_code !== null && info.exit_code !== undefined" class="crash-code">
        进程退出码：<code>{{ info.exit_code }}</code>
      </div>
      <p class="crash-advice">{{ info.advice }}</p>
      <div v-if="info.excerpt" class="crash-excerpt">
        <div class="excerpt-head">崩溃报告摘录</div>
        <pre>{{ info.excerpt }}</pre>
      </div>
    </div>

    <template #footer>
      <div class="crash-footer">
        <span v-if="info?.crash_report" class="crash-path">{{ info.crash_report }}</span>
        <div class="footer-btns">
          <NButton @click="openLogs">查看日志</NButton>
          <NButton type="primary" @click="show = false">知道了</NButton>
        </div>
      </div>
    </template>
  </NModal>
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
  background: var(--panel-2, rgba(255, 255, 255, 0.04));
  border-left: 3px solid var(--accent, #e89a4b);
  font-size: 13px;
  color: var(--text-main, #e6e8f0);
  line-height: 1.6;
}
.crash-excerpt {
  border: 1px solid var(--border, rgba(255, 255, 255, 0.08));
  border-radius: 8px;
  overflow: hidden;
}
.excerpt-head {
  font-size: 12px;
  font-weight: 600;
  padding: 6px 12px;
  color: var(--text-3, #8b8e9c);
  background: rgba(255, 255, 255, 0.03);
  border-bottom: 1px solid var(--border, rgba(255, 255, 255, 0.08));
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