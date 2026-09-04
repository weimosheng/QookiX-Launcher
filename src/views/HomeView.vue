<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useInstancesStore } from "../stores/instances";
import { useAccountsStore } from "../stores/accounts";
import { usePinsStore, type PinItem } from "../stores/pins";
import { useSettingsStore } from "../stores/settings";
import { latencyInfo, loaderBadge } from "../utils/format";
import { api } from "../api";
import { supportsQuickPlay } from "../version";
import { useMessage, NModal } from "naive-ui";
import AppIcon from "../components/AppIcon.vue";
import type { ServerStatus } from "../types";
import { IconClose, IconCompass, IconFolder, IconGlobe, IconPlay, IconRepeat, IconUser } from "../components/icons";

const router = useRouter();
const instances = useInstancesStore();
const accounts = useAccountsStore();
const message = useMessage();
const pinsStore = usePinsStore();
const settingsStore = useSettingsStore();
const launching = ref(false);
const showPicker = ref(false);
const pinLaunching = ref<string>("");
const pinStatus = ref<Record<string, ServerStatus>>({});

const STORAGE_KEY = "qookix.home.selected";

const lastPlayed = computed(() =>
  [...instances.instances].sort((a, b) => (b.last_played ?? 0) - (a.last_played ?? 0))[0] ?? null
);

// 常驻实例：默认取上次选择 / 最近游玩 / 第一个实例
const selectedId = ref<string | null>(null);

function resolveDefault(): string | null {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved && instances.get(saved)) return saved;
  return (lastPlayed.value ?? instances.instances[0])?.id ?? null;
}

const selected = computed(() =>
  selectedId.value ? (instances.get(selectedId.value) ?? null) : null
);

watch(
  () => instances.instances,
  () => {
    if (!selectedId.value) selectedId.value = resolveDefault();
  },
  { immediate: true }
);

watch(selectedId, (id) => {
  if (id) localStorage.setItem(STORAGE_KEY, id);
});

// 切换实例弹窗按分组展示（空分组不显示）
const pickerSections = computed(() => {
  const list = instances.groups
    .map((g) => ({
      key: g.id,
      name: g.name,
      color: g.color as string | null,
      items: instances.instances.filter((i) => i.group === g.id),
    }))
    .filter((s) => s.items.length);
  const rest = instances.instances.filter((i) => !i.group);
  if (rest.length) {
    list.push({ key: "__ungrouped__", name: "未分组", color: null, items: rest });
  }
  return list;
});

const greeting = computed(() => {
  const h = new Date().getHours();
  if (h >= 5 && h < 11) return "早上好";
  if (h >= 11 && h < 13) return "中午好";
  if (h >= 13 && h < 18) return "下午好";
  if (h >= 18 && h < 22) return "晚上好";
  return "夜深了";
});

const hasAccount = computed(() => accounts.accounts.length > 0);

async function launchSelected() {
  const target = selected.value;
  if (!target) {
    message.info("还没有实例，先创建一个吧");
    router.push("/instances");
    return;
  }
  if (!hasAccount.value) {
    message.warning("请先在左下角账号栏添加账号（正版或离线）");
    accounts.showManager = true;
    return;
  }
  launching.value = true;
  try {
    await instances.launch(target.id);
    message.success(`已启动 ${target.name}`);
  } catch (e) {
    message.error(String(e));
  } finally {
    launching.value = false;
  }
}

function pick(inst: { id: string }) {
  selectedId.value = inst.id;
  showPicker.value = false;
}

function openPicker() {
  if (!instances.instances.length) {
    message.info("还没有实例，先创建一个吧");
    router.push("/instances");
    return;
  }
  showPicker.value = true;
}

// —— 固定快捷启动 ——
// 只展示固定到首页的项（侧边栏固定不在此显示）；实例被删除后自动隐藏对应固定项
const validPins = computed(() =>
  pinsStore.items.filter((p) => p.target === "home" && instances.get(p.instanceId))
);

