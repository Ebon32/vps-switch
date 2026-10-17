//! 设置页（规格 §7）。本轮先立骨架：分类导航 + 返回仓库；各分类内容随后逐块补齐。

use gpui_kit::component::*;
use gpui_kit::*;

use crate::workspace::{Page, Workspace};

pub fn render(_workspace: &Workspace, this: &Entity<Workspace>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let this_home = this.clone();
    let tabs = ["常规与自启", "访问安全", "MCP Server", "调用记录", "关于与更新"];
    let mut nav = h_flex()
        .items_center()
        .gap(px(6.))
        .p(px(6.))
        .rounded_2xl()
        .border_1()
        .border_color(rgb(0xe4e4e7))
        .bg(Hsla::from(rgb(0xffffff)).opacity(0.75));
    for (index, label) in tabs.iter().enumerate() {
        let active = index == 0;
        let base = h_flex()
            .items_center()
            .gap(px(8.))
            .rounded_xl()
            .px(px(16.))
            .py(px(8.))
            .text_size(px(12.))
            .child(*label);
        nav = nav.child(if active {
            base.bg(rgb(0x18181b)).text_color(rgb(0xffffff))
        } else {
            base.text_color(theme.muted_foreground).bg(rgb(0xfafafa))
        });
    }

    div()
        .id("settings-scroll")
        .size_full()
        .overflow_y_scroll()
        .p(px(20.))
        .child(
            v_flex()
                .max_w(px(896.))
                .mx_auto()
                .gap(px(16.))
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(
                            v_flex()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight::BOLD)
                                        .child("系统设置"),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.muted_foreground)
                                        .child("管理 MCP Server 远程/本机接入、调用记录与 Web 访问安全"),
                                ),
                        )
                        .child(
                            div()
                                .id("settings-back")
                                .rounded_xl()
                                .border_1()
                                .border_color(rgb(0xe4e4e7))
                                .bg(rgb(0xffffff))
                                .px(px(14.))
                                .py(px(6.))
                                .text_size(px(12.))
                                .text_color(rgb(0x3f3f46))
                                .cursor_pointer()
                                .on_click(move |_, _, cx| {
                                    let _ = this_home.update(cx, |workspace, cx| {
                                        workspace.page = Page::Workspace;
                                        cx.notify();
                                    });
                                })
                                .child("返回仓库"),
                        ),
                )
                .child(nav)
                .child(
                    div()
                        .rounded_2xl()
                        .border_1()
                        .border_color(rgb(0xe4e4e7))
                        .bg(Hsla::from(rgb(0xffffff)).opacity(0.85))
                        .p(px(20.))
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .child("设置页内容按 docs/ui-spec.md §7 逐块实现中。"),
                ),
        )
}
