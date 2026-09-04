<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { fmtBytes } from "../utils/format";
import { isAprilFools } from "../utils/versions";
import { useRouter } from "vue-router";
import { NSelect, NInput, NModal, useMessage } from "naive-ui";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import { useInstancesStore } from "../stores/instances";
import AppIcon from "../components/AppIcon.vue";
import IconPickerDialog from "../components/IconPickerDialog.vue";
import { IconChevronLeft, IconFolder, IconPlus } from "../components/icons";
import type { Loader } from "../types";

const router = useRouter();
const instances = useInstancesStore();
const message = useMessage();

const mode = ref<"fresh" | "import" | "importmc">("fresh");

// ---- fresh create ----
const name = ref("");
const iconStr = ref("");
const showIconPicker = ref(false);
const mcVersion = ref("");
const loader = ref<Loader>("vanilla");
const loaderVersion = ref<string | null>(null);

// 新实例要加入的分组（null = 未分组）
const newGroup = ref<string | null>(null);
const groupOptions = computed(() =>
  instances.groups.map((g) => ({ label: g.name, value: g.id }))
);

const versionCat = ref<string>("release");
const versions = ref<{ id: string; type: string; releaseTime: string }[]>([]);
const loaderVersions = ref<string[]>([]);
const loadingLoader = ref(false);
const creating = ref(false);
const importing = ref(false);

const loaders: { value: Loader; label: string }[] = [
  { value: "vanilla", label: "None" },
  { value: "fabric", label: "Fabric" },
  { value: "neoforge", label: "NeoForge" },
  { value: "forge", label: "Forge" },
  { value: "quilt", label: "Quilt" },
];

const filteredVersions = computed(() => {
  return versions.value.filter((v) => {
    if (versionCat.value === "release") return v.type === "release" || v.type.startsWith("old_");
    if (versionCat.value === "april") return isAprilFools(v);
    return v.type === "snapshot" && !isAprilFools(v);
  });
});

watch([mcVersion, loader], async ([mc, ld]) => {
  // importmc mode auto-detects the loader per version; nothing to fetch here
  if (mode.value === "importmc") return;
  loaderVersion.value = null;
  if (!mc || ld === "vanilla") {
    loaderVersions.value = [];
    return;
  }
  loadingLoader.value = true;
  try {
    loaderVersions.value = await api.getLoaderVersions(ld, mc);
  } catch {
    loaderVersions.value = [];
  } finally {
    loadingLoader.value = false;
  }
});

const loaderOptions = computed(() => [
  { label: "最新稳定版", value: "" },
  ...loaderVersions.value.slice(0, 30).map((v) => ({ label: v, value: v })),
]);

async function create() {
  if (!mcVersion.value) return message.warning("请选择游戏版本");
  const instName = name.value.trim() || mcVersion.value;
  creating.value = true;
  try {
    const inst = await instances.create(
      instName,
      mcVersion.value,
      loader.value,
      loaderVersion.value || null
    );
    if (iconStr.value) {
      await instances.patch({ id: inst.id, icon: iconStr.value });
    }
    if (newGroup.value) {
      await instances.moveToGroup(inst.id, newGroup.value);
    }
    message.success("实例已创建，正在安装游戏文件…");
    router.push(`/instance/${inst.id}`);
    instances.installGame(inst.id).catch((e) => {
      message.error(`安装失败: ${String(e)}`);
    });
  } catch (e) {
    message.error(String(e));
  } finally {
    creating.value = false;
  }
}

// ---- import ----
async function importPack() {
  const file = await open({
    multiple: false,
    filters: [{ name: "整合包", extensions: ["zip", "mrpack"] }],
  });
  if (!file) return;
  importing.value = true;
  try {
    const inst = await api.importModpack(file as string);
    message.success(`已导入「${inst.name}」，接下来安装游戏`);
    router.push(`/instance/${inst.id}`);
  } catch (e) {
    message.error(String(e));
  } finally {
    importing.value = false;
  }
}

