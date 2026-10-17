# GPUI 开发基线（知识防老化）

> 2026-10-17 建立。**目的：任何后续会话/开发者动手前先读本文，不要凭记忆写 GPUI 代码。**
> GPUI 在 2026 年变化极快（`gpui-kit` 这个名字 2026-09 才从 `gpui-component` 改来；GPUI 本体在 crates.io 上只到 2025-10 的 0.2.2，结构已过时）。训练数据里的"GPUI 知识"大概率是错的。
> 方案见 `docs/gpui-client.md`，本文是可执行的事实基线与纪律。

---

## 1. 版本基线（写死，不 auto-update）

| 项 | 版本 | 说明 |
|---|---|---|
| `gpui-kit` | **`=0.7.3`** | crates.io，Apache-2.0。一个依赖带出 GPUI + 组件 + 图标 |
| `gpui-pre` 系列 | **`=0.3.9`**（gpui-kit 内部锁定） | 不要在 `Cargo.toml` 里自己写，也不要 `cargo update` 它 |
| Rust | **1.99.1**（`rust-toolchain.toml` 钉住） | gpui-pre 0.3.x 用了 1.94 后才稳定的 std API；1.95 编不过 |
| `alacritty_terminal` | **0.26** | 与后端 `Cargo.toml` 同版本 |
| 镜像 | rsproxy（`sparse+https://rsproxy.cn/index/`） | 本机与 CI 都配 |

**升级纪律**：

