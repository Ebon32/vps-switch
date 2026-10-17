//! 终端会话：后台线程里创建 pane、连 WS、用 `alacritty_terminal` 解析字节流，
//! 产出一份可绘制的快照。
//!
//! 线程模型与 api.rs 相同：专用线程跑 current_thread tokio 运行时；UI 线程通过
//! 事件通道得知「快照有更新」，再从 `Arc<Mutex<Option<Snapshot>>>` 取数据——
//! 解析线程永远不碰 GPUI 的 executor。

use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, Copy)]
struct GridSize {
    cols: usize,
    lines: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// 单格快照。`extra.zerowidth` 这类组合字符暂不展开，宽字符占位格保留。
#[derive(Clone, Copy, Debug)]
pub struct CellSnapshot {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
}

impl Default for CellSnapshot {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Color::Named(NamedColor::Foreground),
            bg: Color::Named(NamedColor::Background),
            flags: Flags::empty(),
        }
    }
}

/// 一帧可绘制状态。
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<CellSnapshot>,
    /// 光标 (行, 列)，行是屏幕坐标。
    pub cursor: Option<(usize, usize)>,
    pub cursor_shape: CursorShape,
    pub display_offset: usize,
    pub history_size: usize,
    pub cursor_blinking: bool,
}

/// 网络/解析线程交给 UI 的事件。
pub enum SessionEvent {
    /// 快照有更新。
    Updated,
    /// 会话结束；附原因。
    Closed(String),
}

/// UI 交给会话的命令。
pub enum SessionCommand {
    Input(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    /// 向上（负）或向下（正）滚动历史。
    Scroll(i32),
}

pub struct Session {
    commands: async_channel::Sender<SessionCommand>,
    events: mpsc::Receiver<SessionEvent>,
    snapshot: Arc<Mutex<Option<Snapshot>>>,
}

impl Session {
    pub fn spawn(host_id: String, cols: u16, rows: u16) -> Self {
        let (command_tx, command_rx) = async_channel::unbounded::<SessionCommand>();
        let (event_tx, event_rx) = mpsc::channel::<SessionEvent>();
        let snapshot = Arc::new(Mutex::new(None));
        let worker_snapshot = snapshot.clone();
        thread::Builder::new()
            .name("vps-switch-terminal".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                runtime.block_on(async move {
                    let reason = match run(
                        host_id,
                        cols,
                        rows,
                        command_rx,
                        &event_tx,
                        &worker_snapshot,
                    )
                    .await
                    {
                        Ok(()) => "连接已断开".to_owned(),
                        Err(error) => format!("{error:#}"),
                    };
                    let _ = event_tx.send(SessionEvent::Closed(reason));
                });
            })
            .expect("terminal thread");
        Self {
            commands: command_tx,
            events: event_rx,
            snapshot,
        }
    }

    pub fn send(&self, command: SessionCommand) {
        let _ = self.commands.send_blocking(command);
    }

    pub fn try_recv(&self) -> Option<SessionEvent> {
        self.events.try_recv().ok()
    }

    pub fn snapshot(&self) -> Option<Snapshot> {
        self.snapshot.lock().ok().and_then(|guard| guard.clone())
    }
}

