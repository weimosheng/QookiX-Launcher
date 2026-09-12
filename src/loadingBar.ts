import type { LoadingBarApi } from "naive-ui";

let api: LoadingBarApi | null = null;
let pending = 0;
let hasError = false;

/** 路由（页面）加载：延迟一小段再显示，避免瞬间完成的切换让加载条一闪而过 */
const NAV_START_DELAY = 140;
let navActive = false;
let navTimer: ReturnType<typeof setTimeout> | null = null;

export function setLoadingBarApi(a: LoadingBarApi) {
  api = a;
}

export function trackStart() {
  if (pending === 0) {
    api?.start();
    hasError = false;
  }
  pending++;
}

export function trackEnd() {
  pending = Math.max(0, pending - 1);
  if (pending === 0) {
    if (hasError) api?.error();
    else api?.finish();
    hasError = false;
  }
}

export function trackError() {
  hasError = true;
}

/**
 * 页面加载开始（路由切换，含懒加载页面 chunk）。
 * 与网络请求共用同一条顶部加载条；重复调用幂等。
 */
export function trackNavStart() {
  if (navActive) return;
  navActive = true;
  navTimer = setTimeout(() => {
    navTimer = null;
    if (navActive) trackStart();
  }, NAV_START_DELAY);
}

/** 页面加载结束：若还没真正显示则直接取消，避免短切换造成闪烁 */
export function trackNavEnd() {
  if (!navActive) return;
  navActive = false;
  if (navTimer) {
    clearTimeout(navTimer);
    navTimer = null;
    return;
  }
  trackEnd();
}
