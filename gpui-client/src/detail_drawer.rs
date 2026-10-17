//! 右侧详情抽屉：探针仪表盘（规格 §6）。
//!
//! 已知差异：CPU 圆环暂用「大号百分比 + 横向进度条」呈现（GPUI 没有
//! CSS 圆环，自绘弧线元素放到后续迭代）；其余布局、字号、配色按规格。

use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::*;
use gpui_kit::*;

use crate::api::{Host, Inspection};
use crate::top_bar::{IconName, icon, status_dot_color};
use crate::util::{format_bytes, format_rate, format_uptime};
use crate::workspace::Workspace;

pub fn render(workspace: &Workspace, this: &Entity<Workspace>, cx: &App) -> impl IntoElement {
    let Some(host) = workspace.right_wing_host() else {
        return div().into_any_element();
    };
    let theme = cx.theme();
    let inspection = workspace.inspections.get(&host.id);
    let history: Vec<(f64, f64)> = workspace
        .net_history
        .get(&host.id)
        .cloned()
        .unwrap_or_default();

    let this_close = this.clone();
    let this_refresh = this.clone();
    let refresh_id = host.id.clone();
    let this_term = this.clone();
    let term_id = host.id.clone();
    let this_files = this.clone();
    let files_id = host.id.clone();
    let this_star = this.clone();
    let star_id = host.id.clone();

    let header = h_flex()
        .items_center()
        .justify_between()
        .px(px(16.))
        .py(px(12.))
        .border_b_1()
        .border_color(rgb(0xf1f2f4))
        .child(
            h_flex()
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
                        .truncate()
                        .child(host.name.clone()),
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .id("drawer-refresh")
                        .px(px(8.))
                        .py(px(4.))
                        .rounded_lg()
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0xf1f2f4)))
                        .on_click(move |_, _, cx| {
                            let id = refresh_id.clone();
                            let _ = this_refresh.update(cx, |workspace, cx| {
                                workspace.inspect_now(&id, cx);
                            });
                        })
                        .child("刷新"),
                )
                .child(
                    div()
                        .id("drawer-close")
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(24.))
                        .rounded_lg()
                        .text_color(theme.muted_foreground)
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0xf1f2f4)))
                        .on_click(move |_, _, cx| {
                            let _ = this_close.update(cx, |workspace, cx| {
                                workspace.right_wing = None;
                                cx.notify();
                            });
                        })
                        .child(icon(IconName::X, 14., cx)),
                ),
        );

    let body = div()
        .id("drawer-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .p(px(14.))
        .child(v_flex().gap(px(10.)).child(nameplate(host, inspection, cx)).child(cpu_card(host, inspection, cx)).child(memory_card(inspection, cx)).child(disk_card(inspection, cx)).child(network_card(inspection, &history, cx)).child(role_card(host, workspace, this, cx)));

    let footer = h_flex()
        .items_center()
        .justify_between()
        .px(px(14.))
        .py(px(10.))
        .border_t_1()
        .border_color(rgb(0xf1f2f4))
        .bg(rgb(0xfafafa))
        .child(
            h_flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .id("drawer-open-terminal")
                        .rounded_lg()
                        .bg(rgb(0x18181b))
                        .text_color(rgb(0xffffff))
                        .px(px(12.))
                        .py(px(6.))
                        .text_size(px(12.))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let id = term_id.clone();
                            let _ = this_term
                                .update(cx, |workspace, cx| workspace.open_terminal(&id, cx));
                        })
                        .child("打开终端"),
                )
                .child(
                    div()
                        .id("drawer-open-files")
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(0xe4e4e7))
                        .bg(rgb(0xffffff))
                        .px(px(12.))
                        .py(px(6.))
                        .text_size(px(12.))
                        .text_color(rgb(0x3f3f46))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let id = files_id.clone();
                            let _ = this_files
                                .update(cx, |workspace, cx| workspace.open_files(&id, cx));
                        })
                        .child("文件浏览"),
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .id("drawer-star")
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(28.))
                        .rounded_lg()
                        .text_color(if host.favorited {
                            Hsla::from(rgb(0xf59e0b))
                        } else {
                            theme.muted_foreground
                        })
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0xf1f2f4)))
                        .on_click(move |_, _, cx| {
                            let id = star_id.clone();
                            let _ = this_star
                                .update(cx, |workspace, cx| workspace.toggle_favorite(&id, cx));
                        })
                        .child(icon(IconName::Star, 15., cx)),
                ),
        );

    div()
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded_2xl()
        .border_1()
        .border_color(rgb(0xe4e4e7))
        .bg(Hsla::from(rgb(0xffffff)).opacity(0.88))
        .shadow_sm()
        .child(header)
        .child(body)
        .child(footer)
        .into_any_element()
}

