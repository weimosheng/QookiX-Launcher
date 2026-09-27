<script setup lang="ts">
/**
 * 背包与物品编辑：名称 / Lore / 附魔 / 无限耐久 / 属性修饰符 / 复制粘贴。
 * 格子按真实槽位号铺满（0-40），空槽位可以直接粘贴复制好的物品。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { NButton, useMessage } from "naive-ui";
import { api } from "../../api";
import type { ItemSlot } from "../../types";
import EnchantEditor from "./EnchantEditor.vue";

const props = defineProps<{
  instanceId: string;
  world: string;
  uuid: string;
  running: boolean;
  readonly?: boolean;
}>();
const emit = defineEmits<{ (e: "backed-up", name: string): void }>();

const { t } = useI18n();
const message = useMessage();

/** 0-35 背包+快捷栏、36-39 护甲、40 副手 */
const INVENTORY_SLOTS = 41;
/** 末影箱 0-26 */
const ENDER_SLOTS = 27;

/** 常见属性，做输入建议用（附魔建议在 EnchantEditor 里） */
const ATTRIBUTE_IDS = [
  "minecraft:generic.attack_damage",
  "minecraft:generic.attack_speed",
  "minecraft:generic.movement_speed",
  "minecraft:generic.max_health",
  "minecraft:generic.armor",
  "minecraft:generic.luck",
  "minecraft:player.mining_efficiency",
];

const inventory = ref<ItemSlot[]>([]);
const enderChest = ref<ItemSlot[]>([]);
const loading = ref(false);
const saving = ref(false);
const active = ref<{ container: string; item: ItemSlot; isNew: boolean } | null>(null);
/** 复制的物品（可跨玩家/跨槽位粘贴） */
const clipboard = ref<Partial<ItemSlot> | null>(null);

const locked = computed(() => props.running || !!props.readonly);

function listOf(container: string) {
  return container === "inventory" ? inventory.value : enderChest.value;
}

/** 槽位号 → 物品 */
function bySlot(container: string) {
  const m = new Map<number, ItemSlot>();
  for (const s of listOf(container)) m.set(s.slot, s);
  return m;
}
const inventoryMap = computed(() => bySlot("inventory"));
const enderMap = computed(() => bySlot("enderChest"));

const slotNumbers = (total: number) => Array.from({ length: total }, (_, i) => i);

async function load(keep = false) {
  loading.value = true;
  try {
    const r = await api.nbtReadInventory(props.instanceId, props.world, props.uuid);
    inventory.value = r.inventory;
    enderChest.value = r.enderChest;
    if (!keep) {
      active.value = null;
    } else if (active.value) {
      // 保存后留在原槽位，数据换成新的
      const c = active.value.container;
      const n = active.value.item.slot;
      const fresh = bySlot(c).get(n);
      active.value = fresh
        ? { container: c, item: normalize(fresh), isNew: false }
        : null;
    }
  } catch (e) {
    message.error(String(e));
  } finally {
    loading.value = false;
  }
}

/** 拷一份给编辑区，取消编辑不污染列表；字段归一化成可编辑形态 */
function normalize(s: ItemSlot): ItemSlot {
  return {
    ...JSON.parse(JSON.stringify(s)),
    name: s.name ?? "",
    lore: [...(s.lore ?? [])],
  };
}

function openSlot(container: string, slotNo: number) {
  const found = bySlot(container).get(slotNo);
  if (found) {
    active.value = { container, item: normalize(found), isNew: false };
    return;
  }
  if (!clipboard.value) {
    active.value = null;
    message.info(t("nbt.emptySlotPick"));
    return;
  }
  // 空槽位：用剪贴板里的物品新建一格（保存时才落盘）。
  // index 传 0 即可——后端优先按槽位号定位，找不到才追加。
  active.value = {
    container,
    isNew: true,
    item: normalize({
      ...(clipboard.value as ItemSlot),
      index: 0,
      slot: slotNo,
    }),
  };
}

function copyItem() {
  if (!active.value) return;
  const i = active.value.item;
  clipboard.value = {
    id: i.id,
    count: i.count,
    name: i.name,
    lore: [...i.lore],
    unbreakable: i.unbreakable,
    enchantments: i.enchantments.map((e) => ({ ...e })),
    modifiers: i.modifiers.map((m) => ({ ...m })),
    modern: i.modern,
  };
  message.success(t("nbt.copied"));
}

function pasteItem() {
  if (!active.value || !clipboard.value) return;
  Object.assign(active.value.item, JSON.parse(JSON.stringify(clipboard.value)));
  message.success(t("nbt.pasted"));
}

