#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use gpui_kit::component::Root;
use gpui_kit::*;

use vps_switch_gpui::{api, assets::AppAssets, rendering, theme, tray, workspace::Workspace};

#[cfg(windows)]
fn acquire_single_instance_or_wakeup() -> Option<std::fs::File> {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::os::windows::fs::OpenOptionsExt;
    use std::time::Duration;

    let lock_path = std::env::temp_dir().join(format!("vps-switch-gpui-{}.lock", api::port()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(&lock_path)
    {
        Ok(file) => Some(file),
        Err(_) => {
            let port_num: u16 = api::port().parse().unwrap_or(18800);
            let addr = SocketAddr::from(([127, 0, 0, 1], port_num));
            if let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(200)) {
                let req = format!(
                    "POST /api/window/show HTTP/1.1\r\nHost: 127.0.0.1:{port_num}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(req.as_bytes());
                let mut buf = [0u8; 128];
                let _ = stream.read(&mut buf);
            }
            None
        }
    }
}

fn main() {
    #[cfg(windows)]
    let _instance_guard = match acquire_single_instance_or_wakeup() {
        Some(guard) => guard,
        None => return,
    };

    let start_minimized = std::env::args().any(|a| a == "--minimized" || a == "--hidden");

    gpui_kit::application()
        .with_assets(AppAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            tray::init();
            if !start_minimized {
                cx.activate(true);
            }

            let api = api::Api::spawn();
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1240.), px(780.)),
                    cx,
                ))),
                show: !start_minimized,
                ..WindowOptions::default()
            };
            cx.open_window(options, move |window, cx| {
                window.set_window_title("VPS Switch");
                rendering::note_renderer(window, cx);
                let workspace = cx.new(|cx| Workspace::new(api, window, cx));
                let ws_weak = workspace.downgrade();
                window.on_window_should_close(cx, move |window, cx| {
                    let close_action = ws_weak
                        .upgrade()
                        .map(|ws| ws.read(cx).settings.close_action.clone())
                        .unwrap_or_else(|| "tray".into());
                    if close_action == "exit" || !tray::is_available() {
                        cx.quit();
                        true
                    } else {
                        tray::hide_window(window);
                        false
                    }
                });
                cx.new(|cx| Root::new(workspace, window, cx))
            })
            .expect("failed to open window");
        });
}
