//! 右侧详情抽屉：环境铭牌、探针仪表盘、角色/标签/快捷链接与底部操作栏
//! （1:1 对齐 `HostDetailDrawer.vue` + `docs/ui-spec.md` §6）。

use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::*;

use crate::api::{Host, Inspection};
use crate::assets::CatalogIcon;
use crate::palette::Palette;
use crate::util::{format_bytes, format_rate, format_uptime};
use crate::widgets::{OsBadgeSize, cpu_ring, lucide_icon, os_badge, status_dot};
use crate::workspace::Workspace;

pub fn render(
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
    _cx: &App,
) -> impl IntoElement {
    let Some(host) = workspace.right_wing_host() else {
        return div().into_any_element();
    };
    let inspection = workspace.inspections.get(&host.id);
    let inspect_err = workspace.inspection_errors.get(&host.id);
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
    let this_edit = this.clone();
    let edit_id = host.id.clone();
    let this_delete = this.clone();
    let delete_id = host.id.clone();

    let is_inspecting = workspace.inspecting_host.as_deref() == Some(host.id.as_str());

    // 1. 顶部栏
    let mut header_left = h_flex()
        .min_w_0()
        .items_center()
        .gap(px(8.))
        .child(status_dot(
            &host.status,
            false,
            10.0,
            format!("drawer-dot-{}", host.id),
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
    if is_inspecting {
        header_left = header_left.child(
            div()
                .size(px(6.))
                .rounded_full()
                .bg(palette.text_faint),
        );
    }

    let header = h_flex()
        .items_center()
        .justify_between()
        .px(px(16.))
        .py(px(12.))
        .border_b_1()
        .border_color(palette.divider)
        .child(header_left)
        .child(
            h_flex()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .id("drawer-refresh")
                        .test_support()
                        .px(px(8.))
                        .py(px(4.))
                        .rounded_lg()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.text_secondary)
                        .cursor_pointer()
                        .hover(|s| s.bg(palette.subtle_bg).text_color(palette.text_primary))
                        .on_click(move |_, _, cx| {
                            let id = refresh_id.clone();
                            let _ = this_refresh.update(cx, |w, cx| w.inspect_now(&id, cx));
                        })
                        .child("刷新"),
                )
                .child(
                    div()
                        .id("drawer-close")
                        .test_support()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(24.))
                        .rounded_lg()
                        .cursor_pointer()
                        .hover(|s| s.bg(palette.subtle_bg))
                        .on_click(move |_, _, cx| {
                            let _ = this_close.update(cx, |w, cx| {
                                w.right_wing = None;
                                cx.notify();
                            });
                        })
                        .child(lucide_icon(CatalogIcon::X, 14., palette.text_faint)),
                ),
        );

    // 2. 滚动内容区（外包 v_flex().w_full() 避免 Scrollable 宽度坍缩）
    let mut stack = v_flex()
        .w_full()
        .gap(px(10.))
        .child(nameplate(host, inspection, palette));

    if let Some(err) = inspect_err
        && inspection.is_none()
    {
        stack = stack.child(
            div()
                .rounded_xl()
                .border_1()
                .border_color(rgb(0xfecaca))
                .bg(rgb(0xfef2f2))
                .px(px(12.))
                .py(px(8.))
                .text_size(px(11.))
                .text_color(rgb(0xb91c1c))
                .child(err.clone()),
        );
    }

    stack = stack
        .child(cpu_card(host, inspection, palette))
        .child(memory_card(inspection, palette))
        .child(disk_card(inspection, palette))
        .child(network_card(inspection, &history, palette))
        .child(role_tags_links_card(host, workspace, this, palette));

    let body = div()
        .id("drawer-scroll")
        .test_support()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .p(px(14.))
        .child(stack);

    // 3. 底部操作栏（左：打开终端 / 文件浏览；右：编辑 / 删除）
    let mut footer_right = h_flex().items_center().gap(px(4.)).child(
        div()
            .id("drawer-edit")
            .test_support()
            .rounded_lg()
            .px(px(10.))
            .py(px(6.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette.text_secondary)
            .cursor_pointer()
            .hover(|s| s.bg(palette.subtle_bg).text_color(palette.text_primary))
            .on_click(move |_, window, cx| {
                let id = edit_id.clone();
                let _ = this_edit.update(cx, |w, cx| w.open_edit_modal(&id, window, cx));
            })
            .child("编辑"),
    );

    if host.kind != "local" {
        footer_right = footer_right.child(
            div()
                .id("drawer-delete")
                .test_support()
                .rounded_lg()
                .px(px(8.))
                .py(px(6.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(0xdc2626))
                .cursor_pointer()
                .hover(|s| s.bg(Hsla::from(rgb(0xfef2f2))))
                .on_click(move |_, _, cx| {
                    let id = delete_id.clone();
                    let _ = this_delete.update(cx, |w, cx| w.request_delete_host(&id, cx));
                })
                .child("删除"),
        );
    }

    let footer = h_flex()
        .items_center()
        .justify_between()
        .px(px(14.))
        .py(px(10.))
        .border_t_1()
        .border_color(palette.divider)
        .bg(palette.muted_surface_bg)
        .child(
            h_flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .id("drawer-open-terminal")
                        .test_support()
                        .rounded_lg()
                        .bg(palette.primary_bg)
                        .text_color(palette.primary_fg)
                        .px(px(12.))
                        .py(px(6.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let id = term_id.clone();
                            let _ = this_term.update(cx, |w, cx| w.open_terminal(&id, cx));
                        })
                        .child("打开终端"),
                )
                .child(
                    div()
                        .id("drawer-open-files")
                        .test_support()
                        .rounded_lg()
                        .border_1()
                        .border_color(palette.border)
                        .bg(palette.card_solid_bg)
                        .px(px(12.))
                        .py(px(6.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.text_secondary)
                        .cursor_pointer()
                        .hover(|s| s.bg(palette.subtle_bg))
                        .on_click(move |_, _, cx| {
                            let id = files_id.clone();
                            let _ = this_files.update(cx, |w, cx| w.open_files(&id, cx));
                        })
                        .child("文件浏览"),
                ),
        )
        .child(footer_right);

    div()
        .id("detail-drawer")
        .test_support()
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded_2xl()
        .border_1()
        .border_color(palette.border)
        .bg(palette.panel_bg)
        .shadow_sm()
        .child(header)
        .child(body)
        .child(footer)
        .into_any_element()
}

fn card_shell(palette: &Palette) -> Div {
    div()
        .rounded_xl()
        .border_1()
        .border_color(palette.border)
        .bg(palette.card_solid_bg)
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

fn nameplate(host: &Host, inspection: Option<&Inspection>, palette: &Palette) -> impl IntoElement {
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

    let mut title_row = h_flex().items_center().justify_between().gap(px(4.)).child(
        div()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.text_primary)
            .truncate()
            .child(os_label(host)),
    );
    if let Some(tcp) = tcp {
        let is_bbr = tcp.to_lowercase().contains("bbr");
        title_row = title_row.child(
            div()
                .flex_shrink_0()
                .rounded_md()
                .px(px(6.))
                .py(px(1.))
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .bg(if is_bbr {
                    Hsla::from(rgb(0xd1fae5))
                } else {
                    palette.subtle_bg
                })
                .text_color(if is_bbr {
                    Hsla::from(rgb(0x047857))
                } else {
                    palette.text_secondary
                })
                .child(tcp.to_uppercase()),
        );
    }

    card_shell(palette)
        .child(
            h_flex()
                .items_start()
                .gap(px(10.))
                .child(os_badge(host, OsBadgeSize::Md, palette))
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .child(title_row)
                        .child(
                            div()
                                .mt(px(2.))
                                .text_size(px(11.))
                                .text_color(palette.text_muted)
                                .truncate()
                                .child(cpu),
                        ),
                ),
        )
        .child(
            h_flex()
                .mt(px(10.))
                .pt(px(8.))
                .border_t_1()
                .border_color(palette.divider)
                .justify_between()
                .child(stat_column("运行时间", uptime, palette))
                .child(stat_column("内核 / 架构", kernel, palette))
                .child(stat_column("SSH 响应", latency, palette)),
        )
}

