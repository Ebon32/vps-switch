//! 终端舞台：顶栏（主机状态 + OSC 7 工作目录胶囊 + OSC 133 运行态/退出码 + 分屏/黑白底/右栏/独立窗口/收起）
//! 与分屏画布（对齐 `TerminalStage.vue` + `docs/ui-spec.md` §5）。

use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::*;
use gpui_kit::*;

use crate::assets::CatalogIcon;
use crate::palette::Palette;
use crate::widgets::{lucide_icon, status_dot};
use crate::workspace::{RightWing, SplitDirection, Workspace};

pub fn render(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
    cx: &App,
) -> AnyElement {
    let Some(tab_index) = workspace.active_tab_index() else {
        return div().into_any_element();
    };
    let tab = &workspace.open_tabs[tab_index];
    let Some(host) = workspace.host_by_id(&tab.host_id) else {
        return div().into_any_element();
    };

    let focused_idx = tab
        .focused_pane
        .filter(|i| *i < tab.panes.len())
        .or_else(|| (!tab.panes.is_empty()).then(|| tab.panes.len() - 1));
    let active_meta = focused_idx.map(|idx| tab.panes[idx].read(cx).meta.clone());

    // ---- 顶栏左侧 ----
    let address = if host.kind == "local" {
        "PowerShell (ConPTY)".to_owned()
    } else {
        format!("{}@{}:{}", host.username, host.addr, host.port)
    };

    let mut left = h_flex()
        .min_w_0()
        .items_center()
        .gap(px(8.))
        .child(status_dot(
            &host.status,
            false,
            10.0,
            format!("stage-dot-{}", host.id),
            palette,
        ))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text_primary)
                .truncate()
                .child(host.name.clone()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(palette.text_faint)
                .child(address),
        );

    // OSC 7 实时工作目录胶囊
    if let Some(cwd) = active_meta
        .as_ref()
        .and_then(|m| m.cwd.as_deref())
        .filter(|s| !s.is_empty())
    {
        left = left.child(
            h_flex()
                .id("stage-cwd-pill")
                .test_support()
                .max_w(px(200.))
                .items_center()
                .gap(px(4.))
                .rounded_lg()
                .border_1()
                .border_color(palette.border)
                .bg(palette.muted_surface_bg)
                .px(px(8.))
                .py(px(2.))
                .text_size(px(11.))
                .text_color(palette.text_secondary)
                .child(lucide_icon(CatalogIcon::Folder, 12., palette.text_faint))
                .child(div().truncate().child(cwd.to_owned())),
        );
    }

    // OSC 133 实时命令运行态与真实 Exit Code
    if let Some(meta) = &active_meta {
        if meta.running {
            left = left.child(
                h_flex()
                    .id("stage-running-badge")
                    .test_support()
                    .items_center()
                    .gap(px(4.))
                    .rounded_lg()
                    .bg(Hsla::from(rgb(0xfffbeb)))
                    .px(px(8.))
                    .py(px(2.))
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(Hsla::from(rgb(0xb45309)))
                    .child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .bg(Hsla::from(rgb(0xf59e0b))),
                    )
                    .child("运行中"),
            );
        } else if let Some(code) = meta.last_exit_code {
            let ok = code == 0;
            left = left.child(
                div()
                    .id("stage-exit-code-badge")
                    .test_support()
                    .rounded_lg()
                    .px(px(6.))
                    .py(px(2.))
                    .text_size(px(10.))
                    .font_weight(FontWeight::MEDIUM)
                    .bg(if ok {
                        Hsla::from(rgb(0xecfdf5))
                    } else {
                        Hsla::from(rgb(0xfef2f2))
                    })
                    .text_color(if ok {
                        Hsla::from(rgb(0x047857))
                    } else {
                        Hsla::from(rgb(0xdc2626))
                    })
                    .child(if ok {
                        "✓ 0".to_owned()
                    } else {
                        format!("✗ {code}")
                    }),
            );
        }
    }

    // ---- 顶栏右侧工具条 ----
    let host_id = host.id.clone();
    let terminal_dark = workspace.terminal_dark;

    let this_split_row = this.clone();
    let this_split_col = this.clone();
    let this_refresh = this.clone();
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
        // 左右分屏 / 上下分屏
        .child(
            h_flex()
                .items_center()
                .gap(px(2.))
                .p(px(2.))
                .rounded_xl()
                .border_1()
                .border_color(palette.border)
                .bg(palette.muted_surface_bg)
                .child(
                    tool_button(palette)
                        .id("split-row")
                        .test_support()
                        .on_click(move |_, window, cx| {
                            let _ = this_split_row.update(cx, |w, cx| {
                                w.split(SplitDirection::Row, window, cx);
                            });
                        })
                        .child(lucide_icon(
                            CatalogIcon::Columns2,
                            13.,
                            palette.text_secondary,
                        ))
                        .child("左右分屏"),
                )
                .child(
                    tool_button(palette)
                        .id("split-col")
                        .test_support()
                        .on_click(move |_, window, cx| {
                            let _ = this_split_col.update(cx, |w, cx| {
                                w.split(SplitDirection::Col, window, cx);
                            });
                        })
                        .child(lucide_icon(CatalogIcon::Rows2, 13., palette.text_secondary))
                        .child("上下分屏"),
                ),
        )
        // 刷新当前终端会话
        .child(
            bordered_tool_button(palette)
                .id("refresh-terminal")
                .test_support()
                .on_click(move |_, window, cx| {
                    let _ = this_refresh.update(cx, |w, cx| {
                        w.refresh_active_terminal_pane(window, cx);
                    });
                })
                .child(lucide_icon(
                    CatalogIcon::RefreshCw,
                    13.,
                    palette.text_secondary,
                ))
                .child("刷新"),
        )
        // 白底 / 黑底切换
        .child(
            bordered_tool_button(palette)
                .id("toggle-terminal-dark")
                .test_support()
                .on_click(move |_, _, cx| {
                    let _ = this_dark.update(cx, |w, cx| w.toggle_terminal_dark(cx));
                })
                .child(if terminal_dark { "白底" } else { "黑底" }),
        )
        // 文件管理右栏开关
        .child(wing_button(
            palette,
            workspace.right_wing == Some(RightWing::Files),
            "toggle-right-files",
            "文件管理",
            CatalogIcon::Folder,
            move |this, cx| {
                let _ = this.update(cx, |w, cx| w.toggle_right_wing(RightWing::Files, cx));
            },
            this_files,
        ))
        // 详情图表右栏开关
        .child(wing_button(
            palette,
            workspace.right_wing == Some(RightWing::Detail),
            "toggle-right-detail",
            "详情图表",
            CatalogIcon::ChartColumnBig,
            move |this, cx| {
                let _ = this.update(cx, |w, cx| w.toggle_right_wing(RightWing::Detail, cx));
            },
            this_detail,
        ))
        // 独立窗口
        .child(
            bordered_tool_button(palette)
                .id("popout-terminal")
                .test_support()
                .on_click(move |_, _, cx| {
                    let id = popout_id.clone();
                    let _ = this_popout.update(cx, |w, cx| w.popout_terminal(&id, cx));
                })
                .child(lucide_icon(
                    CatalogIcon::ExternalLink,
                    13.,
                    palette.text_secondary,
                ))
                .child("独立窗口"),
        )
        // 收起舞台
        .child(
            div()
                .id("collapse-stage")
                .test_support()
                .flex()
                .items_center()
                .justify_center()
                .size(px(26.))
                .rounded_lg()
                .cursor_pointer()
                .hover(|s| s.bg(palette.subtle_bg))
                .on_click(move |_, _, cx| {
                    let _ = this_collapse.update(cx, |w, cx| w.close_stage(cx));
                })
                .child(lucide_icon(CatalogIcon::X, 14., palette.text_faint)),
        );

    let header = h_flex()
        .h(px(44.))
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .px(px(16.))
        .py(px(8.))
        .border_b_1()
        .border_color(palette.divider)
        .child(left)
        .child(right);

    // ---- 画布区 ----
    let single_error = if tab.panes.len() == 1 {
        tab.panes[0].read(cx).create_error.clone()
    } else {
        None
    };
    let single_connecting = if tab.panes.len() == 1 {
        let p = tab.panes[0].read(cx);
        p.connecting && p.snapshot.is_none() && p.create_error.is_none()
    } else {
        false
    };

    let canvas = if let Some(err) = single_error {
        let pane_entity = tab.panes[0].clone();
        v_flex()
            .id("terminal-stage-error")
            .test_support()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .p(px(24.))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(0xdc2626))
                    .child(lucide_icon(
                        CatalogIcon::CircleAlert,
                        15.,
                        Hsla::from(rgb(0xdc2626)),
                    ))
                    .child(format!("连接 {} 终端失败", host.name)),
            )
            .child(
                div()
                    .max_w(px(460.))
                    .text_size(px(12.))
                    .text_color(palette.text_muted)
                    .child(err),
            )
            .child(
                div()
                    .id("retry-pane")
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
                    .hover(|s| s.bg(palette.subtle_bg))
                    .on_click(move |_, _, cx| {
                        let _ = pane_entity.update(cx, |v, cx| v.reconnect_fresh(cx));
                    })
                    .child("重试"),
            )
            .into_any_element()
    } else if single_connecting {
        v_flex()
            .id("terminal-stage-loading")
            .test_support()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(palette.text_faint)
            .child(format!("正在连接 {} 终端...", host.name))
            .into_any_element()
    } else {
        let mut panes_container = div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(1.))
            .bg(palette.divider);
        panes_container = if tab.split == SplitDirection::Col {
            panes_container.flex_col()
        } else {
            panes_container.flex_row()
        };

        let multi = tab.panes.len() > 1;
        for (index, pane) in tab.panes.iter().enumerate() {
            let focused = focused_idx == Some(index);
            let this_focus = this.clone();
            let this_close_pane = this.clone();
            let mut cell = div()
                .id(SharedString::from(format!("pane-cell-{index}")))
                .test_support()
                .relative()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = this_focus.update(cx, |w, cx| w.focus_pane(tab_index, index, cx));
                })
                .child(pane.clone());

            if focused && multi {
                cell = cell.border_1().border_color(palette.border_selected);
            }

            if multi {
                let close_btn = div()
                    .id(SharedString::from(format!("close-pane-{index}")))
                    .test_support()
                    .absolute()
                    .top(px(8.))
                    .right(px(10.))
                    .rounded_lg()
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.card_bg)
                    .px(px(6.))
                    .py(px(2.))
                    .text_size(px(11.))
                    .text_color(palette.text_muted)
                    .cursor_pointer()
                    .hover(|s| s.bg(palette.card_solid_bg).text_color(palette.text_primary))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(move |_, _, cx| {
                        let _ = this_close_pane
                            .update(cx, |w, cx| w.close_pane(tab_index, index, cx));
                        cx.stop_propagation();
                    })
                    .child("关闭分屏 ✕");
                cell = cell.child(close_btn);
            }

            panes_container = panes_container.child(cell);
        }
        panes_container.into_any_element()
    };

    div()
        .id("terminal-stage")
        .test_support()
        .size_full()
        .min_h_0()
        .min_w(px(420.))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded_2xl()
        .border_1()
        .border_color(palette.border)
        .bg(palette.card_solid_bg)
        .shadow(palette.card_shadow(true))
        .child(header)
        .child(canvas)
        .into_any_element()
}

