# QookiX Launcher 项目交接文档

> 交接时间：2026-09-08 ｜ 当前版本：**v0.5.13** ｜ 仓库：https://github.com/weimosheng/QookiX-Launcher
> 面向：接手开发的同学。读完本文应能独立跑起来、改代码、打包发布、定位常见故障。

---

## 1. 项目概览

一款免费、纯净、无广告的 Minecraft 第三方启动器。核心能力：多实例管理、Modrinth / CurseForge 内容浏览与安装、整合包安装、自动 Java 检测与下载、联机房间（Terracotta）、崩溃日志分析、世界备份与实例分享导入导出、应用自更新。

**技术栈**

| 层 | 技术 | 说明 |
|---|---|---|
| 桌面壳 | Tauri 2（Rust） | `identifier: cn.swkj1.qookix-launcher`，Windows 主打，同时支持 macOS / Linux |
| 后端 | Rust 2021 | `src-tauri/src`，114 个 Tauri 命令，约 1.5 万行 |
| 前端 | Vue 3 + TypeScript + Vite | `src`，40 个 `.vue` + 33 个 `.ts`，naive-ui 组件库 |
| 数据 | 本地 JSON 文件 | 无数据库，实例/设置/账号全部落盘在应用数据目录 |

**关键第三方**：`reqwest`（system-proxy / socks / rustls）、`tokio`、`zip`、`tauri-plugin-updater`（自更新）、`tauri-plugin-deep-link`、`tauri-plugin-dialog/opener/process`。

---

## 2. 环境与构建

### 依赖
- Node.js（前端）+ Rust 工具链 + Tauri 系统依赖（WebView2 等）
- `npm install`

### 常用命令

```bash
npm run dev              # 只起 vite（前端热更）
npm run tauri dev        # 完整开发模式（经 scripts/run-tauri.mjs 包装）
npm run build            # 仅构建前端 dist（vue-tsc 类型检查 + vite build）
npm run tauri build      # 打包完整安装包
npm run version          # 同步三处版本号（见下）
```

### ⚠️ 构建第一大坑：dist 不会自动重建

`npm run tauri build` **不会**重新构建前端，它只把已有的 `dist/` 打进二进制。**改了 Vue/TS 后必须先 `npm run build`**，否则打包进去的是几天前的旧前端。

本次交接前就踩过：exe 是当天构建的，但 `dist/` 停留在两天前，导致改了 `InstallDialog.vue` 却怎么都测不出效果，排查耗掉大量时间。

```bash
npm run build && npm run tauri build    # 正确姿势（前端有改动时）
```

### 版本号与发布
- 版本号在三处，必须一致：`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml`，用 `npm run version` 统一改。
- 构建脚本 `scripts/run-tauri.mjs`：Windows 下会先构建 `installer-ui` crate（WebView2 安装器界面）并注入 `QOOKIX_INSTALLER_UI` 给 NSIS；设了 `QOOKIX_CERT_PFX` 时用 `scripts/sign-windows.ps1` 对主程序 / NSIS / MSI 签名。
- 更新器需要 `TAURI_SIGNING_PRIVATE_KEY`（缺失时构建末段会报 “public key found but no private key”，**不影响 exe 产物**，只是无法生成更新签名）。
- CI：`.github/workflows/build.yml`；发布相关脚本在 `scripts/`（`create-update-manifest.mjs`、`generate-release-notes.mjs`、`verify-artifacts.mjs` 等）。
- 打包产物：`src-tauri/target/release/bundle/`（nsis / msi）。

---

## 3. 目录结构

### 后端 `src-tauri/src`

| 模块 | 行数 | 职责 |
|---|---|---|
| `commands/` | ~2900 | **命令层**，按域拆 12 个子模块：`instances` `browse` `settings` `files` `skins` `accounts` `crash` `hosted` `multiplayer` `storage` `version` `world_backup`；`mod.rs` 统一 `pub use` 导出 |
| `install.rs` | 1135 | 游戏安装：版本 json 补丁（Fabric/Quilt/Forge/NeoForge）、库下载、natives 解压、assets、Forge processors |
| `instances.rs` | 1143 | 实例 CRUD、分组、游玩时长、已装内容记录 |
| `launch.rs` | 1040 | 启动参数拼装、Java 选择、进程管理、日志/退出事件 |
| `crash.rs` | 1384 | 崩溃日志解析与诊断 |
| `servers.rs` | 1141 | 联机房间 / Terracotta 集成 |
| `curseforge.rs` | 776 | CurseForge API + 内容安装 + 整合包安装 |
| `modrinth.rs` | 762 | Modrinth API + 内容安装 + 整合包安装 |
| `download.rs` | 703 | 下载引擎：并发、分片、断点续传、重试、镜像回退 |
| `util.rs` | 775 | 通用工具（zip、图标、哈希、`fs_best_effort`、`log_line`） |
| `models.rs` | 704 | 数据结构与 serde 定义（含 `Settings`、`Instance`、`VersionJson`） |
| `java.rs` `mcmeta.rs` `mirror.rs` `mcping.rs` `mcmod.rs` `modpack.rs` `state.rs` `settings.rs` `storage.rs` `accounts.rs` `terracotta.rs` `updater.rs` `world_backup.rs` `instance_share.rs` `paths.rs` `pins.rs` | — | 各领域逻辑 |

