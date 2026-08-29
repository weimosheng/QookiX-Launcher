#!/usr/bin/env node
/**
 * 统一版本号脚本。
 * 用法：node scripts/update-version.mjs 0.3.0
 * 或：  npm run version -- 0.3.0
 *
 * 会将新版本号同步到以下文件：
 *  - package.json            (前端版本)
 *  - src-tauri/Cargo.toml    (Rust 包版本)
 *  - src-tauri/tauri.conf.json (应用版本)
 *  - src/views/SettingsView.vue (关于页面 "vX.Y.Z")
 */
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");

// ---- 解析新版本号 ----
const arg = process.argv[2];
if (!arg || !/^\d+\.\d+\.\d+/.test(arg)) {
  console.error("用法: node scripts/update-version.mjs <版本号，如 0.3.0>");
  process.exit(1);
}
const newVersion = arg.replace(/^v/, "");

// ---- 当前版本来源（package.json）----
const pkgPath = join(root, "package.json");
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
const oldVersion = pkg.version;
console.log(`版本号: ${oldVersion} -> ${newVersion}`);

// ---- 逐个更新 ----
function patch(file, replaceFn) {
  const path = join(root, file);
  let content = readFileSync(path, "utf8");
  const next = replaceFn(content);
  if (next !== content) {
    writeFileSync(path, next);
    console.log(`  已更新 ${file}`);
  } else {
    console.log(`  (未匹配) ${file}`);
  }
}

// 1. package.json（保留格式）
pkg.version = newVersion;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
console.log("  已更新 package.json");

// 2. Cargo.toml 顶层 [package] version
patch("src-tauri/Cargo.toml", (c) =>
  c.replace(/^(version\s*=\s*")[^"]+(")/m, `$1${newVersion}$2`),
);

// 3. tauri.conf.json 顶层 version
patch("src-tauri/tauri.conf.json", (c) =>
  c.replace(/^(\s*"version"\s*:\s*")[^"]+(")/m, `$1${newVersion}$2`),
);

// 4. SettingsView.vue 的 "vX.Y.Z"
patch("src/views/SettingsView.vue", (c) =>
  c.replace(/v\d+\.\d+\.\d+/g, `v${newVersion}`),
);

console.log("完成。");
