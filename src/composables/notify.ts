/**
 * 全局消息 API 持有器。
 *
 * naive-ui 的 useMessage() 只能在组件 setup 上下文中调用；
 * 在异步回调（如 deep-link 处理、全局事件）里调用会直接抛异常。
 * App.vue 在 setup 中调用 setMessageApi() 存下实例，
 * 任何非组件代码通过 notifySuccess/notifyError 使用。
 */

import { defineComponent, h } from "vue";
import { useMessage, type MessageApi } from "naive-ui";

let holder: MessageApi | null = null;

export function setMessageApi(api: MessageApi) {
  holder = api;
}

/** 放在 n-message-provider 内部，把 MessageApi 存入模块级持有器 */
export const MessageBridge = defineComponent({
  name: "MessageBridge",
  setup() {
    setMessageApi(useMessage());
    return () => h("span", { style: "display:none" });
  },
});

export function notifySuccess(content: string) {
  try {
    holder?.success(content);
  } catch {
    console.log("[notify]", content);
  }
}

export function notifyError(content: string) {
  try {
    holder?.error(content);
  } catch {
    console.error("[notify]", content);
  }
}

export function notifyWarning(content: string) {
  try {
    holder?.warning(content);
  } catch {
    console.warn("[notify]", content);
  }
}
