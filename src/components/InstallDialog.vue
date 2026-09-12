<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useSettingsStore } from "../stores/settings";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useRouter } from "vue-router";
import { NModal, NSelect, NButton, useMessage } from "naive-ui";
import { api } from "../api";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { fmtDateStr as fmtDate, instanceLabel } from "../utils/format";
import { useInstancesStore } from "../stores/instances";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import { IconCopy, IconExternal, IconGlobe } from "./icons";
import type { ProjectDependency, ProjectHit, ProjectVersion } from "../types";

const props = defineProps<{
  show: boolean;
  project: ProjectHit | null;
  defaultInstance?: string | null;
}>();
const emit = defineEmits<{
  "update:show": [v: boolean];
  "install-dep": [dep: ProjectDependency];
}>();

// 点击弹窗卡片外部即关闭（用 document 委托，不依赖 naive-ui 的 mask 机制）。
// 用 mousedown 而非 click：打开弹窗的那次按下发生在 show 变 true 之前，
// 会被 `!props.show` 拦截，避免弹窗刚打开就被自身触发的点击冒泡关掉。
const cardRef = ref<HTMLElement | null>(null);
function onDocMouseDown(e: MouseEvent) {
  if (!props.show) return;
  const t = e.target as Element | null;
  if (!t) return;
  // 点击弹窗卡片内部、或 naive-ui 的下拉/弹层（teleport 到 body）都不应关闭
  if (cardRef.value?.contains(t)) return;
  if (t.closest(".v-binder-follower-container, .n-base-select-menu, .n-popover, .n-dropdown")) return;
  emit("update:show", false);
}
onMounted(() => document.addEventListener("mousedown", onDocMouseDown));
onBeforeUnmount(() => document.removeEventListener("mousedown", onDocMouseDown));

const instances = useInstancesStore();
const router = useRouter();
const message = useMessage();

async function copyName() {
  if (!props.project?.title) return;
  try {
    await navigator.clipboard.writeText(props.project.title);
    message.success("已复制名称");
  } catch {
    message.error("复制失败");
  }
}

const versions = ref<ProjectVersion[]>([]);
const loadingVersions = ref(false);
const selectedVersion = ref<string | null>(null);
const selectedInstance = ref<string | null>(null);
const installing = ref(false);
const installMsg = ref("");
const typeFilter = ref<"all" | "release" | "beta" | "alpha">("all");

// 版本类型 tabs 的滑动高亮指示器
const typeTabBox = ref<HTMLElement | null>(null);
const { indicatorStyle: typeTabIndicatorStyle, refresh: refreshTypeTabIndicator } = useSlidingIndicator(
  typeTabBox,
  () => Array.from(typeTabBox.value?.querySelectorAll<HTMLElement>(".id-type-tabs button") ?? []),
  () => ["all", "release", "beta", "alpha"].indexOf(typeFilter.value),
  { axis: "horizontal" }
);
watch(typeFilter, () => nextTick(() => refreshTypeTabIndicator()));
const deps = ref<ProjectDependency[]>([]);
const loadingDeps = ref(false);

const isModpack = computed(() => props.project?.project_type === "modpack");

const mcWikiUrl = ref("");
const sourceUrl = computed(() => {
  const p = props.project;
  if (!p) return "";
  if (p.provider === "modrinth") {
    return `https://modrinth.com/${p.project_type}/${p.slug}`;
  }
  if (p.provider === "curseforge") {
    const kind = p.project_type === "modpack" ? "modpacks"
      : p.project_type === "resourcepack" ? "texture-packs"
      : p.project_type === "shader" ? "shaders"
      : "mc-mods";
    return `https://www.curseforge.com/minecraft/${kind}/${p.slug}`;
  }
  return "";
});

async function loadMcWikiUrl() {
  if (!props.project) return;
  try {
    mcWikiUrl.value = await api.mcWikiUrl(props.project.title, props.project.slug, props.project.provider);
  } catch {
    mcWikiUrl.value = "";
  }
}

const instanceOptions = () =>
  instances.instances.filter((i) => i.loader !== "vanilla").map((i) => ({
    label: instanceLabel(i),
    value: i.id,
  }));

function versionType(v: ProjectVersion): string {
  // modrinth: version_type; curseforge: release_type (1=release,2=beta,3=alpha)
  if (v.version_type) return v.version_type;
  if (v.release_type === 2) return "beta";
  if (v.release_type === 3) return "alpha";
  return "release";
}

function typeLabel(t: string) {
  return { release: "正式版", beta: "测试版", alpha: "先行版" }[t] ?? "正式版";
}

