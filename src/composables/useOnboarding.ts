import { ref } from "vue";
import { driver, type Driver, type DriveStep } from "driver.js";
import "driver.js/dist/driver.css";
import router from "../router";
import { useSettingsStore } from "../stores/settings";

// 模块级单例：任意位置 import 均共享同一份向导状态。
const showOnboarding = ref(false);
/** 引导进行中：SideBar 据此临时展开，便于高亮各导航项 */
const tourActive = ref(false);

let driverObj: Driver | null = null;
let pending = false;

/** 指引库：跨页面高亮真实元素 + 气泡详述，按功能逻辑顺序编排。
 *  data.route 标注该步骤所在路由；onNextClick/onPrevClick 据此切换页面，
 *  driver.js 的 waitForElement 等目标页面元素渲染后再高亮。 */
function buildSteps(): DriveStep[] {
  return [
    {
      element: "#app-sidebar",
      popover: {
        title: "导航栏",
        description: "左侧是主要功能导航。接下来会带你切换到每个页面，逐一介绍它们的作用。",
      },
    },
    {
      element: "#account-chip",
      popover: {
        title: "账号管理",
        description: "点击头像可以添加微软正版、离线或皮肤站账号，也可切换当前用于游戏的账号。",
      },
    },
    {
      element: "#app-content",
      data: { route: "/" },
      popover: {
        title: "首页",
        description:
          "展示欢迎信息与你最近游玩的实例。有实例时这里会出现一键启动按钮，固定到首页的实例快捷入口也会显示在此，是启动游戏的常用起点。",
      },
    },
    {
      element: "#app-content",
      data: { route: "/instances" },
      popover: {
        title: "游戏实例",
        description:
          "管理所有 Minecraft 实例。每个实例是独立的游戏安装，包含游戏版本、模组加载器、模组与设置。可按分组整理；点击标题栏的「新建实例」按钮即可创建。",
      },
    },
    {
      element: "#browse-search",
      data: { route: "/browse" },
      popover: {
        title: "内容中心",
        description:
          "浏览 Modrinth 与 CurseForge 上的模组、整合包、资源包与光影。在搜索框输入名称即可查找，找到后可一键安装到当前实例。",
      },
    },
    {
      element: "#mp-tabs",
      data: { route: "/multiplayer" },
      popover: {
        title: "多人游戏",
        description:
          "两种联机模式：本地服务器（自建并托管 Minecraft 服务端）与联机房间（局域网联机）。可管理服务器列表、一键启动与加入。",
      },
    },
    {
      element: "#skin-preview",
      data: { route: "/skins" },
      popover: {
        title: "皮肤中心",
        description:
          "3D 实时预览你的 Minecraft 皮肤，支持行走、奔跑等动作展示。可从本地上传皮肤、管理皮肤库，并一键应用到当前账号。",
      },
    },
    {
      element: "#settings-nav",
      data: { route: "/settings" },
      popover: {
        title: "设置",
        description:
          "左侧是配置分类：常规、外观、Java、下载、内容服务、存储、关于。可调整主题、内存、下载源、代理等所有启动器行为。",
      },
    },
    {
      element: "#app-content",
      data: { route: "/" },
      popover: {
        title: "指引结束",
        description:
          "你已了解 QookiX 的主要功能。遇到问题随时在「设置 → 关于」点击「重播新手向导」重新查看。开始你的 Minecraft 之旅吧！",
      },
    },
  ];
}

function open() {
  if (driverObj || pending) return;
  pending = true;
  showOnboarding.value = true;
  // 先触发侧边栏展开（tourActive），等其宽度 transition（0.2s）完成后再启动引导，
  // 否则 driver.js 高亮 #app-sidebar 时会基于折叠尺寸计算高亮框，展开后高亮框错位。
  tourActive.value = true;
  const steps = buildSteps();
  // 预加载各页面 chunk，避免跨页导航时懒加载导致 waitForElement 超时卡顿
  const preload = Promise.all([
    import("../views/HomeView.vue"),
    import("../views/InstancesView.vue"),
    import("../views/BrowseView.vue"),
    import("../views/MultiplayerView.vue"),
    import("../views/SkinView.vue"),
    import("../views/SettingsView.vue"),
  ]);
  void Promise.all([new Promise((r) => setTimeout(r, 280)), preload]).then(() => {
    if (!pending) return;
    pending = false;
    driverObj = driver({
      showProgress: true,
      nextBtnText: "下一步 →",
      prevBtnText: "← 上一步",
      doneBtnText: "完成",
      skipMissingElement: true,
      waitForElement: 3000,
      steps,
      // 切换到下一步前，若该步位于其他路由则先导航，driver.js 会 waitForElement 等元素渲染。
      // 注意：定义了 onNextClick 后 driver 不再自动 moveNext，必须手动调用。
      // 跨页导航后 router-view 有 page-rise 过渡（~180ms），过渡中元素带 transform，
      // driver.js 测量到中间位置会导致高亮框/气泡错位，故导航后等过渡结束再 moveNext。
      onNextClick: (_el, _step, opts) => {
        const nextIdx = (opts.state.activeIndex ?? 0) + 1;
        if (nextIdx >= steps.length) {
          opts.driver.destroy();
          return;
        }
        const target = steps[nextIdx]?.data?.route as string | undefined;
        const needNav = !!target && router.currentRoute.value.path !== target;
        if (needNav) router.push(target);
        const act = () => opts.driver.moveNext();
        needNav ? setTimeout(act, 260) : act();
      },
      onPrevClick: (_el, _step, opts) => {
        const prevIdx = (opts.state.activeIndex ?? 0) - 1;
        const target = steps[prevIdx]?.data?.route as string | undefined;
        const needNav = !!target && router.currentRoute.value.path !== target;
        if (needNav) router.push(target);
        const act = () => opts.driver.movePrevious();
        needNav ? setTimeout(act, 260) : act();
      },
      onDestroyed: () => {
        driverObj = null;
        showOnboarding.value = false;
        tourActive.value = false;
        try {
          const settings = useSettingsStore();
          void settings.patch({ onboarding_completed: true }).catch(() => {});
        } catch {
          /* Pinia 未激活时忽略 */
        }
      },
    });
    driverObj.drive();
  });
}

function close() {
  pending = false;
  driverObj?.destroy();
  driverObj = null;
  showOnboarding.value = false;
  tourActive.value = false;
}

export function useOnboarding() {
  return { showOnboarding, tourActive, open, close };
}