- `gpui-kit` 0.7.x 仍有 breaking change（`Table`→`DataTable`、`Divider`→`Separator` 等都发生过）。**升级 = 一次明确的工程任务**：读 [gpui-kit releases](https://gpui-kit.com/releases) → 全量编译 → 修 breaking → 真机验收。绝不混在日常改动里。
- `gpui-pre` 由 gpui-kit 精确锁定，**每升一版 gpui-kit 才动一次**。曾经有人用 caret 依赖被周更打崩（gpui-kit #3156），CI 现在强制精确 pin。
- 升级后第一件事：跑本机 spike（见 §8）验证编译与开窗，再跑 IME 手测。

---

## 2. 知识来源优先级（不要凭记忆）

写任何 GPUI/gpui-kit 代码前，按此顺序核实 API：

1. **本地源码**（已下载，最可靠）：
   - `~/.cargo/registry/src/*/gpui-component-0.7.3/`
   - `~/.cargo/registry/src/*/gpui-base-0.7.3/`
   - `~/.cargo/registry/src/*/gpui-pre-0.3.9/src/`
2. **组件文档**：https://gpui-kit.com/component/<name>.md ；索引 https://gpui-kit.com/docs/components
3. **官方 AI skills**（含编码规范与设计规范，shell-rs 把它当规范用）：`npx skills add longbridge/gpui-kit`
4. **参照项目**（本地克隆，见 §7）——读架构可以，代码注意许可证。

**反面模式**（记忆里出现过、实际已过时的说法）：

- ❌ "crates.io 上的 gpui 可以直接用" —— 0.2.2 结构过时（早于平台拆分），不可用。
- ❌ "stable Rust 1.90+ 就够" —— 那是 2025-12 的信息；现在需要 ≥1.98，本项目钉 1.99.1。
- ❌ "gpui-terminal 有个 358 行的现成实现可以依赖" —— 已停更近一年，API 对不上当前 GPUI，只能读思路。
- ❌ "Zed 官方会处理平台问题" —— 2026-02 起官方只做 Zed 用得到的部分，社区需求一律指向 fork。上游不会替我们修任何东西。

---

## 3. 已验证的入口骨架（本机编译 + 开窗通过）

```rust
use gpui_kit::component::input::*;
use gpui_kit::component::*;
use gpui_kit::*;

fn main() {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);                       // 必须先调用，再使用任何组件
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| MyView::new(window, cx))
        })
        .expect("Failed to open window");
    });
}
```

- `gpui_kit::open_window` 自动挂 `Root`（对话框/toast 的宿主），不要自己再包一层。
- 完整可用样例：`E:\1shell4.7\gpui-spike\src\main.rs`（含 Input 与 Button，中文占位符验证过）。
- 带 `Root`、单实例、设置加载、Store 注入的生产级骨架：`E:\1shell4.7\gpui-refs\shell-rs\src\main.rs`（147 行，GPL，读结构不抄代码）。

---

## 4. 工程模式（从 shell-rs AGENTS.md 提炼，已被生产验证）

- **一个命令一个处理器**：按钮、菜单、快捷键全部派发 `actions.rs` 里的 action，由 Workspace 统一处理。不写临时闭包直接改状态。
- **状态经 Entity 流动**：mutator 在 Entity 方法里「改内存 → 写库 → notify」；消费方 `cx.observe`。**渲染回调内不得读取其他实体**，只能传普通快照（渲染闭包运行在 render 过程中，重入会 panic 或死锁）。
- **焦点规则**：`dispatch_action` 只沿焦点元素的祖先链走。工作区内部必须有东西持有焦点；**永远不要聚焦根节点**（对话框层是它的子元素，祖先持焦点会让对话框的焦点陷阱失效）。标题栏按钮用 `FocusHandle::dispatch_action` 派发。
- **对话框关闭后焦点会丢**（gpui-component 既有行为）：这是上游行为不是 bug，别花时间修。
- **彩色 SVG 不能用 `Icon`**：`svg()` 走 `paint_svg`，整图当遮罩单色上色。彩色要 `img()`（光栅化成位图）。
- **gpui-kit 的 Scrollable 不是就地在原元素加滚动条**：它重建外层 div。滚动区内先放一层 `v_flex().w_full()` 再写内容（否则子块按内容宽度收缩，宽度参差）。
- **无头 UI 测试**：`gpui-kit` 的 `test-support` feature + `gpui_kit::test::TestWindowExt`（真实 Workspace 在无头窗口里渲染，`click`/`input`/`find` 驱动）。每个里程碑的验收都要有这类测试，别只靠肉眼看窗口。
  - ⚠️ **但 gpui-kit 0.7.3 的 `test-support` feature 在 Windows 上解析失败**：它会拉入 `gpui-fast` 系列（0.1.7），与 `gpui-pre 0.3.9` 的依赖树冲突（`cocoa` 要 `=0.26.0` vs `0.26.1`，随后 `unicode-properties` 也冲突）。
  - ✅ **替代方案（已在本仓库验证）**：dev-dependencies 用 `gpui-base = { version = "=0.7.3", features = ["test-support"] }` 与 `gpui-component = { version = "=0.7.3", features = ["test-support"] }`，不用 gpui-kit 的 test-support。TDD 辅助函数在 `gpui-client/tests/ui.rs` 里自实现（`render_frame` = `window.refresh(); window.draw(cx).clear(cx)`；`find` = `gpui_base::test_support::find(window, &[], &ElementId)`；点击 = `window.dispatch_event(MouseDownEvent{..}.to_platform_input(), cx)`）。
  - 测试元素必须调 `.test_support()`（`gpui_kit::TestSupportExt`）才会出现在查询里。
  - `TestAppContext::open_window(size, |window, cx| Root::new(view, window, cx))` **返回 `WindowHandle<V>` 而非 Result**，闭包返回 `V` 本身而不是 `Entity<V>`。
  - `App::subscribe` 的监听器是 **3 个参数** `(Entity<T>, &Event, &mut App)`；`Context::subscribe` 是 4 个参数。
  - 测试模块**不要 `use gpui_kit::*`**：glob 会把 gpui 的 `test` 宏带进来遮蔽 `#[test]`，宏展开递归直到超限。全部显式导入。
- 尺寸：`px(..)` 只用于 API 要求 `Pixels` 的地方；其余用 `cx.theme()` token 与 rem 助手。

### 导入与 API 位置（2026-10-17 首次编译踩过的坑）

- `h_flex` / `v_flex` 在 **`gpui_kit::component::*`**，不在 `gpui_kit::*`。
- `.when(...)` 依赖 `gpui::util::FluentBuilder`，**gpui-kit 没有再导出它**（`gpui_kit::util` 也不存在）——别用 `when`，写普通 `if` 分支。
- `.overflow_y_scroll()` 是 `StatefulInteractiveElement` 的方法：**元素必须先 `.id(...)`**（普通 `div()` 没有它）。style 层面的 overflow 没有独立 helper。
- 字重：`.font_weight(gpui_kit::FontWeight::SEMIBOLD)`，没有 `font_semibold` 这类 helper。
- 主题色：`cx.theme().success` / `.danger` / `.muted_foreground` / `.border` / `.accent` / `.background`（`ActiveTheme` 由 `gpui_kit::component::*` 带入）。
- 若报 "no method found for struct Div"，先怀疑是 trait 不在作用域（`gpui_kit::*` 是 `pub use ::gpui::*`，不包含 prelude 的全部 trait），逐个显式导入。

---

## 5. 终端渲染要点（四个参照项目的共识）

1. **解析线程 + UI 定时批量刷新**：解析线程持有 `alacritty_terminal::Term`；UI 线程 ~16ms 取一次快照。锁内只做快照（拷贝成纯数据），**锁外做颜色解析与绘制**。
2. **每行一次 `shape_line`**；`force_width` 传**单格宽度**——传行宽会把首字符之后全部推向右边（shell-rs 实测教训，源码里有注释）。
3. **背景按连续区间合并**成矩形，不逐格画。
4. **字形原点 = `列号 × cell_width`**，不允许布局引擎移动字形。连字默认关（逐格塑形）。
5. 输入编码在 UI 侧完成（`Keystroke` → 按终端模式编码字节），协议回复/鼠标报告/IME 预编辑不走通用输入路径。
6. 后端已有同版本 `alacritty_terminal` 与 2MiB 字节环；客户端收 WS 字节流（0x00 实时 / 0x01 输入 / 0x02 整份回放），**0x02 要重置再重建**（`\x1bc` 或清屏），这是浏览器端踩过的坑（commit `a868ce9`）。

---

## 6. 平台事实

| 平台 | GPU | 托盘 | 字体 | IME |
|---|---|---|---|---|
| Windows | DX11 兼容；虚拟机用模拟适配器**能工作**（官方文档原话），纯 Basic Display Adapter 可能打不开 | 无内置；`gpui-tray 0.1.4` 或移植 tty7 `src/ui/tray/native.rs`（148 行） | DirectWrite，走系统 | **风险**：zed#56327 中文 IME 未修，M0 必须手测 |
| macOS | 全系 Metal，无问题 | 原生 NSStatusItem | 系统 | NSTextInputClient 语义，正常 |
| Linux | 需要 Vulkan；缺驱动报 `NoSupportedDeviceFound`，装驱动可解；无文档化软件渲染 | StatusNotifierItem（tty7 `sni.rs`） | fontconfig | X11/Wayland 输入法框架 |

- 发布说明必须写明 GPU 要求。
- 三平台自启：注册表 `HKCU\...\Run` / LaunchAgent `plist` / systemd user unit。

---

## 7. 参照项目（本地克隆，`E:\1shell4.7\gpui-refs\`）

| 目录 | 项目 | 许可证 | 能做什么 |
|---|---|---|---|
| `tty7/` | l0ng-ai/tty7（1263★） | **Apache-2.0** | **唯一可以抄代码的**：托盘、守护进程模型、终端渲染、IME 处理，保留版权声明即可 |
| `shell-rs/` | since2006/shell-rs（334★） | GPL-3.0 | 零 fork gpui-kit 路线的完整工程样板，**只读结构与文档** |
| `pebrel/` | Kuddev/pebrel（3418★） | GPL-3.0 | 渲染契约、逐格塑形思路，只读 |
| `oxideterm/` | AnalyseDeCircuit/oxideterm（1681★） | GPL-3.0 | vendor GPUI 的做法（我们不采用），只读 |

**纪律**：GPL 项目一行代码都不能复制进 vps-switch。Apache-2.0 的 tty7 可以移植，保留其版权声明。

---

## 8. Spike（每次升级 gpui-kit 后重跑）

`E:\1shell4.7\gpui-spike\`（仓库外，独立 cargo 工程）：

- `cargo build` 通过 = 工具链与版本兼容。
- `cargo run` 出窗口 + 在输入框里打中文 = 开窗与 IME 验证。
- 内存参考：debug 构建窗口进程约 74MB。
- 未验证、需要补的：托盘、透明窗口、Dock 布局、终端 Element 画 5000 格、macOS/Linux 本机构建。

---

## 9. 调研过程教训（避免重复浪费）

- **不开子 agent 做调研**：2026-10-17 四个并行 agent 烧掉约 40 万 token 并撞 429 配额全部失败。读文件自己读，每次让 agent 报告结论会把上下文翻倍。
- **先跑通再写文档**：方案里每个"可行"都要有本机实测支撑（编译、开窗）。
- **验收用真实界面**，不用代理指标（抓包/日志）。