const filteredVersions = computed(() => {
  if (typeFilter.value === "all") return versions.value;
  return versions.value.filter((v) => versionType(v) === typeFilter.value);
});

async function loadDeps() {
  deps.value = [];
  if (!props.project || !selectedVersion.value) return;
  loadingDeps.value = true;
  try {
    deps.value = await api.projectDependencies(props.project.provider, props.project.id, selectedVersion.value);
  } catch {
    deps.value = [];
  } finally {
    loadingDeps.value = false;
  }
}

watch(selectedVersion, () => { if (!isModpack.value) loadDeps(); });

// ---- 描述翻译 ----
const settingsStore = useSettingsStore();
/** 当前翻译服务：default（内置）| custom（用户 AI）| baidu_web（跳转浏览器） */
const translateService = computed(() => settingsStore.settings?.translate_provider ?? "default");
/** 质量反馈接口仅内置翻译服务支持 */
const translateIsDefault = computed(() => translateService.value === "default");
const descZh = ref<string | null>(null);
const descLoading = ref(false);

/** 百度网页模式：用浏览器打开翻译页，由用户自行查看结果（不做自动抓取） */
function openBaiduTranslate() {
  const p = props.project;
  if (!p) return;
  const q = encodeURIComponent(p.description || p.title);
  openUrl(`https://fanyi.baidu.com/mtpe-individual/transText?query=${q}&lang=en2zh`).catch(
    (e: unknown) => message.error("打开浏览器失败：" + String(e))
  );
}

// ---- 翻译反馈面板 ----
const feedbackPanel = ref(false);
const feedbackMode = ref<"choose" | "quality">("choose");
const issueType = ref("wrong_translation");
const userSuggestion = ref("");
const userComment = ref("");
const submittingFeedback = ref(false);
const ISSUE_TYPES = [
  { v: "wrong_translation", label: "翻译错了" },
  { v: "unnatural", label: "生硬不自然" },
  { v: "missing", label: "漏翻了内容" },
  { v: "other", label: "其他" },
] as const;

watch(
  () => [props.show, props.project?.id, props.project?.provider] as const,
  async ([show, id, provider]) => {
    descZh.value = null;
    feedbackPanel.value = false;
    const custom = translateService.value === "custom";
    // 百度网页模式不请求翻译；内置服务只支持 Modrinth；自定义 AI 两个平台都能翻
    if (!show || !id || !provider || translateService.value === "baidu_web") return;
    if (provider !== "modrinth" && !custom) return;
    descLoading.value = true;
    try {
      const r = await api.translateDescriptions(provider, [id]);
      descZh.value = r.translations[id] ?? null;
    } catch {
      descZh.value = null;
    } finally {
      descLoading.value = false;
    }
  }
);

function toggleFeedbackPanel() {
  feedbackPanel.value = !feedbackPanel.value;
  feedbackMode.value = "choose";
}

async function reportStale() {
  const p = props.project;
  if (!p) return;
  try {
    const status = await api.reportStaleTranslation(p.provider, p.slug);
    message.success("感谢反馈");
    feedbackPanel.value = false;
    if (status === "updated") {
      const r = await api.translateDescriptions(p.provider, [p.slug]);
      descZh.value = r.translations[p.slug] ?? null;
    }
  } catch (e) {
    message.error("反馈失败：" + String(e));
  }
}

async function submitQuality() {
  const p = props.project;
  if (!p || submittingFeedback.value) return;
  submittingFeedback.value = true;
  try {
    await api.reportQualityFeedback(
      p.provider,
      p.slug,
      issueType.value,
      userSuggestion.value,
      userComment.value
    );
    message.success("感谢反馈");
    feedbackPanel.value = false;
    feedbackMode.value = "choose";
    issueType.value = "wrong_translation";
    userSuggestion.value = "";
    userComment.value = "";
  } catch (e) {
    message.error("反馈失败：" + String(e));
  } finally {
    submittingFeedback.value = false;
  }
}

async function loadVersions() {
  if (!props.project) return;
  loadingVersions.value = true;
  selectedVersion.value = null;
  deps.value = [];
  try {
    // 已选实例时用实例的 MC 版本 + 加载器请求上游，让 Modrinth/CurseForge
    // 只返回兼容的版本，从而 API 本身返回更少的数据（而非全量拉取后本地过滤）
    const inst = selectedInstance.value ? instances.get(selectedInstance.value) : null;
    const mc = inst?.mc_version ?? "";
    const ld = inst && inst.loader !== "vanilla" ? inst.loader : "";
    const res = await api.projectVersions(props.project.provider, props.project.id, mc, ld);
    versions.value = res.versions;
    if (inst && !res.versions.length) {
      // 上游按实例筛选无结果时，回退拉取全部版本供选择
      const all = await api.projectVersions(props.project.provider, props.project.id, "", "");
      versions.value = all.versions;
    }
    const picked = versions.value.find((v) => versionType(v) === "release") ?? versions.value[0];
    if (picked) selectedVersion.value = picked.id;
  } catch (e) {
    message.error(String(e));
  } finally {
    loadingVersions.value = false;
  }
}

