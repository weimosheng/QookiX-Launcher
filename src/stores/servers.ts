import { defineStore } from "pinia";
import { api } from "../api";
import type { ServerConfig } from "../types";

export const useServersStore = defineStore("servers", {
  state: () => ({
    servers: [] as ServerConfig[],
    loaded: false,
    runningIds: {} as Record<string, boolean>,
    // 由标题栏的“创建服务器”按钮递增，多人游戏页监听后打开创建对话框
    createRequest: 0,
    // 仅在“服务器”标签页时标题栏才显示创建按钮
    canCreate: true,
  }),
  getters: {
    count: (state) => state.servers.length,
    byId: (state) => (id: string) => state.servers.find((s) => s.id === id) ?? null,
    isRunning: (state) => (id: string) => !!state.runningIds[id],
  },
  actions: {
    async load() {
      try {
        this.servers = await api.listHostedServers();
        this.loaded = true;
        const next: Record<string, boolean> = {};
        await Promise.all(
          this.servers.map(async (s) => {
            try {
              next[s.id] = await api.isHostedServerRunning(s.id);
            } catch {
              next[s.id] = false;
            }
          }),
        );
        this.runningIds = next;
      } catch {
        /* ignore */
      }
    },
    async create(name: string, core: string, mcVersion: string): Promise<ServerConfig> {
      const s = await api.createHostedServer(name, core, mcVersion);
      this.servers = [...this.servers, s];
      return s;
    },
    async update(patch: Record<string, unknown>): Promise<ServerConfig> {
      const s = await api.updateHostedServer(patch);
      const idx = this.servers.findIndex((x) => x.id === s.id);
      if (idx >= 0) this.servers[idx] = s;
      return s;
    },
    async remove(id: string) {
      await api.deleteHostedServer(id);
      this.servers = this.servers.filter((s) => s.id !== id);
    },
    async installCore(id: string) {
      await api.installHostedServerCore(id);
    },
    async start(id: string): Promise<number> {
      const { pid } = await api.startHostedServer(id);
      this.runningIds = { ...this.runningIds, [id]: true };
      const idx = this.servers.findIndex((s) => s.id === id);
      if (idx >= 0) this.servers[idx] = { ...this.servers[idx], last_started: Date.now() };
      return pid;
    },
    async stop(id: string) {
      await api.stopHostedServer(id);
      this.runningIds = { ...this.runningIds, [id]: false };
    },
    setRunning(id: string, running: boolean) {
      this.runningIds = { ...this.runningIds, [id]: running };
    },
    requestCreate() {
      this.createRequest++;
    },
    setCanCreate(v: boolean) {
      this.canCreate = v;
    },
  },
});
