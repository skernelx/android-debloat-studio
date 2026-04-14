# Android Debloat Studio

A desktop app that removes Android bloatware without root, built with Tauri 2, React, and Rust.

Connect your phone via USB, scan preinstalled packages, review a risk-layered analysis, and uninstall what you don't need — all through a visual interface instead of raw ADB commands.

[![Latest Release](https://img.shields.io/github/v/release/skernelx/android-debloat-studio?style=flat-square)](https://github.com/skernelx/android-debloat-studio/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-blue?style=flat-square)](#download)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-FFC131?style=flat-square)](https://tauri.app)

## Features

- **Device auto-detection** — scans connected devices and displays brand, model, Android version, SDK level, and build info
- **Risk-based package analysis** — classifies every package into *core keep*, *safe remove*, or *user installed* based on vendor rules and runtime role detection
- **Two cleanup modes** — *Balanced* (conservative, preserves dialer/SMS/camera) and *Minimal Core* (aggressive, keeps only the OS skeleton)
- **Per-package health checks** — runs a liveness probe after each uninstall; aborts the batch if the device becomes unhealthy
- **One-click restore** — rolls back the most recent cleanup batch
- **Operation history** — records every cleanup and restore with timestamps and health status
- **Multi-vendor rules** — built-in rule packs for Xiaomi/Redmi, Huawei/Honor, Samsung, OPPO/realme/OnePlus, and vivo/iQOO
- **Bundled ADB** — ships with its own `adb` binary, no separate installation required
- **Connection guidance** — step-by-step USB debugging instructions for each major brand, shown directly in the UI

## How it works

1. Reads device info and all installed packages via ADB
2. Applies vendor-specific rules + runtime role detection (home launcher, default dialer, default SMS) to classify packages
3. Presents a selectable list of packages deemed safe to remove
4. Executes `pm uninstall --user 0` for selected packages, with a health check after each removal
5. Saves the batch to local history for potential rollback

> **Note:** `pm uninstall --user 0` hides/uninstalls the app for the current user. It does not delete the APK from the system partition. A factory reset will bring everything back.

## Download

Grab the latest installer from [**Releases**](https://github.com/skernelx/android-debloat-studio/releases):

| Platform | File |
|----------|------|
| macOS (Apple Silicon) | `Android.Debloat.Studio_*.dmg` |
| Windows (x64) | `Android.Debloat.Studio_*_x64-setup.exe` |

## Quick start

1. **Enable USB Debugging** on your phone — go to *About Phone*, tap *Build Number* 7 times, then enable *USB Debugging* in *Developer Options*
2. **Connect** your phone via a data-capable USB cable and authorize the computer when prompted
3. **Launch** Android Debloat Studio and click *Refresh Devices*
4. **Analyze** — once the device shows as *Ready*, click *Analyze Preinstalled Packages*
5. **Review & clean** — check the packages you want to remove, then click the cleanup button
6. **Restore if needed** — use *Restore Last Cleanup* to roll back

## Cleanup modes

| Mode | Strategy | Best for |
|------|----------|----------|
| **Balanced** | Preserves dialer, SMS, camera, browser, and vendor security apps | Safe first-pass cleanup |
| **Minimal Core** | Only keeps OS skeleton, launcher, installer, and connectivity | Maximum debloat when you don't need built-in phone/SMS/camera |

## Supported vendors

| Vendor family | Brands |
|---------------|--------|
| Xiaomi | Xiaomi, Redmi, POCO (MIUI / HyperOS) |
| Huawei | Huawei, Honor (EMUI / MagicUI) |
| Samsung | Samsung (One UI) |
| ColorOS | OPPO, realme, OnePlus (ColorOS) |
| vivo | vivo, iQOO (Funtouch / OriginOS) |
| Generic | All other Android devices |

## Development

### Tech stack

- **Backend:** Rust + Tauri 2
- **Frontend:** React 19 + TypeScript
- **Build:** Vite 8 + pnpm

### Prerequisites

- Node.js 22+
- pnpm
- Rust toolchain
- Xcode Command Line Tools (macOS)

### Setup

```bash
pnpm install
pnpm tauri:dev
```

### Build

```bash
pnpm tauri:build
```

### Lint & test

```bash
pnpm lint
cargo test --manifest-path src-tauri/Cargo.toml
```

### Project structure

```
src/                  React frontend (App, API layer, types)
src-tauri/
  src/adb.rs          ADB invocation, device scanning, package collection
  src/analyzer.rs     Risk classification and vendor rule engine
  src/cleanup.rs      Batch uninstall, health probes, rollback
  src/records.rs      Local operation history persistence
  rules/              Vendor-specific rule packs (JSON)
.github/workflows/    CI/CD release pipeline
```

### Release

Push a version tag to trigger an automated build:

```bash
git tag v0.1.3
git push origin v0.1.3
```

GitHub Actions will build and publish macOS `.dmg` and Windows `.exe` installers.

---

<details>
<summary><strong>中文说明</strong></summary>

# Android Debloat Studio

免 root 的安卓预装清理桌面工具，基于 Tauri 2 + React + Rust 构建。

通过 USB 连接手机，自动扫描预装包，按风险分层展示分析结果，勾选后一键清理——不用手敲 ADB 命令，不用自己查包名。

## 功能

- **设备自动识别** — 扫描已连接设备，显示品牌、型号、Android 版本、SDK、Build 信息
- **风险分层分析** — 根据厂商规则和运行时角色检测，将包分为「核心保留」「安全可删」「用户安装」三类
- **双清理模式** — 「平衡模式」保守清理，保留电话/短信/相机；「极限精简」只保系统骨架
- **逐包验活** — 每删一个包后检查设备健康状态，异常时自动中止
- **一键回滚** — 恢复最近一次清理批次
- **操作历史** — 记录每次清理和恢复的时间、结果、健康状态
- **多厂商规则** — 内置小米/Redmi、华为/荣耀、三星、OPPO/realme/一加、vivo/iQOO 规则包
- **内置 ADB** — 自带 `adb`，不需要额外安装
- **连接引导** — 界面内直接展示各品牌 USB 调试开启方式

## 工作原理

1. 通过 ADB 读取设备信息和全部已安装包
2. 根据厂商规则 + 运行时角色（桌面、默认电话、默认短信）做风险分层
3. 将判定为「安全可删」的包展示为可勾选列表
4. 对选中的包执行 `pm uninstall --user 0`，每删一个包后做验活检查
5. 将本次操作写入本地历史，支持后续回滚

> **说明：** `pm uninstall --user 0` 是对当前用户隐藏/卸载应用，不会删除系统分区中的 APK。恢复出厂设置后会全部恢复。

## 下载

从 [**Releases**](https://github.com/skernelx/android-debloat-studio/releases) 下载安装包：

| 平台 | 文件 |
|------|------|
| macOS (Apple Silicon) | `Android.Debloat.Studio_*.dmg` |
| Windows (x64) | `Android.Debloat.Studio_*_x64-setup.exe` |

## 快速开始

1. **开启 USB 调试** — 进入「关于手机」连续点击版本号 7 次，然后在「开发者选项」中打开「USB 调试」
2. **连接手机** — 用数据线连接电脑，手机弹窗点「允许」
3. **启动应用** — 打开 Android Debloat Studio，点击「刷新设备」
4. **分析** — 设备显示「已就绪」后，点击「分析预装包」
5. **清理** — 勾选要删除的包，点击清理按钮
6. **回滚** — 如需撤回，点击「恢复最近一次清理」

## 清理模式

| 模式 | 策略 | 适用场景 |
|------|------|---------|
| **平衡模式** | 保留电话、短信、相机、浏览器、厂商安全中心 | 稳妥的首次清理 |
| **极限精简** | 只保系统骨架、桌面、安装器、网络连接 | 不需要原生电话/短信/相机时的深度精简 |

## 支持厂商

| 厂商族 | 品牌 |
|--------|------|
| 小米 | 小米、Redmi、POCO（MIUI / HyperOS） |
| 华为 | 华为、荣耀（EMUI / MagicUI） |
| 三星 | 三星（One UI） |
| ColorOS | OPPO、realme、一加（ColorOS） |
| vivo | vivo、iQOO（Funtouch / OriginOS） |
| 通用 | 其他 Android 设备 |

## 开发

### 技术栈

- **后端：** Rust + Tauri 2
- **前端：** React 19 + TypeScript
- **构建：** Vite 8 + pnpm

### 环境要求

- Node.js 22+
- pnpm
- Rust 工具链
- Xcode Command Line Tools（macOS）

### 本地开发

```bash
pnpm install
pnpm tauri:dev
```

### 打包

```bash
pnpm tauri:build
```

### 代码检查与测试

```bash
pnpm lint
cargo test --manifest-path src-tauri/Cargo.toml
```

### 发布

推送版本 tag 触发自动构建：

```bash
git tag v0.1.3
git push origin v0.1.3
```

GitHub Actions 会自动构建并发布 macOS `.dmg` 和 Windows `.exe` 安装包。

</details>