fn card_shell(cx: &App) -> Div {
    let _ = cx;
    div()
        .rounded_xl()
        .border_1()
        .border_color(rgb(0xe4e4e7))
        .bg(rgb(0xffffff))
        .p(px(14.))
        .shadow_sm()
}

fn os_label(host: &Host) -> String {
    host.pretty_name
        .clone()
        .or_else(|| host.distro_id.clone())
        .or_else(|| host.os.clone())
        .unwrap_or_else(|| "正在探测系统...".into())
}

fn nameplate(host: &Host, inspection: Option<&Inspection>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let uptime = inspection
        .map(|i| format_uptime(i.uptime_seconds))
        .unwrap_or_else(|| "--".into());
    let kernel = host
        .kernel
        .as_deref()
        .map(|k| k.split('-').next().unwrap_or(k).to_owned())
        .or_else(|| host.arch.clone())
        .unwrap_or_else(|| "--".into());
    let latency = host
        .latency_ms
        .map(|ms| format!("{ms} ms"))
        .unwrap_or_else(|| "--".into());
    let cpu = inspection
        .and_then(|i| i.cpu_model.clone())
        .or_else(|| host.cpu_model.clone())
        .or_else(|| host.arch.clone())
        .unwrap_or_else(|| "正在探测处理器型号...".into());
    let tcp = inspection.and_then(|i| i.tcp_cc.clone());

    let mut shell = card_shell(cx);
    let mut title = h_flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .truncate()
                .child(os_label(host)),
        );
    if let Some(tcp) = tcp {
        let is_bbr = tcp.to_lowercase().contains("bbr");
        title = title.child(
            div()
                .rounded_md()
                .px(px(6.))
                .py(px(1.))
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .bg(if is_bbr { rgb(0xd1fae5) } else { rgb(0xf1f2f4) })
                .text_color(if is_bbr { rgb(0x047857) } else { rgb(0x52525b) })
                .child(tcp.to_uppercase()),
        );
    }
    shell = shell
        .child(title)
        .child(
            div()
                .mt(px(2.))
                .text_size(px(11.))
                .text_color(theme.muted_foreground)
                .truncate()
                .child(cpu),
        )
        .child(
            h_flex()
                .mt(px(10.))
                .pt(px(8.))
                .border_t_1()
                .border_color(rgb(0xf1f2f4))
                .justify_between()
                .child(stat_column("运行时间", uptime, cx))
                .child(stat_column("内核 / 架构", kernel, cx))
                .child(stat_column("SSH 响应", latency, cx)),
        );
    shell
}

fn stat_column(label: &'static str, value: String, cx: &App) -> impl IntoElement {
    let _ = cx;
    v_flex()
        .items_center()
        .gap(px(2.))
        .child(
            div()
                .text_size(px(10.))
                .text_color(rgb(0xa1a1aa))
                .child(label),
        )
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0x27272a))
                .child(value),
        )
}