// 切换实例时按新实例的 MC 版本/加载器重新拉取
watch(selectedInstance, () => {
  if (props.show && props.project) loadVersions();
});

function resetForProject() {
  if (!props.project) return;
  const nonVanilla = instances.instances.filter((i) => i.loader !== "vanilla");
  const pref = props.defaultInstance && nonVanilla.some((i) => i.id === props.defaultInstance)
    ? props.defaultInstance
    : nonVanilla[0]?.id ?? null;
  selectedInstance.value = isModpack.value ? null : pref;
  installMsg.value = "";
  typeFilter.value = "all";
  loadVersions();
  loadMcWikiUrl();
}

async function onOpen() {
  if (!props.show) return;
  api.logDebug(
    `[fe] 对话框打开 project=${props.project?.provider}/${props.project?.id} type=${props.project?.project_type}`
  );
  resetForProject();
  loadBody(false);
}

// —— 正文面板（右侧栏）：译文/原文按钮切换，手动翻译的结果走缓存 ——
type BodyState = {
  status: "loading" | "ok" | "error";
  bodyHtml: string; // 译文渲染后的 HTML（空 = 尚未翻译）
  originalHtml: string; // 原文渲染后的 HTML
  showZh: boolean; // 当前显示译文（false = 显示原文）
  note: string; // 顶部提示（不支持 / 失败原因）
  translating: boolean; // 正在请求译文
};
const bodyState = ref<BodyState | null>(null);
let bodySeq = 0;

/** 内置服务仅支持 Modrinth 正文翻译；自定义 API / 百度网页模式不支持 */
const canTranslateBody = computed(
  () => props.project?.provider === "modrinth" && settingsStore.settings?.translate_provider === "default"
);

/** 当前应渲染的内容：showZh 且有译文 → 译文，否则原文 */
const bodyDisplayHtml = computed(() => {
  const st = bodyState.value;
  if (!st) return "";
  return st.showZh && st.bodyHtml ? st.bodyHtml : st.originalHtml;
});
const bodyIsZh = computed(() => {
  const st = bodyState.value;
  return !!st && st.showZh && !!st.bodyHtml;
});

/** Modrinth 正文是 Markdown；CurseForge 是 HTML。统一渲染后消毒，防 XSS。 */
function renderContent(text: string, provider: string): string {
  const html = provider === "curseforge" ? text : (marked.parse(text, { async: false }) as string);
  return DOMPurify.sanitize(html);
}

/** 拉取正文。translate=false 仅原文；缓存命中的译文会随响应直接返回 */
async function loadBody(translate: boolean) {
  const p = props.project;
  if (!p) return;
  const seq = ++bodySeq;
  const auto = !!settingsStore.settings?.body_translate_auto;
  const wantTranslate = translate || (auto && canTranslateBody.value);
  bodyState.value = {
    status: "loading",
    bodyHtml: "",
    originalHtml: "",
    showZh: false,
    note: "",
    translating: wantTranslate,
  };
  try {
    const r = await api.translateBody(p.provider, p.slug || p.id, wantTranslate);
    if (seq !== bodySeq) return; // 已切换到别的项目，丢弃过期结果
    const state: BodyState = {
      status: "ok",
      bodyHtml: r.body ? renderContent(r.body, p.provider) : "",
      originalHtml: r.original ? renderContent(r.original, p.provider) : "",
      // 缓存命中的译文直接展示；未翻译则显示原文
      showZh: !!r.body,
      note: "",
      translating: false,
    };
    if (r.error) state.note = `正文翻译失败：${r.error}`;
    else if (!r.supported) state.note = "该平台暂无正文内容";
    bodyState.value = state;
  } catch (e) {
    if (seq !== bodySeq) return;
    bodyState.value = {
      status: "error",
      bodyHtml: "",
      originalHtml: "",
      showZh: false,
      note: `正文加载失败：${String(e)}`,
      translating: false,
    };
  }
}

