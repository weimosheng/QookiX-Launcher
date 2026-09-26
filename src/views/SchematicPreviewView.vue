<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useI18n } from "vue-i18n";
import { NPopover, NModal } from "naive-ui";
import { IconFolder, IconClock } from "../components/icons";
import {
  loadSchematicResources,
  type SchematicResources,
  type BlockState,
} from "../composables/useSchematicResources";
import {
  createSchematicRenderer,
  parseChunkData,
  type SchematicRenderer,
  type ViewMode,
} from "../composables/useSchematicRenderer";
import { createSchematicWorkerPool, type SchematicWorkerPool } from "../composables/useSchematicWorkerPool";

const { t } = useI18n();

interface PreviewManifest {
  sessionId: string;
  fileName: string;
  format: string;
  formatVersion: number;
  dataVersion: number | null;
  name: string | null;
  description: string | null;
  author: string | null;
  createdAt: number | null;
  modifiedAt: number | null;
  min: [number, number, number];
  max: [number, number, number];
  size: [number, number, number];
  blockCount: number;
  entityCount: number;
  blockEntityCount: number;
  palette: BlockState[];
  materials: { name: string; count: number }[];
  regions: {
    id: string;
    name: string;
    origin: [number, number, number];
    size: [number, number, number];
    min: [number, number, number];
    max: [number, number, number];
    blockCount: number;
    chunks: { position: [number, number, number]; nonAirBlocks: number }[];
  }[];
  warnings: string[];
}

const manifest = ref<PreviewManifest | null>(null);
const loading = ref(false);
const loadProgress = ref(0);
const loadTotal = ref(0);
const error = ref<string | null>(null);
const layerY = ref<number | null>(null);
const viewMode = ref<ViewMode>("orbit");
const walkLocked = ref(false);
const walkSpeed = ref(8);
const showMaterialsModal = ref(false);

const canvasRef = ref<HTMLCanvasElement | null>(null);
let renderer: SchematicRenderer | null = null;
let resources: SchematicResources | null = null;
let workerPool: SchematicWorkerPool | null = null;
let resizeObserver: ResizeObserver | null = null;

// —— 最近打开 ——
interface RecentFile {
  path: string;
  name: string;
  format: string;
  size: [number, number, number];
  blockCount: number;
  at: number;
}
const RECENT_KEY = "qookix.schematic.recent";
const recentFiles = ref<RecentFile[]>([]);

function loadRecent() {
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    const arr = raw ? JSON.parse(raw) : [];
    recentFiles.value = Array.isArray(arr) ? arr.slice(0, 20) : [];
  } catch {
    recentFiles.value = [];
  }
}

function pushRecent(m: PreviewManifest, path: string) {
  const item: RecentFile = {
    path,
    name: m.name ?? m.fileName,
    format: m.format,
    size: m.size,
    blockCount: m.blockCount,
    at: Date.now(),
  };
  recentFiles.value = [item, ...recentFiles.value.filter((r) => r.path !== path)].slice(0, 20);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(recentFiles.value));
  } catch { /* ignore */ }
}

function clearRecent() {
  recentFiles.value = [];
  try { localStorage.removeItem(RECENT_KEY); } catch { /* ignore */ }
}

async function openRecent(r: RecentFile) {
  await openSchematic({ kind: "external", path: r.path });
}

async function ensureResources() {
  if (resources) return resources;
  resources = await loadSchematicResources();
  return resources;
}

async function pickAndOpen() {
  const path = await openDialog({
    multiple: false,
    filters: [
      { name: "Schematics", extensions: ["litematic", "schem", "schematic"] },
    ],
  });
  if (!path || typeof path !== "string") return;
  await openSchematic({ kind: "external", path });
}

let currentPath: string | null = null;

