import { createRouter, createWebHistory } from "vue-router";
import { useSettingsStore } from "./stores/settings";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/", name: "home", component: () => import("./views/HomeView.vue"), meta: { title: "首页", icon: "home" } },
    { path: "/news", name: "news", component: () => import("./views/NewsView.vue"), meta: { title: "新闻", icon: "newspaper" } },
    { path: "/browse", name: "browse", component: () => import("./views/BrowseView.vue"), meta: { title: "内容中心", icon: "compass" } },
    { path: "/downloads", name: "downloads", component: () => import("./views/DownloadsView.vue"), meta: { title: "下载中心", icon: "download" } },
    { path: "/instances", name: "instances", component: () => import("./views/InstancesView.vue"), meta: { title: "游戏实例", icon: "grid", action: { text: "新建实例", icon: "plus", to: "/create" } } },
    { path: "/instance/:id", name: "instance", component: () => import("./views/InstanceDetailView.vue"), meta: { title: "实例详情", icon: "grid" } },
    { path: "/create", name: "create", component: () => import("./views/CreateInstanceView.vue"), meta: { title: "创建实例", icon: "plus" } },
    { path: "/multiplayer", name: "multiplayer", component: () => import("./views/MultiplayerView.vue"), meta: { title: "多人游戏", icon: "users" } },
    { path: "/multiplayer/:id", name: "server-detail", component: () => import("./views/ServerDetailView.vue"), meta: { title: "服务器详情", icon: "users" } },
    { path: "/settings", name: "settings", component: () => import("./views/SettingsView.vue"), meta: { title: "设置", icon: "settings" } },
    { path: "/skins", name: "skins", component: () => import("./views/SkinView.vue"), meta: { title: "皮肤中心", icon: "user" } },
  ],
});

// 关闭「新闻」后，直接访问 /news 会跳回首页（侧边栏入口本身也已隐藏）。
// 设置尚未加载时按「显示」处理，避免启动瞬间误跳转。
router.beforeEach((to) => {
  if (to.path !== "/news") return true;
  try {
    const s = useSettingsStore();
    if (s.settings && s.settings.show_news === false) return { path: "/", replace: true };
  } catch {
    // store 尚未初始化（Pinia 未激活）时放行
  }
  return true;
});

export default router;