`lib.rs` 是入口：模块声明、`AppState` 初始化、插件注册、`generate_handler!` 注册全部 114 条命令。

### 前端 `src`

- `views/`（11）：`Home` `Instances` `InstanceDetail` `Browse`（内容中心）`Downloads`（下载中心）`Multiplayer` `ServerDetail` `News` `Settings` `Skin` `CreateInstance`
- `components/`（25）：含 `instance/`（ContentTab / SavesTab / SettingsTab 等已从 InstanceDetailView 拆出）、`InstallDialog`、`ExportDialog`、`LaunchProgress`、`PlaytimeCard`
- `stores/`（7）：pinia —— `instances` `tasks` `settings` `accounts` `servers` `news` `pins`
- `api.ts`：**所有后端调用的封装**，统一的 `invoke<T>(cmd, args, { silent })` 包装（默认触发顶部加载条）
- `composables/`（7）、`utils/`（7，含 `format.ts` 的 `fmtCount`/`fmtDate` 等统一格式化）

---

## 4. 架构与核心机制

### 4.1 命令层约定
- `api.ts` 的 `invoke` 包装：默认触发顶部加载条（`trackStart`/`trackEnd`）。**长任务或自带进度反馈的命令必须显式传 `{ silent: true }`**，否则顶部条会和下载中心进度重复。
- 命令参数一律 camelCase，Rust 侧 snake_case，Tauri 自动转换。

### 4.2 事件协议（前后端主通道）

| 事件 | 方向 | 用途 |
|---|---|---|
| `install://progress` | 后端→前端 | 安装阶段进度（`stage`: manifest / client / loader / forge-processors / libraries / natives / assets / modpack / **done**） |
| `download://progress` | 后端→前端 | 文件/字节级下载进度 |
| `launch://progress` | 后端→前端 | 启动步骤（10 登录 → 25 账号 → 40 Java → 60 参数 → 80 进程 → 100 成功） |
| `launch://log` / `launch://exit` / `launch://crash` / `launch://state` / `launch://pid` | 后端→前端 | 游戏输出、进程退出、崩溃、状态、pid |
| `import://progress` / `import://warning` | 后端→前端 | 分享包导入 |
| `server://log` / `server://state` / `server://error` | 后端→前端 | 联机服务器 |
| `terracotta://state` | 后端→前端 | 陶瓦联机状态 |
| `share-import://game-install-failed` | 后端→前端 | 分享导入后装游戏失败通知 |

前端统一在 `stores/tasks.ts` 与组件里 `listen`。**下载中心的任务卡片完全由这两条事件驱动**——任务以 `stage == "done"` 作为归档信号。

### 4.3 数据目录（默认 `%APPDATA%\QookiX-Launcher`，可在设置里改）

```
settings.json      # 全部设置（含 CF API Key、代理、镜像）
instances/<id>/    # 实例目录（instance.json + mods/ + saves/ + config/ + natives/ ...）
versions/          # 各实例打过补丁的 version json 与 client jar
libraries/         # maven 依赖库（公共）
assets/            # 资源索引与对象
runtimes/          # Java 运行时、Forge installer、下载的整合包
logs/              # 游戏日志
skins/ backgrounds/ game-icons/
playtime.json      # 按天游玩时长
```

### 4.4 下载子系统（`download.rs`）
- 并发文件数 `download_threads`（默认 8），单文件分片 `download_chunk_threads`（默认 4），≥8MB 且服务端支持 Range 才分片。
- 断点续传：写 `.part` 临时文件，失败按已有大小续传；换源（镜像→官方）时清空重来。
- 重试：单文件最多 3 次，HTTP 4xx/5xx 视为永久性错误不再重试。
- 镜像：`mirror.rs`，`official` / `bmclapi` / `custom`，镜像拉取失败自动回退官方。
- 代理：`proxy_mode` = `system`（默认，reqwest 系统代理）/ `direct`（no_proxy）/ `custom`（显式 URL；**URL 为空时等价于走系统代理**）。

