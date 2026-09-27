<script setup lang="ts">
/**
 * 类 NBTExplorer 的节点树，world 属性（level.dat 的 Data 层）与单个区块共用。
 * 支持展开 / 编辑值 / 新增子节点 / 删除（删除需二次确认）/ 右键菜单 / 复制路径。
 * 改值会把节点类型一并告诉后端——NBT 里 `1` 是 Byte 还是 Int 由类型决定，
 * 判错类型会让游戏读不出数据。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { NSelect, useDialog, useMessage } from "naive-ui";
import { api } from "../../api";
import { copyText } from "../../utils/clipboard";
import { useNbtLabels } from "../../composables/useNbtLabels";
import type { NbtNode } from "../../types";
import TreeNode from "./TreeNode.vue";

const props = defineProps<{
  instanceId: string;
  world: string;
  running: boolean;
  readonly?: boolean;
  /** 表单模式改动过的字段名，用于高亮 */
  highlight?: string[];
  /** 传了就是编辑区块（.mca），否则编辑 level.dat 的世界数据层 */
  chunk?: { dim: string; cx: number; cz: number } | null;
}>();
const emit = defineEmits<{ (e: "backed-up", name: string): void }>();

const { t } = useI18n();
const { fieldLabel } = useNbtLabels();
const message = useMessage();
const dialog = useDialog();

const root = ref<NbtNode | null>(null);
const loading = ref(false);
const expanded = ref<Set<string>>(new Set());
const editing = ref<{ path: string[]; node: NbtNode } | null>(null);
const editValue = ref("");
/** 右键菜单（坐标 + 目标节点） */
const menu = ref<{ x: number; y: number; path: string[]; node: NbtNode } | null>(null);
/** 新增子节点的编辑区 */
const adding = ref<{ path: string[]; node: NbtNode } | null>(null);
const newKey = ref("");
const newType = ref("string");
const newValue = ref("");

const boolOptions = [
  { label: "0", value: 0 },
  { label: "1", value: 1 },
];
const typeOptions = [
  { label: "byte", value: "byte" },
  { label: "short", value: "short" },
  { label: "int", value: "int" },
  { label: "long", value: "long" },
  { label: "float", value: "float" },
  { label: "double", value: "double" },
  { label: "string", value: "string" },
  { label: "compound", value: "compound" },
  { label: "list", value: "list" },
];
const NUMERIC_TYPES = ["byte", "short", "int", "long", "float", "double"];

/** byte 且当前是 0/1 → 布尔下拉；其余数值走数字输入 */
const byteIsBool = computed(
  () => editing.value?.node.type === "byte" && (editValue.value === "0" || editValue.value === "1")
);
/** 下拉的 0/1 与文本框共用同一个字符串状态 */
const boolValue = computed({
  get: () => Number(editValue.value),
  set: (v: number) => {
    editValue.value = String(v);
  },
});
/** 数组类型不支持在这里逐项编辑 */
const unsupported = computed(() =>
  ["byte_array", "int_array", "long_array"].includes(editing.value?.node.type ?? "")
);
const locked = computed(() => props.running || !!props.readonly);

/** 当前编辑目标：区块（.mca）还是 level.dat 的世界数据层 */
function target() {
  const c = props.chunk;
  return c
    ? {
        read: () => api.nbtReadChunk(props.instanceId, props.world, c.dim, c.cx, c.cz),
        set: (path: string[], value: unknown, type: string) =>
          api.nbtSetChunkNode(props.instanceId, props.world, c.dim, c.cx, c.cz, path, value, type),
        del: (path: string[]) =>
          api.nbtDeleteChunkNode(props.instanceId, props.world, c.dim, c.cx, c.cz, path),
      }
    : {
        read: () => api.nbtTreeView(props.instanceId, props.world),
        set: (path: string[], value: unknown, type: string) =>
          api.nbtSetNode(props.instanceId, props.world, path, value, type),
        del: (path: string[]) => api.nbtDeleteNode(props.instanceId, props.world, path),
      };
}

async function load() {
  loading.value = true;
  try {
    const r = await target().read();
    root.value = r.root;
    // 展开根节点（level.dat 是 Data，区块是 Chunk）
    expanded.value = new Set([r.root.name]);
  } catch (e) {
    message.error(String(e));
  } finally {
    loading.value = false;
  }
}

function toggle(path: string[]) {
  const k = path.join("/");
  const next = new Set(expanded.value);
  if (next.has(k)) next.delete(k);
  else next.add(k);
  expanded.value = next;
}