fn cpu_card(host: &Host, inspection: Option<&Inspection>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let cores = inspection
        .and_then(|i| i.cpu_cores)
        .or(host.cpu_cores)
        .unwrap_or(1);
    let usage = inspection.map(|i| i.cpu_usage_pct).unwrap_or(0.0);
    let steal = inspection.map(|i| i.cpu_steal_pct).unwrap_or(0.0);
    let load1 = inspection.map(|i| i.load1).unwrap_or(0.0);
    let load5 = inspection.map(|i| i.load5).unwrap_or(0.0);
    let load15 = inspection.map(|i| i.load15).unwrap_or(0.0);
    let iowait = inspection.map(|i| i.cpu_iowait_pct).unwrap_or(0.0);
    let has_data = inspection.is_some();
    let usage_text = if has_data {
        format!("{usage:.1}%")
    } else {
        "--".into()
    };
    let number = |value: Option<f64>| match value {
        Some(value) if has_data => format!("{value:.2}"),
        _ => "--".into(),
    };

    card_shell(cx)
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child("CPU 算力与负载"),
                        )
                        .child(badge(format!("{cores} 核"), cx)),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(if steal >= 5.0 {
                            rgb(0xc2410c)
                        } else {
                            rgb(0x71717a)
                        })
                        .child(format!("超售 Steal {steal:.1}%")),
                ),
        )
        .child(
            h_flex()
                .mt(px(10.))
                .gap(px(14.))
                .child(
                    v_flex()
                        .items_center()
                        .gap(px(6.))
                        .w(px(56.))
                        .child(
                            div()
                                .text_size(px(18.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(if usage > 80.0 {
                                    rgb(0xf97316)
                                } else {
                                    rgb(0x18181b)
                                })
                                .child(usage_text),
                        )
                        .child(
                            div()
                                .w_full()
                                .h(px(4.))
                                .rounded_full()
                                .bg(rgb(0xf1f2f4))
                                .child(
                                    div()
                                        .h_full()
                                        .rounded_full()
                                        .bg(if usage > 80.0 {
                                            rgb(0xf97316)
                                        } else {
                                            rgb(0x18181b)
                                        })
                                        .w(relative(usage as f32 / 100.0)),
                                ),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(6.))
                        .rounded_lg()
                        .bg(rgb(0xfafafa))
                        .px(px(12.))
                        .py(px(8.))
                        .child(metric_row("1分负载", number(Some(load1)), cx))
                        .child(metric_row("5分负载", number(Some(load5)), cx))
                        .child(metric_row("15分负载", number(Some(load15)), cx))
                        .child(metric_row("IOWait", format!("{iowait:.1}%"), cx)),
                ),
        )
}

fn metric_row(label: &'static str, value: String, cx: &App) -> impl IntoElement {
    let _ = cx;
    h_flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(0xa1a1aa))
                .child(label),
        )
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0x27272a))
                .child(value),
        )
}

fn memory_card(inspection: Option<&Inspection>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (total, used, cached, swap_total, swap_used) = inspection
        .map(|i| {
            (
                i.mem_total_bytes,
                i.mem_used_bytes,
                i.mem_cached_bytes,
                i.swap_total_bytes,
                i.swap_used_bytes,
            )
        })
        .unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0));
    let used_pct = if total > 0.0 {
        ((used * 100.0) / total).round().min(100.0)
    } else {
        0.0
    };
    let cache_pct = if total > 0.0 {
        ((cached * 100.0) / total).round().min(100.0 - used_pct).max(0.0)
    } else {
        0.0
    };
    let summary = if total > 0.0 {
        format!("{} / {}", format_bytes(used), format_bytes(total))
    } else {
        "-- / --".into()
    };
    let swap = if swap_total > 0.0 {
        format!(
            "Swap: {} / {} ({}%)",
            format_bytes(swap_used),
            format_bytes(swap_total),
            ((swap_used * 100.0) / swap_total).round()
        )
    } else {
        "Swap: 未开启".into()
    };

    card_shell(cx)
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child("内存结构 (RAM & Swap)"),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(summary),
                ),
        )
        .child(
            h_flex()
                .mt(px(8.))
                .h(px(8.))
                .w_full()
                .overflow_hidden()
                .rounded_full()
                .bg(rgb(0xf1f2f4))
                .child(
                    div()
                        .h_full()
                        .bg(rgb(0x18181b))
                        .w(relative(used_pct as f32 / 100.0)),
                )
                .child(
                    div()
                        .h_full()
                        .bg(rgb(0xd4d4d8))
                        .w(relative(cache_pct as f32 / 100.0)),
                ),
        )
        .child(
            h_flex()
                .mt(px(8.))
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(4.))
                                .child(div().size(px(8.)).rounded_sm().bg(rgb(0x18181b)))
                                .child(format!("实占 {used_pct:.0}%")),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(4.))
                                .child(div().size(px(8.)).rounded_sm().bg(rgb(0xd4d4d8)))
                                .child(format!("缓存 {}", format_bytes(cached))),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(0xa1a1aa))
                        .child(swap),
                )
                .text_size(px(11.))
                .text_color(theme.muted_foreground),
        )
}

