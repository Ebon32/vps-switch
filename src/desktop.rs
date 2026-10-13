#![cfg(windows)]

use crate::{api::AppState, terminal::UiEvent};
use anyhow::Result;
use std::sync::Arc;
use tao::{
    dpi::{LogicalSize, Size},
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::{Icon as WindowIcon, WindowBuilder},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon as TrayIconImage, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
};
use wry::WebViewBuilder;

#[derive(Debug, Clone)]
pub enum DesktopUserEvent {
    TrayIcon(TrayIconEvent),
    TrayMenu(MenuEvent),
    ShowWindow,
}

const MENU_ID_SHOW: &str = "app:show";
const MENU_ID_QUIT: &str = "app:quit";
const MENU_HOST_PREFIX: &str = "host:";

/// 纯代码光栅化生成 64x64 高清抗锯齿 VPS Switch 图标（深色圆角矩形 + 白色 > 终端符 + 活力橙圆点）
pub fn render_brand_icon_rgba(size: u32) -> Vec<u8> {
    let mut buf = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;

    for y in 0..size {
        for x in 0..size {
            let fx = (x as f32 + 0.5) / s * 64.0;
            let fy = (y as f32 + 0.5) / s * 64.0;

            // 1. 圆角方块底 (64x64, rx=15)
            let bg_dist = sd_rounded_box(fx - 32.0, fy - 32.0, 29.0, 29.0, 14.0);
            if bg_dist > 1.0 {
                continue;
            }
            let bg_alpha = (0.5 - bg_dist).clamp(0.0, 1.0);

            let mut r = 24.0;
            let mut g = 24.0;
            let mut b = 27.0;

            // 2. 左侧白色终端折线 > : (16, 20) -> (28, 32) -> (16, 44)
            let d_seg1 = sd_segment(fx, fy, 16.0, 20.0, 27.5, 32.0) - 2.7;
            let d_seg2 = sd_segment(fx, fy, 27.5, 32.0, 16.0, 44.0) - 2.7;
            let d_chevron = d_seg1.min(d_seg2);
            if d_chevron < 0.8 {
                let w = (0.5 - d_chevron).clamp(0.0, 1.0);
                r = r * (1.0 - w) + 250.0 * w;
                g = g * (1.0 - w) + 250.0 * w;
                b = b * (1.0 - w) + 250.0 * w;
            }

            // 3. 右侧活力橙 Switch 节点圆 (cx=43, cy=32, r=6.8)
            let d_dot = ((fx - 43.0).powi(2) + (fy - 32.0).powi(2)).sqrt() - 6.8;
            if d_dot < 0.8 {
                let w = (0.5 - d_dot).clamp(0.0, 1.0);
                r = r * (1.0 - w) + 249.0 * w;
                g = g * (1.0 - w) + 115.0 * w;
                b = b * (1.0 - w) + 22.0 * w;
            }

            let idx = ((y * size + x) * 4) as usize;
            buf[idx] = r as u8;
            buf[idx + 1] = g as u8;
            buf[idx + 2] = b as u8;
            buf[idx + 3] = (bg_alpha * 255.0) as u8;
        }
    }
    buf
}

fn sd_rounded_box(px: f32, py: f32, bx: f32, by: f32, r: f32) -> f32 {
    let qx = px.abs() - bx + r;
    let qy = py.abs() - by + r;
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r
}

fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let h = ((pax * bax + pay * bay) / (bax * bax + bay * bay)).clamp(0.0, 1.0);
    ((pax - bax * h).powi(2) + (pay - bay * h).powi(2)).sqrt()
}

fn build_tray_menu(state: &Arc<AppState>) -> Result<Menu> {
    let menu = Menu::new();
    let show_item = MenuItem::with_id(MENU_ID_SHOW, "打开 VPS Switch 主界面", true, None);
    menu.append(&show_item)?;
    menu.append(&PredefinedMenuItem::separator())?;

    if let Ok(hosts) = state.db.list_public_hosts() {
        let mut active: Vec<_> = hosts.into_iter().filter(|h| !h.archived).collect();
        // 优先排：本机 -> 收藏 -> 置顶 -> 在线
        active.sort_by(|a, b| {
            (a.kind != "local")
                .cmp(&(b.kind != "local"))
                .then_with(|| b.favorited.cmp(&a.favorited))
                .then_with(|| b.pinned.cmp(&a.pinned))
                .then_with(|| (a.status != "online").cmp(&(b.status != "online")))
        });

        for h in active.into_iter().take(28) {
            let prefix = if h.favorited {
                "★ "
            } else if h.status == "online" {
                "● "
            } else {
                "○ "
            };
            let label = if h.kind == "local" {
                format!("{prefix}{} (PowerShell)", h.name)
            } else {
                format!("{prefix}{} ({})", h.name, h.addr)
            };
            let item = MenuItem::with_id(
                format!("{MENU_HOST_PREFIX}{}", h.id),
                label,
                true,
                None,
            );
            let _ = menu.append(&item);
        }
        menu.append(&PredefinedMenuItem::separator())?;
    }

    let quit_item = MenuItem::with_id(MENU_ID_QUIT, "退出 VPS Switch", true, None);
    menu.append(&quit_item)?;
    Ok(menu)
}

