//! 左侧主机面板：Dock 里的一个面板，列出守护进程返回的主机。
//! 单击选中；双击（或单击已选中项）让工作区在中央区打开终端标签。

use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::*;
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::*;

use crate::api::HostSummary;

/// 面板向工作区发出的事件。
#[derive(Clone, Debug)]
pub enum HostListEvent {
    /// 用户要在这台主机上打开终端。
    Open(HostSummary),
}

pub struct HostList {
    focus_handle: FocusHandle,
    hosts: Vec<HostSummary>,
    /// 正在显示的错误（例如守护进程没起来）；None 表示还没加载过。
    error: Option<String>,
    loaded: bool,
    selected: Option<String>,
}

impl HostList {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            hosts: Vec::new(),
            error: None,
            loaded: false,
            selected: None,
        }
    }

    pub fn apply_hosts(&mut self, hosts: Vec<HostSummary>, cx: &mut Context<Self>) {
        self.hosts = hosts;
        self.error = None;
        self.loaded = true;
        cx.notify();
    }

    pub fn apply_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }

    fn render_host(
        &self,
        host: &HostSummary,
        this: &Entity<Self>,
        cx: &App,
    ) -> impl IntoElement {
        let online = host.last_error.is_none() && host.latency_ms.is_some();
        let dot_color = if online {
            cx.theme().success
        } else if host.last_error.is_some() {
            cx.theme().danger
        } else {
            cx.theme().muted_foreground
        };
        let detail = if host.kind == "local" {
            format!("本机 · {}", host.pretty_name.as_deref().unwrap_or(""))
        } else {
            host.addr.clone()
        };
        let selected = self.selected.as_deref() == Some(host.id.as_str());
        let host_click = host.clone();
        let this_click = this.clone();
        let mut row = h_flex()
            .id(SharedString::from(format!("host-{}", host.id)))
            .px_3()
            .py_2()
            .gap_2()
            .w_full()
            .rounded_md()
            .hover(|style| style.bg(cx.theme().accent))
            .on_click(move |event, _window, cx| {
                let host = host_click.clone();
                let open = event.click_count() >= 2;
                let _ = this_click.update(cx, |this, cx| {
                    let already = this.selected.as_deref() == Some(host.id.as_str());
                    this.selected = Some(host.id.clone());
                    cx.notify();
                    if open || already {
                        cx.emit(HostListEvent::Open(host.clone()));
                    }
                });
            })
            .child(div().size(px(8.)).rounded_full().bg(dot_color).flex_shrink_0())
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_sm().truncate().child(host.name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(detail),
                    ),
            );
        if selected {
            row = row.bg(cx.theme().accent);
        }
        if host.pinned {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("置顶"),
            );
        }
        // 注册给无头测试的元素查询（生产构建里是恒等）。
        row.test_support()
    }
}

impl Focusable for HostList {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for HostList {}
impl EventEmitter<HostListEvent> for HostList {}

impl BasePanel for HostList {
    fn panel_name(&self) -> &'static str {
        "hosts"
    }
}

impl Panel for HostList {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("主机".into())
    }
}

impl Render for HostList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let body = if let Some(error) = &self.error {
            v_flex()
                .p_3()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error.clone()),
                )
                .into_any_element()
        } else if !self.loaded {
            v_flex()
                .p_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("正在连接守护进程…"),
                )
                .into_any_element()
        } else {
            v_flex()
                .p_2()
                .gap_0p5()
                .children(
                    self.hosts
                        .iter()
                        .map(|host| self.render_host(host, &this, cx)),
                )
                .into_any_element()
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("VPS Switch"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} 台", self.hosts.len())),
                    ),
            )
            .child(
                div()
                    .id("host-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
    }
}
