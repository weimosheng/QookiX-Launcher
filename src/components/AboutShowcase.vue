<script setup lang="ts">
/**
 * 关于页顶部展示卡片画布。
 *
 * 实现：
 * - 背景：红色格子「桌布」，随时间做斜向（x/y 同步）循环移动。
 * - 中央：QookiX 曲奇 logo，循环播放「完整 → 被咬 → 复原 → 保持……」。
 * - 关键：咬是「一个事件」，不是先后分开的阶段。咬下的那一帧就**同时**：
 *   1) 咬口弹簧把缺口「弹开」；
 *   2) 给旋转弹簧注入一个角速度冲量，曲奇立刻抖动；
 *   3) 从缺口喷出碎屑粒子，并在咬开过程中持续掉落。
 *   因此咬、抖、粒子三者同时发生，观感连贯顺滑。
 * - 咬口：用 destination-out + globalAlpha 按咬口弹簧值(0..1)从右上边缘抹掉一小块，
 *   并带几颗「牙印」。复原阶段咬口弹簧弹回 0，整块曲奇愈合。
 * - 2D Procrustes 对齐：咬掉后质心偏移，用两圆相交面积比做反向平移，
 *   把质心拉回画布中心。
 * - 物理：全部弹簧用「固定小步长累积积分」推进，避免可变帧长导致回弹不一致，
 *   保证动画在不同帧率下都顺滑。
 */
import { onMounted, onUnmounted, ref } from "vue";
import logoUrl from "../assets/logo.png";

const wrapRef = ref<HTMLDivElement | null>(null);
const canvasRef = ref<HTMLCanvasElement | null>(null);

/* ---- 阻尼弹簧振荡器（半隐式欧拉积分，target 可随时间变化） ---- */
interface Spring {
  value: number;
  velocity: number;
  stiffness: number;
  damping: number;
  target: number;
}

function springStep(s: Spring, dt: number) {
  const force = (s.target - s.value) * s.stiffness - s.velocity * s.damping;
  s.velocity += force * dt;
  s.value += s.velocity * dt;
}

/* ---- 动画时间线（单位：秒） ---- */
// 周期：静止(settle) → [咬下: 张开(open) + 愈合(heal)] → 尾段静止
const SETTLE = 1.15;
const OPEN = 0.85; // 咬口张开时长
const HEAL = 0.9; // 复原时长
const CYCLE = 3.4;
const IMPACT = -2.2; // 咬下注入的角速度冲量：让曲奇「被撞」一下，快速偏到位
const BITE_TILT = -0.34; // 咬着时曲奇保持的偏转（rad，约 -20°）

/* 旋转弹簧分两套参数，分别对应两个动作，两者都保留「弹簧弹性」：
 * - 被咬时(tilt)：明显欠阻尼 —— 注入冲量后曲奇被撞得先冲过 BITE_TILT，
 *   再被弹回到 BITE_TILT 稳住，即「移动到位置」带弹性过冲，不是硬到位。
 * - 复位时(restore)：更慢更欠阻尼 —— 从偏转慢慢回正，冲过头越过 0°
 *   到另一侧，再被弹回正位停稳。关键是慢（ω≈7.7，全程约 1 秒）才看得清；
 *   之前 stiffness 220 时约 0.1 秒摆完，看着像闪一下。 */
const TILT_STIFF = 90;
const TILT_DAMP = 9; // 远小于临界阻尼 2*sqrt(90)≈19 → 欠阻尼，保留过冲回弹
const RESTORE_STIFF = 60;
const RESTORE_DAMP = 2.8;

const bite: Spring = { value: 0, velocity: 0, stiffness: 300, damping: 26, target: 0 };
const angle: Spring = { value: 0, velocity: 0, stiffness: TILT_STIFF, damping: TILT_DAMP, target: 0 };
/* 躲避位移弹簧：被咬时曲奇整体往「远离咬口」方向躲闪的位移。
 * 独立于 bite 弹簧、欠阻尼 → 位移本身带惯性弹性（冲过头再弹回），
 * 而不是跟着咬口瞬间到位。value 目标 1=躲开、0=回位。 */
const RECOIL_DAMP = 8;
const recoil: Spring = { value: 0, velocity: 0, stiffness: 80, damping: RECOIL_DAMP, target: 0 };

