//! 仓库视图：主机卡片列表（规格 §4）。

use gpui_kit::component::*;
use gpui_kit::*;

use crate::api::Host;
use crate::top_bar::{IconName, icon, status_dot_color};
use crate::workspace::{RightWing, Workspace};

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
        .child(list);
    if empty {
        scroll = scroll.child(empty_state(cx));
    }
    scroll
}

fn empty_state(cx: &App) -> impl IntoElement {
    div()
        .mt(px(8.))
        .rounded_xl()
        .border_1()
        .border_color(cx.theme().border)
        .bg(rgb(0xffffff))
        .p(px(40.))
        .text_center()
        .text_size(px(12.))
        .text_color(cx.theme().muted_foreground)
        .child("没有匹配的 VPS 主机")
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

    let terminal_active = workspace.has_stage()
        && workspace.stage_host_id.as_deref() == Some(host.id.as_str())
        && workspace.center_stage == Some(crate::workspace::CenterStage::Terminal);
    let files_active = workspace.has_stage()
        && workspace.stage_host_id.as_deref() == Some(host.id.as_str())
        && workspace.center_stage == Some(crate::workspace::CenterStage::Files);

    let mut left = h_flex()
        .min_w_0()
        .items_center()
        .gap(px(12.))
        .child(os_badge(host, if show_controls { 40. } else { 24. }, cx));
    let mut title_row = h_flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .size(px(10.))
                .rounded_full()
                .bg(status_dot_color(&host.status, cx))
                .shadow_sm(),
        )
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground)
                .truncate()
                .child(host.name.clone()),
        );
    if show_controls && host.pinned && host.kind != "local" {
        title_row = title_row.child(tag_chip("置顶", cx));
    }
    if show_controls
        && let Some(role) = &host.role
        && let Some(label) = role_label(role)
    {
        title_row = title_row.child(tag_chip(label, cx));
    }
    let mut detail_row = h_flex()
        .gap(px(8.))
        .text_size(px(11.))
        .text_color(theme.muted_foreground)
        .child(div().truncate().child(address));
    if show_controls
        && let Some(jump) = &host.jump_host_name
    {
        detail_row = detail_row.child(div().truncate().child(format!("via {jump}")));
    }
    left = left.child(
        v_flex()
            .min_w_0()
            .gap(px(2.))
            .child(title_row)
            .child(detail_row),
    );

    let mut right = h_flex().flex_shrink_0().items_center().gap(px(6.));
    right = right
        .child(
            small_button(terminal_active, cx)
                .id(SharedString::from(format!("open-term-{}", host.id)))
                .on_click(move |_, _, cx| {
                    let id = term_id.clone();
                    let _ = this_term.update(cx, |workspace, cx| workspace.open_terminal(&id, cx));
                    cx.stop_propagation();
                })
                .child("终端"),
        )
        .child(
            small_button(files_active, cx)
                .id(SharedString::from(format!("open-files-{}", host.id)))
                .on_click(move |_, _, cx| {
                    let id = files_id.clone();
                    let _ = this_files.update(cx, |workspace, cx| workspace.open_files(&id, cx));
                    cx.stop_propagation();
                })
                .child("文件"),
        );
    if show_controls {
        let star_color = if host.favorited {
            Hsla::from(rgb(0xf59e0b))
        } else {
            theme.muted_foreground
        };
        right = right.child(
            div()
                .id(SharedString::from(format!("star-{}", host.id)))
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded_lg()
                .text_color(star_color)
                .hover(|style| style.bg(rgb(0xf1f2f4)))
                .on_click(move |_, _, cx| {
                    let id = star_id.clone();
                    let _ = this_star.update(cx, |workspace, cx| workspace.toggle_favorite(&id, cx));
                    cx.stop_propagation();
                })
                .child(icon(IconName::Star, 16., cx)),
        );
        if host.kind != "local" {
            let pin_color = if host.pinned {
                theme.foreground
            } else {
                theme.muted_foreground
            };
            right = right.child(
                div()
                    .id(SharedString::from(format!("pin-{}", host.id)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .rounded_lg()
                    .text_color(pin_color)
                    .hover(|style| style.bg(rgb(0xf1f2f4)))
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
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded_lg()
                .text_color(theme.muted_foreground)
                .hover(|style| style.bg(rgb(0xf1f2f4)))
                .child(icon(IconName::Pencil, 16., cx)),
        );
    }

    let base = h_flex()
        .id(SharedString::from(format!("host-card-{}", host.id)))
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
        .child(left)
        .child(right);

    if selected {
        base.border_color(rgb(0xd4d4d8))
            .bg(rgb(0xffffff))
            .shadow_md()
    } else {
        base.border_color(rgb(0xe4e4e7))
            .bg(Hsla::from(rgb(0xffffff)).opacity(0.82))
            .hover(|style| style.bg(rgb(0xffffff)).border_color(rgb(0xd4d4d8)))
    }
}

fn small_button(active: bool, _cx: &App) -> Div {
    let base = div()
        .flex()
        .items_center()
        .h(px(26.))
        .px(px(10.))
        .rounded_lg()
        .border_1()
        .text_size(px(12.))
        .cursor_pointer();
    if active {
        base.border_color(rgb(0x18181b))
            .bg(rgb(0x18181b))
            .text_color(rgb(0xffffff))
    } else {
        base.border_color(rgb(0xe4e4e7))
            .bg(rgb(0xffffff))
            .text_color(rgb(0x3f3f46))
            .hover(|style| style.bg(rgb(0xf1f2f4)))
    }
}

fn tag_chip(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    let _ = cx;
    div()
        .rounded_md()
        .bg(rgb(0xf1f2f4))
        .px(px(6.))
        .py(px(1.))
        .text_size(px(10.))
        .text_color(rgb(0x52525b))
        .child(text.into())
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
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded_full()
        .border_1()
        .border_color(rgb(0xe4e4e7))
        .bg(rgb(0xf4f4f5))
        .child(
            div()
                .text_size(px(font))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(glyph),
        )
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
