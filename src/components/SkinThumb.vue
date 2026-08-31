<script setup lang="ts">
import { onMounted, ref, watch } from "vue";

const props = withDefaults(
  defineProps<{ src: string | null; slim?: boolean }>(),
  { slim: false },
);

const canvasRef = ref<HTMLCanvasElement | null>(null);

function draw() {
  const canvas = canvasRef.value;
  if (!canvas || !props.src) return;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const img = new Image();
  img.onload = () => {
    if (!canvasRef.value) return;
    const c = canvasRef.value;
    const cx = c.getContext("2d");
    if (!cx) return;
    cx.clearRect(0, 0, c.width, c.height);
    cx.imageSmoothingEnabled = false;
    const s = (u: number, v: number, w: number, h: number, x: number, y: number) =>
      cx.drawImage(img, u, v, w, h, x, y, w, h);
    // slim 模型手臂宽 3px；为了让双臂都紧贴身体、左右对称，
    // 向右偏移 1px，画布两侧各留 1px 空白。
    const armW = props.slim ? 3 : 4;
    const rightArmX = props.slim ? 1 : 0;
    const leftArmX = 12;
    s(8, 8, 8, 8, 4, 0);
    s(20, 20, 8, 12, 4, 8);
    s(44, 20, armW, 12, rightArmX, 8);
    if (img.height >= 64) {
      s(36, 52, armW, 12, leftArmX, 8);
    } else {
      cx.save();
      cx.translate(16, 0);
      cx.scale(-1, 1);
      cx.drawImage(img, 44, 20, armW, 12, 0, 8, armW, 12);
      cx.restore();
    }
    // 第二层（overlay）—— 仅在 64px 高清皮肤上存在
    if (img.height >= 64) {
      s(40, 8, 8, 8, 4, 0); // 头部 overlay
      s(20, 36, 8, 12, 4, 8); // 身体 overlay
      s(44, 36, armW, 12, rightArmX, 8); // 右臂 overlay
      s(52, 52, armW, 12, leftArmX, 8); // 左臂 overlay
    }
  };
  img.src = props.src;
}

onMounted(draw);
watch(() => props.src, draw);
watch(() => props.slim, draw);
</script>

<template>
  <canvas ref="canvasRef" class="skin-thumb" width="16" height="16"></canvas>
</template>

<style scoped>
.skin-thumb {
  width: 100%;
  height: 100%;
  image-rendering: pixelated;
  image-rendering: crisp-edges;
}
</style>
