//! 顶栏（规格 §3）。尺寸与颜色取自旧前端的 computed style（Playwright 实测）：
//! 高 50px；`bg white/75` + 底边框 `zinc-200/75`；按钮高 29px；
//! 「仓库」按钮圆角 12px + 白底 + 细阴影；搜索/筛选/检测为胶囊；圆钮 32px、白 85% 底。

use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::*;

pub use gpui_kit::assets::IconName;

use crate::workspace::{OpenTab, Page, Workspace};

pub fn render(workspace: &Workspace, this: &Entity<Workspace>, cx: &App) -> impl IntoElement {
    let has_stage = workspace.has_stage();
    let theme = cx.theme();

    let mut left = h_flex().min_w_0().flex_1().items_center().gap(px(6.));

    // 品牌（旧版：logo 24px + 8px 间距 + 14px/700 文字）
    left = left.child(
        h_flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(8.))
            .mr(px(4.))
            .child(brand_logo())
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.foreground)
                    .child("VPS Switch"),
            ),
    );

    // 「仓库」按钮：圆角 12px、白底、边框 zinc-200/90、阴影、高 29
    let home_active = workspace.page == Page::Workspace && !has_stage;
    let this_home = this.clone();
    left = left.child(
        raised_button(home_active)
            .id("nav-home")
            .on_click(move |_, _, cx| {
                let _ = this_home.update(cx, |workspace, cx| workspace.close_stage(cx));
            })
            .child(icon(IconName::House, 14., cx))
            .child("仓库")
            .child(count_badge(format!(
                "{}/{}",
                workspace.online_count(),
                workspace.total_active_count()
            ))),
    );

    // 「侧栏」切换（仅有舞台时）
    if has_stage {
        let this_side = this.clone();
        left = left.child(
            raised_button(workspace.show_sidebar_in_stage)
                .id("nav-sidebar")
                .on_click(move |_, _, cx| {
                    let _ = this_side.update(cx, |workspace, cx| workspace.toggle_sidebar(cx));
                })
                .child(icon(IconName::PanelLeft, 14., cx))
                .child(if workspace.show_sidebar_in_stage {
                    "收起侧栏"
                } else {
                    "展开侧栏"
                }),
        );
    }

    // 已打开的会话标签
    for tab in &workspace.open_tabs {
        left = left.child(session_tab(workspace, tab, this, cx));
    }

    // ---- 右组 ----
    let mut right = h_flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(8.))
        .child(div().w(px(176.)).child(Input::new(&workspace.search)))
        .child({
            let active = workspace.has_active_filter();
            let this_filter = this.clone();
            let label = filter_summary(workspace);
            let mut wrapper = div()
                .relative()
                .child(
                    pill_button(active)
                        .id("nav-filter")
                        .on_click(move |_, _, cx| {
                            let _ = this_filter.update(cx, |workspace, cx| {
                                workspace.filter_open = !workspace.filter_open;
                                cx.notify();
                            });
                        })
                        .child(icon(IconName::Funnel, 14., cx))
                        .child(label),
                );
            if workspace.filter_open {
                wrapper = wrapper.child(filter_panel(workspace, this, cx));
            }
            wrapper
        });

    if workspace.page == Page::Workspace {
        let checking = workspace.checking_all;
        let this_check = this.clone();
        right = right.child(
            pill_button(false)
                .id("nav-check")
                .on_click(move |_, _, cx| {
                    let _ = this_check.update(cx, |workspace, cx| workspace.check_all(cx));
                })
                .child(icon(IconName::RefreshCw, 14., cx))
                .child(if checking { "检测中" } else { "检测在线" }),
        );
    }

    // 主题切换（32px 圆钮，白 85% 底）
    let this_theme = this.clone();
    let is_dark = workspace.is_dark;
    right = right.child(
        round_button(false)
            .id("nav-theme")
            .on_click(move |_, _, cx| {
                let _ = this_theme.update(cx, |workspace, cx| workspace.toggle_theme(cx));
            })
            .child(icon(
                if is_dark { IconName::Sun } else { IconName::Moon },
                16.,
                cx,
            )),
    );

    // 设置
    let this_settings = this.clone();
    let settings_active = workspace.page == Page::Settings;
    right = right.child(
        round_button(settings_active)
            .id("nav-settings")
            .on_click(move |_, _, cx| {
                let _ = this_settings.update(cx, |workspace, cx| workspace.open_settings(cx));
            })
            .child(icon(IconName::Settings, 16., cx)),
    );

    // 新增 VPS（橙色 #f97316，白字，32px）
    right = right.child(
        div()
            .id("nav-add")
            .flex()
            .items_center()
            .justify_center()
            .size(px(32.))
            .rounded_full()
            .bg(rgb(0xf97316))
            .text_color(rgb(0xffffff))
            .shadow_sm()
            .hover(|style| style.bg(rgb(0xea580c)))
            .child(icon(IconName::Plus, 16., cx)),
    );

    h_flex()
        .h(px(50.))
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .px(px(16.))
        .py(px(8.))
        .border_b_1()
        .border_color(Hsla::from(rgb(0xe4e4e7)).opacity(0.75))
        .bg(Hsla::from(rgb(0xffffff)).opacity(0.75))
        .child(left)
        .child(right)
}

