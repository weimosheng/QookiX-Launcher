import { ref } from "vue";

// 根据侧边栏导航项顺序计算页面转场方向。
// 旧页面用 transform 移出提供方向感（淡出中磨砂异常不可见），
// 新页面纯 opacity 淡入（无 transform，backdrop-filter 全程稳定）。
export const transitionName = ref("page-fade");

// 侧边栏顺序：home(0) browse(1) instances(2) multiplayer(3) skins(4) toolbox(5) settings(6) news(7)
// 详情页/子页归到父级位置。
const NAV_ORDER: { match: RegExp; order: number }[] = [
  { match: /^\/$/, order: 0 },
  { match: /^\/browse/, order: 1 },
  { match: /^\/(instances|instance\/|create|downloads)/, order: 2 },
  { match: /^\/multiplayer/, order: 3 },
  { match: /^\/skins/, order: 4 },
  { match: /^\/toolbox/, order: 5 },
  { match: /^\/settings/, order: 6 },
  { match: /^\/news/, order: 7 },
];

function getNavOrder(path: string): number {
  for (const { match, order } of NAV_ORDER) {
    if (match.test(path)) return order;
  }
  return 0;
}

export function setTransitionName(from: string, to: string) {
  const fo = getNavOrder(from);
  const toOrder = getNavOrder(to);
  if (toOrder > fo) transitionName.value = "slide-forward";
  else if (toOrder < fo) transitionName.value = "slide-back";
  else transitionName.value = "page-fade";
}
