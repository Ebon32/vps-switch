//! 板块六：系统设置与 MCP Server 管理页（1:1 对齐 `App.vue` 1844-2688 行 + `docs/ui-spec.md` §7）。
//!
//! 五大分类：
//! 1. `常规与自启`：Windows 开机自启开关（失败乐观回滚）、关闭窗口行为（最小化到托盘 / 直接退出）、界面昼夜主题、终端画布黑白底
//! 2. `访问安全`：Web 登录鉴权开关 + 管理员账号 + 管理员密码 + 保存登录设置
//! 3. `MCP Server`：总开关 + 远程 HTTP API Key 鉴权开关 + API Key 显示/隐藏/复制/重置 + 本机 Claude Code / Codex 自动写入状态与立即同步 + 客户端接入配置 JSON 预览（远程 HTTP / 本机 Stdio）+ 命令黑名单（16 行 Textarea + 复制路径 + 保存名单）
//! 4. `调用记录`：筛选条（起始/结束日期、主机、工具、结果、重置）+ 计数 + 调用记录表格 + 点击任意行展开完整参数与返回结果（支持复制）
//! 5. `关于与更新`：品牌 Logo + 版本号 + 检查更新按钮与反馈卡片 + 本地服务监听端口 + 可执行文件路径与复制 + GitHub 官方仓库链接与复制

use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::*;
use gpui_kit::*;

use crate::api::{ApiCommand, McpCallRecord, SettingsBundle, UpdateCheckOutcome};
use crate::assets::CatalogIcon;
use crate::palette::Palette;
use crate::widgets::{BrandLogoSize, brand_logo, lucide_icon};
use crate::workspace::{Page, ToastKind, Workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Security,
    Mcp,
    Calls,
    About,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpConfigPreviewMode {
    RemoteHttp,
    LocalStdio,
}

pub struct SettingsState {
    pub active_tab: SettingsTab,

    // 常规与自启
    pub autostart: bool,
    pub close_action: String, // "tray" | "exit"

    // 访问安全
    pub auth_enabled: bool,
    pub admin_username: Entity<InputState>,
    pub admin_password: Entity<InputState>,
    pub has_admin_password: bool,
    pub pending_username_sync: Option<String>,

    // MCP Server & 命令黑名单
    pub mcp_enabled: bool,
    pub mcp_require_api_key: bool,
    pub mcp_api_key: String,
    pub show_api_key: bool,
    pub claude_code_synced: bool,
    pub codex_synced: bool,
    pub config_preview_mode: McpConfigPreviewMode,
    pub guard_path: String,
    pub guard_editor: Entity<TextareaState>,
    pub pending_guard_sync: Option<String>,

    // 调用记录
    pub calls: Vec<McpCallRecord>,
    pub call_hosts: Vec<String>,
    pub call_tools: Vec<String>,
    pub filter_from: Entity<InputState>,
    pub filter_to: Entity<InputState>,
    pub filter_host: String,
    pub filter_tool: String,
    pub filter_ok: String, // "" | "true" | "false"
    pub expanded_call_id: Option<String>,
    pub expanded_detail: Option<(String, String)>,
    pub loading_detail: bool,

    // 关于与更新
    pub version: String,
    pub port: u16,
    pub exe_path: String,
    pub checking_update: bool,
    pub update_outcome: Option<UpdateCheckOutcome>,
}

impl SettingsState {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let admin_username = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("admin")
                .default_value("admin")
        });
        let admin_password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("设置或修改登录密码")
                .masked(true)
        });
        let guard_editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(16)
                .placeholder("# 一行一条，* 通配")
        });

        let filter_from = cx.new(|cx| InputState::new(window, cx).placeholder("YYYY-MM-DD"));
        cx.subscribe_in(
            &filter_from,
            window,
            |ws, _, event: &InputEvent, _window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    ws.reload_mcp_calls(cx);
                }
            },
        )
        .detach();

        let filter_to = cx.new(|cx| InputState::new(window, cx).placeholder("YYYY-MM-DD"));
        cx.subscribe_in(
            &filter_to,
            window,
            |ws, _, event: &InputEvent, _window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    ws.reload_mcp_calls(cx);
                }
            },
        )
        .detach();

        Self {
            active_tab: SettingsTab::General,
            autostart: false,
            close_action: "tray".into(),
            auth_enabled: false,
            admin_username,
            admin_password,
            has_admin_password: false,
            pending_username_sync: None,
            mcp_enabled: true,
            mcp_require_api_key: true,
            mcp_api_key: String::new(),
            show_api_key: false,
            claude_code_synced: true,
            codex_synced: true,
            config_preview_mode: McpConfigPreviewMode::RemoteHttp,
            guard_path: "data/deny-list.txt".into(),
            guard_editor,
            pending_guard_sync: None,
            calls: Vec::new(),
            call_hosts: Vec::new(),
            call_tools: Vec::new(),
            filter_from,
            filter_to,
            filter_host: String::new(),
            filter_tool: String::new(),
            filter_ok: String::new(),
            expanded_call_id: None,
            expanded_detail: None,
            loading_detail: false,
            version: "v0.1.0".into(),
            port: 18800,
            exe_path: "vps-switch.exe".into(),
            checking_update: false,
            update_outcome: None,
        }
    }

    pub fn apply_bundle(&mut self, bundle: SettingsBundle) {
        self.mcp_enabled = bundle.mcp_enabled;
        self.mcp_require_api_key = bundle.mcp_require_api_key;
        self.mcp_api_key = bundle.mcp_api_key;
        self.auth_enabled = bundle.auth_enabled;
        self.has_admin_password = bundle.has_admin_password;
        self.autostart = bundle.autostart;
        self.close_action = if bundle.close_action == "exit" {
            "exit".into()
        } else {
            "tray".into()
        };
        self.claude_code_synced = bundle.claude_code_synced;
        self.codex_synced = bundle.codex_synced;
        if !bundle.exe_path.is_empty() {
            self.exe_path = bundle.exe_path;
        }
        if bundle.port > 0 {
            self.port = bundle.port;
        }
        if !bundle.version.is_empty() {
            self.version = bundle.version;
        }
        if !bundle.guard_path.is_empty() {
            self.guard_path = bundle.guard_path;
        }
        self.pending_username_sync = Some(bundle.admin_username);
        self.pending_guard_sync = Some(bundle.guard_content);
    }

    pub fn masked_api_key(&self) -> String {
        if self.mcp_api_key.is_empty() {
            return "（未生成密钥）".into();
        }
        if self.show_api_key || self.mcp_api_key.len() <= 14 {
            return self.mcp_api_key.clone();
        }
        let prefix = &self.mcp_api_key[..9];
        let suffix = &self.mcp_api_key[self.mcp_api_key.len() - 4..];
        format!("{prefix}••••••••••••••••{suffix}")
    }

    pub fn displayed_mcp_config_json(&self) -> String {
        match self.config_preview_mode {
            McpConfigPreviewMode::RemoteHttp => {
                let url = format!("http://127.0.0.1:{}/mcp", self.port);
                let mut entry = serde_json::json!({
                    "type": "http",
                    "url": url,
                });
                if self.mcp_require_api_key && !self.mcp_api_key.is_empty() {
                    entry["headers"] = serde_json::json!({
                        "Authorization": format!("Bearer {}", self.mcp_api_key),
                    });
                }
                serde_json::to_string_pretty(&serde_json::json!({
                    "mcpServers": {
                        "vps-switch": entry
                    }
                }))
                .unwrap_or_default()
            }
            McpConfigPreviewMode::LocalStdio => serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "vps-switch": {
                        "type": "stdio",
                        "command": self.exe_path,
                        "args": ["--mcp-stdio"]
                    }
                }
            }))
            .unwrap_or_default(),
        }
    }

    pub fn has_call_filter(&self, cx: &App) -> bool {
        !self.filter_from.read(cx).value().trim().is_empty()
            || !self.filter_to.read(cx).value().trim().is_empty()
            || !self.filter_host.is_empty()
            || !self.filter_tool.is_empty()
            || !self.filter_ok.is_empty()
    }
}

