<script setup lang="ts">
/**
 * 实例分享包导出：逐项勾选要打包的内容（模组 / 资源包 / 光影 / 截图 /
 * 存档 / 附属数据文件夹 / 设置文件），支持自定义整合包名称与版本。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { NCollapse, NCollapseItem, NButton, NInput, NModal, NScrollbar, useMessage } from "naive-ui";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../../api";
import type { ExportGroup, ExportItem, ExportPreview, IdentifiedMod } from "../../types";
import { fmtCount } from "../../utils/format";

const props = defineProps<{ instanceId: string; instanceName: string }>();
const show = defineModel<boolean>("show", { required: true });

const message = useMessage();
const loading = ref(false);
const exporting = ref(false);
const preview = ref<ExportPreview | null>(null);

const packName = ref("");
const packVersion = ref("1.0.0");

// 高级选项：在线来源模组的打包方式
const bundleOnlineFiles = ref(false);
const modrinthOnly = ref(false);

// 未登记模组的来源识别（减小包体积用）
const identifying = ref(false);
const identified = ref<IdentifiedMod[]>([]);
const adopted = ref<Set<string>>(new Set());
const identProgress = ref<{ done: number; total: number; current: string } | null>(null);

let unlistenIdentify: (() => void) | null = null;
onMounted(async () => {
  unlistenIdentify = await listen<{ done: number; total: number; current: string }>(
    "share://identify",
    (e) => {
      identProgress.value = e.payload;
    },
  );
});
onBeforeUnmount(() => unlistenIdentify?.());

// group key → 选中的 item key
const selected = ref<Record<string, Set<string>>>({});
// 分组折叠状态
const collapsed = ref<Record<string, boolean>>({});

function groupSelected(g: ExportGroup): Set<string> {
  return selected.value[g.key] ?? new Set();
}
function allSelected(g: ExportGroup): boolean {
  const s = groupSelected(g);
  return s.size >= g.items.length && g.items.length > 0;
}
function someSelected(g: ExportGroup): boolean {
  const s = groupSelected(g);
  return s.size > 0 && s.size < g.items.length;
}
function toggleItem(g: ExportGroup, key: string, on: boolean) {
  const s = groupSelected(g);
  if (on) s.add(key);
  else s.delete(key);
  selected.value[g.key] = s;
}
function toggleGroup(g: ExportGroup, on: boolean) {
  const s = new Set<string>();
  if (on) for (const it of g.items) s.add(it.key);
  selected.value[g.key] = s;
}

/** 从后端 hint 文案提取徽标（已禁用 / 在线 / 未登记） */
function itemBadges(it: ExportItem): { text: string; cls: string }[] {
  const out: { text: string; cls: string }[] = [];
  if (it.hint?.includes("已禁用")) out.push({ text: "已禁用", cls: "badge-gray" });
  if (it.hint?.includes("在线来源")) out.push({ text: "在线", cls: "badge-blue" });
  if (it.hint?.includes("未登记")) out.push({ text: "未登记", cls: "badge-yellow" });
  return out;
}
function itemTitle(it: ExportItem): string {
  return [it.label, it.hint, it.key].filter(Boolean).join("\n");
}

/** 默认勾选：模组 / 资源包 / 光影全选，存档与文件夹不默认选（体积大或含个人数据） */
function applyDefaults(p: ExportPreview) {
  const sel: Record<string, Set<string>> = {};
  for (const g of p.groups) {
    if (g.required) continue;
    const s = new Set<string>();
    if (g.key === "mods" || g.key === "resourcepacks" || g.key === "shaders") {
      for (const it of g.items) s.add(it.key);
    }
    sel[g.key] = s;
  }
  selected.value = sel;
  packName.value = p.name;
}

async function loadPreview() {
  loading.value = true;
  try {
    preview.value = await api.exportPreview(props.instanceId);
    applyDefaults(preview.value);
  } catch (e) {
    message.error(String(e));
  } finally {
    loading.value = false;
  }
}

watch(show, (v) => {
  if (v) loadPreview();
});

/** 勾选内容的合计大小（在线来源模组只记引用，不计入包内体积） */
const selectedSize = computed(() => {
  let total = 0;
  for (const g of preview.value?.groups ?? []) {
    if (g.required) continue;
    const s = groupSelected(g);
    for (const it of g.items) {
      if (s.has(it.key)) total += it.size;
    }
  }
  return total;
});