async function saveItem() {
  if (!active.value) return;
  if (props.running) {
    message.warning(t("nbt.runningWarn"));
    return;
  }
  if (props.readonly) {
    message.warning(t("nbt.readonlyWarn"));
    return;
  }
  const { container, item } = active.value;
  if (!item.id) {
    message.error(t("nbt.itemIdRequired"));
    return;
  }
  saving.value = true;
  try {
    const r = await api.nbtSaveItem(
      props.instanceId,
      props.world,
      props.uuid,
      container,
      item.index,
      item as unknown as Record<string, unknown>,
    );
    emit("backed-up", r.backup);
    message.success(t("nbt.saved"));
    await load(true);
  } catch (e) {
    message.error(String(e));
  } finally {
    saving.value = false;
  }
}

function removeEnchant(i: number) {
  active.value?.item.enchantments.splice(i, 1);
}
function addModifier() {
  active.value?.item.modifiers.push({
    name: "",
    attribute: "minecraft:generic.attack_damage",
    amount: 1,
    operation: 0,
    slot: null,
  });
}
function removeModifier(i: number) {
  active.value?.item.modifiers.splice(i, 1);
}

function itemLabel(i: ItemSlot) {
  return i.name || i.id.replace("minecraft:", "") || "—";
}

watch(() => props.uuid, () => load(), { immediate: true });
</script>

<template>
  <div class="item-editor">
    <div v-if="loading" class="center">{{ t("nbt.readingInventory") }}</div>
    <div v-else class="ie-grid">
      <!-- 背包格子 -->
      <div class="ie-slots">
        <h4 class="sec">{{ t("nbt.groupBackpack") }}</h4>
        <div class="slot-grid">
          <button
            v-for="n in slotNumbers(INVENTORY_SLOTS)"
            :key="'i' + n"
            class="slot"
            :class="{
              active: active?.container === 'inventory' && active.item.slot === n,
              empty: !inventoryMap.has(n),
              armor: n >= 36,
              ender: false,
            }"
            :title="inventoryMap.get(n) ? itemLabel(inventoryMap.get(n)!) : t('nbt.slotEmpty')"
            @click="openSlot('inventory', n)"
          >
            <span class="slot-idx">{{ n }}</span>
            <span class="slot-name">{{ inventoryMap.has(n) ? itemLabel(inventoryMap.get(n)!) : "" }}</span>
            <span v-if="(inventoryMap.get(n)?.count ?? 1) > 1" class="slot-count">
              {{ inventoryMap.get(n)!.count }}
            </span>
          </button>
        </div>

        <h4 class="sec">{{ t("nbt.enderChest") }}</h4>
        <div class="slot-grid">
          <button
            v-for="n in slotNumbers(ENDER_SLOTS)"
            :key="'e' + n"
            class="slot ender"
            :class="{ active: active?.container === 'enderChest' && active.item.slot === n, empty: !enderMap.has(n) }"
            :title="enderMap.get(n) ? itemLabel(enderMap.get(n)!) : t('nbt.slotEmpty')"
            @click="openSlot('enderChest', n)"
          >
            <span class="slot-idx">{{ n }}</span>
            <span class="slot-name">{{ enderMap.has(n) ? itemLabel(enderMap.get(n)!) : "" }}</span>
            <span v-if="(enderMap.get(n)?.count ?? 1) > 1" class="slot-count">
              {{ enderMap.get(n)!.count }}
            </span>
          </button>
        </div>
      </div>

      <!-- 物品详情编辑 -->
      <div v-if="active" class="ie-detail">
        <div class="d-head">
          <span class="d-title">
            {{ active.isNew ? t("nbt.newItem") : itemLabel(active.item) }}
          </span>
          <span class="d-id">{{ active.item.id || "—" }}</span>
          <span class="d-slot">{{ t("nbt.slotLabel") }} {{ active.item.slot }}</span>
        </div>

        <div class="field-row">
          <div class="field">
            <label>{{ t("nbt.itemId") }}</label>
            <input v-model="active.item.id" class="input mono" spellcheck="false" placeholder="minecraft:diamond_sword" />
          </div>
          <div class="field narrow">
            <label>{{ t("nbt.count") }}</label>
            <input v-model.number="active.item.count" class="input" type="number" min="1" max="64" />
          </div>
        </div>

        <div class="field">
          <label>{{ t("nbt.unbreakable") }}</label>
          <button
            class="mini-btn"
            :class="{ active: !!active.item.unbreakable }"
            @click="active.item.unbreakable = active.item.unbreakable ? 0 : 1"
          >
            {{ active.item.unbreakable ? t("nbt.yes") : t("nbt.no") }}
          </button>
        </div>

        <div class="field">
          <label>{{ t("nbt.itemName") }}</label>
          <input v-model="active.item.name" class="input" :placeholder="t('nbt.namePlaceholder')" />
        </div>

        <div class="field">
          <label>{{ t("nbt.lore") }}</label>
          <textarea
            class="input lore"
            :value="active.item.lore.join('\n')"
            rows="3"
            :placeholder="t('nbt.lorePlaceholder')"
            @change="(e: Event) => (active!.item.lore = (e.target as HTMLTextAreaElement).value.split('\n').filter(Boolean))"
          ></textarea>
        </div>

        <EnchantEditor
          :enchants="active.item.enchantments"
          @remove="removeEnchant"
        />

        <div class="field">
          <label>{{ t("nbt.modifiers") }}</label>
          <div v-for="(m, i) in active.item.modifiers" :key="i" class="mod-row">
            <input v-model="m.attribute" class="input flex1 mono" list="nbt-attr-ids" placeholder="minecraft:generic.attack_damage" />
            <input v-model.number="m.amount" class="input num" type="number" step="0.5" />
            <button class="mini-btn danger" @click="removeModifier(i)">×</button>
          </div>
          <button class="mini-btn" @click="addModifier">+ {{ t("nbt.addModifier") }}</button>
        </div>

        <div class="d-actions">
          <NButton type="primary" size="small" :loading="saving" :disabled="locked" @click="saveItem">
            {{ t("nbt.save") }}
          </NButton>
          <button class="mini-btn" @click="copyItem">{{ t("nbt.copy") }}</button>
          <button class="mini-btn" :disabled="!clipboard" @click="pasteItem">{{ t("nbt.paste") }}</button>
        </div>
        <p v-if="clipboard" class="hint">{{ t("nbt.pasteHint") }}</p>
      </div>
      <div v-else class="ie-empty">{{ t("nbt.selectSlot") }}</div>
    </div>

    <datalist id="nbt-attr-ids">
      <option v-for="id in ATTRIBUTE_IDS" :key="id" :value="id" />
    </datalist>
  </div>
