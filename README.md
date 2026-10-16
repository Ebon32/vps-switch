# VPS Switch

轻量级 VPS 管理器与面向 AI Agent 远程操控 VPS 的 MCP Server。

VPS Switch 是一个本地优先的主机管理工具。它集成了终端、文件管理与系统探针等基础功能，并通过标准 MCP 协议为 AI Agent（Claude Code、Codex 等）提供多主机协同调度的远程执行底座。

一份纯 Rust 后端，桌面 UI 与 MCP 接口共用同一套高并发连接池与状态机。

---

## 多系统与异构环境适配

无论是桌面端的人工操作，还是外部 Agent 经由 MCP 下达调度指令，底层均提供透明的系统环境适配：

- **主流 Linux 发行版**：全面支持 Debian、Ubuntu、CentOS 等常见系统，并特别适配了 Alpine 等基于 musl libc / busybox (ash) 的超轻量精简环境；
- **Windows 远端主机**：针对 Windows OpenSSH 做了深度优化。底层自动转译为 PowerShell Base64 UTF-16LE 载荷并强制注入 UTF-8 输出，自动拦截清洗 PowerShell 5.1 CLIXML 管道噪音，彻底根除 cmd 转义卡死与中文乱码；
- **POSIX / BSD 体系**：基于标准 OpenSSH 协议栈构建，理论上兼容 macOS 等 BSD 兼容环境。

---

## 界面展示

### 1. 主机仓库与连接底座
单栏横条卡片列表，直观展示多台 VPS 在线状态（SSH 握手即亮，无需等待探针）；支持标签过滤与快速搜索。连接建立时轻量探测远端真实的 Linux 发行版、内核架构与硬件信息。

![主机仓库列表](docs/images/01-主机仓库列表.png)

### 2. 三栏联动工作台与终端
左侧迷你仓库、中间终端、右侧系统详情三栏联动作业。Rust 后端常驻持有终端网格状态（`alacritty_terminal`），切页与断线重连瞬间恢复现场；随包携带最新版 Windows ConPTY，支持 OSC 133 命令边界与退出码截获；支持一键将当前终端抽离为独立桌面窗口。

![三栏全景工作台](docs/images/06-三栏全景工作台.png)

### 3. 文件管理与流式传输
支持高并发 SFTP 目录树浏览，内嵌 CodeMirror 在线文本编辑器。文件传输走独立的流式传输引擎，支持断点续传残片对账与落地 SHA-256 闭环校验。

![文件管理器与传输队列](docs/images/04-文件管理器与传输队列.png)

### 4. 轻量探针看板
直接读取 Linux `/proc` 与 Windows 性能计数器差分计算，智能锁定主物理网卡，杜绝虚拟网卡叠加造成的网速虚高。按需刷新，打开看板时实时更新，平时后台静默不产生多余轮询。

![探针看板与详细信息](docs/images/03-探针看板与详细信息.png)

### 5. MCP 调用留痕与审计
外部 Agent 的每一次工具调用均在独立面板中全程留痕。支持按时间范围、目标主机、调用工具与成败状态多维筛选，可展开查看完整输入参数与执行结果。内置命令不可逆黑名单护栏（如 `rm -rf /` 等危险操作执行前直接拦截防手滑）。

![MCP调用留痕与多维审计](docs/images/07-MCP调用留痕与多维审计.png)

---

## 下载与安装

前往 [**GitHub Releases**](https://github.com/weidu12123/vps-switch/releases/latest) 获取最新版本：

### Windows
- **`vps-switch-0.1.0-windows-x64-setup.exe`（推荐）**：标准向导安装程序，自动配置快捷方式与系统环境变量。
- **`vps-switch-0.1.0-windows-x64-portable.zip`**：免安装绿色便携版，解压后双击 `vps-switch.exe` 即可使用。
- **`vps-switch-0.1.0-x86_64.msi`**：Windows 原生 MSI 包，适合命令行自动化部署。

> *Windows 首次运行提示：因未购买商业代码签名证书，Windows SmartScreen 可能会提示“未知发布者”，点击「更多信息」→「仍要运行」即可正常使用。*

### Linux (x86_64)
适用于将 VPS Switch 作为无头服务直接部署在云服务器或中转机上，Web 前端已完整内嵌在单一二进制中：

```bash
# 解压
tar -xzf vps-switch-*-linux-x86_64.tar.gz
cd vps-switch-*-linux-x86_64

# 快速运行（默认监听 18800 端口，浏览器访问 http://<你的IP>:18800）
./vps-switch

# 或使用附带的 systemd 脚本配置开机自启
sudo mkdir -p /opt/vps-switch
sudo cp vps-switch /opt/vps-switch/
sudo cp vps-switch.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now vps-switch
```

*Linux 首启默认开启 Web 登录密码，默认初始凭据为：`admin` / `vps-switch`（登录后可在设置面板修改）。*

---

## 接入 Claude Code / Agent

在设置页中开启 MCP Server 服务（本机 127.0.0.1 访问默认免 API Key 鉴权）。

### 一键接入 Claude Code
在终端中直接运行：

```bash
# 本机模式
claude mcp add vps-switch http://127.0.0.1:18800/mcp

# 远端模式（附带 API Key）
claude mcp add vps-switch http://<服务器IP>:18800/mcp -H "x-vps-switch-key: YOUR_API_KEY"
```

### Agent 可调用的核心工具

| 工具名称 | 作用说明 |
|---|---|
| `vps_list` | 查询已配置的主机列表与元数据，支持关键字搜索与状态过滤 |
| `vps_inspect` | 获取一台或多台主机的实时硬件指标（CPU、内存、磁盘、网速、负载） |
| `vps_exec` | 在一台或多台主机上并发执行命令，支持主机名寻址；自动路由适配 Linux（Shell）与 Windows（PowerShell 编码转译），Agent 无需关心系统差异；短任务同步返回，长任务支持阻塞等待 |
| `vps_task` | 查询或取消后台运行中的长时间任务，支持 `wait_seconds` 阻塞等待完成 |
| `vps_fs` | 远程文件系统操作（列目录、读写改删、压缩解压），带单文件读写大小保护 |
| `vps_transfer` | 高速文件流式传输（上传/下载），支持断点续传残片对账与落地 SHA-256 校验 |
| `vps_terminal` | 打开、读取或向交互式终端发送输入，遇到交互确认支持唤醒桌面 UI 人工介入 |

---

## 技术架构

- **后端**：Rust 2021 + Tokio + Axum（单一常驻进程，内存占用 ~30MB）
- **SSH 底座**：纯异步 `russh` 0.63 + `russh-sftp`（单主机多 channel 复用与跳板机穿透）
- **终端引擎**：`alacritty_terminal` 权威网格 + 随包 Windows ConPTY + 二进制 WebSocket
- **前端**：Vue 3 + TypeScript + Tailwind CSS + xterm.js + CodeMirror 6
- **本地存储**：单文件 SQLite（`rusqlite` bundled）+ 关键凭据 AES-GCM 本地加密
