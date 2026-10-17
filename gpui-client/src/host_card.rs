//! 仓库视图：主机卡片列表（规格 §4，逐项对齐旧前端 `App.vue` 2705-2885 行）。
//!
//! 卡片的三种视觉态：默认（`border-zinc-200/80` + `bg-white/82` + 细阴影）、
//! 选中（`border-zinc-900/25` + 白底 + 大阴影）、拖拽悬停（橙色虚线边框）。

use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::*;
use gpui_kit::*;

use crate::api::Host;
use crate::top_bar::{IconName, icon};
use crate::workspace::{CenterStage, RightWing, Workspace};

pub fn render_list(workspace: &Workspace, this: &Entity<Workspace>, cx: &App) -> impl IntoElement {
    let filtered = workspace.filtered_hosts();
    let show_controls = !workspace.has_stage();
    let empty = filtered.is_empty() && !workspace.loading;
    let mut list = v_flex().gap(px(8.));
    for host in &filtered {
        list = list.child(card(workspace, host, show_controls, this, cx));
    }
    let mut scroll = div()
        .id("host-scroll")
        .size_full()
        .min_h_0()
        .overflow_y_scroll()
        .pr(px(4.))
        .child(list);
    if empty {
        scroll = scroll.child(empty_state(cx));
    }
    scroll
}

fn empty_state(cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .mt(px(8.))
        .rounded_2xl()
        .border_1()
        .border_dashed()
        .border_color(rgb(0xd4d4d8))
        .bg(Hsla::from(rgb(0xffffff)).opacity(0.6))
        .p(px(40.))
        .text_center()
        .text_size(px(12.))
        .text_color(theme.muted_foreground)
        .child("没有匹配的 VPS 主机")
}

/// 拖动时跟随指针的预览块。
struct DragChip {
    name: String,
}

impl Render for DragChip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(12.))
            .py(px(6.))
            .rounded_lg()
            .border_1()
            .border_color(rgb(0xe4e4e7))
            .bg(rgb(0xffffff))
            .shadow_lg()
            .text_size(px(12.))
            .text_color(cx.theme().foreground)
            .child(self.name.clone())
    }
}

