<script setup lang="ts">
import { onMounted } from "vue";
import { useRouter } from "vue-router";
import { useInstancesStore } from "../stores/instances";
import InstanceCard from "../components/InstanceCard.vue";
import { IconGrid, IconPlus } from "../components/icons";

const instances = useInstancesStore();
const router = useRouter();

onMounted(() => instances.load());
</script>

<template>
  <div class="instances-view">
    <div class="head">
      <div>
        <h1>游戏实例</h1>
        <p class="sub">每个实例拥有独立的游戏目录、模组与配置</p>
      </div>
      <button class="btn primary" @click="router.push('/create')">
        <IconPlus /> 新建实例
      </button>
    </div>

    <div v-if="instances.loading" class="loading">加载中…</div>
    <div v-else-if="!instances.instances.length" class="empty glass">
      <div class="empty-icon"><IconGrid /></div>
      <p>还没有任何实例，创建一个开始游戏吧</p>
      <button class="btn primary" @click="router.push('/create')">创建第一个实例</button>
    </div>
    <div v-else class="grid">
      <InstanceCard v-for="inst in instances.instances" :key="inst.id" :instance="inst" />
    </div>
  </div>
</template>

<style scoped>
.instances-view {
  max-width: 1080px;
  margin: 0 auto;
}
.head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  margin-bottom: 22px;
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
  box-shadow: 0 6px 22px rgba(232, 154, 75, 0.3);
}
.btn.primary:hover {
  filter: brightness(1.08);
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 16px;
}
.loading {
  padding: 60px;
  text-align: center;
  color: var(--text-3);
}
.empty {
  padding: 50px;
  text-align: center;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  color: var(--text-3);
}
.empty-icon {
  font-size: 34px;
  opacity: 0.6;
}
</style>
