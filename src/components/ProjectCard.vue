<script setup lang="ts">
import { IconBox, IconDownload, IconHeart } from "./icons";
import type { ProjectHit } from "../types";

defineProps<{ project: ProjectHit }>();
const emit = defineEmits<{ install: [p: ProjectHit] }>();

function fmt(n: number) {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "K";
  return String(n);
}

function typeLabel(t: string) {
  return (
    { mod: "模组", modpack: "整合包", resourcepack: "资源包", shader: "光影" }[t] ?? t
  );
}
</script>

<template>
  <div class="p-card glass">
    <div class="p-main">
      <div class="p-icon-wrap">
        <img v-if="project.icon_url" :src="project.icon_url" class="p-icon" alt="" loading="lazy" />
        <div v-else class="p-icon ph"><IconBox /></div>
      </div>
      <div class="p-body">
        <div class="p-title text-ellipsis" :title="project.title">{{ project.title }}</div>
        <div class="p-author">{{ project.author }}</div>
        <div class="p-desc">{{ project.description }}</div>
        <div class="p-cats">
          <span v-for="c in project.categories.slice(0, 3)" :key="c" class="cat">{{ c }}</span>
        </div>
      </div>
    </div>
    <div class="p-foot">
      <div class="p-stats">
        <span class="type-badge">{{ typeLabel(project.project_type) }}</span>
        <span class="dl"><IconDownload /> {{ fmt(project.downloads) }}</span>
        <span v-if="project.follows" class="fl"><IconHeart /> {{ fmt(project.follows) }}</span>
      </div>
      <div class="p-actions">
        <button class="install-btn" @click="emit('install', project)">
          <IconDownload /> 安装
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.p-card {
  display: flex;
  flex-direction: column;
  padding: 14px;
  gap: 12px;
}
.p-main {
  display: flex;
  gap: 13px;
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
.fl {
  display: inline-flex;
  align-items: center;
  gap: 3px;
}
.type-badge {
  background: var(--accent-soft);
  color: var(--accent);
  padding: 1px 7px;
  border-radius: 6px;
  font-weight: 600;
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
  border-color: rgba(232, 154, 75, 0.5);
}
</style>