// ---- import existing .minecraft folder ----
const importSrc = ref("");
// base instance name is derived from the selected folder (no manual entry needed)
const importMcBaseName = computed(() =>
  importSrc.value ? pathBasename(importSrc.value) : ""
);
const migrateMode = ref<"copy" | "symlink">("copy");
const scanning = ref(false);
const showVersionDialog = ref(false);
const importingMc = ref(false);
const importStep = ref(1);
const calcSize = ref(false);
// live migration progress (one entry per selected version)
const importProgress = ref<{ current: number; total: number; name: string; phase: string; done: boolean } | null>(null);
const mcVersions = ref<{ id: string; raw_id: string; inherits_base: boolean; loader: string; loader_version: string | null; size_bytes: number }[]>([]);

// Group detected versions by loader, sorted by loader then by id, so the long
// list stays readable instead of one undifferentiated block.
const loaderOrder = ["fabric", "forge", "neoforge", "quilt", "optifine", "vanilla"];
const groupedVersions = computed(() => {
  const map = new Map<string, typeof mcVersions.value>();
  for (const v of mcVersions.value) {
    if (!map.has(v.loader)) map.set(v.loader, []);
    map.get(v.loader)!.push(v);
  }
  const groups: { loader: string; items: typeof mcVersions.value }[] = [];
  for (const key of loaderOrder) {
    if (map.has(key)) {
      const items = map.get(key)!;
      items.sort((a, b) => a.id.localeCompare(b.id, undefined, { numeric: true }));
      groups.push({ loader: key, items });
      map.delete(key);
    }
  }
  // any loader not in the known order (shouldn't happen) goes at the end
  for (const [key, items] of map) {
    items.sort((a, b) => a.id.localeCompare(b.id, undefined, { numeric: true }));
    groups.push({ loader: key, items });
  }
  return groups;
});
// multi-select: which detected versions to import (one instance each)
const selectedVersions = ref<string[]>([]);
// live statistics streamed from the backend as it walks the folder
const scan = ref<{
  import_files: number;
  import_bytes: number;
  download_files: number;
  download_bytes: number;
  assets_known: boolean;
} | null>(null);

async function pickMcFolder() {
  const dir = await open({ multiple: false, directory: true });
  if (!dir) return;
  importSrc.value = dir as string;
  await runScan();
}

async function runScan() {
  if (!importSrc.value) {
    scan.value = null;
    mcVersions.value = [];
    return;
  }
  scanning.value = true;
  scan.value = { import_files: 0, import_bytes: 0, download_files: 0, download_bytes: 0, assets_known: false };
  mcVersions.value = [];
  selectedVersions.value = [];
  try {
    // fire-and-forget: versions + live file stats arrive via events
    await api.scanMinecraftImport(importSrc.value);
  } catch (e) {
    message.error(String(e));
  } finally {
    // scanning stays until the `done` event resets it
  }
}

// refresh estimates when the selected versions change:
// now triggered explicitly in goStep2 instead of reactively
async function calcImportSize() {
  if (mode.value !== "importmc" || selectedVersions.value.length === 0 || !importSrc.value) return;
  const list = selectedVersions.value;
  const v = list[list.length - 1];
  const rawIds = list
    .map((id) => mcVersions.value.find((x) => x.id === id)?.raw_id ?? id)
    .filter(Boolean);
  calcSize.value = true;
  try {
    const dl = await api.estimateDownload(v);
    const imp = await api.estimateImport(importSrc.value, rawIds);
    scan.value = {
      import_files: imp.import_files,
      import_bytes: imp.import_bytes,
      download_files: dl.download_files,
      download_bytes: dl.download_bytes,
      assets_known: dl.assets_known,
    };
  } catch {
    /* ignore */
  } finally {
    calcSize.value = false;
  }
}

async function goStep2() {
  importStep.value = 2;
  await calcImportSize();
}

function pathBasename(p: string): string {
  const parts = p.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || "导入的实例";
}

function toggleVersion(id: string) {
  const i = selectedVersions.value.indexOf(id);
  if (i >= 0) {
    selectedVersions.value.splice(i, 1);
  } else {
    selectedVersions.value.push(id);
  }
}