### 4.5 安装子系统（`install.rs`）
- 原版 json → 按 loader 打补丁：`fabric_patch` / `quilt_patch` / `forge_patch`（Forge 与 NeoForge 共用，`is_neoforge` 区分）。
- Forge 两种安装器格式：
  - **旧版（≤1.16）**：`install_profile.json` 顶层 `versionInfo` 就是版本 json，库表完整但**不带 `downloads` 元数据**（只有 maven `url` 基址 + 坐标）。
  - **新版（≥1.17）**：版本信息在 zip 内独立 `version.json`，需跑 `processors` 任务链生成打过补丁的 forge client jar（`run_forge_processors`，需要 Java）。
- natives：新旧两种写法都支持（旧式 `natives` map + classifiers；新式 `...:natives-<os>` 独立条目），解压后 `flatten_natives` 把深层目录的 dll 拍平到 `natives/` 根，并把 JVM 参数里的 `${natives_directory}/xxx` 归一到根目录。
- `dedupe_libraries` **不能只按 name 去重**——旧版本同一坐标会同时出现“普通构件”和“带 natives/extract 的构件”，必须按 `(name, 是否 natives)` 保留两条。

### 4.6 启动子系统（`launch.rs`）
- Java 选择 → 版本 json → classpath → 参数拼装（同时支持新版 `arguments` 与老版 `minecraftArguments`）→ 启动进程 → 转发 stdout/stderr 为 `launch://log`。
- 启动前强校验 `natives` 目录存在，缺则报「缺少 natives 目录，请重新安装游戏」。

### 4.7 其它子系统
- **内容平台**：`modrinth.rs` / `curseforge.rs`，统一产出 `ProjectHit` / `ProjectVersion` 结构供前端渲染；CurseForge 需要 API Key（设置里填，或编译期 `CURSEFORGE_API_KEY`）。
- **账号**：离线 + 微软设备码/OAuth（`accounts.rs`），token 仅本地保存，日志中脱敏。
- **崩溃分析**：`crash.rs` 解析日志给出中文诊断。
- **联机**：`servers.rs` + `terracotta.rs`，下载未修改的 Terracotta 二进制作为独立进程运行（AGPL 例外，见 README 声明）。
- **世界备份 / 实例分享**：`world_backup.rs` + `instance_share.rs`（导出 `.qkxinst`，导入时还原 worlds/config）。
- **自更新**：`updater.rs`，源可切 `bucket` / `github`。

---

## 5. 开发约定（改代码前必读）

1. **best-effort 兜底**：不要写裸的 `let _ = ...`。成功/失败都要留痕：文件系统操作用 `util::fs_best_effort(op, path, result)`，领域函数用 `util::log_best_effort(what, result)`。
2. **路径安全**：任何来自 API / 文件名的路径都要过 `validate_instance_id` / `is_safe_filename`，防目录穿越。
3. **前端格式化统一**：数字/日期统一用 `utils/format.ts`（`fmtCount` / `fmtDate` / `fmtBytes`），不要在组件里各写一个 `fmt`。
4. **样式**：组件内用 `<style scoped>`；需要穿透到 teleport 出去的弹窗（如模态框）时用全局 `<style>` 并加前缀类名（如 `.id-`）。
5. **拆分偏好**：单文件过长就拆（InstanceDetailView 已从 2400+ 行拆出 ContentTab / SavesTab / SettingsTab），新代码优先拆小组件而不是堆大文件。

---

## 6. 安全现状（接手后需要正视）

- ✅ 路径穿越防护、发送前端前剥离 token、日志脱敏 access token，这些都已到位。
- ⚠️ **`models.rs` 里 token 的“加密”实际只是 Base64 编码**（`encode_token` / `decode_token`），属于安全伪善，应改为系统密钥链或真正的加密存储。
- ⚠️ CurseForge API Key 明文存在 `settings.json`。

---

## 7. 当前工作状态（交接时的重要事实）

### 未提交（`git status`）
修改 18 个文件、新增 5 个文件。主要包括：
- 后端：`install.rs`（natives 老式 json 兼容、任务 done 收尾）、`curseforge.rs`（整合包安装日志）、`commands/instances.rs`（启动失败收尾）、`commands/settings.rs`（`log_debug`）、`util.rs`（`log_line`）、`lib.rs`、`instances.rs`、`launch.rs`、`browse.rs`、`Cargo.toml`（dev-dependencies：`tauri` test feature）
- 前端：`InstallDialog.vue`（按钮移入内容区 + fire-and-forget）、`App.vue`、`api.ts`、`ContentTab.vue`、`SavesTab.vue`、`InstanceDetailView.vue`、`InstancesView.vue`、`types.ts`、`utils/format.ts`
- 新增：`src-tauri/src/world_backup.rs`、`instance_share.rs`、`commands/world_backup.rs`、`src/components/PlaytimeCard.vue`、`components/instance/ExportDialog.vue`

