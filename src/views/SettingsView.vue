<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { NTabs, NTabPane, NSwitch, useMessage } from "naive-ui";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSettingsStore } from "../stores/settings";
import { api } from "../api";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import { IconCpu, IconSearch } from "../components/icons";
import type { JavaInfo } from "../types";

const settings = useSettingsStore();
const message = useMessage();

// 主题 seg 滑动高亮
const themeSegRef = ref<HTMLElement | null>(null);
const { indicatorStyle: themeSegStyle, refresh: refreshThemeSeg } = useSlidingIndicator(
  themeSegRef,
  () => Array.from(themeSegRef.value?.querySelectorAll<HTMLElement>(".seg button") ?? []),
  () => (settings.settings?.theme === "light" ? 1 : 0),
  { axis: "horizontal" }
);
watch(() => settings.settings?.theme, () => nextTick(() => refreshThemeSeg()));

// 关闭行为 seg 滑动高亮
const closeSegRef = ref<HTMLElement | null>(null);
const { indicatorStyle: closeSegStyle, refresh: refreshCloseSeg } = useSlidingIndicator(
  closeSegRef,
  () => Array.from(closeSegRef.value?.querySelectorAll<HTMLElement>(".seg button") ?? []),
  () => (settings.settings?.close_behavior === "quit" ? 1 : 0),
  { axis: "horizontal" }
);
watch(() => settings.settings?.close_behavior, () => nextTick(() => refreshCloseSeg()));

const javaCandidates = ref<JavaInfo[]>([]);
const detecting = ref(false);
let saveTimer: ReturnType<typeof setTimeout> | null = null;
const tab = ref("general");

const memTotal = ref(0);
const memUsed = ref(0);
const memAvailable = ref(0);
let memTimer: ReturnType<typeof setInterval> | null = null;

function fmtMem(mb: number): string {
  if (!mb || mb <= 0) return "0 MB";
  if (mb >= 1024) return (mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1) + " GB";
  return Math.round(mb) + " MB";
}

const effectiveMemory = computed(() => {
  if (settings.settings?.memory_mode === "auto") {
    // Base: 40% of available (min 2048 MB), cap at 75% of available
    const cap = Math.max(512, Math.floor(memAvailable.value * 3 / 4));
    return Math.max(512, Math.min(Math.max(2048, Math.floor(memAvailable.value * 40 / 100)), cap, 8192));
  }
  return settings.settings?.max_memory_mb ?? 4096;
});
const usedPercent = computed(() => {
  if (!memTotal.value) return 0;
  return Math.min(100, Math.round((memUsed.value / memTotal.value) * 100));
});
const allocPercent = computed(() => {
  if (!memTotal.value) return 0;
  return Math.min(100, Math.round((effectiveMemory.value / memTotal.value) * 100));
});
const allocStart = computed(() => usedPercent.value);
const allocWidth = computed(() =>
  Math.max(0, Math.min(allocPercent.value, 100 - usedPercent.value))
);

async function loadMemoryInfo() {
  try {
    const res = await api.autoDetectMemory();
    memTotal.value = res.total_mb;
    memUsed.value = res.used_mb;
    memAvailable.value = res.available_mb ?? Math.max(0, res.total_mb - res.used_mb);
  } catch {
    /* ignore */
  }
}

async function detect() {
  detecting.value = true;
  try {
    await settings.loadJava(true);
    javaCandidates.value = settings.javaCandidates;
  } catch (e) {
    message.error(String(e));
  } finally {
    detecting.value = false;
  }
}

async function save() {
  try {
    skipNextSave = true;
    await settings.save();
  } catch (e) {
    message.error(String(e));
  }
}

let skipNextSave = true;
watch(
  () => settings.settings,
  () => {
    if (skipNextSave) {
      skipNextSave = false;
      return;
    }
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(save, 500);
  },
  { deep: true }
);

async function openPath(path: string) {
  try {
    await openUrl("file://" + path.replace(/\\/g, "/"));
  } catch {
    /* ignore */
  }
}

onMounted(() => {
  settings.load();
  // cached scan: no full rescan if another view already fetched recently
  settings.loadJava().then((c) => (javaCandidates.value = c));
  loadMemoryInfo();
  memTimer = setInterval(loadMemoryInfo, 10000);
});
onUnmounted(() => {
  if (memTimer) clearInterval(memTimer);
  if (saveTimer) clearTimeout(saveTimer);
});
</script>

