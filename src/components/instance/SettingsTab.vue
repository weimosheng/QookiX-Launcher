<script setup lang="ts">
/**
 * 实例详情 · 设置 tab。
 * 从 InstanceDetailView 拆出，自行负责：Java 运行时选择/自动下载、内存分配
 * （全局/自动/自定义三模式 + 内存仪表）、实例别名、JVM/游戏参数、账号覆盖、
 * 分辨率、实例图标，以及 edit 草稿的防抖自动保存。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { NSelect, useMessage } from "naive-ui";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../../api";
import { useInstancesStore } from "../../stores/instances";
import { useAccountsStore } from "../../stores/accounts";
import { useSettingsStore } from "../../stores/settings";
import { useMemoryInfo } from "../../composables/useMemoryInfo";
import { fmtMem } from "../../utils/format";
import AppIcon from "../AppIcon.vue";
import IconPickerDialog from "../IconPickerDialog.vue";
import { IconDownload } from "../icons";

const props = defineProps<{ instanceId: string }>();

const instances = useInstancesStore();
const accounts = useAccountsStore();
const settingsStore = useSettingsStore();
const message = useMessage();
const instance = computed(() => instances.get(props.instanceId));

const javaCandidates = ref<{ path: string; version: string; major: number; vendor: string; arch: string }[]>([]);
const requiredJava = ref<number | null>(null);
const needDownload = ref(false);
const downloadingJava = ref(false);
const autoSelecting = ref(false);
const showIconPicker = ref(false);
const edit = ref({
  icon: "",
  max_memory_mb: 4096,
  memory_mode: "global" as "global" | "auto" | "custom",
  jvm_args: "",
  game_args: "",
  java_path: "",
  account_id: "",
  resolution_w: "",
  resolution_h: "",
});
// 别名不进自动保存的 edit 对象：每敲一个字符触发一次 patch + 列表重载
// 会非常卡。改为独立草稿 + 显式保存按钮。
const aliasDraft = ref("");
const savingAlias = ref(false);

const { memTotal, memUsed, memAvailable, startPolling, stopPolling } = useMemoryInfo();

// The custom slider max is capped at the currently available (free) memory,
// so the game allocation can never exceed the remaining space (fallback 16 GB).
const sliderMax = computed(() => {
  if (!memAvailable.value) return 16384;
  return Math.max(1024, memAvailable.value);
});

const globalMemoryMode = computed(() => settingsStore.settings?.memory_mode ?? "custom");
const globalMemory = computed(() => {
  if (globalMemoryMode.value === "auto") return autoMemory.value;
  return settingsStore.settings?.max_memory_mb ?? 4096;
});

const autoMemory = computed(() => {
  const modCount = instance.value?.mods?.length ?? 0;
  // Base: 40% of available (min 2048 MB), +512 MB per 100 mods (cap +4 GB)
  let rec = Math.max(2048, Math.floor(memAvailable.value * 40 / 100)) + Math.min(4096, Math.floor(modCount * 512 / 100));
  // Cap at 75% of available memory, leave room for OS
  const cap = Math.max(512, Math.floor(memAvailable.value * 3 / 4));
  rec = Math.min(rec, cap, 8192);
  return Math.max(rec, 512);
});

const effectiveMemory = computed(() => {
  if (edit.value.memory_mode === "auto") return autoMemory.value;
  if (edit.value.memory_mode === "global") return globalMemory.value;
  return edit.value.max_memory_mb;
});

const usedPercent = computed(() => {
  if (!memTotal.value) return 0;
  return Math.min(100, Math.round((memUsed.value / memTotal.value) * 100));
});
const allocPercent = computed(() => {
  if (!memTotal.value) return 0;
  return Math.min(100, Math.round((effectiveMemory.value / memTotal.value) * 100));
});
// The allocated segment sits right after the used segment so both colors are always visible.
const allocStart = computed(() => usedPercent.value);
const allocWidth = computed(() =>
  Math.max(0, Math.min(allocPercent.value, 100 - usedPercent.value))
);

let skipNextEditSync = false;
watch(
  () => instance.value,
  (i) => {
    if (!i) return;
    if (skipNextEditSync) {
      skipNextEditSync = false;
      return;
    }
    edit.value = {
      icon: i.icon ?? "",
      max_memory_mb: i.max_memory_mb ?? 4096,
      memory_mode: (i.memory_mode as "global" | "auto" | "custom") ?? "global",
      jvm_args: i.jvm_args ?? "",
      game_args: i.game_args ?? "",
      java_path: i.java_path ?? "",
      account_id: i.account_id ?? "",
      resolution_w: i.resolution?.[0]?.toString() ?? "",
      resolution_h: i.resolution?.[1]?.toString() ?? "",
    };
    aliasDraft.value = i.alias ?? "";
  },
  { immediate: true }
);

async function detectJava() {
  try {
    const [cands, rec] = await Promise.all([
      useSettingsStore().loadJava(),
      api.recommendJava(props.instanceId),
    ]);
    javaCandidates.value = cands;
    requiredJava.value = rec.required;
    needDownload.value = rec.needDownload;
  } catch (e) {
    message.error(String(e));
  }
}

/** Auto-pick a suitable Java for this instance (downloads it if missing). */
async function autoSelectJava() {
  autoSelecting.value = true;
  try {
    const rec = await api.recommendJava(props.instanceId);
    requiredJava.value = rec.required;
    if (rec.java && rec.java.major >= rec.required) {
      edit.value.java_path = rec.java.path;
      message.success(`已选择 Java ${rec.java.version}`);
    } else if (rec.needDownload) {
      await downloadJava(rec.required);
    } else {
      message.info("未找到合适的 Java");
    }
  } catch (e) {
    message.error(String(e));
  } finally {
    autoSelecting.value = false;
  }
}