fn disk_card(inspection: Option<&Inspection>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let disks: Vec<(String, f64, f64)> = match inspection {
        Some(i) if !i.disks.is_empty() => i
            .disks
            .iter()
            .map(|d| (d.mount.clone(), d.used_bytes, d.total_bytes))
            .collect(),
        Some(i) if i.disk_total_bytes > 0.0 => vec![(
            i.disk_mount.clone().unwrap_or_else(|| "/".into()),
            i.disk_used_bytes,
            i.disk_total_bytes,
        )],
        _ => Vec::new(),
    };
    let summary = match disks.len() {
        0 => "--".to_owned(),
        1 => format!("{} / {}", format_bytes(disks[0].1), format_bytes(disks[0].2)),
        count => format!("{count} 个挂载分区"),
    };

    let mut shell = card_shell(cx).child(
        h_flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child("磁盘存储"),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(summary),
            ),
    );
    if disks.is_empty() {
        shell = shell.child(
            div()
                .mt(px(8.))
                .text_size(px(12.))
                .text_color(theme.muted_foreground)
                .child("正在探测磁盘容量..."),
        );
    }
    for (mount, used, total) in disks {
        let pct = if total > 0.0 {
            ((used * 100.0) / total).round().min(100.0)
        } else {
            0.0
        };
        shell = shell.child(
            v_flex()
                .mt(px(10.))
                .gap(px(6.))
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(mount),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme.muted_foreground)
                                .child(format!(
                                    "{} / {} ({}%)",
                                    format_bytes(used),
                                    format_bytes(total),
                                    pct
                                )),
                        ),
                )
                .child(
                    div()
                        .h(px(8.))
                        .w_full()
                        .overflow_hidden()
                        .rounded_full()
                        .bg(rgb(0xf1f2f4))
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(if pct > 85.0 {
                                    rgb(0xef4444)
                                } else {
                                    rgb(0x18181b)
                                })
                                .w(relative(pct as f32 / 100.0)),
                        ),
                ),
        );
    }
    shell
}

fn network_card(
    inspection: Option<&Inspection>,
    history: &[(f64, f64)],
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let rx = inspection.map(|i| i.net_rx_bps).unwrap_or(0.0);
    let tx = inspection.map(|i| i.net_tx_bps).unwrap_or(0.0);
    let rx_total = inspection.map(|i| i.net_rx_bytes_total).unwrap_or(0.0);
    let tx_total = inspection.map(|i| i.net_tx_bytes_total).unwrap_or(0.0);
    let iface = inspection
        .and_then(|i| i.net_iface.clone())
        .unwrap_or_else(|| "eth0".into());

    let max = history
        .iter()
        .map(|(rx, tx)| rx.max(*tx))
        .fold(1024f64, f64::max);
    let mut bars: Vec<(f32, f32)> = vec![(0.0, 0.0); 20usize.saturating_sub(history.len())];
    bars.extend(
        history
            .iter()
            .map(|(rx, tx)| {
                (
                    (rx / max * 100.0).max(6.0) as f32,
                    (tx / max * 100.0).max(6.0) as f32,
                )
            })
            .collect::<Vec<_>>(),
    );

    let mut chart = h_flex()
        .mt(px(10.))
        .h(px(48.))
        .items_end()
        .gap(px(3.))
        .rounded_lg()
        .bg(rgb(0xfafafa))
        .px(px(8.))
        .py(px(6.));
    for (rx_h, tx_h) in bars {
        chart = chart.child(
            v_flex()
                .flex_1()
                .h_full()
                .justify_end()
                .gap(px(2.))
                .child(
                    div()
                        .w_full()
                        .rounded_sm()
                        .h(relative((tx_h * 0.45 / 100.0).max(0.08)))
                        .bg(rgb(0x3f3f46)),
                )
                .child(
                    div()
                        .w_full()
                        .rounded_sm()
                        .h(relative((rx_h * 0.55 / 100.0).max(0.08)))
                        .bg(rgb(0x10b981)),
                ),
        );
    }

    card_shell(cx)
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child("网络吞吐"),
                        )
                        .child(badge(iface, cx)),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(10.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(div().text_color(rgb(0x059669)).child(format!("↓ {}", format_rate(rx))))
                        .child(div().child(format!("↑ {}", format_rate(tx)))),
                ),
        )
        .child(chart)
        .child(
            h_flex()
                .mt(px(8.))
                .pt(px(8.))
                .border_t_1()
                .border_color(rgb(0xf1f2f4))
                .items_center()
                .justify_between()
                .text_size(px(10.))
                .text_color(rgb(0xa1a1aa))
                .child(format!("累计下行: {}", format_bytes(rx_total)))
                .child(format!("累计上行: {}", format_bytes(tx_total))),
        )
}

