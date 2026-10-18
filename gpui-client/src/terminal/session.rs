//! 终端会话：后台线程创建 pane、连接 WS、用 `alacritty_terminal` 解析字节流并维护快照。
//!
//! 严格对齐 `TerminalPaneView.vue` 的重连与协议约定：
//! - 二进制 `0x02`（历史字节环回放帧）：重建 `Term` + `Processor` 后写入（等价于 `\x1bc` 整屏重置）
//! - 二进制 `0x00`（实时输出帧）：增量写入 `Processor`
//! - 文本 `{"type": "meta", "pane": ...}`：同步 `cwd`（OSC 7）、`running`、`lastExitCode`（OSC 133）、`exited`
//! - 意外断线且未 `exited` 时：自动做最多 6 次指数退避重连（400ms * 2^n，上限 4000ms），支持手动立即重连
//! - 会话已结束（`exited`）时：支持 `ReconnectFresh` 原地创建新 pane 重新登录

use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Message;

pub const MAX_RECONNECT_ATTEMPTS: u32 = 6;

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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneMeta {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub host_id: String,
    #[serde(default)]
    pub cols: u16,
    #[serde(default)]
    pub rows: u16,
    pub cwd: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub running: bool,
    pub last_exit_code: Option<i32>,
    #[serde(default)]
    pub bracketed_paste: bool,
    #[serde(default)]
    pub headless: bool,
    #[serde(default)]
    pub exited: bool,
}

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

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<CellSnapshot>,
    pub cursor: Option<(usize, usize)>,
    pub cursor_shape: CursorShape,
    pub display_offset: usize,
    pub history_size: usize,
}

pub enum SessionEvent {
    Created { pane_id: String, meta: PaneMeta },
    CreateFailed(String),
    Updated,
    Meta(PaneMeta),
    Connected,
    Disconnected { attempt: u32, exhausted: bool },
}

pub enum SessionCommand {
    Input(Vec<u8>),
    Paste(String),
    Resize { cols: u16, rows: u16 },
    Scroll(i32),
    Clear,
    ManualReconnect,
    ReconnectFresh,
    Shutdown,
}

pub struct Session {
    commands: async_channel::Sender<SessionCommand>,
    events: mpsc::Receiver<SessionEvent>,
    snapshot: Arc<Mutex<Option<Snapshot>>>,
}

impl Session {
    pub fn spawn(host_id: String, cols: u16, rows: u16) -> Self {
        Self::spawn_inner(host_id, None, cols, rows)
    }

    pub fn attach(host_id: String, pane: PaneMeta, cols: u16, rows: u16) -> Self {
        Self::spawn_inner(host_id, Some(pane), cols, rows)
    }