impl Workspace {
    pub fn switch_settings_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        self.settings.active_tab = tab;
        if tab == SettingsTab::Calls {
            self.settings.expanded_call_id = None;
            self.settings.expanded_detail = None;
            self.reload_mcp_calls(cx);
        }
        cx.notify();
    }

    pub fn toggle_autostart(&mut self, cx: &mut Context<Self>) {
        let prev_auto = self.settings.autostart;
        let prev_close = self.settings.close_action.clone();
        self.settings.autostart = !prev_auto;
        self.api.send(ApiCommand::SaveAutostart {
            autostart: self.settings.autostart,
            close_action: prev_close.clone(),
            prev_autostart: prev_auto,
            prev_close_action: prev_close,
        });
        cx.notify();
    }

    pub fn set_close_action(&mut self, action: &str, cx: &mut Context<Self>) {
        let prev_auto = self.settings.autostart;
        let prev_close = self.settings.close_action.clone();
        self.settings.close_action = action.to_owned();
        self.api.send(ApiCommand::SaveAutostart {
            autostart: prev_auto,
            close_action: self.settings.close_action.clone(),
            prev_autostart: prev_auto,
            prev_close_action: prev_close,
        });
        cx.notify();
    }

    pub fn toggle_mcp_enabled(&mut self, cx: &mut Context<Self>) {
        let prev_en = self.settings.mcp_enabled;
        let prev_req = self.settings.mcp_require_api_key;
        self.settings.mcp_enabled = !prev_en;
        self.api.send(ApiCommand::SaveMcp {
            mcp_enabled: self.settings.mcp_enabled,
            mcp_require_api_key: prev_req,
            regenerate_key: false,
            prev_enabled: prev_en,
            prev_require_key: prev_req,
        });
        cx.notify();
    }

    pub fn toggle_mcp_require_key(&mut self, cx: &mut Context<Self>) {
        let prev_en = self.settings.mcp_enabled;
        let prev_req = self.settings.mcp_require_api_key;
        self.settings.mcp_require_api_key = !prev_req;
        self.api.send(ApiCommand::SaveMcp {
            mcp_enabled: prev_en,
            mcp_require_api_key: self.settings.mcp_require_api_key,
            regenerate_key: false,
            prev_enabled: prev_en,
            prev_require_key: prev_req,
        });
        cx.notify();
    }

    pub fn regenerate_mcp_key(&mut self, cx: &mut Context<Self>) {
        let en = self.settings.mcp_enabled;
        let req = self.settings.mcp_require_api_key;
        self.api.send(ApiCommand::SaveMcp {
            mcp_enabled: en,
            mcp_require_api_key: req,
            regenerate_key: true,
            prev_enabled: en,
            prev_require_key: req,
        });
        cx.notify();
    }

    pub fn save_auth_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let username = self
            .settings
            .admin_username
            .read(cx)
            .value()
            .trim()
            .to_owned();
        let pwd = self.settings.admin_password.read(cx).value().to_string();
        if !pwd.trim().is_empty() {
            self.settings.auth_enabled = true;
        }
        self.settings
            .admin_password
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.api.send(ApiCommand::SaveAuth {
            auth_enabled: self.settings.auth_enabled,
            username: if username.is_empty() {
                "admin".into()
            } else {
                username
            },
            password: (!pwd.is_empty()).then_some(pwd),
        });
        cx.notify();
    }

    pub fn save_guard_list(&mut self, cx: &mut Context<Self>) {
        let content = self.settings.guard_editor.read(cx).value().to_string();
        self.api.send(ApiCommand::SaveGuard { content });
        cx.notify();
    }

    pub fn reload_mcp_calls(&mut self, cx: &mut Context<Self>) {
        let from = self.settings.filter_from.read(cx).value().trim().to_owned();
        let to = self.settings.filter_to.read(cx).value().trim().to_owned();
        self.api.send(ApiCommand::LoadMcpCalls {
            from,
            to,
            host: self.settings.filter_host.clone(),
            tool: self.settings.filter_tool.clone(),
            ok: self.settings.filter_ok.clone(),
        });
        cx.notify();
    }

    pub fn reset_mcp_call_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings
            .filter_from
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.settings
            .filter_to
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.settings.filter_host.clear();
        self.settings.filter_tool.clear();
        self.settings.filter_ok.clear();
        self.reload_mcp_calls(cx);
    }

    pub fn toggle_call_expand(&mut self, call_id: &str, cx: &mut Context<Self>) {
        if self.settings.expanded_call_id.as_deref() == Some(call_id) {
            self.settings.expanded_call_id = None;
            self.settings.expanded_detail = None;
        } else {
            self.settings.expanded_call_id = Some(call_id.to_owned());
            self.settings.expanded_detail = None;
            self.settings.loading_detail = true;
            self.api.send(ApiCommand::LoadMcpCallDetail {
                call_id: call_id.to_owned(),
            });
        }
        cx.notify();
    }
}

