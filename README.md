<div align="center">

<img src="src-tauri/icons/128x128.png" alt="QookiX Launcher" width="128" />

# QookiX Launcher

**一款免费、纯净、无广告的 Minecraft 启动器**

支持 Modrinth / CurseForge 双内容中心，模组、整合包、光影、资源包一键安装与升级。

</div>

## 为什么选择它？

- 🧹 **纯净无广告**：没有弹窗、没有充值入口、没有遥测，数据只保存在你自己电脑上
- 🚀 **开箱即用**：自动检测 Java、多线程下载，从零到进游戏只需几步
- 🎨 **多实例管理**：一个启动器管理多套"游戏 + 模组"组合，互不干扰
- 🧩 **海量内容**：内置 Modrinth 与 CurseForge，模组 / 整合包 / 光影 / 资源包随便装
- 🔄 **一键升级**：已装内容自动检查更新，点一下就能升到最新版
- 👤 **账号灵活**：微软正版账号（带皮肤）或离线账号，随时切换

## 快速上手

### 1. 添加账号

点击**左下角的账号栏**：

- 想马上玩 → 添加 **离线账号**，起个名字就能进游戏
- 有正版 → 使用**微软账号登录**（设备码流程，正版皮肤可用）

添加后把它设为「当前游玩账号」即可。

### 2. 创建游戏实例

进入 **游戏实例 → 新建实例**，选择：

- Minecraft 版本（旧版如 1.8 / 1.12.2，新版如 1.21 都支持）
- 加载器：**原版 / Fabric / Quilt / Forge / NeoForge**

点创建，启动器会自动下载游戏文件并安装，无需手动折腾。

### 3. 安装内容

在**内容中心**搜索你想要的模组 / 整合包 / 资源包 / 光影，选好版本一键安装到指定实例。

> 整合包支持 Modrinth 的 `.mrpack` 与 CurseForge 格式，装上即玩。

### 4. 启动游戏

选中实例点「开始」，首次启动会自动下载依赖。之后想更新内容？在**实例详情**里点「检查更新」→「一键升级」。

## 常用设置

在 **设置** 页面可以调整：

| 设置 | 说明 |
| --- | --- |
| Java | 自动检测，或手动指定任意路径 |
| 内存 | 最大 / 最小内存分配（建议至少给 4 GB） |
| JVM / 游戏参数 | 高级玩家可自定义启动参数 |
| 下载线程数 | 默认 8，网络好可以调高加速下载 |
| 主题 | 深色 / 浅色 |
| 关闭行为 | 点关闭是退出还是最小化到后台 |
| 数据目录 | 查看 / 修改所有游戏数据的存放位置 |

## 数据目录

默认位置：`%APPDATA%\QookiX-Launcher`

- `instances/` — 所有游戏实例（各自的模组、配置、存档）
- `versions/`、`libraries/`、`assets/` — 游戏本体与依赖
- `logs/` — 游戏日志
- `settings.json` — 启动器设置

可在「设置」中查看或修改。

## 声明

- 本项目与 Mojang / Microsoft 无任何关联，Minecraft 为 Microsoft 的商标
- 架构参考 [Modrinth Theseus](https://github.com/modrinth/code)（EUPL-1.2），本实现为独立重写
- 请在遵守各平台条款的前提下使用内容服务（Modrinth / CurseForge）

---

### 开发者 / 贡献

技术栈：Tauri 2 + Vue 3 + Rust。开发运行：

```bash
npm install
npm run tauri dev
```

打包发布：

```bash
npm run tauri build
```

产物位于 `src-tauri/target/release/bundle/`。
