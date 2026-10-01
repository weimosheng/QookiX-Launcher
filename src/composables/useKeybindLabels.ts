/**
 * 动作 id → 分组与中文名。id 里的点（hotbar.1）不是合法 i18n 路径，
 * 换成下划线再查词条（keybind.action.hotbar_1）；模组动作没登记，回落原始 id。
 */
import { useI18n } from "vue-i18n";

/** 分组展示顺序；未登记的动作归入 other */
export const KEYBIND_GROUPS = ["movement", "combat", "inventory", "multiplayer", "misc", "other"] as const;

/** 各组内的动作与顺序（对齐游戏内控制设置） */
const ORDERED: Record<string, string[]> = {
  movement: ["forward", "left", "back", "right", "jump", "sneak", "sprint"],
  combat: ["attack", "use", "pickItem", "swapOffhand", "drop"],
  inventory: [
    "inventory",
    ...Array.from({ length: 9 }, (_, i) => `hotbar.${i + 1}`),
    "saveToolbarActivator",
    "loadToolbarActivator",
  ],
  multiplayer: ["chat", "command", "playerlist", "socialInteractions"],
  misc: ["advancements", "screenshot", "togglePerspective", "smoothCamera", "fullscreen", "spectatorOutlines"],
};

const GROUP_OF: Record<string, string> = {};
for (const [group, actions] of Object.entries(ORDERED)) {
  for (const action of actions) GROUP_OF[action] = group;
}

/** 动作属于哪个分组 */
export function keybindGroupOf(action: string): string {
  return GROUP_OF[action] ?? "other";
}

/** 原版（Minecraft 自带）动作全集，用于区分模组按键 */
export const VANILLA_ACTIONS: Set<string> = new Set(Object.values(ORDERED).flat());

/** 动作归属：vanilla / mod:<modid> / other（模组按键形如 `key_key.<modid>.<key>`，取首段匹配） */
export function actionOwner(action: string, modIds: Set<string>): string {
  if (VANILLA_ACTIONS.has(action)) return "vanilla";
  const head = action.split(".")[0].toLowerCase();
  if (head && modIds.has(head)) return `mod:${head}`;
  return "other";
}

/** 组内排序：登记过的按定义顺序，未登记的按字母序跟在后面 */
export function keybindSort(a: string, b: string): number {
  const ia = (ORDERED[keybindGroupOf(a)] ?? []).indexOf(a);
  const ib = (ORDERED[keybindGroupOf(b)] ?? []).indexOf(b);
  if (ia !== -1 && ib !== -1) return ia - ib;
  if (ia !== -1) return -1;
  if (ib !== -1) return 1;
  return a.localeCompare(b);
}

export function useKeybindLabels() {
  const { t, te } = useI18n();
  /** 登记过的动作才有中文名 */
  const isKnownAction = (action: string): boolean => te(`keybind.action.${action.replace(/\./g, "_")}`);
  const actionLabel = (action: string): string => {
    const key = `keybind.action.${action.replace(/\./g, "_")}`;
    return te(key) ? t(key) : action.replace(/^key\./, "");
  };
  const groupLabel = (group: string): string => t(`keybind.group.${group}`);
  return { actionLabel, groupLabel, isKnownAction };
}
