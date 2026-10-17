# 旧前端 UI 规格（一比一复刻的基准）

> 2026-10-17 从 `frontend/src/App.vue`（3042 行）+ 各组件逐行提取。
> **GPUI 客户端以本文为准**：布局、尺寸、颜色、交互、状态流转都要对得上；
> 与本文冲突的实现是错的。文件/弹窗等未精读的组件在对应里程碑补全本文。

---

## 1. 设计语言

| 项 | 值 |
|---|---|
| 页面底 | 径向渐变 `#f9fafb → #f1f3f6`（深色 `#111215 → #090a0c`） |
| 毛玻璃面板 | `bg-white/85` + 细边框 `zinc-200/80` + 阴影；标题栏/筛选菜单/抽屉用 `white/75~95` |
| 边框 | `zinc-200/80`（深色 `zinc-800`）；分隔线 `zinc-100`（深色 `zinc-800`） |
| 文字 | 主 `zinc-900`（#18181b）/ 次 `zinc-600` / 弱 `zinc-400` / 灰 `zinc-500` |
| 主按钮 | `bg-zinc-900 text-white`，hover `zinc-800`（深色主题反相：`zinc-100` 底黑字） |
| 强调 | 新增按钮 `orange-500`；收藏 `amber-500`；成功 `emerald-500`；危险 `red-500/600` |
| 状态灯 | 在线 `emerald-500` + 外发光 `0 0 8px rgba(16,185,129,.55)`；离线 `red-500` + 发光；未知 `zinc-300`（深色 `zinc-600`）；检测中 `amber-400` 呼吸 |
| 圆角 | 卡片/面板 `16px`（rounded-2xl）；控件/胶囊按钮 `12px`（rounded-xl）；圆点、搜索框 `full` |
| 阴影 | 卡片 `0 1px 4px rgba(0,0,0,.02)`；选中 `0 4px 20px rgba(0,0,0,.05)` + `ring-1 zinc-900/10` |
| 动画 | 布局收缩 `300ms cubic-bezier(.22,1,.36,1)`；Toast `150ms`；进度条 `500ms` |
| 滚动条 | 6px，半透明黑 14%（hover 26%），深色反相 |
| 字号 | 标题 sm(14)/semibold；正文 xs(12)；极小 11px；徽章 10px |
| 终端字体 | `JetBrains Mono, Cascadia Mono, Consolas, PingFang SC, Microsoft YaHei, monospace`，13.5px，行高 1.32 |
| 终端主题 | 白底：`#ffffff` / `#18181b` / 光标 `#18181b`；黑底：`#0c0d10` / `#e4e4e7` / 光标 `#f4f4f5`；16 色 ANSI 见 `TerminalPaneView.vue` 的 LIGHT_THEME / DARK_THEME |

---

## 2. 页面级状态机

```
authEnabled && !authenticated  → 登录页（毛玻璃卡片）
isStandalone (?standalone=1)   → 独立终端窗口（无顶栏、无侧栏，全宽终端）
activePage === 'settings'      → 设置页（全页，非弹窗）
activePage === 'workspace'
   ├─ 无舞台（centerStage=null）→ 仓库全宽：主机卡片单栏列表（max-w-6xl 居中）
   └─ 有舞台（terminal|files）   → 三栏：左 336px（可收起）| 中 flex-1 | 右 336px（可选）
```

---

## 3. 顶栏（h=52px，粘顶，毛玻璃 white/75 + 底边框）

**左组**（flex-1，横向可滚动）：
1. 品牌：`BrandLogo sm` + 「VPS Switch」sm/bold
2. 「仓库」按钮：房子图标 + 仓库 + 计数胶囊 `在线/总数`（`bg-zinc-100` 圆角 full，10px mono）
   - 激活态（workspace 且无舞台）：白底 + 边框 + 阴影；否则透明 + hover 浅底
   - 行为：回全宽仓库视图（stage 关闭，会话保活）
3. 「侧栏」切换（仅有舞台时显示）：侧栏图标 + 「展开侧栏 / 收起侧栏」sm 以上可见文字
4. **会话标签**（每台打开的 host 一个，横向排列）：
   - 内容：状态灯 8px + 名称（max 128px 截断）+ `×N` pane 数徽章（>1 时）+ 关闭 ×（hover 出现）
   - 激活：白底 + 边框 + 阴影；非激活：`bg-zinc-100/60` 透明边框
   - 点击：激活该会话（恢复其 mode 与 rightWing）；×：关闭会话（DELETE 全部 panes，回退到最后一个标签或仓库）

