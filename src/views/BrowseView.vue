<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { NButton, NDrawer, NDrawerContent, NSelect, useMessage } from "naive-ui";
import { api } from "../api";
import InstallDialog from "../components/InstallDialog.vue";
import ProjectCard from "../components/ProjectCard.vue";
import SimplePagination from "../components/SimplePagination.vue";
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
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import type { Instance, ProjectDependency, ProjectHit } from "../types";

const message = useMessage();
const route = useRoute();
const provider = ref<"all" | "modrinth" | "curseforge">("all");
const query = ref(typeof route.query.q === "string" ? route.query.q : "");
const type = ref("mod");
const category = ref("");
const page = ref(0);
const results = ref<ProjectHit[]>([]);
const cfError = ref("");
const cfCount = ref(0);
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

// 实例选择：非整合包类型下可选择实例，自动筛选游戏版本和加载器
const instances = ref<Instance[]>([]);
const selectedInstanceId = ref<string | null>(null);
const instanceOptions = computed(() => [
  { label: "不关联实例", value: "" },
  ...instances.value.map((i) => ({
    label: `${i.name} (${i.mc_version}${i.loader !== "vanilla" ? ` ${i.loader}` : ""})`,
    value: i.id,
  })),
]);
const showInstanceSelect = computed(() => type.value !== "modpack" && instances.value.length > 0);
const showLoaderFilter = computed(() => type.value === "mod" || type.value === "modpack");
const instanceSelectWidth = computed(() => {
  const inst = instances.value.find((i) => i.id === selectedInstanceId.value);
  if (!inst) return 140;
  const label = `${inst.name} (${inst.mc_version}${inst.loader !== "vanilla" ? ` ${inst.loader}` : ""})`;
  return Math.max(140, Math.min(label.length * 8 + 40, 300));
});

let searchSeq = 0;
async function search() {
  const seq = ++searchSeq;
  // 短期缓存（5 分钟），避免来回切换页面重复拉取
  const cacheKey = `browse:${provider.value}|${query.value}|${type.value}|${category.value}|${page.value}|${gameVersion.value}|${loader.value}|${sort.value}|${pageSize.value}`;
  const cached = cacheGet<{ hits: ProjectHit[]; total: number; cf_error?: string | null; cf_count?: number }>(cacheKey);
  if (cached) {
    if (seq !== searchSeq) return;
    results.value = cached.hits;
    if (page.value === 0) total.value = cached.total;
    cfError.value = cached.cf_error ?? "";
    cfCount.value = cached.cf_count ?? 0;
    loading.value = false;
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
    if (seq !== searchSeq) return;
    results.value = res.hits;
    if (page.value === 0) total.value = res.total;
    cfError.value = res.cf_error ?? "";
    cfCount.value = res.cf_count ?? 0;
    cacheSet(cacheKey, res, 5 * 60 * 1000);
  } catch (e) {
    if (seq !== searchSeq) return;
    message.error(String(e));
    results.value = [];
    cfError.value = "";
  } finally {
    if (seq === searchSeq) loading.value = false;
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
  if (!showLoaderFilter.value) loader.value = "";
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
  // 如果手动改了版本/加载器，且不再匹配所选实例，则切换为"不关联实例"
  const inst = instances.value.find((i) => i.id === selectedInstanceId.value);
  if (inst) {
    const versionMismatch = gameVersion.value !== inst.mc_version;
    const loaderMismatch = showLoaderFilter.value && loader.value !== (inst.loader === "vanilla" ? "" : inst.loader);
    if (versionMismatch || loaderMismatch) {
      suppressInstanceClear = true;
      selectedInstanceId.value = "";
    }
  }
  page.value = 0;
  search();
});

