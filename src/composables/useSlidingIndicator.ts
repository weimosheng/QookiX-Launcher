import { onBeforeUnmount, onMounted, ref, type Ref } from "vue";

interface Options {
  /** Direction the indicator travels along: vertical = top/height, horizontal = left/width */
  axis?: "vertical" | "horizontal";
}

/**
 * Sliding active-highlight indicator.
 *
 * Positions an absolutely positioned element over the currently active child
 * and smoothly slides/scales it to follow the active item when it changes,
 * producing the "highlight expands to the target button then settles" effect.
 *
 * Usage:
 *   <div ref="box" class="rel">
 *     <div class="indicator" :style="indicatorStyle"></div>
 *     <button v-for="..." ref="items">…</button>
 *   </div>
 *
 * @param containerRef ref of the relative-positioned container
 * @param getItems     () => the clickable items in DOM order (e.g. container.querySelectorAll)
 * @param getActive    () => index of the active item (or -1 when none)
 * @param opts         axis: "vertical" (default) | "horizontal"
 */
export function useSlidingIndicator(
  containerRef: Ref<HTMLElement | null>,
  getItems: () => HTMLElement[],
  getActive: () => number,
  opts: Options = {}
) {
  const axis = opts.axis ?? "vertical";
  const indicatorStyle = ref<Record<string, string>>({ opacity: "0" });

  /** Resolve a ref value to a DOM element (unwraps Vue component instances too). */
  function toElement(v: unknown): HTMLElement | null {
    if (v instanceof HTMLElement) return v;
    // <router-link> and other components expose their root element via $el
    if (v && typeof v === "object" && "$el" in v) {
      const el = (v as { $el?: unknown }).$el;
      return el instanceof HTMLElement ? el : null;
    }
    return null;
  }

  // 记录上一次的矩形，用于“先扩展包裹、再收缩到目标”的两段式动画
  let last: { top: number; height: number; left: number; width: number } | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function setRect(rect: { top: number; height: number; left: number; width: number }, dur: string) {
    const base: Record<string, string> =
    axis === "vertical"
      ? { top: `${rect.top}px`, height: `${rect.height}px`, left: `${rect.left}px`, width: `${rect.width}px` }
      : { left: `${rect.left}px`, width: `${rect.width}px` };
    indicatorStyle.value = {
      opacity: "1",
      ...base,
      transition: `top ${dur}, height ${dur}, left ${dur}, width ${dur}, opacity 0.18s`,
    };
  }

  function update() {
    const container = toElement(containerRef.value);
    if (!container) return;
    // Query items fresh each time so the index always matches the DOM order,
    // even when the list is filtered/reordered dynamically.
    const el = toElement(getItems()[getActive()]);
    if (!el) return;
    const r = el.getBoundingClientRect();
    const c = container.getBoundingClientRect();
    const to = {
      top: r.top - c.top,
      height: r.height,
      left: r.left - c.left,
      width: r.width,
    };
    if (!last) {
      // 首次定位：直接到位，无动画
      setRect(to, "0s");
      last = to;
      return;
    }
    if (timer) clearTimeout(timer);
    // 阶段 1：扩展包裹“当前 + 目标”两段的整个区间
    setRect(
      {
        top: Math.min(last.top, to.top),
        height: Math.max(last.top + last.height, to.top + to.height) - Math.min(last.top, to.top),
        left: Math.min(last.left, to.left),
        width: Math.max(last.left + last.width, to.left + to.width) - Math.min(last.left, to.left),
      },
      "0.18s"
    );
    // 阶段 2：收缩贴合到目标按钮
    timer = setTimeout(() => {
      setRect(to, "0.18s");
      timer = null;
    }, 180);
    last = to;
  }

  let raf = 0;
  /** Schedule a re-measure on the next animation frame (call after DOM updates). */
  function refresh() {
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(update);
  }

  /** Instantly snap to the active item without animation (use after layout changes). */
  function snap() {
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(() => {
      const container = toElement(containerRef.value);
      if (!container) return;
      const el = toElement(getItems()[getActive()]);
      if (!el) return;
      const r = el.getBoundingClientRect();
      const c = container.getBoundingClientRect();
      const to = {
        top: r.top - c.top,
        height: r.height,
        left: r.left - c.left,
        width: r.width,
      };
      if (timer) clearTimeout(timer);
      timer = null;
      setRect(to, "0s");
      last = to;
    });
  }

  onMounted(() => {
    update();
    window.addEventListener("resize", refresh);
  });
  onBeforeUnmount(() => {
    if (timer) clearTimeout(timer);
    cancelAnimationFrame(raf);
    window.removeEventListener("resize", refresh);
  });

  return { indicatorStyle, update, refresh, snap };
}
