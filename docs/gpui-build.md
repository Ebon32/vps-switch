# GPUI 客户端制作文档

> 2026-10-17 制订。**这是施工顺序的权威**：文档 → 依赖补齐 → 逐板块（先功能、后前端）。
> 旧前端的视觉与交互以 `docs/ui-spec.md` 与 Playwright 实测真值为准；本文管的是
> 「旧前端的每个能力，在 GPUI 里用什么来顶」。

---

## 一、流程（不可越级）

1. **本文档定稿**——能力清单、依赖清单、板块顺序一次列全，之后按它执行。
2. **依赖补齐**——一次性把所有"替代旧前端能力"的东西装进来并逐个冒烟验证；缺什么在这里发现，而不是做到一半再发现。
3. **逐板块施工**：每个板块 = ①功能链路（后端已有则跳过）→ ②前端还原 → ③验收 → ④下一块。**一个板块验收通过才允许碰下一个板块。**

**每块前端的施工纪律**

- 动手前：用 Playwright 打开旧前端（守护进程 18800 一直在跑），点开该板块的每个状态，读 computed style（尺寸/间距/字号/颜色/圆角/阴影/行高），并截图存档到 `artifacts/ui-reference/`。
- 实现时：所有数值来自实测，不来自记忆或目测；旧前端的 SVG 一律原样移植，不用字符或近似图形代替。
- 完成后：逐元素核对（尺寸、颜色、间距、图标、悬停态），写无头 UI 测试，最后交用户目视。
- **禁止语**："差不多""近似""占位"——除第二节 2.4 列明的不可还原项。

---

## 二、能力与资源清单（"依赖" = 旧前端每个能力的 GPUI 对应物）

### 2.1 视觉资源（必须原样移植，不允许字符近似）

| 资源 | 旧实现 | GPUI 方案 | 状态 |
|---|---|---|---|
| **OS 图标**（Ubuntu 圆环+三点、Debian 螺旋、Alpine 山、Windows 四格、其余两字母） | `OsBadge.vue` 内联 SVG，品牌色（Ubuntu #e95420 / Debian #e11d48） | 4 个 SVG 原子化到 `gpui-client/assets/icons/os/*.svg`，`svg()` 单色遮罩 + 品牌色；两字母回退保留 | **未做（现在是字符近似，必须替换）** |
| **品牌 Logo**（黑底圆角方块 + 白色双向 L 形滑块 + 中心圆点，三档尺寸 28/36/48） | `BrandLogo.vue` 内联 SVG + shadow + ring | 同源 SVG 到 `assets/logo.svg`，尺寸 sm/md/lg 三档 | **未做（现在是字符近似）** |
| UI 图标（房子/漏斗/刷新/太阳/月亮/齿轮/加号/×/星/置顶箭头/铅笔/拖拽手柄/分屏/文件夹/图表/外链） | 内联 Lucide SVG（stroke 2 / 1.8） | gpui-kit 图标集（Lucide 同源），逐图标核对名称、描边、尺寸 | 已用 Lucide，**待逐图标核对** |
| 字体 | `Inter → PingFang SC → Microsoft YaHei → sans-serif`（本机无 Inter，实测回退浏览器默认 sans） | 主题显式 `Segoe UI`；如仍不像，打包 Inter 字体文件用 `add_fonts` | 已定 Segoe UI（折中，记录差异） |

### 2.2 UI 能力 → gpui-kit 组件

| 旧前端能力 | 旧实现 | GPUI 组件（gpui-kit 0.7.3） |
|---|---|---|
| 输入框 / 搜索框 | `<input>` | `Input` / `InputState`（已用） |
| 下拉选择（跳板机、认证方式） | `<select>` | `Select` / `Combobox` |
| 弹窗（新建/编辑主机、删除确认） | 自绘遮罩 + 卡片 | `Dialog`（含遮罩、焦点陷阱、Esc） |
| 悬浮菜单（筛选面板、右键菜单） | 自绘 absolute | `Popover` / `Menu` / `PopupMenu`；**右键菜单用 `menu::ContextMenuExt`** |
| 应用内提示（Toast） | 自绘右下浮层 2.8s | 自绘（精确样式）或 `Notification` |
| 开关 / 复选框 | `<input type=checkbox>` | `Switch` / `Checkbox` |
| 滑块 | 无 | `Slider` |
| 标签页（设置页分类） | 自绘胶囊 | 自绘（已做）或 `Tab` |
| 树（文件浏览器后续可能用） | 自绘 | `Tree` |
| 表格（MCP 调用记录） | `<table>` | `DataTable` |
| 虚拟列表（大目录） | 自绘 | `VirtualList` |
| 图表（探针：圆环/柱状/曲线） | 手写 SVG | `chart` / `plot`；CPU 圆环用 `plot` 或自绘 Element |
| 代码编辑器（CodeMirror 6） | CodeMirror | `Input` 的 editor 模式 + `tree-sitter`（需开 feature + 语言包） |
| 剪贴板复制 | `navigator.clipboard` | gpui `ClipboardItem`（`cx.write_to_clipboard`） |
| 命令面板 / 搜索（Ctrl+K 聚焦搜索） | keydown | gpui `keymap` + `Action`，或手动 `on_key_down` |
| 滚动条 | 自定义 6px 圆角 | gpui-kit `Scrollable` 样式（对齐 6px、圆角、半透明） |
| 日期选择（调用记录筛选） | `input[type=date]` | **组件库无 DatePicker**——用文本框 `YYYY-MM-DD` 输入（记录差异） |