async function importMc() {
  if (!importSrc.value) return message.warning("请先选择 .minecraft 文件夹");
  if (selectedVersions.value.length === 0) return message.warning("请至少选择一个游戏版本");
  // align loaders / loader versions with the selected versions, using the
  // auto-detected loader for each version
  const loaders: string[] = [];
  const loaderVersions: (string | null)[] = [];
  const rawIds: string[] = [];
  for (const sel of selectedVersions.value) {
    const v = mcVersions.value.find((x) => x.id === sel);
    loaders.push(v?.loader ?? "vanilla");
    loaderVersions.push(v?.loader_version ?? null);
    rawIds.push(v?.raw_id ?? sel);
  }
  importingMc.value = true;
  showVersionDialog.value = false;
  importProgress.value = { current: 0, total: rawIds.length, name: "", phase: "migrate", done: false };
  try {
    const plans = await api.importMinecraftFolder(
      importSrc.value,
      importMcBaseName.value,
      rawIds,
      selectedVersions.value,
      loaders,
      loaderVersions,
      migrateMode.value
    );
    // assign random game icon to each imported instance
    for (const p of plans) {
      try {
        const icons = await api.extractGameIcons(p.instance_id);
        if (icons.length > 0) {
          const pick = icons[Math.floor(Math.random() * icons.length)];
          const bgs = ["amber", "blue", "green", "purple", "red", "slate", "dark"];
          const bg = bgs[Math.floor(Math.random() * bgs.length)];
          await instances.patch({ id: p.instance_id, icon: `bg:${bg},img:${pick.path}` });
        }
      } catch { /* game not fully installed yet — bg-only is fine */ }
    }
    const fellBack = migrateMode.value === "symlink" && plans.some((p) => p.symlink_fallback);
    if (plans.length === 1) {
      if (fellBack) {
        message.warning("符号链接不可用（需要管理员或开发者模式），已自动改用复制");
      } else {
        message.success("导入并安装完成");
      }
      router.push(`/instance/${plans[0].instance_id}`);
    } else {
      if (fellBack) {
        message.warning(`已导入 ${plans.length} 个实例（符号链接不可用，已自动改用复制）`);
      } else {
        message.success(`已导入 ${plans.length} 个实例并安装`);
      }
      router.push("/instances");
    }
  } catch (e) {
    message.error(String(e));
  } finally {
    importingMc.value = false;
    importProgress.value = null;
  }
}

function loaderLabel(ld: string): string {
  switch (ld) {
    case "forge":
      return "Forge";
    case "neoforge":
      return "NeoForge";
    case "fabric":
      return "Fabric";
    case "quilt":
      return "Quilt";
    case "optifine":
      return "OptiFine";
    default:
      return "原版";
  }
}

let unlisteners: Array<() => void> = [];

onMounted(async () => {
  const BGS = ["amber", "blue", "green", "purple", "red", "slate", "dark"];
  const bg = BGS[Math.floor(Math.random() * BGS.length)];
  iconStr.value = `bg:${bg}`;
  try {
    const icons = await api.extractGameIcons(undefined);
    if (icons.length > 0) {
      const pick = icons[Math.floor(Math.random() * icons.length)];
      iconStr.value = `bg:${bg},img:${pick.path}`;
    }
  } catch { /* no jars yet — bg only is fine */ }
  instances.load();
  try {
    const m = await api.getVersionManifest();
    versions.value = m.versions;
  } catch (e) {
    message.error(String(e));
  }
  // live import-scan progress from the backend
  const u1 = await listen<{
    import_files: number;
    import_bytes: number;
    download_files?: number;
    download_bytes?: number;
    assets_known?: boolean;
    done?: boolean;
  }>("import://scan-progress", (ev) => {
    const p = ev.payload;
    scan.value = {
      import_files: p.import_files,
      import_bytes: p.import_bytes,
      download_files: p.download_files ?? scan.value?.download_files ?? 0,
      download_bytes: p.download_bytes ?? scan.value?.download_bytes ?? 0,
      assets_known: p.assets_known ?? scan.value?.assets_known ?? false,
    };
    if (p.done) {
      scanning.value = false;
      if (mcVersions.value.length > 0) { importStep.value = 1; showVersionDialog.value = true; }
    }
  });
  unlisteners.push(u1);
  // versions arrive one-by-one; append and auto-select the first.
  // Dedupe by id so a re-emit (or a second listener) never shows doubles.
  const u2 = await listen<{ id: string; raw_id: string; inherits_base: boolean; loader: string; loader_version: string | null; size_bytes: number }>(
    "import://scan-version",
    (ev) => {
      const v = ev.payload;
      if (mcVersions.value.some((x) => x.id === v.id)) return;
      mcVersions.value.push(v);
      if (mcVersions.value.length === 1) {
        selectedVersions.value = [v.id];
      }
    }
  );
  unlisteners.push(u2);
  // migration progress as each selected version is processed
  const u3 = await listen<{ current: number; total: number; name: string; phase: string; done: boolean }>(
    "import://progress",
    (ev) => {
      importProgress.value = ev.payload;
    }
  );
  unlisteners.push(u3);
  const u4 = await listen<{ name: string; message: string }>("import://warning", (ev) => {
    message.warning(`${ev.payload.name}：${ev.payload.message}`);
  });
  unlisteners.push(u4);
});

