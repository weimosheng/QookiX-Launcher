import { onBeforeUnmount, ref, type Ref } from "vue";
import {
  SkinViewer,
  IdleAnimation,
  WalkingAnimation,
  RunningAnimation,
  type PlayerAnimation,
} from "skinview3d";

export type AnimationKind = "idle" | "walk" | "run" | "none";
export type ModelKind = "default" | "slim";

export function useSkinRenderer(canvasRef: Ref<HTMLCanvasElement | null>) {
  let viewer: SkinViewer | null = null;
  let ro: ResizeObserver | null = null;
  let currentSrc: string | null = null;
  const animation = ref<AnimationKind>("idle");
  const autoRotate = ref(false);
  const model = ref<ModelKind>("default");

  function init() {
    const canvas = canvasRef.value;
    if (!canvas || viewer) return;
    const w = canvas.clientWidth || 320;
    const h = canvas.clientHeight || 400;
    viewer = new SkinViewer({ canvas, width: w, height: h, zoom: 0.9, fov: 50 });
    applyAnimation();
    ro = new ResizeObserver(() => {
      if (!viewer || !canvas) return;
      const nw = canvas.clientWidth;
      const nh = canvas.clientHeight;
      if (nw && nh) viewer.setSize(nw, nh);
    });
    ro.observe(canvas);
  }

  function applyAnimation() {
    if (!viewer) return;
    let a: PlayerAnimation | null = null;
    if (animation.value === "idle") a = new IdleAnimation();
    else if (animation.value === "walk") a = new WalkingAnimation();
    else if (animation.value === "run") a = new RunningAnimation();
    viewer.animation = a;
    viewer.autoRotate = autoRotate.value;
  }

  async function loadSkinFromSrc(src: string | null, modelOverride?: ModelKind): Promise<boolean> {
    if (!viewer) init();
    if (!viewer) return false;
    currentSrc = src;
    if (!src) {
      viewer.loadSkin(null);
      return true;
    }
    const m = modelOverride ?? model.value;
    model.value = m;
    try {
      // 先清空再带 model 重新加载，强制 skinview3d 重建几何体，
      // 否则同源皮肤会早退、model（纤细/经典胳膊宽度）不会生效。
      viewer.loadSkin(null);
      await viewer.loadSkin(src, { model: m });
      return true;
    } catch {
      return false;
    }
  }

  async function setModel(m: ModelKind) {
    model.value = m;
    if (!viewer || !currentSrc) return;
    try {
      // 同一张皮肤纹理时，skinview3d 的 loadSkin 会因为 source 相同而直接
      // 早退、不重建几何体，于是 model（slim/classic 胳膊宽度）不会变。
      // 先清空再带 model 重新加载，强制走重建路径，预览才会跟着切纤细/经典。
      viewer.loadSkin(null);
      await viewer.loadSkin(currentSrc, { model: m });
    } catch {
      /* ignore */
    }
  }

  function setAnimation(a: AnimationKind) {
    animation.value = a;
    applyAnimation();
  }

  function setAutoRotate(v: boolean) {
    autoRotate.value = v;
    if (viewer) viewer.autoRotate = v;
  }

  async function loadCape(src: string | null): Promise<void> {
    if (!viewer) init();
    if (!viewer) return;
    try {
      if (src) {
        await viewer.loadCape(src);
      } else {
        viewer.loadCape(null);
      }
    } catch {
      /* ignore */
    }
  }

  function resetView() {
    if (!viewer) return;
    viewer.resetCameraPose();
  }

  function dispose() {
    ro?.disconnect();
    ro = null;
    if (viewer && !viewer.disposed) viewer.dispose();
    viewer = null;
  }

  onBeforeUnmount(dispose);

  return { animation, autoRotate, model, loadSkinFromSrc, loadCape, setAnimation, setAutoRotate, setModel, resetView, dispose };
}
