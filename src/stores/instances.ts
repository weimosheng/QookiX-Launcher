import { defineStore } from "pinia";
import { api } from "../api";
import type { Instance, InstanceGroup } from "../types";

export const useInstancesStore = defineStore("instances", {
  state: () => ({
    instances: [] as Instance[],
    groups: [] as InstanceGroup[],
    loading: false,
    installingId: null as string | null,
    launchingId: null as string | null,
    installStage: "",
    installDone: 0,
    installTotal: 0,
    /** 最近一次成功拉取的时间戳，用于短 TTL 内跳过重复请求 */
    lastLoadedAt: 0,
  }),
  getters: {
    /** 分组名 -> 分组对象，便于 UI 快速取色 */
    groupMap(state): Record<string, InstanceGroup> {
      const map: Record<string, InstanceGroup> = {};
      for (const g of state.groups) map[g.id] = g;
      return map;
    },
    /** 未分组实例 */
    ungrouped(state): Instance[] {
      return state.instances.filter((i) => !i.group);
    },
    /** 取某个分组下的实例（保持 instances 的默认排序） */
    inGroup: (state) => (groupId: string) =>
      state.instances.filter((i) => i.group === groupId),
  },
  actions: {
    /** 后台静默刷新（不改动 loading），失败保留旧数据 */
    async refresh() {
      try {
        const [list, groups] = await Promise.all([api.listInstances(), api.listGroups()]);
        this.instances = list;
        this.groups = groups;
        this.lastLoadedAt = Date.now();
      } catch {
        /* 后台刷新失败保留旧数据 */
      }
    },
    /**
     * 拉取实例列表。
     * - 已有数据时采用 stale-while-revalidate：立即返回旧数据，后台静默刷新替换。
     * - `force=true` 时始终前台拉取并设 loading（用于写操作后强制刷新）。
     */
    async load(force = false) {
      const now = Date.now();
      // 3 秒内刚拉过，直接跳过，避免切换页面时重复请求
      if (!force && this.lastLoadedAt && now - this.lastLoadedAt < 3000) return;
      const hasData = this.instances.length > 0 || this.groups.length > 0;
      if (hasData && !force) {
        void this.refresh();
        return;
      }
      this.loading = true;
      try {
        const [list, groups] = await Promise.all([api.listInstances(), api.listGroups()]);
        this.instances = list;
        this.groups = groups;
        this.lastLoadedAt = Date.now();
      } finally {
        this.loading = false;
      }
    },
    get(id: string) {
      return this.instances.find((i) => i.id === id);
    },
    groupById(id: string | null | undefined): InstanceGroup | null {
      if (!id) return null;
      return this.groups.find((g) => g.id === id) ?? null;
    },
    // ---- 分组管理 ----
    async loadGroups() {
      this.groups = await api.listGroups();
    },
    async createGroup(name: string, color?: string | null) {
      const g = await api.createGroup(name, color ?? null);
      await this.loadGroups();
      return g;
    },
    async renameGroup(id: string, name: string, color?: string | null) {
      const g = await api.renameGroup(id, name, color ?? null);
      await this.loadGroups();
      return g;
    },
    async deleteGroup(id: string) {
      await api.deleteGroup(id);
      // 组内实例被后端移回未分组，重新拉取保持一致
      await this.load(true);
    },
    async reorderGroups(ids: string[]) {
      this.groups = await api.reorderGroups(ids);
    },
    /** 移动实例到分组（groupId 为空表示移出分组） */
    async moveToGroup(instanceId: string, groupId: string | null) {
      const inst = await api.updateInstance({ id: instanceId, group: groupId ?? "" });
      const idx = this.instances.findIndex((i) => i.id === inst.id);
      if (idx >= 0) this.instances[idx] = inst;
      return inst;
    },
    async create(name: string, mc: string, loader: string, loaderVersion: string | null) {
      const inst = await api.createInstance(name, mc, loader, loaderVersion);
      await this.load(true);
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
      await this.load(true);
    },
    async installGame(id: string) {
      this.installingId = id;
      this.installStage = "准备中…";
      this.installDone = 0;
      this.installTotal = 0;
      try {
        await api.installGame(id);
        await this.load(true);
      } finally {
        this.installingId = null;
      }
    },
    cancelInstall() {
      api.cancelInstall();
      this.installingId = null;
    },
    async launch(id: string, world?: string, server?: string) {
      this.launchingId = id;
      try {
        const res = await api.launchInstance(id, world, server);
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
