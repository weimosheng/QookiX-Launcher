<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { NModal, NSelect, NButton, useMessage } from "naive-ui";
import { api } from "../api";
import { useInstancesStore } from "../stores/instances";
import type { ProjectDependency, ProjectHit, ProjectVersion } from "../types";

const props = defineProps<{
  show: boolean;
  project: ProjectHit | null;
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
  const t = e.target as Node | null;
  if (cardRef.value && t && !cardRef.value.contains(t)) {
    emit("update:show", false);
  }
}
onMounted(() => document.addEventListener("mousedown", onDocMouseDown));
onBeforeUnmount(() => document.removeEventListener("mousedown", onDocMouseDown));

const instances = useInstancesStore();
const router = useRouter();
const message = useMessage();

const versions = ref<ProjectVersion[]>([]);
const loadingVersions = ref(false);
const selectedVersion = ref<string | null>(null);
const selectedInstance = ref<string | null>(null);
const installing = ref(false);
const installMsg = ref("");
const typeFilter = ref<"all" | "release" | "beta" | "alpha">("all");
const deps = ref<ProjectDependency[]>([]);
const loadingDeps = ref(false);

const isModpack = computed(() => props.project?.project_type === "modpack");

const instanceOptions = () =>
  instances.instances.map((i) => ({
    label: `${i.name} (${i.mc_version} ${i.loader === "vanilla" ? "" : i.loader})`,
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
  if (!props.project || props.project.provider !== "modrinth" || !selectedVersion.value) return;
  loadingDeps.value = true;
  try {
    deps.value = await api.projectDependencies(props.project.provider, selectedVersion.value);
  } catch {
    deps.value = [];
  } finally {
    loadingDeps.value = false;
  }
}

watch(selectedVersion, () => loadDeps());

async function loadVersions() {
  if (!props.project) return;
  loadingVersions.value = true;
  selectedVersion.value = null;
  deps.value = [];
  try {
    const res = await api.projectVersions(props.project.provider, props.project.id, "", "");
    versions.value = res.versions;
    if (selectedInstance.value) {
      const inst = instances.get(selectedInstance.value);
      if (inst) {
        const compat = res.versions.find(
          (v) =>
            (v.game_versions ?? []).includes(inst.mc_version) &&
            (inst.loader === "vanilla" || (v.loaders ?? []).includes(inst.loader))
        );
        if (compat) selectedVersion.value = compat.id;
      }
    }
  } catch (e) {
    message.error(String(e));
  } finally {
    loadingVersions.value = false;
  }
}

function resetForProject() {
  if (!props.project) return;
  selectedInstance.value = isModpack.value ? null : (instances.instances[0]?.id ?? null);
  installMsg.value = "";
  typeFilter.value = "all";
  loadVersions();
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
    message.warning("请选择一个版本");
    return;
  }
  installing.value = true;
  try {
    api.installContent(
      selectedInstance.value ?? "",
      props.project.provider,
      props.project.id,
      selectedVersion.value,
      props.project.project_type
    ).catch((e: unknown) => message.error(String(e)));
    message.success("已添加到下载队列");
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
    :on-update:show="(v: boolean) => emit('update:show', v)"
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
            <div class="id-type-tabs">
              <button :class="{ active: typeFilter === 'all' }" @click="typeFilter = 'all'">全部</button>
              <button :class="{ active: typeFilter === 'release' }" @click="typeFilter = 'release'">正式版</button>
              <button :class="{ active: typeFilter === 'beta' }" @click="typeFilter = 'beta'">测试版</button>
              <button :class="{ active: typeFilter === 'alpha' }" @click="typeFilter = 'alpha'">先行版</button>
            </div>
          </div>
          <div v-if="loadingVersions" class="id-loading">加载中…</div>
          <div v-else class="id-ver-list">
            <button
              v-for="v in filteredVersions.slice(0, 40)"
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
        <div v-if="deps.length && !isModpack" class="id-deps">
          <span class="id-deps-label">前置依赖</span>
          <div class="id-deps-list">
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
        <n-button type="primary" :loading="installing" :disabled="!selectedVersion" @click="install">
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
  background: rgba(255, 255, 255, 0.06);
}
.id-info {
  min-width: 0;
  flex: 1;
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
  color: #8b8e9c;
  margin-bottom: 6px;
}
.id-type {
  background: rgba(232, 154, 75, 0.16);
  color: #e89a4b;
  border-radius: 6px;
  padding: 0 7px;
  font-weight: 600;
}
.id-desc {
  font-size: 12px;
  color: #8b8e9c;
  line-height: 1.5;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.id-site {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  background: rgba(255, 255, 255, 0.05);
  color: #c6c8d2;
  border-radius: 8px;
  padding: 6px 11px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  flex-shrink: 0;
}
.id-site:hover {
  color: #e89a4b;
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
  color: #c6c8d2;
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
  display: flex;
  gap: 3px;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 8px;
  padding: 2px;
}
.id-type-tabs button {
  border: none;
  background: transparent;
  color: #8b8e9c;
  padding: 4px 10px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.id-type-tabs button.active {
  background: #e89a4b;
  color: #1a1208;
}
.id-loading {
  padding: 20px;
  text-align: center;
  color: #8b8e9c;
  font-size: 13px;
}
.id-ver-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 200px;
  overflow-y: auto;
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 10px;
  padding: 6px;
  background: rgba(255, 255, 255, 0.02);
}
.id-ver-row {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid transparent;
  background: transparent;
  color: #c6c8d2;
  padding: 6px 10px;
  border-radius: 8px;
  cursor: pointer;
  font-family: inherit;
  text-align: left;
}
.id-ver-row:hover {
  background: rgba(255, 255, 255, 0.05);
}
.id-ver-row.active {
  border-color: rgba(232, 154, 75, 0.5);
  background: rgba(232, 154, 75, 0.14);
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
  background: rgba(232, 154, 75, 0.16);
  color: #e89a4b;
}
.id-ver-type.alpha {
  background: rgba(229, 83, 75, 0.14);
  color: #e5534b;
}
.id-ver-mc {
  font-size: 11px;
  color: #8b8e9c;
  flex: 1;
  text-align: right;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.id-ver-date {
  font-size: 11px;
  color: #8b8e9c;
  flex-shrink: 0;
}
.id-empty {
  text-align: center;
  color: #8b8e9c;
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
  color: #8b8e9c;
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
  border: 1px solid rgba(255, 255, 255, 0.1);
  background: rgba(255, 255, 255, 0.04);
  color: #c6c8d2;
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
  background: rgba(255, 255, 255, 0.08);
  color: #8b8e9c;
}
.id-deps-loading {
  font-size: 12px;
  color: #8b8e9c;
}
.id-msg {
  font-size: 13px;
  color: #e89a4b;
}
.id-noinst {
  font-size: 12px;
  color: #8b8e9c;
}
.id-noinst a {
  color: #e89a4b;
  cursor: pointer;
}
.id-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
</style>
