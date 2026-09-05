// 统一前端日志入口：
// - log / warn 仅在开发构建输出，生产构建静默（避免污染控制台、泄露路径等信息）
// - error 始终输出，便于线上排障
const dev = import.meta.env.DEV;

export function log(...args: unknown[]) {
  if (dev) console.log(...args);
}

export function warn(...args: unknown[]) {
  if (dev) console.warn(...args);
}

export function error(...args: unknown[]) {
  console.error(...args);
}
