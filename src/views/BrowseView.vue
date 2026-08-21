<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { NButton, NDrawer, NDrawerContent, NPagination, NSelect, useMessage } from "naive-ui";
import { api } from "../api";
import InstallDialog from "../components/InstallDialog.vue";
import ProjectCard from "../components/ProjectCard.vue";
import {
  IconAlignJustify,
  IconClose,
  IconGrid,
  IconList,
  IconSearch,
  IconSliders,
} from "../components/icons";
import { cnCfName, CN_CATS } from "../utils/categories";
import { cacheGet, cacheSet } from "../utils/cache";
import type { ProjectDependency, ProjectHit } from "../types";

const message = useMessage();
const provider = ref<"all" | "modrinth" | "curseforge">("all");
const query = ref("");
const type = ref("mod");
const category = ref("");
const page = ref(0);
const results = ref<ProjectHit[]>([]);
const total = ref(0);
const loading = ref(false);
const cfCategories = ref<{ id: number; name: string }[]>([]);
// 筛选状态（游戏版本 / 加载器 / 类别）+ 侧边抽屉
const gameVersion = ref("");
const loader = ref("");
const showFilter = ref(false);
const versionOptions = ref<{ label: string; value: string }[]>([]);
const loaderOptions = [
  { label: "全部加载器", value: "" },
  { label: "Fabric", value: "fabric" },
  { label: "Forge", value: "forge" },
  { label: "NeoForge", value: "neoforge" },
  { label: "Quilt", value: "quilt" },
];
// 来源 / 排序 / 每页数量 / 视图
const providerOptions = [
  { label: "全部来源", value: "all" },
  { label: "Modrinth", value: "modrinth" },
  { label: "CurseForge", value: "curseforge" },
];
const sort = ref("downloads");
const pageSize = ref(20);
const view = ref<"grid" | "list" | "compact">("grid");
const sortOptions = [
  { label: "下载量", value: "downloads" },
  { label: "相关度", value: "relevance" },
  { label: "收藏数", value: "follows" },
  { label: "最新发布", value: "newest" },
  { label: "最近更新", value: "updated" },
];
const pageSizeOptions = [
  { label: "20 条 / 页", value: 20 },
  { label: "40 条 / 页", value: 40 },
  { label: "60 条 / 页", value: 60 },
];

const types = [
  { key: "mod", label: "模组" },
  { key: "modpack", label: "整合包" },
  { key: "resourcepack", label: "资源包" },
  { key: "shader", label: "光影" },
  { key: "datapack", label: "数据包" },
];

const modrinthCategories: Record<string, string[]> = {
  mod: ["", "fabric", "forge", "quilt", "neoforge", "optimization", "library", "utility", "adventure", "magic", "tech", "decoration", "equipment", "food", "misc", "mobs", "storage", "worldgen"],
  modpack: ["", "fabric", "forge", "quilt", "neoforge", "adventure", "challenge", "combat", "hardcore", "magic", "mini-game", "multiplayer", "optimization", "pvp", "tech", "vanilla-plus"],
  resourcepack: ["", "16x", "32x", "64x", "128x", "256x", "512x", "faithful", "cursed", "modern", "semi-realistic", "simplistic", "themed"],
  shader: ["", "potato", "low", "medium", "high", "ultra", "path-tracing", "complementary", "realistic"],
  datapack: ["", "adventure", "challenge", "decoration", "magic", "minigame", "mobs", "optimization", "technology", "utility", "worldgen"],
};

const catOptions = ref<{ label: string; value: string }[]>([]);
const installTarget = ref<ProjectHit | null>(null);
const showInstall = ref(false);