function pinIconSrc(p: PinItem): string | undefined {
  if (p.type !== "server") return undefined;
  const fav = pinStatus.value[p.id]?.favicon;
  if (fav) return fav;
  if (p.icon) return `data:image/png;base64,${p.icon}`;
  return undefined;
}

function worldIconSrc(p: PinItem): string | undefined {
  if (!p.icon) return undefined;
  if (p.icon.startsWith("http://") || p.icon.startsWith("https://") || p.icon.startsWith("data:")) return p.icon;
  return convertFileSrc(p.icon);
}

async function pingPin(p: PinItem) {
  if (p.type !== "server" || !p.address) return;
  try {
    const st = await api.pingServer(p.address);
    pinStatus.value = { ...pinStatus.value, [p.id]: st };
  } catch {
    pinStatus.value = {
      ...pinStatus.value,
      [p.id]: { online: false, address: p.address, name: null, version: null, players_online: null, players_max: null, motd: null, favicon: null, latency_ms: null, error: null },
    };
  }
}

async function launchPin(p: PinItem) {
  if (!hasAccount.value) {
    message.warning("请先在左下角账号栏添加账号（正版或离线）");
    accounts.showManager = true;
    return;
  }
  pinLaunching.value = p.id;
  if (p.type === "world" && !supportsQuickPlay(p.mcVersion)) {
    message.info(`此实例是 ${p.mcVersion}，不支持命令行直达存档，将启动游戏后手动进入存档`);
  }
  try {
    await instances.launch(p.instanceId, p.world, p.address);
    const msg =
      p.type === "server" ? `正在加入服务器「${p.name}」`
      : p.type === "world" ? `正在进入世界「${p.name}」`
      : `正在启动实例「${p.name}」`;
    message.success(msg);
  } catch (e) {
    message.error(String(e));
  } finally {
    pinLaunching.value = "";
  }
}

function unpin(p: PinItem) {
  pinsStore.remove(p.id);
}

function openInstance(p: PinItem) {
  if (p.type === "instance") {
    router.push(`/instance/${p.instanceId}`);
  } else {
    router.push({ path: `/instance/${p.instanceId}`, query: { tab: "saves" } });
  }
}

function pinTypeLabel(t: PinItem["type"]): string {
  return t === "server" ? "服务器" : t === "world" ? "存档" : "实例";
}

onMounted(() => {
  instances.load();
  accounts.load();
  settingsStore.load();
  pinsStore.items.filter((p) => p.type === "server").forEach((p) => pingPin(p));
});
</script>

