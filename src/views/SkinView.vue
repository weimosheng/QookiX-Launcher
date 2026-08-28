<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { NTabs, NTabPane, NInput, NButton, NSwitch, NModal, useMessage } from "naive-ui";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useSkinRenderer, type AnimationKind } from "../composables/useSkinRenderer";
import { useAccountsStore } from "../stores/accounts";
import SkinThumb from "../components/SkinThumb.vue";
import { BUILTIN_SKINS } from "../assets/builtin-skins";
import {
  IconRefresh,
  IconTrash,
  IconDownload,
  IconPlus,
  IconSearch,
  IconCheck,
  IconShield,
  IconClose,
} from "../components/icons";

const message = useMessage();
const accounts = useAccountsStore();

const canvasRef = ref<HTMLCanvasElement | null>(null);
const renderer = useSkinRenderer(canvasRef);

interface SkinEntry {
  name: string;
  filename: string;
  path: string;
  size: number;
  modified: number;
}

const tab = ref("saved");
const skins = ref<SkinEntry[]>([]);
const skinDataUrls = ref<Record<string, string>>({});
const loadingSkins = ref(false);
const uploading = ref(false);

const playerModalShow = ref(false);
const playerInput = ref("");
const fetchingPlayer = ref(false);
const savingCurrent = ref(false);

const currentSrc = ref<string | null>(null);
const currentName = ref<string>("");
const currentKind = ref<"none" | "local" | "official">("none");

const canSaveCurrent = computed(() => currentSrc.value && currentKind.value !== "local");

const applying = ref(false);
const skinVariant = ref<"classic" | "slim">("classic");
const currentAccount = computed(() => accounts.current);
const isCurrentMs = computed(() => currentAccount.value?.type === "microsoft");

const lastAppliedSrc = ref<string | null>(null);
const appliedToCurrent = computed(() => {
  if (!currentSrc.value || !lastAppliedSrc.value) return false;
  return currentSrc.value === lastAppliedSrc.value;
});

let skinLoadToken = 0;

function detectSkinModel(dataUrl: string): Promise<"classic" | "slim"> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = img.width;
      canvas.height = img.height;
      const ctx = canvas.getContext("2d");
      if (!ctx) return resolve("classic");
      ctx.drawImage(img, 0, 0);
      if (img.width < 64 || img.height < 32) return resolve("classic");
      try {
        for (let y = 20; y < 32; y++) {
          const px = ctx.getImageData(47, y, 1, 1).data;
          if (px[3] > 0) return resolve("classic");
        }
        resolve("slim");
      } catch {
        resolve("classic");
      }
    };
    img.onerror = () => resolve("classic");
    img.src = dataUrl;
  });
}

async function loadCurrentAccountSkin() {
  const token = ++skinLoadToken;
  const acc = accounts.current;
  if (!acc) {
    if (token !== skinLoadToken) return;
    resetCapeList();
    await selectOfficial(BUILTIN_SKINS[0]);
    return;
  }
  if (acc.type === "microsoft") {
    try {
      const res = await api.fetchPlayerSkin(acc.username);
      if (token !== skinLoadToken) return;
      const variant = res.model === "slim" ? "slim" : "classic";
      skinVariant.value = variant;
      renderer.setModel(variant === "slim" ? "slim" : "default");
      await previewSkin(res.data_url, acc.username, "official");
      const capeList: CapeEntry[] = [{ id: "none", name: "无披风", dataUrl: null }];
      try {
        const playerCapes = await api.fetchPlayerCapes(acc.uuid);
        if (token !== skinLoadToken) return;
        for (const c of playerCapes) {
          capeList.push({ id: c.id, name: c.name, dataUrl: c.data_url });
        }
      } catch {
        if (res.cape_data_url) {
          capeList.push({ id: "current", name: "当前披风", dataUrl: res.cape_data_url });
        }
      }
      capes.value = capeList;
      const activeCape = capeList.find((c) => c.id !== "none" && c.dataUrl === res.cape_data_url);
      selectedCapeId.value = activeCape?.id ?? "none";
      if (activeCape?.dataUrl) {
        await renderer.loadCape(activeCape.dataUrl);
      } else {
        renderer.loadCape(null);
      }
      lastAppliedSrc.value = res.data_url;
      return;
    } catch {
      resetCapeList();
      /* 网络不可达，回退 */
    }
  } else {
    resetCapeList();
    const saved = localStorage.getItem(`qookix:offline_skin:${acc.uuid}`);
    if (saved) {
      if (token !== skinLoadToken) return;
      const savedVariant = localStorage.getItem(`qookix:offline_variant:${acc.uuid}`);
      const variant = (savedVariant === "slim" || savedVariant === "classic")
        ? savedVariant
        : await detectSkinModel(saved);
      skinVariant.value = variant;
      renderer.setModel(variant === "slim" ? "slim" : "default");
      await previewSkin(saved, acc.username, "local");
      lastAppliedSrc.value = saved;
      return;
    }
  }
  if (token !== skinLoadToken) return;
  await selectOfficial(BUILTIN_SKINS[0]);
}

