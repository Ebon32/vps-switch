//! 仓库视图：主机卡片列表（对齐 `App.vue` 2705-2885 行 + `docs/ui-spec.md` §4）。

use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::*;
use gpui_kit::*;

use crate::api::Host;
use crate::assets::CatalogIcon;
use crate::palette::Palette;
use crate::widgets::{OsBadgeSize, lucide_icon, os_badge, status_dot};
use crate::workspace::{CenterStage, RightWing, Workspace};

pub fn render_list(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
    _cx: &App,
) -> impl IntoElement {
    let filtered = workspace.filtered_hosts();
    let show_controls = !workspace.has_stage();
    let empty = filtered.is_empty() && !workspace.loading;

    let mut list = v_flex().w_full().gap(px(8.));
    for host in &filtered {
        list = list.child(card(workspace, host, show_controls, this, palette));
    }
    if empty {
        list = list.child(empty_state(palette));
    }

    div()
        .id("host-scroll")
        .test_support()
        .size_full()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(4.))
        .py(px(2.))
        .child(list)
}

fn empty_state(palette: &Palette) -> impl IntoElement {
    div()
        .id("host-empty-state")
        .test_support()
        .mt(px(8.))
        .rounded_2xl()
        .border_1()
        .border_dashed()
        .border_color(palette.border)
        .bg(palette.card_bg)
        .p(px(40.))
        .text_center()
        .text_size(px(12.))
        .text_color(palette.text_faint)
        .child("没有匹配的 VPS 主机")
}

struct DragChip {
    name: String,
    is_dark: bool,
}

impl Render for DragChip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_mode(self.is_dark);
        div()
            .px(px(12.))
            .py(px(6.))
            .rounded_xl()
            .border_1()
            .border_color(palette.border)
            .bg(palette.card_solid_bg)
            .shadow_lg()
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette.text_primary)
            .child(self.name.clone())
    }
}