pub fn render(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
    cx: &App,
) -> impl IntoElement {
    let this_back = this.clone();
    let active_tab = workspace.settings.active_tab;

    // 顶部标题与返回按钮
    let top_header = h_flex()
        .items_center()
        .justify_between()
        .child(
            v_flex()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(palette.text_primary)
                        .child("系统设置"),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(palette.text_faint)
                        .child("管理 MCP Server 远程/本机接入、调用记录与 Web 访问安全"),
                ),
        )
        .child(
            div()
                .id("settings-back-home")
                .test_support()
                .rounded_xl()
                .border_1()
                .border_color(palette.border)
                .bg(palette.card_solid_bg)
                .px(px(14.))
                .py(px(6.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette.text_secondary)
                .cursor_pointer()
                .hover(|s| s.bg(palette.subtle_bg).text_color(palette.text_primary))
                .on_click(move |_, _, cx| {
                    let _ = this_back.update(cx, |w, cx| {
                        w.page = Page::Workspace;
                        cx.notify();
                    });
                })
                .child("返回仓库"),
        );

    // 五大分类胶囊导航条
    let tabs: [(&'static str, &'static str, SettingsTab, CatalogIcon); 5] = [
        (
            "常规与自启",
            "general",
            SettingsTab::General,
            CatalogIcon::Settings,
        ),
        (
            "访问安全",
            "security",
            SettingsTab::Security,
            CatalogIcon::Lock,
        ),
        (
            "MCP Server",
            "mcp",
            SettingsTab::Mcp,
            CatalogIcon::Zap,
        ),
        (
            "调用记录",
            "calls",
            SettingsTab::Calls,
            CatalogIcon::FileText,
        ),
        (
            "关于与更新",
            "about",
            SettingsTab::About,
            CatalogIcon::Info,
        ),
    ];

    let mut nav = h_flex()
        .items_center()
        .gap(px(6.))
        .p(px(6.))
        .rounded_2xl()
        .border_1()
        .border_color(palette.border)
        .bg(palette.panel_bg);

    for (label, slug, tab_key, icon_name) in tabs {
        let active = active_tab == tab_key;
        let this_tab = this.clone();
        let fg = if active {
            palette.primary_fg
        } else {
            palette.text_secondary
        };
        let mut btn = h_flex()
            .id(SharedString::from(format!("settings-tab-{slug}")))
            .test_support()
            .items_center()
            .gap(px(8.))
            .rounded_xl()
            .px(px(16.))
            .py(px(8.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _ = this_tab.update(cx, |w, cx| w.switch_settings_tab(tab_key, cx));
            })
            .child(lucide_icon(icon_name, 14., fg))
            .child(label);

        if tab_key == SettingsTab::About
            && workspace
                .settings
                .update_outcome
                .as_ref()
                .is_some_and(|u| u.has_update)
        {
            btn = btn.child(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(palette.status_online),
            );
        }

        nav = nav.child(if active {
            btn.bg(palette.primary_bg)
                .text_color(palette.primary_fg)
                .shadow_sm()
        } else {
            let subtle = palette.subtle_bg;
            btn.text_color(palette.text_secondary)
                .hover(move |s| s.bg(subtle))
        });
    }

    let content = match active_tab {
        SettingsTab::General => render_general_tab(workspace, this, palette),
        SettingsTab::Security => render_security_tab(workspace, this, palette),
        SettingsTab::Mcp => render_mcp_tab(workspace, this, palette),
        SettingsTab::Calls => render_calls_tab(workspace, this, palette, cx),
        SettingsTab::About => render_about_tab(workspace, this, palette),
    };

    div()
        .id("settings-scroll")
        .test_support()
        .size_full()
        .min_h_0()
        .overflow_y_scroll()
        .p(px(20.))
        .child(
            v_flex()
                .w_full()
                .max_w(px(896.))
                .mx_auto()
                .gap(px(16.))
                .child(top_header)
                .child(nav)
                .child(content),
        )
}

fn section_shell(palette: &Palette) -> Div {
    div()
        .rounded_2xl()
        .border_1()
        .border_color(palette.border)
        .bg(palette.panel_bg)
        .p(px(20.))
        .shadow(palette.card_shadow(false))
}

fn inner_row_shell(palette: &Palette) -> Div {
    div()
        .rounded_xl()
        .border_1()
        .border_color(palette.border)
        .bg(palette.muted_surface_bg)
        .p(px(14.))
}

fn check_box(checked: bool, palette: &Palette) -> Div {
    let b = div()
        .size(px(16.))
        .rounded(px(4.))
        .border_1()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.));
    if checked {
        b.border_color(palette.primary_bg)
            .bg(palette.primary_bg)
            .text_color(palette.primary_fg)
            .child("✓")
    } else {
        b.border_color(palette.border).bg(palette.card_solid_bg)
    }
}

fn render_general_tab(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
) -> AnyElement {
    let autostart = workspace.settings.autostart;
    let close_is_tray = workspace.settings.close_action != "exit";
    let is_dark = workspace.is_dark;
    let term_dark = workspace.terminal_dark;

    let this_auto = this.clone();
    let this_tray = this.clone();
    let this_exit = this.clone();
    let this_light = this.clone();
    let this_dark = this.clone();
    let this_term_light = this.clone();
    let this_term_dark = this.clone();

    let card1 = section_shell(palette).child(
        v_flex()
            .gap(px(14.))
            .child(
                v_flex()
                    .pb(px(12.))
                    .border_b_1()
                    .border_color(palette.divider)
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.text_primary)
                            .child("开机自启与后台常驻"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(palette.text_faint)
                            .child("配置 Windows 开机静默后台启动与窗口关闭时的托盘常驻行为"),
                    ),
            )
            .child(
                inner_row_shell(palette).child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            v_flex()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette.text_primary)
                                        .child("Windows 登录时自动启动"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(palette.text_faint)
                                        .child("开机登录后自动在后台启动 VPS Switch 并静默常驻系统托盘，无需手动双击运行"),
                                ),
                        )
                        .child(
                            h_flex()
                                .id("settings-toggle-autostart")
                                .test_support()
                                .items_center()
                                .gap(px(8.))
                                .cursor_pointer()
                                .on_click(move |_, _, cx| {
                                    let _ = this_auto.update(cx, |w, cx| w.toggle_autostart(cx));
                                })
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette.text_secondary)
                                        .child(if autostart { "已开启" } else { "已关闭" }),
                                )
                                .child(check_box(autostart, palette)),
                        ),
                ),
            )
            .child(
                inner_row_shell(palette).child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            v_flex()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette.text_primary)
                                        .child("点击窗口右上角关闭按钮 (✕) 时"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(palette.text_faint)
                                        .child("推荐最小化到系统托盘，保持 SSH 连接池与 MCP Server 在后台随时可用"),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap(px(4.))
                                .p(px(3.))
                                .rounded_xl()
                                .bg(palette.subtle_bg)
                                .child(seg_btn(
                                    "settings-close-tray",
                                    "最小化到托盘",
                                    close_is_tray,
                                    palette,
                                    move |_, _, cx| {
                                        let _ = this_tray
                                            .update(cx, |w, cx| w.set_close_action("tray", cx));
                                    },
                                ))
                                .child(seg_btn(
                                    "settings-close-exit",
                                    "直接退出程序",
                                    !close_is_tray,
                                    palette,
                                    move |_, _, cx| {
                                        let _ = this_exit
                                            .update(cx, |w, cx| w.set_close_action("exit", cx));
                                    },
                                )),
                        ),
                ),
            ),
    );

    let card2 = section_shell(palette).child(
        h_flex()
            .gap(px(12.))
            .child(
                inner_row_shell(palette).flex_1().child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(
                            v_flex()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette.text_primary)
                                        .child("界面昼夜主题"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(palette.text_faint)
                                        .child("日间白色毛玻璃 / 夜间深空黑毛玻璃"),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap(px(4.))
                                .p(px(3.))
                                .rounded_xl()
                                .bg(palette.subtle_bg)
                                .child(seg_btn(
                                    "settings-theme-light",
                                    "☀ 日间",
                                    !is_dark,
                                    palette,
                                    move |_, _, cx| {
                                        let _ = this_light.update(cx, |w, cx| {
                                            if w.is_dark {
                                                w.toggle_theme(cx);
                                            }
                                        });
                                    },
                                ))
                                .child(seg_btn(
                                    "settings-theme-dark",
                                    "☾ 夜间",
                                    is_dark,
                                    palette,
                                    move |_, _, cx| {
                                        let _ = this_dark.update(cx, |w, cx| {
                                            if !w.is_dark {
                                                w.toggle_theme(cx);
                                            }
                                        });
                                    },
                                )),
                        ),
                ),
            )
            .child(
                inner_row_shell(palette).flex_1().child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(
                            v_flex()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette.text_primary)
                                        .child("终端画布配色"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(palette.text_faint)
                                        .child("独立切换终端区域的白底或黑底"),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap(px(4.))
                                .p(px(3.))
                                .rounded_xl()
                                .bg(palette.subtle_bg)
                                .child(seg_btn(
                                    "settings-term-light",
                                    "白底",
                                    !term_dark,
                                    palette,
                                    move |_, _, cx| {
                                        let _ = this_term_light.update(cx, |w, cx| {
                                            if w.terminal_dark {
                                                w.toggle_terminal_dark(cx);
                                            }
                                        });
                                    },
                                ))
                                .child(seg_btn(
                                    "settings-term-dark",
                                    "黑底",
                                    term_dark,
                                    palette,
                                    move |_, _, cx| {
                                        let _ = this_term_dark.update(cx, |w, cx| {
                                            if !w.terminal_dark {
                                                w.toggle_terminal_dark(cx);
                                            }
                                        });
                                    },
                                )),
                        ),
                ),
            ),
    );

    v_flex()
        .id("settings-panel-general")
        .test_support()
        .gap(px(16.))
        .child(card1)
        .child(card2)
        .into_any_element()
}

