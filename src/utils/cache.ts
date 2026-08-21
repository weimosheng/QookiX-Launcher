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