<template>
  <div v-if="settings.settings" class="settings-view">
    <div class="head">
      <div>
        <h1>设置</h1>
        <p class="sub">Java、内存、下载与账号相关配置</p>
      </div>
    </div>

    <n-tabs v-model:value="tab" type="line" animated class="st-tabs">
      <!-- 常规 -->
      <n-tab-pane name="general" tab="常规">
        <div class="grid">
          <div class="card glass">
            <h3>外观与行为</h3>
            <div class="choice-row">
              <span>主题</span>
              <div ref="themeSegRef" class="seg">
                <div class="indicator" :style="themeSegStyle"></div>
                <button
                  :class="{ active: settings.settings.theme === 'dark' }"
                  @click="settings.patch({ theme: 'dark' })"
                >
                  深色
                </button>
                <button
                  :class="{ active: settings.settings.theme === 'light' }"
                  @click="settings.patch({ theme: 'light' })"
                >
                  浅色
                </button>
              </div>
            </div>
            <div class="choice-row">
              <span>关闭窗口时</span>
              <div ref="closeSegRef" class="seg">
                <div class="indicator" :style="closeSegStyle"></div>
                <button
                  :class="{ active: settings.settings.close_behavior === 'minimize' }"
                  @click="settings.patch({ close_behavior: 'minimize' })"
                >
                  最小化到后台
                </button>
                <button
                  :class="{ active: settings.settings.close_behavior === 'quit' }"
                  @click="settings.patch({ close_behavior: 'quit' })"
                >
                  退出程序
                </button>
              </div>
            </div>
            <div class="choice-row">
              <span>版本隔离</span>
              <n-switch
                v-model:value="settings.settings.isolation"
                @update:value="(v: boolean) => settings.patch({ isolation: v })"
              />
            </div>
            <p class="hint">开启后，每个实例的依赖库与资源文件独立存放（更占空间，互不干扰）。</p>
          </div>

          <div class="card glass">
            <h3>数据目录</h3>
            <div class="dir-row">
              <code class="mono dir">{{ settings.settings.data_dir }}</code>
              <button class="mini-btn" @click="openPath(settings.settings.data_dir)">打开</button>
            </div>
            <p class="hint">实例、游戏文件与下载缓存均存储在此目录。</p>
          </div>
        </div>
      </n-tab-pane>

      <!-- Java -->
      <n-tab-pane name="java" tab="Java">
        <div class="grid">
          <div class="card glass">
            <h3><IconCpu /> Java 运行时</h3>
            <div class="java-toolbar">
              <button class="mini-btn" :disabled="detecting" @click="detect">
                <IconSearch /> {{ detecting ? "查找中…" : "查找 Java" }}
              </button>
              <span class="hint-inline">自动扫描注册表、系统路径与常见安装目录</span>
            </div>
            <div v-if="javaCandidates.length" class="java-list">
              <div v-for="j in javaCandidates" :key="j.path" class="java-item">
                <span class="java-name">Java {{ j.major }} ({{ j.version }})</span>
                <span class="java-path">{{ j.path }}</span>
              </div>
            </div>
            <p v-else-if="!detecting" class="hint">未检测到 Java。可在实例设置中触发自动下载。</p>
            <p class="hint">Java 选择按实例独立设置：进入「游戏实例 → 实例 → 设置」，可为每个实例指定 Java 或自动下载适配版本。</p>
          </div>

          <div class="card glass">
            <h3>内存分配（默认值）</h3>
            <div class="mem-mode-row">
              <label class="radio-label" :class="{ active: settings.settings.memory_mode === 'auto' }">
                <input v-model="settings.settings.memory_mode" type="radio" value="auto" />
                自动配置
              </label>
              <label class="radio-label" :class="{ active: settings.settings.memory_mode !== 'auto' }">
                <input v-model="settings.settings.memory_mode" type="radio" value="custom" />
                手动配置
              </label>
            </div>
            <div v-if="settings.settings.memory_mode !== 'auto'" class="mem-row">
              <div>
                <label>最大内存</label>
                <input
                  v-model.number="settings.settings.max_memory_mb"
                  type="range"
                  min="1024"
                  max="16384"
                  step="256"
                  class="range"
                />
                <div class="mem-val">{{ settings.settings.max_memory_mb }} MB</div>
              </div>
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

          <div class="card glass">
            <h3>JVM 参数（额外，默认值）</h3>
            <textarea
              v-model="settings.settings.jvm_args"
              class="text-input mono"
              rows="3"
              placeholder="例如：-XX:+UseG1GC -XX:MaxGCPauseMillis=50"
            />
          </div>

          <div class="card glass">
            <h3>游戏参数（额外，默认值）</h3>
            <input
              v-model="settings.settings.game_args"
              class="text-input mono"
              placeholder="例如：--fullscreen"
            />
          </div>
        </div>
      </n-tab-pane>

      <!-- 下载 -->
      <n-tab-pane name="download" tab="下载">
        <div class="grid">
          <div class="card glass">
            <h3>并行下载</h3>
            <label class="row-label">
              并行下载线程数：{{ settings.settings.download_threads }}
              <input
                v-model.number="settings.settings.download_threads"
                type="range"
                min="1"
                max="32"
                step="1"
                class="range"
              />
            </label>
            <p class="hint">更高的线程数可加快游戏文件、模组与整合包下载速度。</p>
          </div>
          <div class="card glass">
            <h3>下载中心</h3>
            <p class="hint">所有安装与下载任务可在左侧「下载中心」实时查看进度、速度与剩余文件。</p>
          </div>
        </div>
      </n-tab-pane>

      <!-- 内容服务 -->
      <n-tab-pane name="content" tab="内容服务">
        <div class="grid">
          <div class="card glass">
            <h3>CurseForge API Key</h3>
            <input
              v-model="settings.settings.curseforge_api_key"
              class="text-input mono"
              placeholder="在 console.curseforge.com 免费申请"
            />
            <p class="hint">可选。不填则 CurseForge 内容中心不可用，Modrinth 不受影响。</p>
          </div>
          <div class="card glass">
            <h3>下载代理</h3>
            <input
              v-model="settings.settings.proxy"
              class="text-input mono"
              placeholder="http://127.0.0.1:7890 或 socks5://127.0.0.1:1080"
            />
            <p class="hint">可选。用于绕过 CDN 下载失败（404/连接失败）。修改后需重启启动器生效。</p>
          </div>
        </div>
      </n-tab-pane>

      <!-- 关于 -->
      <n-tab-pane name="about" tab="关于">
        <div class="grid">
          <div class="card glass">
            <h3>QookiX Launcher</h3>
            <p class="hint">版本 v0.1.0</p>
            <p class="hint">现代化、简洁、无广告的 Minecraft 启动器。</p>
            <p class="hint">
              支持 Modrinth / CurseForge 内容中心、多线程下载、Java 自动检测。
            </p>
          </div>
        </div>
      </n-tab-pane>
    </n-tabs>
  </div>