fn card(
    workspace: &Workspace,
    host: &Host,
    show_controls: bool,
    this: &Entity<Workspace>,
    palette: &Palette,
) -> impl IntoElement {
    let is_stage_host = workspace.has_stage()
        && workspace.stage_host_id.as_deref() == Some(host.id.as_str());
    let is_detail_host = workspace.right_wing == Some(RightWing::Detail)
        && workspace.detail_host_id.as_deref() == Some(host.id.as_str());
    let selected = is_stage_host || is_detail_host;

    let address = if host.kind == "local" {
        format!("{}@127.0.0.1", host.username)
    } else {
        format!("{}@{}:{}", host.username, host.addr, host.port)
    };

    let this_click = this.clone();
    let host_id = host.id.clone();
    let this_term = this.clone();
    let term_id = host.id.clone();
    let this_files = this.clone();
    let files_id = host.id.clone();
    let this_star = this.clone();
    let star_id = host.id.clone();
    let this_pin = this.clone();
    let pin_id = host.id.clone();
    let this_edit = this.clone();
    let edit_id = host.id.clone();
    let this_drop = this.clone();
    let target_id = host.id.clone();
    let drag_name = host.name.clone();
    let drag_value = host.id.clone();
    let is_dark = workspace.is_dark;

    let terminal_active = workspace.has_stage()
        && workspace.stage_host_id.as_deref() == Some(host.id.as_str())
        && workspace.center_stage == Some(CenterStage::Terminal);
    let files_active = workspace.has_stage()
        && workspace.stage_host_id.as_deref() == Some(host.id.as_str())
        && workspace.center_stage == Some(CenterStage::Files);

    // ---- 左半部 ----
    let mut left = h_flex().min_w_0().items_center().gap(px(10.));
    if selected {
        let bar_color = if is_stage_host {
            palette.accent_orange
        } else {
            palette.primary_bg
        };
        left = left.child(
            div()
                .flex_shrink_0()
                .w(px(4.))
                .h(px(30.))
                .rounded_full()
                .bg(bar_color),
        );
    } else if show_controls {
        let handle_color = if host.kind == "local" {
            palette.text_faint.opacity(0.25)
        } else {
            palette.text_faint.opacity(0.6)
        };
        left = left.child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .size(px(14.))
                .child(lucide_icon(CatalogIcon::GripVertical, 14., handle_color)),
        );
    }

    let badge_size = if show_controls {
        OsBadgeSize::Md
    } else {
        OsBadgeSize::Sm
    };
    left = left.child(os_badge(host, badge_size, palette));

    // 第一行：状态灯 + 名称 + 置顶徽章 + 角色徽章
    let checking = workspace.checking.contains(&host.id);
    let mut title_row = h_flex()
        .items_center()
        .gap(px(8.))
        .child(status_dot(
            &host.status,
            checking,
            10.0,
            format!("card-dot-{}", host.id),
            palette,
        ))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text_primary)
                .truncate()
                .child(host.name.clone()),
        );
    if is_detail_host && !is_stage_host {
        title_row = title_row.child(
            div()
                .flex_shrink_0()
                .rounded_md()
                .bg(palette.primary_bg)
                .px(px(6.))
                .py(px(1.))
                .text_size(px(10.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette.primary_fg)
                .child("详情"),
        );
    }
    if show_controls && host.pinned && host.kind != "local" {
        title_row = title_row.child(pin_chip(palette));
    }
    if show_controls
        && let Some(role) = &host.role
        && let Some(label) = role_label(role)
    {
        title_row = title_row.child(role_chip(label, palette));
    }

    // 第二行：username@addr:port + via 跳板
    let mut detail_row = h_flex()
        .mt(px(2.))
        .gap(px(8.))
        .text_size(px(11.))
        .text_color(palette.text_faint)
        .child(div().truncate().child(address));
    if show_controls && let Some(jump) = &host.jump_host_name {
        detail_row = detail_row.child(div().truncate().child(format!("via {jump}")));
    }
    left = left.child(v_flex().min_w_0().child(title_row).child(detail_row));

    // ---- 右半部：快捷按钮 ----
    let mut right = h_flex().flex_shrink_0().items_center().gap(px(6.));
    right = right
        .child(
            stage_button(terminal_active, true, palette)
                .id(SharedString::from(format!("open-term-{}", host.id)))
                .test_support()
                .on_click(move |_, _, cx| {
                    let id = term_id.clone();
                    let _ = this_term.update(cx, |w, cx| w.open_terminal(&id, cx));
                    cx.stop_propagation();
                })
                .child("终端"),
        )
        .child(
            stage_button(files_active, false, palette)
                .id(SharedString::from(format!("open-files-{}", host.id)))
                .test_support()
                .on_click(move |_, _, cx| {
                    let id = files_id.clone();
                    let _ = this_files.update(cx, |w, cx| w.open_files(&id, cx));
                    cx.stop_propagation();
                })
                .child("文件"),
        );

    if show_controls {
        // 收藏按钮
        right = right.child(
            div()
                .id(SharedString::from(format!("star-{}", host.id)))
                .test_support()
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded_xl()
                .text_size(px(15.))
                .text_color(if host.favorited {
                    palette.accent_amber
                } else {
                    palette.text_faint
                })
                .cursor_pointer()
                .hover(|s| s.bg(palette.subtle_bg))
                .on_click(move |_, _, cx| {
                    let id = star_id.clone();
                    let _ = this_star.update(cx, |w, cx| w.toggle_favorite(&id, cx));
                    cx.stop_propagation();
                })
                .child(if host.favorited { "★" } else { "☆" }),
        );

        // 置顶按钮（非本机）
        if host.kind != "local" {
            let pinned = host.pinned;
            let pin_btn = div()
                .id(SharedString::from(format!("pin-{}", host.id)))
                .test_support()
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded_xl()
                .cursor_pointer()
                .hover(|s| s.bg(palette.subtle_bg))
                .on_click(move |_, _, cx| {
                    let id = pin_id.clone();
                    let _ = this_pin.update(cx, |w, cx| w.toggle_pinned(&id, cx));
                    cx.stop_propagation();
                })
                .child(lucide_icon(
                    CatalogIcon::ArrowUpToLine,
                    15.,
                    if pinned {
                        palette.text_primary
                    } else {
                        palette.text_faint
                    },
                ));
            right = right.child(if pinned {
                pin_btn.bg(palette.subtle_bg)
            } else {
                pin_btn
            });
        }

        // 编辑按钮
        right = right.child(
            div()
                .id(SharedString::from(format!("edit-{}", host.id)))
                .test_support()
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded_xl()
                .cursor_pointer()
                .hover(|s| s.bg(palette.subtle_bg))
                .on_click(move |_, window, cx| {
                    let id = edit_id.clone();
                    let _ = this_edit.update(cx, |w, cx| w.open_edit_modal(&id, window, cx));
                    cx.stop_propagation();
                })
                .child(lucide_icon(CatalogIcon::Pencil, 15., palette.text_faint)),
        );
    }

    let base = h_flex()
        .id(SharedString::from(format!("host-card-{}", host.id)))
        .test_support()
        .relative()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .rounded_2xl()
        .px(px(16.))
        .py(px(14.))
        .cursor_pointer()
        .on_click(move |event, _, cx| {
            let id = host_id.clone();
            let double = event.click_count() >= 2;
            let _ = this_click.update(cx, |w, cx| {
                if double {
                    w.open_terminal(&id, cx);
                } else {
                    w.toggle_detail(&id, cx);
                }
            });
        })
        .on_drag(drag_value, move |_value, _offset, _window, cx| {
            cx.new(|_| DragChip {
                name: drag_name.clone(),
                is_dark,
            })
        })
        .drag_over::<String>(|style, _dragged, _window, _cx| {
            style
                .border_color(Hsla::from(rgb(0xfb923c)))
                .border_dashed()
        })
        .on_drop::<String>(move |dragged_id, _window, cx| {
            let from = dragged_id.clone();
            let to = target_id.clone();
            let _ = this_drop.update(cx, |w, cx| w.reorder(&from, &to, cx));
        })
        .child(left)
        .child(right);

    if is_stage_host {
        base.border_2()
            .border_color(palette.accent_orange)
            .bg(palette.card_selected_bg)
            .shadow(palette.card_shadow(true))
    } else if is_detail_host {
        base.border_2()
            .border_color(palette.border_selected)
            .bg(palette.card_selected_bg)
            .shadow(palette.card_shadow(true))
    } else {
        let hover_bg = palette.card_hover_bg;
        let hover_border = palette.text_faint.opacity(0.45);
        base.border_1()
            .border_color(palette.border)
            .bg(palette.card_bg)
            .shadow(palette.card_shadow(false))
            .hover(move |s| s.bg(hover_bg).border_color(hover_border))
    }
}