async function downloadJava(major: number) {
  downloadingJava.value = true;
  try {
    const info = await api.downloadJava(major);
    message.success(`Java ${major} 已下载（${info.version}）`);
    javaCandidates.value = await useSettingsStore().loadJava(true);
    edit.value.java_path = info.path;
    await detectJava();
  } catch (e) {
    message.error(String(e));
  } finally {
    downloadingJava.value = false;
  }
}

async function pickJava() {
  const file = await open({
    multiple: false,
    filters: [{ name: "Java 可执行文件", extensions: ["exe"] }],
    directory: false,
  });
  if (file) edit.value.java_path = file as string;
}

async function saveSettings() {
  try {
    const mem =
      edit.value.memory_mode === "custom"
        ? Math.min(edit.value.max_memory_mb, sliderMax.value)
        : 0;
    skipNextEditSync = true;
    await instances.patch({
      id: props.instanceId,
      icon: edit.value.icon,
      max_memory_mb: mem,
      memory_mode: edit.value.memory_mode,
      jvm_args: edit.value.jvm_args,
      game_args: edit.value.game_args,
      java_path: edit.value.java_path,
      account_id: edit.value.account_id,
      resolution:
        edit.value.resolution_w && edit.value.resolution_h
          ? [Number(edit.value.resolution_w), Number(edit.value.resolution_h)]
          : null,
    });
  } catch (e) {
    message.error(String(e));
  }
}

/** 显式保存实例别名（点按钮触发，不走自动保存） */
async function saveAlias() {
  if (savingAlias.value) return;
  savingAlias.value = true;
  try {
    await instances.patch({ id: props.instanceId, alias: aliasDraft.value });
    message.success("别名已保存");
  } catch (e) {
    message.error(String(e));
  } finally {
    savingAlias.value = false;
  }
}

let saveTimer: ReturnType<typeof setTimeout> | null = null;
let skipFirstSave = true;
watch(
  edit,
  () => {
    if (skipFirstSave) {
      skipFirstSave = false;
      return;
    }
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(saveSettings, 500);
  },
  { deep: true }
);

onMounted(() => {
  detectJava();
  startPolling();
});
onBeforeUnmount(() => {
  stopPolling();
  if (saveTimer) clearTimeout(saveTimer);
});
</script>

