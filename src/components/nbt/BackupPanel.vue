<script setup lang="ts">
/**
 * 存档备份面板：列出当前世界的单文件备份（level.dat / 玩家数据），
 * 可以恢复或删除。备份是每次修改前自动生成的，这里是唯一需要手动操作的地方。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { NButton, NModal, useDialog, useMessage } from "naive-ui";
import { api } from "../../api";
import { fmtDate, fmtSize } from "../../utils/format";
import type { NbtBackupInfo } from "../../types";
import { IconRefresh, IconTrash } from "../icons";

const props = defineProps<{
  show: boolean;
  instanceId: string;
  world: string;
}>();
const emit = defineEmits<{
  (e: "update:show", v: boolean): void;
  (e: "restored"): void;
}>();

const { t } = useI18n();
const message = useMessage();
const dialog = useDialog();

const backups = ref<NbtBackupInfo[]>([]);
const loading = ref(false);
const busy = ref("");

/** 按文件分组：level.dat 归一组，每个玩家数据各一组 */
const groups = computed(() => {
  const map = new Map<string, NbtBackupInfo[]>();
  for (const b of backups.value) {
    const list = map.get(b.file) ?? [];
    list.push(b);
    map.set(b.file, list);
  }
  return [...map.entries()].map(([file, items]) => ({
    file,
    label: file === "level.dat" ? t("nbt.backupLevel") : file.replace(/^playerdata\//, "").replace(/\.dat$/, ""),
    items,
  }));
});

async function load() {
  if (!props.world) return;
  loading.value = true;
  try {
    const r = await api.nbtListBackups(props.instanceId, props.world);
    backups.value = r.backups;
  } catch (e) {
    message.error(String(e));
    backups.value = [];
  } finally {
    loading.value = false;
  }
}

function restore(item: NbtBackupInfo) {
  dialog.warning({
    title: t("nbt.backupRestoreTitle"),
    content: `${item.name}\n${t("nbt.backupRestoreConfirm")}`,
    positiveText: t("nbt.backupRestore"),
    negativeText: t("nbt.cancel"),
    onPositiveClick: async () => {
      busy.value = item.name;
      try {
        const r = await api.nbtRestoreBackup(props.instanceId, props.world, item.file, item.name);
        message.success(r.backup ? `${t("nbt.backupRestored")}（${t("nbt.backupSafety")}：${r.backup}）` : t("nbt.backupRestored"));
        emit("restored");
        await load();
      } catch (e) {
        message.error(String(e));
      } finally {
        busy.value = "";
      }
    },
  });
}

function remove(item: NbtBackupInfo) {
  dialog.warning({
    title: t("nbt.backupDeleteTitle"),
    content: `${item.name}\n${t("nbt.backupDeleteConfirm")}`,
    positiveText: t("nbt.delete"),
    negativeText: t("nbt.cancel"),
    onPositiveClick: async () => {
      busy.value = item.name;
      try {
        await api.nbtDeleteBackup(props.instanceId, props.world, item.file, item.name);
        message.success(t("nbt.backupDeleted"));
        await load();
      } catch (e) {
        message.error(String(e));
      } finally {
        busy.value = "";
      }
    },
  });
}

watch(
  () => props.show,
  (v) => {
    if (v) load();
  }
);
</script>

<template>
  <NModal
    :show="show"
    preset="card"
    :title="t('nbt.backupTitle')"
    style="max-width: 620px"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <div class="bp">
      <div class="bp-bar">
        <span class="bp-hint">{{ t("nbt.backupHint") }}</span>
        <button class="mini-btn" :disabled="loading" @click="load">
          <IconRefresh /> {{ loading ? t("nbt.loading") : t("nbt.refresh") }}
        </button>
      </div>

      <div v-if="loading" class="bp-empty">{{ t("nbt.readingBackups") }}</div>
      <div v-else-if="!backups.length" class="bp-empty">{{ t("nbt.backupEmpty") }}</div>
      <div v-else class="bp-list">
        <div v-for="g in groups" :key="g.file" class="bp-group">
          <div class="bp-group-title">{{ g.label }}</div>
          <div v-for="b in g.items" :key="b.name" class="bp-item">
            <div class="bp-meta">
              <span class="bp-time">{{ fmtDate(b.modified) }}</span>
              <span class="bp-size">{{ fmtSize(b.size) }}</span>
              <span class="bp-name">{{ b.name }}</span>
            </div>
            <div class="bp-actions">
              <NButton size="tiny" :loading="busy === b.name" @click="restore(b)">
                {{ t("nbt.backupRestore") }}
              </NButton>
              <button class="mini-btn danger" :disabled="busy === b.name" @click="remove(b)">
                <IconTrash />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </NModal>
</template>

<style scoped>
.bp {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.bp-bar {
  display: flex;
  align-items: center;
  gap: 10px;
}
.bp-hint {
  flex: 1;
  font-size: 12px;
  color: var(--text-3);
}
.bp-list {
  max-height: 46vh;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.bp-group-title {
  font-size: 12px;
  font-weight: 700;
  color: var(--text-2);
  margin-bottom: 6px;
}
.bp-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 8px;
  border-radius: 8px;
  background: var(--w-04);
  margin-bottom: 4px;
}
.bp-meta {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: baseline;
  gap: 8px;
  flex-wrap: wrap;
}
.bp-time {
  font-size: 12px;
  font-weight: 600;
}
.bp-size {
  font-size: 11px;
  color: var(--text-3);
}
.bp-name {
  font-size: 10px;
  color: var(--text-3);
  font-family: ui-monospace, Consolas, monospace;
  word-break: break-all;
}
.bp-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.bp-empty {
  padding: 32px;
  text-align: center;
  color: var(--text-3);
  font-size: 12px;
}
.mini-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 9px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--w-04);
  color: var(--text-2);
  font-size: 12px;
  font-family: inherit;
  cursor: pointer;
}
.mini-btn:hover {
  background: var(--w-08);
}
.mini-btn.danger {
  color: #e5534b;
}
.mini-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
