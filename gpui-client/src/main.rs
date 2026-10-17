#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! vps-switch 的 GPUI 桌面客户端。
//!
//! 只做一件事：把守护进程（`vps-switch.exe`，axum + SQLite + SSH + MCP）的
//! HTTP/WS API 渲染成原生界面。所有业务在守护进程里，这里没有第二份实现。

use gpui_kit::component::Root;
use gpui_kit::*;

use vps_switch_gpui::{api, workspace};

fn main() {
    gpui_kit::application().run(|cx| {
        // 必须先于任何组件调用。
        gpui_kit::init(cx);
        cx.activate(true);
        let api = api::Api::spawn();
        gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            window.set_window_title("VPS Switch");
            let workspace = cx.new(|cx| workspace::Workspace::new(api, window, cx));
            cx.new(|cx| Root::new(workspace, window, cx))
        })
        .expect("failed to open window");
    });
}