fn stat_column(label: &'static str, value: String, palette: &Palette) -> impl IntoElement {
    v_flex()
        .items_center()
        .gap(px(2.))
        .child(
            div()
                .text_size(px(10.))
                .text_color(palette.text_faint)
                .child(label),
        )
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text_primary)
                .child(value),
        )
}

fn cpu_card(host: &Host, inspection: Option<&Inspection>, palette: &Palette) -> impl IntoElement {
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

    let fmt_load = |val: f64| {
        if has_data {
            format!("{val:.2}")
        } else {
            "--".into()
        }
    };

    card_shell(palette)
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
                                .text_color(palette.text_muted)
                                .child("CPU 算力与负载"),
                        )
                        .child(badge(format!("{cores} 核"), palette)),
                )
                .child(
                    div()
                        .rounded(px(4.))
                        .px(px(6.))
                        .py(px(1.))
                        .text_size(px(10.))
                        .font_weight(FontWeight::MEDIUM)
                        .bg(if steal >= 5.0 {
                            Hsla::from(rgb(0xffedd5))
                        } else {
                            palette.subtle_bg
                        })
                        .text_color(if steal >= 5.0 {
                            Hsla::from(rgb(0xc2410c))
                        } else {
                            palette.text_muted
                        })
                        .child(format!("超售 Steal {steal:.1}%")),
                ),
        )
        .child(
            h_flex()
                .mt(px(10.))
                .items_center()
                .gap(px(14.))
                // 左侧 52x52px 原生矢量圆环
                .child(cpu_ring(usage, has_data, palette))
                // 右侧 2x2 负载矩阵
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(6.))
                        .rounded_lg()
                        .bg(palette.muted_surface_bg)
                        .px(px(10.))
                        .py(px(8.))
                        .child(
                            h_flex()
                                .gap(px(12.))
                                .child(metric_cell("1分负载", fmt_load(load1), palette))
                                .child(metric_cell("5分负载", fmt_load(load5), palette)),
                        )
                        .child(
                            h_flex()
                                .gap(px(12.))
                                .child(metric_cell("15分负载", fmt_load(load15), palette))
                                .child(metric_cell(
                                    "IOWait",
                                    if has_data {
                                        format!("{iowait:.1}%")
                                    } else {
                                        "--".into()
                                    },
                                    palette,
                                )),
                        ),
                ),
        )
}

