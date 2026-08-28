<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { NModal, NSelect, NButton, useMessage } from "naive-ui";
import { api } from "../api";
import { useInstancesStore } from "../stores/instances";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import { IconCopy, IconExternal, IconGlobe } from "./icons";
import type { ProjectDependency, ProjectHit, ProjectVersion } from "../types";

const props = defineProps<{
  show: boolean;
  project: ProjectHit | null;
  defaultInstance?: string | null;
}>();
const emit = defineEmits<{
  "update:show": [v: boolean];
  "install-dep": [dep: ProjectDependency];
}>();

// 点击弹窗卡片外部即关闭（用 document 委托，不依赖 naive-ui 的 mask 机制）。
// 用 mousedown 而非 click：打开弹窗的那次按下发生在 show 变 true 之前，
// 会被 `!props.show` 拦截，避免弹窗刚打开就被自身触发的点击冒泡关掉。
const cardRef = ref<HTMLElement | null>(null);
function onDocMouseDown(e: MouseEvent) {
  if (!props.show) return;
  const t = e.target as Element | null;
  if (!t) return;
  // 点击弹窗卡片内部、或 naive-ui 的下拉/弹层（teleport 到 body）都不应关闭
  if (cardRef.value?.contains(t)) return;
  if (t.closest(".v-binder-follower-container, .n-base-select-menu, .n-popover, .n-dropdown")) return;
  emit("update:show", false);
}
onMounted(() => document.addEventListener("mousedown", onDocMouseDown));
onBeforeUnmount(() => document.removeEventListener("mousedown", onDocMouseDown));

const instances = useInstancesStore();
const router = useRouter();
const message = useMessage();

async function copyName() {
  if (!props.project?.title) return;
  try {
    await navigator.clipboard.writeText(props.project.title);
    message.success("已复制名称");
  } catch {
    message.error("复制失败");
  }
}

const versions = ref<ProjectVersion[]>([]);
const loadingVersions = ref(false);
const selectedVersion = ref<string | null>(null);
const selectedInstance = ref<string | null>(null);
const installing = ref(false);
const installMsg = ref("");
const typeFilter = ref<"all" | "release" | "beta" | "alpha">("all");

// 版本类型 tabs 的滑动高亮指示器
const typeTabBox = ref<HTMLElement | null>(null);
const { indicatorStyle: typeTabIndicatorStyle, refresh: refreshTypeTabIndicator } = useSlidingIndicator(
  typeTabBox,
  () => Array.from(typeTabBox.value?.querySelectorAll<HTMLElement>(".id-type-tabs button") ?? []),
  () => ["all", "release", "beta", "alpha"].indexOf(typeFilter.value),
  { axis: "horizontal" }
);
watch(typeFilter, () => nextTick(() => refreshTypeTabIndicator()));
const deps = ref<ProjectDependency[]>([]);
const loadingDeps = ref(false);

const isModpack = computed(() => props.project?.project_type === "modpack");

const mcWikiUrl = ref("");
const sourceUrl = computed(() => {
  const p = props.project;
  if (!p) return "";
  if (p.provider === "modrinth") {
    return `https://modrinth.com/${p.project_type}/${p.slug}`;
  }
  if (p.provider === "curseforge") {
    const kind = p.project_type === "modpack" ? "modpacks"
      : p.project_type === "resourcepack" ? "texture-packs"
      : p.project_type === "shader" ? "shaders"
      : "mc-mods";
    return `https://www.curseforge.com/minecraft/${kind}/${p.slug}`;
  }
  return "";
});

async function loadMcWikiUrl() {
  if (!props.project) return;
  try {
    mcWikiUrl.value = await api.mcWikiUrl(props.project.title, props.project.slug, props.project.provider);
  } catch {
    mcWikiUrl.value = "";
  }
}

const instanceOptions = () =>
  instances.instances.filter((i) => i.loader !== "vanilla").map((i) => ({
    label: `${i.name} (${i.mc_version} ${i.loader})`,
    value: i.id,
  }));

function versionType(v: ProjectVersion): string {
  // modrinth: version_type; curseforge: release_type (1=release,2=beta,3=alpha)
  if (v.version_type) return v.version_type;
  if (v.release_type === 2) return "beta";
  if (v.release_type === 3) return "alpha";
  return "release";
}

function typeLabel(t: string) {
  return { release: "正式版", beta: "测试版", alpha: "先行版" }[t] ?? "正式版";
}

