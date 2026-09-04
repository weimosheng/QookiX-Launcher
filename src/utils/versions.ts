/**
 * Minecraft 版本列表相关的工具函数。
 */

/**
 * 判断一个版本是否是愚人节版本（快照分类里要单独归档、不混入普通快照）。
 * 愚人节版本的 id 通常含 april/fools，正式的按发布日期 04-01 判断。
 */
export function isAprilFools(v: { id: string; releaseTime: string }): boolean {
  if (/april|fools/i.test(v.id)) return true;
  const d = v.releaseTime;
  return d.length >= 10 && d.slice(5, 7) === "04" && d.slice(8, 10) === "01";
}
