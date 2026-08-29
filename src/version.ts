/**
 * Minecraft 在 1.20.2 才加入 quick-play（`--quickPlaySingleplayer`）命令行参数，
 * 用于直接进入指定单人存档。此判断需与后端 `launch.rs` 的 `supports_quick_play` 保持一致。
 */
export function supportsQuickPlay(mcVersion: string | undefined): boolean {
  if (!mcVersion) return false;
  const nums: number[] = mcVersion
    .split(/[^0-9]+/)
    .filter((p) => p.length > 0)
    .map((p) => Number(p));
  if (nums.length === 0) return false;
  const [major, minor, patch] = nums;
  if (major === 1) {
    if (minor === undefined) return false;
    if (minor > 20) return true;
    if (minor === 20) return (patch ?? 0) >= 2;
    return false;
  }
  return major >= 2;
}