fn metric_cell(label: &'static str, value: String, palette: &Palette) -> impl IntoElement {
    h_flex()
        .flex_1()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.text_faint)
                .child(label),
        )
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text_primary)
                .child(value),
        )
}

fn memory_card(inspection: Option<&Inspection>, palette: &Palette) -> impl IntoElement {
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
        ((cached * 100.0) / total)
            .round()
            .min(100.0 - used_pct)
            .max(0.0)
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
            "Swap: {} / {} ({:.0}%)",
            format_bytes(swap_used),
            format_bytes(swap_total),
            ((swap_used * 100.0) / swap_total).round()
        )
    } else {
        "Swap: 未开启".into()
    };

    card_shell(palette)
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text_muted)
                        .child("内存结构 (RAM & Swap)"),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text_primary)
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
                .bg(palette.subtle_bg)
                .child(
                    div()
                        .h_full()
                        .bg(palette.primary_bg)
                        .w(relative(used_pct as f32 / 100.0)),
                )
                .child(
                    div()
                        .h_full()
                        .bg(palette.text_faint.opacity(0.45))
                        .w(relative(cache_pct as f32 / 100.0)),
                ),
        )
        .child(
            h_flex()
                .mt(px(8.))
                .items_center()
                .justify_between()
                .text_size(px(11.))
                .text_color(palette.text_muted)
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(4.))
                                .child(div().size(px(8.)).rounded_sm().bg(palette.primary_bg))
                                .child(format!("实占 {used_pct:.0}%")),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .size(px(8.))
                                        .rounded_sm()
                                        .bg(palette.text_faint.opacity(0.45)),
                                )
                                .child(format!("缓存 {}", format_bytes(cached))),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(palette.text_faint)
                        .child(swap),
                ),
        )
}