async function openSchematic(source: { kind: string; path?: string; instanceId?: string; relativePath?: string }) {
  loading.value = true;
  error.value = null;
  manifest.value = null;
  loadProgress.value = 0;
  loadTotal.value = 0;
  currentPath = source.path ?? null;
  try {
    const m = await invoke<PreviewManifest>("schematic_preview_open", { source });
    manifest.value = m;

    const res = await ensureResources();

    await nextTick();
    if (!canvasRef.value) throw new Error("Canvas not ready");
    if (renderer) {
      renderer.clearChunks();
    } else {
      renderer = createSchematicRenderer(canvasRef.value);
      renderer.onLockChange((locked) => {
        walkLocked.value = locked;
      });
      renderer.onSpeedChange((speed) => { walkSpeed.value = speed; });
    }
    const wrapper = canvasRef.value.parentElement;
    if (wrapper && wrapper.clientWidth > 0 && wrapper.clientHeight > 0) {
      renderer.resize(wrapper.clientWidth, wrapper.clientHeight);
    }
    renderer.fitCamera(m.min, m.max);

    if (!workerPool) {
      const workerCount = Math.min(navigator.hardwareConcurrency ?? 4, 8);
      workerPool = createSchematicWorkerPool(workerCount);
      await workerPool.init(res.resourceBase);
    }

    const allChunks: { regionId: string; position: [number, number, number] }[] = [];
    for (const region of m.regions) {
      for (const chunk of region.chunks) {
        if (chunk.nonAirBlocks > 0) {
          allChunks.push({ regionId: region.id, position: chunk.position });
        }
      }
    }
    loadTotal.value = allChunks.length;

    renderer.requestRender();

    await Promise.all(
      allChunks.map(async (c) => {
        const buf = await invoke<ArrayBuffer>("schematic_preview_read_chunk", {
          sessionId: m.sessionId,
          regionId: c.regionId,
          position: c.position,
        });
        const blocks = parseChunkData(buf);
        const { opaque, translucent } = await workerPool!.buildMesh(c.position, blocks, m.palette);
        renderer!.setChunkFromMeshData(c.position, opaque, translucent, res.texture);
        renderer!.requestRender();
        loadProgress.value++;
      }),
    );

    if (currentPath) pushRecent(m, currentPath);
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

function toggleViewMode() {
  if (!renderer) return;
  const next: ViewMode = viewMode.value === "orbit" ? "walk" : "orbit";
  renderer.setViewMode(next);
  viewMode.value = next;
  renderer.requestRender();
}

function resetCamera() {
  renderer?.resetCamera();
}

function blockIconStyle(blockName: string): Record<string, string> | null {
  if (!resources) return null;
  const rect = resources.getBlockIconRect(blockName);
  if (!rect) return null;
  const [x, y, w, h] = rect;
  const size = 24;
  const sq = Math.min(w, h);
  const scale = size / sq;
  return {
    width: `${size}px`,
    height: `${size}px`,
    backgroundImage: `url(${resources.atlasUrl})`,
    backgroundPosition: `${-x * scale}px ${-y * scale}px`,
    backgroundSize: `${resources.atlasWidth * scale}px ${resources.atlasHeight * scale}px`,
    imageRendering: "pixelated",
    flex: "none",
  };
}

function setupResizeObserver() {
  if (!canvasRef.value) return;
  resizeObserver = new ResizeObserver((entries) => {
    for (const entry of entries) {
      const { width, height } = entry.contentRect;
      if (width > 0 && height > 0) {
        renderer?.resize(width, height);
        renderer?.requestRender();
      }
    }
  });
  resizeObserver.observe(canvasRef.value.parentElement ?? canvasRef.value);
}

watch(layerY, (y) => {
  renderer?.setLayerSlice(y);
  renderer?.requestRender();
});

onMounted(() => {
  loadRecent();
  setupResizeObserver();
});

onUnmounted(() => {
  resizeObserver?.disconnect();
  renderer?.dispose();
  workerPool?.dispose();
  workerPool = null;
  if (manifest.value) {
    invoke("schematic_preview_close", { sessionId: manifest.value.sessionId }).catch(() => {});
  }
});
</script>

<template>
  <div class="schematic-view">
    <!-- 参数栏 -->
    <div class="glass param-bar">
      <button class="mini-btn primary" @click="pickAndOpen" :disabled="loading">
        <IconFolder />{{ loading ? t("toolbox.schematicLoading") : t("toolbox.schematicOpen") }}
      </button>
      <NPopover trigger="click" placement="bottom-start" :width="320">
        <template #trigger>
          <button class="mini-btn" :disabled="!recentFiles.length">
            <IconClock />{{ t("toolbox.schematicRecent") }}({{ recentFiles.length }})
          </button>
        </template>
        <div class="recent-panel">
          <div class="recent-head">
            <span>{{ t("toolbox.schematicRecent") }}</span>
            <button class="link-btn" @click="clearRecent">{{ t("toolbox.schematicClearRecent") }}</button>
          </div>
          <div v-if="!recentFiles.length" class="muted">{{ t("toolbox.schematicNoRecent") }}</div>
          <button
            v-for="(r, i) in recentFiles"
            :key="i"
            class="recent-item"
            @click="openRecent(r)"
          >
            <span class="rf-name">{{ r.name }}</span>
            <span class="rf-meta">{{ r.format }} · {{ r.size[0] }}×{{ r.size[1] }}×{{ r.size[2] }} · {{ r.blockCount.toLocaleString() }}</span>
          </button>
        </div>
      </NPopover>
    </div>

    <div v-if="error" class="error glass">{{ error }}</div>

    <!-- 主预览区 -->
    <div v-if="manifest" class="map-wrap glass">
      <!-- 预览画布区 -->
      <div class="canvas-area">
        <canvas ref="canvasRef"></canvas>

        <!-- 漫游提示 -->
        <div v-if="viewMode === 'walk' && !walkLocked" class="walk-hint">
          {{ t("toolbox.schematicWalkHint") }}
        </div>
        <div v-if="viewMode === 'walk' && walkLocked" class="walk-status">
          {{ t("toolbox.schematicWalkSpeed") }}: {{ walkSpeed }}
        </div>

        <!-- 加载进度 -->
        <div v-if="loading && loadTotal > 0" class="map-progress">
          <div class="progress-head">
            <span>{{ t("toolbox.schematicLoading") }}</span>
            <span class="mono">{{ loadProgress }} / {{ loadTotal }}</span>
          </div>
          <div class="progress-track">
            <i :style="{ width: `${(loadProgress / loadTotal) * 100}%` }" />
          </div>
        </div>
      </div>

      <!-- 右侧控制面板（同级） -->
      <aside class="map-side">
        <h3 class="panel-title">{{ t("toolbox.schematicControls") }}</h3>
        <div class="side-row">
          <button class="mini-btn" @click="toggleViewMode" :disabled="loading">
            {{ viewMode === "orbit" ? t("toolbox.schematicWalkMode") : t("toolbox.schematicOrbitMode") }}
          </button>
          <button class="mini-btn" @click="resetCamera" :disabled="loading">
            {{ t("toolbox.schematicResetCamera") }}
          </button>
        </div>
        <label class="layer-control">
          <input
            type="range"
            :min="manifest.min[1]"
            :max="manifest.max[1]"
            :value="layerY ?? manifest.max[1]"
            @input="layerY = Number(($event.target as HTMLInputElement).value) === manifest.max[1] ? null : Number(($event.target as HTMLInputElement).value)"
          />
          <span class="tiny">{{ t("toolbox.schematicLayerSlice") }}: {{ layerY ?? "—" }}</span>
        </label>

        <div class="layer-sep" />

        <div class="info-section">
          <div class="info-item">
            <span class="label">{{ t("toolbox.schematicFileName") }}</span>
            <span class="value">{{ manifest.fileName }}</span>
          </div>
          <div class="info-item">
            <span class="label">{{ t("toolbox.schematicFormat") }}</span>
            <span class="value">{{ manifest.format }} v{{ manifest.formatVersion }}</span>
          </div>
          <div class="info-item">
            <span class="label">{{ t("toolbox.schematicSize") }}</span>
            <span class="value">{{ manifest.size[0] }} × {{ manifest.size[1] }} × {{ manifest.size[2] }}</span>
          </div>
          <div class="info-item">
            <span class="label">{{ t("toolbox.schematicBlockCount") }}</span>
            <span class="value">{{ manifest.blockCount.toLocaleString() }}</span>
          </div>
          <div class="info-item" v-if="manifest.name">
            <span class="label">{{ t("toolbox.schematicName") }}</span>
            <span class="value">{{ manifest.name }}</span>
          </div>
          <div class="info-item" v-if="manifest.author">
            <span class="label">{{ t("toolbox.schematicAuthor") }}</span>
            <span class="value">{{ manifest.author }}</span>
          </div>
        </div>

        <div v-if="manifest.materials.length" class="materials-section">
          <div class="layer-sep" />
          <button class="mini-btn materials-btn" @click="showMaterialsModal = true">
            {{ t("toolbox.schematicMaterials") }} ({{ manifest.materials.length }})
          </button>
        </div>
      </aside>
    </div>

    <!-- 材料统计模态框 -->
    <NModal
      v-model:show="showMaterialsModal"
      preset="card"
      :title="t('toolbox.schematicMaterials')"
      class="materials-modal"
      :bordered="false"
      size="huge"
    >
      <div class="materials-modal-list">
        <div v-for="m in manifest?.materials ?? []" :key="m.name" class="materials-modal-item">
          <div class="mat-icon" :style="blockIconStyle(m.name) ?? {}" v-if="blockIconStyle(m.name)"></div>
          <div class="mat-icon mat-icon-fallback" v-else></div>
          <span class="mat-name">{{ m.name.replace("minecraft:", "") }}</span>
          <span class="mat-count">{{ m.count.toLocaleString() }}</span>
        </div>
      </div>
    </NModal>

    <!-- 空状态：最近打开 -->
    <div v-if="!manifest && !loading && !error" class="empty-state glass">
      <div v-if="recentFiles.length" class="recent-grid">
        <h3>{{ t("toolbox.schematicRecent") }}</h3>
        <div class="recent-cards">
          <button
            v-for="(r, i) in recentFiles.slice(0, 8)"
            :key="i"
            class="recent-card glass clickable"
            @click="openRecent(r)"
          >
            <span class="rc-name">{{ r.name }}</span>
            <span class="rc-format">{{ r.format }}</span>
            <span class="rc-size">{{ r.size[0] }}×{{ r.size[1] }}×{{ r.size[2] }}</span>
            <span class="rc-blocks">{{ r.blockCount.toLocaleString() }} {{ t("toolbox.schematicBlocks") }}</span>
          </button>
        </div>
      </div>
      <div v-else class="empty-hint">
        <p>{{ t("toolbox.schematicEmpty") }}</p>
      </div>
    </div>

  </div>
</template>

<style scoped>
.schematic-view { display: flex; flex-direction: column; gap: 16px; height: 100%; min-height: 0; overflow: hidden; }

/* —— 参数栏 —— */
.param-bar { padding: 10px 14px; display: flex; gap: 8px; align-items: center; }

/* —— 通用按钮 —— */
.mini-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 13px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--w-04);
  color: var(--text-2);
  font-size: 12px;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s, color 0.15s;
}
.mini-btn:hover { background: var(--w-08); color: var(--text-1); }
.mini-btn:disabled { opacity: 0.5; cursor: default; }
.mini-btn.primary { border-color: var(--accent); color: var(--accent); }
.mini-btn.primary:hover { background: var(--accent-soft); }
.mini-btn svg { font-size: 14px; }
.link-btn { padding: 0; border: none; background: none; color: var(--text-3); font-size: 12px; cursor: pointer; }
.link-btn:hover { color: var(--accent); }

