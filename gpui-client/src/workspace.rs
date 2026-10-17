//! 工作区：Dock 布局的宿主，持有左侧主机面板、中央终端标签与 API 事件轮询。

use std::time::Duration;

use gpui_kit::component::dock::{DockArea, DockLayout, DockPlacement, DockSkin, panel_handle};
use gpui_kit::*;

use crate::api::{Api, ApiEvent, HostSummary};
use crate::hosts::{HostList, HostListEvent};
use crate::terminal_panel::TerminalPanel;

pub struct Workspace {
    dock_area: Entity<DockArea>,
    host_list: Entity<HostList>,
    terminals: Vec<Entity<TerminalPanel>>,
    api: Api,
}

impl Workspace {
    pub fn new(api: Api, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let host_list = cx.new(HostList::new);
        let dock_area = cx.new(|cx| {
            DockArea::new("main", Some(1), window, cx).with_renderer(DockSkin::new(cx))
        });
        dock_area.update(cx, |area, cx| {
            area.set_dock(
                DockPlacement::Left,
                DockLayout::tabs().panel_view(panel_handle(host_list.clone()), cx),
                window,
                cx,
            );
            area.set_dock_size(DockPlacement::Left, px(280.), window, cx);
            area.set_dock_collapsible(DockPlacement::Left, true, window, cx);
        });
        // 焦点先落在主机面板：窗口级 action 需要一条焦点路径。
        let focus = host_list.read(cx).focus_handle(cx);
        window.focus(&focus, cx);

        api.load_hosts();
        let this = Self {
            dock_area,
            host_list: host_list.clone(),
            terminals: Vec::new(),
            api,
        };
        cx.subscribe_in(
            &host_list,
            window,
            |this, _, event: &HostListEvent, window, cx| match event {
                HostListEvent::Open(host) => this.open_terminal(host.clone(), window, cx),
            },
        )
        .detach();
        Self::poll_events(cx);
        this
    }

    /// 打开（或聚焦）一台主机的终端标签。
    fn open_terminal(&mut self, host: HostSummary, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .terminals
            .iter()
            .any(|panel| panel.read(cx).host_id() == host.id)
        {
            return;
        }
        let panel = cx.new(|cx| TerminalPanel::new(host, cx));
        let focus = panel.read(cx).focus_handle(cx);
        self.terminals.push(panel);
        self.rebuild_center(window, cx);
        window.focus(&focus, cx);
    }

    /// 中央区按当前标签集合重建（与 shell-rs 同样的做法：Dock 的布局树由
    /// 我们自己的 Vec 派生，关闭标签时同样走这里）。
    fn rebuild_center(&self, window: &mut Window, cx: &mut Context<Self>) {
        let mut center = DockLayout::tabs();
        for panel in &self.terminals {
            center = center.panel_view(panel_handle(panel.clone()), cx);
        }
        self.dock_area.update(cx, |area, cx| {
            area.set_center(center, window, cx);
        });
    }

    /// 每 100ms drain 一次网络事件。后台线程不直接碰 GPUI executor。
    fn poll_events(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let drained = this.update(cx, |this, cx| {
                    while let Some(event) = this.api.try_recv() {
                        match event {
                            ApiEvent::Hosts(hosts) => {
                                this.host_list.update(cx, |list, cx| list.apply_hosts(hosts, cx));
                            }
                            ApiEvent::Failed(error) => {
                                this.host_list.update(cx, |list, cx| list.apply_error(error, cx));
                            }
                        }
                    }
                });
                if drained.is_err() {
                    // 窗口已关，轮询随之结束。
                    break;
                }
            }
        })
        .detach();
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.dock_area.clone())
    }
}
