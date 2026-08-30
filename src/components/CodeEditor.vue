<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { highlight, langLabel } from "../utils/highlight";

const props = defineProps<{
  modelValue: string;
  /** 文件名，用于显示语言类型 */
  filename: string;
  readonly?: boolean;
}>();

const emit = defineEmits<{
  (e: "update:modelValue", v: string): void;
  (e: "save"): void;
  (e: "contextmenu", p: { x: number; y: number }): void;
}>();

const taRef = ref<HTMLTextAreaElement | null>(null);
const scroller = ref<HTMLDivElement | null>(null);
const scrollTop = ref(0);
const cursorLine = ref(1);
const cursorCol = ref(1);
const focused = ref(false);

const LINE_H = 20;
const INDENT = "  ";

const ext = computed(() => {
  const i = props.filename.lastIndexOf(".");
  return i < 0 ? "" : props.filename.slice(i + 1).toLowerCase();
});

const lines = computed(() => props.modelValue.split("\n"));

const highlighted = computed(() => highlight(props.modelValue));

function onInput(e: Event) {
  const ta = e.target as HTMLTextAreaElement;
  emit("update:modelValue", ta.value);
  syncScroll();
  updateCursor();
}

function onScroll() {
  syncScroll();
}

function syncScroll() {
  const el = scroller.value;
  const ta = taRef.value;
  if (!el) return;
  scrollTop.value = el.scrollTop;
  if (ta) {
    // 外层容器滚动时同步 textarea 自身的滚动位置，保证光标可见
    ta.scrollTop = el.scrollTop;
    ta.scrollLeft = el.scrollLeft;
  }
}

function updateCursor() {
  const ta = taRef.value;
  if (!ta) return;
  const upto = ta.value.slice(0, ta.selectionStart);
  const nl = upto.split("\n");
  cursorLine.value = nl.length;
  cursorCol.value = (nl[nl.length - 1]?.length ?? 0) + 1;
}

/** 用 execCommand 插入文本，保留浏览器原生撤销栈 */
function insert(text: string) {
  const ta = taRef.value;
  if (!ta) return;
  ta.focus();
  if (!document.execCommand("insertText", false, text)) {
    const s = ta.selectionStart;
    const e = ta.selectionEnd;
    const v = ta.value.slice(0, s) + text + ta.value.slice(e);
    emit("update:modelValue", v);
    nextTick(() => {
      ta.selectionStart = ta.selectionEnd = s + text.length;
      updateCursor();
    });
  }
}

function onKeydown(e: KeyboardEvent) {
  const ta = taRef.value;
  if (!ta) return;
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
    e.preventDefault();
    emit("save");
    return;
  }
  if (e.key === "Tab") {
    e.preventDefault();
    insert(INDENT);
    return;
  }
  if (e.key === "Enter" && !e.ctrlKey && !e.metaKey && !e.shiftKey) {
    // 继承当前行的缩进
    const upto = ta.value.slice(0, ta.selectionStart);
    const cur = upto.split("\n").pop() ?? "";
    const m = cur.match(/^[ \t]*/);
    if (m && m[0]) {
      e.preventDefault();
      insert("\n" + m[0]);
    }
  }
}

// ---- 供右键菜单调用的编辑操作 ----

function selection(): string {
  const ta = taRef.value;
  if (!ta) return "";
  return ta.value.slice(ta.selectionStart, ta.selectionEnd);
}

async function copySelection() {
  const text = selection();
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    void document.execCommand("copy");
  }
}

function cutSelection() {
  const ta = taRef.value;
  if (!ta) return;
  void copySelection();
  if (!document.execCommand("delete")) {
    const s = ta.selectionStart;
    const e = ta.selectionEnd;
    const v = ta.value.slice(0, s) + ta.value.slice(e);
    emit("update:modelValue", v);
    nextTick(() => {
      ta.selectionStart = ta.selectionEnd = s;
      updateCursor();
    });
  }
}

async function pasteClipboard() {
  try {
    const text = await navigator.clipboard.readText();
    if (text) {
      insert(text);
      return;
    }
  } catch {
    /* 剪贴板读取不可用，回退到 execCommand */
  }
  document.execCommand("paste");
}

function selectAll() {
  const ta = taRef.value;
  if (!ta) return;
  ta.focus();
  ta.select();
  updateCursor();
}

defineExpose({
  focus: () => taRef.value?.focus(),
  copySelection,
  cutSelection,
  pasteClipboard,
  selectAll,
  /** 是否存在选区，用于启用/禁用复制与剪切 */
  hasSelection: () => selection().length > 0,
  /** 滚动到指定行（1 开始） */
  gotoLine: (n: number) => {
    const el = scroller.value;
    if (!el) return;
    el.scrollTop = Math.max(0, (n - 1) * LINE_H - 40);
    syncScroll();
  },
});

