//! 终端舞台：顶栏（主机信息 + 分屏/主题/右栏/独立窗口/收起）+ 分屏画布。
//! 规格见 `docs/ui-spec.md` §5。

use gpui_kit::component::*;
use gpui_kit::*;

use crate::top_bar::{IconName, icon, status_dot_color};
use crate::workspace::{RightWing, SplitDirection, Workspace};

pub fn render(workspace: &Workspace, this: &Entity<Workspace>, cx: &App) -> AnyElement {
    let Some(tab_index) = workspace.active_tab_index() else {
        return div().into_any_element();
    };
    let tab = &workspace.open_tabs[tab_index];
    let Some(host) = workspace.host_by_id(&tab.host_id) else {
        return div().into_any_element();
    };
    let theme = cx.theme();

    // ---- 顶栏左侧 ----
    let address = if host.kind == "local" {
        "PowerShell (ConPTY)".to_owned()
    } else {
        format!("{}@{}:{}", host.username, host.addr, host.port)
    };
    let left = h_flex()
        .min_w_0()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .size(px(10.))
                .rounded_full()
                .bg(status_dot_color(&host.status, cx)),
        )
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground)
                .truncate()
                .child(host.name.clone()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted_foreground)
                .child(address),
        );

    // ---- 顶栏右侧工具条 ----
    let host_id = host.id.clone();
    let terminal_dark = workspace.terminal_dark;

    let this_split_row = this.clone();
    let this_split_col = this.clone();
    let this_dark = this.clone();
    let this_files = this.clone();
    let this_detail = this.clone();
    let this_popout = this.clone();
    let this_collapse = this.clone();
    let popout_id = host_id.clone();

    let right = h_flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.))
        // 分屏组
        .child(
            h_flex()
                .items_center()
                .gap(px(2.))
                .p(px(2.))
                .rounded_lg()
                .border_1()
                .border_color(rgb(0xe4e4e7))
                .bg(rgb(0xfafafa))
                .child(
                    tool_button(cx)
                        .id("split-row")
                        .on_click(move |_, _, cx| {
                            let _ = this_split_row.update(cx, |workspace, cx| {
                                workspace.split(SplitDirection::Row, cx)
                            });
                        })
                        .child(icon(IconName::Columns2, 14., cx))
                        .child("左右分屏"),
                )
                .child(
                    tool_button(cx)
                        .id("split-col")
                        .on_click(move |_, _, cx| {
                            let _ = this_split_col.update(cx, |workspace, cx| {
                                workspace.split(SplitDirection::Col, cx)
                            });
                        })
                        .child(icon(IconName::Rows2, 14., cx))
                        .child("上下分屏"),
                ),
        )
        .child(
            tool_button(cx)
                .id("toggle-terminal-dark")
                .on_click(move |_, _, cx| {
                    let _ = this_dark.update(cx, |workspace, cx| workspace.toggle_terminal_dark(cx));
                })
                .child(if terminal_dark { "白底" } else { "黑底" }),
        )
        .child(
            wing_button(
                cx,
                workspace.right_wing == Some(RightWing::Files),
                "toggle-right-files",
                "文件管理",
                IconName::Folder,
                move |this, cx| {
                    let _ = this.update(cx, |workspace, cx| {
                        workspace.toggle_right_wing(RightWing::Files, cx)
                    });
                },
                this_files,
            ),
        )
        .child(
            wing_button(
                cx,
                workspace.right_wing == Some(RightWing::Detail),
                "toggle-right-detail",
                "详情图表",
                IconName::ChartColumnBig,
                move |this, cx| {
                    let _ = this.update(cx, |workspace, cx| {
                        workspace.toggle_right_wing(RightWing::Detail, cx)
                    });
                },
                this_detail,
            ),
        )
        .child(
            tool_button(cx)
                .id("popout-terminal")
                .on_click(move |_, _, cx| {
                    let id = popout_id.clone();
                    let _ = this_popout.update(cx, |workspace, cx| workspace.popout_terminal(&id, cx));
                })
                .child(icon(IconName::ExternalLink, 14., cx))
                .child("独立窗口"),
        )
        .child(
            div()
                .id("collapse-stage")
                .flex()
                .items_center()
                .justify_center()
                .size(px(26.))
                .rounded_lg()
                .text_color(theme.muted_foreground)
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0xf1f2f4)).text_color(theme.foreground))
                .on_click(move |_, _, cx| {
                    let _ = this_collapse.update(cx, |workspace, cx| workspace.close_stage(cx));
                })
                .child(icon(IconName::X, 14., cx)),
        );

    let header = h_flex()
        .h(px(42.))
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .px(px(16.))
        .py(px(8.))
        .border_b_1()
        .border_color(rgb(0xf1f2f4))
        .child(left)
        .child(right);

    // ---- 画布 ----
    let canvas = if !tab.panes.is_empty() {
        let mut panes = div().flex_1().min_h_0().flex().gap(px(1.)).bg(rgb(0xf1f2f4));
        panes = if tab.split == SplitDirection::Col {
            panes.flex_col()
        } else {
            panes.flex_row()
        };
        for (index, pane) in tab.panes.iter().enumerate() {
            let focused = tab.focused_pane == Some(index) || tab.panes.len() == 1;
            let this_focus = this.clone();
            let tab_index = tab_index;
            let mut cell = div()
                .id(SharedString::from(format!("pane-cell-{index}")))
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = this_focus
                        .update(cx, |workspace, cx| workspace.focus_pane(tab_index, index, cx));
                })
                .child(pane.clone());
            if focused && tab.panes.len() > 1 {
                cell = cell.border_1().border_color(rgb(0xd4d4d8));
            }
            panes = panes.child(cell);
        }
        panes.into_any_element()
    } else if let Some(error) = &tab.error {
        let this_retry = this.clone();
        let tab_index_retry = tab_index;
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .p(px(24.))
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(0xdc2626))
                    .child(format!("连接 {} 终端失败", host.name)),
            )
            .child(
                div()
                    .max_w(px(460.))
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .child(error.clone()),
            )
            .child(
                div()
                    .id("retry-pane")
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0xe4e4e7))
                    .bg(rgb(0xffffff))
                    .px(px(12.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let _ = this_retry.update(cx, |workspace, cx| {
                            workspace.open_tabs[tab_index_retry].error = None;
                            workspace.create_pane(tab_index_retry, cx);
                        });
                    })
                    .child("重试"),
            )
            .into_any_element()
    } else {
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(theme.muted_foreground)
            .child(format!("正在连接 {} 终端...", host.name))
            .into_any_element()
    };

    div()
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded_2xl()
        .border_1()
        .border_color(rgb(0xe4e4e7))
        .bg(rgb(0xffffff))
        .shadow_md()
        .child(header)
        .child(canvas)
        .into_any_element()
}

fn tool_button(cx: &App) -> Div {
    let _ = cx;
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(26.))
        .px(px(8.))
        .rounded_lg()
        .text_size(px(11.))
        .text_color(rgb(0x52525b))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0xf4f4f5)).text_color(rgb(0x18181b)))
}

fn wing_button(
    cx: &App,
    active: bool,
    id: &'static str,
    label: &'static str,
    icon_name: IconName,
    on_click: impl Fn(&Entity<Workspace>, &mut App) + 'static,
    this: Entity<Workspace>,
) -> impl IntoElement {
    let theme = cx.theme();
    let base = div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(26.))
        .px(px(8.))
        .rounded_lg()
        .border_1()
        .text_size(px(11.))
        .cursor_pointer()
        .on_click(move |_, _, cx| on_click(&this, cx))
        .child(icon(icon_name, 14., cx))
        .child(label);
    if active {
        base.border_color(rgb(0x18181b))
            .bg(rgb(0x18181b))
            .text_color(rgb(0xffffff))
    } else {
        base.border_color(rgb(0xe4e4e7))
            .bg(rgb(0xffffff))
            .text_color(theme.muted_foreground)
            .hover(|style| style.bg(rgb(0xf4f4f5)))
    }
}