async fn run(
    host_id: String,
    cols: u16,
    rows: u16,
    commands: async_channel::Receiver<SessionCommand>,
    events: &mpsc::Sender<SessionEvent>,
    snapshot: &Arc<Mutex<Option<Snapshot>>>,
) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/terminals", crate::api::base_url()))
        .json(&serde_json::json!({"hostId": host_id, "cols": cols, "rows": rows}))
        .send()
        .await?;
    let status = response.status();
    let body: serde_json::Value = response.json().await?;
    if !status.is_success() {
        let reason = body
            .get("error")
            .and_then(|error| error.as_str())
            .unwrap_or("创建终端失败");
        anyhow::bail!("{reason}");
    }
    let pane_id = body
        .get("pane")
        .and_then(|pane| pane.get("id"))
        .and_then(|id| id.as_str())
        .ok_or_else(|| anyhow::anyhow!("守护进程未返回 pane id"))?
        .to_owned();

    let (socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://127.0.0.1:{}/api/ws/terminal/{pane_id}",
        crate::api::port()
    ))
    .await?;
    let (mut ws_tx, mut ws_rx) = socket.split();

    let mut size = GridSize {
        cols: cols as usize,
        lines: rows as usize,
    };
    let mut term = Term::new(TermConfig::default(), &size, VoidListener);
    let mut processor: Processor = Processor::new();
    store_snapshot(snapshot, &term, size);
    let _ = events.send(SessionEvent::Updated);

    loop {
        tokio::select! {
            message = ws_rx.next() => {
                match message {
                    Some(Ok(Message::Binary(data))) => {
                        // 0x02 是整份回放：重建现场（与浏览器端收到回放先 \x1bc 同理），
                        // 0x00 是实时输出；其余（无前缀）按原样喂。
                        let payload: &[u8] = match data.first() {
                            Some(0x02) => {
                                term = Term::new(TermConfig::default(), &size, VoidListener);
                                &data[1..]
                            }
                            Some(0x00) => &data[1..],
                            _ => &data[..],
                        };
                        if !payload.is_empty() {
                            processor.advance(&mut term, payload);
                        }
                        store_snapshot(snapshot, &term, size);
                        let _ = events.send(SessionEvent::Updated);
                    }
                    Some(Ok(Message::Text(_))) | Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                    _ => return Ok(()),
                }
            }
            command = commands.recv() => {
                match command {
                    Ok(SessionCommand::Input(bytes)) => {
                        let mut frame = Vec::with_capacity(1 + bytes.len());
                        frame.push(0x01);
                        frame.extend_from_slice(&bytes);
                        ws_tx.send(Message::Binary(frame.into())).await?;
                    }
                    Ok(SessionCommand::Resize { cols, rows }) => {
                        size = GridSize { cols: cols as usize, lines: rows as usize };
                        term.resize(size);
                        ws_tx
                            .send(Message::Text(
                                serde_json::json!({"type": "resize", "cols": cols, "rows": rows})
                                    .to_string()
                                    .into(),
                            ))
                            .await?;
                        store_snapshot(snapshot, &term, size);
                        let _ = events.send(SessionEvent::Updated);
                    }
                    Ok(SessionCommand::Scroll(delta)) => {
                        term.scroll_display(Scroll::Delta(delta));
                        store_snapshot(snapshot, &term, size);
                        let _ = events.send(SessionEvent::Updated);
                    }
                    Err(_) => return Ok(()),
                }
            }
        }
    }
}

fn store_snapshot(snapshot: &Arc<Mutex<Option<Snapshot>>>, term: &Term<VoidListener>, size: GridSize) {
    let content = term.renderable_content();
    let mut cells = vec![CellSnapshot::default(); size.cols * size.lines];
    for indexed in content.display_iter {
        let Point { line, column } = indexed.point;
        let row = line.0;
        if row < 0 || row as usize >= size.lines || column.0 >= size.cols {
            continue;
        }
        let cell = indexed.cell;
        cells[row as usize * size.cols + column.0] = CellSnapshot {
            c: cell.c,
            fg: cell.fg,
            bg: cell.bg,
            flags: cell.flags,
        };
    }
    let cursor_point = content.cursor.point;
    let cursor_row = cursor_point.line.0;
    let cursor = if cursor_row >= 0 && (cursor_row as usize) < size.lines {
        Some((cursor_row as usize, cursor_point.column.0))
    } else {
        None
    };
    let next = Snapshot {
        cols: size.cols,
        rows: size.lines,
        cells,
        cursor,
        cursor_shape: content.cursor.shape,
        display_offset: content.display_offset,
        history_size: term.grid().history_size(),
        cursor_blinking: true,
    };
    if let Ok(mut guard) = snapshot.lock() {
        *guard = Some(next);
    }
}