fn role_card(
    host: &Host,
    _workspace: &Workspace,
    this: &Entity<Workspace>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let roles: [(&str, &str); 4] = [
        ("primary", "主力"),
        ("proxy", "代理"),
        ("test", "测试"),
        ("archive", "归档"),
    ];
    let mut role_row = h_flex().flex_wrap().gap(px(4.));
    for (key, label) in roles {
        let active = host.role.as_deref() == Some(key);
        let this_role = this.clone();
        let host_id = host.id.clone();
        let role_key = key.to_owned();
        let base = div()
            .id(SharedString::from(format!("role-{key}-{}", host.id)))
            .rounded_lg()
            .px(px(8.))
            .py(px(4.))
            .text_size(px(11.))
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let id = host_id.clone();
                let role = role_key.clone();
                let _ = this_role.update(cx, |workspace, cx| {
                    let next = if workspace.host_by_id(&id).and_then(|h| h.role.as_deref())
                        == Some(role.as_str())
                    {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(role)
                    };
                    workspace.patch_meta(&id, serde_json::json!({ "role": next }), cx);
                });
            })
            .child(label.to_owned());
        role_row = role_row.child(if active {
            base.bg(rgb(0x18181b)).text_color(rgb(0xffffff))
        } else {
            base.bg(rgb(0xf1f2f4)).text_color(theme.muted_foreground)
        });
    }

    let mut tag_row = h_flex().flex_wrap().gap(px(6.));
    for tag in &host.tags {
        let this_tag = this.clone();
        let host_id = host.id.clone();
        let tag_value = tag.clone();
        tag_row = tag_row.child(
            h_flex()
                .items_center()
                .gap(px(4.))
                .rounded_md()
                .border_1()
                .border_color(rgb(0xe4e4e7))
                .bg(rgb(0xfafafa))
                .px(px(6.))
                .py(px(1.))
                .text_size(px(11.))
                .child(format!("#{tag}"))
                .child(
                    div()
                        .id(SharedString::from(format!("tag-x-{tag}-{}", host.id)))
                        .text_color(rgb(0xa1a1aa))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let id = host_id.clone();
                            let value = tag_value.clone();
                            let _ = this_tag.update(cx, |workspace, cx| {
                                if let Some(host) = workspace.host_by_id(&id) {
                                    let tags: Vec<String> = host
                                        .tags
                                        .iter()
                                        .filter(|t| **t != value)
                                        .cloned()
                                        .collect();
                                    workspace.patch_meta(
                                        &id,
                                        serde_json::json!({ "tags": tags }),
                                        cx,
                                    );
                                }
                            });
                        })
                        .child("×"),
                ),
        );
    }

    let mut shell = card_shell(cx)
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child("角色分类"),
                )
                .child(if host.jump_host_name.is_some() {
                    div()
                        .rounded_md()
                        .bg(rgb(0xf1f2f4))
                        .px(px(6.))
                        .py(px(2.))
                        .text_size(px(11.))
                        .text_color(rgb(0x52525b))
                        .child(format!(
                            "跳板: {}",
                            host.jump_host_name.clone().unwrap_or_default()
                        ))
                } else {
                    div()
                }),
        )
        .child(div().mt(px(8.)).child(role_row));
    if !host.tags.is_empty() {
        shell = shell.child(div().mt(px(10.)).child(tag_row));
    }
    shell
}

fn badge(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
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