function startEdit(path: string[], node: NbtNode) {
  editing.value = { path, node };
  editValue.value = String(node.value ?? "");
  adding.value = null;
}

/** 写盘前的统一拦截：游戏运行中 / 只读时直接提示 */
function blocked() {
  if (props.running) {
    message.warning(t("nbt.runningWarn"));
    return true;
  }
  if (props.readonly) {
    message.warning(t("nbt.readonlyWarn"));
    return true;
  }
  return false;
}

async function commitEdit() {
  if (!editing.value) return;
  if (blocked()) return;
  const { path, node } = editing.value;
  const numeric = NUMERIC_TYPES.includes(node.type);
  const value: unknown = numeric ? Number(editValue.value) : editValue.value;
  if (numeric && !Number.isFinite(value as number)) {
    message.error(t("nbt.invalidNumber"));
    return;
  }
  try {
    const r = await target().set(path, value, node.type);
    emit("backed-up", r.backup);
    editing.value = null;
    await load();
  } catch (e) {
    message.error(String(e));
  }
}

/** 在某个 compound 节点下新增子节点 */
function startAdd(path: string[], node: NbtNode) {
  adding.value = { path, node };
  editing.value = null;
  newKey.value = "";
  newType.value = "string";
  newValue.value = "";
}

async function commitAdd() {
  if (!adding.value) return;
  if (blocked()) return;
  const key = newKey.value.trim();
  if (!key || key.includes("/") || key.includes("\\")) {
    message.error(t("nbt.invalidKey"));
    return;
  }
  const type = newType.value;
  let value: unknown;
  if (NUMERIC_TYPES.includes(type)) {
    value = Number(newValue.value);
    if (!Number.isFinite(value as number)) {
      message.error(t("nbt.invalidNumber"));
      return;
    }
  } else if (type === "string") {
    value = newValue.value;
  } else if (type === "compound") {
    value = {};
  } else {
    value = [];
  }
  try {
    const r = await target().set([...adding.value.path, key], value, type);
    emit("backed-up", r.backup);
    message.success(t("nbt.saved"));
    adding.value = null;
    await load();
  } catch (e) {
    message.error(String(e));
  }
}

function removeNode(path: string[], name: string) {
  dialog.warning({
    title: t("nbt.deleteNodeTitle"),
    content: `${t("nbt.deleteNodeConfirm")}：${name}`,
    positiveText: t("nbt.delete"),
    negativeText: t("nbt.cancel"),
    onPositiveClick: async () => {
      try {
        const r = await target().del(path);
        emit("backed-up", r.backup);
        await load();
      } catch (e) {
        message.error(String(e));
      }
    },
  });
}

function openMenu(ev: MouseEvent, path: string[], node: NbtNode) {
  // 贴边时往回收一点，避免菜单跑出窗口
  menu.value = {
    x: Math.min(ev.clientX, window.innerWidth - 190),
    y: Math.min(ev.clientY, window.innerHeight - 190),
    path,
    node,
  };
}
function closeMenu() {
  menu.value = null;
}
function menuAction(action: "edit" | "add" | "copy" | "delete") {
  const m = menu.value;
  closeMenu();
  if (!m) return;
  if (action === "edit") startEdit(m.path, m.node);
  else if (action === "add") startAdd(m.path, m.node);
  else if (action === "copy") {
    copyText(m.path.join(".")).then((ok) =>
      ok ? message.success(t("nbt.pathCopied")) : message.error(t("nbt.copyFailed"))
    );
  } else removeNode(m.path, m.node.name);
}

// 切换世界或切换区块都要重新加载
watch(
  () => [props.world, props.chunk?.dim, props.chunk?.cx, props.chunk?.cz].join("|"),
  load,
  { immediate: true }
);
</script>