<template>
  <div class="settings-grid">
    <div class="set-card glass">
      <h4>Java 运行时</h4>
      <div class="java-req">
        <span class="req-label">该游戏需要</span>
        <span class="req-val">Java {{ requiredJava ?? "?" }}+</span>
        <button class="mini-btn" :disabled="autoSelecting" @click="autoSelectJava">
          自动选择
        </button>
        <button
          v-if="needDownload && requiredJava"
          class="mini-btn accent"
          :disabled="downloadingJava"
          @click="downloadJava(requiredJava)"
        >
          <IconDownload />
          {{ downloadingJava ? "下载中…" : `下载 Java ${requiredJava}` }}
        </button>
      </div>
      <div class="java-row">
        <input v-model="edit.java_path" class="text-input mono" placeholder="留空则自动选择合适版本" />
        <button class="mini-btn" @click="pickJava">浏览…</button>
        <button class="mini-btn" @click="detectJava">刷新列表</button>
      </div>
      <div v-if="javaCandidates.length" class="java-list">
        <button
          v-for="j in javaCandidates.slice(0, 10)"
          :key="j.path"
          class="java-item"
          :class="{ active: edit.java_path === j.path }"
          @click="edit.java_path = j.path"
        >
          <span class="java-name">Java {{ j.major }} ({{ j.version }})</span>
          <span class="java-path">{{ j.path }}</span>
        </button>
      </div>
      <p class="hint">留空时启动器会自动挑选合适版本；没有合适版本会先自动下载。</p>
    </div>

    <div class="set-card glass">
      <h4>内存分配</h4>
      <div class="mem-modes">
        <label
          class="mem-mode"
          :class="{ active: edit.memory_mode === 'global' }"
        >
          <input v-model="edit.memory_mode" type="radio" value="global" />
          根据全局配置
        </label>
        <label
          class="mem-mode"
          :class="{ active: edit.memory_mode === 'auto' }"
        >
          <input v-model="edit.memory_mode" type="radio" value="auto" />
          自动配置
        </label>
        <label
          class="mem-mode"
          :class="{ active: edit.memory_mode === 'custom' }"
        >
          <input v-model="edit.memory_mode" type="radio" value="custom" />
          自定义
        </label>
      </div>

      <template v-if="edit.memory_mode === 'custom'">
        <input
          v-model.number="edit.max_memory_mb"
          type="range"
          min="1024"
          :max="sliderMax"
          step="256"
          class="range"
        />
        <div class="range-labels"><span>1 GB</span><span>{{ fmtMem(sliderMax) }}</span></div>
        <div class="mem-current">{{ edit.max_memory_mb }} MB</div>
      </template>

      <div v-else class="mem-current">
        {{ effectiveMemory }} MB
        <span v-if="edit.memory_mode === 'global' && globalMemoryMode === 'auto'" class="mem-mode-note">（全局自动配置）</span>
        <span v-else-if="edit.memory_mode === 'global'" class="mem-mode-note">（全局手动配置）</span>
        <span v-else-if="edit.memory_mode === 'auto'" class="mem-mode-note">（自动配置）</span>
      </div>

      <div class="mem-gauge">
        <div class="mem-gauge-track">
          <div class="mem-gauge-used" :style="{ width: usedPercent + '%' }"></div>
          <div
            class="mem-gauge-alloc"
            :style="{ left: allocStart + '%', width: allocWidth + '%' }"
          ></div>
        </div>
        <div class="mem-gauge-labels">
          <span><i class="dot used"></i>已使用 {{ fmtMem(memUsed) }}（{{ usedPercent }}%）</span>
          <span><i class="dot alloc"></i>游戏分配 {{ fmtMem(effectiveMemory) }}（{{ allocPercent }}%）</span>
          <span><i class="dot total"></i>总内存 {{ fmtMem(memTotal) }} / 可用 {{ fmtMem(memAvailable) }}</span>
        </div>
      </div>
    </div>

    <div class="set-card glass">
      <h4>实例别名（协议启动）</h4>
      <div class="alias-row">
        <input
          v-model="aliasDraft"
          class="text-input mono"
          placeholder="例如 my-sky（仅小写字母、数字、- 和 _）"
          @keydown.enter="saveAlias"
        />
        <button
          class="mini-btn"
          :disabled="savingAlias || aliasDraft === (instance?.alias ?? '')"
          @click="saveAlias"
        >
          {{ savingAlias ? "保存中…" : "保存" }}
        </button>
      </div>
      <p class="hint">
        设置后可用 <code>qookix://launch/{{ aliasDraft || "别名" }}</code> 从浏览器或命令行直接启动本实例。
      </p>
    </div>

    <div class="set-card glass">
      <h4>JVM 参数（额外）</h4>
      <textarea v-model="edit.jvm_args" class="text-input mono" rows="3" placeholder="例如：-XX:+UseG1GC -Dfile.encoding=UTF-8" />
    </div>

    <div class="set-card glass">
      <h4>游戏参数（额外）</h4>
      <input v-model="edit.game_args" class="text-input mono" placeholder="例如：--fullscreen" />
    </div>

    <div class="set-card glass">
      <h4>账号</h4>
      <n-select
        v-model:value="edit.account_id"
        :options="[
          { label: `跟随全局当前账号（${accounts.current?.username ?? '未选择'}）`, value: '' },
          ...accounts.accounts.map((a) => ({
            label: `${a.username}（${a.type === 'microsoft' ? '正版' : '离线'}）`,
            value: a.uuid,
          })),
        ]"
      />
    </div>

    <div class="set-card glass">
      <h4>游戏窗口分辨率（可选）</h4>
      <div class="res-row">
        <input v-model="edit.resolution_w" class="text-input" placeholder="宽，如 1920" />
        <span>×</span>
        <input v-model="edit.resolution_h" class="text-input" placeholder="高，如 1080" />
      </div>
    </div>

    <div class="set-card glass">
      <h4>实例图标</h4>
      <div class="icon-pick">
        <div class="icon-preview">
          <AppIcon :name="edit.icon" />
        </div>
        <button class="btn" @click="showIconPicker = true">选择图标</button>
      </div>
    </div>

    <IconPickerDialog
      v-model:show="showIconPicker"
      :value="edit.icon"
      :instance-id="instanceId"
      @save="edit.icon = $event"
    />
  </div>