fn disk_card(inspection: Option<&Inspection>, palette: &Palette) -> impl IntoElement {
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

    let mut shell = card_shell(palette).child(
        h_flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.text_muted)
                    .child("磁盘存储"),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.text_primary)
                    .child(summary),
            ),
    );
    if disks.is_empty() {
        shell = shell.child(
            div()
                .mt(px(8.))
                .text_size(px(12.))
                .text_color(palette.text_faint)
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
                                .text_color(palette.text_primary)
                                .child(mount),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(palette.text_muted)
                                .child(format!(
                                    "{} / {} ({pct:.0}%)",
                                    format_bytes(used),
                                    format_bytes(total),
                                )),
                        ),
                )
                .child(
                    div()
                        .h(px(8.))
                        .w_full()
                        .overflow_hidden()
                        .rounded_full()
                        .bg(palette.subtle_bg)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(if pct > 85.0 {
                                    palette.status_offline
                                } else {
                                    palette.primary_bg
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
    palette: &Palette,
) -> impl IntoElement {
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
    let mut bars: Vec<(f32, f32)> = vec![(8.0, 6.0); 20usize.saturating_sub(history.len())];
    bars.extend(history.iter().map(|(rx, tx)| {
        (
            (rx / max * 100.0).max(6.0) as f32,
            (tx / max * 100.0).max(6.0) as f32,
        )
    }));

    let mut chart = h_flex()
        .mt(px(10.))
        .h(px(48.))
        .items_end()
        .gap(px(3.))
        .rounded_lg()
        .bg(palette.muted_surface_bg)
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
                        .bg(palette.text_secondary.opacity(0.75)),
                )
                .child(
                    div()
                        .w_full()
                        .rounded_sm()
                        .h(relative((rx_h * 0.55 / 100.0).max(0.08)))
                        .bg(palette.status_online.opacity(0.85)),
                ),
        );
    }

    card_shell(palette)
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
                                .text_color(palette.text_muted)
                                .child("网络吞吐"),
                        )
                        .child(badge(iface, palette)),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(10.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(
                            div()
                                .text_color(palette.status_online)
                                .child(format!("↓ {}", format_rate(rx))),
                        )
                        .child(
                            div()
                                .text_color(palette.text_primary)
                                .child(format!("↑ {}", format_rate(tx))),
                        ),
                ),
        )
        .child(chart)
        .child(
            h_flex()
                .mt(px(8.))
                .pt(px(8.))
                .border_t_1()
                .border_color(palette.divider)
                .items_center()
                .justify_between()
                .text_size(px(10.))
                .text_color(palette.text_faint)
                .child(format!("累计下行: {}", format_bytes(rx_total)))
                .child(format!("累计上行: {}", format_bytes(tx_total))),
        )
}