fn pin_chip(palette: &Palette) -> impl IntoElement {
    div()
        .rounded_md()
        .bg(palette.subtle_bg)
        .px(px(6.))
        .py(px(2.))
        .text_size(px(10.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette.text_secondary)
        .child("置顶")
}

fn role_chip(label: &'static str, palette: &Palette) -> impl IntoElement {
    div()
        .rounded_md()
        .border_1()
        .border_color(palette.border)
        .bg(palette.muted_surface_bg)
        .px(px(6.))
        .py(px(2.))
        .text_size(px(10.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette.text_muted)
        .child(label)
}

fn stage_button(active: bool, invert_hover: bool, palette: &Palette) -> Div {
    let base = div()
        .flex()
        .items_center()
        .h(px(26.))
        .px(px(10.))
        .rounded_xl()
        .border_1()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer();
    if active {
        base.border_color(palette.primary_bg)
            .bg(palette.primary_bg)
            .text_color(palette.primary_fg)
    } else if invert_hover {
        let primary_bg = palette.primary_bg;
        let primary_fg = palette.primary_fg;
        base.border_color(palette.border)
            .bg(palette.card_solid_bg)
            .text_color(palette.text_secondary)
            .hover(move |s| s.bg(primary_bg).text_color(primary_fg))
    } else {
        let subtle = palette.subtle_bg;
        base.border_color(palette.border)
            .bg(palette.card_solid_bg)
            .text_color(palette.text_secondary)
            .hover(move |s| s.bg(subtle))
    }
}

pub fn role_label(role: &str) -> Option<&'static str> {
    match role {
        "primary" => Some("主力"),
        "proxy" => Some("代理"),
        "test" => Some("测试"),
        "archive" => Some("归档"),
        _ => None,
    }
}
