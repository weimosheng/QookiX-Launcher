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

  function update() {
    const container = toElement(containerRef.value);
    if (!container) return;
    // Query items fresh each time so the index always matches the DOM order,
    // even when the list is filtered/reordered dynamically.
    const el = toElement(getItems()[getActive()]);
    if (!el) return;
    const r = el.getBoundingClientRect();
    const c = container.getBoundingClientRect();
    if (axis === "vertical") {
      indicatorStyle.value = {
        opacity: "1",
        top: `${r.top - c.top}px`,
        height: `${r.height}px`,
      };
    } else {
      indicatorStyle.value = {
        opacity: "1",
        left: `${r.left - c.left}px`,
        width: `${r.width}px`,
      };
    }
  }

  let raf = 0;
  /** Schedule a re-measure on the next animation frame (call after DOM updates). */
  function refresh() {
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(update);
  }

  onMounted(() => {
    update();
    window.addEventListener("resize", refresh);
  });
  onBeforeUnmount(() => {
    cancelAnimationFrame(raf);
    window.removeEventListener("resize", refresh);
  });

  return { indicatorStyle, update, refresh };
}