watch(
  () => accounts.current?.uuid,
  async (uuid, oldUuid) => {
    if (!uuid || uuid === oldUuid) return;
    await loadCurrentAccountSkin();
  },
);

function setSkinModel(m: "classic" | "slim") {
  skinVariant.value = m;
  renderer.setModel(m === "slim" ? "slim" : "default");
}

const offlineHintShow = ref(false);
const renameModalShow = ref(false);
const capeModalShow = ref(false);
const renameInput = ref("");
let pendingSkinDataUrl = "";

interface CapeEntry {
  id: string;
  name: string;
  dataUrl: string | null;
}
const capes = ref<CapeEntry[]>([{ id: "none", name: "无披风", dataUrl: null }]);
const selectedCapeId = ref<string>("none");

async function selectCape(c: CapeEntry) {
  selectedCapeId.value = c.id;
  if (c.dataUrl) {
    await renderer.loadCape(c.dataUrl);
  } else {
    renderer.loadCape(null);
  }
}

function resetCapeList() {
  capes.value = [{ id: "none", name: "无披风", dataUrl: null }];
  selectedCapeId.value = "none";
  renderer.loadCape(null);
}

async function applySkin() {
  if (!currentSrc.value) {
    message.warning("请先选择皮肤");
    return;
  }
  if (!currentSrc.value.startsWith("data:")) {
    message.error("当前皮肤无法直接应用，请先「保存到本地」后再应用");
    return;
  }
  if (!isCurrentMs.value) {
    applying.value = true;
    try {
      await api.applySkinOffline(currentSrc.value, skinVariant.value, currentAccount.value!.uuid);
      lastAppliedSrc.value = currentSrc.value;
      localStorage.setItem(`qookix:offline_skin:${currentAccount.value!.uuid}`, currentSrc.value);
      localStorage.setItem(`qookix:offline_variant:${currentAccount.value!.uuid}`, skinVariant.value);
      accounts.bumpAvatar();
      offlineHintShow.value = true;
      message.success("皮肤已保存，启动游戏时自动应用");
    } catch (e) {
      message.error(String(e));
    } finally {
      applying.value = false;
    }
    return;
  }
  const uuid = currentAccount.value!.uuid;
  applying.value = true;
  try {
    await api.applySkinToAccount(uuid, currentSrc.value, skinVariant.value);
    const capeId = selectedCapeId.value === "none" ? null : selectedCapeId.value;
    try {
      await api.applyCapeToAccount(uuid, capeId);
    } catch (e) {
      message.warning(`皮肤已应用，但披风应用失败: ${String(e)}`);
    }
    lastAppliedSrc.value = currentSrc.value;
    localStorage.setItem(`qookix:offline_variant:${uuid}`, skinVariant.value);
    accounts.bumpAvatar();
    message.success(`皮肤已应用到 ${currentAccount.value!.username}`);
  } catch (e) {
    message.error(String(e));
  } finally {
    applying.value = false;
  }
}

