# Coral Launcher

<br/>
<p align="center">
  <img src="logo.png" alt="Coral Launcher" width="128" />
</p>
<p align="center">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-FFC131?style=flat&logo=tauri&logoColor=white" />
  <img alt="React" src="https://img.shields.io/badge/React-18-61DAFB?style=flat&logo=react" />
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-5-3178C6?style=flat&logo=typescript&logoColor=white" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-1-orange?style=flat&logo=rust" />
</p>

> 用 AI 做了一个 Minecraft 启动器，圆了我刚入行时的梦。

## 关于这个项目

说来感慨，当初正是因为想写 Minecraft 启动器和模组，我才踏入了计算机这个行业。说 MC 改变了我的人生轨迹，一点也不为过。

刚入行那会儿，还没有 AI，前端也远没有今天这么多自动化工具。一个个人开发者想独立做出一款完整的桌面软件，每一步都步履维艰。

时代越来越进步，记忆越来越模糊。而现在，我们有了 AI——不再需要精通每一个技术栈，几句话就能完成过去需要大量时间才能做完的工作。虽然如今到处都在讨论"AI 替代""AI 取代"，但对我而言，AI 是实现想法的工具：它能帮我圆年少时的梦，也能陪我做想做的事情。

其实早在我刚入行的时候，各种自动化工具就已经在悄然涌现，计算机行业从未停止过"工具替代人力"的进程，AI 只是让这个进程走得快了一些。AI 永远是工具，重要的从来都是使用工具的人。如今，我能用它完成过去一个人难以企及的工作，也有底气去尝试更多不可思议的事。

**Coral Launcher** 就是在这样的背景下诞生的——这是我的第一个 AI 辅助开发项目，一款使用 Tauri 2、React、TypeScript 和 Rust 构建的轻量级 Minecraft Java 版启动器。

这是一个桌面客户端，而非网站。React 仅作为界面层渲染在 Tauri 的原生 WebView 窗口中；打包后的 Windows 版本生成的是可直接双击运行的标准 `.exe` 文件。

## 目录

