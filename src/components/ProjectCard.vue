<script setup lang="ts">
import { ref } from "vue";
import { IconBox, IconCheck, IconClock, IconCopy, IconDownload, IconHeart } from "./icons";
import { translateCategory } from "../utils/categories";
import { fmtRelative as fmtDate } from "../utils/format";
import type { ProjectHit } from "../types";

const props = withDefaults(defineProps<{ project: ProjectHit; view?: "grid" | "list" | "compact" }>(), {
  view: "grid",
});
const emit = defineEmits<{ install: [p: ProjectHit] }>();
const iconError = ref(false);

const copied = ref(false);
async function copyName() {
  try {
    await navigator.clipboard.writeText(props.project.title);
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
  } catch {
    /* 剪贴板不可用时忽略 */
  }
}

function fmt(n: number) {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "K";
  return String(n);
}

</script>

<template>
  <div class="p-card glass" :class="`view-${view}`" @click="emit('install', project)">
    <template v-if="view === 'grid'">
      <div class="p-main">
        <div class="p-icon-wrap">
          <img v-if="project.icon_url && !iconError" :src="project.icon_url" class="p-icon" alt="" loading="lazy" @error="iconError = true" />
          <div v-else class="p-icon ph"><IconBox /></div>
        </div>
        <div class="p-body">
          <div class="p-title text-ellipsis" :title="project.title">{{ project.title }}</div>
          <div class="p-author">
            {{ project.author }}
            <span v-if="fmtDate(project.updated)" class="p-updated"><IconClock /> {{ fmtDate(project.updated) }}</span>
          </div>
          <div class="p-desc">{{ project.description }}</div>
          <div class="p-cats">
            <span v-for="c in project.categories.slice(0, 3)" :key="c" class="cat">{{ translateCategory(c) }}</span>
          </div>
        </div>
      </div>
      <div class="p-foot">
        <div class="p-stats">
          <span class="provider-badge" :class="project.provider">{{ project.provider === 'modrinth' ? 'Modrinth' : 'CurseForge' }}</span>
          <span class="dl"><IconDownload /> {{ fmt(project.downloads) }}</span>
          <span v-if="project.follows" class="fl"><IconHeart /> {{ fmt(project.follows) }}</span>
        </div>
        <div class="p-actions">
          <button
            class="copy-btn"
            :title="copied ? '已复制' : '复制名称'"
            @click.stop="copyName"
          >
            <IconCheck v-if="copied" />
            <IconCopy v-else />
          </button>
          <button class="install-btn" @click.stop="emit('install', project)">
            <IconDownload /> 安装
          </button>
        </div>
      </div>
    </template>

    <template v-else>
      <div class="p-icon-wrap">
        <img v-if="project.icon_url && !iconError" :src="project.icon_url" class="p-icon" alt="" loading="lazy" @error="iconError = true" />
        <div v-else class="p-icon ph"><IconBox /></div>
      </div>
      <div class="p-body">
        <div class="p-title text-ellipsis" :title="project.title">{{ project.title }}</div>
        <div class="p-author">{{ project.author }}</div>
        <div v-if="view === 'list'" class="p-desc">{{ project.description }}</div>
        <div class="p-cats">
          <span v-for="c in project.categories.slice(0, 3)" :key="c" class="cat">{{ translateCategory(c) }}</span>
        </div>
      </div>
      <div class="p-side">
        <div class="p-stats">
          <span class="provider-badge" :class="project.provider">{{ project.provider === 'modrinth' ? 'Modrinth' : 'CurseForge' }}</span>
          <span class="dl"><IconDownload /> {{ fmt(project.downloads) }}</span>
          <span v-if="project.follows" class="fl"><IconHeart /> {{ fmt(project.follows) }}</span>
          <span v-if="fmtDate(project.updated)" class="up"><IconClock /> {{ fmtDate(project.updated) }}</span>
        </div>
        <div class="p-side-actions">
          <button
            class="copy-btn"
            :title="copied ? '已复制' : '复制名称'"
            @click.stop="copyName"
          >
            <IconCheck v-if="copied" />
            <IconCopy v-else />
          </button>
          <button class="install-btn" @click.stop="emit('install', project)">
            <IconDownload /> 安装
          </button>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.p-card {
  display: flex;
  flex-direction: column;
  padding: 14px;
  gap: 12px;
  cursor: pointer;
  transition: transform 0.1s ease;
  position: relative;
  overflow: hidden;
}
.p-card:active {
  transform: scale(0.97);
}
.copy-btn {
  width: 30px;
  height: 30px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-2);
  border-radius: 8px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  font-size: 13px;
  transition: all 0.12s;
}
.copy-btn:hover {
  color: var(--accent);
  border-color: var(--accent-05);
}
.p-side-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.p-card.view-list,
.p-card.view-compact {
  flex-direction: row;
  align-items: center;
  gap: 14px;
}
.p-card.view-compact {
  padding: 10px 14px;
  gap: 12px;
}
.p-card.view-list .p-icon,
.p-card.view-compact .p-icon {
  width: 46px;
  height: 46px;
}
.p-card.view-compact .p-icon {
  width: 38px;
  height: 38px;
}
.p-card.view-compact .p-desc {
  display: none;
}
.p-card.view-compact .p-cats {
  display: none;
}
.p-card.view-compact .p-title {
  font-size: 13px;
  margin-bottom: 2px;
}
.p-card.view-compact .p-author {
  margin-bottom: 4px;
}
.p-side {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 8px;
  flex-shrink: 0;
}
.p-card.view-compact .p-side {
  flex-direction: row;
  align-items: center;
  gap: 8px;
}
.p-main {
  display: flex;
  gap: 13px;
  flex: 1;
}
.p-icon-wrap {
  flex-shrink: 0;
}
.p-icon {
  width: 52px;
  height: 52px;
  border-radius: 11px;
  object-fit: cover;
  background: rgba(255, 255, 255, 0.05);
}
.p-icon.ph {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 22px;
  color: var(--text-3);
}
.p-body {
  min-width: 0;
  flex: 1;
}
.p-title {
  font-weight: 700;
  font-size: 14px;
  margin-bottom: 2px;
}
.p-author {
  font-size: 11px;
  color: var(--text-3);
  margin-bottom: 6px;
  display: flex;
  align-items: center;
  gap: 8px;
}
.p-updated {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 11px;
}
.p-desc {
  font-size: 12px;
  color: var(--text-2);
  line-height: 1.5;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  margin-bottom: 7px;
}
.p-cats {
  display: flex;
  gap: 5px;
  flex-wrap: wrap;
}
.cat {
  font-size: 10px;
  background: rgba(255, 255, 255, 0.07);
  color: var(--text-3);
  padding: 1px 7px;
  border-radius: 6px;
}
.p-foot {
  display: flex;
  justify-content: space-between;
  align-items: center;
  border-top: 1px solid var(--border);
  padding-top: 10px;
}
.p-stats {
  display: flex;
  align-items: center;
  gap: 9px;
  font-size: 11px;
  color: var(--text-3);
}
.p-stats svg {
  font-size: 11px;
  vertical-align: -1px;
}
.dl,
.fl,
.up {
  display: inline-flex;
  align-items: center;
  gap: 3px;
}
.provider-badge {
  font-size: 10px;
  padding: 1px 7px;
  border-radius: 6px;
  font-weight: 600;
}
.provider-badge.modrinth {
  background: rgba(0, 175, 92, 0.15);
  color: #2bbd6e;
}
.provider-badge.curseforge {
  background: rgba(241, 100, 54, 0.15);
  color: #f17a36;
}
.install-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: none;
  background: linear-gradient(135deg, var(--accent), var(--accent-deep));
  color: #1a1208;
  border-radius: 8px;
  padding: 6px 13px;
  font-size: 12px;
  font-weight: 700;
  cursor: pointer;
  font-family: inherit;
  transition: filter 0.12s;
}
.install-btn:hover {
  filter: brightness(1.1);
}
.p-actions {
  display: flex;
  gap: 6px;
  align-items: center;
}
.site-btn {
  width: 30px;
  height: 30px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-2);
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  font-size: 13px;
  transition: all 0.12s;
}
.site-btn:hover {
  color: var(--accent);
  border-color: var(--accent-05);
}
</style>