### 2.3 交互能力

| 能力 | 旧前端 | GPUI | 状态 |
|---|---|---|---|
| 单击 / 双击卡片 | `@click` / `@dblclick` | `on_click`（`click_count`） | 已做 |
| 悬停反馈 | CSS `hover:` | `.hover()` | 已做（无过渡，见 2.4） |
| 拖拽排序 | HTML5 drag | `on_drag` / `drag_over` / `on_drop` | 已做 |
| 文件拖放上传（拖进文件面板） | `@drop` | gpui `FileDropEvent`（窗口级） | 未做（板块三） |
| 快捷键 | Esc、Ctrl+K | `on_key_down` / keymap | 未接 |
| 右键菜单（**旧网页版没有**） | WebView2 默认菜单 | **终端右键菜单（复制/粘贴/清屏）**：已记录，待六大板块前端主干完成后统一用 `PopupMenu` 补上；**托盘右键菜单**在 M5 重建 | 待前端主干完成后补 |
| 复制（API Key / 路径 / 链接） | `navigator.clipboard` | gpui 剪贴板 | 未接 |
| 打开外部链接（GitHub release） | `<a target=_blank>` | `open` crate（系统浏览器） | **依赖未补** |
| 文件保存 / 选择对话框（下载、上传本机路径） | 浏览器下载 | `rfd` 原生对话框 | **依赖未补** |
| SSE 事件流（`/api/events`，agent 唤起终端） | `EventSource` | `reqwest` stream 特性手写 | **依赖未补** |
| 系统通知（提醒） | 无 | `notify-rust` | M5 |
| 独立终端窗口（旧版由 tao 桌面壳打开） | `POST /api/window/open-terminal` | GPUI `open_window` 自建（M6 替代旧壳时） | M5/M6 |
| 主题偏好持久化 | `localStorage` | 后端设置 API（`/api/settings`） | 待接 |

### 2.4 明确不可还原（不追求，只做记录）

| 项 | 原因 | 替代 |
|---|---|---|
| 背景模糊 `backdrop-blur` | GPUI 无背景模糊能力 | 半透明白 + 边框 + 阴影 |
| 径向渐变页面底 | GPUI 只有线性渐变 | 中间色近似；或自绘背景 Element（后续） |
| CSS 过渡动画 | 需逐个用 `with_animation` 手写 | 逐块补 hover 过渡 |
| 浏览器字体回退栈 | GPUI 按名取系统字体 | 主题定 `Segoe UI`；必要时打包 Inter |
| 浏览器窗口形态 | 旧版在浏览器里运行 | GPUI 原生窗口（产品形态变化，不是还原目标） |

---

## 三、依赖补齐（一次性，做完验收才进板块）

### 3.1 Cargo 依赖

| 依赖 | 用途 | 操作 |
|---|---|---|
| `gpui-kit` features：`tree-sitter` + `tree-sitter-bash/json/yaml/toml/python/javascript/css/html/markdown` | 代码编辑器高亮（板块三） | 加到 `gpui-client/Cargo.toml` |
| `rfd` | 原生文件选择/保存对话框（板块三/四：上传、下载） | 新增 |
| `open` | 系统浏览器打开链接（关于页） | 新增 |
| `reqwest` 增 `stream` 特性 | SSE `/api/events` | 改已有 |
| `gpui-tray`（M5） | 托盘图标与右键菜单 | 板块六前加 |
| `notify-rust`（M5） | 系统通知 | 板块六前加 |

### 3.2 资源文件

- `gpui-client/assets/icons/os/ubuntu.svg`、`debian.svg`、`alpine.svg`、`windows.svg`——从 `OsBadge.vue` 原样复制 path。
- `gpui-client/assets/logo.svg`——从 `BrandLogo.vue` 原样复制。
- 资源加载：gpui 的 `AssetSource`（gpui-kit `application().with_assets`），自定义图标用 `Icon::default().path(...)`（与 shell-rs 的 `OS_ICONS` 同法）。

### 3.3 依赖验收（逐个冒烟）

每项依赖补齐后写一个最小验证（编译通过 + 实际跑一次）：SVG 能渲染且颜色正确、Dialog 能弹出与关闭、ContextMenu 能右键弹出、Clipboard 能写入、rfd 能打开对话框、open 能拉起浏览器、SSE 能收到事件。

---

## 四、逐板块施工（先功能，后前端）

> 后端五大板块与 MCP 都已存在（v0.1.0 已发布），所以"功能"阶段主要是确认 API 契约；前端阶段按板块做透。

### 板块一：VPS 仓库（当前）

