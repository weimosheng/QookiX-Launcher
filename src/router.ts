import { createRouter, createWebHistory } from "vue-router";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/", name: "home", component: () => import("./views/HomeView.vue") },
    { path: "/browse", name: "browse", component: () => import("./views/BrowseView.vue") },
    { path: "/downloads", name: "downloads", component: () => import("./views/DownloadsView.vue") },
    { path: "/instances", name: "instances", component: () => import("./views/InstancesView.vue") },
    { path: "/instance/:id", name: "instance", component: () => import("./views/InstanceDetailView.vue") },
    { path: "/create", name: "create", component: () => import("./views/CreateInstanceView.vue") },
    { path: "/settings", name: "settings", component: () => import("./views/SettingsView.vue") },
  ],
});

export default router;