    fn spawn_inner(
        host_id: String,
        initial_pane: Option<PaneMeta>,
        cols: u16,
        rows: u16,
    ) -> Self {
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
                runtime.block_on(worker_loop(
                    host_id,
                    initial_pane,
                    cols,
                    rows,
                    command_rx,
                    event_tx,
                    worker_snapshot,
                ));
            })
            .expect("terminal thread");
        Self {
            commands: command_tx,
            events: event_rx,
            snapshot,
        }
    }

    /// 无头测试专用：不连守护进程， directly 预填一份静态终端网格快照。
    pub fn mock_with_text(cols: u16, rows: u16, sample_line: &str) -> Self {
        let (command_tx, _command_rx) = async_channel::unbounded::<SessionCommand>();
        let (event_tx, event_rx) = mpsc::channel::<SessionEvent>();
        let mut cells = vec![CellSnapshot::default(); (cols as usize) * (rows as usize)];
        for (idx, ch) in sample_line.chars().take(cols as usize).enumerate() {
            cells[idx].c = ch;
        }
        let snap = Snapshot {
            cols: cols as usize,
            rows: rows as usize,
            cells,
            cursor: Some((0, sample_line.chars().count().min(cols as usize - 1))),
            cursor_shape: CursorShape::Block,
            display_offset: 0,
            history_size: 0,
        };
        let _ = event_tx.send(SessionEvent::Created {
            pane_id: "mock-pane-1".into(),
            meta: PaneMeta {
                id: "mock-pane-1".into(),
                host_id: "vip-hk".into(),
                cols,
                rows,
                cwd: Some("/root/app".into()),
                title: None,
                running: false,
                last_exit_code: Some(0),
                bracketed_paste: true,
                headless: false,
                exited: false,
            },
        });
        let _ = event_tx.send(SessionEvent::Connected);
        let _ = event_tx.send(SessionEvent::Updated);
        Self {
            commands: command_tx,
            events: event_rx,
            snapshot: Arc::new(Mutex::new(Some(snap))),
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

async fn worker_loop(
    host_id: String,
    mut initial_pane: Option<PaneMeta>,
    initial_cols: u16,
    initial_rows: u16,
    commands: async_channel::Receiver<SessionCommand>,
    events: mpsc::Sender<SessionEvent>,
    snapshot: Arc<Mutex<Option<Snapshot>>>,
) {
    let client = reqwest::Client::new();
    let mut cols = initial_cols;
    let mut rows = initial_rows;

    'session: loop {
        let meta = match initial_pane.take() {
            Some(existing) if !existing.id.is_empty() => existing,
            _ => {
                let create_fut = create_pane_http(&client, &host_id, cols, rows);
                tokio::pin!(create_fut);
                let create_res = loop {
                    tokio::select! {
                        res = &mut create_fut => break Some(res),
                        cmd = commands.recv() => {
                            match cmd {
                                Ok(SessionCommand::Resize { cols: c, rows: r }) => {
                                    cols = c;
                                    rows = r;
                                }
                                Ok(SessionCommand::ReconnectFresh) | Ok(SessionCommand::ManualReconnect) => {
                                    break None;
                                }
                                Ok(SessionCommand::Shutdown) | Err(_) => return,
                                _ => {}
                            }
                        }
                    }
                };
                let Some(create_res) = create_res else {
                    continue 'session;
                };
                match create_res {
                    Ok(meta) => meta,
                    Err(err) => {
                        let _ = events.send(SessionEvent::CreateFailed(err));
                        // 等待 UI 发 ReconnectFresh 或 Shutdown
                        while let Ok(cmd) = commands.recv().await {
                            match cmd {
                                SessionCommand::Resize { cols: c, rows: r } => {
                                    cols = c;
                                    rows = r;
                                }
                                SessionCommand::ReconnectFresh | SessionCommand::ManualReconnect => {
                                    continue 'session;
                                }
                                SessionCommand::Shutdown => return,
                                _ => {}
                            }
                        }
                        return;
                    }
                }
            }
        };

        let pane_id = meta.id.clone();
        if events
            .send(SessionEvent::Created {
                pane_id: pane_id.clone(),
                meta: meta.clone(),
            })
            .is_err()
        {
            return;
        }

        let mut size = GridSize {
            cols: cols as usize,
            lines: rows as usize,
        };
        let mut term = Term::new(TermConfig::default(), &size, VoidListener);
        let mut processor: Processor = Processor::new();
        store_snapshot(&snapshot, &term, size);
        let _ = events.send(SessionEvent::Updated);

        let mut reconnect_attempts: u32 = 0;
        let mut pane_exited = meta.exited;

        'ws_connect: loop {
            let ws_url = format!(
                "ws://127.0.0.1:{}/api/ws/terminal/{pane_id}",
                crate::api::port()
            );
            match tokio_tungstenite::connect_async(&ws_url).await {
                Ok((socket, _)) => {
                    reconnect_attempts = 0;
                    let _ = events.send(SessionEvent::Connected);
                    let (mut ws_tx, mut ws_rx) = socket.split();

                    // 连上后立即同步当前窗口尺寸
                    let _ = ws_tx
                        .send(Message::Text(
                            serde_json::json!({
                                "type": "resize",
                                "cols": cols,
                                "rows": rows,
                            })
                            .to_string()
                            .into(),
                        ))
                        .await;

                    loop {
                        tokio::select! {
                            message = ws_rx.next() => {
                                match message {
                                    Some(Ok(Message::Binary(data))) => {
                                        let payload: &[u8] = match data.first() {
                                            Some(0x02) => {
                                                // 整份历史字节环回放：先重置终端再写入，杜绝重复内容叠加
                                                term = Term::new(TermConfig::default(), &size, VoidListener);
                                                processor = Processor::new();
                                                &data[1..]
                                            }
                                            Some(0x00) => &data[1..],
                                            _ => &data[..],
                                        };
                                        if !payload.is_empty() {
                                            processor.advance(&mut term, payload);
                                        }
                                        store_snapshot(&snapshot, &term, size);
                                        let _ = events.send(SessionEvent::Updated);
                                    }
                                    Some(Ok(Message::Text(text))) => {
                                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text)
                                            && val.get("type").and_then(|v| v.as_str()) == Some("meta")
                                            && let Some(pane_val) = val.get("pane")
                                            && let Ok(meta) = serde_json::from_value::<PaneMeta>(pane_val.clone())
                                        {
                                            pane_exited = meta.exited;
                                            let _ = events.send(SessionEvent::Meta(meta));
                                        }
                                    }
                                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                                    _ => {
                                        // WS 断开
                                        break;
                                    }
                                }
                            }
                            command = commands.recv() => {
                                match command {
                                    Ok(SessionCommand::Input(bytes)) => {
                                        if is_auto_da_response(&bytes) {
                                            continue;
                                        }
                                        let mut frame = Vec::with_capacity(1 + bytes.len());
                                        frame.push(0x01);
                                        frame.extend_from_slice(&bytes);
                                        let _ = ws_tx.send(Message::Binary(frame.into())).await;
                                    }
                                    Ok(SessionCommand::Paste(text)) => {
                                        let _ = ws_tx
                                            .send(Message::Text(
                                                serde_json::json!({
                                                    "type": "paste",
                                                    "text": text,
                                                })
                                                .to_string()
                                                .into(),
                                            ))
                                            .await;
                                    }
                                    Ok(SessionCommand::Resize { cols: c, rows: r }) => {
                                        cols = c;
                                        rows = r;
                                        size = GridSize {
                                            cols: cols as usize,
                                            lines: rows as usize,
                                        };
                                        term.resize(size);
                                        let _ = ws_tx
                                            .send(Message::Text(
                                                serde_json::json!({
                                                    "type": "resize",
                                                    "cols": cols,
                                                    "rows": rows,
                                                })
                                                .to_string()
                                                .into(),
                                            ))
                                            .await;
                                        store_snapshot(&snapshot, &term, size);
                                        let _ = events.send(SessionEvent::Updated);
                                    }
                                    Ok(SessionCommand::Scroll(delta)) => {
                                        term.scroll_display(Scroll::Delta(delta));
                                        store_snapshot(&snapshot, &term, size);
                                        let _ = events.send(SessionEvent::Updated);
                                    }
                                    Ok(SessionCommand::Clear) => {
                                        processor.advance(&mut term, b"\x1b[2J\x1b[3J\x1b[H");
                                        store_snapshot(&snapshot, &term, size);
                                        let _ = events.send(SessionEvent::Updated);
                                        let _ = ws_tx
                                            .send(Message::Binary(vec![0x01, 0x0c].into()))
                                            .await;
                                    }
                                    Ok(SessionCommand::ManualReconnect) => {
                                        reconnect_attempts = 0;
                                        continue 'ws_connect;
                                    }
                                    Ok(SessionCommand::ReconnectFresh) => {
                                        reset_and_delete_pane(&client, &pane_id, &snapshot).await;
                                        continue 'session;
                                    }
                                    Ok(SessionCommand::Shutdown) | Err(_) => return,
                                }
                            }
                        }
                    }
                }
                Err(_) => {}
            }

            // 如果会话已经 exited，不再重连旧 WS，等待用户点击「重新连接」或关闭
            if pane_exited {
                while let Ok(cmd) = commands.recv().await {
                    match cmd {
                        SessionCommand::Resize { cols: c, rows: r } => {
                            cols = c;
                            rows = r;
                            size = GridSize {
                                cols: cols as usize,
                                lines: rows as usize,
                            };
                            term.resize(size);
                            store_snapshot(&snapshot, &term, size);
                            let _ = events.send(SessionEvent::Updated);
                        }
                        SessionCommand::Scroll(delta) => {
                            term.scroll_display(Scroll::Delta(delta));
                            store_snapshot(&snapshot, &term, size);
                            let _ = events.send(SessionEvent::Updated);
                        }
                        SessionCommand::ReconnectFresh => {
                            reset_and_delete_pane(&client, &pane_id, &snapshot).await;
                            continue 'session;
                        }
                        SessionCommand::ManualReconnect => {
                            reconnect_attempts = 0;
                            continue 'ws_connect;
                        }
                        SessionCommand::Shutdown => return,
                        _ => {}
                    }
                }
                return;
            }

            // 未 exited 而 WS 断开：按 400ms * 2^attempt（上限 4000ms）退避重连，最多 6 次
            if reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
                let _ = events.send(SessionEvent::Disconnected {
                    attempt: reconnect_attempts,
                    exhausted: true,
                });
                while let Ok(cmd) = commands.recv().await {
                    match cmd {
                        SessionCommand::ManualReconnect => {
                            reconnect_attempts = 0;
                            continue 'ws_connect;
                        }
                        SessionCommand::ReconnectFresh => {
                            reset_and_delete_pane(&client, &pane_id, &snapshot).await;
                            continue 'session;
                        }
                        SessionCommand::Resize { cols: c, rows: r } => {
                            cols = c;
                            rows = r;
                        }
                        SessionCommand::Shutdown => return,
                        _ => {}
                    }
                }
                return;
            }

            let delay_ms = (400u64 * 2u64.saturating_pow(reconnect_attempts)).min(4000);
            reconnect_attempts += 1;
            let _ = events.send(SessionEvent::Disconnected {
                attempt: reconnect_attempts,
                exhausted: false,
            });

            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(delay_ms)) => {
                    continue 'ws_connect;
                }
                cmd = commands.recv() => {
                    match cmd {
                        Ok(SessionCommand::ManualReconnect) => {
                            reconnect_attempts = 0;
                            continue 'ws_connect;
                        }
                        Ok(SessionCommand::ReconnectFresh) => {
                            reset_and_delete_pane(&client, &pane_id, &snapshot).await;
                            continue 'session;
                        }
                        Ok(SessionCommand::Resize { cols: c, rows: r }) => {
                            cols = c;
                            rows = r;
                        }
                        Ok(SessionCommand::Shutdown) | Err(_) => return,
                        _ => {}
                    }
                }
            }
        }
    }
}