function fmtSize(n: number): string {
  if (n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
}

function buildSelection() {
  const p = preview.value!;
  const get = (key: string) => Array.from(selected.value[key] ?? []);
  return {
    name: packName.value.trim() || p.name,
    version: packVersion.value.trim() || "1.0.0",
    mods: get("mods"),
    includeDisabledMods: true,
    resourcepacks: get("resourcepacks"),
    shaders: get("shaders"),
    screenshots: get("screenshots"),
    worlds: get("worlds"),
    folders: get("folders"),
    optionsTxt: (selected.value["settings"] ?? new Set()).has("options_txt"),
    serversDat: (selected.value["settings"] ?? new Set()).has("servers_dat"),
    bundleOnlineFiles: bundleOnlineFiles.value,
    modrinthOnly: modrinthOnly.value,
    identified: identified.value.filter((m) => adopted.value.has(m.filename)),
  };
}

async function doExport() {
  if (!preview.value) return;
  const dest = await save({
    defaultPath: `${packName.value.trim() || preview.value.name}.qkxinst`,
    filters: [{ name: "QookiX 实例分享包", extensions: ["qkxinst"] }],
  });
  if (!dest) return;
  exporting.value = true;
  try {
    const n = await api.exportInstancePack(props.instanceId, dest as string, buildSelection());
    message.success(`已导出 ${fmtCount(n)} 个内容到 ${dest}`);
    show.value = false;
  } catch (e) {
    message.error(String(e));
  } finally {
    exporting.value = false;
  }
}

async function runIdentify() {
  identifying.value = true;
  identProgress.value = { done: 0, total: 0, current: "" };
  try {
    identified.value = await api.identifyManualMods(props.instanceId);
    // 哈希精确匹配的可信，默认采纳；按文件名猜测的一律留给用户决定
    adopted.value = new Set(
      identified.value.filter((m) => m.confidence === "hash").map((m) => m.filename),
    );
    if (!identified.value.length) message.info("没有找到可在 Modrinth 上定位的未登记模组");
  } catch (e) {
    message.error(String(e));
  } finally {
    identifying.value = false;
    identProgress.value = null;
  }
}

function toggleAdopt(filename: string, on: boolean) {
  const s = new Set(adopted.value);
  if (on) s.add(filename);
  else s.delete(filename);
  adopted.value = s;
}

function toggleCollapsed(g: ExportGroup) {
  if (g.required) return;
  collapsed.value[g.key] = !collapsed.value[g.key];
}
</script>

<template>
  <n-modal
    v-model:show="show"
    preset="card"
    title="导出实例分享包"
    style="width: 640px; max-width: 94vw"
    :mask-closable="true"
    :close-on-esc="true"
  >
    <div class="ex-body">
      <div v-if="loading" class="ex-loading">正在扫描实例内容…</div>
      <template v-else-if="preview">
        <div class="ex-meta">
          <label class="ex-field">
            <span>整合包名称</span>
            <n-input v-model:value="packName" size="small" placeholder="分享包的名称" />
          </label>
          <label class="ex-field ex-ver">
            <span>版本</span>
            <n-input v-model:value="packVersion" size="small" placeholder="1.0.0" />
          </label>
        </div>

        <n-scrollbar style="max-height: 48vh" trigger="none">
          <div class="ex-groups">
            <div v-for="g in preview.groups" :key="g.key" class="ex-group">
              <div class="ex-group-head" @click="toggleCollapsed(g)">
                <span
                  v-if="!g.required"
                  class="cb"
                  :class="{ on: allSelected(g), ind: someSelected(g) }"
                  @click.stop="toggleGroup(g, !allSelected(g))"
                >
                  <svg v-if="allSelected(g)" viewBox="0 0 12 12"><path d="M2 6.2 4.8 9 10 3" /></svg>
                  <i v-else-if="someSelected(g)" class="cb-dash" />
                </span>
                <span class="ex-group-label">{{ g.label }}</span>
                <span v-if="g.hint" class="ex-group-hint">{{ g.hint }}</span>
                <span v-if="!g.required && g.items.length" class="ex-group-ops" @click.stop>
                  <button v-if="g.key === 'mods'" :disabled="identifying" @click="runIdentify">
                    {{ identifying ? `识别中 ${identProgress?.done ?? 0}/${identProgress?.total ?? "?"}` : "识别来源" }}
                  </button>
                  <button @click="toggleGroup(g, true)">全选</button>
                  <button @click="toggleGroup(g, false)">清空</button>
                </span>
                <svg v-if="!g.required" class="ex-arrow" :class="{ open: !collapsed[g.key] }" viewBox="0 0 12 12">
                  <path d="M3 4.5 6 7.5 9 4.5" />
                </svg>
              </div>

              <div v-if="g.required" class="ex-required">{{ g.items[0]?.hint ?? g.hint }}</div>

              <div v-else class="ex-collapse" :class="{ open: !collapsed[g.key] }">
                <div class="ex-collapse-inner">
                  <div class="ex-items">
                    <div
                      v-for="it in g.items"
                      :key="it.key"
                      class="ex-item"
                      :title="itemTitle(it)"
                      @click="toggleItem(g, it.key, !groupSelected(g).has(it.key))"
                    >
                      <span class="cb" :class="{ on: groupSelected(g).has(it.key) }">
                        <svg v-if="groupSelected(g).has(it.key)" viewBox="0 0 12 12"><path d="M2 6.2 4.8 9 10 3" /></svg>
                      </span>
                      <span class="ex-name">
                        {{ it.label }}
                        <i v-for="b in itemBadges(it)" :key="b.text" class="ex-badge" :class="b.cls">{{ b.text }}</i>
                      </span>
                      <span v-if="it.size" class="ex-size">{{ fmtSize(it.size) }}</span>
                    </div>

                    <div v-if="g.key === 'mods' && identifying && identProgress" class="ex-ident-progress">
                      正在识别 {{ identProgress.done }}/{{ identProgress.total }}
                      <span v-if="identProgress.current" class="ex-ident-current">· {{ identProgress.current }}</span>
                    </div>

                    <template v-if="g.key === 'mods' && identified.length">
                      <div class="ex-ident-warn">
                        以下未登记模组在 Modrinth 上找到了来源，勾选后只记录版本 ID（导入时重新下载），可显著减小包体积。
                        <b>「按文件名猜测」可能因重名装错模组，确认无误再勾；不确定就保持打包文件。</b>
                      </div>
                      <div
                        v-for="m in identified"
                        :key="m.filename"
                        class="ex-item"
                        :title="`${m.name}\n${m.filename}\n${m.confidence === 'hash' ? '按文件哈希精确匹配' : '按文件名猜测，可能错配'}`"
                        @click="toggleAdopt(m.filename, !adopted.has(m.filename))"
                      >
                        <span class="cb" :class="{ on: adopted.has(m.filename) }">
                          <svg v-if="adopted.has(m.filename)" viewBox="0 0 12 12"><path d="M2 6.2 4.8 9 10 3" /></svg>
                        </span>
                        <span class="ex-name">
                          {{ m.name }}
                          <i class="ex-badge" :class="m.confidence === 'hash' ? 'badge-green' : 'badge-yellow'">
                            {{ m.confidence === "hash" ? "哈希精确" : "文件名猜测" }}
                          </i>
                        </span>
                        <span class="ex-file">{{ m.filename }}</span>
                      </div>
                    </template>

                    <div v-if="!g.items.length" class="ex-empty">（无）</div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </n-scrollbar>

        <p class="ex-tip">
          已选约 <b>{{ fmtSize(selectedSize) }}</b
          >。在线安装的模组默认只记录版本 ID，导入时自动重新下载；游戏本体不在包内，导入后会自动安装。
        </p>

        <n-collapse>
          <n-collapse-item title="高级选项" name="adv">
            <div class="ex-adv">
              <label class="ex-adv-row">
                <n-checkbox v-model:checked="bundleOnlineFiles" />
                <span>打包资源文件，以避免在导入时下载</span>
              </label>
              <p class="ex-adv-hint">
                将模组、资源包、光影包的文件直接放入整合包中，导入时无需联网下载。
                建议仅在无法稳定连接 CurseForge 或 Modrinth 时勾选。
              </p>
              <label class="ex-adv-row" :class="{ disabled: !bundleOnlineFiles }">
                <n-checkbox v-model:checked="modrinthOnly" :disabled="!bundleOnlineFiles" />
                <span>仅打包 Modrinth 来源的资源文件</span>
              </label>
              <p class="ex-adv-hint">
                CurseForge 的分发协议禁止第三方整合包转打包其文件，Modrinth 无此限制；
                开启后 CurseForge 来源的模组仍只记录版本 ID。
              </p>
            </div>
          </n-collapse-item>
        </n-collapse>
      </template>

      <div class="ex-actions">
        <n-button @click="show = false">取消</n-button>
        <n-button type="primary" :loading="exporting" :disabled="!preview || loading" @click="doExport">
          选择位置并导出
        </n-button>
      </div>
    </div>
  </n-modal>
</template>

<style scoped>
.ex-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.ex-loading {
  padding: 24px 0;
  text-align: center;
  font-size: 13px;
  color: var(--text-3);
}
.ex-meta {
  display: flex;
  gap: 12px;
}
.ex-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex: 1;
  font-size: 12px;
  color: var(--text-2);
}
.ex-ver {
  max-width: 110px;
}
.ex-groups {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding-right: 6px;
}
.ex-group {
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 8px 10px;
  background: var(--panel-sub, rgba(255, 255, 255, 0.02));
}
.ex-group + .ex-group {
  margin-top: 4px;
}
.ex-group-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 4px;
  cursor: pointer;
  border-radius: 6px;
}
.ex-group-head:hover {
  background: rgba(255, 255, 255, 0.03);
}
/* 折叠动画：grid 0fr → 1fr */
.ex-collapse {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows 0.28s ease;
}
.ex-collapse.open {
  grid-template-rows: 1fr;
}
.ex-collapse-inner {
  overflow: hidden;
  min-height: 0;
}
.ex-arrow {
  width: 12px;
  height: 12px;
  margin-left: 2px;
  flex-shrink: 0;
  fill: none;
  stroke: var(--text-3);
  stroke-width: 1.5;
  stroke-linecap: round;
  stroke-linejoin: round;
  transition: transform 0.28s ease;
}
.ex-arrow.open {
  transform: rotate(180deg);
}
/* 自绘打钩式复选框（方形圆角，选中填充主题色 + 白钩，支持半选横线） */
.cb {
  width: 15px;
  height: 15px;
  border-radius: 4px;
  border: 1.5px solid var(--text-3);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  background: transparent;
  transition:
    background 0.15s ease,
    border-color 0.15s ease;
}
.cb:hover {
  border-color: var(--accent);
}
.cb.on {
  background: var(--accent);
  border-color: var(--accent);
}
.cb svg {
  width: 10px;
  height: 10px;
  fill: none;
  stroke: #fff;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
  opacity: 0;
  transform: scale(0.5);
  transition:
    opacity 0.15s ease,
    transform 0.15s ease;
}
.cb.on svg {
  opacity: 1;
  transform: scale(1);
}
.cb.ind {
  border-color: var(--accent);
}
.cb-dash {
  width: 8px;
  height: 2px;
  border-radius: 1px;
  background: var(--accent);
}
.ex-group-label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.ex-group-hint {
  font-size: 11px;
  color: var(--text-3);
}
.ex-group-ops {
  margin-left: auto;
  display: flex;
  gap: 12px;
}
.ex-group-ops button {
  background: none;
  border: none;
  padding: 0;
  font-size: 11px;
  color: var(--accent);
  cursor: pointer;
}
.ex-group-ops button:hover {
  text-decoration: underline;
}
.ex-group-ops button:disabled {
  opacity: 0.5;
  cursor: default;
}
.ex-items {
  display: flex;
  flex-direction: column;
  margin: 4px 0 2px 22px;
}
.ex-item {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-2);
  cursor: pointer;
  padding: 3px 6px;
  border-radius: 6px;
  user-select: none;
}
.ex-item:hover {
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-1);
}
.ex-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ex-badge {
  font-style: normal;
  font-size: 10px;
  padding: 1px 5px;
  border-radius: 4px;
  margin-left: 4px;
  white-space: nowrap;
  vertical-align: 1px;
}
.badge-gray {
  color: var(--text-3);
  background: rgba(128, 128, 128, 0.15);
}
.badge-blue {
  color: #5aa9e6;
  background: rgba(90, 169, 230, 0.12);
}
.badge-yellow {
  color: #e0a44b;
  background: rgba(224, 164, 75, 0.12);
}
.badge-green {
  color: #4ec9a0;
  background: rgba(78, 201, 160, 0.12);
}
.ex-size {
  width: 62px;
  text-align: right;
  font-size: 11px;
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}
.ex-file {
  max-width: 42%;
  font-size: 11px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex-shrink: 0;
}
.ex-empty {
  margin-left: 22px;
  font-size: 12px;
  color: var(--text-3);
}
.ex-required {
  margin-left: 22px;
  font-size: 12px;
  color: var(--text-3);
}
.ex-ident-progress {
  margin: 2px 0 2px 22px;
  font-size: 12px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ex-ident-current {
  color: var(--text-2);
}
.ex-ident-warn {
  margin: 4px 0;
  padding: 6px 8px;
  font-size: 11px;
  line-height: 1.6;
  color: var(--text-3);
  background: rgba(224, 164, 75, 0.06);
  border-radius: 6px;
}
.ex-ident-warn b {
  color: #e0a44b;
}
.ex-tip {
  margin: 0;
  font-size: 11px;
  line-height: 1.7;
  color: var(--text-3);
}
.ex-tip b {
  color: var(--text-2);
}
.ex-adv {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 4px 0;
}
.ex-adv-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-1);
  cursor: pointer;
}
.ex-adv-row.disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.ex-adv-hint {
  margin: 0 0 2px 22px;
  font-size: 11px;
  line-height: 1.6;
  color: var(--text-3);
}
</style>