功能：`/api/hosts` CRUD、check / check-all、inspect、meta（收藏/置顶/标签/角色）、reorder、prewarm —— 全部已有。
前端块与顺序：
1. 卡片列表（含拖拽排序、状态四态）——**已做，待最终核对（含 OS 真图标替换）**
2. 顶栏首页部分（品牌、仓库按钮、搜索、筛选下拉、检测在线）——**已做，待核对**
3. 新建/编辑主机弹窗（表单：名称/地址/端口/用户名/认证方式/密码/私钥/口令/跳板/角色/标签/置顶/归档/快捷链接）——**未做**
4. 删除确认弹窗——**未做**
5. Toast——**已有简版，待按真值细化**
6. 详情抽屉的仓库部分（铭牌、角色、标签、快捷链接、底部操作栏）——**已有骨架，待核对与补快捷链接**
7. 快捷键（Ctrl+K 聚焦搜索、Esc 关闭浮层/弹窗）——**未做**

### 板块二：终端

功能：`/api/terminals`、WS 终端（0x00/0x01/0x02）、resize、paste —— 已有；终端渲染已建。
前端块：
1. 终端舞台顶栏（cwd 胶囊、运行中/退出码徽章、分屏、白黑底、文件/详情、独立窗口、收起）
2. 分屏画布与焦点环、多 pane 关闭按钮 hover 显隐
3. 断线/退出/加载/失败四条状态条与重连
4. 中文 IME 输入（InputHandler 完整实现）
5. Ctrl+C/Ctrl+V 语义（有选区复制、bracketed paste）
6. 独立窗口模式（standalone）

### 板块三：文件浏览器

功能：`/api/files/*`、/api/transfers —— 已有。
前端块：面包屑、文件列表（DataTable/VirtualList）、上传/下载（rfd + 原生对话框 + FileDropEvent）、压缩/解压/重命名/删除、在线编辑器（CodeMirror → Input editor + tree-sitter）、与终端 cwd 联动。

### 板块四：文件上传下载

前端块：传输队列面板（速率、进度、剩余时间、暂停/继续/取消）、拖拽上传、跨 VPS 直传入口。

### 板块五：探针

前端块：CPU 圆环（plot/自绘）、内存双色条、磁盘多分区、网络吞吐柱状谱与历史、实时刷新节流（2.8s 已接）。

### 板块六：MCP 与设置

前端块：设置页五分类（常规与自启 / 访问安全 / MCP Server / 调用记录 / 关于与更新）、命令黑名单编辑器、调用记录表格（DataTable + 日期筛选）、关于与更新（检查更新、外链 open）、托盘与自启（M5）。

---

## 五、每块施工清单

**开始前**：旧版该块截图存档 → Playwright 读真值 → 对照本文第二节确认用到的能力已补齐。
**实现中**：数值来自真值；SVG 原样移植；不做近似。
**完成后**：逐元素核对表（尺寸/颜色/间距/图标/hover/选中态）→ 无头 UI 测试 → 用户目视验收 → 通过才标 ✅。

## 六、当前进度（2026-10-18 更新）

- ✅ 方案与规格：`docs/gpui-client.md`、`docs/ui-spec.md`
- ✅ 视觉底座补齐：`AppAssets`（全套 Lucide + `BrandLogo.svg` + Simple Icons 官方 OS 图标）、`Palette` 亮暗双色板与线性渐变、52px 原生矢量 `cpu_ring`（同心居中）、渲染热路径 `opt-level = 3`、守护进程自动探活拉起
- ✅ 板块一（VPS 仓库）：顶栏、筛选下拉、主机卡片列表、新建/编辑主机弹窗、删除确认弹窗、`Ctrl+K` / `Escape`
- ✅ 板块二（终端）：终端舞台、分屏画布、256 色与反显渲染修复、中文 IME `EntityInputHandler`、鼠标拖拽选区与 `Ctrl+C`/`Ctrl+V`、指数退避重连、原生独立终端窗口
- ✅ 板块三 & 四（文件浏览器与上传下载）：双形态面板、分段面包屑与手输路径、目录缓存、新建/改名/删除/压缩/解压、内置代码编辑器 `Editor`（`Ctrl+S` 保存）、原生文件选择与拖拽上传、实时传输队列
- ✅ 板块五（探针详情抽屉）：6 大卡片 + 2.8s 实时采样 + 角色/标签/快捷链接 + 底部操作栏
- ✅ 板块六（系统设置与 MCP Server）：五大分类全量实现（常规与自启 / 访问安全 / MCP Server 与黑名单 / 调用记录展开 / 关于与更新）
- ✅ 细节与原生能力补全：
  1. 终端右键菜单（复制 / 粘贴 / 清屏 / 重新连接）
  2. SSE 实时事件流（`/api/events`，响应外部 Agent `revealTerminal` 唤起终端并附着指定 pane）
  3. 系统托盘常驻、关闭窗口行为（按 `close_action` 最小化到托盘 / 直接退出）、托盘右键主机列表秒开独立终端窗口、`build.rs` 图标内嵌

