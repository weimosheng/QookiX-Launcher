import { ref } from "vue";

/**
 * 全局单例：同一时刻只允许一个实例卡片的「更多」下拉菜单处于打开状态，
 * 避免页面上多个实例的下拉菜单同时出现、相互遮挡。
 * 通过模块级 ref 在多个 InstanceCard 实例间共享。
 */
export const openMenuId = ref<string | null>(null);

let outsideBound = false;

function onPointerDown(e: Event) {
  if (openMenuId.value === null) return;
  const t = e.target as HTMLElement | null;
  // 点在某个「更多」按钮或菜单内部时不关闭，交给各自的点击逻辑处理
  if (t && t.closest(".more-wrap")) return;
  openMenuId.value = null;
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape" && openMenuId.value !== null) openMenuId.value = null;
}

/** 仅在首次打开菜单时绑定一次全局监听（关闭由单例统一处理） */
export function bindMenuOutside() {
  if (outsideBound) return;
  outsideBound = true;
  document.addEventListener("pointerdown", onPointerDown, true);
  document.addEventListener("keydown", onKeydown, true);
}