const filteredVersions = computed(() => {
  if (typeFilter.value === "all") return versions.value;
  return versions.value.filter((v) => versionType(v) === typeFilter.value);
});

async function loadDeps() {
  deps.value = [];
  if (!props.project || !selectedVersion.value) return;
  loadingDeps.value = true;
  try {
    deps.value = await api.projectDependencies(props.project.provider, props.project.id, selectedVersion.value);
  } catch {
    deps.value = [];
  } finally {
    loadingDeps.value = false;
  }
}

watch(selectedVersion, () => { if (!isModpack.value) loadDeps(); });

async function loadVersions() {
  if (!props.project) return;
  loadingVersions.value = true;
  selectedVersion.value = null;
  deps.value = [];
  try {
    // 已选实例时用实例的 MC 版本 + 加载器请求上游，让 Modrinth/CurseForge
    // 只返回兼容的版本，从而 API 本身返回更少的数据（而非全量拉取后本地过滤）
    const inst = selectedInstance.value ? instances.get(selectedInstance.value) : null;
    const mc = inst?.mc_version ?? "";
    const ld = inst && inst.loader !== "vanilla" ? inst.loader : "";
    const res = await api.projectVersions(props.project.provider, props.project.id, mc, ld);
    versions.value = res.versions;
    if (inst && !res.versions.length) {
      // 上游按实例筛选无结果时，回退拉取全部版本供选择
      const all = await api.projectVersions(props.project.provider, props.project.id, "", "");
      versions.value = all.versions;
    }
    const picked = versions.value.find((v) => versionType(v) === "release") ?? versions.value[0];
    if (picked) selectedVersion.value = picked.id;
  } catch (e) {
    message.error(String(e));
  } finally {
    loadingVersions.value = false;
  }
}

// 切换实例时按新实例的 MC 版本/加载器重新拉取
watch(selectedInstance, () => {
  if (props.show && props.project) loadVersions();
});

function resetForProject() {
  if (!props.project) return;
  const nonVanilla = instances.instances.filter((i) => i.loader !== "vanilla");
  const pref = props.defaultInstance && nonVanilla.some((i) => i.id === props.defaultInstance)
    ? props.defaultInstance
    : nonVanilla[0]?.id ?? null;
  selectedInstance.value = isModpack.value ? null : pref;
  installMsg.value = "";
  typeFilter.value = "all";
  loadVersions();
  loadMcWikiUrl();
}

async function onOpen() {
  if (!props.show) return;
  resetForProject();
}

// 弹窗内切换项目（点击前置依赖）时重新加载版本与依赖
watch(
  () => props.project,
  (proj, oldProj) => {
    if (proj && oldProj && proj !== oldProj) resetForProject();
  }
);

function depLabel(t: string) {
  return { required: "必需", optional: "可选", incompatible: "不兼容", embedded: "内嵌" }[t] ?? t;
}

async function install() {
  if (!props.project) return;
  if (!isModpack.value && !selectedInstance.value) {
    message.warning("请选择一个实例");
    return;
  }
  if (!selectedVersion.value) {
    message.warning(versions.value.length ? "请选择一个版本" : "该 mod 没有可用版本，可能不兼容当前实例或加载失败");
    return;
  }
  installing.value = true;
  message.success("已添加到下载队列");
  try {
    await api.installContent(
      selectedInstance.value ?? "",
      props.project.provider,
      props.project.id,
      selectedVersion.value,
      props.project.project_type
    );
    message.success("安装完成");
    emit("update:show", false);
  } catch (e) {
    message.error(String(e));
  } finally {
    installing.value = false;
  }
}

function fmtDate(s: string) {
  if (!s) return "";
  const d = new Date(s);
  return d.toLocaleDateString();
}
</script>

