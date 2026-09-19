<script setup lang="ts">
import { computed, onMounted, ref, watch, inject } from "vue";
import { useRouter } from "vue-router";
import { useInstancesStore } from "../stores/instances";
import InstanceCard from "../components/InstanceCard.vue";
import PlaytimeCard from "../components/PlaytimeCard.vue";
import { useMessage, NModal, NButton, NInput } from "naive-ui";
import { useI18n } from "vue-i18n";
import { open as dialogOpen } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import type { Instance, InstanceGroup } from "../types";
import {
  IconChevronDown,
  IconChevronRight,
  IconGrid,
  IconPlus,
  IconTrash,
} from "../components/icons";

const instances = useInstancesStore();
const router = useRouter();
const message = useMessage();
const { t } = useI18n();

const FILTER_KEY = "qookix.instances.filter";
const COLLAPSE_KEY = "qookix.instances.collapsedGroups";

const PALETTE = [
  "#e89a4b",
  "#5ab0ff",
  "#7ad08a",
  "#c78aff",
  "#ff7a90",
  "#ffd166",
  "#4ecdc4",
  "#a0a4b8",
];

/** "all" | "ungrouped" | 分组 id */
const filter = ref<string>("all");
const collapsed = ref<Record<string, boolean>>({});
const movingInstance = ref<Instance | null>(null);

type GroupDialog = {
  mode: "create" | "rename";
  id: string | null;
  name: string;
  color: string | null;
};
const groupDialog = ref<GroupDialog | null>(null);
const groupSaving = ref(false);

const confirmState = ref<{
  title: string;
  content: string;
  positiveText: string;
  onOk: () => void | Promise<void>;
} | null>(null);
const confirmLoading = ref(false);

// ---- 导入实例分享包 ----
const importingPack = ref(false);
async function importPack() {
  const file = await dialogOpen({
    multiple: false,
    filters: [{ name: t("instances.packFilterName"), extensions: ["qkxinst"] }],
  });
  if (!file) return;
  importingPack.value = true;
  try {
    const r = await api.importInstancePack(file as string);
    message.success(
      r.pendingDownloads > 0
        ? t("instances.importSuccessWithDownloads", { name: r.instance.name, count: r.pendingDownloads })
        : t("instances.importSuccessInstalling", { name: r.instance.name })
    );
    await instances.refresh();
    router.push(`/instance/${r.instance.id}`);
  } catch (e) {
    message.error(String(e));
  } finally {
    importingPack.value = false;
  }
}

async function handleConfirm() {
  if (!confirmState.value) return;
  confirmLoading.value = true;
  try {
    await confirmState.value.onOk();
    confirmState.value = null;
  } finally {
    confirmLoading.value = false;
  }
}

function loadCollapsed() {
  try {
    const raw = localStorage.getItem(COLLAPSE_KEY);
    collapsed.value = raw ? (JSON.parse(raw) as Record<string, boolean>) : {};
  } catch {
    collapsed.value = {};
  }
}

function saveCollapsed() {
  try {
    localStorage.setItem(COLLAPSE_KEY, JSON.stringify(collapsed.value));
  } catch {
    /* localStorage 不可用时忽略 */
  }
}

onMounted(() => {
  loadCollapsed();
  const saved = localStorage.getItem(FILTER_KEY);
  if (saved) filter.value = saved;
  instances.load();
});

watch(filter, (v) => {
  if (v === "all") localStorage.removeItem(FILTER_KEY);
  else localStorage.setItem(FILTER_KEY, v);
});

// 分组被删除后回退到"全部"，避免出现空页面
watch(
  () => instances.groups.map((g) => g.id).join(","),
  () => {
    if (filter.value === "all" || filter.value === "ungrouped") return;
    if (!instances.groups.some((g) => g.id === filter.value)) filter.value = "all";
  }
);

function toggleCollapse(id: string) {
  collapsed.value = { ...collapsed.value, [id]: !collapsed.value[id] };
  saveCollapsed();
}

