<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
  page: number;
  pageCount: number;
}>();

const emit = defineEmits<{
  "update:page": [page: number];
}>();

const displayPages = computed(() => {
  const current = props.page;
  const total = props.pageCount;
  const pages: (number | "ellipsis")[] = [];

  if (total <= 7) {
    for (let i = 1; i <= total; i++) pages.push(i);
    return pages;
  }

  pages.push(1);

  if (current <= 4) {
    for (let i = 2; i <= 5; i++) pages.push(i);
    pages.push("ellipsis");
    pages.push(total);
  } else if (current >= total - 3) {
    pages.push("ellipsis");
    for (let i = total - 4; i <= total; i++) pages.push(i);
  } else {
    pages.push("ellipsis");
    for (let i = current - 1; i <= current + 1; i++) pages.push(i);
    pages.push("ellipsis");
    pages.push(total);
  }

  return pages;
});

function go(p: number) {
  if (p < 1 || p > props.pageCount || p === props.page) return;
  emit("update:page", p);
}
</script>

<template>
  <div class="simple-pager">
    <button
      class="sp-btn sp-nav"
      :disabled="page <= 1"
      title="上一页"
      @click="go(page - 1)"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="15 18 9 12 15 6" />
      </svg>
    </button>
    <template v-for="(p, i) in displayPages" :key="i">
      <span v-if="p === 'ellipsis'" class="sp-ellipsis">…</span>
      <button
        v-else
        class="sp-btn"
        :class="{ active: p === page }"
        @click="go(p)"
      >
        {{ p }}
      </button>
    </template>
    <button
      class="sp-btn sp-nav"
      :disabled="page >= pageCount"
      title="下一页"
      @click="go(page + 1)"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="9 18 15 12 9 6" />
      </svg>
    </button>
  </div>
</template>

<style scoped>
.simple-pager {
  display: flex;
  align-items: center;
  gap: 4px;
  user-select: none;
}
.sp-btn {
  min-width: 30px;
  height: 30px;
  padding: 0 6px;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  font-size: 13px;
  font-weight: 600;
  font-family: inherit;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: all 0.12s;
}
.sp-btn:hover:not(:disabled):not(.active) {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-1);
}
.sp-btn.active {
  background: var(--accent-soft);
  border-color: var(--accent);
  color: var(--accent);
}
.sp-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.sp-nav {
  padding: 0;
}
.sp-ellipsis {
  min-width: 24px;
  text-align: center;
  color: var(--text-3);
  font-size: 13px;
}
</style>
