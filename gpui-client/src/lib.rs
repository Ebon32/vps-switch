//! vps-switch 的 GPUI 桌面客户端库。
//!
//! 二进制入口在 `main.rs`；模块在这里公开，让 `tests/` 能驱动真实面板。

pub mod api;
pub mod hosts;
pub mod terminal_panel;
pub mod workspace;