fn seg_btn(
    id: &'static str,
    label: &'static str,
    active: bool,
    palette: &Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let base = div()
        .id(id)
        .test_support()
        .rounded_lg()
        .px(px(10.))
        .py(px(4.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .on_click(on_click)
        .child(label);
    if active {
        base.bg(palette.card_solid_bg)
            .text_color(palette.text_primary)
            .shadow_sm()
    } else {
        base.text_color(palette.text_muted)
    }
}

fn render_security_tab(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
) -> AnyElement {
    let s = &workspace.settings;
    let auth_on = s.auth_enabled;
    let pwd_label = if s.has_admin_password {
        "管理员密码（已设置，留空保持不变）"
    } else {
        "管理员密码"
    };
    let this_toggle = this.clone();
    let this_save = this.clone();

    section_shell(palette)
        .id("settings-panel-security")
        .test_support()
        .child(
            v_flex()
                .gap(px(16.))
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .pb(px(12.))
                        .border_b_1()
                        .border_color(palette.divider)
                        .child(
                            v_flex()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette.text_primary)
                                        .child("Web 登录鉴权"),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(palette.text_faint)
                                        .child("Windows 本机免密直进；VPS 公网部署默认开启密码，防止未授权访问"),
                                ),
                        )
                        .child(
                            h_flex()
                                .id("settings-toggle-auth")
                                .test_support()
                                .items_center()
                                .gap(px(8.))
                                .cursor_pointer()
                                .on_click(move |_, _, cx| {
                                    let _ = this_toggle.update(cx, |w, cx| {
                                        w.settings.auth_enabled = !w.settings.auth_enabled;
                                        cx.notify();
                                    });
                                })
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette.text_secondary)
                                        .child("开启密码"),
                                )
                                .child(check_box(auth_on, palette)),
                        ),
                )
                .child(
                    h_flex()
                        .items_end()
                        .gap(px(12.))
                        .child(
                            v_flex()
                                .flex_1()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette.text_secondary)
                                        .child("管理员账号"),
                                )
                                .child(Input::new(&s.admin_username)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette.text_secondary)
                                        .child(pwd_label),
                                )
                                .child(Input::new(&s.admin_password)),
                        )
                        .child(
                            div()
                                .id("settings-save-auth")
                                .test_support()
                                .rounded_xl()
                                .bg(palette.primary_bg)
                                .text_color(palette.primary_fg)
                                .px(px(16.))
                                .py(px(8.))
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .on_click(move |_, window, cx| {
                                    let _ = this_save
                                        .update(cx, |w, cx| w.save_auth_settings(window, cx));
                                })
                                .child("保存登录设置"),
                        ),
                ),
        )
        .into_any_element()
}

