# Android Debloat Studio

一个面向 macOS 的 Android 预装精简桌面工具。

它通过 ADB 读取设备包清单，按规则把系统包分成“核心保留”“安全可删”“用户安装”三类，并支持直接执行 `pm uninstall --user 0`、自动验活和最近一次回滚。

## 当前状态

- 桌面端：Tauri 2 + React 19
- 目标平台：macOS / Windows
- 打包形式：macOS `.app` / `.dmg`，Windows `NSIS .exe`
- 运行方式：应用内置 `adb`

本地开发默认在 macOS 上完成。

公开发布时，仓库会通过 GitHub Actions 同时构建：

- macOS `.dmg`
- Windows `NSIS .exe`

Windows 发布资产会在构建时自动下载 Windows 版 Android platform-tools，并把 `adb.exe` / `AdbWinApi.dll` / `AdbWinUsbApi.dll` 一起打进安装包。

## 现在能做什么

- 扫描已连接 Android 设备并展示品牌、型号、Android 版本、SDK、Build 信息
- 采集系统包、用户包、桌面入口包、HOME 包和运行时角色信息
- 分析预装包风险分层
- 只对“安全可删”项执行 `pm uninstall --user 0`
- 每删一个包后做验活，异常时自动停止并尝试回退
- 保留最近一次清理记录，支持恢复
- 展示批次历史和设备健康检查结果

## 处理原则

当前版本的默认思路是“保守执行”：

- 不碰用户自己安装的应用
- 尽量保护系统骨架组件
- 清理动作只作用于 `user 0`
- 失败时优先保设备仍能正常进入系统

这意味着它更像是“受控精简工具”，不是“无规则全删脚本”。

## 项目结构

```text
android-debloat-studio/
├── src/                     React 前端
├── src-tauri/src/adb.rs     ADB 调用、设备扫描、包采集
├── src-tauri/src/analyzer.rs
│   预装包分析与风险分层
├── src-tauri/src/cleanup.rs
│   执行清理、验活、回滚
├── src-tauri/src/records.rs
│   历史记录持久化
└── src-tauri/rules/vendor_rules.json
    厂商保护规则
```

## 本地开发

### 依赖

- Node.js
- pnpm
- Rust
- Xcode Command Line Tools

### 安装依赖

```bash
pnpm install
```

### 启动开发模式

```bash
pnpm tauri:dev
```

### 质量检查

```bash
pnpm lint
cargo test --manifest-path src-tauri/Cargo.toml
```

### 本地打包

```bash
pnpm tauri:build
```

打包产物默认在：

- `src-tauri/target/release/bundle/macos/`
- `src-tauri/target/release/bundle/dmg/`

### 发布到 GitHub Release

推送形如 `v0.1.1` 的 tag 后，GitHub Actions 会自动构建并发布：

```bash
git tag v0.1.1
git push origin v0.1.1
```

## 运行说明

1. 用 USB 连接手机
2. 在手机里打开 USB 调试
3. 允许当前电脑的调试授权
4. 打开应用，先点“刷新设备”
5. 识别到设备后再点“分析预装包”

如果设备没有进入 `device` 状态，应用会直接在界面上提示，不会继续执行分析或清理。

## 仓库说明

- 仓库地址：<https://github.com/skernelx/android-debloat-studio>
- Release 会附带 macOS `dmg` 和 Windows `exe`
- 当前没有额外发布 npm 包
- 当前没有单独设置开源许可证文件

如果你要继续把它往“极限精简模式”推进，后续最值得动的地方是：

- 调整分析器的保护策略
- 收紧或放宽验活标准
- 把“保守模式”和“极限模式”拆成两套明确可选的执行策略
