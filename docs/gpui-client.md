# GPUI 原生 Windows 客户端方案

> 2026-10-17 调研结论。本文是 `docs/plan.md` 的展开，边界修改见文末「对 plan.md 的修改」。

---

## 结论

| 决策 | 选择 | 依据 |
|---|---|---|
| GPUI 分发 | crates.io `gpui-kit = "0.7.3"`（内部精确锁定 `gpui-pre =0.3.9`） | 本机 Windows 11 实测：一次编译通过（1m55s），窗口正常打开 |
| 组件库 | gpui-kit（含 gpui-component） | 75+ 组件、Dock/DataTable/Tree/VirtualList/Editor 齐备、Apache-2.0、Longbridge Pro 生产验证 |
| 终端渲染 | `alacritty_terminal 0.26` + 自写 GPUI Element | 四个参照项目全部走这条路；后端已有同版本解析器与字节环 |
| 后端集成 | GPUI 客户端走 HTTP/WS 连现有 axum 守护进程，后端零改动 | 一份实现、两个前端；现有 API 已覆盖全部板块 |
| 产物 | `vps-switch`（守护进程，Linux/Windows 无头）+ `vps-switch-gpui`（Windows / macOS / Linux 三平台客户端） | 受众优先：能构建的平台都构建，与 shell-rs / tty7 的发布矩阵一致 |

**不 fork GPUI、不打补丁。** 参照项目里只有 shell-rs 是零 fork 路线，它已在生产用 gpui-kit 0.6.1 做了完整 SSH 客户端；我们的 spike 在本机复现了同样的可行性。

---

## 四个参照项目的路线对照（2026-10-17 实测克隆）

