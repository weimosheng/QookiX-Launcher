/**
 * 本地托管服务器核心的展示常量。
 *
 * 此前 CORE_LABELS / CORE_COLORS 在 MultiplayerView 与 ServerDetailView
 * 各复制了一份，颜色一旦调整就会出现两处显示不一致，统一收敛到这里。
 */
import type { ServerCore } from "../types";

/** 核心 → 显示名 */
export const CORE_LABELS: Record<ServerCore, string> = {
  vanilla: "Vanilla",
  paper: "Paper",
  spigot: "Spigot",
  purpur: "Purpur",
  forge: "Forge",
  fabric: "Fabric",
};

/** 核心 → 徽标颜色 */
export const CORE_COLORS: Record<ServerCore, string> = {
  vanilla: "#a0a4b8",
  paper: "#5aa2f0",
  spigot: "#4ecdc4",
  purpur: "#c78aff",
  forge: "#e89a4b",
  fabric: "#b48ead",
};