</template>

<style scoped>
.settings-view {
  max-width: 1000px;
  margin: 0 auto;
}
.head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  margin-bottom: 18px;
}
.head h1 {
  margin: 0 0 4px;
  font-size: 24px;
}
.sub {
  margin: 0;
  color: var(--text-3);
  font-size: 13px;
}
.btn {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border: none;
  border-radius: 10px;
  padding: 10px 18px;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.15s;
}
.btn.primary {
  background: linear-gradient(135deg, var(--accent), var(--accent-deep));
  color: #1a1208;
}
.btn.primary:hover:not(:disabled) {
  filter: brightness(1.08);
}
.btn:disabled {
  opacity: 0.6;
}
.st-tabs {
  margin-top: 4px;
}
.grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
  margin-top: 14px;
}
.card {
  padding: 18px;
}
.card h3 {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 0 14px;
  font-size: 14px;
}
.card h3 svg {
  color: var(--accent);
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
  border-color: rgba(232, 154, 75, 0.5);
}
textarea.text-input {
  resize: vertical;
  width: 100%;
}
.mini-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 13px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  white-space: nowrap;
}
.mini-btn:hover {
  background: rgba(255, 255, 255, 0.08);
}
.mini-btn:disabled {
  opacity: 0.5;
}
.java-toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}
.hint-inline {
  font-size: 12px;
  color: var(--text-3);
}
.java-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 260px;
  overflow-y: auto;
}
.java-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 7px 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-2);
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
.hint {
  font-size: 12px;
  color: var(--text-3);
  margin: 10px 0 0;
}
.mem-row {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.mem-row label {
  display: block;
  font-size: 13px;
  color: var(--text-2);
  margin-bottom: 8px;
}
.mem-val {
  font-size: 13px;
  color: var(--accent);
  font-weight: 600;
  margin-top: 4px;
}
.mem-mode-row {
  display: flex;
  gap: 16px;
  margin-bottom: 14px;
}
.radio-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--text-2);
  cursor: pointer;
  user-select: none;
}
.radio-label input {
  accent-color: var(--accent);
  cursor: pointer;
}
.radio-label.active {
  color: var(--accent);
  font-weight: 600;
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
.range {
  width: 100%;
  accent-color: var(--accent);
}
.row-label {
  display: block;
  font-size: 13px;
  color: var(--text-2);
  margin-bottom: 10px;
}
.choice-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 14px;
  font-size: 13px;
  color: var(--text-2);
}
.seg {
  position: relative;
  display: flex;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 9px;
  padding: 3px;
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
  padding: 6px 13px;
  border-radius: 7px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.seg button.active {
  color: var(--accent);
}
.dir-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.dir {
  flex: 1;
  font-size: 12px;
  color: var(--text-2);
  background: rgba(255, 255, 255, 0.05);
  padding: 8px 10px;
  border-radius: 8px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