let prevBiteOpen = false; // 上一次 target 是否"张开"，用于检测咬下/复位那一帧

/* ---- 时间推进 / 咬事件 ---- */
const STEP = 1 / 180; // 固定物理步长
let acc = 0;
let elapsed = 0;

let burstDone = false;

function computeBiteTarget(s: number) {
  if (s < SETTLE) return 0;
  if (s < SETTLE + OPEN) return 1;
  if (s < SETTLE + OPEN + HEAL) return 0; // 愈合阶段
  return 0; // 尾段静止
}

function updatePhysics() {
  const s = elapsed % CYCLE;
  bite.target = computeBiteTarget(s);

  // 检测「咬下」的一帧：目标从闭合(0)翻转到张开(1)
  const isOpen = bite.target >= 0.5;
  if (isOpen && !prevBiteOpen) {
    // 被咬：欠阻尼弹簧 → 曲奇被撞得冲过 BITE_TILT 再弹回到位（有弹性）
    angle.stiffness = TILT_STIFF;
    angle.damping = TILT_DAMP;
    angle.target = BITE_TILT;
    angle.velocity += IMPACT; // 咬的瞬间被「撞」一下
    // 躲避位移：往外躲，欠阻尼 → 冲过头再弹回稳定位置
    recoil.target = 1;
    recoil.velocity += 3.2;
    spawnBurst(7); // 同步喷出碎屑
    burstDone = false;
  } else if (!isOpen && prevBiteOpen) {
    // 复位（愈合开始）：切到「软」弹簧，目标回正位 0。
    // 欠阻尼让它从偏转慢慢回正，并冲过头越过 0° 到另一侧，再弹回停稳。
    angle.stiffness = RESTORE_STIFF;
    angle.damping = RESTORE_DAMP;
    angle.target = 0;
    // 位移也回位，同样带弹性
    recoil.target = 0;
  }
  prevBiteOpen = isOpen;

  // 咬开过程中持续掉落几粒碎屑，让「咬」更有啃食感
  if (isOpen && !burstDone && Math.random() < 0.5) {
    spawnBurst(1);
  }
  // 张开到位后停（value 接近 target 就视为到位，避免整周期喷屑）
  if (isOpen && Math.abs(bite.value - 1) < 0.05) burstDone = true;

  springStep(bite, STEP);
  springStep(angle, STEP);
  springStep(recoil, STEP);
  updateCrumbs(STEP);
}

/* ---- 画布尺寸 / 句柄 ---- */
let last = 0;
let raf = 0;
let dpr = 1;
let cssW = 1;
let cssH = 1;
let ctxRef: CanvasRenderingContext2D | null = null;
let offscreen: HTMLCanvasElement | null = null;
let offCtx: CanvasRenderingContext2D | null = null;

const cookieImg = new Image();
cookieImg.src = logoUrl;
let imgLoaded = false;
cookieImg.onload = () => {
  imgLoaded = true;
};
cookieImg.onerror = () => {
  imgLoaded = false;
};

/* ---- 咬口碎屑粒子 ---- */
interface Crumb {
  x: number;
  y: number;
  vx: number;
  vy: number;
  size: number;
  life: number;
  age: number;
}
let crumbs: Crumb[] = [];

function spawnBurst(n: number) {
  if (!ctxRef || n < 1) return;
  const cx = cssW / 2;
  const cy = cssH / 2;
  const c = localToWorld(
    biteDc * Math.cos(BITE_PHI),
    biteDc * Math.sin(BITE_PHI),
    cx,
    cy,
  );
  for (let i = 0; i < n; i++) {
    const a = Math.atan2(c.y - cy, c.x - cx) + (Math.random() - 0.5) * 2.2;
    const sp = (30 + Math.random() * 90) * (n === 7 ? 1 : 0.5);
    crumbs.push({
      x: c.x,
      y: c.y,
      vx: Math.cos(a) * sp,
      vy: Math.sin(a) * sp - 40,
      size: 1.6 + Math.random() * 3.4,
      life: 0.5 + Math.random() * 0.55,
      age: 0,
    });
  }
}

function updateCrumbs(dt: number) {
  for (let i = crumbs.length - 1; i >= 0; i--) {
    const cr = crumbs[i];
    cr.age += dt;
    if (cr.age >= cr.life) {
      crumbs.splice(i, 1);
      continue;
    }
    cr.vx *= Math.exp(-1.8 * dt);
    cr.vy += 70 * dt;
    cr.x += cr.vx * dt;
    cr.y += cr.vy * dt;
  }
}