async fn reset_and_delete_pane(
    client: &reqwest::Client,
    pane_id: &str,
    snapshot: &Arc<Mutex<Option<Snapshot>>>,
) {
    if let Ok(mut guard) = snapshot.lock() {
        *guard = None;
    }
    let _ = client
        .delete(format!("{}/api/terminals/{pane_id}", crate::api::base_url()))
        .timeout(Duration::from_secs(3))
        .send()
        .await;
}

async fn create_pane_http(
    client: &reqwest::Client,
    host_id: &str,
    cols: u16,
    rows: u16,
) -> Result<PaneMeta, String> {
    let response = client
        .post(format!("{}/api/terminals", crate::api::base_url()))
        .json(&serde_json::json!({
            "hostId": host_id,
            "cols": cols,
            "rows": rows,
        }))
        .send()
        .await
        .map_err(|err| format!("连接守护进程失败: {err}"))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|err| format!("解析终端创建响应失败: {err}"))?;
    let ok = body.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
    if !status.is_success() || !ok {
        let reason = body
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("创建终端失败");
        return Err(reason.to_owned());
    }
    let pane_val = body
        .get("pane")
        .ok_or_else(|| "守护进程未返回 pane 对象".to_owned())?;
    serde_json::from_value::<PaneMeta>(pane_val.clone())
        .map_err(|err| format!("解析 pane 元数据失败: {err}"))
}

fn is_auto_da_response(bytes: &[u8]) -> bool {
    // 拦截 \x1b[?1;2c 等设备属性应答
    bytes.len() >= 4
        && bytes.starts_with(b"\x1b[")
        && matches!(bytes[2], b'?' | b'>' | b'=')
        && bytes.last() == Some(&b'c')
        && bytes[3..bytes.len() - 1]
            .iter()
            .all(|b| b.is_ascii_digit() || *b == b';')
}

fn store_snapshot(
    snapshot: &Arc<Mutex<Option<Snapshot>>>,
    term: &Term<VoidListener>,
    size: GridSize,
) {
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
    };
    if let Ok(mut guard) = snapshot.lock() {
        *guard = Some(next);
    }
}
