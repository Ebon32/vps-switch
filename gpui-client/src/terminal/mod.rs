//! 板块二：终端会话（WS + `alacritty_terminal` 状态机 + 指数退避重连）与 GPUI 渲染视图。

pub mod session;
pub mod view;

pub use session::{PaneMeta, Session, SessionCommand, SessionEvent, Snapshot};
pub use view::TerminalView;