/* ---- 曲奇几何 ---- */
function cookieRadius() {
  return Math.round(Math.min(cssH * 0.34, cssW * 0.17));
}
const BITE_PHI = -0.95; // 咬口位于曲奇右上
let R = 60;
let biteR = 30;
let biteDc = 50;

/* 两圆相交面积（R: 曲奇半径, r: 咬口半径, d: 圆心距） */
function lensArea(Rr: number, r: number, d: number): number {
  if (d >= Rr + r) return 0;
  if (d <= Math.abs(Rr - r)) return Math.PI * Math.min(Rr, r) ** 2;
  const t1 = Rr * Rr * Math.acos((d * d + Rr * Rr - r * r) / (2 * d * Rr));
  const t2 = r * r * Math.acos((d * d + r * r - Rr * Rr) / (2 * d * r));
  const t3 =
    0.5 *
    Math.sqrt(
      (-d + Rr + r) * (d + Rr - r) * (d - Rr + r) * (d + Rr + r),
    );
  return t1 + t2 - t3;
}

/* 咬口 = 一个主圆 + 沿其圆周（朝曲奇内部那侧）分布的一圈「大小不一的小圆」。
 * 主圆把曲奇右上啃出一个圆凹口；小圆圆心落在主圆圆周上、半径各不同，
 * 只让圆凹的边缘变成参差的波状，整体仍是"一个凹口"，不是一排整齐的齿。
 * 数据存角度 offset（绕主圆中心、相对「咬口指向曲奇中心」的方向展开）
 * 与半径因子；模块级固定随机，避免逐帧闪烁。 */
const BITE_EDGES: { off: number; rf: number }[] = (() => {
  let s = 7919;
  const rnd = () => {
    s = (s * 16807) % 2147483647;
    return s / 2147483647;
  };
  const N = 9;
  const arr: { off: number; rf: number }[] = [];
  for (let i = 0; i < N; i++) {
    const t = i / (N - 1);
    arr.push({
      off: -1.5 + t * 3 + (rnd() - 0.5) * 0.45, // 沿主圆边缘铺开大半圈
      rf: 0.045 + rnd() * 0.07, // 细碎小圆，只为啃出轻微锯齿
    });
  }
  return arr;
})();

/* 局部(含质心补偿平移 + 旋转) → 画布坐标 */
function localToWorld(px: number, py: number, cx: number, cy: number) {
  const qx = px + centeringDx();
  const qy = py + centeringDy();
  const c = Math.cos(angle.value);
  const s = Math.sin(angle.value);
  return { x: cx + qx * c - qy * s, y: cy + qx * s + qy * c };
}

let centeringDx = () => 0;
let centeringDy = () => 0;

/* ---- 背景：斜向滚动的格子桌布 ----
 * 配色说明：用冷调柔和蓝灰，与 UI 深蓝灰背景(#0b0d12)同系，
 * 又与暖棕曲奇形成冷暖互补对比，让曲奇成为画面唯一焦点。
 * 之前用高饱和正红 #e4574f，既跟深蓝灰 UI 冲突，又和曲奇同属暖色、明度接近，
 * 主体被背景糊住——所以换成低饱和冷色。想换风格改这两个常量即可。
 * 备选(暖褐木质，跟 accent 同系)：CLOTH_A="#4a382a"、CLOTH_B="#6b5442"
 * 备选(经典红白野餐布，更活泼)：CLOTH_A="#c2605a"、CLOTH_B="#f3e6d2" */
const CLOTH_A = "#E19859"; // 调浅的木色，避免过深
const CLOTH_B = "#f4efe6"; // 浅格改暖米白，与木质同系又不刺眼

function drawTablecloth(g: CanvasRenderingContext2D, t: number) {
  const tile = 46;
  const speed = 40; // px/s，x/y 同步 → 斜向
  const off = (speed * t) % tile;
  const nX = Math.ceil(cssW / tile) + 2;
  const nY = Math.ceil(cssH / tile) + 2;
  for (let j = -1; j < nY; j++) {
    for (let i = -1; i < nX; i++) {
      const even = (i + j) & 1;
      g.fillStyle = even ? CLOTH_A : CLOTH_B;
      g.fillRect(i * tile + off, j * tile + off, tile + 1, tile + 1);
    }
  }
}