/* —— 最近打开面板 —— */
.recent-panel { display: flex; flex-direction: column; gap: 2px; max-height: 360px; overflow-y: auto; }
.recent-head { display: flex; align-items: center; justify-content: space-between; font-size: 12px; color: var(--text-3); padding-bottom: 6px; }
.recent-item { display: flex; flex-direction: column; gap: 2px; text-align: left; padding: 6px 8px; border: none; border-radius: 6px; background: transparent; cursor: pointer; color: var(--text-1); font: inherit; }
.recent-item:hover { background: var(--panel-hover); }
.rf-name { font-size: 13px; }
.rf-meta { font-size: 11px; color: var(--text-3); }

/* —— 主预览区（flex 行：画布 + 右侧面板） —— */
.map-wrap { flex: 1; display: flex; gap: 0; border-radius: 12px; overflow: hidden; min-height: 0; }
.canvas-area { flex: 1; position: relative; min-width: 0; overflow: hidden; }
.canvas-area canvas { width: 100%; height: 100%; display: block; }

/* —— 浮层面板共用 —— */
.map-progress,
.walk-hint,
.walk-status {
  border: 1px solid var(--border);
  background: var(--feat-solid);
  background: color-mix(in srgb, var(--feat-solid) 88%, transparent);
  backdrop-filter: blur(calc(var(--glass-blur, 8px) + 2px));
  -webkit-backdrop-filter: blur(calc(var(--glass-blur, 8px) + 2px));
}