async function loadSkins() {
  loadingSkins.value = true;
  try {
    skins.value = await api.listSkins();
    for (const s of skins.value) {
      if (!skinDataUrls.value[s.filename]) {
        try {
          skinDataUrls.value[s.filename] = await api.readSkinDataUrl(s.filename);
        } catch {
          /* ignore */
        }
      }
    }
  } catch (e) {
    message.error(String(e));
  } finally {
    loadingSkins.value = false;
  }
}

async function previewSkin(src: string, name: string, kind: "local" | "official") {
  currentSrc.value = src;
  currentName.value = name;
  currentKind.value = kind;
  await renderer.loadSkinFromSrc(src);
}

async function selectLocal(s: SkinEntry) {
  const url = skinDataUrls.value[s.filename];
  if (!url) return;
  await previewSkin(url, s.name, "local");
}

async function selectOfficial(s: (typeof BUILTIN_SKINS)[number]) {
  skinVariant.value = s.model === "slim" ? "slim" : "classic";
  renderer.setModel(s.model === "slim" ? "slim" : "default");
  await previewSkin(s.dataUrl, s.name, "official");
}

async function fetchPlayerAndSave() {
  const name = playerInput.value.trim();
  if (!name) {
    message.warning("请输入玩家名");
    return;
  }
  fetchingPlayer.value = true;
  try {
    const res = await api.fetchPlayerSkin(name);
    const entry = await api.saveSkinFromData(name, res.data_url);
    skinDataUrls.value[entry.filename] = res.data_url;
    await loadSkins();
    await selectLocal(entry);
    playerModalShow.value = false;
    playerInput.value = "";
    message.success(`已保存 ${name} 的皮肤到本地`);
  } catch (e) {
    message.error(String(e));
  } finally {
    fetchingPlayer.value = false;
  }
}

async function uploadSkin() {
  const file = await open({
    multiple: false,
    filters: [{ name: "皮肤 PNG", extensions: ["png"] }],
  });
  if (!file) return;
  uploading.value = true;
  try {
    const dataUrl = await api.readSkinDataUrl(file as string);
    const baseName = (file as string).split(/[\\/]/).pop()!.replace(/\.png$/i, "");
    pendingSkinDataUrl = dataUrl;
    renameInput.value = baseName;
    renameModalShow.value = true;
  } catch (e) {
    message.error(String(e));
  } finally {
    uploading.value = false;
  }
}

async function confirmRename() {
  const name = renameInput.value.trim();
  if (!name) {
    message.warning("请输入皮肤名称");
    return;
  }
  try {
    const entry = await api.saveSkinFromData(name, pendingSkinDataUrl);
    skinDataUrls.value[entry.filename] = pendingSkinDataUrl;
    await loadSkins();
    await selectLocal(entry);
    renameModalShow.value = false;
    message.success("皮肤已上传");
  } catch (e) {
    message.error(String(e));
  }
}

async function saveCurrentToLocal() {
  if (!currentSrc.value || !canSaveCurrent.value) return;
  savingCurrent.value = true;
  try {
    let dataUrl = currentSrc.value;
    if (!dataUrl.startsWith("data:")) {
      const name = currentName.value || "skin";
      const entry = await api.downloadSkinFromUrl(name, dataUrl);
      skinDataUrls.value[entry.filename] = await api.readSkinDataUrl(entry.filename);
      await loadSkins();
      await selectLocal(entry);
      message.success("已保存到本地");
      return;
    }
    const name = currentName.value || "skin";
    const entry = await api.saveSkinFromData(name, dataUrl);
    skinDataUrls.value[entry.filename] = dataUrl;
    await loadSkins();
    await selectLocal(entry);
    message.success("已保存到本地");
  } catch (e) {
    message.error(String(e));
  } finally {
    savingCurrent.value = false;
  }
}

