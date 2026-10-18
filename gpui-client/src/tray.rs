//! 系统托盘常驻、托盘右键主机列表秒开终端、窗口隐藏/唤醒控制（对齐 `src/desktop.rs`）。

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use gpui_kit::Window;

use crate::api::Host;

pub const MENU_ID_SHOW: &str = "app:show";
pub const MENU_ID_QUIT: &str = "app:quit";
pub const MENU_HOST_PREFIX: &str = "host:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayAction {
    ShowMainWindow,
    OpenHostTerminal(String),
    Quit,
}

static TRAY_AVAILABLE: AtomicBool = AtomicBool::new(false);
static TRAY_SENDER: Mutex<Option<mpsc::Sender<TrayAction>>> = Mutex::new(None);
static TRAY_RECEIVER: Mutex<Option<mpsc::Receiver<TrayAction>>> = Mutex::new(None);

#[cfg(windows)]
thread_local! {
    static TRAY_INSTANCE: std::cell::RefCell<Option<tray_icon::TrayIcon>> = const { std::cell::RefCell::new(None) };
    static LAST_MENU_ENTRIES: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// 纯代码 SDF 光栅化生成 64x64 高清抗锯齿 VPS Switch 纯黑白双 L 滑块品牌图标。
pub fn render_brand_icon_rgba(size: u32) -> Vec<u8> {
    let mut buf = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;

    for y in 0..size {
        for x in 0..size {
            let fx = (x as f32 + 0.5) / s * 64.0;
            let fy = (y as f32 + 0.5) / s * 64.0;

            let bg_dist = sd_rounded_box(fx - 32.0, fy - 32.0, 29.0, 29.0, 14.0);
            if bg_dist > 1.0 {
                continue;
            }
            let bg_alpha = (0.5 - bg_dist).clamp(0.0, 1.0);

            let mut r = 0.0;
            let mut g = 0.0;
            let mut b = 0.0;

            if bg_dist > -1.2 && bg_dist < 0.2 {
                let border_w = (1.0 - (bg_dist + 0.5).abs()).clamp(0.0, 1.0) * 0.22;
                r += 255.0 * border_w;
                g += 255.0 * border_w;
                b += 255.0 * border_w;
            }

            let d_b1_horiz = sd_rounded_box(fx - 27.0, fy - 22.0, 11.0, 4.0, 1.5);
            let d_b1_vert = sd_rounded_box(fx - 20.0, fy - 29.0, 4.0, 11.0, 1.5);
            let d_b1 = d_b1_horiz.min(d_b1_vert);

            let d_b2_horiz = sd_rounded_box(fx - 37.0, fy - 42.0, 11.0, 4.0, 1.5);
            let d_b2_vert = sd_rounded_box(fx - 44.0, fy - 35.0, 4.0, 11.0, 1.5);
            let d_b2 = d_b2_horiz.min(d_b2_vert);

            let d_dot = ((fx - 32.0).powi(2) + (fy - 32.0).powi(2)).sqrt() - 3.5;

            let d_glyph = d_b1.min(d_b2).min(d_dot);
            if d_glyph < 0.8 {
                let w = (0.5 - d_glyph).clamp(0.0, 1.0);
                r = r * (1.0 - w) + 255.0 * w;
                g = g * (1.0 - w) + 255.0 * w;
                b = b * (1.0 - w) + 255.0 * w;
            }

            let idx = ((y * size + x) * 4) as usize;
            buf[idx] = r.min(255.0) as u8;
            buf[idx + 1] = g.min(255.0) as u8;
            buf[idx + 2] = b.min(255.0) as u8;
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

/// 根据当前主机列表生成托盘右键菜单的主机条目 `(menu_id, label)`。
/// 排序规则对齐 `src/desktop.rs`：本机 -> 收藏 -> 置顶 -> 在线，最多展示 28 台未归档主机。
pub fn build_host_menu_entries(hosts: &[Host]) -> Vec<(String, String)> {
    let mut active: Vec<&Host> = hosts.iter().filter(|h| !h.archived).collect();
    active.sort_by(|a, b| {
        (a.kind != "local")
            .cmp(&(b.kind != "local"))
            .then_with(|| b.favorited.cmp(&a.favorited))
            .then_with(|| b.pinned.cmp(&a.pinned))
            .then_with(|| (a.status != "online").cmp(&(b.status != "online")))
    });

    active
        .into_iter()
        .take(28)
        .map(|h| {
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
            (format!("{MENU_HOST_PREFIX}{}", h.id), label)
        })
        .collect()
}

/// 将托盘菜单 ID 解码为 `TrayAction`。
pub fn decode_menu_id(id: &str) -> Option<TrayAction> {
    if id == MENU_ID_SHOW {
        Some(TrayAction::ShowMainWindow)
    } else if id == MENU_ID_QUIT {
        Some(TrayAction::Quit)
    } else {
        id.strip_prefix(MENU_HOST_PREFIX)
            .filter(|s| !s.is_empty())
            .map(|hid| TrayAction::OpenHostTerminal(hid.to_owned()))
    }
}

/// 初始化系统托盘图标与事件回调（主进程启动时调用一次）。
pub fn init() {
    let (tx, rx) = mpsc::channel::<TrayAction>();
    if let Ok(mut slot) = TRAY_SENDER.lock() {
        *slot = Some(tx);
    }
    if let Ok(mut slot) = TRAY_RECEIVER.lock() {
        *slot = Some(rx);
    }

    #[cfg(windows)]
    {
        use tray_icon::{
            Icon as TrayIconImage, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
            menu::MenuEvent,
        };

        TrayIconEvent::set_event_handler(Some(|event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                emit_action(TrayAction::ShowMainWindow);
            }
        }));

        MenuEvent::set_event_handler(Some(|event: MenuEvent| {
            if let Some(action) = decode_menu_id(event.id.0.as_str()) {
                emit_action(action);
            }
        }));

        let rgba = render_brand_icon_rgba(64);
        if let Ok(icon) = TrayIconImage::from_rgba(rgba, 64, 64)
            && let Ok(menu) = build_native_menu(&[])
            && let Ok(tray) = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .with_tooltip("VPS Switch · 右键快速连接 VPS 终端")
                .with_icon(icon)
                .build()
        {
            TRAY_INSTANCE.with(|cell| {
                *cell.borrow_mut() = Some(tray);
            });
            TRAY_AVAILABLE.store(true, Ordering::Relaxed);
        }
    }
}

pub fn emit_action(action: TrayAction) {
    if let Ok(guard) = TRAY_SENDER.lock()
        && let Some(tx) = guard.as_ref()
    {
        let _ = tx.send(action);
    }
}

pub fn try_recv() -> Vec<TrayAction> {
    let Ok(guard) = TRAY_RECEIVER.lock() else {
        return Vec::new();
    };
    let Some(rx) = guard.as_ref() else {
        return Vec::new();
    };
    rx.try_iter().collect()
}

pub fn is_available() -> bool {
    TRAY_AVAILABLE.load(Ordering::Relaxed)
}

/// 当主机列表发生变化时，动态刷新系统托盘右键菜单。
pub fn update_hosts(hosts: &[Host]) {
    #[cfg(windows)]
    {
        let entries = build_host_menu_entries(hosts);
        let unchanged = LAST_MENU_ENTRIES.with(|cell| *cell.borrow() == entries);
        if unchanged {
            return;
        }
        if let Ok(menu) = build_native_menu(&entries) {
            TRAY_INSTANCE.with(|cell| {
                if let Some(tray) = cell.borrow_mut().as_mut() {
                    tray.set_menu(Some(Box::new(menu)));
                }
            });
            LAST_MENU_ENTRIES.with(|cell| {
                *cell.borrow_mut() = entries;
            });
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hosts;
    }
}

#[cfg(windows)]
fn build_native_menu(entries: &[(String, String)]) -> anyhow::Result<tray_icon::menu::Menu> {
    use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};

    let menu = Menu::new();
    let show_item = MenuItem::with_id(MENU_ID_SHOW, "打开 VPS Switch 主界面", true, None);
    menu.append(&show_item)?;
    menu.append(&PredefinedMenuItem::separator())?;

    if !entries.is_empty() {
        for (id, label) in entries {
            let item = MenuItem::with_id(id.clone(), label.clone(), true, None);
            let _ = menu.append(&item);
        }
        menu.append(&PredefinedMenuItem::separator())?;
    }

    let quit_item = MenuItem::with_id(MENU_ID_QUIT, "退出 VPS Switch", true, None);
    menu.append(&quit_item)?;
    Ok(menu)
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn ShowWindow(hwnd: isize, n_cmd_show: i32) -> i32;
    fn IsIconic(hwnd: isize) -> i32;
    fn SetForegroundWindow(hwnd: isize) -> i32;
}

#[cfg(windows)]
fn extract_hwnd(window: &Window) -> Option<isize> {
    let handle = raw_window_handle::HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        raw_window_handle::RawWindowHandle::Win32(win32) => Some(win32.hwnd.get()),
        _ => None,
    }
}

/// 将窗口隐藏到系统托盘（保留所有内存状态与终端连接）。
pub fn hide_window(window: &mut Window) {
    #[cfg(windows)]
    if let Some(hwnd) = extract_hwnd(window) {
        const SW_HIDE: i32 = 0;
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }
        return;
    }
    window.minimize_window();
}

