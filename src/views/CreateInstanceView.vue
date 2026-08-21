<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { NSelect, NInput, useMessage } from "naive-ui";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useInstancesStore } from "../stores/instances";
import AppIcon from "../components/AppIcon.vue";
import IconPickerDialog from "../components/IconPickerDialog.vue";
import { IconChevronLeft, IconFolder, IconPlus } from "../components/icons";
import type { Loader } from "../types";

const router = useRouter();
const instances = useInstancesStore();
const message = useMessage();

const mode = ref<"fresh" | "import">("fresh");

// ---- fresh create ----
const name = ref("");
const iconStr = ref("");
const showIconPicker = ref(false);
const mcVersion = ref("");
const loader = ref<Loader>("vanilla");
const loaderVersion = ref<string | null>(null);

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

function isAprilFools(v: { id: string; releaseTime: string }) {
  if (/april|fools/i.test(v.id)) return true;
  const d = v.releaseTime;
  return d.length >= 10 && d.slice(5, 7) === "04" && d.slice(8, 10) === "01";
}

const filteredVersions = computed(() => {
  return versions.value.filter((v) => {
    if (versionCat.value === "release") return v.type === "release" || v.type.startsWith("old_");
    if (versionCat.value === "april") return isAprilFools(v);
    return v.type === "snapshot" && !isAprilFools(v);
  });
});

watch([mcVersion, loader], async ([mc, ld]) => {
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
  if (!name.value.trim()) return message.warning("请输入实例名称");
  if (!mcVersion.value) return message.warning("请选择游戏版本");
  creating.value = true;
  try {
    const inst = await instances.create(
      name.value.trim(),
      mcVersion.value,
      loader.value,
      loaderVersion.value || null
    );
    if (iconStr.value) {
      await instances.patch({ id: inst.id, icon: iconStr.value });
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

onMounted(async () => {
  instances.load();
  try {
    const m = await api.getVersionManifest();
    versions.value = m.versions;
  } catch (e) {
    message.error(String(e));
  }
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
        <n-input v-model:value="name" placeholder="例如：我的生存世界" maxlength="40" />
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
    <div v-else class="import glass">
      <div class="import-icon"><IconFolder /></div>
      <h2>导入整合包</h2>
      <p>支持 Modrinth 整合包（.mrpack）与 CurseForge 整合包（.zip）。</p>
      <p class="sub-p">导入后将自动创建对应版本与加载器的实例，并把模组等文件放入其中。</p>
      <button class="btn primary big" :disabled="importing" @click="importPack">
        <IconFolder /> {{ importing ? "导入中…" : "选择整合包文件" }}
      </button>
    </div>

    <IconPickerDialog v-model:show="showIconPicker" :value="iconStr" @save="iconStr = $event" />
  </div>
</template>

<style scoped>
.create-view {
  max-width: 760px;
  margin: 0 auto;
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
.fresh-head {
  display: flex;
  align-items: center;
  gap: 16px;
}
.icon-box {
  width: 64px;
  height: 64px;
  border-radius: 16px;
  background: rgba(255, 255, 255, 0.05);
  border: 1px dashed rgba(255, 255, 255, 0.2);
  overflow: hidden;
  cursor: pointer;
  font-size: 30px;
  transition: all 0.12s;
}
.icon-box:hover {
  border-color: rgba(232, 154, 75, 0.6);
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
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 6px;
  max-height: 220px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 8px;
  background: rgba(255, 255, 255, 0.02);
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
  border-color: rgba(232, 154, 75, 0.5);
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
  border-color: rgba(232, 154, 75, 0.45);
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
</style>
