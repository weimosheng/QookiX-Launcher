/**
 * 极简语法高亮，覆盖 Minecraft 实例目录里常见的文本格式：
 * JSON / Properties / TOML / YAML / 日志 等。
 *
 * 只做词法着色（字符串、注释、数字、关键字、键名、小节标题），
 * 不做语法校验，保证对任意内容都不会出错。
 */

// 匹配顺序很重要：字符串必须排在注释之前，否则 "#" 会被误判为注释。
const TOKEN_RE = new RegExp(
  [
    /"(?:\\.|[^"\\])*"(?=\s*:)/.source, // 1 键名
    /"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'/.source, // 2 字符串
    /#[^\n]*|\/\/[^\n]*|\/\*[\s\S]*?\*\//.source, // 3 注释
    /(?:^|\n)\[[^\]\n]*\]/.source, // 4 小节标题 [section]（含前导换行）
    /\b(?:true|false|null|yes|no|on|off)\b/.source, // 5 关键字
    /\b-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b/.source, // 6 数字
  ]
    .map((s) => `(${s})`)
    .join("|"),
  "gm"
);

const CLASSES = ["tk-key", "tk-str", "tk-com", "tk-sec", "tk-kw", "tk-num"];

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** 超过这个体量就跳过着色，避免超大文件卡顿。 */
const MAX_HIGHLIGHT_LEN = 200_000;

export function highlight(src: string): string {
  if (!src) return "";
  if (src.length > MAX_HIGHLIGHT_LEN) return esc(src);
  let out = "";
  let last = 0;
  TOKEN_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = TOKEN_RE.exec(src)) !== null) {
    if (m[0].length === 0) {
      TOKEN_RE.lastIndex++;
      continue;
    }
    const cls = CLASSES[m.slice(1).findIndex((g) => g !== undefined)];
    out += esc(src.slice(last, m.index));
    out += cls ? `<span class="${cls}">${esc(m[0])}</span>` : esc(m[0]);
    last = m.index + m[0].length;
  }
  out += esc(src.slice(last));
  return out;
}

const LANG_LABEL: Record<string, string> = {
  json: "JSON",
  json5: "JSON5",
  properties: "Properties",
  cfg: "Config",
  conf: "Config",
  config: "Config",
  toml: "TOML",
  yaml: "YAML",
  yml: "YAML",
  ini: "INI",
  xml: "XML",
  lang: "语言文件",
  mcmeta: "MCMeta",
  snbt: "SNBT",
  nbt: "NBT",
  js: "JavaScript",
  mjs: "JavaScript",
  ts: "TypeScript",
  lua: "Lua",
  py: "Python",
  sh: "Shell",
  bat: "批处理",
  log: "日志",
  txt: "纯文本",
  md: "Markdown",
  csv: "CSV",
  html: "HTML",
  css: "CSS",
};

/** 返回用于展示的语言名，未知扩展名返回大写的扩展名。 */
export function langLabel(ext: string): string {
  if (!ext) return "纯文本";
  return LANG_LABEL[ext] ?? ext.toUpperCase();
}
