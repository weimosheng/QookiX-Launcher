<script setup lang="ts">
/**
 * NBT 存档编辑器。
 * 三种视图：世界属性（表单 / 树形）、玩家数据、背包物品。
 * 安全设计：写回只改提交的字段（后端保证未知字段不丢），
 * 保存前自动单文件备份，游戏运行中或只读时不写盘。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { NButton, NModal, NProgress, NSelect, NSwitch, useDialog, useMessage } from "naive-ui";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useInstancesStore } from "../stores/instances";
import { fmtDate, fmtSize } from "../utils/format";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import type { PlayerSummary, WorldForm, WorldSummary } from "../types";
import {
  IconBox,
  IconFolder,
  IconInfo,
  IconRefresh,
  IconRestore,
  IconSave,
  IconUser,
} from "../components/icons";
import PlayerEditor from "../components/nbt/PlayerEditor.vue";
import ItemEditor from "../components/nbt/ItemEditor.vue";
import NbtTreeEditor from "../components/nbt/NbtTreeEditor.vue";
import ChunkEditor from "../components/nbt/ChunkEditor.vue";
import BackupPanel from "../components/nbt/BackupPanel.vue";

const { t, te } = useI18n();
const message = useMessage();
const dialog = useDialog();
const instances = useInstancesStore();

const MODE_KEY = "qookix:nbt:mode";
const TAB_KEY = "qookix:nbt:tab";
/** 树形模式的操作说明只自动弹一次 */
const HELP_KEY = "qookix:nbt:treeHelp";

type Mode = "form" | "tree";
type Tab = "world" | "chunk" | "player" | "item";

const instanceId = ref("");
const worlds = ref<WorldSummary[]>([]);
const loadingWorlds = ref(false);
const current = ref<WorldSummary | null>(null);
/** 手动指定的世界（不在实例 saves 下） */
const manual = ref(false);

const mode = ref<Mode>((localStorage.getItem(MODE_KEY) as Mode) || "form");
const tab = ref<Tab>((localStorage.getItem(TAB_KEY) as Tab) || "world");

const form = ref<WorldForm | null>(null);
const original = ref<WorldForm | null>(null);
const running = ref(false);
const editable = ref(true);
const readonlyReason = ref<string | null>(null);
const saving = ref(false);
const lastBackup = ref("");

const players = ref<PlayerSummary[]>([]);
const activePlayer = ref<PlayerSummary | null>(null);
const backupShow = ref(false);
const helpShow = ref(false);

/**
 * 当前正在进行的耗时操作。
 * 存档读写是本地磁盘操作，世界大时会有几秒无响应，
 * 这里给用户看到"正在做什么 + 进度"，避免误以为程序卡死。
 */
const busy = ref<{
  kind: "scan" | "work";
  label: string;
  detail: string;
  pct: number | null;
} | null>(null);
let unlisten: (() => void) | null = null;

function startBusy(kind: "scan" | "work", label: string) {
  busy.value = { kind, label, detail: "", pct: kind === "scan" ? 0 : null };
}
function endBusy() {
  busy.value = null;
}

/** 常用游戏规则（布尔）：值在 level.dat 里是字符串 "true"/"false" */
const BOOL_RULES = [
  "keepInventory",
  "doDaylightCycle",
  "doWeatherCycle",
  "doMobSpawning",
  "mobGriefing",
  "doFireTick",
  "doTileDrops",
  "doMobLoot",
  "naturalRegeneration",
  "doInsomnia",
  "disableRaids",
  "fallDamage",
  "fireDamage",
  "pvp",
  "doImmediateRespawn",
  "showCoordinates",
  "announceAdvancements",
  "commandBlockOutput",
];
/** 数值型规则（用输入框） */
const NUMERIC_RULES = [
  "randomTickSpeed",
  "spawnRadius",
  "playersSleepingPercentage",
  "maxEntityCramming",
];

