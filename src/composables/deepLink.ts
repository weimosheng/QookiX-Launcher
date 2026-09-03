/**
 * qookix:// 协议唤起处理。
 *
 * 支持的 URL：
 *   qookix://skin              → 打开皮肤中心
 *   qookix://instances         → 打开实例列表
 *   qookix://news              → 打开新闻
 *   qookix://settings          → 打开设置
 *   qookix://launch/<别名>      → 启动设置了该别名的实例
 *
 * 实现说明：桌面端由 tauri-plugin-deep-link 注册 qookix scheme（见
 * tauri.conf.json），运行中的实例通过 onOpenUrl 事件收到 URL；
 * 冷启动（应用未运行时点击链接）由插件缓存后同样通过 onOpenUrl 下发。
 */
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { api } from "../api";
import { useInstancesStore } from "../stores/instances";
import { useMessage } from "naive-ui";
import router from "../router";

/** 页面级 scheme → 路由路径映射 */
const PAGE_ROUTES: Record<string, string> = {
  home: "/",
  browse: "/browse",
  instances: "/instances",
  multiplayer: "/multiplayer",
  skins: "/skins",
  settings: "/settings",
  news: "/news",
  downloads: "/downloads",
};

async function handleUrl(raw: string) {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return;
  }
  if (url.protocol !== "qookix:") return;
  // host 为第一段（qookix://skin），pathname 用于带参数的情况（qookix://launch/xxx）
  const host = url.hostname.toLowerCase();
  const path = decodeURIComponent(url.pathname.replace(/^\/+|\/+$/g, ""));
  const message = useMessage();

  if (host === "launch") {
    const alias = path.trim().toLowerCase();
    if (!alias) {
      message.warning("协议缺少实例别名：qookix://launch/<别名>");
      return;
    }
    try {
      const instances = useInstancesStore();
      await instances.load();
      const inst = instances.instances.find((i) => (i.alias ?? "").toLowerCase() === alias);
      if (!inst) {
        message.error(`未找到别名为「${alias}」的实例`);
        return;
      }
      await api.launchInstance(inst.id);
      message.success(`正在启动「${inst.name}」`);
    } catch (e) {
      message.error("启动失败：" + String(e));
    }
    return;
  }

  const route = PAGE_ROUTES[host];
  if (route) {
    await router.push(route);
  } else {
    message.warning(`未知的 qookix:// 命令：${host || "(空)"}`);
  }
}

/** 在应用入口调用一次；冷启动与运行中的唤起都会触发 */
export async function initDeepLink() {
  try {
    await onOpenUrl((urls) => {
      for (const u of urls) void handleUrl(u);
    });
  } catch (e) {
    // deep-link 权限缺失等情况不应阻塞应用启动
    console.error("[deeplink] init failed:", e);
  }
}