/** 全部视图下的展示顺序：各分组（按后端顺序）+ 未分组收尾 */
const sections = computed(() => {
  const list = instances.groups.map((g) => ({
    key: g.id,
    group: g as InstanceGroup | null,
    name: g.name,
    color: g.color,
    items: instances.inGroup(g.id),
  }));
  list.push({
    key: "__ungrouped__",
    group: null,
    name: t("instances.ungrouped"),
    color: null,
    items: instances.ungrouped,
  });
  return list;
});

const filtered = computed<Instance[]>(() => {
  if (filter.value === "all") return instances.instances;
  if (filter.value === "ungrouped") return instances.ungrouped;
  return instances.inGroup(filter.value);
});

const totalCount = computed(() => instances.instances.length);

function openCreateGroup() {
  groupDialog.value = {
    mode: "create",
    id: null,
    name: "",
    color: PALETTE[instances.groups.length % PALETTE.length],
  };
}

// 监听标题栏“新建分组”按钮的触发信号
const groupDialogRequest = inject<{ value: number }>("groupDialogRequest", { value: 0 });
watch(groupDialogRequest, () => openCreateGroup());

function openRenameGroup(g: InstanceGroup) {
  groupDialog.value = { mode: "rename", id: g.id, name: g.name, color: g.color };
}

async function saveGroupDialog() {
  const d = groupDialog.value;
  if (!d) return;
  const name = d.name.trim();
  if (!name) {
    message.warning(t("instances.enterGroupName"));
    return;
  }
  groupSaving.value = true;
  try {
    if (d.mode === "create") {
      await instances.createGroup(name, d.color);
      message.success(t("instances.groupCreated", { name }));
    } else if (d.id) {
      await instances.renameGroup(d.id, name, d.color);
      message.success(t("instances.groupUpdated"));
    }
    groupDialog.value = null;
  } catch (e) {
    message.error(String(e));
  } finally {
    groupSaving.value = false;
  }
}

function confirmDeleteGroup(g: InstanceGroup) {
  const count = instances.inGroup(g.id).length;
  confirmState.value = {
    title: t("instances.deleteGroup"),
    content: count
      ? t("instances.deleteGroupConfirmHasItems", { name: g.name, count })
      : t("instances.deleteGroupConfirmEmpty", { name: g.name }),
    positiveText: t("instances.deleteGroup"),
    onOk: async () => {
      try {
        await instances.deleteGroup(g.id);
        message.success(t("instances.groupDeleted"));
      } catch (e) {
        message.error(String(e));
      }
    },
  };
}

async function moveTo(groupId: string | null) {
  const inst = movingInstance.value;
  if (!inst) return;
  try {
    await instances.moveToGroup(inst.id, groupId);
    const name = groupId ? (instances.groupById(groupId)?.name ?? "") : t("instances.ungrouped");
    message.success(t("instances.movedTo", { name: inst.name, target: name }));
    movingInstance.value = null;
  } catch (e) {
    message.error(String(e));
  }
}
</script>