async function search() {
  // 短期缓存（5 分钟），避免来回切换页面重复拉取
  const cacheKey = `browse:${provider.value}|${query.value}|${type.value}|${category.value}|${page.value}|${gameVersion.value}|${loader.value}|${sort.value}|${pageSize.value}`;
  const cached = cacheGet<{ hits: ProjectHit[]; total: number }>(cacheKey);
  if (cached) {
    results.value = cached.hits;
    total.value = cached.total;
    return;
  }
  loading.value = true;
  try {
    const res = await api.browse(
      provider.value,
      query.value,
      type.value,
      category.value,
      page.value,
      gameVersion.value,
      loader.value,
      sort.value,
      pageSize.value
    );
    results.value = res.hits;
    total.value = res.total;
    cacheSet(cacheKey, res, 5 * 60 * 1000);
  } catch (e) {
    message.error(String(e));
    results.value = [];
  } finally {
    loading.value = false;
  }
}

const pageCount = computed(() => Math.max(1, Math.ceil(total.value / pageSize.value)));
function onPage(p: number) {
  page.value = p - 1;
  search();
}

const hasFilter = computed(() => !!category.value || !!gameVersion.value || !!loader.value);

function resetFilters() {
  category.value = "";
  gameVersion.value = "";
  loader.value = "";
  page.value = 0;
  search();
}

function catLabel(v: string) {
  return catOptions.value.find((o) => o.value === v)?.label ?? v;
}
function loaderLabel(v: string) {
  return loaderOptions.find((o) => o.value === v)?.label ?? v;
}

async function loadCfCategories() {
  if (provider.value !== "curseforge") return;
  const cacheKey = `cf-cats:${type.value}`;
  const cached = cacheGet<{ id: number; name: string }[]>(cacheKey);
  if (cached) {
    cfCategories.value = cached;
    return;
  }
  try {
    cfCategories.value = (await api.curseforgeCategories(type.value)).categories;
    cacheSet(cacheKey, cfCategories.value, 10 * 60 * 1000);
  } catch {
    cfCategories.value = [];
  }
}

async function loadVersions() {
  const cacheKey = "versions:release";
  const cached = cacheGet<{ label: string; value: string }[]>(cacheKey);
  if (cached) {
    versionOptions.value = cached;
    return;
  }
  try {
    const res = await api.getVersionManifest();
    const ids = res.versions
      .filter((v) => v.type === "release")
      .map((v) => v.id)
      .slice(0, 40);
    versionOptions.value = [
      { label: "全部版本", value: "" },
      ...ids.map((id) => ({ label: id, value: id })),
    ];
    cacheSet(cacheKey, versionOptions.value, 10 * 60 * 1000);
  } catch {
    versionOptions.value = [{ label: "全部版本", value: "" }];
  }
}

function rebuildOptions() {
  if (provider.value === "all" || provider.value === "modrinth") {
    catOptions.value = (modrinthCategories[type.value] ?? []).map((c) => ({
      label: c ? CN_CATS[c] ?? c : "全部分类",
      value: c,
    }));
  } else {
    catOptions.value = [
      { label: "全部分类", value: "" },
      ...cfCategories.value.map((c) => ({ label: cnCfName(c.name), value: String(c.id) })),
    ];
  }
}

let debounce: ReturnType<typeof setTimeout> | null = null;
watch(query, () => {
  if (debounce) clearTimeout(debounce);
  debounce = setTimeout(() => {
    page.value = 0;
    search();
  }, 450);
});

watch([provider, type], async () => {
  category.value = "";
  page.value = 0;
  if (provider.value === "curseforge") await loadCfCategories();
  rebuildOptions();
  search();
});

watch(category, () => {
  page.value = 0;
  search();
});

watch([gameVersion, loader], () => {
  page.value = 0;
  search();
});

watch([sort, pageSize], () => {
  page.value = 0;
  search();
});

function openInstall(p: ProjectHit) {
  installTarget.value = p;
  showInstall.value = true;
}

async function openInstallDep(dep: ProjectDependency) {
  try {
    // 拉取依赖项目的完整信息（图标/作者/描述/下载量等）
    const info = await api.projectInfo("modrinth", dep.projectId);
    installTarget.value = info;
  } catch {
    // 拉取失败时退回精简数据
    installTarget.value = {
      provider: "modrinth",
      id: dep.projectId,
      slug: dep.slug,
      title: dep.title,
      description: "",
      author: "",
      downloads: 0,
      follows: 0,
      icon_url: "",
      project_type: "mod",
      categories: [],
      latest_version: "",
      game_versions: [],
    };
  }
  showInstall.value = true;
}

