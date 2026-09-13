<script setup lang="ts">
import { useRouter } from "vue-router";
import { IconMapPin, IconTool } from "../components/icons";

const router = useRouter();

const tools = [
  {
    key: "seed",
    title: "种子地图",
    desc: "在本地以地貌阴影 + 生物群系着色浏览世界种子，可叠加结构与高亮群系图层等功能。",
    icon: IconMapPin,
    to: "/toolbox/seed",
  },
];

function go(to: string) {
  router.push(to);
}
</script>

<template>
  <div id="toolbox-root" class="toolbox-view">
    <div class="grid">
      <section v-for="t in tools" :key="t.key" class="card glass clickable" @click="go(t.to)">
        <h3><component :is="t.icon" /> {{ t.title }}</h3>
        <p class="hint">{{ t.desc }}</p>
      </section>
      <section class="card glass placeholder">
        <h3><IconTool /> 更多功能</h3>
        <p class="hint">更多工具即将推出。</p>
      </section>
    </div>
  </div>
</template>

<style scoped>
.toolbox-view { display: flex; flex-direction: column; gap: 16px; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(340px, 1fr)); gap: 16px; }
.card { padding: 18px; }
.card h3 { display: flex; align-items: center; gap: 8px; margin: 0 0 14px; font-size: 14px; }
.card h3 svg { color: var(--accent); }
.hint { margin: 0; font-size: 12px; color: var(--text-3); line-height: 1.6; }
.placeholder { opacity: 0.6; cursor: default; }
</style>