<template>
  <div id="instances-root" class="instances-view">
    <div v-if="instances.loading" class="loading">{{ t("instances.loading") }}</div>

    <template v-else-if="totalCount">
      <PlaytimeCard />
      <div class="toolbar">
        <div class="chips">
          <button
            class="chip"
            :class="{ active: filter === 'all' }"
            @click="filter = 'all'"
          >
            {{ t("instances.filterAll") }} <span class="chip-count">{{ totalCount }}</span>
          </button>
          <button
            v-for="g in instances.groups"
            :key="g.id"
            class="chip"
            :class="{ active: filter === g.id }"
            @click="filter = g.id"
          >
            <i class="dot" :style="{ background: g.color || 'var(--accent)' }"></i>
            {{ g.name }}
            <span class="chip-count">{{ instances.inGroup(g.id).length }}</span>
          </button>
          <button
            class="chip"
            :class="{ active: filter === 'ungrouped' }"
            @click="filter = 'ungrouped'"
          >
            {{ t("instances.ungrouped") }} <span class="chip-count">{{ instances.ungrouped.length }}</span>
          </button>
          <button class="chip import-chip" :disabled="importingPack" @click="importPack">
            <IconPlus /> {{ t("instances.importPack") }}
          </button>
        </div>
      </div>

      <!-- 分组视图 -->
      <div v-if="filter === 'all'" class="groups">
        <section v-for="s in sections" :key="s.key" class="group-block">
          <header class="group-head">
            <button class="group-toggle" @click="toggleCollapse(s.key)">
              <IconChevronDown v-if="!collapsed[s.key]" />
              <IconChevronRight v-else />
              <i class="dot" :style="{ background: s.color || 'var(--text-3)' }"></i>
              <span class="group-name">{{ s.name }}</span>
              <span class="group-count">{{ s.items.length }}</span>
            </button>
            <div v-if="s.group" class="group-ops">
              <button class="op" :title="t('instances.renameGroup')" @click="openRenameGroup(s.group)">
                {{ t("instances.rename") }}
              </button>
              <button class="op danger" :title="t('instances.deleteGroup')" @click="confirmDeleteGroup(s.group)">
                <IconTrash />
              </button>
            </div>
          </header>
          <div v-if="!collapsed[s.key]" class="grid">
            <InstanceCard
              v-for="inst in s.items"
              :key="inst.id"
              :instance="inst"
              @move="movingInstance = $event"
            />
            <p v-if="!s.items.length" class="group-empty">{{ t("instances.groupEmpty") }}</p>
          </div>
        </section>
      </div>

      <!-- 单分组 / 未分组视图 -->
      <div v-else class="grid">
        <InstanceCard
          v-for="inst in filtered"
          :key="inst.id"
          :instance="inst"
          @move="movingInstance = $event"
        />
        <p v-if="!filtered.length" class="group-empty">{{ t("instances.filteredGroupEmpty") }}</p>
      </div>
    </template>

    <div v-else class="empty glass">
      <div class="empty-icon"><IconGrid /></div>
      <p>{{ t("instances.emptyHint") }}</p>
      <button class="btn primary" @click="router.push('/create')">{{ t("instances.createFirst") }}</button>
    </div>

    <!-- 新建 / 重命名分组 -->
    <n-modal
      :show="groupDialog !== null"
      preset="card"
      :title="groupDialog?.mode === 'create' ? t('instances.newGroup') : t('instances.renameGroup')"
      style="width: 420px; max-width: 92vw"
      @update:show="(v: boolean) => { if (!v) groupDialog = null; }"
    >
      <div v-if="groupDialog" class="dialog-body">
        <label class="field">
          <span>{{ t("instances.nameLabel") }}</span>
          <n-input
            v-model:value="groupDialog.name"
            :placeholder="t('instances.namePlaceholder')"
            maxlength="40"
            @keydown.enter="saveGroupDialog"
          />
        </label>
        <div class="field">
          <span>{{ t("instances.colorLabel") }}</span>
          <div class="palette">
            <button
              v-for="c in PALETTE"
              :key="c"
              class="swatch"
              :class="{ on: groupDialog.color === c }"
              :style="{ background: c }"
              :title="c"
              @click="groupDialog.color = c"
            ></button>
          </div>
        </div>
        <div class="dialog-foot">
          <n-button @click="groupDialog = null">{{ t("instances.cancel") }}</n-button>
          <n-button type="primary" :loading="groupSaving" @click="saveGroupDialog">{{ t("instances.save") }}</n-button>
        </div>
      </div>
    </n-modal>

    <!-- 移动实例到分组 -->
    <n-modal
      :show="movingInstance !== null"
      preset="card"
      :title="t('instances.moveToGroup')"
      style="width: 380px; max-width: 92vw"
      @update:show="(v: boolean) => { if (!v) movingInstance = null; }"
    >
      <div v-if="movingInstance" class="move-list">
        <p class="move-hint">{{ movingInstance.name }}</p>
        <button
          class="move-item"
          :class="{ current: !movingInstance.group }"
          @click="moveTo(null)"
        >
          <i class="dot" style="background: var(--text-3)"></i>
          <span>{{ t("instances.ungrouped") }}</span>
        </button>
        <button
          v-for="g in instances.groups"
          :key="g.id"
          class="move-item"
          :class="{ current: movingInstance.group === g.id }"
          @click="moveTo(g.id)"
        >
          <i class="dot" :style="{ background: g.color || 'var(--accent)' }"></i>
          <span>{{ g.name }}</span>
        </button>
        <button class="move-item add" @click="openCreateGroup">
          <IconPlus /> {{ t("instances.newGroup") }}
        </button>
      </div>
    </n-modal>

    <n-modal
      :show="confirmState !== null"
      preset="card"
      :title="confirmState?.title ?? ''"
      style="width: 420px; max-width: 92vw"
      @update:show="(v: boolean) => { if (!v) confirmState = null; }"
    >
      <div v-if="confirmState" class="dialog-body">
        <div class="confirm-text">{{ confirmState.content }}</div>
        <div class="dialog-foot">
          <n-button @click="confirmState = null">{{ t("instances.cancel") }}</n-button>
          <n-button type="error" :loading="confirmLoading" @click="handleConfirm">
            {{ confirmState.positiveText }}
          </n-button>
        </div>
      </div>
    </n-modal>
  </div>
