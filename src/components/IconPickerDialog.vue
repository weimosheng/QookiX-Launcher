<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { NModal, useMessage } from "naive-ui";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api } from "../api";
import AppIcon from "./AppIcon.vue";
import { parseIcon } from "../instance-icons";
import { IconCheck, IconClose } from "./icons";

const props = defineProps<{ show: boolean; value: string; instanceId?: string }>();
const emit = defineEmits<{ "update:show": [v: boolean]; save: [value: string] }>();

const message = useMessage();
const draft = ref(props.value);
const importing = ref(false);
const loading = ref(false);
const gameIcons = ref<{ name: string; label: string; path: string }[]>([]);

const BG_OPTIONS = [
  { name: "无", value: "" },
  { name: "琥珀", value: "amber" },
  { name: "蓝", value: "blue" },
  { name: "绿", value: "green" },
  { name: "紫", value: "purple" },
  { name: "红", value: "red" },
  { name: "灰蓝", value: "slate" },
  { name: "深色", value: "dark" },
];

const preview = computed(() => draft.value);
const currentImg = computed(() => parseIcon(draft.value).img ?? "");

function setBg(bg: string) {
  const parts = draft.value.split(",").filter((p) => p && !p.startsWith("bg:"));
  if (bg) parts.unshift(`bg:${bg}`);
  draft.value = parts.join(",");
}

function setIcon(path: string) {
  const parts = draft.value.split(",").filter((p) => p && !p.startsWith("img:"));
  parts.push(`img:${path}`);
  draft.value = parts.join(",");
}

async function importImage() {
  const file = await open({
    multiple: false,
    filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"] }],
  });
  if (!file) return;
  importing.value = true;
  try {
    const path = await api.importInstanceImage(file as string);
    setIcon(path);
  } catch (e) {
    message.error(String(e));
  } finally {
    importing.value = false;
  }
}

async function loadGameIcons() {
  loading.value = true;
  try {
    gameIcons.value = await api.extractGameIcons(props.instanceId);
  } catch (e) {
    message.warning(String(e));
    gameIcons.value = [];
  } finally {
    loading.value = false;
  }
}

watch(
  () => props.show,
  (v) => {
    if (v) {
      draft.value = props.value;
      loadGameIcons();
    }
  }
);

function save() {
  emit("save", draft.value);
  emit("update:show", false);
}
</script>

<template>
  <n-modal
    :show="props.show"
    preset="card"
    title="选择实例图标"
    style="width: 560px; max-width: 94vw"
    :on-update:show="(v: boolean) => emit('update:show', v)"
  >
    <div class="ip-body">
      <div class="ip-preview">
        <div class="ip-preview-box">
          <AppIcon :name="preview" />
        </div>
        <div class="ip-preview-label">预览</div>
      </div>

      <div class="ip-section">
        <div class="ip-label">背景</div>
        <div class="ip-bgs">
          <button
            v-for="b in BG_OPTIONS"
            :key="b.value"
            class="ip-bg"
            :class="{ active: draft.split(',').find((p) => p.startsWith('bg:'))?.slice(3) === b.value || (!draft.includes('bg:') && b.value === '') }"
            @click="setBg(b.value)"
          >
            <span class="ip-swatch" :class="'bg-' + (b.value || 'none')"></span>
            {{ b.name }}
          </button>
        </div>
      </div>

      <div class="ip-section">
        <div class="ip-label">游戏素材图标</div>
        <div v-if="loading" class="ip-loading">正在从游戏文件提取图标…</div>
        <div v-else-if="gameIcons.length === 0" class="ip-empty">未找到游戏图标，请先安装游戏</div>
        <div v-else class="ip-icons">
          <button
            v-for="icon in gameIcons"
            :key="icon.name"
            class="ip-icon"
            :class="{ active: currentImg === icon.path }"
            :title="icon.label"
            @click="setIcon(icon.path)"
          >
            <img :src="convertFileSrc(icon.path)" class="ip-icon-img" alt="" />
          </button>
        </div>
      </div>

      <div class="ip-section">
        <div class="ip-label">自定义图片</div>
        <button class="ip-import" :disabled="importing" @click="importImage">
          {{ importing ? "导入中…" : "导入图片文件" }}
        </button>
        <span v-if="draft.includes('img:') && !gameIcons.some((g) => g.path === currentImg)" class="ip-img-ok">已使用自定义图片</span>
      </div>
    </div>

    <template #footer>
      <div class="ip-footer">
        <button class="ip-btn" @click="emit('update:show', false)">
          <IconClose /> 取消
        </button>
        <button class="ip-btn primary" @click="save">
          <IconCheck /> 保存
        </button>
      </div>
    </template>
  </n-modal>
