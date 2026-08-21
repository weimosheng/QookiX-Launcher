import { defineStore } from "pinia";
import { api } from "../api";
import type { Instance } from "../types";

export const useInstancesStore = defineStore("instances", {
  state: () => ({
    instances: [] as Instance[],
    loading: false,
    installingId: null as string | null,
    launchingId: null as string | null,
    installStage: "",
    installDone: 0,
    installTotal: 0,
  }),
  actions: {
    async load() {
      this.loading = true;
      try {
        this.instances = await api.listInstances();
      } finally {
        this.loading = false;
      }
    },
    get(id: string) {
      return this.instances.find((i) => i.id === id);
    },
    async create(name: string, mc: string, loader: string, loaderVersion: string | null) {
      const inst = await api.createInstance(name, mc, loader, loaderVersion);
      await this.load();
      return inst;
    },
    async patch(patch: Record<string, unknown>) {
      const inst = await api.updateInstance(patch);
      const idx = this.instances.findIndex((i) => i.id === inst.id);
      if (idx >= 0) this.instances[idx] = inst;
      return inst;
    },
    async remove(id: string) {
      await api.deleteInstance(id);
      await this.load();
    },
    async installGame(id: string) {
      this.installingId = id;
      this.installStage = "准备中…";
      this.installDone = 0;
      this.installTotal = 0;
      try {
        await api.installGame(id);
        await this.load();
      } finally {
        this.installingId = null;
      }
    },
    cancelInstall() {
      api.cancelInstall();
      this.installingId = null;
    },
    async launch(id: string, world?: string) {
      this.launchingId = id;
      try {
        const res = await api.launchInstance(id, world);
        return res;
      } finally {
        this.launchingId = null;
      }
    },
    async stop() {
      await api.stopGame();
    },
  },
});
