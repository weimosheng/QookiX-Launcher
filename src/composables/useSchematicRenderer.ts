import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { PointerLockControls } from "three/addons/controls/PointerLockControls.js";
import type { MeshData } from "./schematicMeshWorker";

const CHUNK_VOLUME = 16 * 16 * 16;

export type ViewMode = "orbit" | "walk";

export interface SchematicRenderer {
  dispose: () => void;
  resize: (width: number, height: number) => void;
  setChunkFromMeshData: (chunkPos: [number, number, number], opaque: MeshData, translucent: MeshData, texture: THREE.Texture) => void;
  removeChunk: (chunkPos: [number, number, number]) => void;
  clearChunks: () => void;
  fitCamera: (min: [number, number, number], max: [number, number, number]) => void;
  resetCamera: () => void;
  setLayerSlice: (y: number | null) => void;
  setViewMode: (mode: ViewMode) => void;
  getViewMode: () => ViewMode;
  getWalkSpeed: () => number;
  isLocked: () => boolean;
  onLockChange: (cb: (locked: boolean) => void) => void;
  onSpeedChange: (cb: (speed: number) => void) => void;
  requestRender: () => void;
}

export function createSchematicRenderer(canvas: HTMLCanvasElement): SchematicRenderer {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x1a1a2e);

  const orbitCamera = new THREE.OrthographicCamera(-100, 100, 100, -100, -2000, 2000);
  const walkCamera = new THREE.PerspectiveCamera(75, 1, 0.1, 5000);
  let activeCamera: THREE.Camera = orbitCamera;
  let viewMode: ViewMode = "orbit";

  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, logarithmicDepthBuffer: true, powerPreference: "high-performance" });
  renderer.setPixelRatio(Math.min(Math.max(window.devicePixelRatio, 1.5), 2));
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.3;
  renderer.localClippingEnabled = true;

  const onContextLost = (e: Event) => { e.preventDefault(); };
  const onContextRestored = () => { requestRender(); };
  canvas.addEventListener("webglcontextlost", onContextLost, false);
  canvas.addEventListener("webglcontextrestored", onContextRestored, false);

  const orbitControls = new OrbitControls(orbitCamera, canvas);
  orbitControls.enableDamping = true;
  orbitControls.dampingFactor = 0.08;
  orbitControls.minZoom = 0.05;
  orbitControls.maxZoom = 20;
  const walkControls = new PointerLockControls(walkCamera, canvas);

  const ambient = new THREE.AmbientLight(0xffffff, 0.8);
  scene.add(ambient);
  const hemi = new THREE.HemisphereLight(0x87ceeb, 0x444444, 0.6);
  scene.add(hemi);
  const keyLight = new THREE.DirectionalLight(0xfffbf0, 1.2);
  keyLight.position.set(-1, 1.5, 0.5);
  scene.add(keyLight);
  const fillLight = new THREE.DirectionalLight(0xffffff, 0.4);
  fillLight.position.set(1, 0.5, -1);
  scene.add(fillLight);

  const chunkGroup = new THREE.Group();
  scene.add(chunkGroup);

  const boundsBox = new THREE.LineSegments(
    new THREE.EdgesGeometry(new THREE.BoxGeometry(1, 1, 1)),
    new THREE.LineBasicMaterial({ color: 0x96b5e1, transparent: true, opacity: 0.25 }),
  );
  boundsBox.visible = false;
  scene.add(boundsBox);

  const chunkMeshes = new Map<string, THREE.Mesh[]>();
  const materials = new Map<THREE.Texture, { opaque: THREE.MeshLambertMaterial; translucent: THREE.MeshLambertMaterial }>();

  let walkSpeed = 8;
  let lastWalkTime = 0;
  const pressedKeys = new Set<string>();
  let lockCallback: ((locked: boolean) => void) | null = null;
  let speedCallback: ((speed: number) => void) | null = null;

  function chunkKey(pos: [number, number, number]): string {
    return `${pos[0]},${pos[1]},${pos[2]}`;
  }

  function ensureMaterials(texture: THREE.Texture) {
    let mats = materials.get(texture);
    if (!mats) {
      mats = {
        opaque: new THREE.MeshLambertMaterial({
          map: texture, vertexColors: true, side: THREE.FrontSide, alphaTest: 0.5,
        }),
        translucent: new THREE.MeshLambertMaterial({
          map: texture, vertexColors: true, side: THREE.FrontSide,
          alphaTest: 0.1, transparent: true, opacity: 0.85, depthWrite: false,
        }),
      };
      materials.set(texture, mats);
    }
    return mats;
  }

  function createMeshFromData(data: MeshData, material: THREE.Material): THREE.Mesh {
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(data.positions, 3));
    geo.setAttribute("normal", new THREE.BufferAttribute(data.normals, 3));
    geo.setAttribute("uv", new THREE.BufferAttribute(data.uvs, 2));
    geo.setAttribute("color", new THREE.BufferAttribute(data.colors, 3));
    const mesh = new THREE.Mesh(geo, material);
    mesh.frustumCulled = true;
    return mesh;
  }

  function setChunkFromMeshData(
    chunkPos: [number, number, number],
    opaque: MeshData,
    translucent: MeshData,
    texture: THREE.Texture,
  ) {
    removeChunk(chunkPos);
    const mats = ensureMaterials(texture);
    const meshes: THREE.Mesh[] = [];

    if (opaque.positions.length > 0) {
      const mesh = createMeshFromData(opaque, mats.opaque);
      chunkGroup.add(mesh);
      meshes.push(mesh);
    }
    if (translucent.positions.length > 0) {
      const mesh = createMeshFromData(translucent, mats.translucent);
      chunkGroup.add(mesh);
      meshes.push(mesh);
    }
    if (meshes.length > 0) {
      chunkMeshes.set(chunkKey(chunkPos), meshes);
    }
  }

  function removeChunk(chunkPos: [number, number, number]) {
    const key = chunkKey(chunkPos);
    const meshes = chunkMeshes.get(key);
    if (meshes) {
      for (const mesh of meshes) {
        chunkGroup.remove(mesh);
        mesh.geometry.dispose();
      }
      chunkMeshes.delete(key);
    }
  }

  function clearChunks() {
    for (const key of chunkMeshes.keys()) {
      const [x, y, z] = key.split(",").map(Number);
      removeChunk([x, y, z]);
    }
  }

  let savedBounds: { min: [number, number, number]; max: [number, number, number] } | null = null;

  function fitCamera(min: [number, number, number], max: [number, number, number]) {
    savedBounds = { min, max };
    const cx = (min[0] + max[0] + 1) / 2;
    const cy = (min[1] + max[1] + 1) / 2;
    const cz = (min[2] + max[2] + 1) / 2;
    const dx = max[0] - min[0] + 1;
    const dy = max[1] - min[1] + 1;
    const dz = max[2] - min[2] + 1;
    const maxDim = Math.max(dx, dy, dz);
    const dist = maxDim * 1.8;

    boundsBox.scale.set(dx, dy, dz);
    boundsBox.position.set(cx, cy, cz);
    boundsBox.visible = true;

    orbitCamera.position.set(cx + dist * 0.7, cy + dist * 0.8, cz + dist * 0.7);
    orbitCamera.lookAt(cx, cy, cz);
    orbitControls.target.set(cx, cy, cz);

    const aspect = canvas.width / canvas.height;
    const halfH = maxDim * 0.6;
    const halfW = halfH * aspect;
    orbitCamera.left = -halfW;
    orbitCamera.right = halfW;
    orbitCamera.top = halfH;
    orbitCamera.bottom = -halfH;
    orbitCamera.near = -maxDim * 3;
    orbitCamera.far = maxDim * 3;
    orbitCamera.zoom = 1;
    orbitCamera.updateProjectionMatrix();
    orbitControls.update();
  }

  function resetCamera() {
    if (!savedBounds || transitioning) return;
    if (viewMode === "walk") {
      if (walkControls.isLocked) walkControls.unlock();
      viewMode = "orbit";
      activeCamera = orbitCamera;
      orbitControls.enabled = true;
    }
    fitCamera(savedBounds.min, savedBounds.max);
    requestRender();
  }

  function setLayerSlice(y: number | null) {
    const planes = y !== null
      ? [new THREE.Plane(new THREE.Vector3(0, -1, 0), y + 1)]
      : [];
    for (const mats of materials.values()) {
      mats.opaque.clippingPlanes = planes;
      mats.translucent.clippingPlanes = planes;
      mats.opaque.needsUpdate = true;
      mats.translucent.needsUpdate = true;
    }
  }

  let transitioning = false;

  function easeInOutCubic(t: number): number {
    return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
  }

  function setViewMode(mode: ViewMode) {
    if (mode === viewMode || transitioning) return;
    const duration = 600;
    const startPos = new THREE.Vector3();
    const startQuat = new THREE.Quaternion();
    const endPos = new THREE.Vector3();
    const endQuat = new THREE.Quaternion();

    if (mode === "walk") {
      const dir = new THREE.Vector3().subVectors(orbitCamera.position, orbitControls.target).normalize();
      const dist = Math.max(6, orbitCamera.position.distanceTo(orbitControls.target) * 0.12);
      startPos.copy(orbitCamera.position);
      startQuat.copy(orbitCamera.quaternion);
      endPos.copy(orbitControls.target).addScaledVector(dir, dist);
      endQuat.copy(startQuat);
      const aspect = canvas.width / canvas.height;
      walkCamera.aspect = aspect;
      walkCamera.position.copy(startPos);
      walkCamera.quaternion.copy(startQuat);
      activeCamera = walkCamera;
      orbitControls.enabled = false;
      const orbitHalfH = orbitCamera.top / orbitCamera.zoom;
      const orbitDist = Math.max(0.1, orbitCamera.position.distanceTo(orbitControls.target));
      const startFov = Math.min(75, 2 * Math.atan(orbitHalfH / orbitDist) * 180 / Math.PI);
      const endFov = 75;
      transitioning = true;
      const startTime = performance.now();
      const animate = () => {
        if (disposed || !transitioning) return;
        const t = Math.min((performance.now() - startTime) / duration, 1);
        const e = easeInOutCubic(t);
        walkCamera.position.lerpVectors(startPos, endPos, e);
        walkCamera.quaternion.copy(startQuat);
        walkCamera.fov = startFov * (1 - e) + endFov * e;
        walkCamera.updateProjectionMatrix();
        renderer.render(scene, activeCamera);
        if (t < 1) requestAnimationFrame(animate);
        else { transitioning = false; viewMode = "walk"; walkControls.lock(); }
      };
      requestAnimationFrame(animate);
    } else {
      if (walkControls.isLocked) walkControls.unlock();
      const walkDir = new THREE.Vector3();
      walkCamera.getWorldDirection(walkDir);
      const targetDist = Math.max(20, orbitCamera.position.distanceTo(orbitControls.target));
      endPos.copy(walkCamera.position).addScaledVector(walkDir, -targetDist * 0.5);
      endPos.y += targetDist * 0.3;
      const lookTarget = walkCamera.position.clone().addScaledVector(walkDir, 10);
      startPos.copy(walkCamera.position);
      startQuat.copy(walkCamera.quaternion);
      const tmpCam = new THREE.PerspectiveCamera();
      tmpCam.position.copy(endPos);
      tmpCam.lookAt(lookTarget);
      endQuat.copy(tmpCam.quaternion);
      const aspect = canvas.width / canvas.height;
      const walkFovRad = THREE.MathUtils.degToRad(walkCamera.fov);
      const startHalfH = startPos.distanceTo(lookTarget) * Math.tan(walkFovRad / 2);
      const endHalfH = orbitCamera.top / orbitCamera.zoom;
      orbitCamera.position.copy(startPos);
      orbitCamera.quaternion.copy(startQuat);
      activeCamera = orbitCamera;
      transitioning = true;
      const startTime = performance.now();
      const animate = () => {
        if (disposed || !transitioning) return;
        const t = Math.min((performance.now() - startTime) / duration, 1);
        const e = easeInOutCubic(t);
        orbitCamera.position.lerpVectors(startPos, endPos, e);
        orbitCamera.quaternion.slerpQuaternions(startQuat, endQuat, e);
        const halfH = startHalfH * (1 - e) + endHalfH * e;
        orbitCamera.left = -halfH * aspect;
        orbitCamera.right = halfH * aspect;
        orbitCamera.top = halfH;
        orbitCamera.bottom = -halfH;
        orbitCamera.zoom = 1;
        orbitCamera.updateProjectionMatrix();
        renderer.render(scene, activeCamera);
        if (t < 1) requestAnimationFrame(animate);
        else {
          transitioning = false;
          viewMode = "orbit";
          orbitControls.target.copy(lookTarget);
          orbitControls.enabled = true;
          orbitControls.update();
          requestRender();
        }
      };
      requestAnimationFrame(animate);
    }
  }

  function resize(width: number, height: number) {
    renderer.setSize(width, height, false);
    const aspect = width / height;
    const halfH = (orbitCamera.top - orbitCamera.bottom) / 2;
    const halfW = halfH * aspect;
    orbitCamera.left = -halfW;
    orbitCamera.right = halfW;
    orbitCamera.updateProjectionMatrix();
    walkCamera.aspect = aspect;
    walkCamera.updateProjectionMatrix();
  }

  const onKeyDown = (e: KeyboardEvent) => {
    if (viewMode !== "walk" || transitioning) return;
    if (["KeyW", "KeyA", "KeyS", "KeyD", "Space", "ShiftLeft", "ShiftRight"].includes(e.code)) {
      e.preventDefault();
      const wasIdle = pressedKeys.size === 0;
      pressedKeys.add(e.code);
      if (wasIdle) lastWalkTime = performance.now();
      requestRender();
    }
  };
  const onKeyUp = (e: KeyboardEvent) => { pressedKeys.delete(e.code); };
  const onWheel = (e: WheelEvent) => {
    if (viewMode !== "walk" || !walkControls.isLocked || transitioning) return;
    e.preventDefault();
    const multiplier = e.deltaY > 0 ? 0.85 : 1 / 0.85;
    walkSpeed = Math.round(Math.max(4, Math.min(60, walkSpeed * multiplier)));
    speedCallback?.(walkSpeed);
  };
  const onCanvasClick = () => {
    if (viewMode === "walk" && !walkControls.isLocked && !transitioning) walkControls.lock();
  };
  const onLock = () => { lastWalkTime = performance.now(); lockCallback?.(true); requestRender(); };
  const onUnlock = () => { pressedKeys.clear(); lockCallback?.(false); };

  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("keyup", onKeyUp);
  canvas.addEventListener("wheel", onWheel, { passive: false });
  canvas.addEventListener("click", onCanvasClick);
  walkControls.addEventListener("lock", onLock);
  walkControls.addEventListener("unlock", onUnlock);

  let renderRequested = false;
  let disposed = false;

  function requestRender() {
    if (renderRequested || disposed || transitioning) return;
    renderRequested = true;
    requestAnimationFrame(() => {
      renderRequested = false;
      render();
    });
  }

  function render() {
    if (disposed) return;
    if (viewMode === "walk" && walkControls.isLocked) {
      const now = performance.now();
      const delta = Math.min((now - lastWalkTime) / 1000, 0.1);
      lastWalkTime = now;
      const fwd = (pressedKeys.has("KeyW") ? 1 : 0) - (pressedKeys.has("KeyS") ? 1 : 0);
      const right = (pressedKeys.has("KeyD") ? 1 : 0) - (pressedKeys.has("KeyA") ? 1 : 0);
      const vert = (pressedKeys.has("Space") ? 1 : 0) - ((pressedKeys.has("ShiftLeft") || pressedKeys.has("ShiftRight")) ? 1 : 0);
      if (fwd !== 0) walkControls.moveForward(walkSpeed * fwd * delta);
      if (right !== 0) walkControls.moveRight(walkSpeed * right * delta);
      if (vert !== 0) walkCamera.position.y += walkSpeed * vert * delta;
      if (fwd !== 0 || right !== 0 || vert !== 0) requestRender();
    } else {
      lastWalkTime = performance.now();
    }
    if (viewMode === "orbit") {
      if (orbitControls.update()) requestRender();
    }
    renderer.render(scene, activeCamera);
  }

  orbitControls.addEventListener("change", () => requestRender());
  walkControls.addEventListener("change", () => requestRender());

  function dispose() {
    disposed = true;
    clearChunks();
    boundsBox.geometry.dispose();
    (boundsBox.material as THREE.Material).dispose();
    for (const mats of materials.values()) {
      mats.opaque.dispose();
      mats.translucent.dispose();
    }
    materials.clear();
    window.removeEventListener("keydown", onKeyDown);
    window.removeEventListener("keyup", onKeyUp);
    canvas.removeEventListener("wheel", onWheel);
    canvas.removeEventListener("click", onCanvasClick);
    walkControls.removeEventListener("lock", onLock);
    walkControls.removeEventListener("unlock", onUnlock);
    canvas.removeEventListener("webglcontextlost", onContextLost, false);
    canvas.removeEventListener("webglcontextrestored", onContextRestored, false);
    walkControls.dispose();
    renderer.dispose();
    renderer.forceContextLoss();
    orbitControls.dispose();
  }

  return {
    dispose, resize, setChunkFromMeshData, removeChunk, clearChunks, fitCamera, resetCamera, setLayerSlice,
    setViewMode, getViewMode: () => viewMode, getWalkSpeed: () => walkSpeed,
    isLocked: () => walkControls.isLocked, onLockChange: (cb) => { lockCallback = cb; }, onSpeedChange: (cb) => { speedCallback = cb; }, requestRender,
  };
}

export function parseChunkData(buffer: ArrayBuffer): Uint32Array {
  const view = new DataView(buffer);
  const magic = String.fromCharCode(view.getUint8(0), view.getUint8(1), view.getUint8(2), view.getUint8(3));
  if (magic !== "SPC1") {
    throw new Error(`Invalid chunk data magic: ${magic}`);
  }
  const volume = view.getUint32(4, true);
  if (volume !== CHUNK_VOLUME) {
    throw new Error(`Unexpected chunk volume: ${volume}`);
  }
  const blocks = new Uint32Array(CHUNK_VOLUME);
  for (let i = 0; i < CHUNK_VOLUME; i++) {
    blocks[i] = view.getUint32(8 + i * 4, true);
  }
  return blocks;
}