/** 正文里的链接一律用系统浏览器打开，防止 webview 就地导航覆盖整个应用 */
function onBodyClick(e: MouseEvent) {
  const a = (e.target as Element | null)?.closest("a[href]");
  if (!a) return;
  e.preventDefault();
  const href = a.getAttribute("href") ?? "";
  if (/^https?:\/\//i.test(href)) {
    openUrl(href).catch(() => {});
  }
}

/** 标题右侧按钮：未翻译时发起翻译；已有译文时在中/英之间切换 */
function onBodyBtn() {
  const st = bodyState.value;
  if (!st || st.translating || st.status !== "ok") return;
  if (st.bodyHtml) {
    st.showZh = !st.showZh;
    return;
  }
  if (!canTranslateBody.value) {
    message.info("正文翻译暂不支持当前翻译服务（仅内置服务 + Modrinth 支持）");
    return;
  }
  st.translating = true;
  const p = props.project;
  if (!p) return;
  api
    .translateBody(p.provider, p.slug || p.id, true)
    .then((r) => {
      if (p !== props.project || !bodyState.value) return;
      if (r.body) {
        st.bodyHtml = renderContent(r.body, p.provider);
        st.showZh = true;
        st.note = "";
      } else {
        st.note = r.error ?? "正文翻译失败";
      }
    })
    .catch((e) => message.error("正文翻译失败：" + String(e)))
    .finally(() => {
      if (bodyState.value) bodyState.value.translating = false;
    });
}

// 弹窗内切换项目（点击前置依赖）时重新加载版本与依赖
watch(
  () => props.project,
  (proj, oldProj) => {
    if (proj && oldProj && proj !== oldProj) resetForProject();
  }
);

function depLabel(t: string) {
  return { required: "必需", optional: "可选", incompatible: "不兼容", embedded: "内嵌" }[t] ?? t;
}

async function install() {
  if (!props.project) return;
  api.logDebug(
    `[fe] install() 进入 provider=${props.project.provider} id=${props.project.id} type=${props.project.project_type} isModpack=${isModpack.value} versions=${versions.value.length}`
  );
  if (!isModpack.value && !selectedInstance.value) {
    api.logDebug("[fe] 拦截：未选择实例");
    message.warning("请选择一个实例");
    return;
  }
  if (!selectedVersion.value) {
    api.logDebug(`[fe] 拦截：未选择版本 selectedVersion=${String(selectedVersion.value)}`);
    message.warning(versions.value.length ? "请选择一个版本" : "该 mod 没有可用版本，可能不兼容当前实例或加载失败");
    return;
  }
  api.logDebug(
    `[fe] 发起 invoke instanceId=${String(selectedInstance.value)} version=${String(selectedVersion.value)} kind=${props.project.project_type}`
  );
  installing.value = true;
  message.success(isModpack.value ? "已开始安装，进度见下载中心" : "已添加到下载队列");
  // 整合包安装是长任务（下载整包 + 逐个拉取 mod 元数据 + 装游戏本体），
  // 不阻塞对话框——立即关闭，进度与成败都通过 install://progress 事件进下载中心。
  api
    .installContent(
      selectedInstance.value ?? "",
      props.project.provider,
      props.project.id,
      selectedVersion.value,
      props.project.project_type
    )
    .then((r) => {
      api.logDebug(`[fe] 返回成功 ${JSON.stringify(r)}`);
      if (!isModpack.value) message.success("安装完成");
    })
    .catch((e) => {
      api.logDebug(`[fe] 返回失败 ${String(e)}`);
      message.error(String(e));
    })
    .finally(() => {
      installing.value = false;
    });
  emit("update:show", false);
}

</script>

