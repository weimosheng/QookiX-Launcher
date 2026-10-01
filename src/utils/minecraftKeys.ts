/**
 * KeyboardEvent.code ↔ Minecraft 按键码（options.txt 的 key.*）互转：
 * MC 用 GLFW 键名，浏览器用物理键位。展示名里只有特殊键分中英文。
 */
import i18n from "../i18n";

export const UNBOUND = "key.keyboard.unknown";

/** 特殊键位：KeyboardEvent.code → MC 按键码 */
const SPECIAL: Record<string, string> = {
  Space: "key.keyboard.space",
  Enter: "key.keyboard.enter",
  NumpadEnter: "key.keyboard.keypad.enter",
  Tab: "key.keyboard.tab",
  Backspace: "key.keyboard.backspace",
  Delete: "key.keyboard.delete",
  Escape: "key.keyboard.escape",
  CapsLock: "key.keyboard.caps.lock",
  Insert: "key.keyboard.insert",
  Home: "key.keyboard.home",
  End: "key.keyboard.end",
  PageUp: "key.keyboard.page.up",
  PageDown: "key.keyboard.page.down",
  ArrowUp: "key.keyboard.up",
  ArrowDown: "key.keyboard.down",
  ArrowLeft: "key.keyboard.left",
  ArrowRight: "key.keyboard.right",
  Minus: "key.keyboard.minus",
  Equal: "key.keyboard.equal",
  BracketLeft: "key.keyboard.left.bracket",
  BracketRight: "key.keyboard.right.bracket",
  Backslash: "key.keyboard.backslash",
  Semicolon: "key.keyboard.semicolon",
  Quote: "key.keyboard.apostrophe",
  Backquote: "key.keyboard.grave.accent",
  Comma: "key.keyboard.comma",
  Period: "key.keyboard.period",
  Slash: "key.keyboard.slash",
  NumLock: "key.keyboard.num.lock",
  ScrollLock: "key.keyboard.scroll.lock",
  Pause: "key.keyboard.pause",
  PrintScreen: "key.keyboard.print.screen",
  ContextMenu: "key.keyboard.menu",
  ShiftLeft: "key.keyboard.left.shift",
  ShiftRight: "key.keyboard.right.shift",
  ControlLeft: "key.keyboard.left.control",
  ControlRight: "key.keyboard.right.control",
  AltLeft: "key.keyboard.left.alt",
  AltRight: "key.keyboard.right.alt",
  MetaLeft: "key.keyboard.left.win",
  MetaRight: "key.keyboard.right.win",
  NumpadAdd: "key.keyboard.keypad.add",
  NumpadSubtract: "key.keyboard.keypad.subtract",
  NumpadMultiply: "key.keyboard.keypad.multiply",
  NumpadDivide: "key.keyboard.keypad.divide",
  NumpadDecimal: "key.keyboard.keypad.decimal",
  NumpadEqual: "key.keyboard.keypad.equal",
};
for (let i = 0; i <= 9; i++) {
  SPECIAL[`Numpad${i}`] = `key.keyboard.keypad.${i}`;
}

/** KeyboardEvent.code → MC 按键码；识别不了返回 null（保持监听） */
export function codeToMc(code: string): string | null {
  if (SPECIAL[code]) return SPECIAL[code];
  let m = /^Key([A-Z])$/.exec(code);
  if (m) return `key.keyboard.${m[1].toLowerCase()}`;
  m = /^Digit(\d)$/.exec(code);
  if (m) return `key.keyboard.${m[1]}`;
  m = /^F(\d{1,2})$/.exec(code);
  if (m) return `key.keyboard.f${m[1]}`;
  return null;
}

/** MouseEvent.button → MC 按键码（侧键 4/5/6 是 1.16+ 的写法） */
export function mouseButtonToMc(button: number): string | null {
  switch (button) {
    case 0:
      return "key.mouse.left";
    case 1:
      return "key.mouse.middle";
    case 2:
      return "key.mouse.right";
    case 3:
      return "key.mouse.4";
    case 4:
      return "key.mouse.5";
    case 5:
      return "key.mouse.6";
    default:
      return null;
  }
}