### 本轮（交接前）已修复的问题
1. **CurseForge 整合包“点了没反应”** — 根因是安装对话框的「一键安装」按钮在 `#footer` 插槽里**没渲染出来**，用户实际点到的是卡片/遮罩。已把按钮移入内容区。修复后实测 RLCraft 全链路 83 秒装完（51MB 整包 → 187 个 manifest 条目 → 177 个模组 → 解压 overrides → 自动装 Forge 1.12.2）。
2. **1.12.2 老版本缺 natives** — 老式版本 json 无 `downloads` 元数据，natives 条目被静默丢弃。已加 maven `url` 基址回退。
3. **下载中心僵尸任务** — `install_game` 各子阶段（客户端/加载器/Forge 依赖/每个 processor 步骤）只发进度不发 `done`，进度跑满仍显示“进行中”。已全部补发 done。
4. **启动失败后悬浮进度卡卡死** — `launch://exit` 只在进程退出时发，启动中途失败无人收尾。已在 `launch_instance` 失败路径补发。
5. **`[fs] remove_file 找不到文件` 噪音** — `fs_best_effort` 现把 `NotFound` 视为正常，静默跳过。
6. **诊断能力** — 新增 `util::log_line`（写 `%TEMP%/qookix-install-debug.log`）与 `log_debug` 命令。GUI 应用没有控制台，`eprintln!` 用户看不见，排查线上/打包后问题一律落盘。

### 已知待办（按优先级）
1. **【高】整合包安装逻辑三份重复**（`curseforge::install_modpack` / `modrinth::install_modpack` / `instance_share::import_pack`），同样的“下载包 → 解析 → 建实例 → 下载内容 → 解压 overrides → install_game”链写了三遍，改一处漏两处。应抽到 `modpack.rs` 的统一入口，三者只提供数据源差异。
2. **【高】`install_game` 的 taskId 管理分散**（4 处各开各的），应收敛为一个任务上下文对象，谁开谁负责发 done。
3. **【中】token 伪加密**（见第 6 节）。
4. **【中】Phase 1 元数据串行**：CF 整合包逐个拉取每个 mod 的文件元数据（RLCraft 187 个约 65 秒），可并发化。
5. **【低】清理临时诊断代码**：`log_debug` 命令、`download.rs` 里的 `diag` 诊断测试（`#[ignore]`）、前端的 `[fe]` 埋点，确认稳定后可移除。
6. **【低】CF 整合包安装的 Forge 1.12.2 完整验证**：natives 修复后需实机启动确认。

---

## 8. 排障手册

**通用第一步**：看 `%TEMP%\qookix-install-debug.log`——所有关键链路都往这里写。

| 现象 | 排查方向 |
|---|---|
| 点了安装没反应 | 先看日志有没有 `[fe] 一键安装按钮被点击` → `[install_content]` → `[cf_modpack]`。没有前端日志=事件没触发；有前端无后端=invoke 参数不匹配；有 `[cf_modpack]`=看停在哪一步 |
| 改了前端却没有效果 | **先确认 `npm run build` 跑过**（见第 2 节大坑） |
| 缺 natives 目录 | 老版本 json 兼容性问题，看 `install.rs` 的 natives 分支是否覆盖该库格式 |
| 下载中心任务卡在“进行中” | 该任务没收到 `stage == "done"`，检查对应代码路径是否补发 |
| CurseForge 全平台无结果 / 401 | API Key 是否填写生效（新 key 需等几分钟）；`proxy_mode` 是否误设为 `direct` |
| 启动失败但界面卡住 | 检查失败路径是否补发了收尾事件（`launch://exit`） |
| 网络问题定位 | `cargo test diag_cf_modpack_download -- --ignored --nocapture`（在 `download.rs` 里，实测 CDN 下载与 API 请求，分别走 system 代理与直连） |

**测试**：`cargo test`（`lib.rs` 的 `smoke` 模块是网络相关的真实请求测试，需联网；`download.rs` 的诊断测试标了 `#[ignore]`）。

---

## 9. 上手建议路径

1. 跑起来：`npm install && npm run build && npm run tauri dev`
2. 读 `src-tauri/src/lib.rs`（命令全景）→ `commands/instances.rs`（最典型的命令实现）→ `install.rs` + `launch.rs`（核心业务）
3. 前端读 `api.ts`（命令清单）→ `stores/tasks.ts`（事件驱动的任务模型）→ `views/InstanceDetailView.vue`（最大的页面，已拆分）
4. 动手改前先跑一次 `cargo test` 与 `npm run build`，确认基线是绿的
