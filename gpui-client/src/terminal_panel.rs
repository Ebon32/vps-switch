//! 中央区的终端标签页。M1 是占位：显示主机信息；M2 接入 alacritty_terminal
//! 渲染与字节 WS。

use gpui_kit::component::*;
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::*;

use crate::api::HostSummary;

pub struct TerminalPanel {
    focus_handle: FocusHandle,
    host: HostSummary,
}

impl TerminalPanel {
    pub fn new(host: HostSummary, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            host,
        }
    }

    pub fn host_id(&self) -> &str {
        &self.host.id
    }
}

impl Focusable for TerminalPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for TerminalPanel {}

impl BasePanel for TerminalPanel {
    fn panel_name(&self) -> &'static str {
        "terminal"
    }
}

impl Panel for TerminalPanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some(SharedString::from(self.host.name.clone()))
    }
}

impl Render for TerminalPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let os = self
            .host
            .pretty_name
            .clone()
            .or_else(|| self.host.os.clone())
            .unwrap_or_else(|| "未知系统".into());
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(self.host.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} · {}", self.host.addr, os)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("终端将在 M2 接入"),
                    ),
            )
    }
}