| 项目 | 星数 | 许可 | GPUI 来源 | 组件库 | 规模 |
|---|---|---|---|---|---|
| [shell-rs](https://github.com/since2006/shell-rs) | 334 | **GPL-3.0** | crates.io `gpui-kit 0.6.1` | gpui-kit | 13.3 万行，终端模块 1.5 万行 |
| [tty7](https://github.com/l0ng-ai/tty7) | 1263 | **Apache-2.0** | Zed git rev（钉 commit） | gpui-component 自维护分支 | 34 万行，终端 5.7 万行 |
| [pebrel](https://github.com/Kuddev/pebrel) | 3418 | **GPL-3.0** | 自维护 `Kuddev/zed` fork（v1.16.1 + 一串补丁） | gpui-component fork 0.5.2 | 终端核心约 1 万行 |
| [oxideterm](https://github.com/AnalyseDeCircuit/oxideterm) | 1681 | **GPL-3.0** | vendor 整套 `gpui-ce` 进仓库 | 自建 UI | 90 个 crate |

**许可证事实：四个项目里只有 tty7 是 Apache-2.0，可以直接借鉴代码（保留声明）；其余三个是 GPL-3.0，只能读架构、不能抄代码。** 这条约束把 tty7 抬到「第一参照」：它的守护进程模型、终端渲染、托盘实现都可以合法移植。

**它们分叉的原因**（说明为什么我们不能照抄任何一家）：

- pebrel / oxideterm 需要上游没有的东西（DirectWrite 字体内存所有权、虚拟窗口测试、混合 DPI 修复），选择了 fork/vendor，代价是每次上游变更都要手工合并。
- tty7 需要 gpui-component 的未发布修改，选了 git 分支；GPUI 本体钉 Zed commit 而非 gpui-pre，因为它的 `alacritty_terminal` 也是自己的 fork，不能跟周更快照绑死。
- shell-rs 什么都不需要改，所以用 crates.io 版本，交给 gpui-kit 的周更节奏管理升级。

**vps-switch 属于 shell-rs 这一档**：我们没有任何必须改 GPUI 本体的需求。如果后期发现确实需要（例如 IME bug 必须在平台层修），再升级到 tty7 模式（钉 Zed rev），这是渐进的，不影响现在的选型。

---

## 生态事实（决定维护成本的硬数据）

- `gpui-pre` 是 Longbridge（huacnlee）把 Zed 每周快照重发到 crates.io 的系列：0.3.9 = 2026-10-12，Apache-2.0，月下载 10.5 万，是事实上唯一的可用分发。crates.io 上 `gpui` 这个名字的 Zed 官方版本停在 2025-10 的 0.2.2，结构已过时（早于平台拆分），不能用。
- gpui-kit 0.7.3（2026-10-12），周更节奏，**精确锁 `=0.3.9`**（它曾经用 caret 被周更打崩过，现在 CI 强制精确 pin）。升级 GPUI 必须跟它一起走。
- 0.7.x 仍有 breaking change（`Table`→`DataTable` 等），**锁死具体版本升级，不自动跟周更**。
- Zed 官方 2026 年把 GPUI 开发降级为「只做 Zed 用得到的部分」，社区需求一律指向 fork。这对我们意味着：**上游不会替我们修任何东西**，所有平台层缺口要么自己补、要么靠 gpui-kit 这个已被生态接受的通道。
- Zed 的终端 crate 不可复用：GPL-3.0 且与编辑器深度耦合（不在 crates.io，不独立发布）。

---

## 架构

### 进程模型

```
vps-switch-gpui.exe (GPUI 客户端)
        │  HTTP REST + 二进制 WS（127.0.0.1:18800，回环免 Key）
        ▼
vps-switch.exe (守护进程：axum + SQLite + russh + alacritty Term + MCP)
        │
        ├── 本机 PTY（portable-pty + 随包 ConPTY）
        └── 远端 SSH（russh，连接池 + 跳板）
```

- **后端零改动。** 现有 26 个 REST 端点 + 终端二进制 WS（0x00 实时 / 0x01 输入 / 0x02 回放）+ SSE 事件，覆盖全部五大板块与 MCP。
- **客户端启动时自动拉起守护进程**（若 127.0.0.1:18800 无响应），沿用现有 `--minimized` 启动参数与数据目录解析（`%APPDATA%\vps-switch\data`）。
- 终端状态仍是「后端权威 + 客户端第二份解析」：后端 `Term` 保证 pane 保活与回放，客户端 `alacritty_terminal` 把 WS 字节流渲染出来。这与现在浏览器里 xterm.js 的地位完全相同。
- 旧 Windows 桌面壳（`src/desktop.rs`，tao + wry + tray-icon）在 GPUI 客户端达到可用后退役；退役之前两者并存，互不干扰。
- **客户端三平台同一份代码**：GPUI 与 gpui-kit 原生支持 Windows / macOS / Linux，shell-rs（零 fork 的 gpui-kit 路线）与 tty7 都已发布三平台产物。需要平台专用代码的只有托盘（原生 / StatusNotifierItem）与自启（注册表 / LaunchAgent / systemd user unit）。

### 客户端模块（`gpui/` 目录，独立 binary）

| 模块 | 内容 | 复用的后端 |
|---|---|---|
| `workspace` | DockArea 布局：左主机树 / 中工作台 / 右详情，标签页与分屏 | — |
| `hosts` | 主机树（Tree）、增删改（Dialog）、连接测试 | `/api/hosts*` |
| `terminal` | alacritty_terminal 解析 + 自写 Element 渲染 + 输入编码 | `/api/ws/terminal/:pane_id`、`/api/terminals` |
| `files` | 双栏 DataTable、CodeMirror 对应物（gpui-kit 内置 Editor） | `/api/files/*` |
| `inspector` | 探针仪表盘（gpui-kit Chart：Line/Bar/Area） | `/api/hosts/:id/inspect` |
| `settings` | 设置四分类 | `/api/settings*` |
| `tray` | 托盘图标 + 右键主机列表秒开终端 | — |
| `app` | 单实例（CLI socket）、自启、窗口状态持久化 | — |

### 终端渲染（关键实现细节，来自四个项目的共识）

1. 解析线程持有 `alacritty_terminal::Term`，UI 线程定时（16ms）批量取快照，**锁内只做快照、锁外做颜色解析与绘制**（pebrel 契约）。
2. 每行一次 `shape_line`，`force_width` 传**单格宽度**而非行宽（shell-rs 的实测教训：传行宽会把首字符之后全部推向右边）。
3. 背景色按连续区间合并成矩形，不逐格画。
4. 字形原点 = `列号 × cell_width`，不允许布局引擎移动字形（等宽字体正确性的前提）。
5. 连字默认关闭（逐格塑形），后续作为可选特性（pebrel 的做法：同样式相邻 ASCII 合并塑形再映射回固定列号）。

---

## Windows 风险清单（按严重度，全部要在 M0 验证）

| # | 风险 | 事实 | 应对 |
|---|---|---|---|
| 1 | **中文 IME** | zed#56327（2026-05 开，标记 needs repro，2026-10 仍未修）：IME 组合态下 WM_KEYDOWN 未被抑制，可能重复插入或干扰输入 | M0 第一项实测。若复现：在 spike 里接 Win32 `WM_IME_*` 自管组合文本；若平台层限制无法绕开，再考虑钉 Zed rev 自修（升级到 tty7 模式） |
| 2 | **GPU 要求** | 这是 GPUI 框架自身的限制，所有 GPUI 应用（含四个参照项目）完全一样。Windows 要求 DirectX 11 兼容 GPU，**官方文档明确虚拟机里用模拟适配器「能工作、仅性能下降」**；仅纯 Basic Display Adapter 环境可能打不开。Linux 需要 Vulkan，缺驱动报 `NoSupportedDeviceFound`，装驱动可解（软件渲染未文档化）。macOS 全系 Metal，无此问题 | 发布说明写明 GPU 要求。重点自测用户的云电脑场景（虚拟机） |
| 3 | 托盘 | GPUI 上游无内置（托盘 PR #44047 被官方以「无 Zed 用例」关闭）；`gpui-tray 0.1.4`（2026-09-20）可用 | 三平台各一套：Windows/macOS 原生 + Linux StatusNotifierItem（tty7 的 `src/ui/tray/` 共 1150 行，Apache-2.0 可移植） |
| 4 | 窗口透明/毛玻璃 | 未知：GPUI 有 `transparent` 选项，Windows 上亚克力/模糊效果未验证 | M0 验证。退路：纯色深/浅主题（gpui-kit 内置 20+ 主题），毛玻璃作为后续增强 |
| 5 | 字体 | DirectWrite 走系统 shaping/光栅化，无需 font-kit，中文回退正常（Zed 自身每日验证） | 低风险，M0 顺带验证 |

---

## 分阶段计划（每阶段可独立验收）

- **M0 可行性收尾（1 天）**：已完成的编译 + 开窗 ✅。待验证：中文输入、托盘、透明窗口、Dock 布局骨架、终端 Element 画 5000 格。
- **M1 骨架（3–5 天）**：workspace + DockArea + 主机树 + 设置页只读；客户端连守护进程读真实数据。
- **M2 终端**：alacritty_terminal 解析 + Element 渲染 + 输入 + resize + 重连回放；手用终端零注入的既有纪律沿用。
- **M3 文件**：双栏 DataTable + 传输队列 + 内置 Editor。
- **M4 探针**：仪表盘图表。
- **M5 平台**：托盘、自启、单实例、窗口状态、独立终端窗口。
- **M6 收尾**：退役旧桌面壳；打包三平台 —— Windows（Inno Setup，沿用现有流程）、macOS（arm64 + x64 dmg；签名/公证需要 Apple Developer 账号，先做未签名版）、Linux（AppImage，随包 X11/Wayland 依赖）；GitHub Actions 发版流水线（tag 触发、matrix 三平台、产物 + SHA256SUMS）；旧桌面壳退役、发版。

每个里程碑验收用真实机队（vip-hk 等）与真实界面，不用代理指标。

---

## 不做的事（防蔓延）

- 不 fork GPUI、不打补丁（除非 M0 的 IME 验证证明必须）。
- 不学 tty7 的 agent 集成、mobile、remote workspace；不学 oxideterm 的插件/wasm/云同步。
- 不引入 IDE 能力（LSP/补全/diff 视图）——内置 Editor 只用来看和改配置文件。
- Vue 前端不追求与 GPUI 对齐：它降级为「浏览器控制台 + Linux 服务器管理界面」，功能完整但不再加新交互。
- 概念数不增加：客户端/守护进程是既有架构的显式化，不引入新名词。

---

## 对 plan.md 的修改（待拍板）

边界一句话改为：

> **一个轻量级 VPS 管理工具：先把 VPS 仓库、终端、文件浏览器、文件上传下载、探针五大核心板块做到极致，最后将这些现成链路加上护栏工具化为 MCP Server。交付两个产物：GPUI 原生的桌面客户端（Windows / macOS / Linux，`vps-switch-gpui`）与无头的守护进程（`vps-switch`，Linux/Windows），两者共用同一后端与同一套 HTTP/WS API。**

技术栈表「桌面壳」一行改为：

> | 桌面壳 | GPUI（gpui-kit 0.7.3 / gpui-pre 0.3.9） | Windows 客户端；Linux 无头服务不带壳 |