**右组**（固定，间距 8px）：
1. 搜索框（sm 以上）：放大镜 + input，占位「搜索名称 / IP / 标签 (Ctrl+K)」，焦点时宽度 176→224px
   - 过滤：name / addr / port / username / osInfo.prettyName / tags 的子串（小写）
2. 筛选按钮（胶囊，漏斗图标 + 摘要）：
   - 无筛选：「筛选」；有筛选：深色按钮 + `筛选 · ★收藏 · 在线 · 主力 · #tag` + ×重置
   - 点击展开下拉（w=256，rounded-2xl 毛玻璃，右对齐）：
     - 在线状态：全部 / ●在线(N) / ●离线(N) 三宫格
     - 收藏与分类：★ 收藏(N)（琥珀激活）+ 全部分类/主力/代理/测试/归档
     - 标签：#tag 列表（可滚动 max-h-28，点击切换）
3. 「检测在线」按钮（刷新图标，检测中转圈+文案「检测中」）：调 `/api/hosts/check-all`，全量替换列表 + toast「机队快照同步完成：N 台主机在线」
4. 日夜切换圆钮（32px 圆形）
5. 设置圆钮（currentPage==settings 时深色激活）
6. **新增 VPS**：32px 圆形 `orange-500` 加号，hover `orange-600`

** Esc**：关闭筛选菜单 / 删除确认 / 新建弹窗（按此优先级）。

---

## 4. 仓库视图：主机卡片

容器：单栏 `space-y-2` 纵向列表，外层 max-width 1152px 居中，右侧留滚动条 4px。

**卡片**（rounded-2xl，px-4 py-3.5，毛玻璃 white/82，hover white/95 + 边框加深）：
- 选中态（stageHostId==host.id 或详情打开）：`border zinc-900/25 + 白底 + 阴影 + ring`
- 拖拽悬停：橙色虚线边框
- 左组：
  - 拖拽手柄（6 点矩阵，仅非本机，无舞台时显示，opacity 20）
  - OS 徽章：36~40px 圆形（无舞台 md=40px；舞台 sm=24px），zinc-100 底 + 图标（Ubuntu 橙/Debian 玫瑰/Alpine/Windows 四格/其他前两字母）
  - 状态灯 10px + 名称 sm/semibold + 「置顶」徽章 + 角色徽章（主力/代理/测试）
  - 第二行 mono 11px：`username@127.0.0.1`（本机）或 `username@addr:port`（SSH）+ `via 跳板名`
- 右组按钮（点击不冒泡）：
  - 「终端」按钮（rounded-xl 边框）：打开终端舞台；当前就是该主机的终端时深色激活
  - 「文件」按钮：打开文件舞台
  - ★ 收藏（amber 填充）→ PATCH meta
  - ⤒ 置顶（非本机）→ PATCH meta
  - ✎ 编辑 → 打开编辑弹窗
- **交互**：单击=右侧详情开/关（同卡片再点关闭）；双击=打开终端；拖拽排序（PUT `/api/hosts/reorder`，无本机参与）；点卡片时对未离线主机 POST `/api/files/prewarm`
- 空态：「没有匹配的 VPS 主机」（虚线框 40px padding）

---

## 5. 终端舞台（中央）

**外框**：rounded-2xl，`bg-white/95`，阴影 `0 4px 24px rgba(0,0,0,.04)`，min-width 420px。

**顶栏**（px-4 py-2.5，底边框 zinc-100）：
- 左：状态灯 + 主机名 sm/semibold + `PowerShell (ConPTY)` 或 `user@addr:port`（mono xs，sm 以上）+ cwd 胶囊（文件夹图标 + 路径，max 200px，mono 11px，边框圆角 8px）+ 运行中（琥珀底 + ping 点）/ 退出码（`✓ 0` 绿底或 `✗ N` 红底，mono 10px）
- 右工具条：
  - 分屏组（边框圆角 12px 内嵌两个按钮）：「左右分屏」「上下分屏」
  - 「白底 / 黑底」切换
  - 「文件管理」切换右栏 files（激活深色）
  - 「详情图表」切换右栏 detail（激活深色）
  - 「独立窗口」：POST `/api/window/open-terminal`
  - 收起 ✕：回仓库（终端后台保活）