pub fn run_desktop_window(
    state: Arc<AppState>,
    rt_handle: tokio::runtime::Handle,
    port: u16,
) -> Result<()> {
    let event_loop = EventLoopBuilder::<DesktopUserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // 注册外部单实例唤醒回调（当用户第二次双击 vps-switch.exe 时直接唤醒当前窗口）
    {
        let proxy_show = proxy.clone();
        let mut rx = state.terminals.subscribe_ui_events();
        rt_handle.spawn(async move {
            while let Ok(ev) = rx.recv().await {
                match ev {
                    UiEvent::RevealTerminal { .. } | UiEvent::ShowMainWindow => {
                        let _ = proxy_show.send_event(DesktopUserEvent::ShowWindow);
                    }
                }
            }
        });
    }

    let rgba64 = render_brand_icon_rgba(64);
    let win_icon = WindowIcon::from_rgba(rgba64.clone(), 64, 64).ok();
    let tray_icon_img = TrayIconImage::from_rgba(rgba64, 64, 64)?;

    let window = WindowBuilder::new()
        .with_title("VPS Switch")
        .with_inner_size(Size::Logical(LogicalSize::new(1280.0, 800.0)))
        .with_min_inner_size(Size::Logical(LogicalSize::new(920.0, 600.0)))
        .with_window_icon(win_icon)
        .build(&event_loop)?;

    let url = format!("http://127.0.0.1:{port}");
    let _webview = WebViewBuilder::new()
        .with_url(&url)
        .build(&window)?;

    // 系统托盘事件转发到主事件循环
    let proxy_tray = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy_tray.send_event(DesktopUserEvent::TrayIcon(event));
    }));

    let proxy_menu = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy_menu.send_event(DesktopUserEvent::TrayMenu(event));
    }));

    let mut tray_icon = None;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {
                if let Ok(menu) = build_tray_menu(&state) {
                    tray_icon = TrayIconBuilder::new()
                        .with_menu(Box::new(menu))
                        .with_menu_on_left_click(false)
                        .with_tooltip("VPS Switch · 右键快速连接 VPS 终端")
                        .with_icon(tray_icon_img.clone())
                        .build()
                        .ok();
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // 点击右上角 × 最小化驻留到系统托盘，保持后台 MCP Server 与已打开的终端不断连
                window.set_visible(false);
            }
            Event::UserEvent(DesktopUserEvent::ShowWindow) => {
                window.set_visible(true);
                window.set_minimized(false);
                window.set_focus();
            }
            Event::UserEvent(DesktopUserEvent::TrayIcon(tray_ev)) => {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = tray_ev
                {
                    window.set_visible(true);
                    window.set_minimized(false);
                    window.set_focus();
                }
            }
            Event::UserEvent(DesktopUserEvent::TrayMenu(menu_ev)) => {
                let id_str = menu_ev.id.0.as_str();
                if id_str == MENU_ID_SHOW {
                    window.set_visible(true);
                    window.set_minimized(false);
                    window.set_focus();
                } else if id_str == MENU_ID_QUIT {
                    let _ = tray_icon.take();
                    *control_flow = ControlFlow::Exit;
                } else if let Some(host_id) = id_str.strip_prefix(MENU_HOST_PREFIX) {
                    // 托盘右键点选某台 VPS：立即唤醒主窗口并在界面上直接打开该主机的终端！
                    window.set_visible(true);
                    window.set_minimized(false);
                    window.set_focus();

                    let st = state.clone();
                    let hid = host_id.to_string();
                    rt_handle.spawn(async move {
                        if let Some(existing) = st.terminals.find_active_pane_for_host(&hid) {
                            st.terminals.reveal_pane_in_ui(&existing.id, None);
                        } else if let Ok(created) = st
                            .terminals
                            .create_pane(&st.ssh_pool, &st.db, &hid, 100, 30, None, false)
                            .await
                        {
                            st.terminals.reveal_pane_in_ui(&created.id, None);
                        }
                    });
                }
            }
            _ => {}
        }
    });
}