fn render_mcp_tab(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
) -> AnyElement {
    let s = &workspace.settings;
    let mcp_on = s.mcp_enabled;
    let req_key = s.mcp_require_api_key;
    let show_key = s.show_api_key;
    let key_display = s.masked_api_key();
    let raw_key = s.mcp_api_key.clone();
    let json_preview = s.displayed_mcp_config_json();
    let json_for_copy = json_preview.clone();
    let guard_path = s.guard_path.clone();
    let guard_path_copy = guard_path.clone();

    let this_mcp_on = this.clone();
    let this_req_key = this.clone();
    let this_show_key = this.clone();
    let this_copy_key = this.clone();
    let this_regen_key = this.clone();
    let this_sync_cli = this.clone();
    let this_mode_remote = this.clone();
    let this_mode_stdio = this.clone();
    let this_copy_json = this.clone();
    let this_save_guard = this.clone();
    let this_copy_guard = this.clone();

    let card1 = section_shell(palette).child(
        v_flex()
            .gap(px(16.))
            // 顶部开关
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .pb(px(12.))
                    .border_b_1()
                    .border_color(palette.divider)
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .size(px(10.))
                                    .rounded_full()
                                    .bg(if mcp_on {
                                        palette.status_online
                                    } else {
                                        palette.status_unknown
                                    }),
                            )
                            .child(
                                v_flex()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette.text_primary)
                                            .child("MCP Server 服务与 API Key"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(palette.text_faint)
                                            .child("支持本机 Stdio 自动接入，或部署在 VPS 上通过 HTTP + API Key 供远程调用"),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .id("settings-toggle-mcp")
                            .test_support()
                            .items_center()
                            .gap(px(8.))
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                let _ = this_mcp_on.update(cx, |w, cx| w.toggle_mcp_enabled(cx));
                            })
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette.text_secondary)
                                    .child(if mcp_on { "已启用" } else { "已关闭" }),
                            )
                            .child(check_box(mcp_on, palette)),
                    ),
            )
            // API Key 鉴权与密钥框
            .child(
                h_flex()
                    .items_stretch()
                    .gap(px(12.))
                    .child(
                        inner_row_shell(palette).flex_1().child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .gap(px(10.))
                                .child(
                                    v_flex()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(palette.text_primary)
                                                .child("远程 HTTP 要求 API Key 鉴权"),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(palette.text_faint)
                                                .child("部署在公网 VPS 时请开启，拒绝未携带有效 API Key 的外部请求"),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("settings-toggle-mcp-key-req")
                                        .test_support()
                                        .cursor_pointer()
                                        .on_click(move |_, _, cx| {
                                            let _ = this_req_key
                                                .update(cx, |w, cx| w.toggle_mcp_require_key(cx));
                                        })
                                        .child(check_box(req_key, palette)),
                                ),
                        ),
                    )
                    .child(
                        inner_row_shell(palette).flex_1().child(
                            v_flex()
                                .gap(px(8.))
                                .child(
                                    h_flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(palette.text_primary)
                                                .child("MCP API Key"),
                                        )
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .gap(px(10.))
                                                .text_size(px(11.))
                                                .child(
                                                    div()
                                                        .id("settings-show-api-key")
                                                        .test_support()
                                                        .text_color(palette.text_muted)
                                                        .cursor_pointer()
                                                        .hover(|s| s.text_color(palette.text_primary))
                                                        .on_click(move |_, _, cx| {
                                                            let _ = this_show_key.update(
                                                                cx,
                                                                |w, cx| {
                                                                    w.settings.show_api_key =
                                                                        !w.settings.show_api_key;
                                                                    cx.notify();
                                                                },
                                                            );
                                                        })
                                                        .child(if show_key { "隐藏" } else { "显示" }),
                                                )
                                                .child(
                                                    div()
                                                        .id("settings-copy-api-key")
                                                        .test_support()
                                                        .text_color(palette.text_muted)
                                                        .cursor_pointer()
                                                        .hover(|s| s.text_color(palette.text_primary))
                                                        .on_click(move |_, _, cx| {
                                                            let k = raw_key.clone();
                                                            cx.write_to_clipboard(
                                                                ClipboardItem::new_string(k),
                                                            );
                                                            let _ = this_copy_key.update(
                                                                cx,
                                                                |w, cx| {
                                                                    w.show_toast(
                                                                        "已复制 MCP API Key",
                                                                        ToastKind::Ok,
                                                                        cx,
                                                                    );
                                                                },
                                                            );
                                                        })
                                                        .child("复制"),
                                                )
                                                .child(
                                                    div()
                                                        .id("settings-regen-api-key")
                                                        .test_support()
                                                        .text_color(palette.accent_orange)
                                                        .cursor_pointer()
                                                        .on_click(move |_, _, cx| {
                                                            let _ = this_regen_key.update(
                                                                cx,
                                                                |w, cx| w.regenerate_mcp_key(cx),
                                                            );
                                                        })
                                                        .child("重置密钥"),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(palette.border)
                                        .bg(palette.card_solid_bg)
                                        .px(px(10.))
                                        .py(px(6.))
                                        .text_size(px(12.))
                                        .text_color(palette.text_primary)
                                        .truncate()
                                        .child(key_display),
                                ),
                        ),
                    ),
            )
            // 本机 CLI 同步状态
            .child(
                inner_row_shell(palette).child(
                    v_flex()
                        .gap(px(10.))
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette.text_primary)
                                        .child("本机 Claude Code / Codex 自动写入状态"),
                                )
                                .child(
                                    div()
                                        .id("settings-sync-cli")
                                        .test_support()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(palette.border)
                                        .bg(palette.card_solid_bg)
                                        .px(px(10.))
                                        .py(px(4.))
                                        .text_size(px(11.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette.text_secondary)
                                        .cursor_pointer()
                                        .hover(|s| s.bg(palette.subtle_bg))
                                        .on_click(move |_, _, cx| {
                                            let _ = this_sync_cli.update(cx, |w, _| {
                                                w.api.send(ApiCommand::SyncCli);
                                            });
                                        })
                                        .child("立即同步到本机 CLI"),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap(px(10.))
                                .child(cli_status_box(
                                    "Claude Code",
                                    "~/.claude.json",
                                    s.claude_code_synced,
                                    palette,
                                ))
                                .child(cli_status_box(
                                    "Codex CLI",
                                    "~/.codex/config.toml",
                                    s.codex_synced,
                                    palette,
                                )),
                        ),
                ),
            )
            // 客户端接入配置 JSON 预览
            .child(
                v_flex()
                    .gap(px(8.))
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette.text_primary)
                                            .child("客户端接入配置预览"),
                                    )
                                    .child(
                                        h_flex()
                                            .gap(px(4.))
                                            .p(px(3.))
                                            .rounded_xl()
                                            .bg(palette.subtle_bg)
                                            .child(seg_btn(
                                                "settings-preview-remote",
                                                "远程 HTTP + API Key",
                                                s.config_preview_mode
                                                    == McpConfigPreviewMode::RemoteHttp,
                                                palette,
                                                move |_, _, cx| {
                                                    let _ = this_mode_remote.update(cx, |w, cx| {
                                                        w.settings.config_preview_mode =
                                                            McpConfigPreviewMode::RemoteHttp;
                                                        cx.notify();
                                                    });
                                                },
                                            ))
                                            .child(seg_btn(
                                                "settings-preview-stdio",
                                                "本机 Stdio",
                                                s.config_preview_mode
                                                    == McpConfigPreviewMode::LocalStdio,
                                                palette,
                                                move |_, _, cx| {
                                                    let _ = this_mode_stdio.update(cx, |w, cx| {
                                                        w.settings.config_preview_mode =
                                                            McpConfigPreviewMode::LocalStdio;
                                                        cx.notify();
                                                    });
                                                },
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .id("settings-copy-config-json")
                                    .test_support()
                                    .rounded_lg()
                                    .bg(palette.primary_bg)
                                    .text_color(palette.primary_fg)
                                    .px(px(12.))
                                    .py(px(4.))
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .cursor_pointer()
                                    .on_click(move |_, _, cx| {
                                        let txt = json_for_copy.clone();
                                        cx.write_to_clipboard(ClipboardItem::new_string(txt));
                                        let _ = this_copy_json.update(cx, |w, cx| {
                                            w.show_toast(
                                                "已复制 MCP 配置 JSON",
                                                ToastKind::Ok,
                                                cx,
                                            );
                                        });
                                    })
                                    .child("复制配置 JSON"),
                            ),
                    )
                    .child(
                        div()
                            .rounded_xl()
                            .border_1()
                            .border_color(palette.border)
                            .bg(rgb(0x09090b))
                            .p(px(14.))
                            .text_size(px(11.))
                            .text_color(rgb(0xe4e4e7))
                            .child(json_preview),
                    ),
            ),
    );

    // 板块 2：命令黑名单
    let card2 = section_shell(palette).child(
        v_flex()
            .gap(px(14.))
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .pb(px(12.))
                    .border_b_1()
                    .border_color(palette.divider)
                    .child(
                        v_flex()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette.text_primary)
                                    .child("命令黑名单（Agent 执行护栏）"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(palette.text_faint)
                                    .child("命中即拒绝执行，agent 拿不到任何执行结果。这里是外部 agent 调用的唯一安全兜底"),
                            ),
                    )
                    .child(
                        div()
                            .id("settings-save-guard")
                            .test_support()
                            .flex_shrink_0()
                            .rounded_lg()
                            .bg(palette.primary_bg)
                            .text_color(palette.primary_fg)
                            .px(px(12.))
                            .py(px(5.))
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                let _ = this_save_guard.update(cx, |w, cx| w.save_guard_list(cx));
                            })
                            .child("保存名单"),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .rounded_xl()
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.muted_surface_bg)
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(11.))
                    .child(
                        h_flex()
                            .min_w_0()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette.text_muted)
                                    .child("名单文件"),
                            )
                            .child(
                                div()
                                    .text_color(palette.text_primary)
                                    .truncate()
                                    .child(guard_path),
                            ),
                    )
                    .child(
                        div()
                            .id("settings-copy-guard-path")
                            .test_support()
                            .text_color(palette.text_muted)
                            .cursor_pointer()
                            .hover(|s| s.text_color(palette.text_primary))
                            .on_click(move |_, _, cx| {
                                let p = guard_path_copy.clone();
                                cx.write_to_clipboard(ClipboardItem::new_string(p));
                                let _ = this_copy_guard.update(cx, |w, cx| {
                                    w.show_toast("已复制名单文件路径", ToastKind::Ok, cx);
                                });
                            })
                            .child("复制路径"),
                    ),
            )
            .child(Textarea::new(&s.guard_editor))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(palette.text_faint)
                    .child("一行一条，# 起头是注释。* 通配，没有 * 就是精确匹配，按词边界匹配（rm -rf / 不会误伤 rm -rf /var/log）。防手滑，不防蓄意绕过。"),
            ),
    );

    v_flex()
        .id("settings-panel-mcp")
        .test_support()
        .gap(px(16.))
        .child(card1)
        .child(card2)
        .into_any_element()
}