onMounted(async () => {
  rebuildOptions();
  loadVersions();
  await search();
});
</script>

<template>
  <div class="browse">
    <div class="head">
      <h1>内容中心</h1>
      <p class="sub">从 Modrinth 与 CurseForge 一键浏览、安装与升级模组、整合包、资源包和光影，支持全部来源整合浏览</p>
    </div>

    <div class="toolbar glass">
      <div class="toolbar-row">
        <div class="search-box">
          <IconSearch />
          <input v-model="query" placeholder="搜索内容…（如 sodium / iris / 某整合包）" />
        </div>
      </div>
      <div class="toolbar-row">
        <div class="type-tabs">
          <button
            v-for="t in types"
            :key="t.key"
            :class="{ active: type === t.key }"
            @click="type = t.key"
          >
            {{ t.label }}
          </button>
        </div>
        <n-select
          v-if="provider !== 'all'"
          v-model:value="sort"
          :options="sortOptions"
          size="small"
          class="tb-select"
        />
        <n-select
          v-model:value="pageSize"
          :options="pageSizeOptions"
          size="small"
          class="tb-select page-size"
        />
        <div class="view-switch">
          <button :class="{ active: view === 'grid' }" title="网格" @click="view = 'grid'"><IconGrid /></button>
          <button :class="{ active: view === 'list' }" title="列表" @click="view = 'list'"><IconList /></button>
          <button :class="{ active: view === 'compact' }" title="紧凑列表" @click="view = 'compact'"><IconAlignJustify /></button>
        </div>
        <button class="filter-btn" :class="{ on: hasFilter }" @click="showFilter = true">
          <IconSliders /> 筛选
        </button>
        <n-select
          v-model:value="provider"
          :options="providerOptions"
          size="small"
          class="tb-select provider"
        />
      </div>
    </div>

    <div v-if="hasFilter" class="filter-tags glass">
      <span v-if="gameVersion" class="ftag">
        版本 {{ gameVersion }}
        <button class="ftag-x" title="移除" @click="gameVersion = ''"><IconClose /></button>
      </span>
      <span v-if="loader" class="ftag">
        加载器 {{ loaderLabel(loader) }}
        <button class="ftag-x" title="移除" @click="loader = ''"><IconClose /></button>
      </span>
      <span v-if="category" class="ftag">
        分类 {{ catLabel(category) }}
        <button class="ftag-x" title="移除" @click="category = ''"><IconClose /></button>
      </span>
      <button class="ftag ftag-clear" @click="resetFilters">清除全部</button>
    </div>

    <div v-if="provider === 'curseforge' && !cfCategories.length && !loading" class="cf-hint glass">
      CurseForge 需要 API Key。请前往
      <a href="https://console.curseforge.com" target="_blank">console.curseforge.com</a>
      免费申请，并在 <router-link to="/settings">设置</router-link> 中填写。
    </div>

    <div v-if="loading" class="center">搜索中…</div>
    <div v-else-if="!results.length" class="center">没有找到相关内容</div>
    <div v-else class="grid" :class="`view-${view}`">
      <ProjectCard
        v-for="p in results"
        :key="p.provider + p.id"
        :project="p"
        :view="view"
        @install="openInstall"
      />
    </div>

    <div v-if="total > 20" class="pager">
      <span class="pager-total">共 {{ total }} 条</span>
      <n-pagination
        :page="page + 1"
        :page-count="pageCount"
        :page-slot="7"
        @update:page="onPage"
      />
    </div>

    <n-drawer v-model:show="showFilter" :width="330" placement="right">
      <n-drawer-content title="筛选" closable>
        <div class="filter-group">
          <label>游戏版本</label>
          <n-select v-model:value="gameVersion" :options="versionOptions" size="small" />
        </div>
        <div class="filter-group">
          <label>加载器</label>
          <n-select v-model:value="loader" :options="loaderOptions" size="small" />
        </div>
        <div class="filter-group">
          <label>类别</label>
          <n-select v-model:value="category" :options="catOptions" size="small" />
          <p v-if="provider === 'all'" class="filter-hint">全部来源下分类按 Modrinth 筛选，CurseForge 结果不受分类影响</p>
        </div>
        <div class="filter-actions">
          <n-button size="small" @click="resetFilters">重置</n-button>
          <n-button size="small" type="primary" @click="showFilter = false">完成</n-button>
        </div>
      </n-drawer-content>
    </n-drawer>

    <InstallDialog v-model:show="showInstall" :project="installTarget" @install-dep="openInstallDep" />
  </div>
