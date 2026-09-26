import { createRouter, createWebHistory } from "vue-router";
import { useSettingsStore } from "./stores/settings";
import { trackNavStart, trackNavEnd, trackError } from "./loadingBar";
import { setTransitionName } from "./composables/usePageTransition";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/", name: "home", component: () => import("./views/HomeView.vue"), meta: { titleKey: "nav.home", icon: "home" } },
    { path: "/news", name: "news", component: () => import("./views/NewsView.vue"), meta: { titleKey: "nav.news", icon: "newspaper" } },
    { path: "/browse", name: "browse", component: () => import("./views/BrowseView.vue"), meta: { titleKey: "nav.browse", icon: "compass" } },
    { path: "/downloads", name: "downloads", component: () => import("./views/DownloadsView.vue"), meta: { titleKey: "nav.downloads", icon: "download" } },
    { path: "/instances", name: "instances", component: () => import("./views/InstancesView.vue"), meta: { titleKey: "nav.instances", icon: "grid", action: { textKey: "page.newInstance", icon: "plus", to: "/create" } } },
    { path: "/instance/:id", name: "instance", component: () => import("./views/InstanceDetailView.vue"), meta: { titleKey: "page.instanceDetail", icon: "grid" } },
    { path: "/create", name: "create", component: () => import("./views/CreateInstanceView.vue"), meta: { titleKey: "page.create", icon: "plus" } },
    { path: "/multiplayer", name: "multiplayer", component: () => import("./views/MultiplayerView.vue"), meta: { titleKey: "nav.multiplayer", icon: "users" } },
    { path: "/multiplayer/:id", name: "server-detail", component: () => import("./views/ServerDetailView.vue"), meta: { titleKey: "page.serverDetail", icon: "users" } },
    { path: "/settings", name: "settings", component: () => import("./views/SettingsView.vue"), meta: { titleKey: "nav.settings", icon: "settings" } },
    { path: "/toolbox", name: "toolbox", component: () => import("./views/ToolboxView.vue"), meta: { titleKey: "nav.toolbox", icon: "tool" } },
    { path: "/toolbox/seed", name: "toolbox-seed", component: () => import("./views/SeedMapView.vue"), meta: { titleKey: "page.seedMap", icon: "tool" } },
    { path: "/toolbox/schematic", name: "toolbox-schematic", component: () => import("./views/SchematicPreviewView.vue"), meta: { titleKey: "page.schematicPreview", icon: "tool" } },
    { path: "/skins", name: "skins", component: () => import("./views/SkinView.vue"), meta: { titleKey: "nav.skins", icon: "user" } },
  ],
});

// —— 页面加载指示 ——
// 顶部加载条只服务于「网络请求」（见 api.ts）与「页面加载」两种场景：
// 这里让路由切换（含懒加载页面 chunk）期间显示加载条。
router.beforeEach((to, from) => {
  if (to.fullPath !== from.fullPath) {
    trackNavStart();
    setTransitionName(from.path, to.path);
  }

  // 关闭「新闻」后，直接访问 /news 会跳回首页（侧边栏入口本身也已隐藏）。
  // 设置尚未加载时按「显示」处理，避免启动瞬间误跳转。
  if (to.path !== "/news") return true;
  try {
    const s = useSettingsStore();
    if (s.settings && s.settings.show_news === false) return { path: "/", replace: true };
  } catch {
    // store 尚未初始化（Pinia 未激活）时放行
  }
  return true;
});

// 导航结束（成功、失败或重定向）都收尾，trackNavEnd 幂等
router.afterEach(() => trackNavEnd());
router.onError(() => {
  trackError();
  trackNavEnd();
});

export default router;