<template>
  <n-modal
    :show="props.show"
    preset="card"
    :title="props.project?.title ?? '安装内容'"
    style="width: 980px; max-width: 96vw"
    :mask-closable="true"
    :close-on-esc="true"
    @update:show="(v: boolean) => emit('update:show', v)"
    @mask-click="() => emit('update:show', false)"
    @after-enter="onOpen"
  >
    <div v-if="props.project" ref="cardRef" class="id-modal">
      <div class="id-cols">
      <div class="id-left">
      <div class="id-head">
        <img v-if="props.project.icon_url" :src="props.project.icon_url" class="id-icon" alt="" />
        <div class="id-info">
          <div class="id-title">{{ props.project.title }}</div>
          <div class="id-meta">
            <span class="id-author">{{ props.project.author }}</span>
            <span class="id-dl">{{ (props.project.downloads / 10000).toFixed(1) }} 万下载</span>
            <span class="id-type">{{ props.project.project_type }}</span>
          </div>
          <div class="id-desc" :class="{ expanded: !!descZh }">
            <template v-if="descZh">
              <div class="id-desc-zh">{{ descZh }}</div>
              <div class="id-desc-en">{{ props.project.description }}</div>
              <button
                v-if="descZh && translateIsDefault"
                class="id-desc-feedback"
                @click="toggleFeedbackPanel"
              >
                {{ feedbackPanel ? "收起" : "翻译有问题？" }}
              </button>
              <button
                v-if="translateService === 'baidu_web'"
                class="id-desc-feedback"
                @click="openBaiduTranslate"
              >用百度翻译打开</button>
              <div v-if="feedbackPanel" class="id-fb-panel">
                <template v-if="feedbackMode === 'choose'">
                  <button class="id-fb-opt" @click="reportStale">描述更新了，翻译是旧的</button>
                  <button class="id-fb-opt" @click="feedbackMode = 'quality'">翻译质量不好，提建议</button>
                </template>
                <template v-else>
                  <div class="id-fb-types">
                    <button
                      v-for="t in ISSUE_TYPES"
                      :key="t.v"
                      class="id-fb-type"
                      :class="{ on: issueType === t.v }"
                      @click="issueType = t.v"
                    >{{ t.label }}</button>
                  </div>
                  <textarea
                    v-model="userSuggestion"
                    class="id-fb-input"
                    rows="2"
                    maxlength="500"
                    placeholder="建议翻译（可选，500 字以内）"
                  ></textarea>
                  <textarea
                    v-model="userComment"
                    class="id-fb-input"
                    rows="2"
                    maxlength="500"
                    placeholder="补充说明（可选，500 字以内）"
                  ></textarea>
                  <div class="id-fb-actions">
                    <button class="id-fb-back" @click="feedbackMode = 'choose'">返回</button>
                    <button class="id-fb-submit" :disabled="submittingFeedback" @click="submitQuality">
                      {{ submittingFeedback ? "提交中…" : "提交反馈" }}
                    </button>
                  </div>
                </template>
              </div>
            </template>
            <template v-else>{{ props.project.description }}</template>
          </div>
          <div class="id-links">
            <a v-if="mcWikiUrl" :href="mcWikiUrl" target="_blank" class="id-link"><IconGlobe /> MC百科</a>
            <a v-if="sourceUrl" :href="sourceUrl" target="_blank" class="id-link"><IconExternal /> 在浏览器打开</a>
            <button class="id-link" @click="copyName"><IconCopy /> 复制名称</button>
          </div>
        </div>
      </div>

      <div class="id-form">
        <label v-if="!isModpack" class="id-field">
          <span>安装到实例</span>
          <n-select v-model:value="selectedInstance" :options="instanceOptions()" placeholder="选择实例" />
        </label>

        <div v-if="isModpack" class="id-field">
          <span class="id-modpack-hint">整合包将自动创建新实例</span>
        </div>

        <div class="id-field">
          <div class="id-ver-head">
            <span>选择版本</span>
            <div ref="typeTabBox" class="id-type-tabs">
              <div class="indicator" :style="typeTabIndicatorStyle"></div>
              <button :class="{ active: typeFilter === 'all' }" @click="typeFilter = 'all'">全部</button>
              <button :class="{ active: typeFilter === 'release' }" @click="typeFilter = 'release'">正式版</button>
              <button :class="{ active: typeFilter === 'beta' }" @click="typeFilter = 'beta'">测试版</button>
              <button :class="{ active: typeFilter === 'alpha' }" @click="typeFilter = 'alpha'">先行版</button>
            </div>
          </div>
          <div v-if="loadingVersions" class="id-loading">加载中…</div>
          <div v-else class="id-ver-list">
            <button
              v-for="v in filteredVersions.slice(0, 80)"
              :key="v.id"
              class="id-ver-row"
              :class="{ active: selectedVersion === v.id }"
              @click="selectedVersion = v.id"
            >
              <span class="id-ver-num mono">{{ v.version_number }}</span>
              <span class="id-ver-type" :class="versionType(v)">{{ typeLabel(versionType(v)) }}</span>
              <span class="id-ver-mc">{{ (v.game_versions ?? []).slice(-2).join(", ") }}</span>
              <span class="id-ver-date">{{ fmtDate(v.date_published) }}</span>
            </button>
            <div v-if="!filteredVersions.length" class="id-empty">该分类下没有版本</div>
          </div>
        </div>

        <!-- dependencies -->
        <div v-if="(deps.length || loadingDeps) && !isModpack" class="id-deps">
          <span class="id-deps-label">前置依赖</span>
          <div v-if="!loadingDeps" class="id-deps-list">
            <button
              v-for="d in deps"
              :key="d.projectId"
              class="id-dep-chip"
              :class="d.dependencyType"
              :title="`查看 ${d.title} 详情`"
              @click="emit('install-dep', d)"
            >
              <span class="id-dep-tag">{{ depLabel(d.dependencyType) }}</span>
              {{ d.title }}
            </button>
          </div>
          <div v-if="loadingDeps" class="id-deps-loading">正在查询前置…</div>
        </div>
      </div>
      </div>

      <!-- 右栏：正文（译文优先，原文在后）。标题在滚动区外，永不与内容重叠 -->
      <aside class="id-right" @click="onBodyClick">
        <div class="id-body-head">
          详情正文
          <button
            v-if="bodyState && bodyState.status === 'ok' && canTranslateBody"
            class="id-body-translate-btn"
            :disabled="bodyState.translating"
            @click="onBodyBtn"
          >
            {{ bodyState.translating
              ? "翻译中…"
              : bodyState.showZh && bodyState.bodyHtml ? "显示原文"
              : bodyState.bodyHtml ? "显示译文" : "翻译" }}
          </button>
        </div>
        <div class="id-body-scroll">
          <div v-if="!bodyState || bodyState.status === 'loading'" class="id-body-note">加载正文…</div>
          <template v-else>
            <div v-if="bodyState.note" class="id-body-note">{{ bodyState.note }}</div>
            <Transition name="bodyfade" mode="out-in">
              <div
                v-if="bodyDisplayHtml"
                :key="bodyIsZh ? 'zh' : 'en'"
                class="id-body-content"
                :class="{ 'id-body-orig': !bodyIsZh }"
                v-html="bodyDisplayHtml"
              ></div>
              <div v-else key="empty" class="id-body-note">暂无正文内容</div>
            </Transition>
          </template>
        </div>
      </aside>
      </div>

      <div v-if="installMsg" class="id-msg">{{ installMsg }}</div>
      <div v-if="!isModpack && !instances.instances.length" class="id-noinst">
        还没有实例，<a @click="emit('update:show', false); router.push('/instances')">先去创建实例</a>
      </div>

      <!-- 操作按钮放在内容区内（而非 #footer）：
           版本列表较长时会把窗口撑高，footer 会跑到视口外点不到。 -->
      <div class="id-footer">
        <n-button @click="emit('update:show', false)">关闭</n-button>
        <n-button
          type="primary"
          :loading="installing"
          @click="
            api.logDebug('[fe] 一键安装按钮被点击');
            install();
          "
        >
          一键安装
        </n-button>
      </div>
    </div>
  </n-modal>
