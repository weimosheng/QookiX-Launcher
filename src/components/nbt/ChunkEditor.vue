<script setup lang="ts">
/**
 * 区块编辑：地图（点一下选区块）或按坐标列表选区块，
 * 选中后复用 NbtTreeEditor 改区块 NBT。
 * 区块数据在 `.mca`（region 文件）里，写盘前会自动备份该 region 文件。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { NSelect, useMessage } from "naive-ui";
import { api } from "../../api";
import type { ChunkEntry } from "../../types";
import { IconRefresh } from "../icons";
import ChunkMap from "./ChunkMap.vue";
import NbtTreeEditor from "./NbtTreeEditor.vue";

/** 改完区块要让地图上的那张旧图作废 */
const mapRef = ref<{ refresh: () => void } | null>(null);

function onChunkSaved(name: string) {
  emit("backed-up", name);
  mapRef.value?.refresh();
}

const props = defineProps<{
  instanceId: string;
  world: string;
  running: boolean;
  readonly?: boolean;
}>();
const emit = defineEmits<{ (e: "backed-up", name: string): void }>();

const { t } = useI18n();
const message = useMessage();

const dim = ref("overworld");
const view = ref<"map" | "list">("map");
const chunks = ref<ChunkEntry[]>([]);
const loading = ref(false);
const selected = ref<{ cx: number; cz: number } | null>(null);

const dimOptions = computed(() => [
  { label: t("nbt.dimOverworld"), value: "overworld" },
  { label: t("nbt.dimNether"), value: "nether" },
  { label: t("nbt.dimEnd"), value: "end" },
]);
const chunkOptions = computed(() =>
  chunks.value.map((c) => ({ label: `${c.cx}, ${c.cz}`, value: `${c.cx},${c.cz}` }))
);
const chunkValue = computed(() =>
  selected.value ? `${selected.value.cx},${selected.value.cz}` : null
);

function pickChunk(v: string | null) {
  if (!v) {
    selected.value = null;
    return;
  }
  const [cx, cz] = v.split(",").map(Number);
  selected.value = { cx, cz };
}

async function loadChunks() {
  loading.value = true;
  selected.value = null;
  try {
    const r = await api.nbtListChunks(props.instanceId, props.world, dim.value);
    chunks.value = r.chunks;
  } catch (e) {
    message.error(String(e));
    chunks.value = [];
  } finally {
    loading.value = false;
  }
}

// 切维度要重新清点；地图模式下也顺便知道区块总数
watch(dim, loadChunks, { immediate: true });
</script>

<template>
  <div class="chunk-editor">
    <div class="bar">
      <span class="lbl">{{ t("nbt.dimension") }}</span>
      <NSelect v-model:value="dim" :options="dimOptions" size="small" style="width: 132px" />

      <div class="seg-group">
        <button class="seg" :class="{ active: view === 'map' }" @click="view = 'map'">
          {{ t("nbt.mapView") }}
        </button>
        <button class="seg" :class="{ active: view === 'list' }" @click="view = 'list'">
          {{ t("nbt.listView") }}
        </button>
      </div>

      <template v-if="view === 'list'">
        <NSelect
          :value="chunkValue"
          :options="chunkOptions"
          size="small"
          filterable
          clearable
          virtual-scroll
          :placeholder="loading ? t('nbt.loading') : t('nbt.chunkPickHint')"
          style="min-width: 190px"
          @update:value="pickChunk"
        />
        <button class="mini-btn" :disabled="loading" @click="loadChunks">
          <IconRefresh /> {{ t("nbt.refresh") }}
        </button>
      </template>
      <span class="count">{{ t("nbt.chunkCount", { n: chunks.length }) }}</span>
    </div>

    <ChunkMap
      v-if="view === 'map'"
      ref="mapRef"
      :instance-id="instanceId"
      :world="world"
      :dim="dim"
      :selected="selected"
      @pick="(cx: number, cz: number) => (selected = { cx, cz })"
    />
    <div v-if="view === 'list' && !loading && !chunks.length" class="empty">
      {{ t("nbt.chunkEmpty") }}
    </div>

    <NbtTreeEditor
      v-if="selected"
      :key="`${dim}:${selected.cx}:${selected.cz}`"
      :instance-id="instanceId"
      :world="world"
      :running="running"
      :readonly="readonly"
      :chunk="{ dim, cx: selected.cx, cz: selected.cz }"
      @backed-up="onChunkSaved"
    />
    <div v-else-if="view === 'map'" class="hint">{{ t("nbt.mapHint") }}</div>
  </div>
</template>

<style scoped>
.chunk-editor {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.bar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.lbl {
  font-size: 12px;
  color: var(--text-3);
}
.count {
  font-size: 11px;
  color: var(--text-3);
}
.seg-group {
  display: flex;
  gap: 2px;
  padding: 2px;
  border-radius: 9px;
  background: var(--w-05);
}
.seg {
  padding: 4px 12px;
  border: none;
  border-radius: 7px;
  background: none;
  color: var(--text-3);
  font-size: 12px;
  font-family: inherit;
  cursor: pointer;
}
.seg.active {
  background: var(--accent-soft);
  color: var(--accent);
}
.mini-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
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
.mini-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.empty,
.hint {
  padding: 24px;
  text-align: center;
  color: var(--text-3);
  font-size: 12px;
}
.hint {
  padding: 8px;
}
</style>
