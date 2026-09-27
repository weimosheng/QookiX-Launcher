<script setup lang="ts">
/**
 * 附魔编辑：附魔 ID + 等级，输入框带常用附魔建议。
 * 直接改传入的数组元素（与父组件共享同一份数据，父组件负责保存）。
 */
import { useI18n } from "vue-i18n";

defineProps<{ enchants: { id: string; level: number }[] }>();
const emit = defineEmits<{ (e: "remove", index: number): void }>();

const { t } = useI18n();

const ENCHANT_IDS = [
  "minecraft:sharpness",
  "minecraft:smite",
  "minecraft:looting",
  "minecraft:efficiency",
  "minecraft:fortune",
  "minecraft:unbreaking",
  "minecraft:mending",
  "minecraft:protection",
  "minecraft:feather_falling",
  "minecraft:silk_touch",
  "minecraft:power",
  "minecraft:infinity",
];
</script>

<template>
  <div class="field">
    <label>{{ t("nbt.enchantments") }}</label>
    <div v-for="(en, i) in enchants" :key="i" class="ench-row">
      <input
        v-model="en.id"
        class="input flex1 mono"
        list="nbt-ench-ids"
        placeholder="minecraft:sharpness"
      />
      <input v-model.number="en.level" class="input num" type="number" min="1" max="255" />
      <button class="mini-btn danger" @click="emit('remove', i)">×</button>
    </div>
    <button class="mini-btn" @click="enchants.push({ id: 'minecraft:sharpness', level: 1 })">
      + {{ t("nbt.addEnchant") }}
    </button>
    <datalist id="nbt-ench-ids">
      <option v-for="id in ENCHANT_IDS" :key="id" :value="id" />
    </datalist>
  </div>
</template>

<style scoped>
.field {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.field label {
  font-size: 12px;
  color: var(--text-2);
  font-weight: 600;
}
.ench-row {
  display: flex;
  gap: 6px;
  align-items: center;
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
.mini-btn.danger {
  color: #e5534b;
}
</style>