/* ---- 桌布上的柔和投影 ---- */
function drawShadow(g: CanvasRenderingContext2D, cx: number, cy: number) {
  g.save();
  g.translate(cx, cy + R * 0.55);
  g.scale(1, 0.3);
  const grad = g.createRadialGradient(0, 0, 0, 0, 0, R * 1.15);
  grad.addColorStop(0, "rgba(20,10,5,0.28)");
  grad.addColorStop(1, "rgba(20,10,5,0)");
  g.fillStyle = grad;
  g.beginPath();
  g.arc(0, 0, R * 1.15, 0, Math.PI * 2);
  g.fill();
  g.restore();
}

/* ---- 图片未加载时的备用程序化曲奇 ---- */
function drawCookieShape(g: CanvasRenderingContext2D, rr: number) {
  g.save();
  g.shadowColor = "rgba(0,0,0,0.12)";
  g.shadowBlur = 4;
  const body = g.createRadialGradient(-rr * 0.3, -rr * 0.35, rr * 0.1, 0, 0, rr * 1.1);
  body.addColorStop(0, "#d99a5a");
  body.addColorStop(1, "#a76a38");
  g.fillStyle = body;
  g.beginPath();
  g.arc(0, 0, rr, 0, Math.PI * 2);
  g.fill();
  g.shadowBlur = 0;
  g.fillStyle = "rgba(60,32,16,0.9)";
  const chips = [
    [-0.42, -0.25, 0.1], [0.1, -0.48, 0.12], [0.45, 0.1, 0.09],
    [-0.05, 0.42, 0.11], [-0.5, 0.3, 0.08], [0.35, 0.42, 0.09],
  ];
  for (const [sx, sy, sr] of chips) {
    g.beginPath();
    g.arc(sx * rr, sy * rr, sr * rr, 0, Math.PI * 2);
    g.fill();
  }
  g.restore();
}

/* ---- 在离屏层画曲奇 + 咬口（避免擦穿桌布） ---- */
function drawCookie() {
  if (!offCtx || !offscreen) return;
  offCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
  offCtx.clearRect(0, 0, cssW, cssH);

  const cx = cssW / 2;
  const cy = cssH / 2;
  R = cookieRadius();
  biteR = R * 0.5;
  biteDc = R * 0.82;

  // Procrustes 对齐：把咬掉缺口造成的质心偏移拉回画布中心。
  // 位移由独立的 recoil 弹簧驱动（欠阻尼 → 冲过头再弹回），不再是瞬间到位。
  const fullArea = Math.PI * R * R;
  const removed = lensArea(R, biteR, biteDc);
  const bv = Math.min(1, Math.max(0, bite.value));
  const k = recoil.value * (removed / Math.max(1e-6, fullArea - removed));
  const bx0 = biteDc * Math.cos(BITE_PHI);
  const by0 = biteDc * Math.sin(BITE_PHI);
  // 咬口在曲奇右上（bx0>0, by0<0）。曲奇应往「远离咬口」的方向（左下）躲闪，
  // 所以补偿平移取反：-k*bx0（左）、-k*by0（下）。
  centeringDx = () => -k * bx0;
  centeringDy = () => -k * by0;

  offCtx.save();
  offCtx.translate(cx, cy);
  offCtx.rotate(angle.value);
  offCtx.translate(centeringDx(), centeringDy());

  const d = R * 2;
  if (imgLoaded) {
    offCtx.drawImage(cookieImg, -R, -R, d, d);
  } else {
    drawCookieShape(offCtx, R);
  }

  // 咬口：destination-out 一次性填充「主圆 + 沿其圆周朝曲奇内部的小圆」。
  // 主圆给出基本凹口，边缘小圆（大小不一、圆心落在主圆圆周上）把光滑的
  // 圆凹边界啃成参差波浪，整体仍像一个大凹口而非一排齿。
  // 全部子路径同向 → 非零环绕 = 并集，透明度由 globalAlpha 整体控制。
  const depth = bv;
  if (depth > 0.002) {
    // 咬口圆心指向曲奇中心的方向角，作为小圆展开的基准角
    const towardCenter = Math.atan2(-by0, -bx0);
    const path = new Path2D();
    path.arc(bx0, by0, biteR, 0, Math.PI * 2);
    for (const e of BITE_EDGES) {
      const a = towardCenter + e.off;
      path.arc(bx0 + biteR * Math.cos(a), by0 + biteR * Math.sin(a), biteR * e.rf, 0, Math.PI * 2);
    }
    offCtx.globalCompositeOperation = "destination-out";
    offCtx.globalAlpha = depth;
    offCtx.fill(path);
    offCtx.globalAlpha = 1;
    offCtx.globalCompositeOperation = "source-over";
  }
  offCtx.restore();
}

