<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useInstancesStore } from "../stores/instances";
import { useAccountsStore } from "../stores/accounts";
import { useMessage, NModal } from "naive-ui";
import AppIcon from "../components/AppIcon.vue";
import { IconCompass, IconPlay, IconRepeat, IconUser } from "../components/icons";

const router = useRouter();
const instances = useInstancesStore();
const accounts = useAccountsStore();
const message = useMessage();
const launching = ref(false);
const showPicker = ref(false);

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

function loaderBadge(i: { loader: string }) {
  return i.loader === "vanilla" ? "原版" : i.loader.charAt(0).toUpperCase() + i.loader.slice(1);
}

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

onMounted(() => {
  instances.load();
  accounts.load();
});
</script>

<template>
  <div class="home">
    <section class="hero glass">
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
            <span class="badge">{{ loaderBadge(selected) }}</span>
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
      <div class="pick-grid">
        <div
          v-for="inst in instances.instances"
          :key="inst.id"
          class="pick-card"
          :class="{ active: selected?.id === inst.id }"
          @click="pick(inst)"
        >
          <div class="pick-icon"><AppIcon :name="inst.icon" /></div>
          <div class="pick-info">
            <div class="pick-name text-ellipsis">{{ inst.name }}</div>
            <div class="pick-meta">
              <span class="badge">{{ loaderBadge(inst) }}</span>
              <span class="ver-text">{{ inst.mc_version }}</span>
            </div>
          </div>
          <div v-if="selected?.id === inst.id" class="pick-current">当前</div>
        </div>
      </div>
    </n-modal>
  </div>
</template>

<style scoped>
.home {
  display: flex;
  flex-direction: column;
  gap: 26px;
  min-height: 100%;
  max-width: 1080px;
  margin: 0 auto;
}
.hero {
  position: relative;
  overflow: hidden;
  padding: 22px 38px;
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 24px;
}
.hero-glow {
  position: absolute;
  width: 420px;
  height: 420px;
  right: -80px;
  top: -180px;
  background: radial-gradient(circle, rgba(232, 154, 75, 0.25), transparent 65%);
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
  box-shadow: 0 6px 22px rgba(232, 154, 75, 0.35);
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
.section {
  margin-top: auto;
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
  gap: 18px;
}
.resident-icon {
  width: 64px;
  height: 64px;
  border-radius: 16px;
  overflow: hidden;
  background: linear-gradient(135deg, rgba(232, 154, 75, 0.25), rgba(232, 154, 75, 0.08));
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
  background: rgba(232, 154, 75, 0.16);
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
.pick-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 12px;
  max-height: 56vh;
  overflow-y: auto;
  padding-right: 4px;
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
  background: rgba(232, 154, 75, 0.1);
}
.pick-icon {
  width: 42px;
  height: 42px;
  border-radius: 11px;
  overflow: hidden;
  background: linear-gradient(135deg, rgba(232, 154, 75, 0.25), rgba(232, 154, 75, 0.08));
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
