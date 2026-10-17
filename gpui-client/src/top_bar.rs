//! 顶栏：品牌 + 仓库/侧栏按钮 + 会话标签 + 搜索 + 筛选 + 检测 + 主题/设置/新增。
//! 规格见 `docs/ui-spec.md` §3。

use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::*;

pub use gpui_kit::assets::IconName;

use crate::workspace::{OpenTab, Page, Workspace};

pub fn render(workspace: &Workspace, this: &Entity<Workspace>, cx: &App) -> impl IntoElement {
    let has_stage = workspace.has_stage();
    let theme = cx.theme();

    // ---- 左组 ----
    let mut left = h_flex()
        .min_w_0()
        .flex_1()
        .items_center()
        .gap(px(6.));

    left = left.child(
        h_flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(8.))
            .mr(px(4.))
            .child(brand_logo(cx))
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.foreground)
                    .child("VPS Switch"),
            ),
    );

    // 「仓库」按钮
    let home_active = workspace.page == Page::Workspace && !has_stage;
    let this_home = this.clone();
    left = left.child(
        pill(home_active, cx)
            .id("nav-home")
            .on_click(move |_, _, cx| {
                let _ = this_home.update(cx, |workspace, cx| workspace.close_stage(cx));
            })
            .child(icon(IconName::House, 14., cx))
            .child("仓库")
            .child(count_badge(
                format!(
                    "{}/{}",
                    workspace.online_count(),
                    workspace.total_active_count()
                ),
                cx,
            )),
    );

    // 「侧栏」切换（仅有舞台时）
    if has_stage {
        let this_side = this.clone();
        left = left.child(
            pill(workspace.show_sidebar_in_stage, cx)
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
        // 搜索
        .child(
            div()
                .w(px(190.))
                .child(Input::new(&workspace.search)),
        )
        // 筛选按钮
        .child({
            let active = workspace.has_active_filter();
            let this_filter = this.clone();
            let label = filter_summary(workspace);
            pill(active, cx)
                .id("nav-filter")
                .on_click(move |_, _, cx| {
                    let _ = this_filter.update(cx, |workspace, cx| {
                        workspace.filter_open = !workspace.filter_open;
                        cx.notify();
                    });
                })
                .child(icon(IconName::Funnel, 14., cx))
                .child(label)
        });

    if workspace.page == Page::Workspace {
        let checking = workspace.checking_all;
        let this_check = this.clone();
        right = right.child(
            pill(false, cx)
                .id("nav-check")
                .on_click(move |_, _, cx| {
                    let _ = this_check.update(cx, |workspace, cx| workspace.check_all(cx));
                })
                .child(icon(IconName::RefreshCw, 14., cx))
                .child(if checking { "检测中" } else { "检测在线" }),
        );
    }

    // 主题切换
    let this_theme = this.clone();
    let is_dark = workspace.is_dark;
    right = right.child(
        round_button(cx)
            .id("nav-theme")
            .on_click(move |_, _, cx| {
                let _ = this_theme.update(cx, |workspace, cx| workspace.toggle_theme(cx));
            })
            .child(icon(
                if is_dark {
                    IconName::Sun
                } else {
                    IconName::Moon
                },
                16.,
                cx,
            )),
    );

    // 设置
    let this_settings = this.clone();
    let settings_active = workspace.page == Page::Settings;
    right = right.child(
        if settings_active {
            round_button_active(cx)
        } else {
            round_button(cx)
        }
        .id("nav-settings")
        .on_click(move |_, _, cx| {
            let _ = this_settings.update(cx, |workspace, cx| workspace.open_settings(cx));
        })
        .child(icon(IconName::Settings, 16., cx)),
    );

    // 新增 VPS（橙色）
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
            .hover(|style| style.bg(rgb(0xea580c)))
            .child(icon(IconName::Plus, 16., cx)),
    );

    h_flex()
        .h(px(52.))
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .px(px(16.))
        .py(px(8.))
        .border_b_1()
        .border_color(rgb(0xe4e4e7))
        .bg(rgb(0xfafafa))
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
    let name = host.map(|h| h.name.clone()).unwrap_or_else(|| tab.host_id.clone());
    let status = host.map(|h| h.status.clone()).unwrap_or_else(|| "unknown".into());
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
        .h(px(30.))
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
        .child(
            div()
                .max_w(px(128.))
                .truncate()
                .child(name),
        );
    let base = if pane_count > 1 {
        base.child(count_badge(format!("×{pane_count}"), cx))
    } else {
        base
    };
    let styled = if active {
        base.border_color(theme.border)
            .bg(rgb(0xffffff))
            .text_color(theme.foreground)
            .shadow_sm()
    } else {
        base.border_color(Hsla::transparent_black())
            .bg(rgb(0xf1f2f4))
            .text_color(theme.muted_foreground)
    };
    styled.child(
        div()
            .id(SharedString::from(format!("session-close-{}", close_id)))
            .flex()
            .items_center()
            .justify_center()
            .size(px(16.))
            .rounded_sm()
            .text_color(theme.muted_foreground)
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

/// 胶囊按钮的公共样式。
pub fn pill(active: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let base = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(30.))
        .px(px(12.))
        .rounded_lg()
        .border_1()
        .text_size(px(12.))
        .cursor_pointer();
    if active {
        base.border_color(theme.border)
            .bg(rgb(0xffffff))
            .text_color(theme.foreground)
            .shadow_sm()
    } else {
        base.border_color(Hsla::transparent_black())
            .text_color(theme.muted_foreground)
            .hover(|style| style.bg(rgb(0xffffff)))
    }
}

fn round_button(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(32.))
        .rounded_full()
        .border_1()
        .border_color(theme.border)
        .bg(rgb(0xffffff))
        .text_color(theme.muted_foreground)
        .cursor_pointer()
        .hover(|style| style.text_color(theme.foreground))
}

fn round_button_active(_cx: &App) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(32.))
        .rounded_full()
        .border_1()
        .border_color(rgb(0x18181b))
        .bg(rgb(0x18181b))
        .text_color(rgb(0xffffff))
        .cursor_pointer()
}

fn count_badge(text: String, _cx: &App) -> impl IntoElement {
    div()
        .rounded_full()
        .bg(rgb(0xf1f2f4))
        .px(px(6.))
        .py(px(1.))
        .text_size(px(10.))
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

/// 品牌图标：纯黑圆角方块 + 白色滑块符号（旧 `BrandLogo` 的简化等价物）。
fn brand_logo(_cx: &App) -> impl IntoElement {
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