</template>

<style>
.ip-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.ip-preview {
  display: flex;
  align-items: center;
  gap: 14px;
}
.ip-preview-box {
  width: 72px;
  height: 72px;
  border-radius: 16px;
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.1);
  overflow: hidden;
  font-size: 34px;
}
.ip-preview-label {
  font-size: 13px;
  color: #8b8e9c;
}
.ip-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.ip-label {
  font-size: 12px;
  font-weight: 700;
  color: #8b8e9c;
}
.ip-bgs {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.ip-bg {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  background: rgba(255, 255, 255, 0.04);
  color: #c6c8d2;
  border-radius: 9px;
  padding: 6px 12px;
  font-size: 12px;
  cursor: pointer;
  font-family: inherit;
}
.ip-bg.active {
  border-color: #e89a4b;
  background: rgba(232, 154, 75, 0.14);
  color: #e89a4b;
}
.ip-swatch {
  width: 16px;
  height: 16px;
  border-radius: 5px;
  background: #2a2e3a;
  display: inline-block;
}
.ip-swatch.bg-amber { background: linear-gradient(135deg, #e8a05a, #b56a24); }
.ip-swatch.bg-blue { background: linear-gradient(135deg, #5aa2f0, #2f6bbd); }
.ip-swatch.bg-green { background: linear-gradient(135deg, #4ec9a0, #2b9a74); }
.ip-swatch.bg-purple { background: linear-gradient(135deg, #a77ae0, #6d3fb0); }
.ip-swatch.bg-red { background: linear-gradient(135deg, #e5534b, #b0302a); }
.ip-swatch.bg-slate { background: linear-gradient(135deg, #8c96aa, #5a6478); }
.ip-swatch.bg-dark { background: linear-gradient(135deg, #1e2230, #14161d); }
.ip-swatch.bg-none { background: rgba(255, 255, 255, 0.08); }
.ip-loading,
.ip-empty {
  font-size: 13px;
  color: #8b8e9c;
  padding: 16px 0;
  text-align: center;
}
.ip-icons {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(48px, 1fr));
  gap: 6px;
  max-height: 220px;
  overflow-y: auto;
}
.ip-icon {
  aspect-ratio: 1;
  border: 1px solid rgba(255, 255, 255, 0.1);
  background: rgba(255, 255, 255, 0.04);
  border-radius: 10px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 4px;
  overflow: hidden;
}
.ip-icon:hover {
  background: rgba(255, 255, 255, 0.09);
}
.ip-icon.active {
  border-color: #e89a4b;
  background: rgba(232, 154, 75, 0.16);
}
.ip-icon-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  image-rendering: pixelated;
}
.ip-import {
  align-self: flex-start;
  border: 1px solid rgba(255, 255, 255, 0.1);
  background: rgba(255, 255, 255, 0.05);
  color: #f2f3f7;
  border-radius: 9px;
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.ip-import:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
}
.ip-import:disabled {
  opacity: 0.5;
}
.ip-img-ok {
  font-size: 12px;
  color: #4ec9a0;
}
.ip-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.ip-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  background: rgba(255, 255, 255, 0.05);
  color: #f2f3f7;
  border-radius: 9px;
  padding: 8px 18px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.ip-btn.primary {
  background: linear-gradient(135deg, #e89a4b, #d97f33);
  color: #1a1208;
  border: none;
}
</style>