fn card(
    workspace: &Workspace,
    host: &Host,
    show_controls: bool,
    this: &Entity<Workspace>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let selected = (workspace.has_stage()
        && workspace.stage_host_id.as_deref() == Some(host.id.as_str()))
        || (workspace.right_wing == Some(RightWing::Detail)
            && workspace.detail_host_id.as_deref() == Some(host.id.as_str()));

    let address = if host.kind == "local" {
        format!("{}@127.0.0.1", host.username)
    } else {
        format!("{}@{}:{}", host.username, host.addr, host.port)
    };

    // 回调需要实体句柄。
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
    let this_drop = this.clone();
    let target_id = host.id.clone();
    let drag_name = host.name.clone();
    let drag_value = host.id.clone();

    let terminal_active = workspace.has_stage()
        && workspace.stage_host_id.as_deref() == Some(host.id.as_str())
        && workspace.center_stage == Some(CenterStage::Terminal);
    let files_active = workspace.has_stage()
        && workspace.stage_host_id.as_deref() == Some(host.id.as_str())
        && workspace.center_stage == Some(CenterStage::Files);

    // ---- 左半 ----
    let mut left = h_flex().min_w_0().items_center().gap(px(12.));
    if show_controls {
        let handle_color = if host.kind == "local" {
            Hsla::from(rgb(0xd4d4d8)).opacity(0.2)
        } else {
            Hsla::from(rgb(0xd4d4d8))
        };
        left = left.child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .size(px(14.))
                .text_color(handle_color)
                .child(icon(IconName::GripVertical, 14., cx)),
        );
    }
    left = left.child(os_badge(host, if show_controls { 40. } else { 24. }, cx));

    // 第一行：状态灯 + 名称 + 置顶 + 角色
    let mut title_row = h_flex()
        .items_center()
        .gap(px(8.))
        .child(status_dot(host, workspace))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground)
                .truncate()
                .child(host.name.clone()),
        );
    if show_controls && host.pinned && host.kind != "local" {
        title_row = title_row.child(pin_chip());
    }
    if show_controls
        && let Some(role) = &host.role
        && let Some(label) = role_label(role)
    {
        title_row = title_row.child(role_chip(label));
    }
    // 第二行：username@addr:port + via
    let mut detail_row = h_flex()
        .mt(px(2.))
        .gap(px(8.))
        .text_size(px(11.))
        .text_color(rgb(0xa1a1aa))
        .child(div().truncate().child(address));
    if show_controls
        && let Some(jump) = &host.jump_host_name
    {
        detail_row = detail_row.child(div().truncate().child(format!("via {jump}")));
    }
    left = left.child(
        v_flex()
            .min_w_0()
            .child(title_row)
            .child(detail_row),
    );

    // ---- 右半：快捷按钮 ----
    let mut right = h_flex().flex_shrink_0().items_center().gap(px(6.));
    right = right
        .child(
            stage_button(terminal_active, true)
                .id(SharedString::from(format!("open-term-{}", host.id)))
                .on_click(move |_, _, cx| {
                    let id = term_id.clone();
                    let _ = this_term.update(cx, |workspace, cx| workspace.open_terminal(&id, cx));
                    cx.stop_propagation();
                })
                .child("终端"),
        )
        .child(
            stage_button(files_active, false)
                .id(SharedString::from(format!("open-files-{}", host.id)))
                .on_click(move |_, _, cx| {
                    let id = files_id.clone();
                    let _ = this_files.update(cx, |workspace, cx| workspace.open_files(&id, cx));
                    cx.stop_propagation();
                })
                .child("文件"),
        );
    if show_controls {
        right = right.child(
            div()
                .id(SharedString::from(format!("star-{}", host.id)))
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded_lg()
                .text_size(px(15.))
                .text_color(if host.favorited {
                    Hsla::from(rgb(0xf59e0b))
                } else {
                    Hsla::from(rgb(0xa1a1aa))
                })
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0xf4f4f5)))
                .on_click(move |_, _, cx| {
                    let id = star_id.clone();
                    let _ = this_star.update(cx, |workspace, cx| workspace.toggle_favorite(&id, cx));
                    cx.stop_propagation();
                })
                .child(if host.favorited { "★" } else { "☆" }),
        );
        if host.kind != "local" {
            let pinned = host.pinned;
            right = right.child(
                div()
                    .id(SharedString::from(format!("pin-{}", host.id)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .rounded_lg()
                    .text_color(if pinned {
                        rgb(0x18181b)
                    } else {
                        rgb(0xa1a1aa)
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0xf4f4f5)))
                    .on_click(move |_, _, cx| {
                        let id = pin_id.clone();
                        let _ =
                            this_pin.update(cx, |workspace, cx| workspace.toggle_pinned(&id, cx));
                        cx.stop_propagation();
                    })
                    .child(icon(IconName::ArrowUpToLine, 16., cx)),
            );
        }
        right = right.child(
            div()
                .id(SharedString::from(format!("edit-{}", host.id)))
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded_lg()
                .text_color(rgb(0xa1a1aa))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0xf4f4f5)).text_color(rgb(0x27272a)))
                .child(icon(IconName::Pencil, 16., cx)),
        );
    }

    let base = h_flex()
        .id(SharedString::from(format!("host-card-{}", host.id)))
        .relative()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .rounded_2xl()
        .border_1()
        .px(px(16.))
        .py(px(14.))
        .cursor_pointer()
        .on_click(move |event, _, cx| {
            let id = host_id.clone();
            let double = event.click_count() >= 2;
            let _ = this_click.update(cx, |workspace, cx| {
                if double {
                    workspace.open_terminal(&id, cx);
                } else {
                    workspace.toggle_detail(&id, cx);
                }
            });
        })
        // 拖拽排序（本机不参与；拖到本机上也忽略）。
        .on_drag(drag_value, move |_value, _offset, _window, cx| {
            cx.new(|_| DragChip {
                name: drag_name.clone(),
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
            let _ = this_drop.update(cx, |workspace, cx| workspace.reorder(&from, &to, cx));
        })
        .child(left)
        .child(right);

    if selected {
        base.border_color(Hsla::from(rgb(0x18181b)).opacity(0.25))
            .bg(rgb(0xffffff))
            .shadow(vec![BoxShadow {
                color: Hsla::from(rgb(0x000000)).opacity(0.05),
                offset: point(px(0.), px(4.)),
                blur_radius: px(20.),
                spread_radius: px(0.),
                inset: false,
            }])
    } else {
        // 旧前端卡片的 `bg-white/82` 是无效的 Tailwind 类名——默认态实际透明，
        // hover 才变 95% 白（computed style 实测）。
        base.border_color(Hsla::from(rgb(0xe4e4e7)).opacity(0.8))
            .shadow(vec![BoxShadow {
                color: Hsla::from(rgb(0x000000)).opacity(0.02),
                offset: point(px(0.), px(1.)),
                blur_radius: px(4.),
                spread_radius: px(0.),
                inset: false,
            }])
            .hover(|style| {
                style
                    .bg(Hsla::from(rgb(0xffffff)).opacity(0.95))
                    .border_color(rgb(0xd4d4d8))
            })
    }
}

/// 状态灯：检测中琥珀呼吸 / 在线绿 / 离线红 / 未知灰。
fn status_dot(host: &Host, workspace: &Workspace) -> AnyElement {
    let checking = workspace.checking.contains(&host.id);
    let color = if checking {
        Hsla::from(rgb(0xfbbf24))
    } else {
        match host.status.as_str() {
            "online" => Hsla::from(rgb(0x10b981)),
            "offline" => Hsla::from(rgb(0xef4444)),
            _ => Hsla::from(rgb(0xd4d4d8)),
        }
    };
    let dot = div()
        .size(px(10.))
        .flex_shrink_0()
        .rounded_full()
        .bg(color)
        .shadow(vec![BoxShadow {
            color: color.opacity(0.55),
            offset: point(px(0.), px(0.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
            inset: false,
        }]);
    if checking {
        // 旧前端 `animate-pulse`：透明度的三角波。
        dot.with_animation(
            SharedString::from(format!("pulse-{}", host.id)),
            Animation::new(std::time::Duration::from_millis(1400)).repeat(),
            |el, delta| {
                let wave = (delta * std::f32::consts::PI).sin();
                el.opacity(0.35 + 0.65 * wave)
            },
        )
        .into_any_element()
    } else {
        dot.into_any_element()
    }
}

fn pin_chip() -> impl IntoElement {
    div()
        .rounded_md()
        .bg(rgb(0xf1f2f4))
        .px(px(6.))
        .py(px(2.))
        .text_size(px(10.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(0x52525b))
        .child("置顶")
}

fn role_chip(label: &'static str) -> impl IntoElement {
    div()
        .rounded_md()
        .border_1()
        .border_color(Hsla::from(rgb(0xe4e4e7)).opacity(0.8))
        .bg(rgb(0xfafafa))
        .px(px(6.))
        .py(px(2.))
        .text_size(px(10.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(0x71717a))
        .child(label)
}

/// 「终端 / 文件」按钮：终端 hover 反相成黑底，文件 hover 变浅灰。
fn stage_button(active: bool, invert_hover: bool) -> Div {
    let base = div()
        .flex()
        .items_center()
        .h(px(25.))
        .px(px(10.))
        .rounded_lg()
        .border_1()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer();
    if active {
        base.border_color(rgb(0x18181b))
            .bg(rgb(0x18181b))
            .text_color(rgb(0xffffff))
    } else if invert_hover {
        base.border_color(Hsla::from(rgb(0xe4e4e7)).opacity(0.8))
            .bg(rgb(0xffffff))
            .text_color(rgb(0x3f3f46))
            .hover(|style| style.bg(rgb(0x18181b)).text_color(rgb(0xffffff)))
    } else {
        base.border_color(Hsla::from(rgb(0xe4e4e7)).opacity(0.8))
            .bg(rgb(0xffffff))
            .text_color(rgb(0x3f3f46))
            .hover(|style| style.bg(rgb(0xf4f4f5)))
    }
}

/// OS 徽章：按旧 `OsBadge.vue` 的发行版判定与两字母回退。
fn os_badge(host: &Host, size: f32, cx: &App) -> impl IntoElement {
    let distro = distro_of(host);
    let (glyph, color, font) = match distro {
        "ubuntu" => ("◉".to_owned(), Hsla::from(rgb(0xe95420)), size * 0.42),
        "debian" => ("◍".to_owned(), Hsla::from(rgb(0xe11d48)), size * 0.42),
        "alpine" => ("▲".to_owned(), Hsla::from(rgb(0x3f3f46)), size * 0.34),
        "windows" => ("⊞".to_owned(), Hsla::from(rgb(0x3f3f46)), size * 0.42),
        _ => (two_letters(&host.name), cx.theme().foreground, size * 0.3),
    };
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded_full()
        .border_1()
        .border_color(Hsla::from(rgb(0xe4e4e7)).opacity(0.8))
        .bg(Hsla::from(rgb(0xf4f4f5)).opacity(0.9))
        .child(
            div()
                .text_size(px(font))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(glyph),
        )
}

fn role_label(role: &str) -> Option<&'static str> {
    match role {
        "primary" => Some("主力"),
        "proxy" => Some("代理"),
        "test" => Some("测试"),
        "archive" => Some("归档"),
        _ => None,
    }
}

fn distro_of(host: &Host) -> &'static str {
    let os = host.os.as_deref().unwrap_or("").to_lowercase();
    let distro = host.distro_id.as_deref().unwrap_or("").to_lowercase();
    let pretty = host.pretty_name.as_deref().unwrap_or("").to_lowercase();
    if os.contains("win") || distro.contains("win") || pretty.contains("windows") {
        return "windows";
    }
    if distro.contains("ubuntu") || pretty.contains("ubuntu") {
        return "ubuntu";
    }
    if distro.contains("debian") || pretty.contains("debian") {
        return "debian";
    }
    if distro.contains("alpine") || pretty.contains("alpine") {
        return "alpine";
    }
    "linux"
}

fn two_letters(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_alphanumeric())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}
