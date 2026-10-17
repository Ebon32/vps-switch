//! 工作区：Dock 布局的宿主，持有左侧主机面板与 API 事件轮询。

use std::time::Duration;

use gpui_kit::component::dock::{
    DockArea, DockLayout, DockPlacement, DockSkin, panel_handle,
};
use gpui_kit::*;

use crate::api::{Api, ApiEvent};
use crate::hosts::HostList;

pub struct Workspace {
    dock_area: Entity<DockArea>,
    host_list: Entity<HostList>,
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
            host_list,
            api,
        };
        Self::poll_events(cx);
        this
    }

    /// 每 100ms drain 一次网络事件。后台线程不直接碰 GPUI executor。
    fn poll_events(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let drained = this.update(cx, |this, cx| {
                    let mut changed = false;
                    while let Some(event) = this.api.try_recv() {
                        changed = true;
                        match event {
                            ApiEvent::Hosts(hosts) => {
                                this.host_list.update(cx, |list, cx| list.apply_hosts(hosts, cx));
                            }
                            ApiEvent::Failed(error) => {
                                this.host_list.update(cx, |list, cx| list.apply_error(error, cx));
                            }
                        }
                    }
                    changed
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
