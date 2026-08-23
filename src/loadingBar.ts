import type { LoadingBarApi } from "naive-ui";

let api: LoadingBarApi | null = null;
let pending = 0;
let hasError = false;

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