</template>

<style scoped>
.settings-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 14px;
}
.set-card {
  padding: 16px;
}
.set-card h4 {
  margin: 0 0 12px;
  font-size: 14px;
}
.alias-row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.alias-row .text-input {
  flex: 1;
  min-width: 0;
}
.alias-row .mini-btn {
  flex-shrink: 0;
  white-space: nowrap;
}
.java-req {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
  font-size: 13px;
}
.req-label {
  color: var(--text-3);
}
.req-val {
  color: var(--accent);
  font-weight: 700;
}
.java-row {
  display: flex;
  gap: 8px;
}
.text-input {
  flex: 1;
  min-width: 0;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid var(--border);
  border-radius: 9px;
  color: var(--text-1);
  padding: 8px 12px;
  font-size: 13px;
  font-family: inherit;
  outline: none;
  transition: border-color 0.12s;
}
.text-input:focus {
  border-color: var(--accent-05);
}
textarea.text-input {
  resize: vertical;
  width: 100%;
}
.java-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 10px;
  max-height: 200px;
  overflow-y: auto;
}
.java-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 7px 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
  font-family: inherit;
  text-align: left;
}
.java-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.java-path {
  font-size: 11px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.java-item.active {
  border-color: var(--accent-05);
  background: var(--accent-soft);
  color: var(--accent);
}
.range {
  width: 100%;
  accent-color: var(--accent);
}
.range-labels {
  display: flex;
  justify-content: space-between;
  font-size: 11px;
  color: var(--text-3);
}
.mem-modes {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
  flex-wrap: wrap;
}
.mem-mode {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  font-size: 13px;
  cursor: pointer;
  color: var(--text-2);
  transition: all 0.12s;
}
.mem-mode:hover {
  background: rgba(255, 255, 255, 0.08);
}
.mem-mode.active {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}
.mem-mode input {
  accent-color: var(--accent);
}
.mem-current {
  font-size: 14px;
  font-weight: 600;
  color: var(--accent);
  margin-top: 6px;
}
.mem-mode-note {
  font-size: 12px;
  font-weight: 400;
  color: var(--text-3);
}
.mem-gauge {
  margin-top: 14px;
}
.mem-gauge-track {
  position: relative;
  height: 10px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.08);
  overflow: hidden;
}
.mem-gauge-used,
.mem-gauge-alloc {
  position: absolute;
  top: 0;
  bottom: 0;
  height: 100%;
  /* 两端圆角由外层 track 的 overflow:hidden 统一裁剪，
     两段之间保持直角无缝衔接，铺满整个轨道 */
  transition: width 0.2s, left 0.2s;
}
.mem-gauge-used {
  left: 0;
  background: linear-gradient(90deg, #5a8ef0, #8ab4ff);
}
.mem-gauge-alloc {
  background: linear-gradient(90deg, #e89a4b, #f2c079);
}
.mem-gauge-labels {
  display: flex;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 4px;
  font-size: 11px;
  color: var(--text-3);
  margin-top: 6px;
}
.mem-gauge-labels span {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}
.dot.used {
  background: #8ec4ff;
}
.dot.alloc {
  background: #e89a4b;
}
.dot.total {
  background: #9aa4b2;
}
.res-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.icon-pick {
  display: flex;
  align-items: center;
  gap: 10px;
}
.icon-preview {
  width: 38px;
  height: 38px;
  border-radius: 10px;
  overflow: hidden;
  background: transparent;
  position: relative;
  flex-shrink: 0;
  box-sizing: border-box;
}
.icon-preview :deep(.app-icon) {
  position: absolute;
  inset: 0;
  font-size: 18px;
}
.mini-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.mini-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-1);
}
.mini-btn.accent {
  color: var(--accent);
  border-color: var(--accent-04);
}
.btn {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: none;
  border-radius: 10px;
  padding: 9px 16px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.14s;
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-1);
  border: 1px solid var(--border);
}
.btn:hover {
  background: rgba(255, 255, 255, 0.1);
}
.hint {
  font-size: 12px;
  color: var(--text-3);
  margin-top: 4px;
}
</style>