fn session_tab(
    workspace: &Workspace,
    tab: &OpenTab,
    this: &Entity<Workspace>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let host = workspace.host_by_id(&tab.host_id);
    let name = host
        .map(|h| h.name.clone())
        .unwrap_or_else(|| tab.host_id.clone());
    let status = host
        .map(|h| h.status.clone())
        .unwrap_or_else(|| "unknown".into());
    let active = workspace.page == Page::Workspace
        && workspace.has_stage()
        && workspace.stage_host_id.as_deref() == Some(tab.host_id.as_str());
    let pane_count = tab.panes.len();
    let host_id = tab.host_id.clone();

    let this_activate = this.clone();
    let this_close = this.clone();
    let close_id = host_id.clone();

    let base = h_flex()
        .id(SharedString::from(format!("session-{}", host_id)))
        .flex_shrink_0()
        .items_center()
        .gap(px(8.))
        .h(px(29.))
        .px(px(12.))
        .rounded_lg()
        .border_1()
        .text_size(px(12.))
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            let id = host_id.clone();
            let _ = this_activate.update(cx, |workspace, cx| workspace.activate_tab(&id, cx));
        })
        .child(
            div()
                .size(px(8.))
                .rounded_full()
                .bg(status_dot_color(&status, cx)),
        )
        .child(div().max_w(px(128.)).truncate().child(name));
    let base = if pane_count > 1 {
        base.child(count_badge(format!("×{pane_count}")))
    } else {
        base
    };
    let styled = if active {
        base.border_color(Hsla::from(rgb(0xe4e4e7)).opacity(0.9))
            .bg(rgb(0xffffff))
            .text_color(theme.foreground)
            .shadow_sm()
    } else {
        base.border_color(Hsla::transparent_black())
            .bg(Hsla::from(rgb(0xf1f2f4)).opacity(0.6))
            .text_color(rgb(0x52525b))
    };
    styled.child(
        div()
            .id(SharedString::from(format!("session-close-{}", close_id)))
            .flex()
            .items_center()
            .justify_center()
            .size(px(16.))
            .rounded_sm()
            .text_color(rgb(0xa1a1aa))
            .hover(|style| style.bg(rgb(0xe4e4e7)).text_color(theme.foreground))
            .on_click(move |_, _, cx| {
                let id = close_id.clone();
                let _ = this_close.update(cx, |workspace, cx| workspace.close_tab(&id, cx));
                cx.stop_propagation();
            })
            .child(icon(IconName::X, 12., cx)),
    )
}

pub fn status_dot_color(status: &str, cx: &App) -> Hsla {
    match status {
        "online" => Hsla::from(rgb(0x10b981)),
        "offline" => Hsla::from(rgb(0xef4444)),
        _ => cx.theme().muted_foreground,
    }
}

/// 「仓库 / 侧栏」按钮：高 29、圆角 12px、激活白底+细阴影，否则透明+灰字。
fn raised_button(active: bool) -> Div {
    let base = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(29.))
        .px(px(12.))
        .rounded_lg()
        .border_1()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer();
    if active {
        base.border_color(Hsla::from(rgb(0xe4e4e7)).opacity(0.9))
            .bg(rgb(0xffffff))
            .text_color(rgb(0x18181b))
            .shadow_sm()
    } else {
        base.border_color(Hsla::transparent_black())
            .text_color(rgb(0x71717a))
            .hover(|style| style.bg(Hsla::from(rgb(0xffffff)).opacity(0.6)))
    }
}

/// 胶囊按钮（筛选 / 检测）：高 29、圆角 full、白 85% 底、边框 zinc-200/80。
fn pill_button(active: bool) -> Div {
    let base = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(29.))
        .px(px(12.))
        .rounded_full()
        .border_1()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer();
    if active {
        base.border_color(rgb(0x18181b))
            .bg(rgb(0x18181b))
            .text_color(rgb(0xffffff))
            .shadow_sm()
    } else {
        base.border_color(Hsla::from(rgb(0xe4e4e7)).opacity(0.8))
            .bg(Hsla::from(rgb(0xffffff)).opacity(0.85))
            .text_color(rgb(0x3f3f46))
            .hover(|style| style.bg(rgb(0xffffff)))
    }
}

