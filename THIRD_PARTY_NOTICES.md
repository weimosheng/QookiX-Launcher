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

> 如对第三方组件的许可合规有任何疑问，欢迎在本项目仓库提交 Issue。