const isBoolRule = (v: string | undefined) => v === "true" || v === "false";
/** 规则名 → 大白话短名；没收录的规则直接显示原本的键名 */
function ruleLabel(key: string) {
  const k = `nbt.gamerule.${key}`;
  return te(k) ? t(k) : key;
}
/** 白名单之外的全部规则（折叠区展示） */
const otherRules = computed(() => {
  const rules = form.value?.gameRules;
  if (!rules) return [];
  return Object.keys(rules)
    .filter((k) => !BOOL_RULES.includes(k) && !NUMERIC_RULES.includes(k))
    .sort();
});
function setRule(key: string, value: string) {
  if (!form.value?.gameRules) return;
  form.value.gameRules[key] = value;
}
/** 本次实际改动过的规则（只提交这些，未动的后端保持原样） */
const changedRules = computed<Record<string, string>>(() => {
  const out: Record<string, string> = {};
  const before = original.value?.gameRules ?? {};
  const now = form.value?.gameRules ?? {};
  for (const [k, v] of Object.entries(now)) {
    if (before[k] !== v) out[k] = v;
  }
  return out;
});

const gameModeOptions = computed(() => [
  { label: t("nbt.modeSurvival"), value: 0 },
  { label: t("nbt.modeCreative"), value: 1 },
  { label: t("nbt.modeAdventure"), value: 2 },
  { label: t("nbt.modeSpectator"), value: 3 },
]);
const difficultyOptions = computed(() => [
  { label: t("nbt.diffPeaceful"), value: 0 },
  { label: t("nbt.diffEasy"), value: 1 },
  { label: t("nbt.diffNormal"), value: 2 },
  { label: t("nbt.diffHard"), value: 3 },
]);
const yesNoOptions = computed(() => [
  { label: t("nbt.no"), value: 0 },
  { label: t("nbt.yes"), value: 1 },
]);
const instanceOptions = computed(() =>
  instances.instances.map((i) => ({ label: `${i.name}（${i.mc_version}）`, value: i.id }))
);
const noInstance = computed(() => !instanceId.value);

async function loadWorlds() {
  if (!instanceId.value) {
    worlds.value = [];
    return;
  }
  loadingWorlds.value = true;
  startBusy("scan", t("nbt.scanningWorlds"));
  try {
    const r = await api.nbtListWorlds(instanceId.value);
    worlds.value = r.worlds;
  } catch (e) {
    message.error(String(e));
    worlds.value = [];
  } finally {
    loadingWorlds.value = false;
    endBusy();
  }
}

async function openWorld(w: WorldSummary) {
  current.value = w;
  lastBackup.value = "";
  activePlayer.value = null;
  startBusy("work", t("nbt.readingWorld"));
  try {
    const r = await api.nbtOpenWorld(instanceId.value, w.dir);
    form.value = r.form;
    original.value = JSON.parse(JSON.stringify(r.form));
    running.value = r.running;
    editable.value = r.editable;
    readonlyReason.value = r.reason;
    if (r.running) message.warning(t("nbt.runningWarn"));
    else if (r.reason) message.warning(r.reason);
    // 手动打开时卡片元信息是空的，用 level.dat 补上
    if (w.levelName == null) w.levelName = r.form.levelName;
    startBusy("work", t("nbt.readingPlayers"));
    const pl = await api.nbtListPlayers(instanceId.value, w.dir);
    players.value = pl.players;
  } catch (e) {
    message.error(String(e));
    current.value = null;
  } finally {
    endBusy();
  }
}

/** 手动指定世界目录（无实例或存档不在实例目录下时用） */
async function openOtherWorld() {
  const picked = await open({ directory: true, multiple: false, title: t("nbt.openOther") });
  if (!picked || Array.isArray(picked)) return;
  const path = String(picked);
  manual.value = true;
  await openWorld({
    dir: path,
    levelName: null,
    gameType: null,
    difficulty: null,
    seed: null,
    size: 0,
    modified: 0,
  });
}

/** 卡片 / 标题上显示的世界名：目录名（手动选择时是路径的最后一段） */
function worldLabel(w: WorldSummary) {
  return w.levelName || w.dir.split(/[\\/]/).filter(Boolean).pop() || w.dir;
}

function back() {
  current.value = null;
  form.value = null;
  activePlayer.value = null;
  manual.value = false;
}

