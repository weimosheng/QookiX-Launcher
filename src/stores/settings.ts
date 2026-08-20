import { defineStore } from "pinia";
import { api } from "../api";
import type { JavaInfo, Settings } from "../types";

const JAVA_CACHE_TTL = 60_000;

export const useSettingsStore = defineStore("settings", {
  state: () => ({
    settings: null as Settings | null,
    loading: false,
    /** Shared Java detection results (avoid repeated expensive scans) */
    javaCandidates: [] as JavaInfo[],
    javaFetchedAt: 0,
  }),
  getters: {
    maxMemory: (s) => s.settings?.max_memory_mb ?? 4096,
    bestJava: (s): JavaInfo | null => {
      if (!s.javaCandidates.length) return null;
      return s.javaCandidates.reduce((a, b) => (b.major > a.major ? b : a));
    },
  },
  actions: {
    async load() {
      this.loading = true;
      try {
        this.settings = await api.getSettings();
      } finally {
        this.loading = false;
      }
    },
    async patch(patch: Record<string, unknown>) {
      this.settings = await api.setSettings(patch);
    },
    async save() {
      if (this.settings) {
        this.settings = await api.setSettings(this.settings as unknown as Record<string, unknown>);
      }
    },
    /** Fetch detected Java, reusing the cache unless `force` (查找 Java). */
    async loadJava(force = false): Promise<JavaInfo[]> {
      if (
        !force &&
        this.javaCandidates.length &&
        Date.now() - this.javaFetchedAt < JAVA_CACHE_TTL
      ) {
        return this.javaCandidates;
      }
      try {
        const res = await api.detectJava(force);
        this.javaCandidates = res.candidates;
        this.javaFetchedAt = Date.now();
      } catch {
        // keep whatever we had
      }
      return this.javaCandidates;
    },
  },
});