<template>
  <div class="home">
    <section v-if="settingsStore.settings?.show_home_hero" class="hero glass">
      <div class="hero-glow"></div>
      <div class="hero-text">
        <div class="greeting">{{ greeting }}</div>
        <h1>开始你的 <span class="accent">方块之旅</span></h1>
        <p>选择一个实例，一键启动你的 Minecraft 世界。</p>
        <div class="hero-actions">
          <button class="btn ghost big" @click="router.push('/browse')">
            <IconCompass /> 浏览内容
          </button>
          <button class="btn ghost big" @click="accounts.showManager = true">
            <IconUser /> 切换账号
          </button>
        </div>
      </div>
      <div class="hero-logo">
        <img src="/app-icon.png" class="hero-logo-img" draggable="false" alt="" />
      </div>
    </section>

    <section v-if="validPins.length" class="pin-block">
      <div class="pin-grid">
        <div v-for="p in validPins" :key="p.id" class="pin-card glass" @click="openInstance(p)">
          <div class="pin-icon">
            <template v-if="p.type === 'server'">
              <img v-if="pinIconSrc(p)" :src="pinIconSrc(p)" class="pin-icon-img" alt="" />
              <IconGlobe v-else />
            </template>
            <template v-else-if="p.type === 'world'">
              <img v-if="worldIconSrc(p)" :src="worldIconSrc(p)" class="pin-icon-img" alt="" />
              <IconFolder v-else />
            </template>
            <template v-else>
              <AppIcon :name="p.instanceIcon" />
            </template>
          </div>
          <div class="pin-info">
            <div class="pin-title text-ellipsis">{{ p.name }}</div>
            <div class="pin-meta">
              <span class="pin-type" :class="p.type">{{ pinTypeLabel(p.type) }}</span>
              <span v-if="p.type === 'instance'" class="pin-inst text-ellipsis">{{ p.mcVersion }} · {{ p.loader }}</span>
              <span v-else class="pin-inst text-ellipsis">{{ p.instanceName }}</span>
            </div>
            <div v-if="p.type === 'server'" class="pin-status">
              <span v-if="pinStatus[p.id]" :class="['latency', latencyInfo(pinStatus[p.id].latency_ms).tier]">
                <span class="bars">
                  <i v-for="n in 5" :key="n" :class="{ on: n <= latencyInfo(pinStatus[p.id].latency_ms).count }"></i>
                </span>
                <span v-if="pinStatus[p.id].latency_ms != null">{{ pinStatus[p.id].latency_ms }} ms</span>
                <span v-else-if="!pinStatus[p.id].online">离线</span>
                <span v-else>…</span>
              </span>
              <span v-if="pinStatus[p.id]?.players_online != null" class="players">
                {{ pinStatus[p.id].players_online }} 人在线
              </span>
            </div>
          </div>
          <div class="pin-actions">
            <button class="pin-unpin" title="取消固定" @click.stop="unpin(p)">
              <IconClose />
            </button>
            <button class="btn primary" :disabled="pinLaunching === p.id" @click.stop="launchPin(p)">
              <IconPlay /> {{ pinLaunching === p.id ? "启动中…" : "启动" }}
            </button>
          </div>
        </div>
      </div>
    </section>

    <section class="section">
      <div v-if="!instances.instances.length" class="empty glass">
        <p>还没有游戏实例</p>
        <button class="btn primary" @click="router.push('/instances')">创建第一个实例</button>
      </div>

      <div v-else-if="selected" class="resident glass">
        <div class="resident-icon"><AppIcon :name="selected.icon" /></div>
        <div class="resident-info">
          <div class="resident-name text-ellipsis">{{ selected.name }}</div>
          <div class="resident-meta">
            <span class="badge">{{ loaderBadge(selected.loader) }}</span>
            <span class="ver-text">{{ selected.mc_version }}</span>
            <span v-if="selected.loader_version" class="ver-text">· {{ selected.loader_version }}</span>
            <span v-if="selected.last_played" class="ver-text">
              · 最近 {{ new Date(selected.last_played * 1000).toLocaleDateString() }}
            </span>
          </div>
        </div>
        <div class="resident-actions">
          <button class="btn ghost" @click="openPicker">
            <IconRepeat /> 切换实例
          </button>
          <button class="btn primary big" :disabled="launching" @click="launchSelected">
            <IconPlay />
            <span>{{ launching ? "启动中…" : "启动游戏" }}</span>
          </button>
        </div>
      </div>
    </section>

    <n-modal
      :show="showPicker"
      @update:show="(v: boolean) => (showPicker = v)"
      @mask-click="() => (showPicker = false)"
      preset="card"
      title="切换实例"
      style="width: 720px; max-width: 92vw"
    >
      <div class="pick-scroll">
        <section v-for="s in pickerSections" :key="s.key" class="pick-section">
          <div class="pick-group">
            <i class="dot" :style="{ background: s.color || 'var(--text-3)' }"></i>
            <span>{{ s.name }}</span>
            <span class="pick-group-count">{{ s.items.length }}</span>
          </div>
          <div class="pick-grid">
            <div
              v-for="inst in s.items"
              :key="inst.id"
              class="pick-card"
              :class="{ active: selected?.id === inst.id }"
              @click="pick(inst)"
            >
              <div class="pick-icon"><AppIcon :name="inst.icon" /></div>
              <div class="pick-info">
                <div class="pick-name text-ellipsis">{{ inst.name }}</div>
                <div class="pick-meta">
                  <span class="badge">{{ loaderBadge(inst.loader) }}</span>
                  <span class="ver-text">{{ inst.mc_version }}</span>
                </div>
              </div>
              <div v-if="selected?.id === inst.id" class="pick-current">当前</div>
            </div>
          </div>
        </section>
      </div>
    </n-modal>
  </div>
