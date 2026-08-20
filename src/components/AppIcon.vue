<script setup lang="ts">
import { computed } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { parseIcon } from "../instance-icons";

const props = defineProps<{ name?: string | null }>();

const BG_COLORS: Record<string, string> = {
  amber: "linear-gradient(135deg, rgba(232,154,75,0.4), rgba(232,154,75,0.12))",
  blue: "linear-gradient(135deg, rgba(90,162,240,0.4), rgba(90,162,240,0.12))",
  green: "linear-gradient(135deg, rgba(78,201,160,0.4), rgba(78,201,160,0.12))",
  purple: "linear-gradient(135deg, rgba(167,122,224,0.4), rgba(167,122,224,0.12))",
  red: "linear-gradient(135deg, rgba(229,83,75,0.4), rgba(229,83,75,0.12))",
  slate: "linear-gradient(135deg, rgba(140,150,170,0.4), rgba(140,150,170,0.12))",
  dark: "linear-gradient(135deg, rgba(30,34,44,0.9), rgba(20,23,30,0.9))",
};

const parsed = computed(() => parseIcon(props.name));
const wrapStyle = computed(() => {
  const bg = parsed.value.bg ? BG_COLORS[parsed.value.bg] : "";
  return bg ? { background: bg } : {};
});
const imgSrc = computed(() =>
  parsed.value.img ? convertFileSrc(parsed.value.img) : ""
);
</script>

<template>
  <div class="app-icon" :style="wrapStyle">
    <img v-if="imgSrc" :src="imgSrc" class="app-img" alt="" />
  </div>
</template>

<style scoped>
.app-icon {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}
.app-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  image-rendering: pixelated;
}
</style>