onUnmounted(() => {
  for (const u of unlisteners) u();
  unlisteners = [];
});
</script>

<template>
  <div class="create-view">
    <button class="back" @click="router.push('/instances')">
      <IconChevronLeft /> 返回实例列表
    </button>

    <div class="mode-tabs glass">
      <button :class="{ active: mode === 'fresh' }" @click="mode = 'fresh'">全新创建</button>
      <button :class="{ active: mode === 'import' }" @click="mode = 'import'">导入整合包</button>
      <button :class="{ active: mode === 'importmc' }" @click="mode = 'importmc'">导入游戏文件夹</button>
    </div>

    <!-- fresh create -->
    <div v-if="mode === 'fresh'" class="fresh glass">
      <div class="fresh-head">
        <button class="icon-box" title="选择图标" @click="showIconPicker = true">
          <AppIcon :name="iconStr" />
        </button>
        <div class="fresh-title">
          <h2>创建全新实例</h2>
          <p>设置图标、名称、游戏版本与加载器</p>
        </div>
      </div>

      <div class="field">
        <label>实例名称</label>
        <n-input v-model:value="name" placeholder="留空则自动用版本号命名" maxlength="40" />
      </div>

      <div v-if="instances.groups.length" class="field">
        <label>分组</label>
        <n-select
          v-model:value="newGroup"
          :options="groupOptions"
          placeholder="未分组"
          clearable
        />
      </div>

      <div class="field">
        <label>游戏版本</label>
        <div class="ver-cats">
          <button
            v-for="c in [
              { key: 'release', label: '正式版' },
              { key: 'snapshot', label: '快照版' },
              { key: 'april', label: '愚人节版' },
            ]"
            :key="c.key"
            :class="{ active: versionCat === c.key }"
            @click="versionCat = c.key"
          >
            {{ c.label }}
          </button>
        </div>
        <div class="ver-list">
          <button
            v-for="v in filteredVersions"
            :key="v.id"
            class="ver-item"
            :class="{ active: mcVersion === v.id }"
            @click="mcVersion = v.id"
          >
            <span class="ver-id mono">{{ v.id }}</span>
            <span class="ver-type">{{ v.type === 'release' || v.type.startsWith('old_') ? '正式' : '快照' }}</span>
          </button>
          <div v-if="!filteredVersions.length" class="ver-empty">该分类下暂无版本</div>
        </div>
      </div>

      <div class="field">
        <label>加载器</label>
        <div class="loader-row">
          <button
            v-for="l in loaders"
            :key="l.value"
            class="loader-btn"
            :class="{ active: loader === l.value }"
            @click="loader = l.value"
          >
            {{ l.label }}
          </button>
        </div>
        <n-select
          v-if="loader !== 'vanilla'"
          v-model:value="loaderVersion"
          :options="loaderOptions"
          :loading="loadingLoader"
          placeholder="选择加载器版本（留空使用最新稳定版）"
          class="loader-select"
        />
      </div>

      <div class="create-actions">
        <button class="btn primary" :disabled="creating" @click="create">
          <IconPlus /> {{ creating ? "创建中…" : "创建实例" }}
        </button>
      </div>
    </div>

    <!-- import -->
    <div v-else-if="mode === 'import'" class="import glass">
      <div class="import-icon"><IconFolder /></div>
      <h2>导入整合包</h2>
      <p>支持 Modrinth 整合包（.mrpack）与 CurseForge 整合包（.zip）。</p>
      <p class="sub-p">导入后将自动创建对应版本与加载器的实例，并把模组等文件放入其中。</p>
      <button class="btn primary big" :disabled="importing" @click="importPack">
        <IconFolder /> {{ importing ? "导入中…" : "选择整合包文件" }}
      </button>
    </div>

    <!-- import existing .minecraft folder -->
    <div v-else class="importmc glass">
      <div class="fresh-head">
        <div class="fresh-title">
          <h2>导入 .minecraft 游戏文件夹</h2>
          <p>选择已有的游戏目录（PCL2 / HMCL 等），将其存档、模组、配置等迁移到新实例</p>
        </div>
      </div>

      <div class="field">
        <label>游戏文件夹</label>
        <button class="folder-btn" @click="pickMcFolder">
          <IconFolder /> {{ importSrc || "选择 .minecraft 文件夹" }}
        </button>
        <div v-if="importSrc && !scanning && mcVersions.length === 0" class="detected-hint warn">
          未能在文件夹中找到任何版本（versions 目录为空）
        </div>
      </div>

      <div v-if="importProgress" class="import-progress">
        <div class="ip-row">
          <span class="ip-label">
            {{ importProgress.phase === "done" ? "安装完成" : "迁移中" }}：{{ importProgress.name }}
          </span>
          <span class="ip-count">{{ importProgress.current }} / {{ importProgress.total }}</span>
        </div>
        <div class="ip-bar">
          <div
            class="ip-fill"
            :class="{ done: importProgress.phase === 'done' }"
            :style="{ width: (importProgress.total ? (importProgress.current / importProgress.total) * 100 : 0) + '%' }"
          ></div>
        </div>
      </div>

      <NModal v-model:show="showVersionDialog" preset="card" :title="importStep === 1 ? '选择要迁移的版本' : '确认迁移信息'" style="max-width: 640px;" @update:show="(v: boolean) => { if (!v) importStep = 1; }">
        <div class="ver-dialog-body">
          <!-- Step 1: select versions -->
          <template v-if="importStep === 1">
            <div class="detected-hint">
              检测到 {{ mcVersions.length }} 个已安装版本，已选 {{ selectedVersions.length }} 个。每个版本创建一个独立实例，实例名使用版本号。
            </div>
            <div class="ver-list">
              <template v-for="g in groupedVersions" :key="g.loader">
                <div class="ver-group-title">
                  {{ loaderLabel(g.loader) }} <span class="ver-group-count">{{ g.items.length }}</span>
                </div>
                <button
                  v-for="v in g.items"
                  :key="v.id"
                  class="ver-item"
                  :class="{ active: selectedVersions.includes(v.id) }"
                  @click="toggleVersion(v.id)"
                >
                  <span class="ver-id mono">{{ v.id }}</span>
                  <span class="ver-loader" :class="'ld-' + v.loader">
                    {{ loaderLabel(v.loader) }}{{ v.loader_version ? " " + v.loader_version : "" }}
                  </span>
                </button>
              </template>
            </div>
          </template>
          <!-- Step 2: size info + migration method -->
          <template v-else>
            <div class="detected-hint">
              已选 {{ selectedVersions.length }} 个版本：{{ selectedVersions.join("、") }}
            </div>
            <div v-if="calcSize" class="scan-hint">正在计算迁移数据量…</div>
            <div v-else-if="scan" class="scan-panel">
              <div class="scan-row">
                <span class="scan-label">将迁移（{{ migrateMode === 'symlink' ? '符号链接' : '复制' }}）</span>
                <span class="scan-val">{{ scan.import_files }} 个文件 · {{ fmtBytes(scan.import_bytes) }}</span>
              </div>
              <div class="scan-row">
                <span class="scan-label">需要下载（游戏核心）</span>
                <span class="scan-val">
                  {{ scan.download_files }} 个文件 · {{ fmtBytes(scan.download_bytes) }}
                  <em v-if="!scan.assets_known">（资源文件大小需在安装时联网获取）</em>
                </span>
              </div>
            </div>
            <div class="field">
              <label>迁移方式</label>
              <div class="loader-row">
                <button class="loader-btn" :class="{ active: migrateMode === 'copy' }" @click="migrateMode = 'copy'">复制文件</button>
                <button class="loader-btn" :class="{ active: migrateMode === 'symlink' }" @click="migrateMode = 'symlink'">符号链接</button>
              </div>
              <p class="sub-p">
                复制方式占用额外磁盘空间但完全独立；符号链接不占用空间，下载的 mod 会直接保存到原始目录，但原文件夹不可删除或移动到其他磁盘。
              </p>
            </div>
          </template>
        </div>
        <template #footer>
          <div class="dialog-actions">
            <button v-if="importStep === 2" class="btn ghost" @click="importStep = 1">上一步</button>
            <button v-if="importStep === 1" class="btn ghost" @click="showVersionDialog = false">取消</button>
            <button v-if="importStep === 1" class="btn primary" :disabled="selectedVersions.length === 0" @click="goStep2">
              下一步
            </button>
            <button v-if="importStep === 2" class="btn primary" :disabled="importingMc || calcSize || !scan" @click="importMc">
              <IconPlus /> {{ importingMc ? "导入中…" : `迁移 ${selectedVersions.length} 个版本` }}
            </button>
          </div>
        </template>
      </NModal>
    </div>

    <IconPickerDialog v-model:show="showIconPicker" :value="iconStr" @save="iconStr = $event" />
  </div>