/* —— 右侧控制+信息面板（同级） —— */
.map-side {
  flex: none;
  width: 248px;
  overflow-y: auto;
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 9px;
  border-left: 1px solid var(--border);
  background: var(--feat-solid);
  background: color-mix(in srgb, var(--feat-solid) 92%, transparent);
}
.panel-title { margin: 0 0 4px; font-size: 14px; font-weight: 600; color: var(--text-1); }
.side-row { display: flex; gap: 8px; }
.layer-control { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--text-3); }
.layer-control input { width: 100%; }
.tiny { font-size: 11px; }
.layer-sep { height: 1px; background: var(--border); margin: 2px 0; }
.info-section { display: flex; flex-direction: column; gap: 8px; }
.info-item { display: flex; flex-direction: column; gap: 2px; }
.label { font-size: 11px; color: var(--text-3); }
.value { font-size: 13px; font-weight: 500; }
.materials-section { display: flex; flex-direction: column; gap: 4px; }
.materials-btn { width: 100%; justify-content: center; }

/* —— 材料模态框 —— */
.materials-modal { max-width: 600px; }
.materials-modal-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 6px; max-height: 60vh; overflow-y: auto; }
.materials-modal-item { display: flex; align-items: center; gap: 8px; padding: 6px 10px; border-radius: 6px; background: var(--bg-2); }
.mat-icon { border-radius: 3px; flex: none; }
.mat-icon-fallback { width: 24px; height: 24px; background: var(--w-08); border-radius: 3px; }
.mat-name { color: var(--text-2); font-size: 12px; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.mat-count { color: var(--text-3); font-size: 11px; font-variant-numeric: tabular-nums; flex: none; }
.mono { font-variant-numeric: tabular-nums; }
.muted { color: var(--text-3); font-size: 12px; }

/* —— 漫游提示 —— */
.walk-hint { position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%); padding: 12px 20px; border-radius: 10px; color: rgba(255,255,255,0.85); font-size: 12px; text-align: center; pointer-events: none; }
.walk-status { position: absolute; top: 12px; right: 12px; padding: 4px 10px; border-radius: 6px; color: rgba(255,255,255,0.85); font-size: 11px; pointer-events: none; }