const modeSegRef = ref<HTMLElement | null>(null);
const tabSegRef = ref<HTMLElement | null>(null);
const { indicatorStyle: modeSegStyle, refresh: refreshModeSeg } = useSlidingIndicator(
  modeSegRef,
  () => Array.from(modeSegRef.value?.querySelectorAll<HTMLElement>("button") ?? []),
  () => (mode.value === "form" ? 0 : 1)
);
const { indicatorStyle: tabSegStyle, refresh: refreshTabSeg } = useSlidingIndicator(
  tabSegRef,
  () => Array.from(tabSegRef.value?.querySelectorAll<HTMLElement>("button") ?? []),
  () => (["world", "chunk", "player", "item"] as Tab[]).indexOf(tab.value)
);
watch(mode, () => nextTick(() => refreshModeSeg()));
watch(tab, () => {
  nextTick(() => refreshTabSeg());
  if (tab.value === "world") nextTick(() => refreshModeSeg());
});

function setMode(m: Mode) {
  mode.value = m;
  localStorage.setItem(MODE_KEY, m);
  // 第一次切到树形模式，主动弹一次操作说明
  if (m === "tree" && !localStorage.getItem(HELP_KEY)) {
    localStorage.setItem(HELP_KEY, "1");
    helpShow.value = true;
  }
}
function setTab(x: Tab) {
  tab.value = x;
  localStorage.setItem(TAB_KEY, x);
}

/** 与原始值对比，生成 "旧 → 新" 改动摘要（key 用真实 NBT 字段名，树形模式据此高亮） */
const changes = computed<{ label: string; from: string; to: string; key: string }[]>(() => {
  if (!form.value || !original.value) return [];
  const out: { label: string; from: string; to: string; key: string }[] = [];
  const diff = (key: string, label: string, a: unknown, b: unknown) => {
    if (JSON.stringify(a) !== JSON.stringify(b)) {
      out.push({ key, label, from: String(a ?? "-"), to: String(b ?? "-") });
    }
  };
  const label = (
    opts: { label: string; value: number | null }[],
    v: number | null
  ) => opts.find((o) => o.value === v)?.label ?? String(v ?? "-");
  diff("LevelName", t("nbt.levelName"), original.value.levelName, form.value.levelName);
  diff(
    "GameType",
    t("nbt.gameMode"),
    label(gameModeOptions.value, original.value.gameType),
    label(gameModeOptions.value, form.value.gameType)
  );
  diff(
    "Difficulty",
    t("nbt.difficulty"),
    label(difficultyOptions.value, original.value.difficulty),
    label(difficultyOptions.value, form.value.difficulty)
  );
  diff("RandomSeed", t("nbt.seed"), original.value.seed, form.value.seed);
  diff("Time", t("nbt.time"), original.value.time, form.value.time);
  diff("raining", t("nbt.weatherRain"), original.value.raining, form.value.raining);
  diff("thundering", t("nbt.weatherThunder"), original.value.thundering, form.value.thundering);
  diff("SpawnX", t("nbt.spawn"), original.value.spawnX, form.value.spawnX);
  diff("SpawnY", t("nbt.spawn"), original.value.spawnY, form.value.spawnY);
  diff("SpawnZ", t("nbt.spawn"), original.value.spawnZ, form.value.spawnZ);
  diff("allowCommands", t("nbt.allowCheats"), original.value.allowCommands, form.value.allowCommands);
  diff(
    "DifficultyLocked",
    t("nbt.lockDifficulty"),
    original.value.difficultyLocked,
    form.value.difficultyLocked
  );
  diff("hardcore", t("nbt.hardcore"), original.value.hardcore, form.value.hardcore);
  // 游戏规则逐条比对（只列出真正改动的）
  const beforeRules = original.value.gameRules ?? {};
  for (const [k, v] of Object.entries(changedRules.value)) {
    out.push({ key: k, label: ruleLabel(k), from: beforeRules[k] ?? "-", to: v });
  }
  if (form.value.border && original.value.border) {
    // 扁平布局的字段名带 Border 前缀，树形模式高亮要跟着走
    const flat = !!form.value.borderFlat;
    diff(flat ? "BorderCenterX" : "CenterX", t("nbt.center"), original.value.border.centerX, form.value.border.centerX);
    diff(flat ? "BorderCenterZ" : "CenterZ", t("nbt.center"), original.value.border.centerZ, form.value.border.centerZ);
    diff(flat ? "BorderSize" : "Size", t("nbt.size"), original.value.border.size, form.value.border.size);
  }
  return out;
});

