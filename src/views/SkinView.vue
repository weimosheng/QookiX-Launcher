<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { NTabs, NTabPane, NInput, NButton, NSwitch, NModal, useMessage } from "naive-ui";
import { useI18n } from "vue-i18n";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { fmtSize as formatSize } from "../utils/format";
import { useSkinRenderer, type AnimationKind } from "../composables/useSkinRenderer";
import { loadOfflineSkin, saveOfflineSkinCache } from "../composables/useOfflineSkin";
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

const { t } = useI18n();
const message = useMessage();
const accounts = useAccountsStore();

const canvasRef = ref<HTMLCanvasElement | null>(null);
const renderer = useSkinRenderer(canvasRef);

interface SkinEntry {
  id: string;
  name: string;
  filename: string;
  path: string;
  size: number;
  modified: number;
}

const tab = ref("saved");
const skins = ref<SkinEntry[]>([]);
const skinDataUrls = ref<Record<string, string>>({});
/** 每张已保存皮肤的模型（classic/slim），供缩略图与切换时直接取用，避免重复检测 */
const skinModels = ref<Record<string, "classic" | "slim">>({});
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
      const ctx = canvas.getContext("2d", { willReadFrequently: true });
      if (!ctx) return resolve("classic");
      ctx.drawImage(img, 0, 0);
      if (img.width < 64 || img.height < 32) return resolve("classic");
      try {
        // 右臂背面最右两列：classic 4px 宽（x=52-55），slim 3px 宽（x=52-54），
        // 故 x=54/55 在 classic 有像素、slim 为空。任一不透明即判定 classic。
        for (let y = 20; y < 32; y++) {
          if (ctx.getImageData(54, y, 1, 1).data[3] > 0) return resolve("classic");
          if (ctx.getImageData(55, y, 1, 1).data[3] > 0) return resolve("classic");
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

// —— 手动选择的皮肤模型（经典/纤细）持久化 ——
// skinVariant 本身只是组件内的 ref，离开页面组件销毁后就会丢失，
// 回来时 loadCurrentAccountSkin 又会按皮肤自身的模型重置回去，
// 导致「选了纤细、切走再回来又变经典」。这里把用户的手动选择存到
// localStorage，按账号维度记住；换皮肤或换账号时清除，避免错误套用。
function variantOverrideKey(uuid: string | undefined) {
  return `qookix:skin_variant_override:${uuid ?? "anon"}`;
}
function readVariantOverride(): "classic" | "slim" | null {
  const uuid = accounts.current?.uuid;
  const v = localStorage.getItem(variantOverrideKey(uuid));
  return v === "slim" || v === "classic" ? v : null;
}
function writeVariantOverride(m: "classic" | "slim") {
  localStorage.setItem(variantOverrideKey(accounts.current?.uuid), m);
}
function clearVariantOverride() {
  localStorage.removeItem(variantOverrideKey(accounts.current?.uuid));
}
/** 应用持久化的手动选择（在自动加载完当前皮肤后调用） */
function applyVariantOverride() {
  const m = readVariantOverride();
  if (!m) return;
  skinVariant.value = m;
  renderer.setModel(m === "slim" ? "slim" : "default");
}

/**
 * 判断某张皮肤是否为「当前正在预览的皮肤」。
 * 必须比对皮肤内容（dataURL）而不是名字：离线账号自动加载时
 * currentName 是账号昵称，而列表里的 s.name 是皮肤保存名（如正版玩家名），
 * 两者往往不同，用名字比对会导致缩略图/高亮永远匹配不上。
 */
function isCurrentSkin(src: string | undefined | null): boolean {
  return !!src && src === currentSrc.value;
}

// —— Microsoft 账号皮肤/披风缓存 ——
// 每次打开皮肤页都会向 Mojang 服务器请求一次当前正版皮肤 + 披风列表，
// 而这些数据短期内几乎不会变。这里用 localStorage 做带 TTL 的本地缓存，
// 命中且未过期时直接用缓存，不再发请求，避免每次进页面都联网。
// 缓存内容较大（皮肤是 base64 PNG），所以 TTL 设 10 分钟，并允许手动刷新。
const MS_SKIN_TTL = 10 * 60 * 1000;
interface MsSkinCache {
  data_url: string;
  model: string;
  cape_data_url: string | null;
  capes: { id: string; name: string; data_url: string; active: boolean }[];
  ts: number;
}
function msSkinCacheKey(uuid: string) {
  return `qookix:ms_skin_cache:${uuid}`;
}
function readMsSkinCache(uuid: string): MsSkinCache | null {
  const raw = localStorage.getItem(msSkinCacheKey(uuid));
  if (!raw) return null;
  try {
    const c = JSON.parse(raw) as MsSkinCache;
    if (Date.now() - c.ts > MS_SKIN_TTL) return null; // 过期
    return c;
  } catch {
    return null;
  }
}
function writeMsSkinCache(uuid: string, c: MsSkinCache) {
  localStorage.setItem(msSkinCacheKey(uuid), JSON.stringify({ ...c, ts: Date.now() }));
}

/** 用一份 Microsoft 皮肤数据渲染预览 + 披风列表（缓存命中与网络拉取共用） */
async function applyMsSkin(c: MsSkinCache, username: string) {
  const variant = c.model === "slim" ? "slim" : "classic";
  await previewSkin(c.data_url, username, "official", variant);
  const capeList: CapeEntry[] = [{ id: "none", name: t("skin.capeNone"), dataUrl: null }];
  for (const cc of c.capes) {
    capeList.push({ id: cc.id, name: cc.name, dataUrl: cc.data_url });
  }
  if (!c.capes.length && c.cape_data_url) {
        capeList.push({ id: "current", name: t("skin.capeCurrent"), dataUrl: c.cape_data_url });
  }
  capes.value = capeList;
  const activeCape = capeList.find((x) => x.id !== "none" && x.dataUrl === c.cape_data_url);
  selectedCapeId.value = activeCape?.id ?? "none";
  if (activeCape?.dataUrl) {
    await renderer.loadCape(activeCape.dataUrl);
  } else {
    renderer.loadCape(null);
  }
  lastAppliedSrc.value = c.data_url;
  // 保底高亮：把获取的正版皮肤落到本地列表，使其在「已保存皮肤」中高亮；
  // 本地已有相同内容则跳过，换皮肤后会新增一张，以此类推。
  void ensureSkinSavedLocally(c.data_url, username);
}

async function loadCurrentAccountSkin(force = false) {
  const token = ++skinLoadToken;
  const acc = accounts.current;
  if (!acc) {
    if (token !== skinLoadToken) return;
    resetCapeList();
    await selectOfficial(BUILTIN_SKINS[0]);
    return;
  }
  if (acc.type === "microsoft") {
    const cached = !force ? readMsSkinCache(acc.uuid) : null;
    if (cached) {
      // 命中有效缓存：直接渲染，不发任何网络请求
      if (token !== skinLoadToken) return;
      await applyMsSkin(cached, acc.username);
      return;
    }
    try {
      const res = await api.fetchPlayerSkin(acc.username);
      if (token !== skinLoadToken) return;
      let playerCapes: { id: string; name: string; data_url: string; active: boolean }[] = [];
      try {
        playerCapes = await api.fetchPlayerCapes(acc.uuid);
        if (token !== skinLoadToken) return;
      } catch {
        /* 披风拉取失败不致命，下面用 res.cape_data_url 兜底 */
      }
      const payload: MsSkinCache = {
        data_url: res.data_url,
        model: res.model,
        cape_data_url: res.cape_data_url,
        capes: playerCapes,
        ts: Date.now(),
      };
      writeMsSkinCache(acc.uuid, payload);
      await applyMsSkin(payload, acc.username);
      return;
    } catch {
      // 网络不可达：若有旧缓存（即使过期）也先用着，不让页面空白
      const stale = localStorage.getItem(msSkinCacheKey(acc.uuid));
      if (stale) {
        try {
          const c = JSON.parse(stale) as MsSkinCache;
          if (token !== skinLoadToken) return;
          await applyMsSkin(c, acc.username);
          return;
        } catch {
          /* ignore */
        }
      }
      resetCapeList();
      /* 网络不可达，回退 */
    }
  }
  if (acc.type === "yggdrasil") {
    // 皮肤站账号：皮肤由皮肤站管理，这里只读展示（换肤请去皮肤站网站）
    resetCapeList();
    try {
      const tex = await api.yggdrasilTextures(acc.server, acc.uuid);
      if (token !== skinLoadToken) return;
      if (tex?.skin) {
        const variant = await detectSkinModel(tex.skin);
        await previewSkin(tex.skin, acc.username, "official", variant);
        lastAppliedSrc.value = tex.skin;
        void ensureSkinSavedLocally(tex.skin, acc.username);
        if (tex.cape) {
          capes.value = [
            { id: "none", name: t("skin.capeNone"), dataUrl: null },
            { id: "station", name: t("skin.capeStation"), dataUrl: tex.cape },
          ];
          selectedCapeId.value = "station";
          renderer.loadCape(tex.cape);
        }
        return;
      }
      // 该角色在皮肤站还没上传过皮肤：保持默认史蒂夫即可
    } catch {
      /* 皮肤站不可达，回退默认皮肤 */
    }
    await selectOfficial(BUILTIN_SKINS[0]);
    return;
  }
  {
    resetCapeList();
    const saved = await loadOfflineSkin(acc.uuid);
    if (saved) {
      if (token !== skinLoadToken) return;
      const variant = saved.variant ?? (await detectSkinModel(saved.src));
      await previewSkin(saved.src, acc.username, "local", variant);
      lastAppliedSrc.value = saved.src;
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
    // 换了账号：之前的选择不该套用到新账号的皮肤上
    clearVariantOverride();
    await loadCurrentAccountSkin();
  },
);

function setSkinModel(m: "classic" | "slim") {
  skinVariant.value = m;
  renderer.setModel(m === "slim" ? "slim" : "default");
  // 记住用户的手动选择，切换页面/重启后仍然生效
  writeVariantOverride(m);
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
const capes = ref<CapeEntry[]>([{ id: "none", name: t("skin.capeNone"), dataUrl: null }]);
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
  capes.value = [{ id: "none", name: t("skin.capeNone"), dataUrl: null }];
  selectedCapeId.value = "none";
  renderer.loadCape(null);
}

async function applySkin() {
  if (!currentSrc.value) {
    message.warning(t("skin.warnSelectSkin"));
    return;
  }
  if (!currentSrc.value.startsWith("data:")) {
    message.error(t("skin.errCannotApply"));
    return;
  }
  if (currentAccount.value!.type === "yggdrasil") {
    message.info(t("skin.infoStationManaged"));
    return;
  }
  if (!isCurrentMs.value) {
    applying.value = true;
    try {
      await api.applySkinOffline(currentSrc.value, skinVariant.value, currentAccount.value!.uuid);
      lastAppliedSrc.value = currentSrc.value;
      // 应用时同样记住手动选择，切页面回来不会被缓存的 variant 覆盖
      writeVariantOverride(skinVariant.value);
      saveOfflineSkinCache(currentAccount.value!.uuid, {
        src: currentSrc.value,
        variant: skinVariant.value,
      });
      accounts.bumpAvatar();
      offlineHintShow.value = true;
      message.success(t("skin.savedAutoApply"));
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
      message.warning(t("skin.capeApplyFailed", { err: String(e) }));
    }
    lastAppliedSrc.value = currentSrc.value;
    localStorage.setItem(`qookix:offline_variant:${uuid}`, skinVariant.value);
    accounts.bumpAvatar();
    message.success(t("skin.appliedTo", { name: currentAccount.value!.username }));
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
      if (!skinDataUrls.value[s.id]) {
        try {
          skinDataUrls.value[s.id] = await api.readSkinDataUrl(s.filename);
        } catch {
          /* ignore */
        }
      }
      const url = skinDataUrls.value[s.id];
      if (url && !skinModels.value[s.id]) {
        skinModels.value[s.id] = await detectSkinModel(url);
      }
    }
  } catch (e) {
    message.error(String(e));
  } finally {
    loadingSkins.value = false;
  }
}

/**
 * 确保某张皮肤已保存到本地：若本地已有相同内容则跳过，否则保存并刷新列表。
 * 用于正版账号拉取皮肤后「保底高亮」——让获取的皮肤出现在已保存列表并高亮。
 */
async function ensureSkinSavedLocally(dataUrl: string, name: string) {
  const exists = Object.values(skinDataUrls.value).includes(dataUrl);
  if (exists) return;
  try {
    const entry = await api.saveSkinFromData(name, dataUrl);
    skinDataUrls.value[entry.id] = dataUrl;
    skinModels.value[entry.id] = await detectSkinModel(dataUrl);
    await loadSkins();
  } catch {
    /* 保存失败不阻塞预览 */
  }
}

async function previewSkin(src: string, name: string, kind: "local" | "official", model: "classic" | "slim") {
  currentSrc.value = src;
  currentName.value = name;
  currentKind.value = kind;
  skinVariant.value = model;
  await renderer.loadSkinFromSrc(src, model === "slim" ? "slim" : "default");
  // 所有渲染路径的公共出口：渲染完成后套用用户的手动选择（若有的话）。
  // 主动换皮肤时会先 clearVariantOverride() 再走到这里，所以不会误套用；
  // 而切页面/重启后自动加载皮肤时，用户上次的选择会被正确还原。
  applyVariantOverride();
}

async function selectLocal(s: SkinEntry) {
  const url = skinDataUrls.value[s.id];
  if (!url) return;
  // 主动换了皮肤：清除手动选择，按这张皮肤自身的模型显示
  clearVariantOverride();
  const variant = skinModels.value[s.id] ?? (await detectSkinModel(url));
  await previewSkin(url, s.name, "local", variant);
}

async function selectOfficial(s: (typeof BUILTIN_SKINS)[number]) {
  clearVariantOverride();
  await previewSkin(s.dataUrl, s.name, "official", s.model === "slim" ? "slim" : "classic");
}

async function fetchPlayerAndSave() {
  const name = playerInput.value.trim();
  if (!name) {
    message.warning(t("skin.warnEnterPlayerName"));
    return;
  }
  fetchingPlayer.value = true;
  try {
    const res = await api.fetchPlayerSkin(name);
    const entry = await api.saveSkinFromData(name, res.data_url);
    skinDataUrls.value[entry.id] = res.data_url;
    await loadSkins();
    await selectLocal(entry);
    playerModalShow.value = false;
    playerInput.value = "";
    message.success(t("skin.savedPlayerSkin", { name }));
  } catch (e) {
    message.error(String(e));
  } finally {
    fetchingPlayer.value = false;
  }
}

async function uploadSkin() {
  const file = await open({
    multiple: false,
    filters: [{ name: t("skin.skinPngFilter"), extensions: ["png"] }],
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
    message.warning(t("skin.warnEnterSkinName"));
    return;
  }
  try {
    const entry = await api.saveSkinFromData(name, pendingSkinDataUrl);
    skinDataUrls.value[entry.id] = pendingSkinDataUrl;
    await loadSkins();
    await selectLocal(entry);
    renameModalShow.value = false;
    message.success(t("skin.uploaded"));
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
      skinDataUrls.value[entry.id] = await api.readSkinDataUrl(entry.filename);
      await loadSkins();
      await selectLocal(entry);
      message.success(t("skin.savedToLocal"));
      return;
    }
    const name = currentName.value || "skin";
    const entry = await api.saveSkinFromData(name, dataUrl);
    skinDataUrls.value[entry.id] = dataUrl;
    await loadSkins();
    await selectLocal(entry);
    message.success(t("skin.savedToLocal"));
  } catch (e) {
    message.error(String(e));
  } finally {
    savingCurrent.value = false;
  }
}

async function deleteSkin(s: SkinEntry) {
  try {
    await api.deleteSkin(s.filename);
    // 先取内容再删映射：用皮肤内容判断是否为当前预览的皮肤（名字比对不可靠）
    const wasCurrent = isCurrentSkin(skinDataUrls.value[s.id]);
    delete skinDataUrls.value[s.id];
    skins.value = skins.value.filter((x) => x.id !== s.id);
    if (wasCurrent) {
      currentSrc.value = null;
      currentName.value = "";
      currentKind.value = "none";
      renderer.loadSkinFromSrc(null);
    }
    message.success(t("skin.deleted"));
  } catch (e) {
    message.error(String(e));
  }
}

function resetView() {
  renderer.resetView();
}

/** 手动刷新当前账号皮肤：跳过缓存强制重新向服务器拉取 */
const refreshingAccount = ref(false);
async function refreshAccountSkin() {
  if (!currentAccount.value || refreshingAccount.value) return;
  refreshingAccount.value = true;
  try {
    await loadCurrentAccountSkin(true);
    message.success(t("skin.refreshed"));
  } catch (e) {
    message.error(String(e));
  } finally {
    refreshingAccount.value = false;
  }
}

function setAnim(a: AnimationKind) {
  renderer.setAnimation(a);
}

onMounted(async () => {
  await accounts.load();
  await loadSkins();
  // 手动选择由 previewSkin 统一套用（loadCurrentAccountSkin 内部会渲染）
  await loadCurrentAccountSkin();
});
</script>

<template>
  <div id="skin-root" class="skin-view">
    <div class="skin-body">
      <section id="skin-preview" class="preview-pane glass">
        <div class="preview-stage">
          <canvas ref="canvasRef" class="skin-canvas"></canvas>
          <div class="preview-label" :class="{ applied: appliedToCurrent }">
            {{ appliedToCurrent ? t("skin.applied") : t("skin.preview") }}
          </div>
          <div class="drag-hint">{{ t("skin.dragToRotate") }}</div>
        </div>
        <div class="preview-info">
          <div class="info-row">
            <span class="info-label">{{ t("skin.currentSkin") }}</span>
            <span class="info-value">{{ currentName || t("skin.notSelected") }}</span>
            <button
              v-if="isCurrentMs && currentAccount"
              class="info-refresh"
              :class="{ spinning: refreshingAccount }"
              :title="t('skin.refreshSkinBypassCache')"
              @click="refreshAccountSkin"
            >
              <IconRefresh />
            </button>
          </div>
        </div>
        <div class="anim-row">
          <span class="anim-label">{{ t("skin.action") }}</span>
          <div class="seg">
            <button :class="{ active: renderer.animation.value === 'idle' }" @click="setAnim('idle')">{{ t("skin.animIdle") }}</button>
            <button :class="{ active: renderer.animation.value === 'walk' }" @click="setAnim('walk')">{{ t("skin.animWalk") }}</button>
            <button :class="{ active: renderer.animation.value === 'run' }" @click="setAnim('run')">{{ t("skin.animRun") }}</button>
            <button :class="{ active: renderer.animation.value === 'none' }" @click="setAnim('none')">{{ t("skin.animNone") }}</button>
          </div>
        </div>
        <div class="rotate-row">
          <span class="anim-label">{{ t("skin.autoRotate") }}</span>
          <n-switch :value="renderer.autoRotate.value" @update:value="(v: boolean) => renderer.setAutoRotate(v)" />
        </div>
        <div class="preview-actions">
          <button class="mini-btn" @click="resetView">
            <IconRefresh /> {{ t("skin.resetView") }}
          </button>
          <button class="mini-btn" @click="capeModalShow = true">
            <IconShield /> {{ t("skin.cape") }}
          </button>
          <button
            class="mini-btn primary"
            :disabled="!canSaveCurrent || savingCurrent"
            @click="saveCurrentToLocal"
          >
            <IconDownload /> {{ savingCurrent ? t("skin.saving") : t("skin.saveToLocal") }}
          </button>
        </div>
        <div class="apply-row">
          <div class="seg">
            <button :class="{ active: skinVariant === 'classic' }" @click="setSkinModel('classic')">{{ t("skin.modelClassic") }}</button>
            <button :class="{ active: skinVariant === 'slim' }" @click="setSkinModel('slim')">{{ t("skin.modelSlim") }}</button>
          </div>
          <button
            class="mini-btn primary apply-btn"
            :disabled="!currentSrc || applying || !currentAccount"
            @click="applySkin"
          >
            <IconCheck /> {{ applying ? t("skin.applying") : t("skin.apply") }}
          </button>
        </div>
      </section>

      <section class="tabs-pane glass">
        <n-tabs v-model:value="tab" type="line" animated class="sk-tabs">
          <n-tab-pane name="saved" :tab="t('skin.tabSaved')">
            <div class="tab-toolbar">
              <span class="tab-count">{{ t("skin.skinCount", { n: skins.length }) }}</span>
              <div class="toolbar-right">
                <button class="mini-btn" @click="playerModalShow = true">
                  <IconSearch /> {{ t("skin.fetchByPlayer") }}
                </button>
                <button class="mini-btn" :disabled="loadingSkins" @click="loadSkins">
                  <IconRefresh /> {{ loadingSkins ? t("common.loading") : t("common.refresh") }}
                </button>
              </div>
            </div>

            <div class="skin-grid">
              <button class="skin-card upload-card" @click="uploadSkin">
                <div class="thumb-wrap upload-thumb">
                  <IconPlus />
                </div>
                <div class="skin-meta">
                  <div class="skin-name">{{ t("skin.uploadSkin") }}</div>
                  <div class="skin-size">{{ t("skin.selectLocalPng") }}</div>
                </div>
              </button>
              <div
                v-for="s in skins"
                :key="s.id"
                class="skin-card"
                :class="{ active: isCurrentSkin(skinDataUrls[s.id]) }"
                @click="selectLocal(s)"
              >
                <div class="thumb-wrap">
                  <SkinThumb
                    :src="skinDataUrls[s.id] ?? null"
                    :slim="skinModels[s.id] === 'slim'"
                  />
                </div>
                <div class="skin-meta">
                  <div class="skin-name text-ellipsis">{{ s.name }}</div>
                  <div class="skin-size">{{ formatSize(s.size) }}</div>
                </div>
                <button class="del-btn" :title="t('common.delete')" @click.stop="deleteSkin(s)">
                  <IconTrash />
                </button>
              </div>
            </div>
          </n-tab-pane>

          <n-tab-pane name="official" :tab="t('skin.tabOfficial')">
            <div class="tab-toolbar">
              <span class="tab-count">{{ t("skin.minecraftDefault") }}</span>
            </div>
            <div class="skin-grid">
              <div
                v-for="s in BUILTIN_SKINS"
                :key="s.name"
                class="skin-card"
                :class="{ active: currentKind === 'official' && isCurrentSkin(s.dataUrl) }"
                @click="selectOfficial(s)"
              >
                <div class="thumb-wrap">
                  <SkinThumb
                    :src="s.dataUrl"
                    :slim="
                      currentKind === 'official' && isCurrentSkin(s.dataUrl)
                        ? skinVariant === 'slim'
                        : s.model === 'slim'
                    "
                  />
                </div>
                <div class="skin-meta">
                  <div class="skin-name text-ellipsis">{{ s.name }}</div>
                  <div class="skin-size">{{ s.model === "slim" ? t("skin.modelSlim") : t("skin.modelClassic") }}</div>
                </div>
              </div>
            </div>
          </n-tab-pane>
        </n-tabs>
      </section>
    </div>

    <n-modal v-model:show="playerModalShow" preset="card" :title="t('skin.fetchByPlayerTitle')" style="max-width: 420px;">
      <div class="modal-body">
        <n-input
          v-model:value="playerInput"
          :placeholder="t('skin.playerNamePlaceholder')"
          @keyup.enter="fetchPlayerAndSave"
        />
        <div class="modal-actions">
          <n-button @click="playerModalShow = false">{{ t("common.cancel") }}</n-button>
          <n-button type="primary" :loading="fetchingPlayer" @click="fetchPlayerAndSave">
            <template #icon><IconSearch /></template>
            {{ t("skin.fetchAndSave") }}
          </n-button>
        </div>
      </div>
    </n-modal>

    <n-modal v-model:show="offlineHintShow" preset="card" :title="t('skin.offlineApplyTitle')" style="max-width: 420px;">
      <div class="modal-body">
        <p class="offline-hint-text">{{ t("skin.offlineHint") }}</p>
        <div class="modal-actions">
          <n-button type="primary" @click="offlineHintShow = false">{{ t("skin.gotIt") }}</n-button>
        </div>
      </div>
    </n-modal>

    <Teleport to="body">
      <Transition name="cape">
      <div v-if="capeModalShow" class="cape-overlay" @click.self="capeModalShow = false">
        <div class="cape-dialog glass">
          <div class="cape-dialog-head">
            <span>{{ t("skin.capeSettings") }}</span>
            <button class="cape-close-btn" @click="capeModalShow = false">
              <IconClose />
            </button>
          </div>
          <div class="cape-dialog-body">
            <p class="cape-modal-hint">{{ isCurrentMs ? t('skin.capeHintMs') : t('skin.capeHintOffline') }}</p>
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
                  <span v-else class="no-cape-icon">{{ t("skin.capeNoneShort") }}</span>
                </div>
                <div class="skin-meta">
                  <div class="skin-name text-ellipsis">{{ c.name }}</div>
                  <div class="skin-size">{{ c.dataUrl ? t('skin.cape') : t('skin.capeNotUsed') }}</div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
      </Transition>
    </Teleport>

    <n-modal v-model:show="renameModalShow" preset="card" :title="t('skin.renameTitle')" style="max-width: 420px;">
      <div class="modal-body">
        <n-input
          v-model:value="renameInput"
          :placeholder="t('skin.skinNamePlaceholder')"
          @keyup.enter="confirmRename"
        />
        <div class="modal-actions">
          <n-button @click="renameModalShow = false">{{ t("common.cancel") }}</n-button>
          <n-button type="primary" @click="confirmRename">{{ t("common.save") }}</n-button>
        </div>
      </div>
    </n-modal>
  </div>
</template>

<style scoped>
.skin-view {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

/* 预览区固定、皮肤画廊自适应列数；超宽屏下预览区再放宽一些 */
.skin-body {
  display: grid;
  grid-template-columns: 360px 1fr;
  gap: 16px;
  min-height: 540px;
}
@media (min-width: 1600px) {
  .skin-body {
    grid-template-columns: 440px 1fr;
  }
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
    radial-gradient(ellipse at center, var(--accent-08), transparent 70%),
    linear-gradient(180deg, var(--w-03), var(--k-12));
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
  background: var(--k-30);
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
  background: var(--k-40);
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
  background: var(--w-04);
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
.info-refresh {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  padding: 0;
  border: none;
  border-radius: 6px;
  background: var(--w-08);
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s, transform 0.2s;
}
.info-refresh:hover {
  background: var(--w-14);
  color: var(--text-1);
}
.info-refresh svg {
  width: 14px;
  height: 14px;
}
.info-refresh.spinning svg {
  animation: info-spin 0.8s linear infinite;
}
@keyframes info-spin {
  to {
    transform: rotate(360deg);
  }
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
  background: var(--k-50);
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
  background: var(--w-08);
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
  background: var(--w-05);
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
  background: var(--w-04);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  white-space: nowrap;
  transition: all 0.14s;
}
.mini-btn:hover:not(:disabled) {
  background: var(--w-08);
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
  gap: 16px;
}
.skin-card {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 12px;
  border-radius: 11px;
  border: 1px solid var(--border);
  background: var(--w-03);
  cursor: pointer;
  transition: all 0.14s;
  font-family: inherit;
  text-align: left;
}
.skin-card:hover {
  background: var(--w-07);
  border-color: var(--accent-35);
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
  border-color: var(--accent-45);
}
.thumb-wrap {
  width: 100%;
  aspect-ratio: 1 / 1;
  border-radius: 8px;
  overflow: hidden;
  background: var(--k-20);
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
  background: var(--k-45);
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
  background: var(--danger-30);
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
