<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { NSelect, useMessage } from "naive-ui";
import { api } from "../api";
import InstallDialog from "../components/InstallDialog.vue";
import ProjectCard from "../components/ProjectCard.vue";
import { IconChevronLeft, IconChevronRight, IconSearch } from "../components/icons";
import type { ProjectDependency, ProjectHit } from "../types";

const message = useMessage();
const provider = ref<"modrinth" | "curseforge">("modrinth");
const query = ref("");
const type = ref("mod");
const category = ref("");
const page = ref(0);
const results = ref<ProjectHit[]>([]);
const total = ref(0);
const loading = ref(false);
const cfCategories = ref<{ id: number; name: string }[]>([]);

const types = [
  { key: "mod", label: "模组" },
  { key: "modpack", label: "整合包" },
  { key: "resourcepack", label: "资源包" },
  { key: "shader", label: "光影" },
];

const modrinthCategories: Record<string, string[]> = {
  mod: ["", "fabric", "forge", "quilt", "neoforge", "optimization", "library", "utility", "adventure", "magic", "tech", "decoration", "equipment", "food", "misc", "mobs", "storage", "worldgen"],
  modpack: ["", "fabric", "forge", "quilt", "neoforge", "adventure", "challenge", "combat", "hardcore", "magic", "mini-game", "multiplayer", "optimization", "pvp", "tech", "vanilla-plus"],
  resourcepack: ["", "16x", "32x", "64x", "128x", "256x", "512x", "faithful", "cursed", "modern", "semi-realistic", "simplistic", "themed"],
  shader: ["", "potato", "low", "medium", "high", "ultra", "path-tracing", "complementary", "realistic"],
};

const catOptions = ref<{ label: string; value: string }[]>([]);
const installTarget = ref<ProjectHit | null>(null);
const showInstall = ref(false);

function labelOf(c: string) {
  if (!c) return "全部分类";
  return c.replace(/-/g, " ");
}

async function search() {
  loading.value = true;
  try {
    const res = await api.browse(provider.value, query.value, type.value, category.value, page.value);
    results.value = res.hits;
    total.value = res.total;
  } catch (e) {
    message.error(String(e));
    results.value = [];
  } finally {
    loading.value = false;
  }
}

async function loadCfCategories() {
  if (provider.value !== "curseforge") return;
  try {
    cfCategories.value = (await api.curseforgeCategories(type.value)).categories;
  } catch {
    cfCategories.value = [];
  }
}

function rebuildOptions() {
  if (provider.value === "modrinth") {
    catOptions.value = (modrinthCategories[type.value] ?? []).map((c) => ({
      label: labelOf(c),
      value: c,
    }));
  } else {
    catOptions.value = [
      { label: "全部分类", value: "" },
      ...cfCategories.value.map((c) => ({ label: c.name, value: String(c.id) })),
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

function openInstall(p: ProjectHit) {
  installTarget.value = p;
  showInstall.value = true;
}

function openInstallDep(dep: ProjectDependency) {
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
  showInstall.value = true;
}

onMounted(async () => {
  rebuildOptions();
  await search();
});
</script>

<template>
  <div class="browse">
    <div class="head">
      <h1>内容中心</h1>
      <p class="sub">从 Modrinth 与 CurseForge 一键浏览、安装与升级模组、整合包、资源包和光影</p>
    </div>

    <div class="toolbar glass">
      <div class="provider-switch">
        <button :class="{ active: provider === 'modrinth' }" @click="provider = 'modrinth'">Modrinth</button>
        <button :class="{ active: provider === 'curseforge' }" @click="provider = 'curseforge'">CurseForge</button>
      </div>
      <div class="search-box">
        <IconSearch />
        <input v-model="query" placeholder="搜索内容…（如 sodium / iris / 某整合包）" />
      </div>
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
        v-model:value="category"
        :options="catOptions"
        placeholder="分类"
        class="cat-select"
        size="small"
      />
    </div>

    <div v-if="provider === 'curseforge' && !cfCategories.length && !loading" class="cf-hint glass">
      CurseForge 需要 API Key。请前往
      <a href="https://console.curseforge.com" target="_blank">console.curseforge.com</a>
      免费申请，并在 <router-link to="/settings">设置</router-link> 中填写。
    </div>

    <div v-if="loading" class="center">搜索中…</div>
    <div v-else-if="!results.length" class="center">没有找到相关内容</div>
    <div v-else class="grid">
      <ProjectCard v-for="p in results" :key="p.provider + p.id" :project="p" @install="openInstall" />
    </div>

    <div v-if="total > 20" class="pager">
      <button class="pg" :disabled="page === 0" @click="page--; search()">
        <IconChevronLeft /> 上一页
      </button>
      <span>第 {{ page + 1 }} 页 · 共 {{ total }} 条</span>
      <button class="pg" :disabled="(page + 1) * 20 >= total" @click="page++; search()">
        下一页 <IconChevronRight />
      </button>
    </div>

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
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  flex-wrap: wrap;
}
.provider-switch {
  display: flex;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 9px;
  padding: 3px;
  gap: 2px;
}
.provider-switch button {
  border: none;
  background: transparent;
  color: var(--text-3);
  padding: 6px 14px;
  border-radius: 7px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.provider-switch button.active {
  background: var(--accent);
  color: #1a1208;
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
.cat-select {
  width: 130px;
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
.pager {
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 16px;
  color: var(--text-3);
  font-size: 13px;
}
.pg {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  border-radius: 9px;
  padding: 7px 14px;
  cursor: pointer;
  font-family: inherit;
  font-size: 13px;
}
.pg:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.08);
}
.pg:disabled {
  opacity: 0.4;
  cursor: default;
}
</style>