<template>
  <div class="tree-editor">
    <div v-if="loading" class="center">{{ t("nbt.readingTree") }}</div>
    <div v-else-if="!root" class="center">{{ t("nbt.loadFailed") }}</div>
    <div v-else class="tree">
      <TreeNode
        :node="root"
        :path="[]"
        :expanded="expanded"
        :highlight="highlight"
        @toggle="toggle"
        @edit="startEdit"
        @remove="removeNode"
        @menu="openMenu"
      />
    </div>

    <div v-if="editing" class="panel">
      <span class="p-name">{{ fieldLabel(editing.node.name) }}</span>
      <span class="p-key">{{ editing.node.name }}</span>
      <span class="p-type">{{ editing.node.type }}</span>
      <template v-if="unsupported">
        <span class="p-hint">{{ t("nbt.arrayNotEditable") }}</span>
      </template>
      <template v-else-if="byteIsBool">
        <NSelect v-model:value="boolValue" :options="boolOptions" size="small" style="width: 90px" />
      </template>
      <template v-else>
        <input
          v-model="editValue"
          class="input"
          :type="NUMERIC_TYPES.includes(editing.node.type) ? 'number' : 'text'"
        />
      </template>
      <button class="mini-btn" @click="editing = null">{{ t("nbt.reset") }}</button>
      <button class="mini-btn primary" :disabled="locked || unsupported" @click="commitEdit">
        {{ t("nbt.save") }}
      </button>
    </div>

    <div v-if="adding" class="panel">
      <span class="p-name">{{ t("nbt.addNodeIn") }} {{ fieldLabel(adding.node.name) }}</span>
      <input v-model="newKey" class="input" :placeholder="t('nbt.newKeyPlaceholder')" spellcheck="false" />
      <NSelect v-model:value="newType" :options="typeOptions" size="small" style="width: 116px" />
      <input
        v-if="NUMERIC_TYPES.includes(newType)"
        v-model="newValue"
        class="input narrow"
        type="number"
        :placeholder="t('nbt.newValuePlaceholder')"
      />
      <input
        v-else-if="newType === 'string'"
        v-model="newValue"
        class="input narrow"
        :placeholder="t('nbt.newValuePlaceholder')"
      />
      <button class="mini-btn" @click="adding = null">{{ t("nbt.cancel") }}</button>
      <button class="mini-btn primary" :disabled="locked" @click="commitAdd">
        {{ t("nbt.save") }}
      </button>
    </div>

    <!-- 右键菜单：必须 teleport 到 body。
         卡片用了 backdrop-filter（.glass），它会让内部 position:fixed 以卡片为
         包含块，菜单就会跟着偏移，而不是贴鼠标。 -->
    <Teleport to="body">
      <div v-if="menu" class="menu-mask" @click="closeMenu" @contextmenu.prevent="closeMenu">
        <div class="menu" :style="{ left: menu.x + 'px', top: menu.y + 'px' }">
          <button v-if="!menu.node.children?.length" class="menu-item" @click="menuAction('edit')">
            {{ t("nbt.edit") }}
          </button>
          <button
            v-if="menu.node.type === 'compound'"
            class="menu-item"
            @click="menuAction('add')"
          >
            {{ t("nbt.addNode") }}
          </button>
          <button class="menu-item" @click="menuAction('copy')">{{ t("nbt.copyPath") }}</button>
          <button
            v-if="menu.path.length > 1"
            class="menu-item danger"
            @click="menuAction('delete')"
          >
            {{ t("nbt.delete") }}
          </button>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.tree-editor {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.tree {
  max-height: 420px;
  overflow: auto;
}
.panel {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 10px;
  background: var(--w-04);
}
.p-name {
  font-weight: 700;
  font-size: 12px;
  white-space: nowrap;
}
.p-key {
  font-size: 10px;
  color: var(--text-3);
  font-family: ui-monospace, Consolas, monospace;
}
.p-type {
  font-size: 10px;
  color: var(--text-3);
}
.p-hint {
  flex: 1;
  font-size: 11px;
  color: var(--text-3);
}
.input {
  flex: 1;
  min-width: 0;
  padding: 5px 8px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text-1);
  font-family: inherit;
  font-size: 13px;
}
.input.narrow {
  flex: 0 1 140px;
}
.mini-btn {
  padding: 5px 10px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--w-04);
  color: var(--text-2);
  font-size: 12px;
  font-family: inherit;
  cursor: pointer;
  white-space: nowrap;
}
.mini-btn.primary {
  color: var(--accent);
  border-color: var(--accent-04);
  background: var(--accent-soft);
}
.mini-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.center {
  padding: 24px;
  text-align: center;
  color: var(--text-3);
}
.menu-mask {
  position: fixed;
  inset: 0;
  z-index: 2000;
}
.menu {
  position: fixed;
  min-width: 170px;
  padding: 4px;
  border-radius: 10px;
  background: var(--surface-1, var(--surface-2));
  border: 1px solid var(--border);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28);
  display: flex;
  flex-direction: column;
}
.menu-item {
  text-align: left;
  padding: 6px 10px;
  border: none;
  border-radius: 6px;
  background: none;
  color: var(--text-1);
  font-size: 12px;
  font-family: inherit;
  cursor: pointer;
}
.menu-item:hover {
  background: var(--w-08);
}
.menu-item.danger {
  color: #e5534b;
}
</style>
