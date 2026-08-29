import { defineStore } from "pinia";

export interface PinItem {
  /** 唯一标识：`instanceId:type:key` */
  id: string;
  type: "server" | "world";
  instanceId: string;
  instanceName: string;
  instanceIcon: string | null;
  mcVersion: string;
  loader: string;
  /** 展示名：服务器名或世界名 */
  name: string;
  /** 服务器地址（type=server 时） */
  address?: string;
  /** 世界文件夹名（type=world 时） */
  world?: string;
  /** 服务器图标 base64（无 data: 前缀）或世界图标路径 */
  icon?: string | null;
}

const STORAGE_KEY = "qookix.pins";

function load(): PinItem[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw);
    return Array.isArray(arr) ? (arr as PinItem[]) : [];
  } catch {
    return [];
  }
}

export const usePinsStore = defineStore("pins", {
  state: () => ({
    items: load() as PinItem[],
  }),
  getters: {
    isPinned: (s) => (id: string) => s.items.some((i) => i.id === id),
    byId: (s) => (id: string) => s.items.find((i) => i.id === id),
  },
  actions: {
    save() {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.items));
    },
    makeId(type: "server" | "world", instanceId: string, key: string) {
      return `${instanceId}:${type}:${key}`;
    },
    add(pin: PinItem) {
      if (this.items.some((i) => i.id === pin.id)) return;
      this.items.push(pin);
      this.save();
    },
    remove(id: string) {
      const next = this.items.filter((i) => i.id !== id);
      if (next.length !== this.items.length) {
        this.items = next;
        this.save();
      }
    },
    toggle(pin: PinItem) {
      if (this.items.some((i) => i.id === pin.id)) this.remove(pin.id);
      else this.add(pin);
    },
  },
});