**画布**：panes 横排（divide-x）或竖排（divide-y）；每个 pane 有 inset 焦点环；多 pane 时 hover 显示「关闭分屏 ✕」（右上角 2.5/8px）。

**pane 状态条**（吸附在终端底部）：
- 断线：琥珀条「连接中断，正在重连…」+ 「立即重连」按钮；6 次退避（400ms×2^n，上限 4s）后变「终端连接已断开」
- 退出：灰条「终端会话已结束（远端断开或进程退出）」+ 「重新连接」/「移除面板」
- 加载：转圈 + 「正在连接 {name} 终端...」（sm 以上）
- 失败：红图标 + 「连接 {name} 终端失败」+ 原因正文 + 「重试」

**终端输入约定**（沿用旧前端）：
- Ctrl+C：有选区=复制并清选区；无选区=发送 `0x03`
- Ctrl+V：读剪贴板 → 文本帧 `{"type":"paste","text":...}`
- 回放帧 `0x02`：整屏重置后写入（等价 `\x1bc`）
- 自动 DA 应答（`\x1b[?1;2c` 类）不转发

---

## 6. 右侧详情抽屉（336px，毛玻璃 white/88，rounded-2xl）

**顶栏**：状态灯 + 主机名 + 采样中 ping 点 | 刷新 / 收起 ×

**滚动区 6 块**（每块 rounded-xl 白卡片，边框，阴影 0 1px 2px）：

1. **环境铭牌**：OS 徽章 40px + prettyName（截断）+ TCP 拥塞控制徽章（含 bbr → 绿底）；第二行 mono `cpuModel || arch`；第三行三列：运行时间（`N天 H小时`/`H小时 M分`/`M分钟`）、内核/架构（取 kernel 首段）、SSH 响应 `N ms`
2. **CPU 算力与负载**：标题 + 「N 核」徽章 + 右侧「超售 Steal X.X%」（≥5% 橙色）；左 52px 圆环（背景 zinc-100，进度数字颜色：>80% 橙，否则深灰/白；r=22 stroke=4.5 从 -90° 起）+ 中心百分比；右侧 2×2 矩阵（1/5/15 分负载、IOWait）
3. **内存结构**：右对齐 `已用 / 总量`；双色条（实占 zinc-900/100，缓存 zinc-300/600）；图例「实占 N% / 缓存 X B」；右侧 `Swap: used/total (N%)` 或「未开启」
4. **磁盘存储**：单分区时右对齐 `used / total`，多分区「N 个挂载分区」；每分区 mono 行（挂载点 + used/total (N%)）+ 进度条（>85% 红，否则深灰/白）
5. **网络吞吐**：标题 + 网卡名徽章 + 右侧 `↓ emerald X/s` `↑ 深色 X/s`；20 根柱状（累计历史，rx 绿 55%、tx 灰 45%，min 6px，底高 48px）；底部 mono「累计下行 / 累计上行」
6. **角色、标签与快捷链接**：角色按钮组（未标记/主力/代理/测试/归档，选中深色）；标签 chips（#tag ×可删）+ 输入框「添加标签（回车保存）」+ 添加；快捷链接列表（name + url）

**底栏**（毛玻璃 zinc-50/60）：左「打开终端」（深色）+「文件浏览」；右「编辑」+「删除」（红，非本机）

**实时行为**：打开时立即可拉一次 `/api/hosts/:id/inspect`，之后每 **2.8s** 一次（上次返回后才发下一次，序号守卫丢弃过期响应）；网络历史按主机缓存 20 条。

---

## 7. 设置页（全页）