/// 从系统托盘唤醒并前置激活窗口。
pub fn show_window(window: &mut Window) {
    #[cfg(windows)]
    if let Some(hwnd) = extract_hwnd(window) {
        const SW_SHOW: i32 = 5;
        const SW_RESTORE: i32 = 9;
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
    }
    window.activate_window();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_host(
        id: &str,
        name: &str,
        kind: &str,
        status: &str,
        favorited: bool,
        pinned: bool,
        archived: bool,
    ) -> Host {
        Host {
            id: id.into(),
            name: name.into(),
            addr: "198.51.100.1".into(),
            port: 22,
            username: "root".into(),
            kind: kind.into(),
            auth_type: "password".into(),
            has_password: false,
            has_private_key: false,
            has_passphrase: false,
            jump_host_id: None,
            jump_host_name: None,
            status: status.into(),
            os: None,
            pretty_name: None,
            distro_id: None,
            kernel: None,
            arch: None,
            cpu_cores: None,
            cpu_model: None,
            pinned,
            favorited,
            archived,
            role: None,
            tags: Vec::new(),
            links: Vec::new(),
            latency_ms: None,
            last_error: None,
        }
    }

    #[test]
    fn tray_menu_sorts_local_then_favorite_then_online_and_skips_archived() {
        let hosts = vec![
            sample_host("h-off", "离线节点", "ssh", "offline", false, false, false),
            sample_host("h-arch", "归档节点", "ssh", "online", true, true, true),
            sample_host("h-on", "在线节点", "ssh", "online", false, false, false),
            sample_host("h-fav", "收藏节点", "ssh", "offline", true, false, false),
            sample_host("local", "本机", "local", "online", false, false, false),
        ];

        let entries = build_host_menu_entries(&hosts);
        let ids: Vec<&str> = entries.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["host:local", "host:h-fav", "host:h-on", "host:h-off"]);
        assert_eq!(entries[0].1, "● 本机 (PowerShell)");
        assert_eq!(entries[1].1, "★ 收藏节点 (198.51.100.1)");
        assert_eq!(entries[2].1, "● 在线节点 (198.51.100.1)");
        assert_eq!(entries[3].1, "○ 离线节点 (198.51.100.1)");

        assert_eq!(decode_menu_id(MENU_ID_SHOW), Some(TrayAction::ShowMainWindow));
        assert_eq!(decode_menu_id(MENU_ID_QUIT), Some(TrayAction::Quit));
        assert_eq!(
            decode_menu_id("host:vip-hk"),
            Some(TrayAction::OpenHostTerminal("vip-hk".into()))
        );
    }

    #[test]
    fn brand_icon_rasterizer_produces_non_transparent_rgba64() {
        let rgba = render_brand_icon_rgba(64);
        assert_eq!(rgba.len(), 64 * 64 * 4);
        // 中心点 (32, 32) 是白色光核
        let center_idx = ((32 * 64 + 32) * 4) as usize;
        assert_eq!(rgba[center_idx], 255);
        assert_eq!(rgba[center_idx + 1], 255);
        assert_eq!(rgba[center_idx + 2], 255);
        assert_eq!(rgba[center_idx + 3], 255);
    }
}