/// 第 6 块：角色分类（含「未标记」）、标签列表 + 添加标签输入框、快捷链接列表（点击打开浏览器）。
fn role_tags_links_card(
    host: &Host,
    workspace: &Workspace,
    this: &Entity<Workspace>,
    palette: &Palette,
) -> impl IntoElement {
    let roles: [(&str, &str); 5] = [
        ("", "未标记"),
        ("primary", "主力"),
        ("proxy", "代理"),
        ("test", "测试"),
        ("archive", "归档"),
    ];
    let current_role = host.role.as_deref().unwrap_or("");
    let mut role_row = h_flex().flex_wrap().gap(px(4.));
    for (key, label) in roles {
        let active = current_role == key;
        let this_role = this.clone();
        let host_id = host.id.clone();
        let role_key = key.to_owned();
        let slug = if key.is_empty() { "none" } else { key };
        let base = div()
            .id(SharedString::from(format!("drawer-role-{slug}-{}", host.id)))
            .test_support()
            .rounded_lg()
            .px(px(8.))
            .py(px(4.))
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let id = host_id.clone();
                let role = role_key.clone();
                let _ = this_role.update(cx, |w, cx| {
                    let val = if role.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(role)
                    };
                    w.patch_meta(&id, serde_json::json!({ "role": val }), cx);
                });
            })
            .child(label);
        role_row = role_row.child(if active {
            base.bg(palette.primary_bg).text_color(palette.primary_fg)
        } else {
            base.bg(palette.subtle_bg)
                .text_color(palette.text_secondary)
        });
    }

    // 标签列表 + 添加标签输入框
    let mut tag_chips = h_flex().flex_wrap().gap(px(6.));
    for tag in &host.tags {
        let this_tag = this.clone();
        let host_id = host.id.clone();
        let tag_value = tag.clone();
        tag_chips = tag_chips.child(
            h_flex()
                .items_center()
                .gap(px(4.))
                .rounded_md()
                .border_1()
                .border_color(palette.border)
                .bg(palette.muted_surface_bg)
                .px(px(8.))
                .py(px(2.))
                .text_size(px(11.))
                .text_color(palette.text_secondary)
                .child(format!("#{tag}"))
                .child(
                    div()
                        .id(SharedString::from(format!("tag-x-{tag}-{}", host.id)))
                        .test_support()
                        .text_color(palette.text_faint)
                        .cursor_pointer()
                        .hover(|s| s.text_color(palette.text_primary))
                        .on_click(move |_, _, cx| {
                            let id = host_id.clone();
                            let value = tag_value.clone();
                            let _ = this_tag.update(cx, |w, cx| w.remove_tag(&id, &value, cx));
                        })
                        .child("×"),
                ),
        );
    }

    let this_add_tag = this.clone();
    let add_tag_host_id = host.id.clone();
    let tag_section = v_flex()
        .pt(px(4.))
        .gap(px(6.))
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text_muted)
                .child("标签"),
        )
        .child(tag_chips)
        .child(
            h_flex()
                .items_center()
                .gap(px(6.))
                .child(div().flex_1().child(Input::new(&workspace.tag_input)))
                .child(
                    div()
                        .id("drawer-add-tag-btn")
                        .test_support()
                        .rounded_lg()
                        .border_1()
                        .border_color(palette.border)
                        .bg(palette.card_solid_bg)
                        .px(px(10.))
                        .py(px(5.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.text_secondary)
                        .cursor_pointer()
                        .hover(|s| s.bg(palette.subtle_bg))
                        .on_click(move |_, window, cx| {
                            let id = add_tag_host_id.clone();
                            let _ = this_add_tag
                                .update(cx, |w, cx| w.add_tag_from_input(&id, window, cx));
                        })
                        .child("添加"),
                ),
        );

    let mut shell = card_shell(palette)
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text_muted)
                        .child("角色分类"),
                )
                .child(if let Some(jump) = &host.jump_host_name {
                    badge(format!("跳板: {jump}"), palette).into_any_element()
                } else {
                    div().into_any_element()
                }),
        )
        .child(div().mt(px(8.)).child(role_row))
        .child(div().mt(px(10.)).child(tag_section));

    // 快捷链接
    if !host.links.is_empty() {
        let mut link_list = v_flex().mt(px(6.)).gap(px(4.));
        for (idx, link) in host.links.iter().enumerate() {
            let url = link.url.clone();
            link_list = link_list.child(
                h_flex()
                    .id(SharedString::from(format!("drawer-link-{idx}")))
                    .test_support()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .rounded_lg()
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.muted_surface_bg)
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(palette.text_secondary)
                    .cursor_pointer()
                    .hover(|s| s.bg(palette.card_solid_bg).text_color(palette.text_primary))
                    .on_click(move |_, _, cx| {
                        cx.open_url(&url);
                    })
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(link.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(palette.text_faint)
                            .truncate()
                            .child(link.url.clone()),
                    ),
            );
        }
        shell = shell.child(
            v_flex()
                .mt(px(10.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text_muted)
                        .child("快捷链接"),
                )
                .child(link_list),
        );
    }

    shell
}

fn badge(text: impl Into<SharedString>, palette: &Palette) -> impl IntoElement {
    div()
        .rounded_md()
        .bg(palette.subtle_bg)
        .px(px(6.))
        .py(px(1.))
        .text_size(px(10.))
        .text_color(palette.text_secondary)
        .child(text.into())
}
