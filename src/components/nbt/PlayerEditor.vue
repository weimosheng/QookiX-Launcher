<script setup lang="ts">
/** 玩家数据编辑：坐标 / 维度 / 属性 / 飞行 / 游戏模式 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { NButton, NSelect, NSwitch, useMessage } from "naive-ui";
import { api } from "../../api";
import type { PlayerData, PlayerSummary } from "../../types";
import { IconUser } from "../icons";

const props = defineProps<{
  instanceId: string;
  world: string;
  player: PlayerSummary;
  running: boolean;
  readonly?: boolean;
}>();
const emit = defineEmits<{ (e: "backed-up", name: string): void }>();

const { t } = useI18n();
const message = useMessage();

const data = ref<PlayerData | null>(null);
const loading = ref(false);
const saving = ref(false);

const locked = computed(() => props.running || !!props.readonly);

const dimOptions = computed(() => [
  { label: t("nbt.dimOverworld"), value: "minecraft:overworld" },
  { label: t("nbt.dimNether"), value: "minecraft:the_nether" },
  { label: t("nbt.dimEnd"), value: "minecraft:the_end" },
]);
const modeOptions = computed(() => [
  { label: t("nbt.modeSurvival"), value: 0 },
  { label: t("nbt.modeCreative"), value: 1 },
  { label: t("nbt.modeAdventure"), value: 2 },
  { label: t("nbt.modeSpectator"), value: 3 },
]);

async function load() {
  loading.value = true;
  try {
    const r = await api.nbtReadPlayer(props.instanceId, props.world, props.player.uuid);
    // Pos 缺失 / 不完整时补零，避免 v-model 绑到 undefined
    while (r.pos.length < 3) r.pos.push(0);
    data.value = r;
  } catch (e) {
    message.error(String(e));
  } finally {
    loading.value = false;
  }
}

async function save() {
  if (!data.value) return;
  if (props.running) {
    message.warning(t("nbt.runningWarn"));
    return;
  }
  if (props.readonly) {
    message.warning(t("nbt.readonlyWarn"));
    return;
  }
  saving.value = true;
  try {
    const r = await api.nbtSavePlayer(props.instanceId, props.world, props.player.uuid, {
      pos: data.value.pos,
      dimension: data.value.dimension,
      health: data.value.health,
      foodLevel: data.value.foodLevel,
      xpLevel: data.value.xpLevel,
      xpP: data.value.xpP,
      gameType: data.value.gameType,
      abilities: data.value.abilities,
    });
    emit("backed-up", r.backup);
    message.success(t("nbt.saved"));
  } catch (e) {
    message.error(String(e));
  } finally {
    saving.value = false;
  }
}

watch(() => props.player.uuid, load, { immediate: true });
</script>

<template>
  <div class="player-editor">
    <div class="p-head">
      <IconUser class="p-icon" />
      <span class="p-name">{{ player.name }}</span>
      <span class="p-uuid">{{ player.uuid.slice(0, 8) }}…</span>
    </div>

    <div v-if="loading" class="center">{{ t("nbt.readingPlayer") }}</div>
    <div v-else-if="!data" class="center">{{ t("nbt.loadFailed") }}</div>
    <div v-else class="p-body">
      <div class="field">
        <label>{{ t("nbt.coords") }}</label>
        <div class="inline">
          <span>X</span><input v-model.number="data.pos[0]" class="input num" type="number" step="0.5" />
          <span>Y</span><input v-model.number="data.pos[1]" class="input num" type="number" step="0.5" />
          <span>Z</span><input v-model.number="data.pos[2]" class="input num" type="number" step="0.5" />
        </div>
      </div>

      <div class="field-row">
        <div class="field">
          <label>{{ t("nbt.dimension") }}</label>
          <NSelect v-model:value="data.dimension" :options="dimOptions" size="small" />
        </div>
        <div class="field">
          <label>{{ t("nbt.playerGameMode") }}</label>
          <NSelect v-model:value="data.gameType" :options="modeOptions" size="small" />
        </div>
      </div>

      <div class="field-row">
        <div class="field">
          <label>{{ t("nbt.health") }}</label>
          <input v-model.number="data.health" class="input" type="number" step="1" />
        </div>
        <div class="field">
          <label>{{ t("nbt.food") }}</label>
          <input v-model.number="data.foodLevel" class="input" type="number" step="1" />
        </div>
        <div class="field">
          <label>{{ t("nbt.xpLevel") }}</label>
          <input v-model.number="data.xpLevel" class="input" type="number" step="1" />
        </div>
        <div class="field">
          <label>{{ t("nbt.xpP") }}</label>
          <input v-model.number="data.xpP" class="input" type="number" step="0.01" />
        </div>
      </div>

      <div v-if="data.abilities" class="field-row">
        <div class="field">
          <label>{{ t("nbt.allowFly") }}</label>
          <NSwitch :value="!!data.abilities.mayfly" @update:value="(v: boolean) => (data!.abilities!.mayfly = v ? 1 : 0)" />
        </div>
        <div class="field">
          <label>{{ t("nbt.flying") }}</label>
          <NSwitch :value="!!data.abilities.flying" @update:value="(v: boolean) => (data!.abilities!.flying = v ? 1 : 0)" />
        </div>
      </div>

      <NButton type="primary" size="small" :loading="saving" :disabled="locked" @click="save">
        {{ t("nbt.save") }}
      </NButton>
      <p class="hint">{{ t("nbt.dimensionHint") }}</p>
    </div>
  </div>
</template>

<style scoped>
.player-editor {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.p-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.p-icon {
  font-size: 18px;
  color: var(--accent);
}
.p-name {
  font-size: 14px;
  font-weight: 700;
}
.p-uuid {
  font-size: 11px;
  color: var(--text-3);
}
.p-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.field-row {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
}
.field-row .field {
  flex: 1;
  min-width: 120px;
}
.field label {
  font-size: 12px;
  color: var(--text-2);
  font-weight: 600;
}
.inline {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
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
.input.num {
  width: 100px;
}
.hint {
  font-size: 11px;
  color: var(--text-3);
}
.center {
  padding: 24px;
  text-align: center;
  color: var(--text-3);
}
</style>
