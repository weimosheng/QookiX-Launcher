# 第三方组件声明 (Third-Party Notices)

本文件列出了 QookiX Launcher 所使用或所集成的第三方开源软件及其许可证信息，以满足相应开源许可协议的署名（attribution）要求。

## Terracotta（陶瓦联机）

- **项目**：Terracotta（陶瓦联机）
- **作者 / 版权**：Copyright © burningtnt
- **仓库**：https://github.com/burningtnt/Terracotta
- **许可证**：GNU Affero General Public License v3.0 or later（**AGPL-3.0-or-later**）
- **完整许可证文本**：https://github.com/burningtnt/Terracotta/blob/master/LICENSE
- **集成方式**：独立运行的外部进程

**使用说明**：QookiX Launcher 的「联机房间」功能使用 Terracotta 提供 NAT 穿透联机能力。本启动器仅负责下载 Terracotta 的**未修改官方二进制**并作为独立进程启动，再通过其**本地 HTTP 接口**（进程间通信）与之交互，**不将 Terracotta 静态或动态链接进本启动器**。

根据 Terracotta 许可证所附的 **AGPL 例外条款**，上述「打包未修改二进制 / 通过进程间通信接口交互」的使用方式不会导致本启动器被 AGPL 协议涵盖。作为该例外条款的条件之一，本启动器已在程序界面的「联机房间」页面明显处标识了 Terracotta 的版权信息。

Terracotta 与本启动器是两个相互独立、各自以自身许可证发布的开源项目，二者之间不存在任何隶属或背书关系。

---

## cubiomes

- **项目**：cubiomes
- **作者 / 版权**：Copyright © 2020 Cubitect
- **仓库**：https://github.com/Cubitect/cubiomes
- **许可证**：MIT License
- **完整许可证文本**：https://github.com/Cubitect/cubiomes/blob/master/LICENSE
- **集成方式**：Vendored 源码快照，静态链接进本启动器

**使用说明**：「工具箱 → 种子地图」的生物群系、结构与出生点计算由 cubiomes 提供。本启动器在 `src-tauri/vendor/cubiomes/` 保存其**上游未修改的源码快照**（含其 LICENSE 与 README），由 `src-tauri/build.rs` 编译，并通过自写的 C 胶水层（`src-tauri/src/cubiomes_bridge.c`）与 Rust 封装（`src-tauri/src/cubiomes.rs`）调用；本启动器未修改 cubiomes 本身。

cubiomes 采用 MIT 许可证，与本项目采用的 GNU GPL v3 兼容。cubiomes 与本启动器是两个相互独立、各自以自身许可证发布的开源项目，二者之间不存在任何隶属或背书关系。

### MIT License

```
MIT License

Copyright (c) 2020 Cubitect

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## Axolotl Launcher（仅交互设计参考）

- **项目**：Axolotl Launcher（美西螈启动器）
- **作者 / 版权**：Copyright © Mystic-Stars
- **仓库**：https://github.com/Mystic-Stars/Axolotl
- **许可证**：GNU General Public License v3.0 only（**GPL-3.0-only**，完整文本见本仓库根目录 `LICENSE`）
- **集成方式**：不引入其代码或二进制，仅参考其设计思路（Windows 安装脚本 `src-tauri/nsis/*.nsi` 的深色安装器 / 钩子写法、以及「工具箱 → 种子地图」瓦片式浏览的交互设计）

**使用说明**：本启动器未引入 Axolotl 的任何源码、二进制或资源；安装脚本与种子地图的配色、渲染、任务调度均为本启动器自行实现，相关思路参考已在 `src-tauri/nsis/hooks.nsi`、`src-tauri/nsis/installer.nsi` 与 `src/views/SeedMapView.vue` 文件头注明。Axolotl 与本启动器采用相同许可证（GPL-3.0-only）。二者是相互独立、各自发布的开源项目，不存在任何隶属或背书关系。

---

## Feather Icons

- **项目**：Feather Icons
- **作者 / 版权**：Copyright (c) 2013-2017 Cole Bemis
- **来源**：https://feathericons.com/
- **许可证**：MIT License
- **集成方式**：本启动器界面图标改编自 Feather Icons

### MIT License

```
MIT License

Copyright (c) 2013-2017 Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## Lucide Icons

- **项目**：Lucide
- **作者 / 版权**：Copyright (c) 2020 Lucide Contributors
- **来源**：https://lucide.dev/
- **许可证**：ISC License
- **完整许可证文本**：https://github.com/lucide-icons/lucide/blob/main/LICENSE
- **集成方式**：本启动器界面图标改编自 Lucide

### ISC License

```
ISC License

Copyright (c) for portions of project Lucide are held by Cole Bemis 2013-2017 as part of project Feather.
All other copyright (c) for project Lucide are held by Lucide Contributors 2020.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

---

## GitHub Octicons

- **项目**：GitHub Octicons（mark-github）
- **作者 / 版权**：Copyright © GitHub, Inc.
- **来源**：https://github.com/primer/octicons
- **许可证**：MIT License（完整文本见上方 Feather Icons 条目中的 MIT License 全文）
- **完整许可证文本**：https://github.com/primer/octicons/blob/main/LICENSE
- **集成方式**：本启动器界面中的 GitHub 标识图形改编自 Octicons

---

## 前端（npm）依赖

本启动器前端构建产物（Vue 3、naive-ui、skinview3d、marked、DOMPurify、driver.js 等）所使用的第三方库均为宽松许可证（MIT / Apache-2.0 / ISC / BSD 等），其许可证文本随各自包分发于 `node_modules/<包名>/LICENSE*`，完整清单以 `package.json` 中的依赖列表为准。

---

> 如对第三方组件的许可合规有任何疑问，欢迎在本项目仓库提交 Issue。
