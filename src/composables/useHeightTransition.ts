/**
 * 展开 / 收起的高度过渡（`<Transition :css="false">` 的 JS 钩子），下载任务卡与云存档条目共用。
 * 结构：外层管高度 + overflow:hidden，内层放内容——直接量一个 height 正被改写的元素量不准。
 * 两个坑：结束信号必须用 transitionend（setTimeout 会早 1~2 帧、结尾跳一下，另留兜底定时器）；
 * 动画期间内容还会变长，用 ResizeObserver 同步目标高度。父容器 flex+gap 时用负 margin-top 抵消。
 */

export interface HeightTransitionOptions {
  /** 与上一个兄弟节点的间距（父容器 flex gap） */
  gap?: number;
  /** 时长（ms） */
  duration?: number;
  /** 缓动函数 */
  ease?: string;
}

/** 连续快速点击时用它取消上一次动画 */
type AnimEl = HTMLElement & { _htCancel?: () => void };

export function useHeightTransition(options: HeightTransitionOptions = {}) {
  const gap = options.gap ?? 10;
  const duration = options.duration ?? 240;
  const ease = options.ease ?? "cubic-bezier(0.22, 1, 0.36, 1)";
  const transition = `height ${duration}ms ${ease}, margin-top ${duration}ms ${ease}, opacity ${duration}ms ${ease}`;

  function innerOf(el: HTMLElement): HTMLElement | null {
    return el.firstElementChild instanceof HTMLElement ? el.firstElementChild : null;
  }

  function contentHeight(el: HTMLElement): number {
    const inner = innerOf(el);
    return inner ? inner.offsetHeight : el.scrollHeight;
  }

  /** 等 height 真正跑完；定时器只作兜底，不作正常结束信号 */
  function whenHeightDone(el: AnimEl, done: () => void, after?: () => void) {
    let settled = false;
    const finish = () => {
      if (settled) return;
      settled = true;
      el.removeEventListener("transitionend", onEnd);
      clearTimeout(timer);
      el._htCancel = undefined;
      after?.();
      done();
    };
    // 子元素也会冒泡出 transitionend，认准 target 是自己 + 属性是 height
    const onEnd = (ev: TransitionEvent) => {
      if (ev.target !== el || ev.propertyName !== "height") return;
      finish();
    };
    // 兜底：高度没变、元素被隐藏等场景下也要放行
    const timer = window.setTimeout(finish, duration + 120);
    el._htCancel = () => {
      if (settled) return;
      settled = true;
      el.removeEventListener("transitionend", onEnd);
      clearTimeout(timer);
      el._htCancel = undefined;
    };
    el.addEventListener("transitionend", onEnd);
  }

  function onEnter(el: Element, done: () => void) {
    const e = el as AnimEl;
    e._htCancel?.();

    e.style.transition = "none";
    e.style.overflow = "hidden";
    e.style.height = "0px";
    e.style.marginTop = gap ? `-${gap}px` : "0px";
    e.style.opacity = "0";
    void e.offsetHeight; // 强制回流，让起始态生效

    const inner = innerOf(e);
    let ro: ResizeObserver | null = null;
    if (inner) {
      ro = new ResizeObserver(() => {
        e.style.height = `${contentHeight(e)}px`;
      });
      ro.observe(inner);
    }

    e.style.transition = transition;
    e.style.height = `${contentHeight(e)}px`;
    e.style.marginTop = "0px";
    e.style.opacity = "1";

    whenHeightDone(e, done, () => {
      ro?.disconnect();
      e.style.transition = "none";
      // 交还 auto，让后续动态内容能自由撑开
      e.style.height = "";
      e.style.marginTop = "";
      e.style.opacity = "";
      e.style.overflow = "";
      void e.offsetHeight;
      e.style.transition = "";
    });
  }

  function onLeave(el: Element, done: () => void) {
    const e = el as AnimEl;
    e._htCancel?.();

    e.style.transition = "none";
    e.style.overflow = "hidden";
    e.style.height = `${e.offsetHeight}px`;
    e.style.marginTop = "0px";
    e.style.opacity = "1";
    void e.offsetHeight; // 强制回流，让起始态生效

    e.style.transition = transition;
    e.style.height = "0px";
    e.style.marginTop = gap ? `-${gap}px` : "0px";
    e.style.opacity = "0";

    whenHeightDone(e, done, () => {
      e.style.transition = "";
      e.style.height = "";
      e.style.marginTop = "";
      e.style.opacity = "";
      e.style.overflow = "";
    });
  }

  return { onEnter, onLeave };
}