fn cli_status_box(
    title: &'static str,
    path: &'static str,
    synced: bool,
    palette: &Palette,
) -> impl IntoElement {
    h_flex()
        .flex_1()
        .items_center()
        .justify_between()
        .rounded_lg()
        .border_1()
        .border_color(palette.border)
        .bg(palette.card_solid_bg)
        .px(px(12.))
        .py(px(8.))
        .child(
            v_flex()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text_primary)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(palette.text_faint)
                        .child(path),
                ),
        )
        .child(
            div()
                .rounded(px(4.))
                .px(px(8.))
                .py(px(2.))
                .text_size(px(10.))
                .font_weight(FontWeight::MEDIUM)
                .bg(if synced {
                    Hsla::from(rgb(0xecfdf5))
                } else {
                    palette.subtle_bg
                })
                .text_color(if synced {
                    Hsla::from(rgb(0x047857))
                } else {
                    palette.text_muted
                })
                .child(if synced {
                    "✓ 已自动写入"
                } else {
                    "未检测到"
                }),
        )
}

fn render_calls_tab(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
    cx: &App,
) -> AnyElement {
    let s = &workspace.settings;
    let filtered = s.has_call_filter(cx);
    let this_refresh = this.clone();
    let this_host_cycle = this.clone();
    let this_tool_cycle = this.clone();
    let this_ok_cycle = this.clone();
    let this_reset = this.clone();

    let host_label = if s.filter_host.is_empty() {
        "全部主机".to_owned()
    } else {
        s.filter_host.clone()
    };
    let tool_label = if s.filter_tool.is_empty() {
        "全部工具".to_owned()
    } else {
        s.filter_tool.clone()
    };
    let ok_label = match s.filter_ok.as_str() {
        "true" => "成功",
        "false" => "失败",
        _ => "全部",
    };

    let mut filter_bar = h_flex()
        .flex_wrap()
        .items_end()
        .gap(px(10.))
        .child(
            v_flex()
                .w(px(136.))
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.text_muted)
                        .child("起始日期"),
                )
                .child(Input::new(&s.filter_from)),
        )
        .child(
            v_flex()
                .w(px(136.))
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.text_muted)
                        .child("结束日期"),
                )
                .child(Input::new(&s.filter_to)),
        )
        .child(
            v_flex()
                .w(px(140.))
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.text_muted)
                        .child("主机（点击切换）"),
                )
                .child(
                    div()
                        .id("calls-filter-host")
                        .test_support()
                        .h(px(32.))
                        .px(px(10.))
                        .rounded_xl()
                        .border_1()
                        .border_color(palette.border)
                        .bg(palette.card_solid_bg)
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(11.))
                        .text_color(palette.text_primary)
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let _ = this_host_cycle.update(cx, |w, cx| {
                                let mut opts = vec![String::new()];
                                opts.extend(w.settings.call_hosts.clone());
                                let pos = opts
                                    .iter()
                                    .position(|h| *h == w.settings.filter_host)
                                    .unwrap_or(0);
                                w.settings.filter_host = opts[(pos + 1) % opts.len()].clone();
                                w.reload_mcp_calls(cx);
                            });
                        })
                        .child(div().truncate().child(host_label))
                        .child(lucide_icon(
                            CatalogIcon::ChevronsUpDown,
                            12.,
                            palette.text_faint,
                        )),
                ),
        )
        .child(
            v_flex()
                .w(px(140.))
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.text_muted)
                        .child("工具（点击切换）"),
                )
                .child(
                    div()
                        .id("calls-filter-tool")
                        .test_support()
                        .h(px(32.))
                        .px(px(10.))
                        .rounded_xl()
                        .border_1()
                        .border_color(palette.border)
                        .bg(palette.card_solid_bg)
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(11.))
                        .text_color(palette.text_primary)
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let _ = this_tool_cycle.update(cx, |w, cx| {
                                let mut opts = vec![String::new()];
                                opts.extend(w.settings.call_tools.clone());
                                let pos = opts
                                    .iter()
                                    .position(|t| *t == w.settings.filter_tool)
                                    .unwrap_or(0);
                                w.settings.filter_tool = opts[(pos + 1) % opts.len()].clone();
                                w.reload_mcp_calls(cx);
                            });
                        })
                        .child(div().truncate().child(tool_label))
                        .child(lucide_icon(
                            CatalogIcon::ChevronsUpDown,
                            12.,
                            palette.text_faint,
                        )),
                ),
        )
        .child(
            v_flex()
                .w(px(96.))
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.text_muted)
                        .child("结果"),
                )
                .child(
                    div()
                        .id("calls-filter-ok")
                        .test_support()
                        .h(px(32.))
                        .px(px(10.))
                        .rounded_xl()
                        .border_1()
                        .border_color(palette.border)
                        .bg(palette.card_solid_bg)
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(11.))
                        .text_color(palette.text_primary)
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let _ = this_ok_cycle.update(cx, |w, cx| {
                                w.settings.filter_ok = match w.settings.filter_ok.as_str() {
                                    "" => "true".into(),
                                    "true" => "false".into(),
                                    _ => String::new(),
                                };
                                w.reload_mcp_calls(cx);
                            });
                        })
                        .child(ok_label)
                        .child(lucide_icon(
                            CatalogIcon::ChevronsUpDown,
                            12.,
                            palette.text_faint,
                        )),
                ),
        );

    if filtered {
        filter_bar = filter_bar.child(
            div()
                .id("calls-filter-reset")
                .test_support()
                .h(px(32.))
                .px(px(12.))
                .rounded_xl()
                .border_1()
                .border_color(palette.border)
                .bg(palette.card_solid_bg)
                .flex()
                .items_center()
                .text_size(px(11.))
                .text_color(palette.text_secondary)
                .cursor_pointer()
                .hover(|s| s.bg(palette.subtle_bg))
                .on_click(move |_, window, cx| {
                    let _ = this_reset.update(cx, |w, cx| w.reset_mcp_call_filters(window, cx));
                })
                .child("重置筛选"),
        );
    }

    let list_el = if s.calls.is_empty() {
        div()
            .rounded_xl()
            .border_1()
            .border_dashed()
            .border_color(palette.border)
            .py(px(32.))
            .text_center()
            .text_size(px(12.))
            .text_color(palette.text_faint)
            .child(if filtered {
                "没有符合筛选条件的调用记录"
            } else {
                "暂无调用记录"
            })
            .into_any_element()
    } else {
        let mut table = v_flex().w_full();
        // 表头
        table = table.child(
            h_flex()
                .py(px(8.))
                .px(px(8.))
                .text_size(px(11.))
                .text_color(palette.text_faint)
                .child(div().w(px(20.)))
                .child(div().w(px(148.)).child("时间"))
                .child(div().w(px(110.)).child("工具"))
                .child(div().w(px(96.)).child("主机"))
                .child(div().w(px(56.)).child("结果"))
                .child(div().w(px(64.)).child("耗时"))
                .child(div().flex_1().child("参数 / 摘要")),
        );

        for call in &s.calls {
            let expanded = s.expanded_call_id.as_deref() == Some(call.id.as_str());
            let call_id = call.id.clone();
            let this_row = this.clone();
            let subtle = palette.subtle_bg;

            table = table.child(
                h_flex()
                    .id(SharedString::from(format!("mcp-call-row-{}", call.id)))
                    .test_support()
                    .items_start()
                    .py(px(8.))
                    .px(px(8.))
                    .border_t_1()
                    .border_color(palette.divider)
                    .text_size(px(11.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(subtle))
                    .on_click(move |_, _, cx| {
                        let id = call_id.clone();
                        let _ = this_row.update(cx, |w, cx| w.toggle_call_expand(&id, cx));
                    })
                    .child(
                        div()
                            .w(px(20.))
                            .text_color(palette.text_faint)
                            .child(if expanded { "▾" } else { "▸" }),
                    )
                    .child(
                        div()
                            .w(px(148.))
                            .text_color(palette.text_muted)
                            .truncate()
                            .child(call.ts.replace('T', " ").chars().take(19).collect::<String>()),
                    )
                    .child(
                        div()
                            .w(px(110.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.text_primary)
                            .truncate()
                            .child(call.tool.clone()),
                    )
                    .child(
                        div()
                            .w(px(96.))
                            .text_color(palette.text_secondary)
                            .truncate()
                            .child(if call.host.is_empty() {
                                "—".into()
                            } else {
                                call.host.clone()
                            }),
                    )
                    .child(
                        div().w(px(56.)).child(
                            div()
                                .rounded(px(4.))
                                .px(px(6.))
                                .py(px(1.))
                                .text_size(px(10.))
                                .font_weight(FontWeight::MEDIUM)
                                .bg(if call.ok {
                                    Hsla::from(rgb(0xecfdf5))
                                } else {
                                    Hsla::from(rgb(0xfef2f2))
                                })
                                .text_color(if call.ok {
                                    Hsla::from(rgb(0x059669))
                                } else {
                                    Hsla::from(rgb(0xdc2626))
                                })
                                .child(if call.ok { "成功" } else { "失败" }),
                        ),
                    )
                    .child(
                        div()
                            .w(px(64.))
                            .text_color(palette.text_muted)
                            .child(format!("{}ms", call.duration_ms)),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_color(palette.text_secondary)
                                    .truncate()
                                    .child(call.args.clone()),
                            )
                            .child(
                                div()
                                    .text_color(palette.text_faint)
                                    .truncate()
                                    .child(call.summary.clone()),
                            ),
                    ),
            );

            if expanded {
                let (full_args, full_res) = s
                    .expanded_detail
                    .clone()
                    .unwrap_or_else(|| (call.args.clone(), call.summary.clone()));
                let args_copy = full_args.clone();
                let res_copy = full_res.clone();
                let this_copy_a = this.clone();
                let this_copy_r = this.clone();

                let detail_box = if s.loading_detail && s.expanded_detail.is_none() {
                    div()
                        .p(px(12.))
                        .text_size(px(11.))
                        .text_color(palette.text_faint)
                        .child("加载完整内容中…")
                } else {
                    v_flex()
                        .p(px(14.))
                        .gap(px(12.))
                        .border_t_1()
                        .border_color(palette.divider)
                        .bg(palette.muted_surface_bg)
                        .child(
                            v_flex()
                                .gap(px(6.))
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(palette.text_muted)
                                                .child("参数（完整）"),
                                        )
                                        .child(
                                            div()
                                                .id("call-detail-copy-args")
                                                .rounded(px(4.))
                                                .border_1()
                                                .border_color(palette.border)
                                                .bg(palette.card_solid_bg)
                                                .px(px(6.))
                                                .py(px(1.))
                                                .text_size(px(10.))
                                                .text_color(palette.text_muted)
                                                .cursor_pointer()
                                                .on_click(move |_, _, cx| {
                                                    let a = args_copy.clone();
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(a),
                                                    );
                                                    let _ = this_copy_a.update(cx, |w, cx| {
                                                        w.show_toast(
                                                            "已复制完整参数",
                                                            ToastKind::Ok,
                                                            cx,
                                                        );
                                                    });
                                                    cx.stop_propagation();
                                                })
                                                .child("复制"),
                                        ),
                                )
                                .child(
                                    div()
                                        .rounded_xl()
                                        .border_1()
                                        .border_color(palette.border)
                                        .bg(palette.card_solid_bg)
                                        .p(px(10.))
                                        .text_size(px(11.))
                                        .text_color(palette.text_secondary)
                                        .child(if full_args.is_empty() {
                                            "（无参数）".into()
                                        } else {
                                            full_args
                                        }),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap(px(6.))
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(palette.text_muted)
                                                .child("返回结果（完整）"),
                                        )
                                        .child(
                                            div()
                                                .id("call-detail-copy-result")
                                                .rounded(px(4.))
                                                .border_1()
                                                .border_color(palette.border)
                                                .bg(palette.card_solid_bg)
                                                .px(px(6.))
                                                .py(px(1.))
                                                .text_size(px(10.))
                                                .text_color(palette.text_muted)
                                                .cursor_pointer()
                                                .on_click(move |_, _, cx| {
                                                    let r = res_copy.clone();
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(r),
                                                    );
                                                    let _ = this_copy_r.update(cx, |w, cx| {
                                                        w.show_toast(
                                                            "已复制完整返回结果",
                                                            ToastKind::Ok,
                                                            cx,
                                                        );
                                                    });
                                                    cx.stop_propagation();
                                                })
                                                .child("复制"),
                                        ),
                                )
                                .child(
                                    div()
                                        .rounded_xl()
                                        .border_1()
                                        .border_color(palette.border)
                                        .bg(palette.card_solid_bg)
                                        .p(px(10.))
                                        .text_size(px(11.))
                                        .text_color(palette.text_secondary)
                                        .child(if full_res.is_empty() {
                                            "（无返回结果）".into()
                                        } else {
                                            full_res
                                        }),
                                ),
                        )
                };
                table = table.child(detail_box);
            }
        }
        table.into_any_element()
    };

    section_shell(palette)
        .id("settings-panel-calls")
        .test_support()
        .child(
            v_flex()
                .gap(px(14.))
                .child(
                    h_flex()
                        .items_start()
                        .justify_between()
                        .gap(px(12.))
                        .pb(px(12.))
                        .border_b_1()
                        .border_color(palette.divider)
                        .child(
                            v_flex()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette.text_primary)
                                        .child("MCP 调用记录"),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(palette.text_faint)
                                        .child("外部 agent 经 MCP 通道的每一次调用都在此留痕。点任意一行展开完整参数与返回结果"),
                                ),
                        )
                        .child(
                            div()
                                .id("calls-btn-refresh")
                                .test_support()
                                .flex_shrink_0()
                                .rounded_lg()
                                .bg(palette.primary_bg)
                                .text_color(palette.primary_fg)
                                .px(px(12.))
                                .py(px(5.))
                                .text_size(px(11.))
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .on_click(move |_, _, cx| {
                                    let _ = this_refresh.update(cx, |w, cx| w.reload_mcp_calls(cx));
                                })
                                .child("刷新"),
                        ),
                )
                .child(filter_bar)
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(palette.text_faint)
                        .child(format!(
                            "{} {} 条",
                            if filtered {
                                "符合筛选的调用"
                            } else {
                                "最近调用"
                            },
                            s.calls.len()
                        )),
                )
                .child(list_el),
        )
        .into_any_element()
}