</template>

<style scoped>
.item-editor {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.ie-grid {
  display: flex;
  gap: 18px;
  flex-wrap: wrap;
}
.ie-slots {
  flex: 1;
  min-width: 280px;
}
.sec {
  font-size: 12px;
  color: var(--text-3);
  margin: 6px 0;
}
.slot-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(66px, 1fr));
  gap: 5px;
}
.slot {
  position: relative;
  height: 42px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text-1);
  font-size: 10px;
  padding: 4px 4px 2px;
  cursor: pointer;
  overflow: hidden;
  text-align: left;
  font-family: inherit;
}
.slot:hover {
  border-color: var(--accent);
}
.slot.active {
  border-color: var(--accent);
  background: var(--accent-soft);
}
.slot.empty {
  opacity: 0.45;
}
.slot.armor {
  border-style: dashed;
}
.slot.ender {
  background: rgba(120, 80, 160, 0.12);
}
.slot-idx {
  position: absolute;
  left: 3px;
  top: 1px;
  font-size: 9px;
  color: var(--text-3);
}
.slot-name {
  display: block;
  margin-top: 8px;
  word-break: break-all;
  line-height: 1.15;
  max-height: 26px;
  overflow: hidden;
}
.slot-count {
  position: absolute;
  right: 4px;
  bottom: 1px;
  font-weight: 700;
}
.ie-detail {
  flex: 1;
  min-width: 300px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  border-radius: 10px;
  background: var(--w-04);
}
.d-head {
  display: flex;
  flex-direction: column;
}
.d-title {
  font-size: 14px;
  font-weight: 700;
}
.d-id,
.d-slot {
  font-size: 11px;
  color: var(--text-3);
}
.field {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.field-row {
  display: flex;
  gap: 12px;
}
.field-row .field {
  flex: 1;
}
.field-row .field.narrow {
  flex: 0 0 90px;
}
.field label {
  font-size: 12px;
  color: var(--text-2);
  font-weight: 600;
}
.input {
  padding: 6px 10px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text-1);
  font-size: 13px;
  font-family: inherit;
}
.input.mono {
  font-family: ui-monospace, Consolas, monospace;
  font-size: 12px;
}
.input.num {
  width: 72px;
}
.input.flex1 {
  flex: 1;
  min-width: 0;
}
.lore {
  resize: vertical;
}
.mod-row {
  display: flex;
  gap: 6px;
  align-items: center;
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
}
.mini-btn:hover {
  background: var(--w-08);
}
.mini-btn.active {
  color: var(--accent);
  background: var(--accent-soft);
  border-color: var(--accent-04);
}
.mini-btn.danger {
  color: #e5534b;
}
.mini-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.d-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}
.hint {
  font-size: 11px;
  color: var(--text-3);
}
.ie-empty {
  flex: 1;
  min-width: 240px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  font-size: 12px;
}
.center {
  padding: 24px;
  text-align: center;
  color: var(--text-3);
}
</style>
