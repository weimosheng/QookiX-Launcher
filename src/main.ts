import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import router from "./router";
import i18n from "./i18n";
import "./styles.css";

// 桌面启动器不需要浏览器默认右键菜单
window.addEventListener("contextmenu", (e) => e.preventDefault());

const app = createApp(App);
app.use(createPinia());
app.use(router);
app.use(i18n);
app.mount("#app");
