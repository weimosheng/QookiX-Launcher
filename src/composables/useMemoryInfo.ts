/**
 * 系统内存信息（总量 / 已用 / 可用），供内存分配相关的界面使用。
 *
 * 此前 SettingsView 与 InstanceDetailView 各自维护一套 memTotal/memUsed/
 * memAvailable ref + loadMemoryInfo + setInterval，逻辑逐字重复。
 * 统一收敛到这里；调用方自行决定何时 startPolling / stopPolling
 * （例如只在某个标签页可见时轮询），组件卸载时会自动停止，避免计时器泄漏。
 */
import { onUnmounted, ref } from "vue";
import { api } from "../api";

export function useMemoryInfo(intervalMs = 10_000) {
  const memTotal = ref(0);
  const memUsed = ref(0);
  const memAvailable = ref(0);
  let timer: ReturnType<typeof setInterval> | null = null;

  async function loadMemoryInfo() {
    try {
      const res = await api.autoDetectMemory();
      memTotal.value = res.total_mb;
      memUsed.value = res.used_mb;
      memAvailable.value = res.available_mb ?? Math.max(0, res.total_mb - res.used_mb);
    } catch {
      /* 读取失败保持当前值，回退到默认配置 */
    }
  }

  /** 立即刷新一次并开始周期轮询（重复调用不会叠加计时器） */
  function startPolling() {
    loadMemoryInfo();
    if (!timer) timer = setInterval(loadMemoryInfo, intervalMs);
  }

  /** 停止轮询（已停止时为空操作） */
  function stopPolling() {
    if (timer) {
      clearInterval(timer);
      timer = null;
    }
  }

  onUnmounted(stopPolling);

  return { memTotal, memUsed, memAvailable, loadMemoryInfo, startPolling, stopPolling };
}