fn render_about_tab(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
) -> AnyElement {
    let s = &workspace.settings;
    let checking = s.checking_update;
    let exe_path = s.exe_path.clone();
    let exe_copy = exe_path.clone();
    let this_check = this.clone();
    let this_copy_exe = this.clone();
    let this_copy_repo = this.clone();

    let mut stack = v_flex()
        .gap(px(16.))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .pb(px(14.))
                .border_b_1()
                .border_color(palette.divider)
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(12.))
                        .child(brand_logo(BrandLogoSize::Md))
                        .child(
                            v_flex()
                                .gap(px(2.))
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .text_size(px(16.))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(palette.text_primary)
                                                .child("VPS Switch"),
                                        )
                                        .child(
                                            div()
                                                .rounded_full()
                                                .bg(Hsla::from(rgb(0xd1fae5)))
                                                .px(px(8.))
                                                .py(px(2.))
                                                .text_size(px(11.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(Hsla::from(rgb(0x065f46)))
                                                .child(s.version.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(palette.text_faint)
                                        .child("轻量级 VPS 管理工具与 MCP Server · 极简原生 · 零模型调用"),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .id("settings-check-update")
                        .test_support()
                        .items_center()
                        .gap(px(6.))
                        .rounded_xl()
                        .bg(palette.primary_bg)
                        .text_color(palette.primary_fg)
                        .px(px(14.))
                        .py(px(6.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let _ = this_check.update(cx, |w, cx| {
                                if !w.settings.checking_update {
                                    w.settings.checking_update = true;
                                    w.api.send(ApiCommand::CheckUpdate);
                                    cx.notify();
                                }
                            });
                        })
                        .child(lucide_icon(
                            CatalogIcon::RefreshCw,
                            13.5,
                            palette.primary_fg,
                        ))
                        .child(if checking { "正在检查..." } else { "检查更新" }),
                ),
        );

    if let Some(outcome) = &s.update_outcome {
        let mut box_el = v_flex()
            .rounded_xl()
            .border_1()
            .p(px(14.))
            .gap(px(8.))
            .border_color(if outcome.has_update {
                palette.status_online
            } else {
                palette.border
            })
            .bg(if outcome.has_update {
                palette.status_online.opacity(0.08)
            } else {
                palette.muted_surface_bg
            });

        let mut head = h_flex()
            .items_center()
            .justify_between()
            .child(
                h_flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(8.))
                            .rounded_full()
                            .bg(if outcome.has_update {
                                palette.status_online
                            } else {
                                palette.text_faint
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.text_primary)
                            .child(if outcome.has_update {
                                format!("发现新版本 {}", outcome.latest_version)
                            } else {
                                format!("当前已是最新版本 ({})", outcome.current_version)
                            }),
                    ),
            );

        if outcome.has_update
            && let Some(url) = &outcome.release_url
        {
            let release_url = url.clone();
            head = head.child(
                div()
                    .id("settings-open-release-url")
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.status_online)
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        cx.open_url(&release_url);
                    })
                    .child("前往 GitHub 查看发布说明 →"),
            );
        }
        box_el = box_el.child(head);

        if outcome.has_update
            && let Some(notes) = &outcome.release_notes
            && !notes.is_empty()
        {
            box_el = box_el.child(
                div()
                    .rounded_lg()
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.card_solid_bg)
                    .p(px(12.))
                    .text_size(px(12.))
                    .text_color(palette.text_secondary)
                    .child(notes.clone()),
            );
        }
        stack = stack.child(box_el);
    }

    stack = stack.child(
        v_flex()
            .gap(px(10.))
            .child(
                inner_row_shell(palette).child(
                    v_flex()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(palette.text_faint)
                                .child("本地服务监听"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette.text_primary)
                                .child(format!("127.0.0.1:{}", s.port)),
                        ),
                ),
            )
            .child(
                inner_row_shell(palette).child(
                    v_flex()
                        .gap(px(4.))
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(palette.text_faint)
                                        .child("可执行文件绝对路径"),
                                )
                                .child(
                                    div()
                                        .id("settings-copy-exe-path")
                                        .test_support()
                                        .text_size(px(11.))
                                        .text_color(palette.text_muted)
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(palette.text_primary))
                                        .on_click(move |_, _, cx| {
                                            let p = exe_copy.clone();
                                            cx.write_to_clipboard(ClipboardItem::new_string(p));
                                            let _ = this_copy_exe.update(cx, |w, cx| {
                                                w.show_toast(
                                                    "已复制程序绝对路径",
                                                    ToastKind::Ok,
                                                    cx,
                                                );
                                            });
                                        })
                                        .child("复制路径"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(palette.text_secondary)
                                .truncate()
                                .child(exe_path),
                        ),
                ),
            )
            .child(
                inner_row_shell(palette).child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(
                            v_flex()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette.text_primary)
                                        .child("GitHub 官方私有仓库"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(palette.text_faint)
                                        .child("https://github.com/weidu12123/vps-switch"),
                                ),
                        )
                        .child(
                            div()
                                .id("settings-copy-repo-url")
                                .test_support()
                                .rounded_lg()
                                .border_1()
                                .border_color(palette.border)
                                .bg(palette.card_solid_bg)
                                .px(px(10.))
                                .py(px(4.))
                                .text_size(px(11.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette.text_secondary)
                                .cursor_pointer()
                                .hover(|s| s.bg(palette.subtle_bg))
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        "https://github.com/weidu12123/vps-switch".into(),
                                    ));
                                    let _ = this_copy_repo.update(cx, |w, cx| {
                                        w.show_toast("已复制 GitHub 仓库链接", ToastKind::Ok, cx);
                                    });
                                })
                                .child("复制链接"),
                        ),
                ),
            ),
    );

    section_shell(palette)
        .id("settings-panel-about")
        .test_support()
        .child(stack)
        .into_any_element()
}