async function deleteSkin(s: SkinEntry) {
  try {
    await api.deleteSkin(s.filename);
    delete skinDataUrls.value[s.filename];
    skins.value = skins.value.filter((x) => x.filename !== s.filename);
    if (currentKind.value === "local" && currentName.value === s.name) {
      currentSrc.value = null;
      currentName.value = "";
      currentKind.value = "none";
      renderer.loadSkinFromSrc(null);
    }
    message.success("已删除");
  } catch (e) {
    message.error(String(e));
  }
}

function resetView() {
  renderer.resetView();
}

function setAnim(a: AnimationKind) {
  renderer.setAnimation(a);
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}

onMounted(async () => {
  await accounts.load();
  await loadSkins();
  await loadCurrentAccountSkin();
});
</script>

<template>
  <div class="skin-view">
    <div class="head">
      <div>
        <h1>皮肤中心</h1>
        <p class="sub">拖动左侧预览可旋转角色，管理本地与官方皮肤</p>
      </div>
    </div>

    <div class="skin-body">
      <section class="preview-pane glass">
        <div class="preview-stage">
          <canvas ref="canvasRef" class="skin-canvas"></canvas>
          <div class="preview-label" :class="{ applied: appliedToCurrent }">
            {{ appliedToCurrent ? "已应用" : "预览" }}
          </div>
          <div class="drag-hint">拖动旋转</div>
        </div>
        <div class="preview-info">
          <div class="info-row">
            <span class="info-label">当前皮肤</span>
            <span class="info-value">{{ currentName || "未选择" }}</span>
          </div>
        </div>
        <div class="anim-row">
          <span class="anim-label">动作</span>
          <div class="seg">
            <button :class="{ active: renderer.animation.value === 'idle' }" @click="setAnim('idle')">站立</button>
            <button :class="{ active: renderer.animation.value === 'walk' }" @click="setAnim('walk')">行走</button>
            <button :class="{ active: renderer.animation.value === 'run' }" @click="setAnim('run')">奔跑</button>
            <button :class="{ active: renderer.animation.value === 'none' }" @click="setAnim('none')">静止</button>
          </div>
        </div>
        <div class="rotate-row">
          <span class="anim-label">自动旋转</span>
          <n-switch :value="renderer.autoRotate.value" @update:value="(v: boolean) => renderer.setAutoRotate(v)" />
        </div>
        <div class="preview-actions">
          <button class="mini-btn" @click="resetView">
            <IconRefresh /> 重置视角
          </button>
          <button class="mini-btn" @click="capeModalShow = true">
            <IconShield /> 披风
          </button>
          <button
            class="mini-btn primary"
            :disabled="!canSaveCurrent || savingCurrent"
            @click="saveCurrentToLocal"
          >
            <IconDownload /> {{ savingCurrent ? "保存中…" : "保存到本地" }}
          </button>
        </div>
        <div class="apply-row">
          <div class="seg">
            <button :class="{ active: skinVariant === 'classic' }" @click="setSkinModel('classic')">经典</button>
            <button :class="{ active: skinVariant === 'slim' }" @click="setSkinModel('slim')">纤细</button>
          </div>
          <button
            class="mini-btn primary apply-btn"
            :disabled="!currentSrc || applying || !currentAccount"
            @click="applySkin"
          >
            <IconCheck /> {{ applying ? "应用中…" : "应用" }}
          </button>
        </div>
      </section>

      <section class="tabs-pane glass">
        <n-tabs v-model:value="tab" type="line" animated class="sk-tabs">
          <n-tab-pane name="saved" tab="已保存皮肤">
            <div class="tab-toolbar">
              <span class="tab-count">共 {{ skins.length }} 个</span>
              <div class="toolbar-right">
                <button class="mini-btn" @click="playerModalShow = true">
                  <IconSearch /> 按玩家名获取
                </button>
                <button class="mini-btn" :disabled="loadingSkins" @click="loadSkins">
                  <IconRefresh /> {{ loadingSkins ? "加载中…" : "刷新" }}
                </button>
              </div>
            </div>

            <div class="skin-grid">
              <button class="skin-card upload-card" @click="uploadSkin">
                <div class="thumb-wrap upload-thumb">
                  <IconPlus />
                </div>
                <div class="skin-meta">
                  <div class="skin-name">上传皮肤</div>
                  <div class="skin-size">选择本地 PNG</div>
                </div>
              </button>
              <div
                v-for="s in skins"
                :key="s.filename"
                class="skin-card"
                :class="{ active: currentKind === 'local' && currentName === s.name }"
                @click="selectLocal(s)"
              >
                <div class="thumb-wrap">
                  <SkinThumb :src="skinDataUrls[s.filename] ?? null" />
                </div>
                <div class="skin-meta">
                  <div class="skin-name text-ellipsis">{{ s.name }}</div>
                  <div class="skin-size">{{ formatSize(s.size) }}</div>
                </div>
                <button class="del-btn" title="删除" @click.stop="deleteSkin(s)">
                  <IconTrash />
                </button>
              </div>
            </div>
          </n-tab-pane>

          <n-tab-pane name="official" tab="官方皮肤">
            <div class="tab-toolbar">
              <span class="tab-count">Minecraft 默认皮肤</span>
            </div>
            <div class="skin-grid">
              <div
                v-for="s in BUILTIN_SKINS"
                :key="s.name"
                class="skin-card"
                :class="{ active: currentKind === 'official' && currentName === s.name }"
                @click="selectOfficial(s)"
              >
                <div class="thumb-wrap">
                  <SkinThumb :src="s.dataUrl" />
                </div>
                <div class="skin-meta">
                  <div class="skin-name text-ellipsis">{{ s.name }}</div>
                  <div class="skin-size">{{ s.model === "slim" ? "纤细" : "经典" }}</div>
                </div>
              </div>
            </div>
          </n-tab-pane>
        </n-tabs>
      </section>
    </div>

    <n-modal v-model:show="playerModalShow" preset="card" title="按正版玩家名获取皮肤" style="max-width: 420px;">
      <div class="modal-body">
        <n-input
          v-model:value="playerInput"
          placeholder="输入 Minecraft 正版玩家名"
          @keyup.enter="fetchPlayerAndSave"
        />
        <div class="modal-actions">
          <n-button @click="playerModalShow = false">取消</n-button>
          <n-button type="primary" :loading="fetchingPlayer" @click="fetchPlayerAndSave">
            <template #icon><IconSearch /></template>
            获取并保存
          </n-button>
        </div>
      </div>
    </n-modal>

    <n-modal v-model:show="offlineHintShow" preset="card" title="离线账号皮肤应用" style="max-width: 420px;">
      <div class="modal-body">
        <p class="offline-hint-text">离线皮肤通过本地资源包应用。Minecraft 1.6–1.19.2 的兼容性最佳；较新版本为尽力兼容。</p>
        <div class="modal-actions">
          <n-button type="primary" @click="offlineHintShow = false">知道了</n-button>
        </div>
      </div>
    </n-modal>

    <Teleport to="body">
      <Transition name="cape">
      <div v-if="capeModalShow" class="cape-overlay" @click.self="capeModalShow = false">
        <div class="cape-dialog glass">
          <div class="cape-dialog-head">
            <span>披风设置</span>
            <button class="cape-close-btn" @click="capeModalShow = false">
              <IconClose />
            </button>
          </div>
          <div class="cape-dialog-body">
            <p class="cape-modal-hint">{{ isCurrentMs ? '选择披风预览，应用时一并装备到账号' : '离线账号不使用披风' }}</p>
            <div class="skin-grid">
              <div
                v-for="c in capes"
                :key="c.id"
                class="skin-card"
                :class="{ active: selectedCapeId === c.id }"
                @click="selectCape(c)"
              >
                <div class="thumb-wrap cape-thumb">
                  <img v-if="c.dataUrl" :src="c.dataUrl" class="cape-img" />
                  <span v-else class="no-cape-icon">无</span>
                </div>
                <div class="skin-meta">
                  <div class="skin-name text-ellipsis">{{ c.name }}</div>
                  <div class="skin-size">{{ c.dataUrl ? '披风' : '不使用' }}</div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
      </Transition>
    </Teleport>

    <n-modal v-model:show="renameModalShow" preset="card" title="命名皮肤" style="max-width: 420px;">
      <div class="modal-body">
        <n-input
          v-model:value="renameInput"
          placeholder="输入皮肤名称"
          @keyup.enter="confirmRename"
        />
        <div class="modal-actions">
          <n-button @click="renameModalShow = false">取消</n-button>
          <n-button type="primary" @click="confirmRename">保存</n-button>
        </div>
      </div>
    </n-modal>
  </div>
