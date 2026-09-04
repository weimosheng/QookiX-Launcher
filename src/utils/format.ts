/**
 * 全局格式化工具。
 *
 * 此前 fmtSize / fmtBytes / fmtDate / fmtTime 等函数在 14 个视图与组件中
 * 各自复制了一份（部分口径还不一致）。现在统一收敛到这里，调用方通过
 * `import { fmtSize } from "../utils/format"` 使用；本地函数名与导出名
 * 不一致的地方用 import 别名保持模板零改动，例如：
 *   `import { fmtBytes, fmtTimeMs as fmtTime } from "../utils/format";`
 */

const pad2 = (n: number) => String(n).padStart(2, "0");

// ---------------------------------------------------------------- 尺寸 ----

/**
 * 字节数 → 人类可读尺寸。口径：KB/MB 1 位小数、GB 2 位小数。
 * 这是全库多数调用点的既有口径，勿随意调整精度。
 */
export function fmtSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return (bytes / 1024 ** 3).toFixed(2) + " GB";
  if (bytes >= 1024 ** 2) return (bytes / 1024 ** 2).toFixed(1) + " MB";
  if (bytes >= 1024) return (bytes / 1024).toFixed(1) + " KB";
  return `${Math.round(bytes)} B`;
}

/** fmtSize 的别名：历史调用点普遍叫 fmtBytes */
export const fmtBytes = fmtSize;

/** 下载速度（字节/秒）→ "2.3 MB/s" / "850 KB/s" / "12 B/s" */
export function fmtSpeed(bps: number): string {
  if (bps >= 1024 ** 2) return (bps / 1024 ** 2).toFixed(1) + " MB/s";
  if (bps >= 1024) return (bps / 1024).toFixed(0) + " KB/s";
  return `${bps.toFixed(0)} B/s`;
}

/** 内存 MB → "2 GB" / "1.5 GB" / "512 MB"（整 GB 不带小数） */
export function fmtMem(mb: number): string {
  if (!mb || mb <= 0) return "0 MB";
  if (mb >= 1024) return (mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1) + " GB";
  return Math.round(mb) + " MB";
}

// ---------------------------------------------------------------- 时间 ----

/** unix 秒 → "YYYY-MM-DD HH:mm"，0/空 → ""（文件管理器口径） */
export function fmtDate(sec: number): string {
  if (!sec) return "";
  const d = new Date(sec * 1000);
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

/** unix 秒 → "YYYY-MM-DD"（新闻列表日期），0/空 → "" */
export function fmtDateShort(sec: number): string {
  if (!sec) return "";
  const d = new Date(sec * 1000);
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

/** unix 秒 → 本地化完整日期时间（崩溃分析、实例详情口径） */
export function fmtDateLocale(sec: number): string {
  if (!sec) return "";
  return new Date(sec * 1000).toLocaleString();
}

/** unix 秒 → "YYYY-MM-DD HH:mm"，0/空 → "—"（设置页存储统计口径） */
export function fmtTime(sec: number): string {
  return sec ? fmtDate(sec) : "—";
}

/** unix 毫秒 → "MM-DD HH:mm:ss"（下载中心口径） */
export function fmtTimeMs(ms: number): string {
  const d = new Date(ms);
  return `${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}

/** ISO/日期字符串 → 本地化短日期（安装弹窗口径），无效输入 → "" */
export function fmtDateStr(s: string): string {
  if (!s) return "";
  const d = new Date(s);
  return isNaN(d.getTime()) ? "" : d.toLocaleDateString();
}

/** ISO/日期字符串 → 相对时间："刚刚 / N 分钟前 / 昨天 / N 个月前…"（内容卡片口径） */
export function fmtRelative(s: string): string {
  if (!s) return "";
  const d = new Date(s);
  if (isNaN(d.getTime())) return "";
  const now = new Date();
  const diffMs = now.getTime() - d.getTime();
  if (diffMs < 0) return "刚刚";
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 1) return "刚刚";
  if (diffMin < 60) return `${diffMin} 分钟前`;
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const target = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const dayDiff = Math.floor((today.getTime() - target.getTime()) / 86400000);
  const diffHour = Math.floor(diffMs / 3600000);
  if (dayDiff === 0) return `${diffHour} 小时前`;
  if (dayDiff === 1) return "昨天";
  if (dayDiff === 2) return "前天";
  if (dayDiff < 7) return `${dayDiff} 天前`;
  const monthDiff = (now.getFullYear() - d.getFullYear()) * 12 + (now.getMonth() - d.getMonth());
  if (monthDiff <= 0) return `${dayDiff} 天前`;
  if (monthDiff === 1) return "上个月";
  if (monthDiff < 12) return `${monthDiff} 个月前`;
  const yearDiff = now.getFullYear() - d.getFullYear();
  if (yearDiff === 1) return "去年";
  return `${yearDiff} 年前`;
}

// ---------------------------------------------------------------- 网络 ----

/** 延迟（毫秒）→ 信号格数与档位（服务器列表/详情共用） */
export function latencyInfo(
  latency: number | null | undefined
): { count: number; tier: string } {
  if (latency == null) return { count: 0, tier: "off" };
  if (latency <= 50) return { count: 5, tier: "good" };
  if (latency <= 100) return { count: 4, tier: "good" };
  if (latency <= 200) return { count: 3, tier: "mid" };
  if (latency <= 300) return { count: 2, tier: "bad" };
  return { count: 1, tier: "bad" };
}

// ---------------------------------------------------------------- 加载器 ----

/** 加载器徽标文字：vanilla → 原版，其余首字母大写（Forge/Fabric/…） */
export function loaderBadge(loader: string): string {
  return loader === "vanilla"
    ? "原版"
    : loader.charAt(0).toUpperCase() + loader.slice(1);
}