<template>
  <n-modal
    :show="props.show"
    preset="card"
    :title="props.project?.title ?? '安装内容'"
    style="width: 640px; max-width: 94vw"
    :mask-closable="true"
    :close-on-esc="true"
    @update:show="(v: boolean) => emit('update:show', v)"
    @mask-click="() => emit('update:show', false)"
    @after-enter="onOpen"
  >
    <div v-if="props.project" ref="cardRef" class="id-modal">
      <div class="id-head">
        <img v-if="props.project.icon_url" :src="props.project.icon_url" class="id-icon" alt="" />
        <div class="id-info">
          <div class="id-title">{{ props.project.title }}</div>
          <div class="id-meta">
            <span class="id-author">{{ props.project.author }}</span>
            <span class="id-dl">{{ (props.project.downloads / 10000).toFixed(1) }} 万下载</span>
            <span class="id-type">{{ props.project.project_type }}</span>
          </div>
          <div class="id-desc">{{ props.project.description }}</div>
          <div class="id-links">
            <a v-if="mcWikiUrl" :href="mcWikiUrl" target="_blank" class="id-link"><IconGlobe /> MC百科</a>
            <a v-if="sourceUrl" :href="sourceUrl" target="_blank" class="id-link"><IconExternal /> 在浏览器打开</a>
            <button class="id-link" @click="copyName"><IconCopy /> 复制名称</button>
          </div>
        </div>
      </div>

      <div class="id-form">
        <label v-if="!isModpack" class="id-field">
          <span>安装到实例</span>
          <n-select v-model:value="selectedInstance" :options="instanceOptions()" placeholder="选择实例" />
        </label>

        <div v-if="isModpack" class="id-field">
          <span class="id-modpack-hint">整合包将自动创建新实例</span>
        </div>

        <div class="id-field">
          <div class="id-ver-head">
            <span>选择版本</span>
            <div ref="typeTabBox" class="id-type-tabs">
              <div class="indicator" :style="typeTabIndicatorStyle"></div>
              <button :class="{ active: typeFilter === 'all' }" @click="typeFilter = 'all'">全部</button>
              <button :class="{ active: typeFilter === 'release' }" @click="typeFilter = 'release'">正式版</button>
              <button :class="{ active: typeFilter === 'beta' }" @click="typeFilter = 'beta'">测试版</button>
              <button :class="{ active: typeFilter === 'alpha' }" @click="typeFilter = 'alpha'">先行版</button>
            </div>
          </div>
          <div v-if="loadingVersions" class="id-loading">加载中…</div>
          <div v-else class="id-ver-list">
            <button
              v-for="v in filteredVersions.slice(0, 80)"
              :key="v.id"
              class="id-ver-row"
              :class="{ active: selectedVersion === v.id }"
              @click="selectedVersion = v.id"
            >
              <span class="id-ver-num mono">{{ v.version_number }}</span>
              <span class="id-ver-type" :class="versionType(v)">{{ typeLabel(versionType(v)) }}</span>
              <span class="id-ver-mc">{{ (v.game_versions ?? []).slice(-2).join(", ") }}</span>
              <span class="id-ver-date">{{ fmtDate(v.date_published) }}</span>
            </button>
            <div v-if="!filteredVersions.length" class="id-empty">该分类下没有版本</div>
          </div>
        </div>

        <!-- dependencies -->
        <div v-if="(deps.length || loadingDeps) && !isModpack" class="id-deps">
          <span class="id-deps-label">前置依赖</span>
          <div v-if="!loadingDeps" class="id-deps-list">
            <button
              v-for="d in deps"
              :key="d.projectId"
              class="id-dep-chip"
              :class="d.dependencyType"
              :title="`查看 ${d.title} 详情`"
              @click="emit('install-dep', d)"
            >
              <span class="id-dep-tag">{{ depLabel(d.dependencyType) }}</span>
              {{ d.title }}
            </button>
          </div>
          <div v-if="loadingDeps" class="id-deps-loading">正在查询前置…</div>
        </div>
      </div>

      <div v-if="installMsg" class="id-msg">{{ installMsg }}</div>
      <div v-if="!isModpack && !instances.instances.length" class="id-noinst">
        还没有实例，<a @click="emit('update:show', false); router.push('/instances')">先去创建实例</a>
      </div>
    </div>
    <template #footer>
      <div class="id-footer">
        <n-button @click="emit('update:show', false)">关闭</n-button>
        <n-button type="primary" :loading="installing" @click="install">
          一键安装
        </n-button>
      </div>
    </template>
  </n-modal>
</template>

