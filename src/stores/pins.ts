import { defineStore } from "pinia";
import { api } from "../api";
import { error as devError } from "../utils/logger";

export type PinType = "server" | "world" | "instance";

/** 固定位置：首页快捷卡片 / 侧边栏图标。两者互相独立，可分别固定与取消 */
export type PinTarget = "home" | "sidebar";

export interface PinItem {
  /** 唯一标识：`instanceId:type:key:target` */
  id: string;
  type: PinType;
  /** 固定到哪个位置（首页 or 侧边栏） */
  target: PinTarget;
  instanceId: string;
  instanceName: string;
  instanceIcon: string | null;
  mcVersion: string;
  loader: string;
  /** 展示名：服务器名、世界名或实例名 */
  name: string;
  /** 服务器地址（type=server 时） */
  address?: string;
  /** 世界文件夹名（type=world 时） */
  world?: string;
  /** 服务器图标 base64（无 data: 前缀）或世界图标路径 */
  icon?: string | null;
}

const LEGACY_KEY = "qookix.pins";

/**
 * 读取旧版本 localStorage 中的固定项，并补齐 target 字段做迁移：
 * 早期版本没有 target，实例类会同时出现在首页与侧边栏。
 */
function migrateLegacy(): PinItem[] {
  try {
    const raw = localStorage.getItem(LEGACY_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw);
    if (!Array.isArray(arr)) return [];
    const out: PinItem[] = [];
    for (const it of arr as Array<Partial<PinItem>>) {
      if (!it?.id || !it.type) continue;
      if (it.target) {
        out.push(it as PinItem);
        continue;
      }
      const base = it as PinItem;
      out.push({ ...base, id: `${base.id}:home`, target: "home" });
      if (base.type === "instance") {
        out.push({ ...base, id: `${base.id}:sidebar`, target: "sidebar" });
      }
    }
    return out;
  } catch {
    return [];
  }
}

export const usePinsStore = defineStore("pins", {
  state: () => ({
    items: [] as PinItem[],
    loaded: false,
  }),
  getters: {
    isPinned: (s) => (id: string) => s.items.some((i) => i.id === id),
    byId: (s) => (id: string) => s.items.find((i) => i.id === id),
    /** 某个位置下的全部固定项 */
    ofTarget: (s) => (target: PinTarget) => s.items.filter((i) => i.target === target),
  },
  actions: {
    /** 从后端文件（pins.json）加载固定项；若为首次且本地有旧数据则迁移过去 */
    async init() {
      try {
        let items = await api.getPins();
        if (items.length === 0) {
          const legacy = migrateLegacy();
          if (legacy.length) {
            this.items = legacy;
            await this.save();
            localStorage.removeItem(LEGACY_KEY);
            return;
          }
        }
        this.items = items;
      } catch {
        /* 加载失败保留空列表 */
      } finally {
        this.loaded = true;
      }
    },
    /** 持久化：写回数据目录下的 pins.json（经后端命令） */
    async save() {
      try {
        await api.setPins(this.items);
      } catch (e) {
        devError("保存固定项失败", e);
      }
    },
    /** target 参与 id 组成，首页与侧边栏的固定项互不覆盖 */
    makeId(type: PinType, instanceId: string, key: string, target: PinTarget = "home") {
      return `${instanceId}:${type}:${key}:${target}`;
    },
    add(pin: PinItem) {
      if (this.items.some((i) => i.id === pin.id)) return;
      this.items.push(pin);
      void this.save();
    },
    remove(id: string) {
      const next = this.items.filter((i) => i.id !== id);
      if (next.length !== this.items.length) {
        this.items = next;
        void this.save();
      }
    },
    toggle(pin: PinItem) {
      if (this.items.some((i) => i.id === pin.id)) this.remove(pin.id);
      else this.add(pin);
    },
  },
});