watch(
  () => props.filename,
  () => {
    scrollTop.value = 0;
    if (scroller.value) {
      scroller.value.scrollTop = 0;
      scroller.value.scrollLeft = 0;
    }
    requestAnimationFrame(() => taRef.value?.focus());
  }
);

onMounted(() => {
  updateCursor();
});
</script>

<template>
  <div class="ed" :class="{ focus: focused }">
    <div class="ed-gutter" aria-hidden="true">
      <div class="ed-gutter-inner" :style="{ transform: `translateY(${-scrollTop}px)` }">
        <div
          v-for="n in lines.length"
          :key="n"
          class="ed-ln"
          :class="{ cur: n === cursorLine }"
        >
          {{ n }}
        </div>
      </div>
    </div>

    <div ref="scroller" class="ed-scroll" @scroll="onScroll">
      <div class="ed-inner">
        <pre class="ed-hl" aria-hidden="true"><code v-html="highlighted"></code></pre>
        <textarea
          ref="taRef"
          class="ed-input"
          :value="modelValue"
          :readonly="readonly"
          spellcheck="false"
          autocomplete="off"
          autocapitalize="off"
          wrap="off"
          @input="onInput"
          @scroll="onScroll"
          @keydown="onKeydown"
          @click="updateCursor"
          @keyup="updateCursor"
          @select="updateCursor"
          @focus="
            focused = true;
            updateCursor();
          "
          @blur="focused = false"
          @contextmenu.prevent="
            emit('contextmenu', { x: $event.clientX, y: $event.clientY })
          "
        ></textarea>
      </div>
    </div>

    <div class="ed-status">
      <span class="ed-lang">{{ langLabel(ext) }}</span>
      <span>行 {{ cursorLine }}，列 {{ cursorCol }}</span>
      <span>{{ lines.length }} 行</span>
      <span class="ed-tip">Ctrl+S 保存 · Tab 缩进</span>
    </div>
  </div>
</template>

<style scoped>
.ed {
  position: relative;
  display: flex;
  flex: 1;
  min-height: 0;
  overflow: hidden;
  background: rgba(0, 0, 0, 0.22);
  border-radius: 0 0 13px 13px;
  user-select: text;
  -webkit-user-select: text;
}

.ed-gutter {
  width: 56px;
  flex-shrink: 0;
  overflow: hidden;
  padding: 10px 0 22px;
  background: rgba(255, 255, 255, 0.025);
  border-right: 1px solid var(--border);
  text-align: right;
}
.ed-gutter-inner {
  will-change: transform;
}
.ed-ln {
  height: 20px;
  line-height: 20px;
  padding-right: 10px;
  font-family: "Cascadia Code", Consolas, "Courier New", monospace;
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  color: var(--text-3);
  opacity: 0.55;
}
.ed-ln.cur {
  color: var(--accent);
  opacity: 1;
}

.ed-scroll {
  position: relative;
  flex: 1;
  min-width: 0;
  overflow: auto;
  padding-bottom: 22px;
}

.ed-inner {
  position: relative;
  width: max-content;
  min-width: 100%;
  min-height: 100%;
}

.ed-hl,
.ed-input {
  margin: 0;
  padding: 10px 14px 0;
  font-family: "Cascadia Code", Consolas, "Courier New", monospace;
  font-size: 13px;
  line-height: 20px;
  letter-spacing: 0;
  tab-size: 2;
  white-space: pre;
  word-break: normal;
  overflow-wrap: normal;
  border: none;
}

.ed-hl {
  display: block;
  color: var(--text-1);
  pointer-events: none;
}
.ed-hl code {
  font: inherit;
}

.ed-input {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  resize: none;
  outline: none;
  overflow: hidden;
  background: transparent;
  color: transparent;
  -webkit-text-fill-color: transparent;
  caret-color: var(--accent);
}
.ed-input::selection {
  background: rgba(232, 154, 75, 0.32);
}

.ed-status {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 22px;
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 0 12px;
  font-size: 11px;
  color: var(--text-3);
  background: var(--panel);
  border-top: 1px solid var(--border);
}
.ed-lang {
  color: var(--accent);
  font-weight: 600;
}
.ed-tip {
  margin-left: auto;
  opacity: 0.7;
}

/* v-html 生成的内容需要 :deep 才能命中 */
.ed :deep(.tk-key) {
  color: #9cdcfe;
}
.ed :deep(.tk-str) {
  color: #ce9178;
}
.ed :deep(.tk-com) {
  color: #6a9955;
  font-style: italic;
}
.ed :deep(.tk-sec) {
  color: #e8a33d;
  font-weight: 600;
}
.ed :deep(.tk-kw) {
  color: #569cd6;
}
.ed :deep(.tk-num) {
  color: #b5cea8;
}
</style>
