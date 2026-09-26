// 简单的带过期时间的内存缓存（应用会话内有效，避免重复请求）
// 缓存数据存内存，随应用关闭失效；每条数据独立过期，避免缓存太久导致内容过期。

interface CacheEntry<T> {
  data: T;
  expire: number; // 过期时间戳（毫秒）
}

const store = new Map<string, CacheEntry<unknown>>();

/** 读取缓存；过期或不存在时返回 undefined（并清理过期项） */
export function cacheGet<T>(key: string): T | undefined {
  const entry = store.get(key);
  if (!entry) return undefined;
  if (Date.now() > entry.expire) {
    store.delete(key);
    return undefined;
  }
  return entry.data as T;
}

/** 写入缓存，ttlMs 为存活毫秒数 */
export function cacheSet<T>(key: string, data: T, ttlMs: number) {
  store.set(key, { data, expire: Date.now() + ttlMs });
}

/** 清空全部缓存 */
export function cacheClear() {
  store.clear();
}

// —— 持久化层（localStorage，跨会话）——
// Tauri webview2 的 localStorage 默认持久化到磁盘，应用重启后仍在。
// 用于发现页等"有缓存就不重新调 API"的场景。

const PERSIST_PREFIX = "qookix:cache:";

/** 读取持久化缓存（localStorage，跨会话有效）；过期或不存在返回 undefined */
export function cacheGetPersistent<T>(key: string): T | undefined {
  try {
    const raw = localStorage.getItem(PERSIST_PREFIX + key);
    if (!raw) return undefined;
    const entry = JSON.parse(raw) as CacheEntry<T>;
    if (Date.now() > entry.expire) {
      localStorage.removeItem(PERSIST_PREFIX + key);
      return undefined;
    }
    return entry.data;
  } catch {
    return undefined;
  }
}

/** 写入持久化缓存（localStorage）；容量超限或不可用时静默降级（内存缓存仍有效） */
export function cacheSetPersistent<T>(key: string, data: T, ttlMs: number) {
  try {
    localStorage.setItem(PERSIST_PREFIX + key, JSON.stringify({ data, expire: Date.now() + ttlMs } satisfies CacheEntry<T>));
  } catch {
    // localStorage 容量超限或不可用，静默降级
  }
}