function setWeather(kind: "clear" | "rain" | "thunder") {
  if (!form.value) return;
  form.value.raining = kind === "clear" ? 0 : 1;
  form.value.thundering = kind === "thunder" ? 1 : 0;
}
function setTime(preset: "sunrise" | "noon" | "sunset" | "midnight") {
  if (!form.value) return;
  const map = { sunrise: 0, noon: 6000, sunset: 12000, midnight: 18000 };
  form.value.time = map[preset];
}

async function save() {
  if (!form.value || !current.value) return;
  if (running.value) {
    message.warning(t("nbt.runningWarn"));
    return;
  }
  if (!editable.value) {
    message.warning(readonlyReason.value || t("nbt.readonlyWarn"));
    return;
  }
  const list = changes.value;
  if (!list.length) {
    message.info(t("nbt.noChanges"));
    return;
  }
  dialog.warning({
    title: t("nbt.confirmTitle"),
    content: () =>
      `${t("nbt.confirmSummary")}：\n` +
      list.map((c) => `• ${c.label}: ${c.from} → ${c.to}`).join("\n") +
      `\n\n${t("nbt.backupNotice")}`,
    positiveText: t("nbt.save"),
    negativeText: t("nbt.reset"),
    onPositiveClick: async () => {
      saving.value = true;
      startBusy("work", t("nbt.writingSave"));
      try {
        // 落盘前再确认一次可写：文件可能在打开之后被改成只读
        const chk = await api.nbtCanEdit(instanceId.value, current.value!.dir);
        if (!chk.editable) {
          editable.value = false;
          readonlyReason.value = chk.reason;
          message.warning(chk.reason || t("nbt.readonlyWarn"));
          return;
        }
        const r = await api.nbtSaveWorld(instanceId.value, current.value!.dir, {
          LevelName: form.value!.levelName,
          GameType: form.value!.gameType,
          Difficulty: form.value!.difficulty,
          DifficultyLocked: form.value!.difficultyLocked,
          hardcore: form.value!.hardcore,
          RandomSeed: form.value!.seed,
          Time: form.value!.time,
          DayTime: form.value!.time,
          raining: form.value!.raining,
          thundering: form.value!.thundering,
          SpawnX: form.value!.spawnX,
          SpawnY: form.value!.spawnY,
          SpawnZ: form.value!.spawnZ,
          allowCommands: form.value!.allowCommands,
          // 只提交改动过的规则，其余规则后端保持原样
          gameRules: changedRules.value,
          border: form.value!.border,
        });
        lastBackup.value = r.backup;
        original.value = JSON.parse(JSON.stringify(form.value));
        message.success(t("nbt.saved"));
      } catch (e) {
        message.error(String(e));
      } finally {
        saving.value = false;
        endBusy();
      }
    },
  });
}

function resetForm() {
  if (original.value) form.value = JSON.parse(JSON.stringify(original.value));
}

function onBackedUp(name: string) {
  lastBackup.value = name;
}

/** 恢复备份后重新读取世界（表单与玩家列表都要刷新） */
async function onRestored() {
  if (current.value) await openWorld(current.value);
}

onMounted(async () => {
  // 后端扫描世界时逐个汇报进度（"正在扫描 我的世界 3/12"）
  unlisten = await listen<{ current: number; total: number; label: string }>(
    "nbt-progress",
    (e) => {
      if (busy.value?.kind !== "scan") return;
      const { current, total, label } = e.payload;
      busy.value.pct = total ? Math.round((current / total) * 100) : null;
      busy.value.detail = `${label} ${current}/${total}`;
    }
  );
  if (!instances.instances.length) await instances.load();
  const inst = instances.instances.find((i) => i.installed) ?? instances.instances[0];
  if (inst) instanceId.value = inst.id;
});

onBeforeUnmount(() => {
  unlisten?.();
  unlisten = null;
});

watch(instanceId, () => {
  current.value = null;
  form.value = null;
  manual.value = false;
  loadWorlds();
});
</script>