<style>
/* modal content is teleported to <body>; keep styles global */
.id-modal {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.id-head {
  display: flex;
  gap: 14px;
  align-items: flex-start;
}
.id-icon {
  width: 56px;
  height: 56px;
  border-radius: 12px;
  object-fit: cover;
  flex-shrink: 0;
  background: var(--panel);
}
.id-title {
  font-size: 16px;
  font-weight: 700;
  margin-bottom: 4px;
}
.id-meta {
  display: flex;
  gap: 10px;
  font-size: 12px;
  color: var(--text-3);
  margin-bottom: 6px;
}
.id-type {
  background: var(--accent-soft);
  color: var(--accent);
  border-radius: 6px;
  padding: 0 7px;
  font-weight: 600;
}
.id-desc {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.5;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.id-links {
  display: flex;
  gap: 8px;
  margin-top: 6px;
}
.id-link {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--text-3);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 7px;
  padding: 3px 9px;
  text-decoration: none;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.id-link:hover {
  color: var(--accent, #e89a4b);
  border-color: rgba(232, 154, 75, 0.45);
}
.id-site {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border: 1px solid var(--border);
  background: var(--panel);
  color: var(--text-2);
  border-radius: 8px;
  padding: 6px 11px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  flex-shrink: 0;
}
.id-site:hover {
  color: var(--accent);
  border-color: rgba(232, 154, 75, 0.45);
}
.id-form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.id-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
  color: var(--text-2);
}
.id-modpack-hint {
  color: var(--accent, #50c878);
  font-weight: 600;
  font-size: 13px;
}
.id-ver-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.id-type-tabs {
  position: relative;
  display: flex;
  gap: 3px;
  background: var(--panel);
  border-radius: 8px;
  padding: 2px;
}
.id-type-tabs .indicator {
  position: absolute;
  top: 2px;
  bottom: 2px;
  border-radius: 6px;
  background: var(--accent-soft);
  pointer-events: none;
}
.id-type-tabs button {
  border: none;
  background: transparent;
  color: var(--text-3);
  padding: 4px 10px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.id-type-tabs button.active {
  color: var(--accent);
}
.id-loading {
  padding: 20px;
  text-align: center;
  color: var(--text-3);
  font-size: 13px;
}
.id-ver-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 200px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 6px;
  background: var(--panel);
}
.id-ver-row {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-2);
  padding: 6px 10px;
  border-radius: 8px;
  cursor: pointer;
  font-family: inherit;
  text-align: left;
}
.id-ver-row:hover {
  background: var(--panel);
}
.id-ver-row.active {
  border-color: rgba(232, 154, 75, 0.5);
  background: var(--accent-soft);
}
.id-ver-num {
  font-size: 12px;
  font-weight: 600;
  min-width: 0;
  flex-shrink: 1;
}
.id-ver-type {
  font-size: 10px;
  padding: 1px 7px;
  border-radius: 6px;
  font-weight: 600;
  flex-shrink: 0;
}
.id-ver-type.release {
  background: rgba(78, 201, 160, 0.14);
  color: #4ec9a0;
}
.id-ver-type.beta {
  background: var(--accent-soft);
  color: var(--accent);
}
.id-ver-type.alpha {
  background: rgba(229, 83, 75, 0.14);
  color: #e5534b;
}
.id-ver-mc {
  font-size: 11px;
  color: var(--text-3);
  flex: 1;
  text-align: right;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.id-ver-date {
  font-size: 11px;
  color: var(--text-3);
  flex-shrink: 0;
}
.id-empty {
  text-align: center;
  color: var(--text-3);
  font-size: 13px;
  padding: 16px 0;
}
.id-deps {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.id-deps-label {
  font-size: 12px;
  font-weight: 700;
  color: var(--text-3);
}
.id-deps-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.id-dep-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--border);
  background: var(--panel);
  color: var(--text-2);
  border-radius: 8px;
  padding: 4px 10px;
  font-size: 12px;
  cursor: pointer;
  font-family: inherit;
}
.id-dep-chip:hover {
  border-color: rgba(232, 154, 75, 0.45);
}
.id-dep-tag {
  font-size: 10px;
  font-weight: 700;
  padding: 0 6px;
  border-radius: 5px;
}
.id-dep-chip.required .id-dep-tag {
  background: rgba(229, 83, 75, 0.16);
  color: #e5534b;
}
.id-dep-chip.optional .id-dep-tag {
  background: rgba(90, 162, 240, 0.15);
  color: #7cb8f5;
}
.id-dep-chip.incompatible .id-dep-tag {
  background: var(--border);
  color: var(--text-3);
}
.id-deps-loading {
  font-size: 12px;
  color: var(--text-3);
}
.id-msg {
  font-size: 13px;
  color: var(--accent);
}
.id-noinst {
  font-size: 12px;
  color: var(--text-3);
}
.id-noinst a {
  color: var(--accent);
  cursor: pointer;
}
.id-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
</style>