function drawCrumbs(g: CanvasRenderingContext2D) {
  for (const cr of crumbs) {
    const alpha = Math.max(0, 1 - cr.age / cr.life);
    g.globalAlpha = alpha;
    g.fillStyle = "#8a5a2e";
    g.beginPath();
    g.arc(cr.x, cr.y, cr.size, 0, Math.PI * 2);
    g.fill();
  }
  g.globalAlpha = 1;
}

function drawVignette(g: CanvasRenderingContext2D) {
  const grad = g.createRadialGradient(
    cssW / 2, cssH / 2, Math.min(cssW, cssH) * 0.3,
    cssW / 2, cssH / 2, Math.max(cssW, cssH) * 0.75,
  );
  grad.addColorStop(0, "rgba(0,0,0,0)");
  grad.addColorStop(1, "rgba(30,10,5,0.42)");
  g.fillStyle = grad;
  g.fillRect(0, 0, cssW, cssH);
}

function draw(t: number) {
  const g = ctxRef;
  if (!g) return;
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, cssW, cssH);
  drawTablecloth(g, t);
  const cx = cssW / 2;
  const cy = cssH / 2;
  drawShadow(g, cx, cy);
  drawCookie();
  if (offscreen) g.drawImage(offscreen, 0, 0, cssW, cssH);
  drawCrumbs(g);
  drawVignette(g);
}

/* ---- 尺寸 / 渲染循环 ---- */
function resize() {
  const wrap = wrapRef.value;
  const canvas = canvasRef.value;
  if (!wrap || !canvas) return;
  dpr = Math.min(window.devicePixelRatio || 1, 2);
  cssW = Math.max(1, Math.round(wrap.clientWidth));
  cssH = Math.max(1, Math.round(wrap.clientHeight));
  const bw = Math.round(cssW * dpr);
  const bh = Math.round(cssH * dpr);
  if (canvas.width !== bw || canvas.height !== bh) {
    canvas.width = bw;
    canvas.height = bh;
  }
  if (!offscreen || offscreen.width !== bw || offscreen.height !== bh) {
    offscreen = document.createElement("canvas");
    offscreen.width = bw;
    offscreen.height = bh;
    offCtx = offscreen.getContext("2d");
  }
}

function frame(now: number) {
  const dtReal = Math.min((now - last) / 1000, 0.1);
  last = now;
  const canvas = canvasRef.value;
  const active = !!canvas && canvas.clientWidth > 0 && !document.hidden;
  if (active) {
    elapsed += dtReal;
    // 固定小步长累积推进物理，保证弹簧积分稳定顺滑
    acc += dtReal;
    let guard = 0;
    while (acc >= STEP && guard < 40) {
      updatePhysics();
      acc -= STEP;
      guard++;
    }
    draw(elapsed);
  }
  raf = requestAnimationFrame(frame);
}

let ro: ResizeObserver | null = null;

onMounted(() => {
  const canvas = canvasRef.value;
  if (canvas) ctxRef = canvas.getContext("2d");
  resize();
  last = performance.now();
  raf = requestAnimationFrame(frame);
  ro = new ResizeObserver(() => resize());
  if (wrapRef.value) ro.observe(wrapRef.value);
});

onUnmounted(() => {
  cancelAnimationFrame(raf);
  ro?.disconnect();
  crumbs.length = 0;
});
</script>

<template>
  <div ref="wrapRef" class="about-show-wrap">
    <canvas ref="canvasRef" class="about-show-canvas"></canvas>
  </div>
</template>

<style scoped>
.about-show-wrap {
  position: relative;
  width: 100%;
  height: 200px;
  overflow: hidden;
}
.about-show-canvas {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  display: block;
}
</style>
