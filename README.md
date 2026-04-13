# Android Debloat Studio

Android Debloat Studio 是一个桌面端 Android 预装清理工具。

它把一串零散的 ADB 操作收成了一个可视化流程：连接手机、分析预装包、挑选可删项、执行 `pm uninstall --user 0`、记录结果，并在需要时恢复最近一次清理。

这个项目的目标很直接：

- 不用手敲 ADB 命令
- 不用自己翻系统包名
- 不用每次清理后再手动确认设备是不是还活着

## 它适合谁

- 想清理 Android 预装应用，但不想手工跑 ADB 的人
- 想先看分析结果，再决定删什么的人
- 想保留一次“最近清理记录”，出问题能回滚的人

## 它不做什么

- 不 root 手机
- 不改系统分区
- 不刷机
- 不承诺“极限精简”

当前版本仍然是一个**偏保守**的清理工具，不是“除了桌面全删”的激进脚本。

## 当前支持

桌面端：

- macOS Apple Silicon：`.dmg`
- Windows x64：`.exe`

手机侧要求：

- Android 设备
- USB 数据线
- 已开启 USB 调试
- 已允许当前电脑的调试授权

当前 Release：

- [Releases](https://github.com/skernelx/android-debloat-studio/releases)

> 应用内置 ADB。正常使用时，不需要你自己再装一份 `adb`。

## 现在能做什么

- 扫描已连接设备，显示品牌、型号、Android 版本、SDK、Build 信息
- 读取系统包、用户包、桌面入口包、HOME 包和部分运行时角色信息
- 把包分成“核心保留”“安全可删”“用户安装”几类
- 对“安全可删”项执行 `pm uninstall --user 0`
- 清理后做设备验活，异常时中止当前批次
- 保存最近一次清理记录
- 恢复最近一次清理
- 查看历史批次和健康检查结果

## 它是怎么工作的

大致流程是这样：

1. 通过 ADB 读取设备和包信息
2. 根据规则和运行时信息做风险分层
3. 只把判定为 `safeRemove` 的包加入清理队列
4. 执行 `pm uninstall --user 0`
5. 在批次执行过程中持续做验活
6. 把结果写入本地记录，供后续恢复

这里有两个边界要说清楚：

- `pm uninstall --user 0` 不是永久删掉系统分区里的 APK，它是对当前用户隐藏/卸载
- 当前版本只支持恢复**最近一次**清理记录，不是任意时间点快照

## 快速开始

### 1. 下载安装包

从 [Releases](https://github.com/skernelx/android-debloat-studio/releases) 下载对应平台的安装包：

- macOS：`Android.Debloat.Studio_*.dmg`
- Windows：`Android.Debloat.Studio_*_x64-setup.exe`

### 2. 打开手机的 USB 调试

不同品牌路径不一样，但思路基本一致：

1. 在“关于手机”里连续点击版本号 7 次，打开开发者模式
2. 回到设置，进入“开发者选项”
3. 打开“USB 调试”

如果你忘了怎么开，应用内已经做了常见品牌的提示，不需要再去网上搜一轮。

### 3. 连接手机并授权

1. 用数据线连接手机
2. 保持手机亮屏、解锁
3. 如果弹出“允许 USB 调试”，点“允许”
4. 回到应用，点击“刷新设备”

### 4. 分析预装包

设备进入 `device` 状态后，点击“分析预装包”。

应用会读取包信息并给出分析结果。第一次分析可能会比较慢，尤其是在系统包很多的设备上，这是正常现象。

### 5. 执行清理

确认结果后，执行清理。

当前版本默认只会对被判定为“安全可删”的包动手，不会让你直接把所有系统包一把梭。

### 6. 需要时恢复

如果这一批删完发现不合适，可以用“恢复最近一次清理”把刚才那一批撤回。

## 如果手机没开 USB 调试，会怎样

不会静默卡住。

当前界面会区分几种常见状态，并给出对应提示：

- 没检测到设备
- 设备是 `unauthorized`
- 设备是 `offline`
- ADB 当前不可用

也就是说，如果问题出在“没开 USB 调试”或“手机没点允许”，应用会直接提醒你，而不是假装已经连上。

## 开发

### 技术栈

- Tauri 2
- React 19
- TypeScript
- Rust

### 本地依赖

- Node.js
- pnpm
- Rust
- Xcode Command Line Tools（macOS 本地开发时）

### 安装依赖

```bash
pnpm install
```

### 启动开发模式

```bash
pnpm tauri:dev
```

### 代码检查

```bash
pnpm lint
cargo test --manifest-path src-tauri/Cargo.toml
```

### 本地打包

```bash
pnpm tauri:build
```

默认产物目录：

- `src-tauri/target/release/bundle/dmg/`
- `src-tauri/target/release/bundle/nsis/`

## 自动发布

仓库内置 GitHub Actions 发布流程。

推送形如 `v0.1.1` 的 tag 后，会自动构建并上传：

- macOS `.dmg`
- Windows `NSIS .exe`

示例：

```bash
git tag v0.1.1
git push origin refs/tags/v0.1.1
```

## 项目结构

```text
android-debloat-studio/
├── src/                          React 前端
├── src-tauri/src/adb.rs          ADB 调用、设备扫描、包采集
├── src-tauri/src/analyzer.rs     风险分层与分析
├── src-tauri/src/cleanup.rs      清理、验活、恢复
├── src-tauri/src/records.rs      本地记录持久化
├── src-tauri/rules/              厂商规则
└── .github/workflows/release.yml 自动发布
```

## 当前边界

为了避免误导，这里把现状说死一点：

- 当前 Release 重点覆盖的是 macOS 和 Windows
- Linux 安装包暂时没有一起发布
- 当前策略仍然偏保守，目标是“安全清理”，不是“系统只剩桌面”
- 如果你要把它推进到极限精简模式，需要继续改分析器和验活策略

## 仓库

- 仓库地址：[skernelx/android-debloat-studio](https://github.com/skernelx/android-debloat-studio)
- 问题反馈：[Issues](https://github.com/skernelx/android-debloat-studio/issues)