</template>

<style scoped>
.home {
  display: flex;
  flex-direction: column;
  gap: 18px;
  min-height: 100%;
}
.hero {
  position: relative;
  overflow: hidden;
  padding: 20px 24px;
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
}
.hero-glow {
  position: absolute;
  width: 420px;
  height: 420px;
  right: -80px;
  top: -180px;
  background: radial-gradient(circle, var(--accent-25), transparent 65%);
  pointer-events: none;
}
.hero-text {
  position: relative;
  z-index: 1;
}
.hero h1 {
  font-size: 30px;
  margin: 0 0 10px;
  letter-spacing: 0.3px;
  line-height: 1.1;
}
.hero p {
  color: var(--text-2);
  margin: 0 0 22px;
  max-width: 520px;
  line-height: 1.6;
}
.greeting {
  font-size: 18px;
  font-weight: 600;
  color: var(--accent);
  margin: 0;
  line-height: 1.2;
  letter-spacing: 0.5px;
}
.accent {
  color: var(--accent);
}
.hero-actions {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
}
.btn {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border: none;
  border-radius: 10px;
  padding: 9px 18px;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.15s;
  font-family: inherit;
}
.btn.big {
  padding: 10px 22px;
  font-size: 14px;
}
.btn.primary {
  background: linear-gradient(135deg, var(--accent), var(--accent-deep));
  color: #1a1208;
  box-shadow: 0 6px 22px var(--accent-35);
}
.btn.primary:hover {
  filter: brightness(1.08);
}
.btn.primary:disabled {
  opacity: 0.7;
  cursor: default;
}
.btn.ghost {
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-1);
  border: 1px solid var(--border);
}
.btn.ghost:hover {
  background: rgba(255, 255, 255, 0.1);
  transform: none;
}
.hero-logo {
  position: relative;
  z-index: 1;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}
.hero-logo-img {
  width: 128px;
  height: 128px;
  border-radius: 24px;
  object-fit: contain;
}
/* 页面整体不再限宽；只有下方这张实例启动卡片保持限宽居中，
   否则超宽屏上会被拉成一条很长的横条 */