</template>

<style>
/* modal content is teleported to <body>; keep styles global */
.id-modal {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
/* —— 两栏布局：左=安装操作，右=正文 —— */
.id-cols {
  display: flex;
  gap: 18px;
  align-items: stretch;
}
.id-left {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.id-right {
  flex: 0 0 44%;
  max-width: 44%;
  min-width: 0;
  border-left: 1px solid var(--border, rgba(255, 255, 255, 0.08));
  padding: 10px 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  /* 高度完全跟随左栏（stretch 对齐），正文滚动交给 .id-body-scroll */
  background: var(--n-color, #151924);
  border-radius: 10px;
}
.id-body-scroll {
  flex: 1 1 0;
  min-height: 0;
  overflow-y: auto;
  padding-right: 6px;
}
.id-body-head {
  font-size: 13px;
  font-weight: 700;
  color: var(--text-1);
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.id-body-translate-btn {
  border: 1px solid var(--border, rgba(255, 255, 255, 0.15));
  border-radius: 6px;
  background: transparent;
  color: var(--accent);
  font-size: 12px;
  padding: 2px 10px;
  cursor: pointer;
}
.id-body-translate-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.id-body-translate-btn:hover:not(:disabled) {
  background: var(--panel-hover, rgba(255, 255, 255, 0.07));
}
/* 与卡片描述翻译一致的淡入淡出 */
.bodyfade-enter-active,
.bodyfade-leave-active {
  transition: opacity 0.25s ease;
}
.bodyfade-enter-from,
.bodyfade-leave-to {
  opacity: 0;
}
.id-body-note {
  font-size: 12px;
  color: var(--text-3);
}
.id-body-sep {
  margin-top: 10px;
  padding-top: 8px;
  border-top: 1px dashed var(--border, rgba(255, 255, 255, 0.12));
  font-size: 12px;
  font-weight: 600;
  color: var(--text-3);
}
.id-body-orig {
  opacity: 0.75;
  font-size: 13px;
}
/* 正文（markdown / html）基础排版 */
.id-body-content {
  font-size: 13px;
  line-height: 1.65;
  color: var(--text-1);
  word-break: break-word;
}
.id-body-content h1,
.id-body-content h2,
.id-body-content h3,
.id-body-content h4 {
  font-size: 14px;
  font-weight: 700;
  margin: 12px 0 6px;
}
.id-body-content p {
  margin: 6px 0;
}
.id-body-content ul,
.id-body-content ol {
  padding-left: 20px;
  margin: 6px 0;
}
.id-body-content code {
  background: var(--bg-3, rgba(128, 128, 128, 0.15));
  border-radius: 4px;
  padding: 1px 5px;
  font-size: 12px;
}
.id-body-content pre {
  background: var(--bg-3, rgba(128, 128, 128, 0.12));
  border-radius: 8px;
  padding: 10px;
  overflow-x: auto;
}
.id-body-content pre code {
  background: transparent;
  padding: 0;
}
.id-body-content img {
  max-width: 100%;
  border-radius: 8px;
}
.id-body-content a {
  color: var(--accent);
}
.id-body-content blockquote {
  border-left: 3px solid var(--border, rgba(255, 255, 255, 0.15));
  margin: 6px 0;
  padding: 2px 12px;
  color: var(--text-2);
}
.id-body-content table {
  border-collapse: collapse;
  margin: 8px 0;
}
.id-body-content th,
.id-body-content td {
  border: 1px solid var(--border, rgba(255, 255, 255, 0.12));
  padding: 4px 8px;
  font-size: 12px;
}
/* 窄屏退化为单栏，正文移到下方 */
@media (max-width: 860px) {
  .id-cols {
    flex-direction: column;
  }
  .id-right {
    flex: none;
    max-width: none;
    border-left: none;
    border-top: 1px solid var(--border, rgba(255, 255, 255, 0.08));
    padding-left: 0;
    padding-top: 12px;
    max-height: 45vh;
  }
}
.id-head {
  display: flex;
  gap: 14px;
  align-items: flex-start;
}
.id-icon {
  width: 56px;
  height: 56px;
  border-radius: 12px;
  object-fit: cover;
  flex-shrink: 0;
  background: var(--panel);
}
.id-title {
  font-size: 16px;
  font-weight: 700;
  margin-bottom: 4px;
}
.id-meta {
  display: flex;
  gap: 10px;
  font-size: 12px;
  color: var(--text-3);
  margin-bottom: 6px;
}
.id-type {
  background: var(--accent-soft);
  color: var(--accent);
  border-radius: 6px;
  padding: 0 7px;
  font-weight: 600;
}
.id-desc {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.5;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.id-links {
  display: flex;
  gap: 8px;
  margin-top: 6px;
}
.id-link {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--text-3);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 7px;
  padding: 3px 9px;
  text-decoration: none;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.id-link:hover {
  color: var(--accent, #e89a4b);
  border-color: var(--accent-45);
}
.id-site {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border: 1px solid var(--border);
  background: var(--panel);
  color: var(--text-2);
  border-radius: 8px;
  padding: 6px 11px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  flex-shrink: 0;
}
.id-site:hover {
  color: var(--accent);
  border-color: var(--accent-45);
}
.id-form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.id-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
  color: var(--text-2);
}
.id-modpack-hint {
  color: var(--accent, #50c878);
  font-weight: 600;
  font-size: 13px;
}
.id-ver-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.id-type-tabs {
  position: relative;
  display: flex;
  gap: 3px;
  background: var(--panel);
  border-radius: 8px;
  padding: 2px;
}
.id-type-tabs .indicator {
  position: absolute;
  top: 2px;
  bottom: 2px;
  border-radius: 6px;
  background: var(--accent-soft);
  pointer-events: none;
}
.id-type-tabs button {
  border: none;
  background: transparent;
  color: var(--text-3);
  padding: 4px 10px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.id-type-tabs button.active {
  color: var(--accent);
}
.id-loading {
  padding: 20px;
  text-align: center;
  color: var(--text-3);
  font-size: 13px;
}
.id-ver-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 200px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 6px;
  background: var(--panel);
}
.id-ver-row {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-2);
  padding: 6px 10px;
  border-radius: 8px;
  cursor: pointer;
  font-family: inherit;
  text-align: left;
}
.id-ver-row:hover {
  background: var(--panel);
}
.id-ver-row.active {
  border-color: var(--accent-05);
  background: var(--accent-soft);
}
.id-ver-num {
  font-size: 12px;
  font-weight: 600;
  min-width: 0;
  flex-shrink: 1;
}
.id-ver-type {
  font-size: 10px;
  padding: 1px 7px;
  border-radius: 6px;
  font-weight: 600;
  flex-shrink: 0;
}
.id-ver-type.release {
  background: rgba(78, 201, 160, 0.14);
  color: #4ec9a0;
}
.id-ver-type.beta {
  background: var(--accent-soft);
  color: var(--accent);
}
.id-ver-type.alpha {
  background: var(--danger-14);
  color: #e5534b;
}
.id-ver-mc {
  font-size: 11px;
  color: var(--text-3);
  flex: 1;
  text-align: right;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.id-ver-date {
  font-size: 11px;
  color: var(--text-3);
  flex-shrink: 0;
}
.id-empty {
  text-align: center;
  color: var(--text-3);
  font-size: 13px;
  padding: 16px 0;
}
.id-deps {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.id-deps-label {
  font-size: 12px;
  font-weight: 700;
  color: var(--text-3);
}
.id-deps-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.id-dep-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--border);
  background: var(--panel);
  color: var(--text-2);
  border-radius: 8px;
  padding: 4px 10px;
  font-size: 12px;
  cursor: pointer;
  font-family: inherit;
}
.id-dep-chip:hover {
  border-color: var(--accent-45);
}
.id-dep-tag {
  font-size: 10px;
  font-weight: 700;
  padding: 0 6px;
  border-radius: 5px;
}
.id-dep-chip.required .id-dep-tag {
  background: var(--danger-16);
  color: #e5534b;
}
.id-dep-chip.optional .id-dep-tag {
  background: var(--info-15);
  color: #7cb8f5;
}
.id-dep-chip.incompatible .id-dep-tag {
  background: var(--border);
  color: var(--text-3);
}
.id-deps-loading {
  font-size: 12px;
  color: var(--text-3);
}
.id-msg {
  font-size: 13px;
  color: var(--accent);
}
.id-noinst {
  font-size: 12px;
  color: var(--text-3);
}
.id-noinst a {
  color: var(--accent);
  cursor: pointer;
}
.id-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.id-desc-zh {
  color: var(--text-1);
}
/* 中英对照模式放开 2 行截断，长描述完整展示 */
.id-desc.expanded {
  display: block;
  -webkit-line-clamp: unset;
  overflow: visible;
}
.id-desc-en {
  margin-top: 4px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-3);
}
.id-desc-feedback {
  margin-top: 6px;
  background: none;
  border: none;
  color: var(--text-3);
  font-size: 12px;
  cursor: pointer;
  padding: 0;
  text-decoration: underline dotted;
  text-underline-offset: 3px;
}
.id-desc-feedback:hover {
  color: var(--text-1);
}
.id-desc-feedback:disabled {
  cursor: default;
  opacity: 0.6;
}

/* ---- 翻译反馈面板 ---- */
.id-fb-panel {
  margin-top: 8px;
  padding: 10px 12px;
  border: 1px solid var(--w-10);
  border-radius: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.id-fb-opt {
  text-align: left;
  background: var(--w-04);
  border: none;
  border-radius: 8px;
  color: var(--text-2);
  font-size: 12px;
  padding: 7px 10px;
  cursor: pointer;
}
.id-fb-opt:hover {
  background: var(--w-08);
  color: var(--text-1);
}
.id-fb-types {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.id-fb-type {
  background: var(--w-04);
  border: none;
  border-radius: 14px;
  color: var(--text-2);
  font-size: 12px;
  padding: 4px 10px;
  cursor: pointer;
}
.id-fb-type.on {
  background: var(--accent-14);
  color: var(--accent);
}
.id-fb-input {
  background: var(--w-04);
  border: 1px solid var(--w-08);
  border-radius: 8px;
  color: var(--text-1);
  font-size: 12px;
  line-height: 1.5;
  padding: 6px 8px;
  resize: none;
  font-family: inherit;
}
.id-fb-input:focus {
  outline: none;
  border-color: var(--accent-35);
}
.id-fb-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.id-fb-back {
  background: none;
  border: none;
  color: var(--text-3);
  font-size: 12px;
  cursor: pointer;
  padding: 4px 6px;
}
.id-fb-back:hover {
  color: var(--text-1);
}
.id-fb-submit {
  background: var(--accent-14);
  color: var(--accent);
  border: none;
  border-radius: 8px;
  font-size: 12px;
  padding: 5px 12px;
  cursor: pointer;
}
.id-fb-submit:hover {
  background: var(--accent-22);
}
.id-fb-submit:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
