<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { hexToRgb, rgbToHex } from "../theme";

const props = defineProps<{ show: boolean; color: string; presets?: string[] }>();
const emit = defineEmits<{ "update:show": [v: boolean]; "update:color": [v: string] }>();

const h = ref(0);
const s = ref(1);
const v = ref(1);
const hexInput = ref("");

function hexToHsv(hex: string) {
  const { r, g, b } = hexToRgb(hex);
  const rn = r / 255, gn = g / 255, bn = b / 255;
  const max = Math.max(rn, gn, bn), min = Math.min(rn, gn, bn);
  const d = max - min;
  let hh = 0;
  if (d !== 0) {
    if (max === rn) hh = ((gn - bn) / d) % 6;
    else if (max === gn) hh = (bn - rn) / d + 2;
    else hh = (rn - gn) / d + 4;
    hh *= 60;
    if (hh < 0) hh += 360;
  }
  return { h: hh, s: max === 0 ? 0 : d / max, v: max };
}

function hsvToHex(hh: number, ss: number, vv: number) {
  const c = vv * ss;
  const x = c * (1 - Math.abs(((hh / 60) % 2) - 1));
  const m = vv - c;
  let r = 0, g = 0, b = 0;
  if (hh < 60) { r = c; g = x; }
  else if (hh < 120) { r = x; g = c; }
  else if (hh < 180) { g = c; b = x; }
  else if (hh < 240) { g = x; b = c; }
  else if (hh < 300) { r = x; b = c; }
  else { r = c; b = x; }
  return rgbToHex(Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255));
}

function syncFromColor(hex: string) {
  const hsv = hexToHsv(hex);
  h.value = hsv.h;
  s.value = hsv.s;
  v.value = hsv.v;
  hexInput.value = hex.toLowerCase();
}

watch(
  () => props.show,
  (open) => {
    if (open) syncFromColor(props.color);
  },
);

const currentHex = computed(() => hsvToHex(h.value, s.value, v.value));

function commit() {
  hexInput.value = currentHex.value.toLowerCase();
  emit("update:color", currentHex.value);
}

const svRef = ref<HTMLElement | null>(null);
let svDragging = false;

function updateSv(e: PointerEvent) {
  const el = svRef.value;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  s.value = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
  v.value = Math.max(0, Math.min(1, 1 - (e.clientY - rect.top) / rect.height));
  hexInput.value = currentHex.value.toLowerCase();
}