</template>

<style scoped>
.create-view {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.back {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  align-self: flex-start;
  background: none;
  border: none;
  color: var(--text-3);
  font-size: 13px;
  cursor: pointer;
  font-family: inherit;
  padding: 6px 10px;
  border-radius: 8px;
}
.back:hover {
  color: var(--text-1);
  background: rgba(255, 255, 255, 0.06);
}
.mode-tabs {
  display: inline-flex;
  gap: 4px;
  padding: 5px;
  align-self: flex-start;
}
.mode-tabs button {
  border: none;
  background: transparent;
  color: var(--text-2);
  padding: 8px 20px;
  border-radius: 9px;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.mode-tabs button.active {
  background: var(--accent-soft);
  color: var(--accent);
}
.fresh,
.import {
  padding: 24px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}
/* 宽屏：创建表单由单列改双列，避免在超宽屏上被拉成又宽又长的一条 */
@media (min-width: 1200px) {
  .fresh {
    display: grid;
    grid-template-columns: 1fr 1fr;
    align-items: start;
  }
  .fresh > .fresh-head,
  .fresh > .create-actions {
    grid-column: 1 / -1;
  }
}
.fresh-head {
  display: flex;
  align-items: center;
  gap: 16px;
}
.icon-box {
  width: 64px;
  height: 64px;
  border-radius: 16px;
  background: transparent;
  border: 1px dashed rgba(255, 255, 255, 0.2);
  overflow: hidden;
  cursor: pointer;
  font-size: 30px;
  transition: all 0.12s;
  box-sizing: border-box;
  padding: 0;
  position: relative;
}
.icon-box :deep(.app-icon) {
  position: absolute;
  inset: 0;
}
.icon-box:hover {
  border-color: var(--accent-06);
}
.fresh-title h2 {
  margin: 0 0 4px;
  font-size: 18px;
}
.fresh-title p {
  margin: 0;
  font-size: 12px;
  color: var(--text-3);
}
.field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.field label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
}
.ver-cats {
  display: flex;
  gap: 4px;
}
.ver-cats button {
  border: none;
  background: rgba(255, 255, 255, 0.05);
  color: var(--text-2);
  padding: 7px 16px;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.ver-cats button.active {
  background: var(--accent-soft);
  color: var(--accent);
}
.ver-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 340px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: 10px;
}
.ver-dialog-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.ver-group-title {
  margin: 10px 2px 2px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-2);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  padding-bottom: 3px;
}
.ver-group-title:first-child {
  margin-top: 2px;
}
.ver-group-count {
  font-size: 10px;
  font-weight: 400;
  color: var(--text-3);
  background: rgba(255, 255, 255, 0.06);
  border-radius: 8px;
  padding: 0 6px;
  margin-left: 4px;
}
.ver-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-2);
  padding: 7px 10px;
  border-radius: 8px;
  cursor: pointer;
  font-family: inherit;
  text-align: left;
}
.ver-item:hover {
  background: rgba(255, 255, 255, 0.05);
}
.ver-item.active {
  border-color: var(--accent-05);
  background: var(--accent-soft);
  color: var(--accent);
}
.ver-id {
  font-size: 12px;
  font-weight: 600;
}
.ver-type {
  font-size: 10px;
  color: var(--text-3);
}
.ver-loader {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-2);
  margin-top: 2px;
}
.ver-loader.ld-forge { color: #e8a04b; }
.ver-loader.ld-neoforge { color: #7aa2f7; }
.ver-loader.ld-fabric { color: #b48ead; }
.ver-loader.ld-quilt { color: #9ece6a; }
.ver-loader.ld-vanilla { color: var(--text-3); }
.ver-empty {
  grid-column: 1 / -1;
  text-align: center;
  color: var(--text-3);
  font-size: 13px;
  padding: 20px 0;
}
.loader-row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.loader-btn {
  padding: 8px 16px;
  border-radius: 9px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.13s;
  font-family: inherit;
}
.loader-btn:hover {
  background: rgba(255, 255, 255, 0.08);
}
.loader-btn.active {
  background: var(--accent-soft);
  border-color: var(--accent-45);
  color: var(--accent);
}
.loader-select {
  max-width: 320px;
}
.create-actions {
  display: flex;
  justify-content: flex-end;
}
.btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  border: none;
  border-radius: 10px;
  padding: 10px 22px;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.15s;
}
.btn.primary {
  background: linear-gradient(135deg, var(--accent), var(--accent-deep));
  color: #1a1208;
}
.btn.primary:hover:not(:disabled) {
  filter: brightness(1.08);
}
.btn:disabled {
  opacity: 0.6;
}
.btn.big {
  padding: 13px 26px;
  font-size: 15px;
}
.import {
  align-items: center;
  text-align: center;
  padding: 50px 30px;
}
.import-icon {
  font-size: 40px;
  color: var(--accent);
  opacity: 0.8;
}
.import h2 {
  margin: 0;
  font-size: 20px;
}
.import p {
  margin: 0;
  font-size: 13px;
  color: var(--text-2);
  max-width: 420px;
}
.import .sub-p {
  font-size: 12px;
  color: var(--text-3);
}
.folder-btn {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  align-self: flex-start;
  border: 1px dashed var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  padding: 9px 16px;
  border-radius: 9px;
  font-size: 13px;
  cursor: pointer;
  font-family: inherit;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.folder-btn:hover {
  border-color: var(--accent-05);
  color: var(--accent);
}
.scan-panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 16px 18px;
  background: rgba(255, 255, 255, 0.03);
}
.scan-row {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 12px;
  flex-wrap: wrap;
}
.scan-label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
}
.scan-val {
  font-size: 13px;
  color: var(--accent);
  font-variant-numeric: tabular-nums;
}
.scan-val em {
  color: var(--text-3);
  font-style: normal;
  font-size: 11px;
}
.scan-hint {
  font-size: 13px;
  color: var(--text-3);
}
.importmc {
  padding: 24px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}
.import-progress {
  background: var(--glass-2, rgba(255, 255, 255, 0.04));
  border: 1px solid var(--border, rgba(255, 255, 255, 0.08));
  border-radius: 12px;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.ip-row {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 12px;
}
.ip-label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ip-count {
  font-size: 13px;
  color: var(--accent);
  font-variant-numeric: tabular-nums;
  flex: none;
}
.ip-bar {
  height: 6px;
  border-radius: 99px;
  background: rgba(255, 255, 255, 0.08);
  overflow: hidden;
}
.ip-fill {
  height: 100%;
  border-radius: 99px;
  background: var(--accent);
  transition: width 0.25s ease;
}
.ip-fill.done {
  background: #57c98a;
}
.detected-hint {
  font-size: 12px;
  color: var(--text-2);
  margin-top: 4px;
}
.detected-hint.warn {
  color: #e0a85a;
}
</style>