- [功能](#功能)
- [技术选型](#技术选型)
- [快速开始](#快速开始)
- [构建 Windows 可执行文件](#构建-windows-可执行文件)
- [账号设置](#账号设置)
- [数据存储位置](#数据存储位置)
- [参考来源](#参考来源)
- [后续计划](#后续计划)
- [开源协议](#开源协议)

## 功能

- 从 BMCLAPI 获取 Minecraft 版本清单，失败时回退到 Mojang/Piston 元数据源。
- 下载选定版本的清单、客户端 jar、类库、原生库、资源索引及资源文件，支持重试、镜像回退、大小与 SHA1 校验，以及并行的类库和资源下载（类库 8 并发，资源 24 并发）。
- 将原生库解压到对应版本目录中。
- 构建并启动 Java 命令行，支持 Microsoft 认证或离线模式的玩家数据。
- **Microsoft 设备码登录**：无需手动输入 Client ID，支持 refresh token 会话恢复、登出、Xbox Live / XSTS / Minecraft Services 认证、游戏资格检查及角色信息查询。
- **离线模式**：基于 OfflinePlayer 规范的 UUID 生成，支持本地单人游戏或离线服务器。
- **Modrinth 集成**：按游戏版本和加载器（Fabric / Forge / Quilt / NeoForge）搜索模组，一键下载到实例的 `mods` 目录。
- 提供浏览器预览模式，方便在 Tauri 之外进行 UI 开发调试。

## 技术选型

Tauri 使用系统自带的 WebView，而非捆绑 Chromium，同时为下载、校验、解压和进程启动等工作提供了 Rust 后端。这使得 Windows 应用体积小巧，更接近原生 exe 的体验，也为后续的 Linux 移植铺平了道路。

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2 |
| 前端 | React 18 + TypeScript 5 + Vite 5 |
| 后端 | Rust（reqwest / tokio / sha1 / md5 / zip） |
| WebView | Microsoft Edge WebView2（Windows 系统自带） |
| 数据源 | BMCLAPI 镜像 + Mojang 官方回退 |
| 模组平台 | Modrinth API v2 |

## 快速开始

```powershell
# 安装依赖
npm install

# 启动桌面开发模式
npm run dev:desktop
```

如果仅需预览 Web UI 布局（不含后端功能）：

```powershell
npm run dev:web
```

> **注意**：Web 预览模式下无法下载 Minecraft、进行认证、安装模组或启动 Java——这些操作都依赖 Tauri/Rust 桌面后端。

## 构建 Windows 可执行文件

### 环境检查

```powershell
npm run check:win
```

构建需要 **Microsoft C++ Build Tools** 以及 **Windows SDK**，因为 Rust 的 MSVC 目标和 Tauri 的 Windows 资源编译需要 `link.exe` 等工具。

### 构建 exe

```powershell
npm run build:exe
```

构建成功后，exe 生成于项目根目录 `coral-launcher.exe`，同时也保存在 `src-tauri\target\release\coral-launcher.exe`。

> 直接生成的 exe 使用系统自带的 Microsoft Edge WebView2 运行时。目前大多数 Windows 系统已预装该运行时；对于较旧的 Windows 系统，请使用安装包构建方式，以便在安装过程中处理 WebView2 的安装。

### 构建安装包 (NSIS)

```powershell
npm run build:installer
```

### 自定义图标

在构建前将 `logo.png` 或 `logo.ico` 放在项目根目录即可。构建脚本会自动将其复制到 Web UI 资源中，并用于 Windows 可执行文件的图标。

## 账号设置

启动器默认使用内置的公共 Microsoft Client ID，通过 Microsoft 设备码流程请求 `XboxLive.signin offline_access` 权限。如需使用自定义应用注册，请在构建前设置环境变量：

```powershell
$env:CORAL_MS_CLIENT_ID = "your-client-id"
```

登录成功后，启动器会将 Minecraft 会话保存在应用数据目录下，以便下次启动时自动恢复。使用账号页面的刷新按钮可续期 Microsoft / Minecraft 令牌，点击登出则会删除本地会话文件。

## 数据存储位置

| 系统 | 数据路径 |
|---|---|
| Windows | `%APPDATA%\CoralLauncher\minecraft` |
| Linux | `~/.local/share/CoralLauncher/minecraft` |

已保存的账号会话与数据目录同级：Windows 上位于 `%APPDATA%\CoralLauncher\minecraft-account.json`。

## 参考来源

- [Minecraft Version Manifest](https://piston-meta.mojang.com/mc/game/version_manifest_v2.json)
- [BMCLAPI Minecraft 镜像](https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json)
- [Microsoft 设备码认证](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code)
- [PCL 下载实现参考](https://github.com/Meloong-Git/PCL/blob/c86fdc3af17b6b5f3ea63bd8e68aef3576eea3a9/Plain%20Craft%20Launcher%202/Modules/Minecraft/ModDownload.vb)
- [PCL 网络重试/检查参考](https://github.com/Meloong-Git/PCL/blob/c86fdc3af17b6b5f3ea63bd8e68aef3576eea3a9/Plain%20Craft%20Launcher%202/Modules/Base/ModNet.vb)
- [Tauri 架构及体积优势说明](https://v2.tauri.app/start/)
- [Modrinth 搜索 API](https://docs.modrinth.com/api/operations/searchprojects/)
- [CurseForge API（预留集成）](https://docs.curseforge.com/rest-api/)

## 后续计划

- [ ] 添加 Fabric / Quilt / Forge / NeoForge 加载器安装功能。
- [ ] 将 refresh token 迁移到操作系统密钥链或 Tauri Stronghold，替代本地 JSON 文件存储。
- [ ] Java 运行时自动发现与下载（按 Mojang 声明的 Java 大版本匹配）。
- [ ] 并行下载支持断点续传，并提供按文件组细分的下载进度。

## 开源协议

本项目采用 [MIT License](LICENSE)。