let suppressInstanceClear = false;
watch(selectedInstanceId, () => {
  if (suppressInstanceClear) {
    suppressInstanceClear = false;
    return;
  }
  const inst = instances.value.find((i) => i.id === selectedInstanceId.value);
  if (inst) {
    gameVersion.value = inst.mc_version;
    if (showLoaderFilter.value) {
      loader.value = inst.loader === "vanilla" ? "" : inst.loader;
    }
  } else {
    gameVersion.value = "";
    loader.value = "";
  }
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
  const provider = installTarget.value?.provider ?? "modrinth";
  try {
    const info = await api.projectInfo(provider, dep.projectId);
    installTarget.value = info;
  } catch {
    installTarget.value = {
      provider,
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
      updated: "",
      featured_image: "",
    };
  }
  showInstall.value = true;
}

// 类型卡片的滑动高亮指示器（先扩展包裹再收缩）
const typeBox = ref<HTMLElement | null>(null);
const { indicatorStyle: typeIndicatorStyle, refresh: refreshTypeIndicator } = useSlidingIndicator(
  typeBox,
  () => Array.from(typeBox.value?.querySelectorAll<HTMLElement>(".type-card button") ?? []),
  () => types.findIndex((t) => t.key === type.value),
  { axis: "horizontal" }
);
watch(type, () => nextTick(() => refreshTypeIndicator()));

// 视图切换的滑动高亮指示器（网格/列表/紧凑）
const viewBox = ref<HTMLElement | null>(null);
const { indicatorStyle: viewIndicatorStyle, refresh: refreshViewIndicator } = useSlidingIndicator(
  viewBox,
  () => Array.from(viewBox.value?.querySelectorAll<HTMLElement>(".view-switch button") ?? []),
  () => ["grid", "list", "compact"].indexOf(view.value),
  { axis: "horizontal" }
);
watch(view, () => nextTick(() => refreshViewIndicator()));

onMounted(async () => {
  try {
    instances.value = await api.listInstances();
  } catch {
    instances.value = [];
  }
  // 默认选择最近游玩的实例
  if (instances.value.length > 0 && type.value !== "modpack") {
    const sorted = [...instances.value].sort(
      (a, b) => (b.last_played ?? 0) - (a.last_played ?? 0)
    );
    selectedInstanceId.value = sorted[0].id;
  }
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

    <div ref="typeBox" class="type-card glass">
      <div class="indicator" :style="typeIndicatorStyle"></div>
      <button
        v-for="t in types"
        :key="t.key"
        :class="{ active: type === t.key }"
        @click="type = t.key"
      >
        {{ t.label }}
      </button>
    </div>

    <div class="toolbar glass">
      <div class="toolbar-row">
        <div class="search-box">
          <IconSearch />
          <input v-model="query" placeholder="搜索内容…（如 sodium / iris / 某整合包）" />
        </div>
        <n-select
          v-if="showInstanceSelect"
          v-model:value="selectedInstanceId"
          :options="instanceOptions"
          size="small"
          class="tb-select instance-select"
          :style="{ width: instanceSelectWidth + 'px' }"
        />
      </div>
      <div class="toolbar-row">
        <n-select
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
        <div ref="viewBox" class="view-switch">
          <div class="indicator" :style="viewIndicatorStyle"></div>
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

    <div v-if="provider === 'all' && cfError && !loading" class="cf-hint glass">
      CurseForge 来源加载失败：{{ cfError }}。请前往 <router-link to="/settings">设置</router-link> 检查 API Key。
    </div>

    <div v-show="loading" class="center">搜索中…</div>
    <div v-show="!loading && !results.length" class="center">没有找到相关内容</div>
    <div v-show="!loading && results.length" class="grid" :class="`view-${view}`">
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
      <SimplePagination
        :page="page + 1"
        :page-count="pageCount"
        @update:page="onPage"
      />
    </div>

    <n-drawer v-model:show="showFilter" :width="330" placement="right">
      <n-drawer-content title="筛选" closable>
        <div class="filter-group">
          <label>游戏版本</label>
          <n-select v-model:value="gameVersion" :options="versionOptions" size="small" />
        </div>
        <div v-if="showLoaderFilter" class="filter-group">
          <label>加载器</label>
          <div class="filter-chips">
            <button
              v-for="opt in loaderOptions"
              :key="opt.value"
              class="filter-chip"
              :class="{ active: loader === opt.value }"
              @click="loader = opt.value"
            >{{ opt.label }}</button>
          </div>
        </div>
        <div class="filter-group">
          <label>类别</label>
          <div class="filter-chips">
            <button
              v-for="opt in catOptions"
              :key="opt.value"
              class="filter-chip"
              :class="{ active: category === opt.value }"
              @click="category = opt.value"
            >{{ opt.label }}</button>
          </div>
          <p v-if="provider === 'all'" class="filter-hint">全部来源下分类按 Modrinth 筛选，CurseForge 结果不受分类影响</p>
        </div>
        <div class="filter-actions">
          <n-button size="small" @click="resetFilters">重置</n-button>
          <n-button size="small" type="primary" @click="showFilter = false">完成</n-button>
        </div>
      </n-drawer-content>
    </n-drawer>

    <InstallDialog v-model:show="showInstall" :project="installTarget" :default-instance="selectedInstanceId" @install-dep="openInstallDep" />
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
.type-card {
  position: relative;
  display: flex;
  justify-content: flex-start;
  flex-wrap: wrap;
  gap: 8px;
  padding: 10px 14px;
}
.type-card .indicator {
  position: absolute;
  top: 10px;
  bottom: 10px;
  border-radius: 9px;
  background: var(--accent-soft);
  pointer-events: none;
}
.type-card button {
  border: none;
  background: transparent;
  color: var(--text-2);
  padding: 8px 20px;
  border-radius: 9px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.type-card button:hover {
  background: rgba(255, 255, 255, 0.05);
}
.type-card button.active {
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
.filter-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.filter-chip {
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  border-radius: 7px;
  padding: 5px 11px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.filter-chip:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-1);
}
.filter-chip.active {
  background: var(--accent-soft);
  border-color: var(--accent);
  color: var(--accent);
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
.tb-select.instance-select {
  flex-shrink: 0;
}
.tb-select.instance-select :deep(.n-base-selection) {
  --n-height: 35px !important;
}
.view-switch {
  position: relative;
  display: flex;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 9px;
  padding: 3px;
  gap: 2px;
}
.view-switch .indicator {
  position: absolute;
  top: 3px;
  bottom: 3px;
  border-radius: 7px;
  background: var(--accent-soft);
  pointer-events: none;
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
  color: var(--accent);
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
