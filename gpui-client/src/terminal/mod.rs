//! 终端：会话（WS + alacritty_terminal 解析）与 GPUI 渲染。

pub mod session;
pub mod view;

pub use session::{Session, SessionCommand, SessionEvent, Snapshot};
pub use view::TerminalView;