<template>
  <div class="nbt-view">
    <div class="bar glass">
      <span class="bar-label">{{ t("nbt.selectInstance") }}</span>
      <NSelect
        v-model:value="instanceId"
        :options="instanceOptions"
        size="small"
        filterable
        clearable
        :placeholder="t('nbt.noInstanceOption')"
        style="min-width: 240px"
      />
      <button class="mini-btn" :disabled="loadingWorlds || !instanceId" @click="loadWorlds">
        <IconRefresh /> {{ loadingWorlds ? t("nbt.scanning") : t("nbt.refresh") }}
      </button>
      <div class="spacer" />
      <button class="mini-btn" @click="openOtherWorld">
        <IconFolder /> {{ t("nbt.openOther") }}
      </button>
    </div>

    <!-- 进行中的操作：进度条 + 正在做什么 -->
    <div v-if="busy" class="busy">
      <NProgress
        type="line"
        :percentage="busy.pct ?? 100"
        :processing="busy.pct === null"
        :height="6"
        :border-radius="3"
        :show-indicator="false"
      />
      <div class="busy-text">
        <span>{{ busy.label }}</span>
        <span v-if="busy.detail" class="busy-detail">{{ busy.detail }}</span>
      </div>
    </div>

    <!-- 世界列表 -->
    <div v-if="!current" class="world-list">
      <div v-if="noInstance" class="empty glass">{{ t("nbt.needInstance") }}</div>
      <div v-else-if="loadingWorlds" class="empty glass">{{ t("nbt.scanning") }}</div>
      <div v-else-if="!worlds.length" class="empty glass">{{ t("nbt.noWorlds") }}</div>
      <div
        v-for="w in worlds"
        :key="w.dir"
        class="world-card glass clickable"
        @click="openWorld(w)"
      >
        <IconFolder class="world-icon" />
        <div class="c-info">
          <div class="c-name">{{ worldLabel(w) }}</div>
          <div class="c-meta">
            <span>{{ gameModeOptions.find((o) => o.value === w.gameType)?.label ?? "—" }}</span>
            <span>{{ difficultyOptions.find((o) => o.value === w.difficulty)?.label ?? "—" }}</span>
            <span v-if="w.playerName">
              {{ t("nbt.playerLabel") }} {{ w.playerName }}
              <template v-if="(w.playerCount ?? 0) > 1">+{{ (w.playerCount ?? 1) - 1 }}</template>
            </span>
            <span>{{ fmtSize(w.size) }}</span>
            <span v-if="w.modified">{{ fmtDate(w.modified) }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- 编辑器 -->
    <div v-else class="editor glass">
      <div class="editor-head">
        <button class="mini-btn" @click="back">← {{ t("nbt.backToList") }}</button>
        <span class="world-title">{{ worldLabel(current) }}</span>
        <span v-if="manual" class="tag manual">{{ t("nbt.manualTag") }}</span>
        <span v-if="running" class="tag warn">{{ t("nbt.runningWarn") }}</span>
        <span v-else-if="!editable" class="tag warn">{{ t("nbt.readonlyTag") }}</span>
        <div class="spacer" />
        <button class="mini-btn" @click="helpShow = true">
          <IconInfo /> {{ t("nbt.helpTitle") }}
        </button>
        <button class="mini-btn" @click="backupShow = true">
          <IconRestore /> {{ t("nbt.backupTitle") }}
        </button>
        <!-- 表单 / 树形 模式切换 -->
        <div v-if="tab === 'world'" ref="modeSegRef" class="seg">
          <div class="indicator" :style="modeSegStyle"></div>
          <button :class="{ active: mode === 'form' }" @click="setMode('form')">
            {{ t("nbt.formMode") }}
          </button>
          <button :class="{ active: mode === 'tree' }" @click="setMode('tree')">
            {{ t("nbt.treeMode") }}
          </button>
        </div>
      </div>

      <div v-if="readonlyReason && !running" class="readonly-bar">{{ readonlyReason }}</div>

      <!-- 三大分区 -->
      <div ref="tabSegRef" class="seg">
        <div class="indicator" :style="tabSegStyle"></div>
        <button :class="{ active: tab === 'world' }" @click="setTab('world')">
          {{ t("nbt.worldProps") }}
        </button>
        <button :class="{ active: tab === 'chunk' }" @click="setTab('chunk')">
          {{ t("nbt.chunks") }}
        </button>
        <button :class="{ active: tab === 'player' }" @click="setTab('player')">
          <IconUser /> {{ t("nbt.players") }}
        </button>
        <button :class="{ active: tab === 'item' }" @click="setTab('item')">
          <IconBox /> {{ t("nbt.inventory") }}
        </button>
      </div>

      <!-- 世界属性：表单模式 -->
      <div v-if="tab === 'world' && mode === 'form' && form" class="form-body">
        <div class="field">
          <label>{{ t("nbt.levelName") }}</label>
          <input v-model="form.levelName" class="input" spellcheck="false" />
        </div>
        <div class="field-row">
          <div class="field">
            <label>{{ t("nbt.gameMode") }}</label>
            <NSelect v-model:value="form.gameType" :options="gameModeOptions" size="small" />
          </div>
          <div class="field">
            <label>{{ t("nbt.difficulty") }}</label>
            <NSelect v-model:value="form.difficulty" :options="difficultyOptions" size="small" />
          </div>
        </div>
        <div class="field">
          <label>{{ t("nbt.seed") }}</label>
          <input v-model="form.seed" class="input" spellcheck="false" inputmode="numeric" />
          <span class="hint">{{ t("nbt.seedHint") }}</span>
        </div>
        <div class="field">
          <label>{{ t("nbt.time") }}</label>
          <div class="inline">
            <input v-model.number="form.time" class="input num" type="number" />
            <button class="mini-btn" @click="setTime('sunrise')">{{ t("nbt.timeSunrise") }}</button>
            <button class="mini-btn" @click="setTime('noon')">{{ t("nbt.timeNoon") }}</button>
            <button class="mini-btn" @click="setTime('sunset')">{{ t("nbt.timeSunset") }}</button>
            <button class="mini-btn" @click="setTime('midnight')">{{ t("nbt.timeMidnight") }}</button>
          </div>
        </div>
        <div class="field">
          <label>{{ t("nbt.weather") }}</label>
          <div class="inline">
            <button class="mini-btn" :class="{ active: !form.raining && !form.thundering }" @click="setWeather('clear')">{{ t("nbt.weatherClear") }}</button>
            <button class="mini-btn" :class="{ active: form.raining && !form.thundering }" @click="setWeather('rain')">{{ t("nbt.weatherRain") }}</button>
            <button class="mini-btn" :class="{ active: !!form.thundering }" @click="setWeather('thunder')">{{ t("nbt.weatherThunder") }}</button>
          </div>
        </div>
        <div class="field">
          <label>{{ t("nbt.spawn") }}</label>
          <div class="inline">
            <span>X</span><input v-model.number="form.spawnX" class="input num" type="number" />
            <span>Y</span><input v-model.number="form.spawnY" class="input num" type="number" />
            <span>Z</span><input v-model.number="form.spawnZ" class="input num" type="number" />
          </div>
        </div>
        <div class="field-row">
          <div class="field">
            <label>{{ t("nbt.allowCheats") }}</label>
            <NSelect v-model:value="form.allowCommands" :options="yesNoOptions" size="small" />
          </div>
          <div class="field">
            <label>{{ t("nbt.lockDifficulty") }}</label>
            <div class="inline">
              <NSwitch :value="!!form.difficultyLocked" @update:value="(v: boolean) => (form!.difficultyLocked = v ? 1 : 0)" />
              <span class="hint">{{ t("nbt.lockHint") }}</span>
            </div>
          </div>
        </div>
        <div class="field">
          <label>{{ t("nbt.hardcore") }}</label>
          <div class="inline">
            <NSwitch
              :value="!!form.hardcore"
              @update:value="(v: boolean) => (form!.hardcore = v ? 1 : 0)"
            />
            <span class="hint">{{ t("nbt.hardcoreHint") }}</span>
          </div>
        </div>

        <template v-if="form.gameRules">
          <h3 class="sec-title">{{ t("nbt.gameRules") }}</h3>
          <div class="rules">
            <div v-for="k in BOOL_RULES" :key="k" class="rule-row">
              <span class="rule-label" :title="k">{{ ruleLabel(k) }}</span>
              <NSwitch
                size="small"
                :value="form.gameRules?.[k] === 'true'"
                @update:value="(v: boolean) => setRule(k, v ? 'true' : 'false')"
              />
            </div>
          </div>
          <div class="field-row">
            <div v-for="k in NUMERIC_RULES" :key="k" class="field">
              <label :title="k">{{ ruleLabel(k) }}</label>
              <input
                class="input"
                type="number"
                :value="form.gameRules?.[k] ?? ''"
                @input="(e: Event) => setRule(k, (e.target as HTMLInputElement).value)"
              />
            </div>
          </div>
          <details v-if="otherRules.length" class="rules-all">
            <summary>{{ t("nbt.allRules") }}（{{ otherRules.length }}）</summary>
            <div class="rules">
              <div v-for="k in otherRules" :key="k" class="rule-row">
                <span class="rule-label" :title="k">{{ ruleLabel(k) }}</span>
                <NSwitch
                  v-if="isBoolRule(form.gameRules?.[k])"
                  size="small"
                  :value="form.gameRules?.[k] === 'true'"
                  @update:value="(v: boolean) => setRule(k, v ? 'true' : 'false')"
                />
                <input
                  v-else
                  class="input rule-input"
                  :value="form.gameRules?.[k]"
                  @input="(e: Event) => setRule(k, (e.target as HTMLInputElement).value)"
                />
              </div>
            </div>
          </details>
        </template>

        <template v-if="form.border">
          <h3 class="sec-title">{{ t("nbt.worldBorder") }}</h3>
          <div class="field-row">
            <div class="field">
              <label>{{ t("nbt.center") }} X</label>
              <input v-model.number="form.border.centerX" class="input" type="number" step="0.5" />
            </div>
            <div class="field">
              <label>{{ t("nbt.center") }} Z</label>
              <input v-model.number="form.border.centerZ" class="input" type="number" step="0.5" />
            </div>
            <div class="field">
              <label>{{ t("nbt.size") }}</label>
              <input v-model.number="form.border.size" class="input" type="number" step="0.5" />
            </div>
          </div>
        </template>

        <div class="actions">
          <NButton
            type="primary"
            :loading="saving"
            :disabled="running || !editable"
            @click="save"
          >
            <IconSave />&nbsp;{{ t("nbt.save") }}
          </NButton>
          <button class="mini-btn" @click="resetForm">{{ t("nbt.reset") }}</button>
          <span v-if="changes.length" class="dirty">{{ changes.length }} {{ t("nbt.pendingCount") }}</span>
        </div>
        <div v-if="lastBackup" class="backup-tip">{{ t("nbt.backupMade") }}：{{ lastBackup }}</div>
      </div>

      <!-- 世界属性：树形模式 -->
      <NbtTreeEditor
        v-else-if="tab === 'world' && mode === 'tree'"
        :instance-id="instanceId"
        :world="current.dir"
        :running="running"
        :readonly="!editable"
        :highlight="changes.map((c) => c.key)"
        @backed-up="onBackedUp"
      />

      <!-- 区块（.mca） -->
      <ChunkEditor
        v-else-if="tab === 'chunk'"
        :instance-id="instanceId"
        :world="current.dir"
        :running="running"
        :readonly="!editable"
        @backed-up="onBackedUp"
      />

      <!-- 玩家数据 -->
      <div v-else-if="tab === 'player'" class="tab-body">
        <div v-if="!players.length" class="center">{{ t("nbt.noPlayers") }}</div>
        <template v-else>
          <div class="player-list">
            <button
              v-for="p in players"
              :key="p.uuid"
              class="mini-btn player-btn"
              :class="{ active: activePlayer?.uuid === p.uuid }"
              @click="activePlayer = p"
            >
              <IconUser /> {{ p.name }}
            </button>
          </div>
          <PlayerEditor
            v-if="activePlayer"
            :instance-id="instanceId"
            :world="current.dir"
            :player="activePlayer"
            :running="running"
            :readonly="!editable"
            @backed-up="onBackedUp"
          />
          <div v-else class="center">{{ t("nbt.selectPlayer") }}</div>
        </template>
      </div>

      <!-- 背包物品 -->
      <div v-else-if="tab === 'item'" class="tab-body">
        <div v-if="!players.length" class="center">{{ t("nbt.noPlayers") }}</div>
        <template v-else>
          <div class="player-list">
            <button
              v-for="p in players"
              :key="p.uuid"
              class="mini-btn player-btn"
              :class="{ active: activePlayer?.uuid === p.uuid }"
              @click="activePlayer = p"
            >
              <IconUser /> {{ p.name }}
            </button>
          </div>
          <ItemEditor
            v-if="activePlayer"
            :instance-id="instanceId"
            :world="current.dir"
            :uuid="activePlayer.uuid"
            :running="running"
            :readonly="!editable"
            @backed-up="onBackedUp"
          />
          <div v-else class="center">{{ t("nbt.selectPlayer") }}</div>
        </template>
      </div>
    </div>

    <!-- 操作说明：工具栏按钮打开，首次切到树形模式自动弹一次 -->
    <NModal
      v-model:show="helpShow"
      preset="card"
      :title="t('nbt.helpTitle')"
      style="max-width: 460px"
    >
      <p class="help-line">{{ t("nbt.helpLine") }}</p>
    </NModal>

    <BackupPanel
      v-if="current"
      v-model:show="backupShow"
      :instance-id="instanceId"
      :world="current.dir"
      @restored="onRestored"
    />
  </div>
</template>

<style scoped>
.nbt-view {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 4px;
}
.bar {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  padding: 12px 16px;
}
.bar-label {
  font-size: 13px;
  color: var(--text-2);
}
.spacer {
  flex: 1;
}
.busy {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.busy-text {
  display: flex;
  gap: 8px;
  font-size: 12px;
  color: var(--text-3);
}
.busy-detail {
  color: var(--text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.world-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.world-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 14px;
  cursor: pointer;
}
.world-icon {
  font-size: 22px;
  color: var(--accent);
  flex-shrink: 0;
}
.c-info {
  flex: 1;
  min-width: 0;
}
.c-name {
  font-size: 14px;
  font-weight: 600;
}
.c-meta {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
  font-size: 11px;
  color: var(--text-3);
  margin-top: 3px;
}
.editor {
  padding: 18px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.editor-head {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.world-title {
  font-size: 15px;
  font-weight: 700;
}
.tag {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  border: 1px solid var(--border);
  color: var(--text-3);
}
.tag.warn {
  color: #e0a000;
  border-color: #e0a00055;
}
.tag.manual {
  color: var(--accent);
  border-color: var(--accent-04);
}
.readonly-bar {
  padding: 8px 12px;
  border-radius: 8px;
  background: var(--w-04);
  color: var(--text-2);
  font-size: 12px;
}
.seg {
  position: relative;
  display: flex;
  background: var(--panel);
  backdrop-filter: blur(var(--glass-blur, 8px));
  -webkit-backdrop-filter: blur(var(--glass-blur, 8px));
  border-radius: 9px;
  padding: 3px;
  min-height: 38px;
}
.seg .indicator {
  position: absolute;
  top: 3px;
  bottom: 3px;
  border-radius: 7px;
  background: var(--accent-soft);
  pointer-events: none;
}
.seg button {
  border: none;
  background: transparent;
  color: var(--text-3);
  padding: 6px 18px;
  border-radius: 7px;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.seg button.active {
  color: var(--accent);
}
.form-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.field-row {
  display: flex;
  gap: 14px;
  flex-wrap: wrap;
}
.field-row .field {
  flex: 1;
  min-width: 150px;
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
  background: var(--w-04);
  color: var(--text-1);
  font-size: 13px;
  font-family: inherit;
  outline: none;
}
.input:focus {
  border-color: var(--accent-04);
}
.input.num {
  width: 110px;
}
.inline {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.hint {
  font-size: 11px;
  color: var(--text-3);
}
.sec-title {
  font-size: 13px;
  font-weight: 700;
  color: var(--text-2);
  margin-top: 4px;
}
.rules {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
  gap: 6px 16px;
}
.rule-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 3px 0;
}
.rule-label {
  font-size: 12px;
  color: var(--text-2);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rule-input {
  width: 120px;
  padding: 4px 8px;
  font-size: 12px;
}
.rules-all {
  margin-top: 2px;
}
.rules-all summary {
  cursor: pointer;
  font-size: 12px;
  color: var(--text-3);
  padding: 4px 0;
}
.rules-all .rules {
  margin-top: 6px;
}
.mini-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
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
  border-color: var(--accent-04);
  background: var(--accent-soft);
}
.mini-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.actions {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 6px;
}
.dirty {
  font-size: 12px;
  color: var(--accent);
}
.backup-tip {
  font-size: 11px;
  color: var(--text-3);
}
.tab-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.player-list {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.player-btn {
  font-size: 12px;
}
.center {
  padding: 40px;
  text-align: center;
  color: var(--text-3);
}
.empty {
  padding: 48px 24px;
  text-align: center;
  color: var(--text-3);
}
.help-line {
  margin: 0;
  font-size: 13px;
  line-height: 1.6;
  color: var(--text-1);
}
</style>