</template>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  margin-bottom: 18px;
  flex-wrap: wrap;
}
.chips {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: var(--w-04);
  color: var(--text-2);
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.15s;
}
.chip:hover {
  background: var(--w-08);
  color: var(--text-1);
}
.chip.active {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-soft);
}
.chip-count {
  font-size: 11px;
  opacity: 0.7;
}
.dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  flex-shrink: 0;
  display: inline-block;
}
.toolbar-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}
.btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: none;
  border-radius: 10px;
  padding: 8px 14px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.15s;
}
.btn.sm {
  padding: 7px 12px;
  font-size: 13px;
}
.btn.primary {
  background: linear-gradient(135deg, var(--accent), var(--accent-deep));
  color: #1a1208;
}
.btn.primary:hover {
  filter: brightness(1.08);
}
.btn.ghost {
  background: var(--w-06);
  color: var(--text-1);
  border: 1px solid var(--border);
}
.btn.ghost:hover {
  background: var(--w-10);
}
.groups {
  display: flex;
  flex-direction: column;
  gap: 18px;
}
.group-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 12px;
}
.group-toggle {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  background: none;
  border: none;
  padding: 4px 0;
  color: var(--text-1);
  font-size: 15px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.group-toggle:hover {
  color: var(--accent);
}
.group-count {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-3);
}
.group-ops {
  display: flex;
  align-items: center;
  gap: 6px;
  opacity: 0;
  transition: opacity 0.15s;
}
.group-block:hover .group-ops {
  opacity: 1;
}
.op {
  background: transparent;
  border: 1px solid var(--border);
  color: var(--text-3);
  border-radius: 8px;
  padding: 3px 9px;
  font-size: 12px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  font-family: inherit;
}
.op:hover {
  color: var(--text-1);
  background: var(--w-08);
}
.op.danger:hover {
  color: #e5534b;
  border-color: var(--danger-50);
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 16px;
}
.group-empty {
  grid-column: 1 / -1;
  padding: 18px;
  text-align: center;
  color: var(--text-3);
  font-size: 13px;
  border: 1px dashed var(--border);
  border-radius: 12px;
}
.loading {
  padding: 60px;
  text-align: center;
  color: var(--text-3);
}
.empty {
  padding: 50px;
  text-align: center;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  color: var(--text-3);
}
.empty-icon {
  font-size: 34px;
  opacity: 0.6;
}
.dialog-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 8px;
  font-size: 13px;
  color: var(--text-2);
}
.palette {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.swatch {
  width: 24px;
  height: 24px;
  border-radius: 50%;
  border: 2px solid transparent;
  cursor: pointer;
  padding: 0;
}
.swatch.on {
  border-color: var(--text-1);
  box-shadow: 0 0 0 2px var(--w-12);
}
.dialog-foot {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.confirm-text {
  font-size: 14px;
  color: var(--text-2);
  line-height: 1.6;
}
.move-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.move-hint {
  margin: 0 0 6px;
  font-size: 13px;
  color: var(--text-3);
}
.move-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 12px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: var(--w-04);
  color: var(--text-1);
  font-size: 14px;
  cursor: pointer;
  font-family: inherit;
  text-align: left;
}
.move-item:hover {
  background: var(--w-09);
}
.move-item.current {
  border-color: var(--accent);
  color: var(--accent);
}
.move-item.add {
  justify-content: center;
  color: var(--text-2);
  border-style: dashed;
}
</style>
