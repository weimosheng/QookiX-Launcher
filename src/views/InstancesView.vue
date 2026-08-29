<script setup lang="ts">
import { onMounted } from "vue";
import { useRouter } from "vue-router";
import { useInstancesStore } from "../stores/instances";
import InstanceCard from "../components/InstanceCard.vue";
import { IconGrid } from "../components/icons";

const instances = useInstancesStore();
const router = useRouter();

onMounted(() => instances.load());
</script>

<template>
  <div class="instances-view">
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