function onSvPointerDown(e: PointerEvent) {
  svDragging = true;
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  updateSv(e);
}
function onSvPointerMove(e: PointerEvent) {
  if (!svDragging) return;
  updateSv(e);
}
function onSvPointerUp(e: PointerEvent) {
  if (!svDragging) return;
  svDragging = false;
  (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  commit();
}

const hueRef = ref<HTMLElement | null>(null);
let hueDragging = false;

function updateHue(e: PointerEvent) {
  const el = hueRef.value;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  h.value = Math.max(0, Math.min(360, ((e.clientX - rect.left) / rect.width) * 360));
  hexInput.value = currentHex.value.toLowerCase();
}

function onHuePointerDown(e: PointerEvent) {
  hueDragging = true;
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  updateHue(e);
}
function onHuePointerMove(e: PointerEvent) {
  if (!hueDragging) return;
  updateHue(e);
}
function onHuePointerUp(e: PointerEvent) {
  if (!hueDragging) return;
  hueDragging = false;
  (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  commit();
}

function onHexInput(e: Event) {
  let val = (e.target as HTMLInputElement).value.trim();
  if (!val.startsWith("#")) val = "#" + val;
  if (val.length !== 7) return;
  const rgb = hexToRgb(val);
  const hex = rgbToHex(rgb.r, rgb.g, rgb.b);
  const hsv = hexToHsv(hex);
  h.value = hsv.h;
  s.value = hsv.s;
  v.value = hsv.v;
  hexInput.value = hex.toLowerCase();
  emit("update:color", hex);
}

function pickPreset(hex: string) {
  syncFromColor(hex);
  emit("update:color", hex);
}

function onOverlayDown(e: MouseEvent) {
  if (e.target === e.currentTarget) emit("update:show", false);
}
function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") emit("update:show", false);
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="show"
      class="cp-overlay"
      @mousedown="onOverlayDown"
      @keydown="onKeydown"
      tabindex="-1"
    >
      <div class="cp-panel glass">
        <div
          ref="svRef"
          class="cp-sv"
          :style="{
            background:
              'linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, hsl(' +
              h +
              ', 100%, 50%))',
          }"
          @pointerdown="onSvPointerDown"
          @pointermove="onSvPointerMove"
          @pointerup="onSvPointerUp"
        >
          <div class="cp-sv-cursor" :style="{ left: s * 100 + '%', top: (1 - v) * 100 + '%' }"></div>
        </div>
        <div
          ref="hueRef"
          class="cp-hue"
          @pointerdown="onHuePointerDown"
          @pointermove="onHuePointerMove"
          @pointerup="onHuePointerUp"
        >
          <div class="cp-hue-cursor" :style="{ left: (h / 360) * 100 + '%' }"></div>
        </div>
        <div class="cp-bottom">
          <div class="cp-preview" :style="{ background: currentHex }"></div>
          <input
            class="cp-hex"
            :value="hexInput"
            @input="onHexInput"
            maxlength="7"
            spellcheck="false"
            autocomplete="off"
          />
        </div>
        <div v-if="presets && presets.length" class="cp-presets">
          <button
            v-for="p in presets"
            :key="p"
            type="button"
            class="cp-preset"
            :class="{ active: currentHex.toLowerCase() === p.toLowerCase() }"
            :style="{ background: p }"
            :title="p"
            @click="pickPreset(p)"
          ></button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.cp-overlay {
  position: fixed;
  inset: 0;
  z-index: 2000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.22);
  backdrop-filter: blur(2px);
}
:global(.light) .cp-overlay {
  background: rgba(0, 0, 0, 0.1);
}
.cp-panel {
  width: 232px;
  padding: 14px;
  border-radius: 16px;
  background: rgba(24, 27, 36, 0.92);
  color: var(--text-1);
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.34);
  border: 1px solid var(--border);
  backdrop-filter: blur(18px) saturate(1.2);
}
:global(.light) .cp-panel {
  background: rgba(255, 255, 255, 0.94);
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.16);
}
.cp-sv {
  width: 100%;
  height: 150px;
  border-radius: 10px;
  position: relative;
  cursor: crosshair;
  touch-action: none;
  user-select: none;
}
.cp-sv-cursor {
  position: absolute;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 2px solid #fff;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.4), 0 1px 3px rgba(0, 0, 0, 0.3);
  transform: translate(-50%, -50%);
  pointer-events: none;
}
.cp-hue {
  width: 100%;
  height: 12px;
  margin-top: 12px;
  border-radius: 6px;
  background: linear-gradient(
    to right,
    #f00,
    #ff0,
    #0f0,
    #0ff,
    #00f,
    #f0f,
    #f00
  );
  position: relative;
  cursor: pointer;
  touch-action: none;
  user-select: none;
}
.cp-hue-cursor {
  position: absolute;
  top: 50%;
  width: 6px;
  height: 18px;
  border-radius: 3px;
  background: #fff;
  border: 1px solid rgba(0, 0, 0, 0.25);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
  transform: translate(-50%, -50%);
  pointer-events: none;
}
.cp-bottom {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
}
.cp-preview {
  width: 28px;
  height: 28px;
  border-radius: 8px;
  border: 1px solid var(--border);
  flex-shrink: 0;
}
.cp-hex {
  flex: 1;
  height: 28px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--panel);
  color: var(--text-1);
  padding: 0 8px;
  font-size: 13px;
  font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  text-transform: lowercase;
  outline: none;
  transition: border-color 0.15s ease;
}
.cp-hex:focus {
  border-color: var(--accent);
}
.cp-presets {
  display: flex;
  gap: 6px;
  margin-top: 12px;
  flex-wrap: wrap;
}
.cp-preset {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: 2px solid transparent;
  cursor: pointer;
  padding: 0;
  transition: transform 0.12s ease, border-color 0.12s ease;
}
.cp-preset:hover {
  transform: scale(1.15);
}
.cp-preset.active {
  border-color: var(--text-1);
}
</style>
