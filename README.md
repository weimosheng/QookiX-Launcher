<div align="center">

<img src="src-tauri/icons/128x128.png" alt="QookiX Launcher" width="128" />

# QookiX Launcher

**一款免费、纯净、无广告的 Minecraft 启动器**

支持 Modrinth / CurseForge 双内容中心，模组、整合包、光影、资源包一键安装与升级。

</div>

## 为什么选择它？

- **纯净无广告**：没有弹窗、没有充值入口、没有遥测，数据只保存在你自己电脑上
- **开箱即用**：自动检测 Java、多线程下载，从零到进游戏只需几步
- **多实例管理**：一个启动器管理多套"游戏 + 模组"组合，互不干扰
- **海量内容**：内置 Modrinth 与 CurseForge，模组 / 整合包 / 光影 / 资源包随便装
- **跨平台体验**：支持 Windows、macOS、Linux及各类主流发行版本

## 声明

### 非官方项目

- 本项目是一个 **独立的第三方开源项目**，与 Mojang Studios、Microsoft Corporation、Modrinth、CurseForge 或任何第三方均**无任何隶属、合作或背书关系**
- 不使用官方启动器的任何专有代码或资源
- 本项目的开发和分发遵守 [Mojang Studios 的 Minecraft 使用准则](https://www.minecraft.net/zh-hans/usage-guidelines)，不包含、分发或修改任何 Minecraft 的专有代码或受保护资产

### 商标与版权

- Minecraft 是 Microsoft Corporation 的商标；Mojang、Modrinth、CurseForge 均为其各自所有者的商标或版权
- 本项目不主张对这些名称、商标或版权的任何权利，使用它们仅用于描述兼容性

### 内容与下载

- 本启动器仅提供获取游戏本体及 MOD / 资源包等内容的**下载与安装工具**，本身不托管、不存储任何游戏文件或第三方内容
- 游戏客户端来自 Mojang / Microsoft 官方渠道；模组、资源包等内容来自 Modrinth、CurseForge 等第三方平台，其版权归各自创作者所有
- 请确保你有权使用相关内容（例如拥有 Minecraft 账号与正版授权）
- 用户需自行遵守各平台的服务条款（Microsoft 服务条款、Modrinth 政策、CurseForge 使用条款等）

### 账号与数据

- 微软账号登录仅在你本人打开的重定向页面或设备码流程中授权，令牌仅保存在你本地
- 本启动器不采集、不上传任何个人数据或使用遥测

### 免责条款

- 本项目按“原样”提供，不附带任何明示或默示担保
- 在法律允许的最大范围内，作者与贡献者不对因使用本项目造成的任何损失承担责任
- 使用本项目即表示你已阅读并同意上述声明

## 参考致谢

- 界面图标改编自 [Feather Icons](https://feathericons.com/)（MIT License，Copyright (c) 2013-2017 Cole Bemis）
- 本项目使用的 Feather Icons 遵循其 MIT 许可证要求，已保留其版权声明
- Windows 徽标等品牌图标版权归其各自所有者，仅在本项目中用于兼容性展示

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