</template>

<style scoped>
.browse {
  max-width: 1160px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.head h1 {
  margin: 0 0 4px;
  font-size: 24px;
}
.sub {
  margin: 0;
  color: var(--text-3);
  font-size: 13px;
}
.toolbar {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 14px;
}
.toolbar-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.toolbar-row .tb-select.provider {
  margin-left: auto;
}
.tb-select.provider {
  width: 120px;
}
.search-box {
  flex: 1;
  min-width: 220px;
  display: flex;
  align-items: center;
  gap: 8px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid var(--border);
  border-radius: 9px;
  padding: 0 12px;
  color: var(--text-3);
}
.search-box input {
  flex: 1;
  background: none;
  border: none;
  outline: none;
  color: var(--text-1);
  padding: 8px 0;
  font-size: 13px;
  font-family: inherit;
}
.type-tabs {
  display: flex;
  gap: 4px;
}
.type-tabs button {
  border: none;
  background: transparent;
  color: var(--text-2);
  padding: 7px 12px;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.type-tabs button:hover {
  background: rgba(255, 255, 255, 0.05);
}
.type-tabs button.active {
  background: var(--accent-soft);
  color: var(--accent);
}
.filter-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  border-radius: 9px;
  padding: 7px 13px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.filter-btn:hover {
  background: rgba(255, 255, 255, 0.08);
}
.filter-btn.on {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-soft);
}
.filter-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 18px;
}
.filter-group label {
  font-size: 12px;
  color: var(--text-2);
  font-weight: 600;
}
.filter-hint {
  font-size: 11px;
  color: var(--text-3);
  margin: 4px 0 0;
}
.filter-actions {
  display: flex;
  gap: 10px;
  margin-top: 8px;
}
.cf-hint {
  padding: 12px 16px;
  font-size: 13px;
  color: var(--text-2);
}
.cf-hint a {
  color: var(--accent);
  text-decoration: none;
}
.center {
  padding: 70px;
  text-align: center;
  color: var(--text-3);
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(330px, 1fr));
  gap: 14px;
}
.grid.view-list,
.grid.view-compact {
  grid-template-columns: 1fr;
}
.grid.view-compact {
  gap: 8px;
}
.tb-select {
  width: 110px;
}
.tb-select.page-size {
  width: 130px;
}
.view-switch {
  display: flex;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 9px;
  padding: 3px;
  gap: 2px;
}
.view-switch button {
  border: none;
  background: transparent;
  color: var(--text-3);
  width: 30px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 7px;
  cursor: pointer;
  font-size: 14px;
}
.view-switch button:hover {
  color: var(--text-1);
}
.view-switch button.active {
  background: var(--accent);
  color: #1a1208;
}
.filter-tags {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  padding: 8px 14px;
}
.ftag {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  background: var(--accent-soft);
  color: var(--accent);
  border: 1px solid rgba(232, 154, 75, 0.3);
  border-radius: 8px;
  padding: 3px 10px;
}
.ftag-x {
  border: none;
  background: transparent;
  color: inherit;
  cursor: pointer;
  display: inline-flex;
  padding: 0;
  font-size: 12px;
  opacity: 0.7;
}
.ftag-x:hover {
  opacity: 1;
}
.ftag-clear {
  background: transparent;
  color: var(--text-3);
  border-color: var(--border);
  cursor: pointer;
  font-family: inherit;
}
.ftag-clear:hover {
  color: var(--text-1);
}
.pager {
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 16px;
  color: var(--text-3);
  font-size: 13px;
}
.pager-total {
  color: var(--text-3);
}
</style>