/** 有中英文之分的按键名：[中文, English] */
const NAMED: Record<string, [string, string]> = {
  "key.keyboard.space": ["空格", "Space"],
  "key.keyboard.enter": ["回车", "Enter"],
  "key.keyboard.backspace": ["退格", "Backspace"],
  "key.keyboard.delete": ["删除", "Del"],
  "key.keyboard.caps.lock": ["大写锁定", "Caps Lock"],
  "key.keyboard.insert": ["插入", "Insert"],
  "key.keyboard.page.up": ["上一页", "Page Up"],
  "key.keyboard.page.down": ["下一页", "Page Down"],
  "key.keyboard.up": ["方向键 ↑", "Arrow Up"],
  "key.keyboard.down": ["方向键 ↓", "Arrow Down"],
  "key.keyboard.left": ["方向键 ←", "Arrow Left"],
  "key.keyboard.right": ["方向键 →", "Arrow Right"],
  "key.keyboard.left.shift": ["左 Shift", "L-Shift"],
  "key.keyboard.right.shift": ["右 Shift", "R-Shift"],
  "key.keyboard.left.control": ["左 Ctrl", "L-Ctrl"],
  "key.keyboard.right.control": ["右 Ctrl", "R-Ctrl"],
  "key.keyboard.left.alt": ["左 Alt", "L-Alt"],
  "key.keyboard.right.alt": ["右 Alt", "R-Alt"],
  "key.keyboard.left.win": ["左 Win", "L-Win"],
  "key.keyboard.right.win": ["右 Win", "R-Win"],
  "key.keyboard.num.lock": ["数字锁定", "Num Lock"],
  "key.keyboard.scroll.lock": ["滚动锁定", "Scroll Lock"],
  "key.keyboard.pause": ["暂停", "Pause"],
  "key.keyboard.print.screen": ["截屏键", "Print Screen"],
  "key.keyboard.keypad.enter": ["小键盘回车", "Keypad Enter"],
  "key.keyboard.keypad.add": ["小键盘 +", "Keypad +"],
  "key.keyboard.keypad.subtract": ["小键盘 −", "Keypad −"],
  "key.keyboard.keypad.multiply": ["小键盘 ×", "Keypad ×"],
  "key.keyboard.keypad.divide": ["小键盘 ÷", "Keypad ÷"],
  "key.keyboard.keypad.decimal": ["小键盘 .", "Keypad ."],
  "key.keyboard.keypad.equal": ["小键盘 =", "Keypad ="],
  "key.mouse.left": ["鼠标左键", "Mouse L"],
  "key.mouse.middle": ["鼠标中键", "Mouse M"],
  "key.mouse.right": ["鼠标右键", "Mouse R"],
  "key.mouse.4": ["鼠标侧键 4", "Mouse 4"],
  "key.mouse.5": ["鼠标侧键 5", "Mouse 5"],
  "key.mouse.6": ["鼠标侧键 6", "Mouse 6"],
  [UNBOUND]: ["未绑定", "Not bound"],
};
for (let i = 0; i <= 9; i++) {
  NAMED[`key.keyboard.keypad.${i}`] = [`小键盘 ${i}`, `Keypad ${i}`];
}

function isZh(): boolean {
  return i18n.global.locale.value === "zh-CN";
}

/** 按键码 → 展示名（W / F2 / 左 Shift / 鼠标左键…） */
export function mcKeyLabel(mc: string): string {
  const named = NAMED[mc];
  if (named) return isZh() ? named[0] : named[1];
  let m = /^key\.keyboard\.(.+)$/.exec(mc);
  if (m) return m[1].toUpperCase();
  m = /^key\.mouse\.(.+)$/.exec(mc);
  if (m) return `Mouse ${m[1]}`;
  return mc;
}