/* —— 加载进度 —— */
.map-progress { position: absolute; bottom: 0; left: 0; right: 0; padding: 10px 16px; }
.progress-head { display: flex; justify-content: space-between; font-size: 11px; color: var(--text-2); margin-bottom: 6px; }
.progress-track { height: 4px; border-radius: 2px; background: var(--w-08); overflow: hidden; }
.progress-track i { display: block; height: 100%; border-radius: 2px; background: var(--accent); transition: width 0.15s ease; }

/* —— 错误 —— */
.error { padding: 12px 16px; border-radius: 10px; color: var(--danger); font-size: 13px; }

/* —— 空状态 —— */
.empty-state { flex: 1; border-radius: 12px; padding: 24px; display: flex; flex-direction: column; gap: 16px; overflow-y: auto; }
.empty-hint { display: flex; align-items: center; justify-content: center; flex: 1; }
.empty-hint p { margin: 0; font-size: 14px; color: var(--text-3); }
.recent-grid h3 { margin: 0 0 12px; font-size: 15px; font-weight: 600; }
.recent-cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 12px; }
.recent-card { display: flex; flex-direction: column; gap: 4px; padding: 14px; border-radius: 10px; border: 1px solid var(--border); cursor: pointer; text-align: left; font: inherit; color: var(--text-1); transition: border-color 0.15s, background 0.15s; }
.recent-card:hover { border-color: var(--accent); background: var(--accent-soft); }
.rc-name { font-size: 14px; font-weight: 600; }
.rc-format { font-size: 11px; color: var(--accent); }
.rc-size { font-size: 12px; color: var(--text-2); }
.rc-blocks { font-size: 11px; color: var(--text-3); }
</style>
