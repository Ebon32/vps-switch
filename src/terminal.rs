use crate::{
    db::Database,
    models::LOCAL_HOST_ID,
    ssh::{open_host_channel, SshPool},
};
use alacritty_terminal::{
    event::VoidListener,
    grid::Dimensions,
    index::{Column, Line, Point},
    term::{Config as TermConfig, Term, TermMode},
    vte::ansi::Processor,
};
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use russh::ChannelMsg;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

const RING_BUFFER_CAPACITY: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy)]
struct TermDimensions {
    cols: usize,
    lines: usize,
}

impl Dimensions for TermDimensions {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneMeta {
    pub id: String,
    pub host_id: String,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub running: bool,
    pub last_exit_code: Option<i32>,
    pub bracketed_paste: bool,
    pub headless: bool,
    pub exited: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenSnapshot {
    pub pane_id: String,
    pub host_id: String,
    pub state: String, // "idle" | "waiting_input" | "running" | "exited"
    pub cursor_line: String,
    pub screen: String,
    pub cwd: Option<String>,
    pub last_exit_code: Option<i32>,
    pub headless: bool,
    pub exited: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UiEvent {
    RevealTerminal {
        host_id: String,
        pane: PaneMeta,
        reason: Option<String>,
    },
    ShowMainWindow,
}

#[derive(Clone)]
pub enum PaneBroadcast {
    Output(Vec<u8>),
    Meta(PaneMeta),
}

pub enum PaneCommand {
    Input(Vec<u8>),
    Paste(String),
    Resize(u16, u16),
    Close,
}

pub struct PaneInternalState {
    pub term: Term<VoidListener>,
    pub processor: Processor,
    pub ring_buffer: VecDeque<u8>,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub running: bool,
    pub last_exit_code: Option<i32>,
    pub headless: bool,
    pub exited: bool,
    pub output_seq: u64,
    pub last_output_at: Instant,
    osc_tail: Vec<u8>,
}

impl PaneInternalState {
    fn new(cols: u16, rows: u16, initial_cwd: Option<String>, headless: bool) -> Self {
        let dims = TermDimensions {
            cols: cols.max(2) as usize,
            lines: rows.max(1) as usize,
        };
        let term = Term::new(TermConfig::default(), &dims, VoidListener);
        Self {
            term,
            processor: Processor::new(),
            ring_buffer: VecDeque::with_capacity(64 * 1024),
            cols,
            rows,
            cwd: initial_cwd,
            title: None,
            running: false,
            last_exit_code: None,
            headless,
            exited: false,
            output_seq: 0,
            last_output_at: Instant::now(),
            osc_tail: Vec::new(),
        }
    }

    pub fn feed_bytes(&mut self, data: &[u8]) -> bool {
        self.processor.advance(&mut self.term, data);
        self.output_seq = self.output_seq.wrapping_add(1);
        self.last_output_at = Instant::now();

        let clean_for_replay = strip_terminal_queries(data);
        if clean_for_replay.len() >= RING_BUFFER_CAPACITY {
            self.ring_buffer.clear();
            let start = clean_for_replay.len() - RING_BUFFER_CAPACITY;
            self.ring_buffer.extend(&clean_for_replay[start..]);
        } else if !clean_for_replay.is_empty() {
            let overflow = (self.ring_buffer.len() + clean_for_replay.len())
                .saturating_sub(RING_BUFFER_CAPACITY);
            if overflow > 0 {
                self.ring_buffer.drain(..overflow);
            }
            self.ring_buffer.extend(&clean_for_replay);
        }

        self.scan_osc_sequences(data)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols.max(2);
        self.rows = rows.max(1);
        let dims = TermDimensions {
            cols: self.cols as usize,
            lines: self.rows as usize,
        };
        self.term.resize(dims);
    }

    pub fn bracketed_paste_enabled(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// 直接从 alacritty_terminal::Term 二维屏幕网格读取最后 max_lines 行与光标所在行（零 ANSI 噪音）
    pub fn read_screen_grid(&self, max_lines: usize) -> (String, String) {
        let bottom = self.term.bottommost_line();
        let top = self.term.topmost_line();
        let last_col = self.term.last_column();
        let cursor_pt = self.term.grid().cursor.point;

        let cursor_line = self
            .term
            .bounds_to_string(
                Point::new(cursor_pt.line, Column(0)),
                Point::new(cursor_pt.line, last_col),
            )
            .trim_end()
            .to_string();

        let requested = max_lines.clamp(5, 200) as i32;
        let start_line = std::cmp::max(top, Line(bottom.0 - requested + 1));
        let raw_screen = self.term.bounds_to_string(
            Point::new(start_line, Column(0)),
            Point::new(bottom, last_col),
        );

        let lines: Vec<&str> = raw_screen.lines().collect();
        let mut last_non_empty = lines.len();
        while last_non_empty > 0 && lines[last_non_empty - 1].trim().is_empty() {
            last_non_empty -= 1;
        }
        let trimmed_screen = lines[..last_non_empty].join("\n");
        (trimmed_screen, cursor_line)
    }

    fn scan_osc_sequences(&mut self, chunk: &[u8]) -> bool {
        let mut changed = false;
        self.osc_tail.extend_from_slice(chunk);
        if self.osc_tail.len() > 4096 {
            let keep = 1024;
            let drain_len = self.osc_tail.len() - keep;
            self.osc_tail.drain(..drain_len);
        }

        let mut idx = 0;
        let mut consumed = 0;
        while idx + 2 <= self.osc_tail.len() {
            if self.osc_tail[idx] == 0x1b && self.osc_tail[idx + 1] == b']' {
                let start = idx + 2;
                let mut end = None;
                let mut next_pos = start;
                while next_pos < self.osc_tail.len() {
                    if self.osc_tail[next_pos] == 0x07 {
                        end = Some((next_pos, next_pos + 1));
                        break;
                    }
                    if next_pos + 1 < self.osc_tail.len()
                        && self.osc_tail[next_pos] == 0x1b
                        && self.osc_tail[next_pos + 1] == b'\\'
                    {
                        end = Some((next_pos, next_pos + 2));
                        break;
                    }
                    next_pos += 1;
                }

                if let Some((payload_end, after_end)) = end {
                    let osc_owned = std::str::from_utf8(&self.osc_tail[start..payload_end])
                        .ok()
                        .map(str::to_owned);
                    if let Some(osc_str) = osc_owned {
                        if self.handle_osc_payload(&osc_str) {
                            changed = true;
                        }
                    }
                    consumed = after_end;
                    idx = after_end;
                } else {
                    break;
                }
            } else {
                idx += 1;
            }
        }

        if consumed > 0 {
            self.osc_tail.drain(..consumed);
        } else if let Some(esc_pos) = self.osc_tail.iter().rposition(|&b| b == 0x1b) {
            if esc_pos > 0 {
                self.osc_tail.drain(..esc_pos);
            }
        } else {
            self.osc_tail.clear();
        }

        changed
    }

    fn handle_osc_payload(&mut self, osc: &str) -> bool {
        if let Some(rest) = osc.strip_prefix("7;") {
            if let Some(path_part) = rest.strip_prefix("file://") {
                if let Some(slash_idx) = path_part.find('/') {
                    let raw_path = &path_part[slash_idx..];
                    let decoded = percent_decode(raw_path);
                    let clean = if decoded.len() >= 3
                        && decoded.starts_with('/')
                        && decoded.as_bytes()[2] == b':'
                    {
                        decoded[1..].to_string()
                    } else {
                        decoded
                    };
                    if !clean.is_empty() && self.cwd.as_deref() != Some(&clean) {
                        self.cwd = Some(clean);
                        return true;
                    }
                }
            }
        }

        if let Some(rest) = osc.strip_prefix("133;") {
            if rest.starts_with('A') {
                if self.running {
                    self.running = false;
                    return true;
                }
            } else if rest.starts_with('C') {
                if !self.running {
                    self.running = true;
                    return true;
                }
            } else if let Some(done_rest) = rest.strip_prefix('D') {
                let code = done_rest
                    .strip_prefix(';')
                    .and_then(|s| s.split(';').next())
                    .and_then(|s| s.trim().parse::<i32>().ok());
                let prev_running = self.running;
                let prev_code = self.last_exit_code;
                self.running = false;
                if code.is_some() {
                    self.last_exit_code = code;
                }
                return prev_running || prev_code != self.last_exit_code;
            }
        }

        if let Some(title) = osc.strip_prefix("0;").or_else(|| osc.strip_prefix("2;")) {
            let trimmed = title.trim();
            if !trimmed.is_empty() {
                let mut changed = false;
                if self.title.as_deref() != Some(trimmed) {
                    self.title = Some(trimmed.to_string());
                    changed = true;
                }
                if let Some((_, dir_part)) = trimmed.split_once(": ") {
                    let dir = dir_part.trim();
                    if (dir.starts_with('/') || dir.starts_with('~'))
                        && self.cwd.as_deref() != Some(dir)
                    {
                        self.cwd = Some(dir.to_string());
                        changed = true;
                    }
                }
                if self.running {
                    self.running = false;
                    changed = true;
                }
                return changed;
            }
        }

        false
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(((h << 4) | l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn strip_terminal_queries(data: &[u8]) -> Vec<u8> {
    const QUERIES: &[&[u8]] = &[
        b"\x1b[c",
        b"\x1b[0c",
        b"\x1b[>c",
        b"\x1b[>0c",
        b"\x1b[=c",
        b"\x1b[=0c",
        b"\x1b[5n",
        b"\x1b[6n",
        b"\x1b[?6n",
    ];
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    'outer: while i < data.len() {
        if data[i] == 0x1b {
            for q in QUERIES {
                if data[i..].starts_with(q) {
                    i += q.len();
                    continue 'outer;
                }
            }
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

pub fn looks_like_interactive_prompt(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    let patterns = [
        "[y/n]",
        "[yes/no]",
        "(y/n)",
        "(yes/no",
        "continue?",
        "proceed?",
        "password:",
        "password for",
        "passphrase",
        "enter ",
        "press enter",
        "overwrite",
        "are you sure",
        "请确认",
        "请输入",
    ];
    if patterns.iter().any(|p| lower.contains(p)) {
        return true;
    }
    (lower.ends_with('?') || lower.ends_with(':')) && !looks_like_shell_prompt(line)
}

pub fn looks_like_shell_prompt(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    if t.starts_with("PS ") && t.ends_with('>') {
        return true;
    }
    if (t.contains('@') || t.starts_with('[') || t.contains(":~") || t.contains(":/"))
        && (t.ends_with('#') || t.ends_with('$') || t.ends_with('%'))
    {
        return true;
    }
    t.ends_with(":~#") || t.ends_with(":~$") || t == "#" || t == "$"
}

pub struct PaneHandle {
    pub id: String,
    pub host_id: String,
    pub state: Arc<Mutex<PaneInternalState>>,
    pub cmd_tx: mpsc::UnboundedSender<PaneCommand>,
    pub broadcast_tx: broadcast::Sender<PaneBroadcast>,
}

impl PaneHandle {
    pub fn snapshot_meta(&self) -> PaneMeta {
        let guard = self.state.lock().unwrap();
        PaneMeta {
            id: self.id.clone(),
            host_id: self.host_id.clone(),
            cols: guard.cols,
            rows: guard.rows,
            cwd: guard.cwd.clone(),
            title: guard.title.clone(),
            running: guard.running,
            last_exit_code: guard.last_exit_code,
            bracketed_paste: guard.bracketed_paste_enabled(),
            headless: guard.headless,
            exited: guard.exited,
        }
    }

    pub fn replay_bytes(&self) -> Vec<u8> {
        let guard = self.state.lock().unwrap();
        let (a, b) = guard.ring_buffer.as_slices();
        let mut out = Vec::with_capacity(a.len() + b.len());
        out.extend_from_slice(a);
        out.extend_from_slice(b);
        out
    }

    pub fn read_screen_snapshot(&self, lines: usize) -> ScreenSnapshot {
        let guard = self.state.lock().unwrap();
        let (screen, cursor_line) = guard.read_screen_grid(lines);
        let state_str = if guard.exited {
            "exited"
        } else if looks_like_interactive_prompt(&cursor_line) {
            "waiting_input"
        } else if looks_like_shell_prompt(&cursor_line) || !guard.running {
            "idle"
        } else {
            "running"
        };
        ScreenSnapshot {
            pane_id: self.id.clone(),
            host_id: self.host_id.clone(),
            state: state_str.to_string(),
            cursor_line,
            screen,
            cwd: guard.cwd.clone(),
            last_exit_code: guard.last_exit_code,
            headless: guard.headless,
            exited: guard.exited,
        }
    }

    /// 向 PTY 发送输入并智能等待：
    /// - 若命令执行完毕回到提示符，立即返回 `idle`；
    /// - 若脚本中途停在 `[Y/n]` / `yes/no` / `password:` 等交互提问超过 600ms，立即返回 `waiting_input`（绝不卡死！）。
    pub async fn send_and_wait(
        &self,
        bytes: Vec<u8>,
        max_lines: usize,
        wait_ms: u64,
    ) -> ScreenSnapshot {
        let seq_before = self.state.lock().unwrap().output_seq;
        let _ = self.cmd_tx.send(PaneCommand::Input(bytes));

        let deadline = Instant::now() + Duration::from_millis(wait_ms.clamp(200, 60_000));
        loop {
            tokio::time::sleep(Duration::from_millis(80)).await;
            let (exited, seq_now, quiet_ms, running, cursor_line) = {
                let g = self.state.lock().unwrap();
                let (_, cl) = g.read_screen_grid(5);
                (
                    g.exited,
                    g.output_seq,
                    g.last_output_at.elapsed().as_millis() as u64,
                    g.running,
                    cl,
                )
            };

            if exited {
                break;
            }
            if seq_now > seq_before {
                if looks_like_interactive_prompt(&cursor_line) && quiet_ms >= 550 {
                    break;
                }
                if (!running || looks_like_shell_prompt(&cursor_line)) && quiet_ms >= 350 {
                    break;
                }
            }
            if Instant::now() >= deadline {
                break;
            }
        }

        self.read_screen_snapshot(max_lines)
    }
}

pub struct TerminalManager {
    panes: Mutex<HashMap<String, Arc<PaneHandle>>>,
    ui_events_tx: broadcast::Sender<UiEvent>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        let (ui_events_tx, _) = broadcast::channel(128);
        Self {
            panes: Mutex::new(HashMap::new()),
            ui_events_tx,
        }
    }

    pub fn subscribe_ui_events(&self) -> broadcast::Receiver<UiEvent> {
        self.ui_events_tx.subscribe()
    }

    pub fn notify_show_main_window(&self) {
        let _ = self.ui_events_tx.send(UiEvent::ShowMainWindow);
    }

    pub fn list_panes(&self, host_id_filter: Option<&str>, include_headless: bool) -> Vec<PaneMeta> {
        let guard = self.panes.lock().unwrap();
        guard
            .values()
            .filter(|p| host_id_filter.map(|h| p.host_id == h).unwrap_or(true))
            .map(|p| p.snapshot_meta())
            .filter(|m| include_headless || !m.headless)
            .collect()
    }

    pub fn get_pane(&self, pane_id: &str) -> Option<Arc<PaneHandle>> {
        let guard = self.panes.lock().unwrap();
        guard.get(pane_id).cloned()
    }

    pub fn find_active_pane_for_host(&self, host_id: &str) -> Option<Arc<PaneHandle>> {
        let guard = self.panes.lock().unwrap();
        guard
            .values()
            .find(|p| p.host_id == host_id && !p.state.lock().unwrap().exited)
            .cloned()
    }

    /// 将一个无头终端（或已存在的终端）唤起到前端界面，供用户亲自输入隐私密码或手动操作
    pub fn reveal_pane_in_ui(&self, pane_id: &str, reason: Option<String>) -> Option<PaneMeta> {
        let pane = self.get_pane(pane_id)?;
        {
            let mut g = pane.state.lock().unwrap();
            g.headless = false;
        }
        let meta = pane.snapshot_meta();
        let _ = self.ui_events_tx.send(UiEvent::RevealTerminal {
            host_id: pane.host_id.clone(),
            pane: meta.clone(),
            reason,
        });
        Some(meta)
    }

    pub fn close_pane(&self, pane_id: &str) -> bool {
        let removed = {
            let mut guard = self.panes.lock().unwrap();
            guard.remove(pane_id)
        };
        if let Some(pane) = removed {
            let _ = pane.cmd_tx.send(PaneCommand::Close);
            true
        } else {
            false
        }
    }

    pub async fn create_pane(
        &self,
        pool: &SshPool,
        db: &Database,
        host_id: &str,
        cols: u16,
        rows: u16,
        initial_cwd: Option<String>,
        headless: bool,
    ) -> Result<PaneMeta> {
        let cols = cols.clamp(20, 500);
        let rows = rows.clamp(5, 200);
        let pane_id = format!("pane_{}", Uuid::new_v4());

        let handle = if host_id == LOCAL_HOST_ID {
            spawn_local_conpty_pane(
                pane_id.clone(),
                host_id.to_string(),
                cols,
                rows,
                initial_cwd,
                headless,
            )?
        } else {
            spawn_ssh_pty_pane(
                pool,
                db,
                pane_id.clone(),
                host_id.to_string(),
                cols,
                rows,
                initial_cwd,
                headless,
            )
            .await?
        };

        let meta = handle.snapshot_meta();
        {
            let mut guard = self.panes.lock().unwrap();
            guard.insert(pane_id, handle);
        }
        Ok(meta)
    }
}

fn build_meta_from_guard(pane_id: &str, host_id: &str, guard: &PaneInternalState) -> PaneMeta {
    PaneMeta {
        id: pane_id.to_string(),
        host_id: host_id.to_string(),
        cols: guard.cols,
        rows: guard.rows,
        cwd: guard.cwd.clone(),
        title: guard.title.clone(),
        running: guard.running,
        last_exit_code: guard.last_exit_code,
        bracketed_paste: guard.bracketed_paste_enabled(),
        headless: guard.headless,
        exited: guard.exited,
    }
}

fn spawn_local_conpty_pane(
    pane_id: String,
    host_id: String,
    cols: u16,
    rows: u16,
    initial_cwd: Option<String>,
    headless: bool,
) -> Result<Arc<PaneHandle>> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("创建本机 ConPTY 失败")?;

    let mut cmd = CommandBuilder::new("powershell.exe");
    let init_hook = r#"$global:__vps_orig_prompt = $function:prompt; function global:prompt { $ec = if ($?) { 0 } else { if ($LASTEXITCODE) { $LASTEXITCODE } else { 1 } }; $p = ($executionContext.SessionState.Path.CurrentLocation.Path -replace '\\','/'); [Console]::Write("$([char]27)]133;D;$ec$([char]7)$([char]27)]7;file://localhost/$p$([char]7)$([char]27)]133;A$([char]7)"); & $global:__vps_orig_prompt }"#;
    cmd.args(["-NoLogo", "-NoExit", "-Command", init_hook]);
    cmd.env("TERM", "xterm-256color");
    cmd.env("TERM_PROGRAM", "vps-switch");

    if let Some(ref cwd) = initial_cwd {
        cmd.cwd(cwd);
    } else if let Ok(home) = std::env::var("USERPROFILE") {
        cmd.cwd(home);
    }

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .context("启动本机 PowerShell 失败")?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .context("克隆 PTY reader 失败")?;
    let mut writer = pair.master.take_writer().context("获取 PTY writer 失败")?;
    let master = pair.master;

    let state = Arc::new(Mutex::new(PaneInternalState::new(
        cols,
        rows,
        initial_cwd,
        headless,
    )));
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<PaneCommand>();
    let (broadcast_tx, _) = broadcast::channel::<PaneBroadcast>(512);

    let handle = Arc::new(PaneHandle {
        id: pane_id.clone(),
        host_id: host_id.clone(),
        state: state.clone(),
        cmd_tx,
        broadcast_tx: broadcast_tx.clone(),
    });

    let state_read = state.clone();
    let bcast_read = broadcast_tx.clone();
    let pid_read = pane_id.clone();
    let hid_read = host_id.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    let meta_opt = {
                        let mut guard = state_read.lock().unwrap();
                        let changed = guard.feed_bytes(&chunk);
                        if changed {
                            Some(build_meta_from_guard(&pid_read, &hid_read, &guard))
                        } else {
                            None
                        }
                    };
                    let _ = bcast_read.send(PaneBroadcast::Output(chunk));
                    if let Some(m) = meta_opt {
                        let _ = bcast_read.send(PaneBroadcast::Meta(m));
                    }
                }
            }
        }

        let final_meta = {
            let mut guard = state_read.lock().unwrap();
            guard.exited = true;
            guard.running = false;
            build_meta_from_guard(&pid_read, &hid_read, &guard)
        };
        let _ = bcast_read.send(PaneBroadcast::Meta(final_meta));
    });

    let state_cmd = state.clone();
    let bcast_cmd = broadcast_tx.clone();
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                PaneCommand::Input(bytes) => {
                    if bytes.contains(&b'\r') || bytes.contains(&b'\n') {
                        let mut guard = state_cmd.lock().unwrap();
                        guard.running = true;
                    }
                    let _ = writer.write_all(&bytes);
                    let _ = writer.flush();
                }
                PaneCommand::Paste(text) => {
                    let payload = encode_safe_paste(&state_cmd, &text);
                    let _ = writer.write_all(&payload);
                    let _ = writer.flush();
                }
                PaneCommand::Resize(c, r) => {
                    let meta = {
                        let mut guard = state_cmd.lock().unwrap();
                        guard.resize(c, r);
                        build_meta_from_guard(&pane_id, &host_id, &guard)
                    };
                    let _ = master.resize(PtySize {
                        rows: r.max(1),
                        cols: c.max(2),
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                    let _ = bcast_cmd.send(PaneBroadcast::Meta(meta));
                }
                PaneCommand::Close => {
                    let _ = child.kill();
                    break;
                }
            }
        }
    });

