<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useInstancesStore } from "../stores/instances";
import { useAccountsStore } from "../stores/accounts";
import { useMessage } from "naive-ui";
import AppIcon from "../components/AppIcon.vue";
import { IconChevronRight, IconCompass, IconPlay, IconUser } from "../components/icons";

const router = useRouter();
const instances = useInstancesStore();
const accounts = useAccountsStore();
const message = useMessage();
const launching = ref(false);

const lastPlayed = computed(() =>
  [...instances.instances].sort((a, b) => (b.last_played ?? 0) - (a.last_played ?? 0))[0] ?? null
);

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

async function quickLaunch() {
  const target = lastPlayed.value ?? instances.instances[0];
  if (!target) {
    message.info("还没有实例，先创建一个吧");
    router.push("/instances");
    return;
  }
  if (!accounts.accounts.length) {
    message.warning("请先在左下角账号栏添加账号（正版或离线）");
    accounts.showManager = true;
    return;
  }
  launching.value = true;
  try {
    await instances.launch(target.id);
    message.success("游戏已启动");
  } catch (e) {
    message.error(String(e));
  } finally {
    launching.value = false;
  }
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
        <div class="hero-actions">
          <button class="btn primary big" :disabled="launching" @click="quickLaunch">
            <IconPlay />
            <span>{{ launching ? "启动中…" : "快速开始" }}</span>
          </button>
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
      <div class="section-head">
        <h2>最近玩过的实例</h2>
        <button class="link-btn" @click="router.push('/instances')">
          查看全部 <IconChevronRight />
        </button>
      </div>
      <div v-if="!instances.instances.length" class="empty glass">
        <p>还没有游戏实例</p>
        <button class="btn primary" @click="router.push('/instances')">创建第一个实例</button>
      </div>
      <div v-else class="inst-grid">
        <div
          v-for="inst in instances.instances.slice(0, 4)"
          :key="inst.id"
          class="mini-card glass clickable"
          @click="router.push(`/instance/${inst.id}`)"
        >
          <div class="mini-icon"><AppIcon :name="inst.icon" /></div>
          <div class="mini-info">
            <div class="mini-name text-ellipsis">{{ inst.name }}</div>
            <div class="mini-meta">
              <span class="badge">{{ loaderBadge(inst) }}</span>
              <span class="ver-text">{{ inst.mc_version }}</span>
            </div>
          </div>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.home {
  display: flex;
  flex-direction: column;
  gap: 26px;
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
.hero p {
  color: var(--text-2);
  margin: 0 0 22px;
  max-width: 520px;
  line-height: 1.6;
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
  padding: 8px 16px;
  font-size: 13px;
}
.btn.primary {
  background: linear-gradient(135deg, var(--accent), var(--accent-deep));
  color: #1a1208;
  box-shadow: 0 6px 22px rgba(232, 154, 75, 0.35);
}
.btn.primary:hover {
  filter: brightness(1.08);
  transform: translateY(-1px);
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
.inst-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
  gap: 14px;
}
.mini-card {
  display: flex;
  align-items: center;
  gap: 13px;
  padding: 14px;
}
.mini-icon {
  width: 44px;
  height: 44px;
  border-radius: 12px;
  overflow: hidden;
  background: linear-gradient(135deg, rgba(232, 154, 75, 0.25), rgba(232, 154, 75, 0.08));
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  color: var(--accent);
  flex-shrink: 0;
}
.mini-info {
  min-width: 0;
}
.mini-name {
  font-weight: 600;
  font-size: 14px;
  margin-bottom: 5px;
}
.mini-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
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
.state.ok {
  color: #4ec9a0;
}
.state.warn {
  color: #e0a030;
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
</style>
