import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import router from "./router";
import i18n from "./i18n";
import "./styles.css";

// 桌面启动器不需要浏览器默认右键菜单
window.addEventListener("contextmenu", (e) => e.preventDefault());

// 懒加载页面（路由 chunk）拉取失败时 Vite 会派发 preloadError：
// dev 下多见于长时间 HMR 后模块失效，正式版多见于发版后旧 chunk 被清掉。
// 不处理的话表现就是"点页面/点卡片没反应"（路由既不切换也不报错），
// 所以整体重载一次把它救回来；10 秒内只重载一次，避免一直失败时无限刷新。
window.addEventListener("vite:preloadError", () => {
  const key = "qookix:preload-reload-at";
  const last = Number(sessionStorage.getItem(key) ?? 0);
  if (Date.now() - last < 10_000) return;
  sessionStorage.setItem(key, String(Date.now()));
  window.location.reload();
});

const app = createApp(App);
app.use(createPinia());
app.use(router);
app.use(i18n);
app.mount("#app");