    Ok(handle)
}

async fn spawn_ssh_pty_pane(
    pool: &SshPool,
    db: &Database,
    pane_id: String,
    host_id: String,
    cols: u16,
    rows: u16,
    initial_cwd: Option<String>,
    headless: bool,
) -> Result<Arc<PaneHandle>> {
    let (_record, mut channel) = open_host_channel(pool, db, &host_id).await?;

    channel
        .request_pty(
            false,
            "xterm-256color",
            cols as u32,
            rows as u32,
            0,
            0,
            &[],
        )
        .await
        .context("请求 SSH PTY 失败")?;

    channel
        .request_shell(true)
        .await
        .context("启动远端 Shell 失败")?;

    let state = Arc::new(Mutex::new(PaneInternalState::new(
        cols,
        rows,
        initial_cwd.or_else(|| Some("~".to_string())),
        headless,
    )));
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<PaneCommand>();
    let (broadcast_tx, _) = broadcast::channel::<PaneBroadcast>(512);

    let handle = Arc::new(PaneHandle {
        id: pane_id.clone(),
        host_id: host_id.clone(),
        state: state.clone(),
        cmd_tx,
        broadcast_tx: broadcast_tx.clone(),
    });

    let state_loop = state.clone();
    let bcast_loop = broadcast_tx.clone();
    let pid_loop = pane_id.clone();
    let hid_loop = host_id.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg_opt = channel.wait() => {
                    match msg_opt {
                        Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                            let chunk = data.to_vec();
                            let meta_opt = {
                                let mut guard = state_loop.lock().unwrap();
                                let changed = guard.feed_bytes(&chunk);
                                if changed {
                                    Some(build_meta_from_guard(&pid_loop, &hid_loop, &guard))
                                } else {
                                    None
                                }
                            };
                            let _ = bcast_loop.send(PaneBroadcast::Output(chunk));
                            if let Some(m) = meta_opt {
                                let _ = bcast_loop.send(PaneBroadcast::Meta(m));
                            }
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            let mut guard = state_loop.lock().unwrap();
                            guard.last_exit_code = Some(exit_status as i32);
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                            break;
                        }
                        _ => {}
                    }
                }
                cmd_opt = cmd_rx.recv() => {
                    match cmd_opt {
                        Some(PaneCommand::Input(bytes)) => {
                            if bytes.contains(&b'\r') || bytes.contains(&b'\n') {
                                let mut guard = state_loop.lock().unwrap();
                                guard.running = true;
                            }
                            if channel.data(&bytes[..]).await.is_err() {
                                break;
                            }
                        }
                        Some(PaneCommand::Paste(text)) => {
                            let payload = encode_safe_paste(&state_loop, &text);
                            if channel.data(&payload[..]).await.is_err() {
                                break;
                            }
                        }
                        Some(PaneCommand::Resize(c, r)) => {
                            let meta = {
                                let mut guard = state_loop.lock().unwrap();
                                guard.resize(c, r);
                                build_meta_from_guard(&pid_loop, &hid_loop, &guard)
                            };
                            let _ = channel.window_change(c.max(2) as u32, r.max(1) as u32, 0, 0).await;
                            let _ = bcast_loop.send(PaneBroadcast::Meta(meta));
                        }
                        Some(PaneCommand::Close) | None => {
                            let _ = channel.close().await;
                            break;
                        }
                    }
                }
            }
        }

        let final_meta = {
            let mut guard = state_loop.lock().unwrap();
            guard.exited = true;
            guard.running = false;
            build_meta_from_guard(&pid_loop, &hid_loop, &guard)
        };
        let _ = bcast_loop.send(PaneBroadcast::Meta(final_meta));
    });

    Ok(handle)
}

fn encode_safe_paste(state: &Arc<Mutex<PaneInternalState>>, text: &str) -> Vec<u8> {
    let bracketed = state.lock().unwrap().bracketed_paste_enabled();
    let sanitized: String = text
        .chars()
        .filter(|&c| c != '\x1b' && c != '\0')
        .collect();
    if bracketed {
        let mut out = Vec::with_capacity(sanitized.len() + 12);
        out.extend_from_slice(b"\x1b[200~");
        out.extend_from_slice(sanitized.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        sanitized.into_bytes()
    }
}