.section {
  margin-top: auto;
  width: 100%;
  max-width: 1080px;
  margin-left: auto;
  margin-right: auto;
}
/* 固定快捷启动 */
.pin-block {
  margin-top: 8px;
}
.pin-sub {
  color: var(--text-3);
  font-size: 12px;
}
.pin-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 16px;
}
.pin-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 18px;
  cursor: pointer;
  transition: border-color 0.15s, transform 0.15s, background 0.15s;
}
.pin-card:hover {
  border-color: var(--accent-45);
  background: rgba(255, 255, 255, 0.06);
  transform: translateY(-1px);
}
.pin-icon {
  width: 46px;
  height: 46px;
  border-radius: 12px;
  flex-shrink: 0;
  background: linear-gradient(135deg, var(--accent-22), var(--accent-08));
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  color: var(--accent);
  overflow: hidden;
}
.pin-icon-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.pin-info {
  flex: 1;
  min-width: 0;
}
.pin-title {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 6px;
}
.pin-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}
.pin-type {
  border-radius: 6px;
  padding: 0 6px;
  font-weight: 600;
  flex-shrink: 0;
}
.pin-type.server {
  background: rgba(90, 176, 255, 0.15);
  color: #5ab0ff;
}
.pin-type.world {
  background: rgba(122, 208, 138, 0.15);
  color: #7ad08a;
}
.pin-type.instance {
  background: var(--accent-16);
  color: var(--accent);
}
.pin-inst {
  color: var(--text-3);
}
.pin-status {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 6px;
  font-size: 12px;
}
.pin-status .latency {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.pin-status .bars {
  display: inline-flex;
  gap: 2px;
}
.pin-status .bars i {
  width: 4px;
  height: 10px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.15);
}
.pin-status .bars i.on {
  background: currentColor;
}
.pin-status .latency.good {
  color: #7ad08a;
}
.pin-status .latency.mid {
  color: #ffc34d;
}
.pin-status .latency.bad {
  color: #ff6b6b;
}
.pin-status .latency.off {
  color: var(--text-3);
}
.pin-status .players {
  color: var(--text-3);
}
.pin-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.pin-actions .btn {
  padding: 8px 14px;
  font-size: 13px;
}
.pin-unpin {
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-3);
  border-radius: 8px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
}
.pin-unpin:hover {
  background: rgba(255, 255, 255, 0.08);
  color: #ff6b6b;
}
.section-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  margin-bottom: 14px;
}
.section-head h2 {
  font-size: 17px;
  margin: 0;
}
.link-btn {
  background: none;
  border: none;
  color: var(--text-3);
  cursor: pointer;
  font-size: 13px;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.link-btn:hover {
  color: var(--accent);
}
.empty {
  padding: 40px;
  text-align: center;
  color: var(--text-3);
  display: flex;
  flex-direction: column;
  gap: 14px;
  align-items: center;
}

/* 常驻实例卡片 */
.resident {
  padding: 20px 24px;
  display: flex;
  align-items: center;
  gap: 16px;
}
.resident-icon {
  width: 64px;
  height: 64px;
  border-radius: 16px;
  overflow: hidden;
  background: linear-gradient(135deg, var(--accent-25), var(--accent-08));
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 28px;
  color: var(--accent);
  flex-shrink: 0;
}
.resident-info {
  flex: 1;
  min-width: 0;
}
.resident-name {
  font-size: 18px;
  font-weight: 600;
  margin-bottom: 8px;
}
.resident-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  flex-wrap: wrap;
}
.badge {
  background: var(--accent-16);
  color: var(--accent);
  border-radius: 6px;
  padding: 1px 7px;
  font-weight: 600;
}
.ver-text {
  color: var(--text-3);
}
.resident-actions {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-shrink: 0;
}

/* 切换弹窗 */
.pick-scroll {
  display: flex;
  flex-direction: column;
  gap: 16px;
  max-height: 56vh;
  overflow-y: auto;
  padding-right: 4px;
}
.pick-group {
  display: flex;
  align-items: center;
  gap: 7px;
  margin-bottom: 10px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
}
.pick-group .dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  flex-shrink: 0;
}
.pick-group-count {
  font-size: 11px;
  color: var(--text-3);
}
.pick-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 12px;
}
.pick-card {
  position: relative;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px;
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid var(--border);
  cursor: pointer;
  transition: border-color 0.15s, transform 0.15s, background 0.15s;
}
.pick-card:hover {
  background: rgba(255, 255, 255, 0.07);
}
.pick-card.active {
  border-color: var(--accent);
  box-shadow: 0 0 0 1px var(--accent);
  background: var(--accent-01);
}
.pick-icon {
  width: 42px;
  height: 42px;
  border-radius: 11px;
  overflow: hidden;
  background: linear-gradient(135deg, var(--accent-25), var(--accent-08));
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 19px;
  color: var(--accent);
  flex-shrink: 0;
}
.pick-info {
  min-width: 0;
  flex: 1;
}
.pick-name {
  font-weight: 600;
  font-size: 14px;
  margin-bottom: 5px;
}
.pick-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}
.pick-current {
  position: absolute;
  top: 8px;
  right: 8px;
  font-size: 11px;
  font-weight: 600;
  color: var(--accent);
  background: var(--accent-soft);
  border-radius: 6px;
  padding: 1px 6px;
}
</style>