</template>

<style scoped>
.skin-view {
  max-width: 1120px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: 18px;
}
.head h1 {
  margin: 0 0 4px;
  font-size: 24px;
}
.sub {
  margin: 0;
  color: var(--text-3);
  font-size: 13px;
}
.skin-body {
  display: grid;
  grid-template-columns: 360px 1fr;
  gap: 16px;
  min-height: 540px;
}
.preview-pane {
  display: flex;
  flex-direction: column;
  padding: 16px;
  gap: 14px;
}
.preview-stage {
  position: relative;
  flex: 1;
  min-height: 320px;
  border-radius: 12px;
  background:
    radial-gradient(ellipse at center, rgba(232, 154, 75, 0.08), transparent 70%),
    linear-gradient(180deg, rgba(255, 255, 255, 0.03), rgba(0, 0, 0, 0.12));
  border: 1px solid var(--border);
  overflow: hidden;
  display: flex;
  align-items: center;
  justify-content: center;
}
.skin-canvas {
  width: 100%;
  height: 100%;
  cursor: grab;
  touch-action: none;
}
.skin-canvas:active {
  cursor: grabbing;
}
.drag-hint {
  position: absolute;
  bottom: 10px;
  left: 50%;
  transform: translateX(-50%);
  font-size: 11px;
  color: var(--text-3);
  background: rgba(0, 0, 0, 0.3);
  padding: 3px 10px;
  border-radius: 8px;
  pointer-events: none;
  backdrop-filter: blur(4px);
}
.not-account-hint,
.preview-label {
  position: absolute;
  top: 10px;
  left: 10px;
  font-size: 11px;
  color: var(--text-2);
  background: rgba(0, 0, 0, 0.4);
  padding: 3px 10px;
  border-radius: 8px;
  pointer-events: none;
  backdrop-filter: blur(4px);
  white-space: nowrap;
  font-weight: 600;
}
.preview-label.applied {
  color: #6fcf6f;
  background: rgba(60, 180, 60, 0.2);
}
.preview-info {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid var(--border);
}
.info-row {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
}
.info-label {
  color: var(--text-3);
}
.info-value {
  color: var(--text-1);
  font-weight: 600;
}
.preview-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.cape-thumb {
  object-fit: contain;
  padding: 8px;
}
.cape-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  image-rendering: pixelated;
}
.no-cape-icon {
  font-size: 18px;
  font-weight: 700;
  color: var(--text-3);
}
.cape-modal-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.cape-overlay {
  position: fixed;
  inset: 0;
  z-index: 2000;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  backdrop-filter: blur(2px);
}
.cape-enter-active,
.cape-leave-active {
  transition: opacity 0.2s ease;
}
.cape-enter-active .cape-dialog,
.cape-leave-active .cape-dialog {
  transition: transform 0.2s ease, opacity 0.2s ease;
}
.cape-enter-from,
.cape-leave-to {
  opacity: 0;
}
.cape-enter-from .cape-dialog,
.cape-leave-to .cape-dialog {
  transform: scale(0.94);
  opacity: 0;
}
.cape-dialog {
  width: 520px;
  max-width: 90vw;
  max-height: 80vh;
  border-radius: 12px;
  border: 1px solid var(--border);
  background: var(--bg-2);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.cape-dialog-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 18px;
  font-size: 16px;
  font-weight: 700;
  color: var(--text-1);
  border-bottom: 1px solid var(--border);
}
.cape-close-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 8px;
  border: none;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition: all 0.14s;
}
.cape-close-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-1);
}
.cape-dialog-body {
  padding: 18px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow-y: auto;
}
.cape-modal-hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-3);
}
.apply-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  padding-top: 4px;
  border-top: 1px solid var(--border);
  margin-top: 4px;
}
.apply-btn {
  margin-left: auto;
}
.hint {
  font-size: 11px;
  color: var(--text-3);
  margin: 6px 0 0;
}
.anim-row,
.rotate-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  font-size: 12px;
  color: var(--text-3);
}
.seg {
  display: flex;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 8px;
  padding: 3px;
  gap: 2px;
}
.seg button {
  border: none;
  background: transparent;
  color: var(--text-3);
  padding: 5px 10px;
  border-radius: 6px;
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: color 0.12s, background 0.12s;
}
.seg button:hover {
  color: var(--text-1);
}
.seg button.active {
  background: var(--accent-soft);
  color: var(--accent);
}
.anim-label {
  white-space: nowrap;
}
.mini-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  border-radius: 9px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  white-space: nowrap;
  transition: all 0.14s;
}
.mini-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-1);
}
.mini-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.mini-btn.primary {
  background: linear-gradient(135deg, var(--accent), var(--accent-deep));
  color: #1a1208;
  border: none;
}
.mini-btn.primary:hover:not(:disabled) {
  filter: brightness(1.08);
}
.tabs-pane {
  padding: 16px 18px;
  min-height: 540px;
  overflow: hidden;
}
.sk-tabs {
  height: 100%;
}
.tab-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
  gap: 8px;
}
.toolbar-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.tab-count {
  font-size: 12px;
  color: var(--text-3);
  white-space: nowrap;
}
.skin-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(108px, 1fr));
  gap: 12px;
}
.skin-card {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px;
  border-radius: 11px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.03);
  cursor: pointer;
  transition: all 0.14s;
  font-family: inherit;
  text-align: left;
}
.skin-card:hover {
  background: rgba(255, 255, 255, 0.07);
  border-color: rgba(232, 154, 75, 0.35);
  transform: translateY(-1px);
}
.skin-card:active {
  transform: scale(0.97);
}
.skin-card.active {
  border-color: var(--accent);
  background: var(--accent-soft);
  box-shadow: 0 0 0 1px var(--accent);
}
.upload-card {
  border-style: dashed;
  color: var(--text-3);
}
.upload-card:hover {
  color: var(--text-1);
  border-color: rgba(232, 154, 75, 0.45);
}
.thumb-wrap {
  width: 100%;
  aspect-ratio: 1 / 1;
  border-radius: 8px;
  overflow: hidden;
  background: rgba(0, 0, 0, 0.2);
  display: flex;
  align-items: center;
  justify-content: center;
  image-rendering: pixelated;
}
.upload-thumb {
  color: var(--text-3);
  font-size: 22px;
}
.upload-card:hover .upload-thumb {
  color: var(--accent);
}
.skin-meta {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.skin-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-1);
}
.skin-size {
  font-size: 11px;
  color: var(--text-3);
}
.del-btn {
  position: absolute;
  top: 6px;
  right: 6px;
  width: 22px;
  height: 22px;
  border-radius: 6px;
  border: none;
  background: rgba(0, 0, 0, 0.45);
  color: #f0907f;
  font-size: 12px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  transition: opacity 0.14s;
}
.skin-card:hover .del-btn {
  opacity: 1;
}
.del-btn:hover {
  background: rgba(229, 83, 75, 0.3);
}
.modal-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.offline-hint-text {
  margin: 0;
  font-size: 13px;
  line-height: 1.6;
  color: var(--text-2);
}
</style>