fn tool_button(palette: &Palette) -> Div {
    let solid = palette.card_solid_bg;
    let primary = palette.text_primary;
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .h(px(24.))
        .px(px(8.))
        .rounded_lg()
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette.text_secondary)
        .cursor_pointer()
        .hover(move |s| s.bg(solid).text_color(primary))
}

fn bordered_tool_button(palette: &Palette) -> Div {
    let subtle = palette.subtle_bg;
    let primary = palette.text_primary;
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .h(px(28.))
        .px(px(10.))
        .rounded_xl()
        .border_1()
        .border_color(palette.border)
        .bg(palette.card_solid_bg)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette.text_secondary)
        .cursor_pointer()
        .hover(move |s| s.bg(subtle).text_color(primary))
}

fn wing_button(
    palette: &Palette,
    active: bool,
    id: &'static str,
    label: &'static str,
    icon_name: CatalogIcon,
    on_click: impl Fn(&Entity<Workspace>, &mut App) + 'static,
    this: Entity<Workspace>,
) -> impl IntoElement {
    let fg = if active {
        palette.primary_fg
    } else {
        palette.text_secondary
    };
    let base = div()
        .id(id)
        .test_support()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(28.))
        .px(px(10.))
        .rounded_xl()
        .border_1()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .on_click(move |_, _, cx| on_click(&this, cx))
        .child(lucide_icon(icon_name, 13.5, fg))
        .child(label);
    if active {
        base.border_color(palette.primary_bg)
            .bg(palette.primary_bg)
            .text_color(palette.primary_fg)
            .shadow_sm()
    } else {
        let subtle = palette.subtle_bg;
        base.border_color(palette.border)
            .bg(palette.card_solid_bg)
            .text_color(palette.text_secondary)
            .hover(move |s| s.bg(subtle))
    }
}