- 顶部：标题「系统设置」+ 副标题；右侧「退出登录」（enabled 时）+「返回仓库」
- 分类胶囊导航（毛玻璃圆角条）：常规与自启 / 访问安全 / MCP Server / 调用记录 / 关于与更新（有更新时末尾绿点 ping）
- **常规与自启**：开机自启开关（乐观更新，失败回滚 + toast）；关闭行为分段（最小化到托盘 / 直接退出）；界面日夜分段；终端白/黑底分段
- **访问安全**：开启密码开关 + 管理员账号 + 密码（已设置时提示留空不变）+ 保存按钮
- **MCP Server**：总开关（带状态圆点）；API Key 鉴权开关；Key 显示/隐藏/复制/重置；本机 Claude Code / Codex 同步状态 + 「立即同步」；接入配置 JSON 预览（远程 HTTP / 本机 Stdio 分段）+ 复制；**命令黑名单**（名单文件路径 + 复制、textarea 16 行、保存名单、格式说明）
- **调用记录**：筛选（起始/结束日期、主机、工具、结果）+ 计数文案 + 表格（▸ 展开、时间 mono、工具、主机、成功/失败徽章、耗时 ms、参数/摘要）+ 展开行拉取完整参数与返回（复制按钮、mono pre）
- **关于与更新**：品牌 + 版本徽章 + 描述；检查更新按钮（转圈）；结果卡片（有新版本绿 + release notes，无 = 当前最新）；运行时信息（端口、exe 路径+复制、GitHub 仓库+复制）

---

## 8. 弹窗与反馈

- **新建/编辑主机弹窗**：主机名称、IP/域名、端口、用户名、认证方式（密码/私钥）、密码/私钥/口令、角色分类、标签、置顶、归档、跳板机、快捷链接（名称+URL 列表）
- **删除确认**：毛玻璃遮罩 + 卡片，「确认删除 VPS 主机」+ 主机名/地址 + 取消/确认删除（红）
- **Toast**：右下 20px，2.8s，成功深色底白字 / 失败白底红字，圆角 12px，毛玻璃

---

## 9. 数据与接口

| 数据 | 接口 |
|---|---|
| 主机列表 | `GET /api/hosts` → `{hosts, inspections}` |
| 单主机检测 | `POST /api/hosts/:id/check` → `{host, inspection}` |
| 全量检测 | `POST /api/hosts/check-all` |
| 探针 | `GET /api/hosts/:id/inspect` → `{inspection, host}` |
| 元数据 | `PATCH /api/hosts/:id/meta`（favorited/pinned/tags/role） |
| 排序 | `PUT /api/hosts/reorder` `{orderedIds}` |
| 增改删 | `POST/PUT/DELETE /api/hosts[/:id]` |
| 终端 | `POST /api/terminals`、`DELETE /api/terminals/:id`、`GET /api/terminals`、`WS /api/ws/terminal/:id` |
| 文件预热 | `POST /api/files/prewarm` |
| 事件流 | `GET /api/events`（SSE，`revealTerminal` 事件唤起终端标签） |
| 设置/安全/MCP/更新 | `/api/settings*`、`/api/guard`、`/api/mcp-calls*`、`/api/system/*` |

---

## 10. GPUI 实现映射与已知差异

| 旧实现 | GPUI 实现 |
|---|---|
| `backdrop-blur` 毛玻璃 | **无背景模糊**：半透明白 + 边框 + 阴影近似；真模糊（Windows DWM）列为后续增强 |
| **三栏 flex + 顶栏会话标签** | 同样自绘：`h_flex` 三栏 + 自绘胶囊标签。**不用 DockArea**（Dock 的可拖拽面板与旧设计不同） |
| xterm.js | alacritty_terminal + 自绘 Element（已具备） |
| SVG 圆环/柱状/进度条 | GPUI 自绘（paint_quad 矩形与圆角；圆环用 PathBuilder 或分段描边） |
| OS 徽章 SVG | 单色图形可走 `svg()`；Ubuntu 的橙色图标需 `img()` 光栅化或自绘圆形近似 |
| Vue 响应式 | GPUI Entity + `cx.notify()`；全局状态集中在 Workspace 实体 |
| CSS transition | GPUI 动画（后期统一处理，先用硬切） |
| Toast | 自绘右下角浮层（2.8s 定时） |

---

## 11. 待补（对应里程碑再精读原组件）

- `FileBrowserPanel.vue`（1324 行）：双栏/单栏文件管理器、上传/下载/压缩/解压/重命名/删除/面包屑——M3 精读
- `HostModal.vue`（389 行）：表单细节与校验——M1 精读
- `CodeEditor.vue`（287 行）：CodeMirror 编辑器——M3 精读
- `BrandLogo.vue`（27 行）：品牌图标绘制
