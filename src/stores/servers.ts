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
    /** 最近一次成功拉取的时间戳，用于短 TTL 内跳过重复请求 */
    lastLoadedAt: 0,
  }),
  getters: {
    count: (state) => state.servers.length,
    byId: (state) => (id: string) => state.servers.find((s) => s.id === id) ?? null,
    isRunning: (state) => (id: string) => !!state.runningIds[id],
  },
  actions: {
    /** 后台静默刷新服务器列表 + 运行状态，失败保留旧数据 */
    async refresh() {
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
        this.lastLoadedAt = Date.now();
      } catch {
        /* ignore */
      }
    },
    /**
     * 拉取服务器列表与运行状态。
     * - 已有数据时采用 stale-while-revalidate：立即返回旧数据，后台静默刷新替换。
     * - `force=true` 时始终前台拉取（用于创建/删除/启停后强制刷新）。
     */
    async load(force = false) {
      const now = Date.now();
      if (!force && this.lastLoadedAt && now - this.lastLoadedAt < 3000) return;
      const hasData = this.servers.length > 0;
      if (hasData && !force) {
        void this.refresh();
        return;
      }
      await this.refresh();
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