/// 圆钮：32px、白 85% 底、边框 zinc-200/80、图标色 zinc-500。
fn round_button(active: bool) -> Div {
    let base = div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(32.))
        .rounded_full()
        .border_1()
        .cursor_pointer();
    if active {
        base.border_color(rgb(0x18181b))
            .bg(rgb(0x18181b))
            .text_color(rgb(0xffffff))
            .shadow_sm()
    } else {
        base.border_color(Hsla::from(rgb(0xe4e4e7)).opacity(0.8))
            .bg(Hsla::from(rgb(0xffffff)).opacity(0.85))
            .text_color(rgb(0x52525b))
            .hover(|style| style.bg(rgb(0xffffff)).text_color(rgb(0x18181b)))
    }
}

fn count_badge(text: String) -> impl IntoElement {
    div()
        .rounded_full()
        .bg(rgb(0xf1f2f4))
        .px(px(6.))
        .py(px(1.))
        .text_size(px(10.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(0x71717a))
        .child(text)
}

fn filter_summary(workspace: &Workspace) -> String {
    let mut parts: Vec<String> = Vec::new();
    if workspace.favorite_only {
        parts.push("★ 收藏".into());
    }
    match workspace.status_filter {
        crate::workspace::StatusFilter::Online => parts.push("在线".into()),
        crate::workspace::StatusFilter::Offline => parts.push("离线".into()),
        crate::workspace::StatusFilter::All => {}
    }
    match workspace.role_filter {
        crate::workspace::RoleFilter::Primary => parts.push("主力".into()),
        crate::workspace::RoleFilter::Proxy => parts.push("代理".into()),
        crate::workspace::RoleFilter::Test => parts.push("测试".into()),
        crate::workspace::RoleFilter::Archived => parts.push("归档".into()),
        crate::workspace::RoleFilter::All => {}
    }
    if !workspace.tag_filter.is_empty() {
        parts.push(format!("#{}", workspace.tag_filter));
    }
    if parts.is_empty() {
        "筛选".into()
    } else {
        format!("筛选 · {}", parts.join(" · "))
    }
}

/// 图标：颜色与字号继承外层文本样式。
pub fn icon(name: IconName, size: f32, _cx: &App) -> impl IntoElement {
    div().text_size(px(size)).child(Icon::new(name))
}

/// 筛选下拉面板（真值：256px 宽、白 95%、圆角 16、14px 内边距、Tailwind shadow-xl）。
fn filter_panel(workspace: &Workspace, this: &Entity<Workspace>, cx: &App) -> AnyElement {
    use crate::workspace::{RoleFilter, StatusFilter};

    let mut panel = v_flex()
        .absolute()
        .top(px(37.))
        .right(px(0.))
        .w(px(256.))
        .rounded_2xl()
        .border_1()
        .border_color(Hsla::from(rgb(0xe4e4e7)).opacity(0.9))
        .bg(Hsla::from(rgb(0xffffff)).opacity(0.95))
        .p(px(14.))
        .shadow(vec![
            BoxShadow {
                color: Hsla::from(rgb(0x000000)).opacity(0.1),
                offset: point(px(0.), px(20.)),
                blur_radius: px(25.),
                spread_radius: px(-5.),
                inset: false,
            },
            BoxShadow {
                color: Hsla::from(rgb(0x000000)).opacity(0.1),
                offset: point(px(0.), px(8.)),
                blur_radius: px(10.),
                spread_radius: px(-6.),
                inset: false,
            },
        ]);

    // ---- 在线状态 ----
    let mut head = h_flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0xa1a1aa))
                .child("在线状态"),
        );
    if workspace.has_active_filter() {
        let this_reset = this.clone();
        head = head.child(
            div()
                .id("filter-reset")
                .text_size(px(11.))
                .text_color(rgb(0x71717a))
                .cursor_pointer()
                .hover(|style| style.text_color(rgb(0x18181b)))
                .on_click(move |_, _, cx| {
                    let _ = this_reset.update(cx, |workspace, cx| {
                        workspace.reset_filters();
                        cx.notify();
                    });
                })
                .child("重置全部"),
        );
    }
    panel = panel.child(head);

    let mut grid = h_flex().mt(px(6.)).gap(px(4.));
    let statuses: [(&'static str, StatusFilter, Option<Hsla>); 3] = [
        ("全部", StatusFilter::All, None),
        ("在线", StatusFilter::Online, Some(Hsla::from(rgb(0x10b981)))),
        ("离线", StatusFilter::Offline, Some(Hsla::from(rgb(0xef4444)))),
    ];
    for (label, filter, dot) in statuses {
        let active = workspace.status_filter == filter;
        let count = match filter {
            StatusFilter::Online => Some(workspace.online_count()),
            StatusFilter::Offline => Some(workspace.offline_count()),
            StatusFilter::All => None,
        };
        let text = match count {
            Some(count) => format!("{label} ({count})"),
            None => label.to_owned(),
        };
        let this_status = this.clone();
        let mut chip = filter_chip(active)
            .id(SharedString::from(format!("filter-status-{}", label)))
            .on_click(move |_, _, cx| {
                let _ = this_status.update(cx, |workspace, cx| {
                    workspace.status_filter = filter;
                    cx.notify();
                });
            });
        if let Some(dot_color) = dot {
            chip = chip.child(div().size(px(8.)).rounded_full().bg(dot_color));
        }
        grid = grid.child(chip.child(text));
    }
    panel = panel.child(grid);

    // ---- 收藏与分类 ----
    panel = panel.child(
        v_flex()
            .mt(px(12.))
            .pt(px(12.))
            .border_t_1()
            .border_color(rgb(0xf1f2f4))
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0xa1a1aa))
                    .child("收藏与分类"),
            ),
    );

    let mut chips = h_flex().flex_wrap().gap(px(4.));
    // 收藏
    {
        let active = workspace.favorite_only;
        let this_fav = this.clone();
        let base = div()
            .id("filter-favorite")
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(24.))
            .px(px(10.))
            .rounded_lg()
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _ = this_fav.update(cx, |workspace, cx| {
                    workspace.favorite_only = !workspace.favorite_only;
                    cx.notify();
                });
            })
            .child(format!("★ 收藏 ({})", workspace.favorite_count()));
        chips = chips.child(if active {
            base.bg(rgb(0xf59e0b)).text_color(rgb(0xffffff))
        } else {
            base.bg(rgb(0xf4f4f5)).text_color(rgb(0x52525b))
        });
    }
    // 分类
    let roles: [(&'static str, RoleFilter); 5] = [
        ("全部分类", RoleFilter::All),
        ("主力", RoleFilter::Primary),
        ("代理", RoleFilter::Proxy),
        ("测试", RoleFilter::Test),
        ("归档", RoleFilter::Archived),
    ];
    for (label, role) in roles {
        let active = workspace.role_filter == role;
        let this_role = this.clone();
        let base = div()
            .id(SharedString::from(format!("filter-role-{}", label)))
            .flex()
            .items_center()
            .h(px(24.))
            .px(px(10.))
            .rounded_lg()
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _ = this_role.update(cx, |workspace, cx| {
                    workspace.role_filter = role;
                    cx.notify();
                });
            })
            .child(label);
        chips = chips.child(if active {
            base.bg(rgb(0x18181b)).text_color(rgb(0xffffff))
        } else {
            base.bg(rgb(0xf4f4f5)).text_color(rgb(0x52525b))
        });
    }
    panel = panel.child(chips);

    // ---- 标签 ----
    let tags = workspace.all_tags();
    if !tags.is_empty() {
        let mut tag_chips = h_flex().flex_wrap().gap(px(4.));
        for tag in tags {
            let active = workspace.tag_filter == tag;
            let this_tag = this.clone();
            let value = tag.clone();
            let base = div()
                .id(SharedString::from(format!("filter-tag-{}", tag)))
                .h(px(20.))
                .px(px(8.))
                .rounded_lg()
                .text_size(px(11.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let value = value.clone();
                    let _ = this_tag.update(cx, |workspace, cx| {
                        workspace.tag_filter = if workspace.tag_filter == value {
                            String::new()
                        } else {
                            value
                        };
                        cx.notify();
                    });
                })
                .child(format!("#{tag}"));
            tag_chips = tag_chips.child(if active {
                base.bg(rgb(0x18181b)).text_color(rgb(0xffffff))
            } else {
                base.bg(rgb(0xf4f4f5)).text_color(rgb(0x52525b))
            });
        }
        panel = panel.child(
            v_flex()
                .mt(px(12.))
                .pt(px(12.))
                .border_t_1()
                .border_color(rgb(0xf1f2f4))
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(0xa1a1aa))
                        .child("标签"),
                )
                .child(tag_chips),
        );
    }

    panel.into_any_element()
}

/// 筛选面板里的状态按钮（真值：圆角 12、12px/500、padding 6/8，激活黑底白字）。
fn filter_chip(active: bool) -> Div {
    let base = div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .py(px(6.))
        .px(px(8.))
        .rounded_lg()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer();
    if active {
        base.bg(rgb(0x18181b)).text_color(rgb(0xffffff))
    } else {
        base.bg(rgb(0xf4f4f5))
            .text_color(rgb(0x52525b))
            .hover(|style| style.bg(rgb(0xe4e4e7)))
    }
}

/// 品牌图标：纯黑圆角方块 + 白色滑块（旧 `BrandLogo` 的简化等价物）。
fn brand_logo() -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(24.))
        .rounded_md()
        .bg(rgb(0x111113))
        .child(
            div()
                .text_size(px(10.))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xffffff))
                .child("◧"),
        )
}
