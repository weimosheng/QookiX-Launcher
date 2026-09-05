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
 * 两条到达路径（都会进入 onOpenUrl）：
 *   - 冷启动：deep-link 插件在 Rust 侧解析 argv 后发出 deep-link://new-url
 *   - 运行中唤起：tauri-plugin-single-instance 把二次启动的 argv 转发回
 *     主实例（lib.rs 回调里过滤 qookix:// 并重发同一事件），主实例 JS 端收到
 */
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { api } from "../api";
import { useInstancesStore } from "../stores/instances";
import { notifySuccess, notifyError, notifyWarning } from "./notify";
import router from "../router";
import { log as devLog, warn as devWarn, error as devError } from "../utils/logger";

/** 页面级 scheme → 路由路径映射 */
const PAGE_ROUTES: Record<string, string> = {
  home: "/",
  browse: "/browse",
  instances: "/instances",
  multiplayer: "/multiplayer",
  skins: "/skins",
  skin: "/skins", // 单数别名
  settings: "/settings",
  news: "/news",
  downloads: "/downloads",
};

async function handleUrl(raw: string) {
  devLog("[deeplink] handleUrl:", raw);
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    devWarn("[deeplink] 无法解析:", raw);
    return;
  }
  if (url.protocol !== "qookix:") return;
  // host 为第一段（qookix://skin），pathname 用于带参数的情况（qookix://launch/xxx）
  const host = url.hostname.toLowerCase();
  const path = decodeURIComponent(url.pathname.replace(/^\/+|\/+$/g, ""));

  if (host === "launch") {
    await launchByAlias(path);
    return;
  }

  const route = PAGE_ROUTES[host];
  if (route) {
    await router.push(route);
    devLog("[deeplink] 已跳转:", route);
    return;
  }
  // 非页面命令：把 host 当作实例别名处理（qookix://<别名> 是
  // qookix://launch/<别名> 的便捷写法）
  await launchByAlias(host);
}

/** 按别名查找实例并启动 */
async function launchByAlias(alias: string) {
  const a = alias.trim().toLowerCase();
  if (!a) {
    notifyWarning("协议缺少实例别名：qookix://launch/<别名>");
    return;
  }
  try {
    const instances = useInstancesStore();
    await instances.load();
    const inst = instances.instances.find((i) => (i.alias ?? "").toLowerCase() === a);
    if (!inst) {
      notifyError(`未找到别名为「${a}」的实例`);
      return;
    }
    await api.launchInstance(inst.id);
    notifySuccess(`正在启动「${inst.name}」`);
  } catch (e) {
    notifyError("启动失败：" + String(e));
  }
}

/** 在应用入口调用一次；冷启动与运行中的唤起都会触发 */
export async function initDeepLink() {
  // 去重闸：同一 URL 在短窗口内只处理一次。
  // 事件可能从多条路径到达（冷启动回放 + single-instance 转发 + 钩子转发），
  // 每条都合法但会导致实例被启动多次（实测同一链接连开三个游戏）。
  let lastUrl = "";
  let lastTs = 0;
  try {
    await onOpenUrl((urls) => {
      devLog("[deeplink] 收到 URL:", urls);
      const now = Date.now();
      for (const u of urls) {
        if (u === lastUrl && now - lastTs < 2000) {
          devLog("[deeplink] 2 秒内重复 URL，跳过:", u);
          continue;
        }
        lastUrl = u;
        lastTs = now;
        void handleUrl(u);
      }
    });
    devLog("[deeplink] 监听已注册");
  } catch (e) {
    // deep-link 权限缺失等情况不应阻塞应用启动
    devError("[deeplink] init failed:", e);
  }
}
