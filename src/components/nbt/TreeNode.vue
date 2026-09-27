<script setup lang="ts">
/** 树形节点（递归自引用） */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { useNbtLabels } from "../../composables/useNbtLabels";
import { IconTrash } from "../icons";
import type { NbtNode } from "../../types";

const props = defineProps<{
  node: NbtNode;
  path: string[];
  expanded: Set<string>;
  highlight?: string[];
}>();
const emit = defineEmits<{
  (e: "toggle", path: string[]): void;
  (e: "edit", path: string[], node: NbtNode): void;
  (e: "remove", path: string[], name: string): void;
  (e: "menu", ev: MouseEvent, path: string[], node: NbtNode): void;
}>();

const { t } = useI18n();
const { fieldLabel } = useNbtLabels();

const path = [...props.path, props.node.name];
const rowKey = path.join("/");
const hasChildren = computed(() => !!props.node.children?.length);
/** 展开状态必须是 computed：父组件每次 toggle 都会换一个新的 Set，
 *  写成普通常量的话除了首帧就再也不会变（表现就是"带三角但点不开"）。 */
const open = computed(() => props.expanded.has(rowKey));
const highlighted = computed(() => !!props.highlight?.includes(props.node.name));
/** 根节点（Data）不允许删除 */
const removable = path.length > 1;

/** 字段名的大白话显示（只有收录过的键才有，未收录保持原名） */
const label = computed(() => fieldLabel(props.node.name));
/** 有中文名时把小字原名一起显示，方便对照 NBTExplorer */
const showRawKey = computed(() => label.value !== props.node.name);
</script>

<template>
  <div class="tn">
    <div
      class="tn-row"
      :class="{ hl: highlighted, foldable: hasChildren }"
      @contextmenu.prevent="emit('menu', $event, path, node)"
      @click="hasChildren && emit('toggle', path)"
    >
      <button class="tn-toggle" @click.stop="hasChildren && emit('toggle', path)">
        {{ hasChildren ? (open ? "▾" : "▸") : "·" }}
      </button>
      <span class="tn-name">{{ label }}</span>
      <span v-if="showRawKey" class="tn-key">{{ node.name }}</span>
      <span class="tn-type">{{ node.type }}</span>
      <span v-if="!hasChildren" class="tn-value">{{ String(node.value ?? "") }}</span>
      <button
        v-if="!hasChildren"
        class="tn-act"
        :title="t('nbt.edit')"
        @click.stop="emit('edit', path, node)"
      >
        {{ t("nbt.edit") }}
      </button>
      <button
        v-if="removable"
        class="tn-act danger"
        :title="t('nbt.deleteNodeTitle')"
        @click.stop="emit('remove', path, node.name)"
      >
        <IconTrash />
      </button>
    </div>
    <div v-if="hasChildren && open" class="tn-children">
      <TreeNode
        v-for="(c, i) in node.children"
        :key="i"
        :node="c"
        :path="path"
        :expanded="expanded"
        :highlight="highlight"
        @toggle="(p) => emit('toggle', p)"
        @edit="(p, n) => emit('edit', p, n)"
        @remove="(p, n) => emit('remove', p, n)"
        @menu="(ev, p, n) => emit('menu', ev, p, n)"
      />
    </div>
  </div>
</template>

<style scoped>
.tn-children {
  margin-left: 14px;
  border-left: 1px dashed var(--border);
  padding-left: 8px;
}
.tn-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 4px;
  border-radius: 6px;
}
.tn-row:hover {
  background: var(--w-04);
}
.tn-row.foldable {
  cursor: pointer;
}
.tn-row.hl {
  background: var(--accent-soft);
}
.tn-toggle {
  width: 18px;
  height: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  color: var(--text-3);
  font-size: 11px;
  cursor: pointer;
}
.tn-name {
  font-weight: 600;
  font-size: 12px;
  white-space: nowrap;
}
.tn-key {
  font-size: 10px;
  color: var(--text-3);
  font-family: ui-monospace, Consolas, monospace;
  white-space: nowrap;
}
.tn-type {
  font-size: 10px;
  color: var(--text-3);
}
.tn-value {
  font-size: 12px;
  color: var(--text-2);
  max-width: 260px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tn-act {
  display: inline-flex;
  align-items: center;
  background: none;
  border: none;
  color: var(--accent);
  font-size: 11px;
  cursor: pointer;
}
.tn-act.danger {
  color: #e5534b;
  font-size: 13px;
}
</style>
