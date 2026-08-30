import { defineStore } from "pinia";
import { ref } from "vue";
import { api } from "../api";
import type { NewsItem } from "../types";

const CACHE_TTL = 10 * 60 * 1000;

export const useNewsStore = defineStore("news", () => {
  const news = ref<NewsItem[]>([]);
  const loadedAt = ref(0);
  const loading = ref(false);

  function isFresh() {
    return news.value.length > 0 && Date.now() - loadedAt.value < CACHE_TTL;
  }

  async function load(force = false) {
    if (!force && isFresh()) return;
    const hasCache = news.value.length > 0;
    if (!hasCache || force) loading.value = true;
    try {
      news.value = await api.fetchNews();
      loadedAt.value = Date.now();
    } finally {
      loading.value = false;
    }
  }

  return { news, loading, isFresh, load };
});
