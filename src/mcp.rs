use crate::{
    api::AppState,
    db::Database,
    files,
    guard::Verdict,
    models::{HostRecord, LOCAL_HOST_ID},
    ssh::open_host_channel,
    terminal::looks_like_explicit_interactive_prompt,
};
use anyhow::{anyhow, Result};
use chrono::Utc;
use futures::stream::{self, StreamExt};
use russh::ChannelMsg;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{io::AsyncBufReadExt, sync::oneshot};
use uuid::Uuid;

/// 按名称、别名、IP 或 ID 解析主机（彻底告别逼 Agent 查 UUID 编号的痛点）
pub fn resolve_host(db: &Database, query: &str) -> Result<HostRecord> {
    let q = query.trim();
    if q.is_empty() {
        return Err(anyhow!("host 参数不能为空"));
    }
    let records = db.list_records()?;
    let lower = q.to_lowercase();

    let check_exposed = |rec: &HostRecord| -> Result<HostRecord> {
        if !rec.mcp_exposed {
            return Err(anyhow!(
                "主机「{}」未在 MCP 中暴露（可在「设置 → MCP Server」中勾选开启）",
                rec.name
            ));
        }
        Ok(rec.clone())
    };

    if lower == "local" || lower == "本机" || lower == "localhost" || lower == "127.0.0.1" {
        if let Some(loc) = records.iter().find(|r| r.id == LOCAL_HOST_ID) {
            return check_exposed(loc);
        }
    }

    // 1. 精确匹配 name（大小写不敏感）或 id
    if let Some(exact) = records
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(q) || r.id.eq_ignore_ascii_case(q))
    {
        return check_exposed(exact);
    }

    // 2. 精确匹配 IP / 域名
    if let Some(by_addr) = records.iter().find(|r| r.addr.eq_ignore_ascii_case(q)) {
        return check_exposed(by_addr);
    }

    // 3. 唯一子串匹配 name（优先在已暴露主机中匹配）
    let partial_exposed: Vec<_> = records
        .iter()
        .filter(|r| !r.archived && r.mcp_exposed && r.name.to_lowercase().contains(&lower))
        .collect();
    if partial_exposed.len() == 1 {
        return Ok(partial_exposed[0].clone());
    }
    if partial_exposed.is_empty() {
        let partial_hidden: Vec<_> = records
            .iter()
            .filter(|r| !r.mcp_exposed && r.name.to_lowercase().contains(&lower))
            .collect();
        if partial_hidden.len() == 1 {
            return check_exposed(partial_hidden[0]);
        }
    }

    let available: Vec<String> = records
        .iter()
        .filter(|r| !r.archived && r.mcp_exposed)
        .map(|r| r.name.clone())
        .collect();
    Err(anyhow!(
        "未找到主机「{}」。可用主机名: {}",
        q,
        available.join(", ")
    ))
}

// ==================== 后台长耗时命令管理器 (ExecTaskManager) ====================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecTaskSnapshot {
    pub id: String,
    pub host_id: String,
    pub host_name: String,
    pub command: String,
    pub state: String, // "running" | "completed" | "failed" | "interactive_stall" | "cancelled"
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub hint: Option<String>,
    pub started_at: String,
}

struct ExecTaskInner {
    id: String,
    host_id: String,
    host_name: String,
    command: String,
    state: String,
    exit_code: Option<i32>,
    started_inst: Instant,
    ended_ms: Option<u64>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    last_output_inst: Instant,
    hint: Option<String>,
    started_at: String,
    cancel_tx: Option<oneshot::Sender<()>>,
}

impl ExecTaskInner {
    fn snapshot(&self) -> ExecTaskSnapshot {
        let dur = self
            .ended_ms
            .unwrap_or_else(|| self.started_inst.elapsed().as_millis() as u64);
        ExecTaskSnapshot {
            id: self.id.clone(),
            host_id: self.host_id.clone(),
            host_name: self.host_name.clone(),
            command: self.command.clone(),
            state: self.state.clone(),
            exit_code: self.exit_code,
            duration_ms: dur,
            stdout: String::from_utf8_lossy(&self.stdout).to_string(),
            stderr: String::from_utf8_lossy(&self.stderr).to_string(),
            stdout_truncated: self.stdout.len() > MAX_OUTPUT_BYTES,
            stderr_truncated: self.stderr.len() > MAX_OUTPUT_BYTES,
            hint: self.hint.clone(),
            started_at: self.started_at.clone(),
        }
    }
}

#[derive(Default)]
pub struct ExecTaskManager {
    tasks: Mutex<HashMap<String, Arc<Mutex<ExecTaskInner>>>>,
}

/// 单条任务 stdout / stderr 各自的上限，超出即截断并显式标注（避免常驻进程被大输出撑爆内存）
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
/// 任务表保留上限，超出后按开始时间淘汰最旧的已完成任务
const MAX_TASKS: usize = 60;
/// 任务表里 stdout + stderr 缓冲的合计上限。
/// 单任务最多 2 MiB，60 个全在跑就是 120 MiB——而"一次 fan-out 到整支机队"正是这个工具的
/// 主打用法。所以条数之外再压一道字节预算，超了就淘汰最旧的**已结束**任务。
const MAX_TOTAL_BUFFER_BYTES: usize = 32 * 1024 * 1024;

fn append_capped(buf: &mut Vec<u8>, chunk: &[u8]) {
    if buf.len() > MAX_OUTPUT_BYTES || chunk.is_empty() {
        return;
    }
    let room = MAX_OUTPUT_BYTES - buf.len();
    if chunk.len() <= room {
        buf.extend_from_slice(chunk);
    } else {
        buf.extend_from_slice(&chunk[..room]);
        buf.extend_from_slice("\n...[输出超过 1 MiB，已截断]".as_bytes());
    }
}

impl ExecTaskManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 淘汰最旧的已完成任务，保证长驻进程的任务表不会无界增长。
    /// 两个上限任一超标就淘汰，且**只淘汰已结束的任务**——跑着的绝不动，
    /// 所以极端情况下（几十个任务同时还在跑）字节数仍可能短暂超预算，这是刻意的取舍：
    /// 宁可短暂占多一点，也不掐掉正在执行的任务。
    fn evict_old_finished(&self, map: &mut HashMap<String, Arc<Mutex<ExecTaskInner>>>) {
        let mut total_bytes = 0usize;
        let mut finished: Vec<(Instant, String, usize)> = Vec::new();
        for (id, task) in map.iter() {
            let guard = task.lock().unwrap();
            let bytes = guard.stdout.len() + guard.stderr.len();
            total_bytes += bytes;
            if guard.state != "running" {
                finished.push((guard.started_inst, id.clone(), bytes));
            }
        }
        if map.len() <= MAX_TASKS && total_bytes <= MAX_TOTAL_BUFFER_BYTES {
            return;
        }
        finished.sort_by(|a, b| a.0.cmp(&b.0));
        let mut removed = 0usize;
        for (_, id, bytes) in finished {
            if map.len() - removed <= MAX_TASKS && total_bytes <= MAX_TOTAL_BUFFER_BYTES {
                break;
            }
            map.remove(&id);
            removed += 1;
            total_bytes = total_bytes.saturating_sub(bytes);
        }
    }

    pub fn get_task(&self, task_id: &str) -> Option<ExecTaskSnapshot> {
        let map = self.tasks.lock().unwrap();
        map.get(task_id).map(|t| t.lock().unwrap().snapshot())
    }

    pub fn list_tasks(&self) -> Vec<ExecTaskSnapshot> {
        let map = self.tasks.lock().unwrap();
        map.values().map(|t| t.lock().unwrap().snapshot()).collect()
    }

    pub async fn cancel_task(&self, task_id: &str) -> Result<ExecTaskSnapshot> {
        let arc = self
            .tasks
            .lock()
            .unwrap()
            .get(task_id)
            .cloned()
            .ok_or_else(|| anyhow!("未找到可取消的命令任务: {task_id}"))?;
        {
            let mut g = arc.lock().unwrap();
            if g.state != "running" {
                return Ok(g.snapshot());
            }
            if let Some(tx) = g.cancel_tx.take() {
                let _ = tx.send(());
            }
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            {
                let g = arc.lock().unwrap();
                if g.state != "running" {
                    return Ok(g.snapshot());
                }
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "取消请求已发送，但尚未确认命令停止（taskId: {task_id}）"
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub async fn exec_with_wait(
        &self,
        state: &Arc<AppState>,
        host: &HostRecord,
        command: &str,
        wait_seconds: u64,
    ) -> Result<ExecTaskSnapshot> {
        let task_id = format!("exec_{}", Uuid::new_v4());
        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();

        let inner = Arc::new(Mutex::new(ExecTaskInner {
            id: task_id.clone(),
            host_id: host.id.clone(),
            host_name: host.name.clone(),
            command: command.to_string(),
            state: "running".to_string(),
            exit_code: None,
            started_inst: Instant::now(),
            ended_ms: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            last_output_inst: Instant::now(),
            hint: None,
            started_at: Utc::now().to_rfc3339(),
            cancel_tx: Some(cancel_tx),
        }));

        {
            let mut map = self.tasks.lock().unwrap();
            map.insert(task_id.clone(), inner.clone());
            self.evict_old_finished(&mut map);
        }

        let st = state.clone();
        let host_clone = host.clone();
        let cmd_str = command.to_string();
        let inner_bg = inner.clone();

        tokio::spawn(async move {
            if host_clone.id == LOCAL_HOST_ID {
                run_local_exec_task(inner_bg, &cmd_str, &mut cancel_rx).await;
            } else {
                run_ssh_exec_task(&st, &host_clone, inner_bg, &cmd_str, &mut cancel_rx).await;
            }
        });

        let wait_dur = Duration::from_secs(wait_seconds.clamp(1, 120));
        let deadline = Instant::now() + wait_dur;

        loop {
            tokio::time::sleep(Duration::from_millis(70)).await;
            {
                let mut g = inner.lock().unwrap();
                if g.state != "running" {
                    return Ok(g.snapshot());
                }

                // 交互式提问早退保护：只在命中**明确关键词**（[Y/n] / password: / proceed? …）时才判定。
                // 早先这里用宽松版（"以 ? / : 结尾"也算），会把 `echo "Processing:"` 这类正常命令
                // 在静默 2 秒后直接掐断并误报成 interactive_stall（已实测复现）。
                if g.last_output_inst.elapsed() >= Duration::from_millis(2000)
                    && (!g.stdout.is_empty() || !g.stderr.is_empty())
                {
                    let combined = format!(
                        "{}\n{}",
                        String::from_utf8_lossy(&g.stdout),
                        String::from_utf8_lossy(&g.stderr)
                    );
                    if let Some(last_line) = combined.lines().rev().find(|l| !l.trim().is_empty()) {
                        if looks_like_explicit_interactive_prompt(last_line) {
                            if let Some(tx) = g.cancel_tx.take() {
                                let _ = tx.send(());
                            }
                            g.state = "interactive_stall".to_string();
                            g.ended_ms = Some(g.started_inst.elapsed().as_millis() as u64);
                            g.hint = Some(format!(
                                "检测到命令停在交互式提问: 「{}」。非交互通道无法输入；请加 -y / DEBIAN_FRONTEND=noninteractive 参数，或改用 vps_terminal 工具（默认无头自动回复 y，遇到隐私密码可设 reveal_ui: true 让用户输入）。",
                                last_line.trim()
                            ));
                            return Ok(g.snapshot());
                        }
                    }
                }
            }

            if Instant::now() >= deadline {
                let mut g = inner.lock().unwrap();
                if g.state == "running" {
                    // 只交代现状与"一次等待"的入口，绝不暗示反复查询：旧文案（"请使用 vps_task 查询后续输出"）
                    // 会让 agent 陷入 vps_task 轮询死循环（vps_task 无阻塞模式、返回里也没有终止信号）。
                    g.hint = Some(format!(
                        "命令在 {} 秒内未结束，仍在后台运行（taskId: {}，ok=false 表示未完成、并非失败）。需要结果时调用一次 vps_task(action:\"get\", task_id:\"{}\", wait_seconds:N) 等待其结束即可；不需要结果可直接忽略，不要反复查询。",
                        wait_seconds, g.id, g.id
                    ));
                }
                return Ok(g.snapshot());
            }
        }
    }
}

async fn run_local_exec_task(
    inner: Arc<Mutex<ExecTaskInner>>,
    cmd: &str,
    cancel_rx: &mut oneshot::Receiver<()>,
) {
    use tokio::io::AsyncReadExt;
    #[cfg(windows)]
    let mut child = {
        let encoded = windows_ps_encoded_payload(cmd);
        let mut ps_cmd = tokio::process::Command::new("powershell.exe");
        ps_cmd
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand",
                encoded.as_str(),
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        ps_cmd.creation_flags(0x08000000);
        match ps_cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let mut g = inner.lock().unwrap();
                g.state = "failed".to_string();
                g.stderr
                    .extend_from_slice(format!("启动本机 PowerShell 失败: {e}").as_bytes());
                g.ended_ms = Some(g.started_inst.elapsed().as_millis() as u64);
                return;
            }
        }
    };

    #[cfg(not(windows))]
    let mut child = {
        let mut sh_cmd = tokio::process::Command::new("sh");
        sh_cmd
            .args(["-c", cmd])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        match sh_cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let mut g = inner.lock().unwrap();
                g.state = "failed".to_string();
                g.stderr
                    .extend_from_slice(format!("启动本地 Shell 失败: {e}").as_bytes());
                g.ended_ms = Some(g.started_inst.elapsed().as_millis() as u64);
                return;
            }
        }
    };

    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let mut out_buf = [0u8; 4096];
    let mut err_buf = [0u8; 4096];
    let mut out_eof = false;
    let mut err_eof = false;

    loop {
        if out_eof && err_eof {
            break;
        }
        tokio::select! {
            _ = &mut *cancel_rx => {
                let killed = child.kill().await;
                finish_exec_cancellation(&inner, killed.err().map(|e| format!("终止本机进程失败: {e}")));
                return;
            }
            res = stdout.read(&mut out_buf), if !out_eof => {
                match res {
                    Ok(0) | Err(_) => out_eof = true,
                    Ok(n) => {
                        let mut g = inner.lock().unwrap();
                        append_capped(&mut g.stdout, &out_buf[..n]);
                        g.last_output_inst = Instant::now();
                    }
                }
            }
            res = stderr.read(&mut err_buf), if !err_eof => {
                match res {
                    Ok(0) | Err(_) => err_eof = true,
                    Ok(n) => {
                        let mut g = inner.lock().unwrap();
                        append_capped(&mut g.stderr, &err_buf[..n]);
                        g.last_output_inst = Instant::now();
                    }
                }
            }
        }
    }

    let code = child.wait().await.ok().and_then(|s| s.code()).unwrap_or(-1);
    let mut g = inner.lock().unwrap();
    // 命令已结束：清掉超时期间写下的"仍在后台运行"提示，否则完成态还挂着过期 hint
    g.hint = None;
    g.exit_code = Some(code);
    g.state = if code == 0 {
        "completed".to_string()
    } else {
        "failed".to_string()
    };
    g.ended_ms = Some(g.started_inst.elapsed().as_millis() as u64);
}

/// 把任意 PowerShell 脚本编码为 `-EncodedCommand` 所需的 UTF-16LE base64。
/// 本机与远端两条执行路径共用：统一注入 UTF-8 输出编码（否则中文按 GBK 落管道变乱码）、
/// 抑制进度条噪音，并把真实退出码原样带出。
fn windows_ps_encoded_payload(command: &str) -> String {
    let payload = format!(
        "$ProgressPreference = 'SilentlyContinue'\n\
         try {{ [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 }} catch {{}}\n\
         $global:LASTEXITCODE = $null\n\
         {}\n\
         $__vpsOk = $?\n\
         if ($null -ne $LASTEXITCODE -and $LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }}\n\
         if (-not $__vpsOk) {{ exit 1 }}\n\
         exit 0",
        command.trim()
    );

    let utf16_bytes: Vec<u8> = payload
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(&utf16_bytes)
}

fn wrap_windows_remote_command(cmd: &str) -> String {
    let trimmed = cmd.trim();
    if trimmed.starts_with("powershell") || trimmed.starts_with("cmd.exe") {
        return cmd.to_string();
    }

    format!(
        "powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {}",
        windows_ps_encoded_payload(cmd)
    )
}

fn clean_clixml_stderr(raw: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(raw);
    if !text.contains("<Objs") && !text.contains("#< CLIXML") {
        return raw.to_vec();
    }
    let mut result = String::new();
    let mut cursor = 0;
    while let Some(start_tag) = text[cursor..].find("<S") {
        let tag_start = cursor + start_tag;
        if let Some(close_bracket) = text[tag_start..].find('>') {
            let content_start = tag_start + close_bracket + 1;
            if let Some(end_tag) = text[content_start..].find("</S>") {
                let content = &text[content_start..content_start + end_tag];
                let unescaped = content
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
                    .replace("&apos;", "'")
                    .replace("&amp;", "&")
                    .replace("_x000D__x000A_", "\n")
                    .replace("_x000D_", "\r")
                    .replace("_x000A_", "\n");
                result.push_str(&unescaped);
                cursor = content_start + end_tag + 4;
                continue;
            }
        }
        cursor += 2;
    }
    if result.is_empty() {
        text.replace("#< CLIXML", "").into_bytes()
    } else {
        result.into_bytes()
    }
}

async fn run_ssh_exec_task(
    state: &Arc<AppState>,
    host: &HostRecord,
    inner: Arc<Mutex<ExecTaskInner>>,
    cmd: &str,
    cancel_rx: &mut oneshot::Receiver<()>,
) {
    // 并发执行闸：permit 持有到本任务结束，保证同一主机上的 session channel 不超过 MaxSessions
    let slot = tokio::select! {
        _ = &mut *cancel_rx => {
            finish_exec_cancellation(&inner, None);
            return;
        }
        result = state.ssh_pool.acquire_exec_slot(&host.id) => result,
    };
    let _exec_permit = match slot {
        Ok(p) => p,
        Err(e) => {
            let mut g = inner.lock().unwrap();
            g.state = "failed".to_string();
            g.stderr.extend_from_slice(format!("{e:#}").as_bytes());
            g.ended_ms = Some(g.started_inst.elapsed().as_millis() as u64);
            return;
        }
    };

    let opened = tokio::select! {
        _ = &mut *cancel_rx => {
            finish_exec_cancellation(&inner, None);
            return;
        }
        result = open_host_channel(&state.ssh_pool, &state.db, &host.id) => result,
    };
    let mut channel = match opened {
        Ok((_, ch)) => ch,
        Err(e) => {
            let mut g = inner.lock().unwrap();
            g.state = "failed".to_string();
            g.stderr
                .extend_from_slice(format!("SSH 连接失败: {e:#}").as_bytes());
            g.ended_ms = Some(g.started_inst.elapsed().as_millis() as u64);
            return;
        }
    };

    let is_windows = host
        .os_info
        .as_ref()
        .and_then(|o| o.os.as_deref())
        .map(|s| s.eq_ignore_ascii_case("windows"))
        .unwrap_or(false);

    let pid_prefix = format!("\u{1e}vps-exec:{}:", inner.lock().unwrap().id);
    let final_cmd = if is_windows {
        wrap_windows_remote_command(cmd)
    } else {
        // 记录这一条 exec 的进程组，不改用户命令的 shell、输出通道或退出码。
        format!(
            "printf '\\036vps-exec:{}:%s\\037' \"$$\" >&2; eval '{}'",
            inner.lock().unwrap().id,
            cmd.replace('\'', "'\\''")
        )
    };

    if cancel_rx.try_recv().is_ok() {
        let _ = channel.close().await;
        finish_exec_cancellation(&inner, None);
        return;
    }

    if let Err(e) = channel.exec(true, final_cmd.as_str()).await {
        let mut g = inner.lock().unwrap();
        g.state = "failed".to_string();
        g.stderr
            .extend_from_slice(format!("发送 exec 请求失败: {e}").as_bytes());
        g.ended_ms = Some(g.started_inst.elapsed().as_millis() as u64);
        return;
    }

    let mut exit_code: Option<i32> = None;
    let mut exit_signal = None;
    let mut cancel_deadline = None;
    let mut connection_lost = false;
    let mut remote_pid = None;
    let mut pid_pending = !is_windows;
    let mut pid_buffer = Vec::new();
    let mut group_kill_sent = false;
    let mut cancel_error = None;
    loop {
        tokio::select! {
            _ = &mut *cancel_rx, if cancel_deadline.is_none() => {
                // CHANNEL_CLOSE 不会终止远端进程。发送 SSH signal 请求，随后等服务端确认退出。
                if let Err(e) = channel.signal(russh::Sig::KILL).await {
                    let _ = channel.close().await;
                    finish_exec_cancellation(&inner, Some(format!("发送远端终止信号失败，无法确认命令停止: {e}")));
                    return;
                }
                cancel_deadline = Some(Instant::now() + Duration::from_secs(5));
            }
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(
                cancel_deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(86400))
            )), if cancel_deadline.is_some() => {
                let _ = channel.close().await;
                finish_exec_cancellation(&inner, Some(cancel_error.unwrap_or_else(|| "远端未确认终止信号，无法保证命令已停止".to_string())));
                return;
            }
            msg_opt = channel.wait() => {
                match msg_opt {
                    Some(ChannelMsg::Data { data }) => {                        let mut g = inner.lock().unwrap();
                        append_capped(&mut g.stdout, &data);
                        g.last_output_inst = Instant::now();
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        let cleaned = if is_windows {
                            clean_clixml_stderr(&data)
                        } else if pid_pending {
                            consume_remote_pid(&mut pid_buffer, &mut pid_pending, &mut remote_pid, &pid_prefix, &data)
                        } else {
                            data.to_vec()
                        };
                        let mut g = inner.lock().unwrap();
                        append_capped(&mut g.stderr, &cleaned);
                        g.last_output_inst = Instant::now();
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = Some(exit_status as i32);
                    }
                    Some(ChannelMsg::ExitSignal { signal_name, .. }) => {
                        exit_signal = Some(signal_name);
                    }
                    // EOF 只表示"输出结束"：OpenSSH 的 exit-status 常在其之后到达，
                    // 必须读到 Close 才收尾，否则高频丢退出码（旧构建靠谎报成功掩盖了它）
                    Some(ChannelMsg::Eof) => {}
                    Some(ChannelMsg::Close) => break,
                    // russh 在收到对端 CHANNEL_CLOSE 时会先投递显式的 Close/Eof，只有会话整体消失才会得到 None
                    None => {
                        connection_lost = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
        if cancel_deadline.is_some() && !group_kill_sent {
            if let Some(pid) = remote_pid {
                group_kill_sent = true;
                // 某些 OpenSSH 忽略 signal 请求；用同一连接的短控制 channel 终止已确认的进程组。
                if let Err(e) = kill_remote_exec_group(state, &host.id, pid).await {
                    cancel_error = Some(format!("终止远端进程组失败，无法确认命令停止: {e:#}"));
                }
            }
        }
    }

    if !pid_buffer.is_empty() {
        append_capped(&mut inner.lock().unwrap().stderr, &pid_buffer);
    }

    if cancel_deadline.is_some() && matches!(exit_signal, Some(russh::Sig::KILL)) {
        finish_exec_cancellation(&inner, None);
        return;
    }

    let mut g = inner.lock().unwrap();
    if g.state == "running" {
        g.ended_ms = Some(g.started_inst.elapsed().as_millis() as u64);
        match exit_code {
            Some(code) => {
                g.exit_code = Some(code);
                g.state = if code == 0 {
                    "completed".to_string()
                } else {
                    "failed".to_string()
                };
                // 终态清掉超时期间写下的"仍在后台运行"提示
                g.hint = None;
            }
            None => {
                // 没拿到远端退出码就不能声称成功：连接中断与"服务端未上报退出码"都如实报失败
                g.exit_code = None;
                g.state = "failed".to_string();
                if connection_lost {
                    append_capped(
                        &mut g.stderr,
                        "\n[SSH 连接在命令结束前中断，输出可能不完整，退出码未知]".as_bytes(),
                    );
                    g.hint = Some(
                        "SSH 连接中断，命令结果不完整。请重试，或改用 vps_task 查询后续输出。"
                            .to_string(),
                    );
                } else {
                    g.hint = Some(
                        "远端未上报退出码，无法确认命令是否成功（已按失败处理，stdout/stderr 仍原样返回）。"
                            .to_string(),
                    );
                }
            }
        }
    }
}

fn consume_remote_pid(
    buffer: &mut Vec<u8>,
    pending: &mut bool,
    pid: &mut Option<u32>,
    prefix: &str,
    data: &[u8],
) -> Vec<u8> {
    buffer.extend_from_slice(data);
    if let Some(start) = buffer
        .windows(prefix.len())
        .position(|w| w == prefix.as_bytes())
    {
        let value_start = start + prefix.len();
        if let Some(end_rel) = buffer[value_start..].iter().position(|b| *b == 0x1f) {
            let end = value_start + end_rel;
            if let Some(value) = std::str::from_utf8(&buffer[value_start..end])
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|pid| *pid > 1 && *pid <= i32::MAX as u32)
            {
                *pid = Some(value);
                buffer.drain(start..=end);
                *pending = false;
                return std::mem::take(buffer);
            }
        }
    }
    if buffer.len() > 4096 {
        *pending = false;
        return std::mem::take(buffer);
    }
    Vec::new()
}

async fn kill_remote_exec_group(state: &Arc<AppState>, host_id: &str, pid: u32) -> Result<()> {
    let (_, mut channel) = tokio::time::timeout(
        Duration::from_secs(3),
        open_host_channel(&state.ssh_pool, &state.db, host_id),
    )
    .await??;
    // /proc 的进程组必须仍是这条 SSH exec 自己的组，不能对共享会话发送组信号。
    let command = format!("IFS= read -r s < /proc/{pid}/stat || exit 1; s=${{s##*) }}; set -- $s; [ \"$3\" = \"{pid}\" ] || exit 2; kill -KILL -- -{pid}");
    channel.exec(true, command.as_str()).await?;
    let mut exit_code = None;
    let mut output = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status),
                ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                    output.extend_from_slice(&data)
                }
                ChannelMsg::Close => break,
                _ => {}
            }
        }
    })
    .await;
    let _ = channel.close().await;
    result?;
    if exit_code != Some(0) {
        return Err(anyhow!(
            "控制命令退出码 {:?}: {}",
            exit_code,
            String::from_utf8_lossy(&output)
        ));
    }
    Ok(())
}

fn finish_exec_cancellation(inner: &Arc<Mutex<ExecTaskInner>>, error: Option<String>) {
    let mut g = inner.lock().unwrap();
    if g.state == "running" {
        g.state = if error.is_some() {
            "failed"
        } else {
            "cancelled"
        }
        .to_string();
        g.hint = error;
        g.ended_ms = Some(g.started_inst.elapsed().as_millis() as u64);
    }
}

// ==================== MCP JSON-RPC 2.0 协议与 7 大精锐工具定义 ====================

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

pub fn mcp_tool_definitions_filtered(enabled_tools: &[String]) -> Value {
    let all = mcp_tool_definitions();
    let Some(arr) = all.as_array() else {
        return json!([]);
    };
    let filtered: Vec<Value> = arr
        .iter()
        .filter(|item| {
            item.get("name")
                .and_then(|v| v.as_str())
                .is_some_and(|name| enabled_tools.iter().any(|t| t == name))
        })
        .cloned()
        .collect();
    Value::Array(filtered)
}

pub fn mcp_tool_definitions() -> Value {
    json!([
        {
            "name": "vps_list",
            "description": "列出所有托管的 VPS 与本机（直接以主机名标识，无需查 UUID），同时返回每台主机的在线状态、延迟以及已缓存的探针硬件概览（CPU%、Steal% 超售率、内存已用/总量、磁盘已用/总量、BBR 状态）。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "refresh": {
                        "type": "boolean",
                        "description": "是否先并发实时刷新全机队探针快照（默认 false 直接秒回内存缓存）"
                    },
                    "role": {
                        "type": "string",
                        "description": "按角色过滤（primary/project/proxy/relay/test/archive）"
                    },
                    "search": {
                        "type": "string",
                        "description": "按主机名称、IP 或标签搜索"
                    }
                }
            }
        },
        {
            "name": "vps_inspect",
            "description": "对一台或多台 VPS 立即触发实时深度探索（纯内核计数差分），返回精确 CPU%、Steal% 超售率、1/5/15 分钟负载、内存与缓存解剖、全部挂载磁盘容量、主物理网卡实时上下行速率与累计总流量。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host": {
                        "type": "string",
                        "description": "目标主机名（如 'web-1'、'db-2'、'本机'）或 IP"
                    },
                    "hosts": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "批量并发探索多台主机名列表"
                    }
                }
            }
        },
        {
            "name": "vps_exec",
            "description": "在一台或多台 VPS 上并发执行非交互式命令（直接传主机名如 'web-1'）。若在 wait_seconds 内完成则同步返回 stdout/stderr/exitCode（ok=true）；若超时未完则返回 state=running、ok=false 与 taskId（表示未完成、不是失败），此时若需要结果，用**一次** vps_task(action:'get', task_id, wait_seconds:N) 等待其结束即可，不要反复查询；若检测到命令卡在 [Y/n] 等交互提问超过 2 秒会自动早退提醒改用 vps_terminal。命令会先过本机命令黑名单，命中不可逆灾难（如 rm -rf /、mkfs、格式化盘）即直接拒绝执行、不产生任何输出——名单文件是 data/deny-list.txt，被拦时不要换写法重试，直接告诉用户哪条被拦。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host": {
                        "type": "string",
                        "description": "单台目标主机名（如 'web-1'、'db-2'、'本机'）或 IP"
                    },
                    "hosts": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "多台目标主机名数组（将并发在所有指定主机上同时执行该命令）"
                    },
                    "command": {
                        "type": "string",
                        "description": "要执行的命令"
                    },
                    "wait_seconds": {
                        "type": "integer",
                        "description": "同步等待秒数（默认 25，上限 120）；超时后任务继续在后台运行并返回 taskId"
                    }
                },
                "required": ["command"]
            }
        },
        {
            "name": "vps_terminal",
            "description": "有状态 PTY 交互式终端工具（基于 Rust alacritty_terminal 真实二维屏幕网格）。默认无头运行（reveal_ui: false），专门用于运行需要交互确认（如输入 yes/[Y/n]、选择安装菜单）的脚本——当脚本停下等待输入时会立即返回 state='waiting_input' 及光标所在行文字，你可以继续调用 send 发送 'y'。当遇到需要用户亲自输入隐私密码（如数据库密码、sudo密码、密钥口令）或人工接手时，设置 reveal_ui=true 或调用 action='reveal'，即可像 Playwright 一样直接将终端弹窗展现在用户界面上。action='send' 的输入同样会先过命令黑名单，命中不可逆灾难即拒绝发送。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host": {
                        "type": "string",
                        "description": "目标主机名（如 'web-1'、'db-2'、'本机'）"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["send", "key", "read", "reveal", "close"],
                        "description": "动作：send(发送命令或交互回答如 'y') / key(发送控制键如 'ctrl+c','enter') / read(读取当前屏幕网格) / reveal(在用户界面唤起并展示该终端供用户输密码或接手) / close(关闭终端)"
                    },
                    "input": {
                        "type": "string",
                        "description": "action='send' 时要输入的命令或回答文本（默认自动附带回车），或 action='key' 时的按键名（ctrl+c / ctrl+d / ctrl+z / enter / esc / tab / up / down）"
                    },
                    "submit": {
                        "type": "boolean",
                        "description": "action='send' 时是否在末尾自动发送回车（默认 true）"
                    },
                    "reveal_ui": {
                        "type": "boolean",
                        "description": "是否将该终端在用户前端界面自动弹出展示（默认 false 无头运行不打扰用户；需要用户亲自输密码或观察时设为 true）"
                    },
                    "reason": {
                        "type": "string",
                        "description": "唤起界面终端时展示给用户的提示原因（如 '请在终端中输入 MySQL root 密码'）"
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "发送输入后等待屏幕稳定或提问出现的毫秒数（默认 6000）"
                    },
                    "lines": {
                        "type": "integer",
                        "description": "返回的终端屏幕尾部行数（默认 35）"
                    },
                    "pane_id": {
                        "type": "string",
                        "description": "可选的指定 paneId（不传则自动复用或创建该主机的主终端）"
                    }
                },
                "required": ["host", "action"]
            }
        },
        {
            "name": "vps_fs",
            "description": "统一本机与远端 SFTP 文件系统操作：列目录(list)、读文本文件(read)、写文本/配置文件(write)、新建目录(mkdir)、重命名(rename)、删除(remove)、打包压缩(compress)、解压归档(extract)。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host": {
                        "type": "string",
                        "description": "目标主机名（如 'web-1'、'本机'）"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["list", "read", "write", "mkdir", "rename", "remove", "compress", "extract"],
                        "description": "文件操作类型"
                    },
                    "path": {
                        "type": "string",
                        "description": "目标文件或目录路径"
                    },
                    "new_path": {
                        "type": "string",
                        "description": "action='rename' 时的目标新路径"
                    },
                    "content": {
                        "type": "string",
                        "description": "action='write' 时写入的完整 UTF-8 文本内容"
                    },
                    "is_dir": {
                        "type": "boolean",
                        "description": "action='remove' 时是否为目录"
                    }
                },
                "required": ["host", "action", "path"]
            }
        },
        {
            "name": "vps_transfer",
            "description": "满速流式文件传输引擎（16MiB 通道窗口 + 64 路并发 SFTP 流水线 + 12 秒无推进失速看门狗）。支持三种方向：upload（本机路径 -> 远端路径）、download（远端路径 -> 本机路径）、cross（源 VPS 路径 -> 目标 VPS 路径，内存直传不落本机硬盘）。无需分片或 base64，一次调用直接完成。落盘走临时文件原子替换，完成后自动做 SHA-256 闭环校验（返回 sha256 与 verified 字段；校验不一致时任务判失败）。断点续传：中断时 `<目标>.vps-part` 残片保留不删，对同一个目标再调用一次即自动哈希对账后从残片末尾续传（返回 resumedFrom 字段说明本次从第几字节接上）。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "direction": {
                        "type": "string",
                        "enum": ["upload", "download", "cross"],
                        "description": "upload(本机上传到VPS) / download(从VPS下载到本机) / cross(从一台VPS直接传到另一台VPS)"
                    },
                    "host": {
                        "type": "string",
                        "description": "upload 或 download 时的目标 VPS 主机名"
                    },
                    "local_path": {
                        "type": "string",
                        "description": "upload 或 download 时的本机文件完整路径"
                    },
                    "remote_path": {
                        "type": "string",
                        "description": "upload 或 download 时的远端 VPS 文件或目录路径"
                    },
                    "source_host": {
                        "type": "string",
                        "description": "direction='cross' 时的源 VPS 主机名"
                    },
                    "source_path": {
                        "type": "string",
                        "description": "direction='cross' 时的源 VPS 文件路径"
                    },
                    "target_host": {
                        "type": "string",
                        "description": "direction='cross' 时的目标 VPS 主机名"
                    },
                    "target_path": {
                        "type": "string",
                        "description": "direction='cross' 时的目标 VPS 文件或目录路径"
                    },
                    "wait_seconds": {
                        "type": "integer",
                        "description": "同步等待秒数（默认 25）；超大文件若超时未传完则返回 transferId（未完成），需要结果时用一次 vps_task 的 wait_seconds 等待，不要反复查询"
                    }
                },
                "required": ["direction"]
            }
        },
        {
            "name": "vps_task",
            "description": "查询或取消正在后台运行的命令任务（exec_...）或大文件传输任务（tr_...）。action='get' 时可传 wait_seconds 阻塞等待任务结束——**一次调用**即拿到最终结果；不要用反复 get 来轮询（既慢又会一直跑下去）。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["get", "list", "cancel"],
                        "description": "get(查询单个任务) / list(列出所有任务) / cancel(取消运行中的命令任务)"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "exec_... 或 tr_... 任务 ID"
                    },
                    "wait_seconds": {
                        "type": "integer",
                        "description": "action='get' 时阻塞等待任务结束的秒数（默认 0 立即返回；建议需要结果时传一个上限，如 120，一次拿到最终结果）"
                    }
                },
                "required": ["action"]
            }
        }
    ])
}

/// 从 MCP 调用参数里取目标主机标签（用于留痕）：优先 host，其次 source_host/target_host，再其次 hosts 列表。
fn mcp_call_host_label(args: &Value) -> Option<String> {
    for key in ["host", "source_host", "target_host"] {
        if let Some(s) = args.get(key).and_then(|v| v.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    if let Some(arr) = args.get("hosts").and_then(|v| v.as_array()) {
        let joined = arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(",");
        if !joined.is_empty() {
            return Some(joined);
        }
    }
    None
}

/// 给一条 MCP 调用结果生成简短摘要（exec 类给出 state/exit/输出头，其余给裁剪后的 JSON）。
fn mcp_result_summary(val: &Value) -> String {
    fn one(r: &Value) -> String {
        let state = r.get("state").and_then(|v| v.as_str()).unwrap_or("");
        let code = r.get("exitCode").and_then(|v| v.as_i64());
        let out = r
            .get("stdout")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| r.get("stderr").and_then(|v| v.as_str()))
            .unwrap_or("");
        let head: String = out.trim().chars().take(200).collect();
        format!("state={state} exit={code:?} out={head}")
    }
    if let Some(r) = val.get("result") {
        return one(r);
    }
    if let Some(arr) = val.get("results").and_then(|v| v.as_array()) {
        let states: Vec<String> = arr
            .iter()
            .map(|r| {
                r.get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string()
            })
            .collect();
        return format!("{} 台: {}", arr.len(), states.join(","));
    }
    if let Some(t) = val.get("task").or_else(|| val.get("transfer")) {
        return one(t);
    }
    let s = serde_json::to_string(val).unwrap_or_default();
    s.chars().take(300).collect()
}

pub async fn handle_mcp_request(
    state: &Arc<AppState>,
    exec_tasks: &Arc<ExecTaskManager>,
    req: JsonRpcRequest,
) -> Option<Value> {
    let id = req.id.clone();
    match req.method.as_str() {
        "notifications/initialized" | "notifications/cancelled" => None,
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "vps-switch",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        })),
        "ping" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {}
        })),
        "tools/list" => {
            let enabled = state.db.get_enabled_mcp_tools();
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": mcp_tool_definitions_filtered(&enabled)
                }
            }))
        }
        "tools/call" => {
            let tool_name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args = req
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));

            // 全程留痕：MCP 是外部 agent 唯一能下达命令的通道，每一次调用都在这里落库
            // （工具、目标主机、参数、成败、耗时、结果摘要）。内部执行（探针/tar/reg 等）不记。
            let args_json = serde_json::to_string(&args).unwrap_or_default();
            let host_label = mcp_call_host_label(&args);
            let started = Instant::now();

            let outcome = dispatch_tool_call(state, exec_tasks, &tool_name, args).await;
            let (payload, record_ok, summary, result_text) = match outcome {
                Ok(val) => {
                    let text = serde_json::to_string_pretty(&val).unwrap_or_default();
                    let ok = val.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
                    let summary = format!("ok={ok} {}", mcp_result_summary(&val));
                    (
                        json!({ "content": [{ "type": "text", "text": &text }], "isError": false }),
                        ok,
                        summary,
                        text,
                    )
                }
                Err(err) => {
                    let msg = format!("{err:#}");
                    let text = json!({ "ok": false, "error": msg }).to_string();
                    (
                        json!({ "content": [{ "type": "text", "text": &text }], "isError": true }),
                        false,
                        format!("error: {msg}"),
                        text,
                    )
                }
            };

            if let Err(e) = state.db.record_mcp_call(
                &tool_name,
                host_label.as_deref(),
                &args_json,
                &result_text,
                record_ok,
                started.elapsed().as_millis() as u64,
                &summary,
            ) {
                tracing::warn!("写入 MCP 调用记录失败: {e:#}");
            }

            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": payload
            }))
        }
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": format!("Method not found: {}", req.method)
            }
        })),
    }
}

async fn dispatch_tool_call(
    state: &Arc<AppState>,
    exec_tasks: &Arc<ExecTaskManager>,
    name: &str,
    args: Value,
) -> Result<Value> {
    if crate::db::ALL_MCP_TOOLS.contains(&name) {
        let enabled = state.db.get_enabled_mcp_tools();
        if !enabled.iter().any(|t| t == name) {
            return Err(anyhow!(
                "MCP 工具「{name}」已在设置中关闭（可在「设置 → MCP Server」中勾选开启）"
            ));
        }
    }
    match name {
        "vps_list" => tool_vps_list(state, args).await,
        "vps_inspect" => tool_vps_inspect(state, args).await,
        "vps_exec" => tool_vps_exec(state, exec_tasks, args).await,
        "vps_terminal" => tool_vps_terminal(state, args).await,
        "vps_fs" => tool_vps_fs(state, args).await,
        "vps_transfer" => tool_vps_transfer(state, args).await,
        "vps_task" => tool_vps_task(state, exec_tasks, args).await,
        _ => Err(anyhow!("未知的 MCP 工具: {name}")),
    }
}

async fn tool_vps_list(state: &Arc<AppState>, args: Value) -> Result<Value> {
    let refresh = args
        .get("refresh")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let role_filter = args
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let search_q = args
        .get("search")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();

    if refresh {
        let records = state.db.list_records()?;
        let active: Vec<_> = records
            .into_iter()
            .filter(|r| !r.archived && r.mcp_exposed)
            .collect();
        stream::iter(active)
            .for_each_concurrent(8, |rec| {
                let st = state.clone();
                async move {
                    let _ = st
                        .inspector
                        .inspect_host(&st.ssh_pool, &st.db, &rec.id)
                        .await;
                }
            })
            .await;
    }

    let hosts = state.db.list_public_hosts()?;
    let snaps = state.inspector.get_all_cached();

    let mut out = Vec::new();
    for h in hosts {
        if h.archived || !h.mcp_exposed {
            continue;
        }
        if !role_filter.is_empty() && h.role.as_deref() != Some(role_filter) {
            continue;
        }
        if !search_q.is_empty() {
            let hay = format!(
                "{} {} {} {}",
                h.name,
                h.addr,
                h.os_info
                    .as_ref()
                    .and_then(|o| o.pretty_name.as_deref())
                    .unwrap_or(""),
                h.tags.join(" ")
            )
            .to_lowercase();
            if !hay.contains(&search_q) {
                continue;
            }
        }

        let snap = snaps.get(&h.id);
        out.push(json!({
            "name": h.name,
            "status": h.status,
            "latencyMs": h.latency_ms,
            "address": if h.kind == "local" { "127.0.0.1".to_string() } else { format!("{}:{}", h.addr, h.port) },
            "username": h.username,
            "os": h.os_info.as_ref().and_then(|o| o.pretty_name.clone()),
            "role": h.role,
            "tags": h.tags,
            "viaJump": h.jump_host_name,
            "metrics": snap.map(|s| json!({
                "cpuUsagePct": s.cpu_usage_pct,
                "cpuStealPct": s.cpu_steal_pct,
                "load1": s.load_1,
                "memUsedMB": s.mem_used_bytes / 1024 / 1024,
                "memTotalMB": s.mem_total_bytes / 1024 / 1024,
                "diskUsedGB": ((s.disk_used_bytes as f64 / 1073741824.0) * 10.0).round() / 10.0,
                "diskTotalGB": ((s.disk_total_bytes as f64 / 1073741824.0) * 10.0).round() / 10.0,
                "tcpCc": s.tcp_cc,
            })),
        }));
    }

    Ok(json!({
        "ok": true,
        "note": "所有工具均支持直接传 name（如 'web-1'、'db-2'、'本机'）定位主机，无需使用内部 ID。",
        "total": out.len(),
        "hosts": out,
    }))
}

async fn tool_vps_inspect(state: &Arc<AppState>, args: Value) -> Result<Value> {
    let mut target_queries: Vec<String> = Vec::new();
    if let Some(h) = args.get("host").and_then(|v| v.as_str()) {
        if !h.trim().is_empty() {
            target_queries.push(h.trim().to_string());
        }
    }
    if let Some(arr) = args.get("hosts").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str() {
                if !s.trim().is_empty() {
                    target_queries.push(s.trim().to_string());
                }
            }
        }
    }
    if target_queries.is_empty() {
        return Err(anyhow!("请提供 host 或 hosts 参数"));
    }

    let mut resolved = Vec::new();
    for q in target_queries {
        resolved.push(resolve_host(&state.db, &q)?);
    }

    let results: Vec<Value> = stream::iter(resolved)
        .map(|rec| {
            let st = state.clone();
            async move {
                match st
                    .inspector
                    .inspect_host(&st.ssh_pool, &st.db, &rec.id)
                    .await
                {
                    Ok(ins) => json!({
                        "host": rec.name,
                        "ok": true,
                        "inspection": ins,
                    }),
                    Err(e) => json!({
                        "host": rec.name,
                        "ok": false,
                        "error": format!("{e:#}"),
                    }),
                }
            }
        })
        .buffer_unordered(8)
        .collect()
        .await;

    Ok(json!({
        "ok": results.iter().all(|r| r["ok"] == true),
        "results": results,
    }))
}

async fn tool_vps_exec(
    state: &Arc<AppState>,
    exec_tasks: &Arc<ExecTaskManager>,
    args: Value,
) -> Result<Value> {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("command 参数不能为空"))?
        .to_string();

    // 护栏：agent 通道的命令先过黑名单。命中即拒绝执行，不给任何执行结果。
    if let Verdict::Deny { why } = state.guard.check(&command) {
        return Err(anyhow!("{why}"));
    }

    let wait_seconds = args
        .get("wait_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(25);

    let mut host_queries = Vec::new();
    if let Some(h) = args.get("host").and_then(|v| v.as_str()) {
        if !h.trim().is_empty() {
            host_queries.push(h.trim().to_string());
        }
    }
    if let Some(arr) = args.get("hosts").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str() {
                if !s.trim().is_empty() {
                    host_queries.push(s.trim().to_string());
                }
            }
        }
    }
    if host_queries.is_empty() {
        return Err(anyhow!(
            "请提供 host 或 hosts 参数（直接传主机名，如 'web-1'）"
        ));
    }

    let mut targets = Vec::new();
    for q in host_queries {
        targets.push(resolve_host(&state.db, &q)?);
    }

    if targets.len() == 1 {
        let snap = exec_tasks
            .exec_with_wait(state, &targets[0], &command, wait_seconds)
            .await?;
        return Ok(json!({
            // 未完成（running / interactive_stall）一律 ok=false：旧值把 running 也算成功，
            // 配合"去轮询"的 hint 让 agent 以为一切正常、继续等下去。
            "ok": snap.state == "completed",
            "result": snap,
        }));
    }

    let results: Vec<ExecTaskSnapshot> = stream::iter(targets)
        .map(|rec| {
            let st = state.clone();
            let et = exec_tasks.clone();
            let cmd = command.clone();
            async move {
                et.exec_with_wait(&st, &rec, &cmd, wait_seconds)
                    .await
                    .unwrap_or_else(|e| ExecTaskSnapshot {
                        id: String::new(),
                        host_id: rec.id,
                        host_name: rec.name,
                        command: cmd,
                        state: "failed".to_string(),
                        exit_code: Some(-1),
                        duration_ms: 0,
                        stdout: String::new(),
                        stderr: format!("{e:#}"),
                        stdout_truncated: false,
                        stderr_truncated: false,
                        hint: None,
                        started_at: Utc::now().to_rfc3339(),
                    })
            }
        })
        .buffer_unordered(8)
        .collect()
        .await;

    // 多机：全部 completed 才算 ok；有仍在跑的（超时转后台）如实报 false。
    let all_done = results.iter().all(|r| r.state == "completed");
    Ok(json!({
        "ok": all_done,
        "results": results,
    }))
}

async fn tool_vps_terminal(state: &Arc<AppState>, args: Value) -> Result<Value> {
    let host_q = args
        .get("host")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 host 参数"))?;
    let host = resolve_host(&state.db, host_q)?;

    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("read");
    let reveal_ui = args
        .get("reveal_ui")
        .and_then(|v| v.as_bool())
        .unwrap_or(action == "reveal");
    let reason = args
        .get("reason")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let max_lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(35) as usize;
    let wait_ms = args.get("wait_ms").and_then(|v| v.as_u64()).unwrap_or(6000);

    let pane_id_arg = args.get("pane_id").and_then(|v| v.as_str());
    let specified_pane = pane_id_arg
        .map(|pid| {
            let pane = state
                .terminals
                .get_pane(pid)
                .ok_or_else(|| anyhow!("指定的终端 pane 不存在: {pid}"))?;
            if pane.host_id != host.id {
                return Err(anyhow!("终端 {pid} 不属于目标主机「{}」", host.name));
            }
            Ok(pane)
        })
        .transpose()?;

    if action == "close" {
        if let Some(pane) = specified_pane {
            state.terminals.close_pane(&state.ssh_pool, &pane.id).await;
        } else if let Some(pane) = state.terminals.find_active_pane_for_host(&host.id) {
            state.terminals.close_pane(&state.ssh_pool, &pane.id).await;
        }
        return Ok(json!({ "ok": true, "host": host.name, "closed": true }));
    }

    // 查找或自动创建该主机的终端会话（默认 headless=true 无头运行，除非 reveal_ui=true）
    let pane = if let Some(pane) = specified_pane {
        pane
    } else if let Some(existing) = state.terminals.find_active_pane_for_host(&host.id) {
        existing
    } else {
        let created = state
            .terminals
            .create_pane(
                &state.ssh_pool,
                &state.db,
                &host.id,
                100,
                30,
                None,
                !reveal_ui,
            )
            .await?;
        let handle = state
            .terminals
            .get_pane(&created.id)
            .ok_or_else(|| anyhow!("创建终端后获取句柄失败"))?;
        handle
    };

    if reveal_ui {
        state.terminals.reveal_pane_in_ui(&pane.id, reason.clone());
    }

    let snap = match action {
        "reveal" | "read" => pane.read_screen_snapshot(max_lines),
        "send" => {
            let input = args.get("input").and_then(|v| v.as_str()).unwrap_or("");
            // 护栏：终端也是命令通道，`vps_exec` 拦下的写法不能从这里绕过去。
            // 交互回答（"y"、密码）不会命中黑名单，照常发送。
            if let Verdict::Deny { why } = state.guard.check(input) {
                return Err(anyhow!("{why}"));
            }
            let submit = args.get("submit").and_then(|v| v.as_bool()).unwrap_or(true);
            let mut bytes = input.as_bytes().to_vec();
            if submit {
                bytes.push(b'\r');
            }
            pane.send_and_wait(bytes, max_lines, wait_ms).await?
        }
        "key" => {
            let raw_input = args
                .get("input")
                .and_then(|v| v.as_str())
                .unwrap_or("enter");
            // 护栏：key 分支的兜底分支会把未知字符串原样当按键发出去，等于一条不受检查的文本
            // 通道（`key:"rm -rf /"` 再 `key:"enter"` 就能执行）。这里和 send 一样先过黑名单。
            // 真正的控制键（ctrl+c/enter/…）不会命中名单，照常发送。
            if let Verdict::Deny { why } = state.guard.check(raw_input) {
                return Err(anyhow!("{why}"));
            }
            let key_name = raw_input.to_lowercase();
            let bytes: Vec<u8> = match key_name.as_str() {
                "ctrl+c" | "c-c" | "sigint" => vec![0x03],
                "ctrl+d" | "c-d" | "eof" => vec![0x04],
                "ctrl+z" | "c-z" => vec![0x1a],
                "ctrl+l" | "c-l" => vec![0x0c],
                "enter" | "return" => vec![b'\r'],
                "esc" | "escape" => vec![0x1b],
                "tab" => vec![b'\t'],
                "space" => vec![b' '],
                "up" => b"\x1b[A".to_vec(),
                "down" => b"\x1b[B".to_vec(),
                "right" => b"\x1b[C".to_vec(),
                "left" => b"\x1b[D".to_vec(),
                other => other.as_bytes().to_vec(),
            };
            pane.send_and_wait(bytes, max_lines, wait_ms).await?
        }
        other => return Err(anyhow!("未知的 vps_terminal action: {other}")),
    };

    Ok(json!({
        "ok": true,
        "host": host.name,
        "revealedInUi": !snap.headless,
        "terminal": snap,
    }))
}

async fn tool_vps_fs(state: &Arc<AppState>, args: Value) -> Result<Value> {
    let host_q = args
        .get("host")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 host 参数"))?;
    let host = resolve_host(&state.db, host_q)?;
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 action 参数"))?;
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");

    match action {
        "list" => {
            let listing =
                files::list_dir(&state.ssh_pool, &state.db, &host.id, Some(path), false).await?;
            Ok(json!({
                "ok": true,
                "host": host.name,
                "path": listing.path,
                "parent": listing.parent,
                "items": listing.items,
            }))
        }
        "read" => {
            let res = files::read_text_file(&state.ssh_pool, &state.db, &host.id, path).await?;
            Ok(json!({
                "ok": true,
                "host": host.name,
                "path": res.path,
                "size": res.size,
                "content": res.content,
            }))
        }
        "write" => {
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("write 操作需要 content 参数"))?;
            let size =
                files::write_text_file(&state.ssh_pool, &state.db, &host.id, path, content).await?;
            Ok(json!({
                "ok": true,
                "host": host.name,
                "path": path,
                "size": size,
            }))
        }
        "mkdir" => {
            files::create_dir(&state.ssh_pool, &state.db, &host.id, path).await?;
            Ok(json!({ "ok": true, "host": host.name, "path": path }))
        }
        "rename" => {
            let new_path = args
                .get("new_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("rename 操作需要 new_path 参数"))?;
            files::rename_item(&state.ssh_pool, &state.db, &host.id, path, new_path).await?;
            Ok(json!({ "ok": true, "host": host.name, "oldPath": path, "newPath": new_path }))
        }
        "remove" => {
            let is_dir = args
                .get("is_dir")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            files::remove_item(&state.ssh_pool, &state.db, &host.id, path, is_dir).await?;
            Ok(json!({ "ok": true, "host": host.name, "removed": path }))
        }
        "compress" => {
            let out = files::compress_item(&state.ssh_pool, &state.db, &host.id, path).await?;
            Ok(json!({ "ok": true, "host": host.name, "archivePath": out }))
        }
        "extract" => {
            let out = files::extract_item(&state.ssh_pool, &state.db, &host.id, path).await?;
            Ok(json!({ "ok": true, "host": host.name, "extractedPath": out }))
        }
        other => Err(anyhow!("未知的 vps_fs action: {other}")),
    }
}

async fn tool_vps_transfer(state: &Arc<AppState>, args: Value) -> Result<Value> {
    let direction = args
        .get("direction")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 direction 参数 (upload | download | cross)"))?;
    let wait_seconds = args
        .get("wait_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(25)
        .clamp(2, 120);

    if direction == "cross" {
        let src_q = args
            .get("source_host")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("cross 模式需要 source_host"))?;
        let dst_q = args
            .get("target_host")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("cross 模式需要 target_host"))?;
        let src_path = args
            .get("source_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("cross 模式需要 source_path"))?
            .to_string();
        let dst_path = args
            .get("target_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("cross 模式需要 target_path"))?
            .to_string();

        let src_host = resolve_host(&state.db, src_q)?;
        let dst_host = resolve_host(&state.db, dst_q)?;
        let st = state.clone();
        let src_id = src_host.id.clone();
        let dst_id = dst_host.id.clone();

        // 必须 spawn：若把 future 直接交给 timeout，超时会把它 drop 掉（传输被真正取消），
        // 却仍对外声称"仍在后台传输"，属于谎报。spawn 后超时只是不再等待，传输继续跑。
        let handle = tokio::spawn(async move {
            files::transfer_remote_to_remote(
                &st.ssh_pool,
                &st.db,
                &st.transfers,
                &src_id,
                &src_path,
                &dst_id,
                &dst_path,
            )
            .await
        });
        match tokio::time::timeout(Duration::from_secs(wait_seconds), handle).await {
            Ok(join_res) => {
                let task = join_res??;
                return Ok(json!({ "ok": true, "transfer": task }));
            }
            Err(_) => {
                let latest = state.transfers.list_tasks(Some(&dst_host.id));
                return Ok(json!({
                    "ok": false,
                    "state": "running_in_background",
                    "transfer": latest.first(),
                    "hint": "大文件仍在后台传输（未完成，ok=false 并非失败）。需要结果时调用一次 vps_task(action:\"get\", task_id:<transferId>, wait_seconds:N) 等待其结束即可；不需要可直接忽略，不要反复查询。"
                }));
            }
        }
    }

    let host_q = args
        .get("host")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 host 参数"))?;
    let host = resolve_host(&state.db, host_q)?;
    let local_path = args
        .get("local_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 local_path 参数"))?
        .to_string();
    let remote_path = args
        .get("remote_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少 remote_path 参数"))?
        .to_string();

    let st = state.clone();
    let dir_str = direction.to_string();
    let hid = host.id.clone();
    let handle = tokio::spawn(async move {
        files::transfer_local_remote_file(
            &st.ssh_pool,
            &st.db,
            &st.transfers,
            &hid,
            &dir_str,
            &local_path,
            &remote_path,
        )
        .await
    });

    match tokio::time::timeout(Duration::from_secs(wait_seconds), handle).await {
        Ok(join_res) => {
            let task = join_res??;
            Ok(json!({
                "ok": true,
                "host": host.name,
                "transfer": task,
            }))
        }
        Err(_) => {
            let latest = state.transfers.list_tasks(Some(&host.id));
            Ok(json!({
                "ok": false,
                "host": host.name,
                "state": "running_in_background",
                "transfer": latest.first(),
                "hint": "大文件仍在后台传输（未完成，ok=false 并非失败）。需要结果时调用一次 vps_task(action:\"get\", task_id:<transferId>, wait_seconds:N) 等待其结束即可；不需要可直接忽略，不要反复查询（若连续 12 秒无字节推进将由看门狗自动中断报错）。"
            }))
        }
    }
}

async fn tool_vps_task(
    state: &Arc<AppState>,
    exec_tasks: &Arc<ExecTaskManager>,
    args: Value,
) -> Result<Value> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("list");
    let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
    // 阻塞等待秒数：>0 时等到任务结束或超时再返回，**一次调用**即拿到最终结果，
    // 取代"查询→running→再查询"的轮询（旧设计里 vps_task 无阻塞模式，是死循环的另一半）。
    let wait_seconds = args
        .get("wait_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .min(600);

    match action {
        "get" => {
            if task_id.starts_with("exec_") {
                let mut snap = exec_tasks
                    .get_task(task_id)
                    .ok_or_else(|| anyhow!("未找到命令任务: {task_id}"))?;
                if wait_seconds > 0 && snap.state == "running" {
                    let deadline = Instant::now() + Duration::from_secs(wait_seconds);
                    loop {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        match exec_tasks.get_task(task_id) {
                            Some(s) => {
                                snap = s;
                                if snap.state != "running" {
                                    break;
                                }
                            }
                            None => break,
                        }
                        if Instant::now() >= deadline {
                            break;
                        }
                    }
                }
                Ok(json!({ "ok": snap.state == "completed", "task": snap }))
            } else {
                let mut t = state
                    .transfers
                    .list_tasks(None)
                    .into_iter()
                    .find(|x| x.id == task_id)
                    .ok_or_else(|| anyhow!("未找到传输任务: {task_id}"))?;
                if wait_seconds > 0 && t.state == "running" {
                    let deadline = Instant::now() + Duration::from_secs(wait_seconds);
                    loop {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        match state
                            .transfers
                            .list_tasks(None)
                            .into_iter()
                            .find(|x| x.id == task_id)
                        {
                            Some(s) => {
                                t = s;
                                if t.state != "running" {
                                    break;
                                }
                            }
                            None => break,
                        }
                        if Instant::now() >= deadline {
                            break;
                        }
                    }
                }
                Ok(json!({ "ok": t.state == "completed", "transfer": t }))
            }
        }
        "cancel" => {
            let t = exec_tasks.cancel_task(task_id).await?;
            Ok(json!({ "ok": t.state == "cancelled" || t.state == "completed", "task": t }))
        }
        _ => Ok(json!({
            "ok": true,
            "execTasks": exec_tasks.list_tasks(),
            "transfers": state.transfers.list_tasks(None),
        })),
    }
}

/// `--mcp-stdio` 本地模式：若常驻主进程正在监听则直接桥接过去（与桌面 UI 共享连接池、探针缓存和终端唤起事件），
/// 若主进程未运行则在当前进程内直接处理。
pub async fn run_stdio_bridge(
    state: Arc<AppState>,
    exec_tasks: Arc<ExecTaskManager>,
    port: u16,
) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut lines = tokio::io::BufReader::new(stdin).lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<JsonRpcRequest>(trimmed) else {
            continue;
        };

        let settings = state.db.get_app_settings().ok();
        // 尊重 MCP 总开关（否则下面的进程内回退会绕过它）
        if settings.as_ref().map(|s| !s.mcp_enabled).unwrap_or(false) {
            let err = json!({
                "jsonrpc": "2.0",
                "id": req.id,
                "error": { "code": -32000, "message": "MCP Server 已在设置中关闭" }
            });
            println!("{}", serde_json::to_string(&err)?);
            continue;
        }
        let api_key = settings.map(|s| s.mcp_api_key).unwrap_or_default();

        // 优先转发给已运行的常驻主进程（共享 SshPool 与 UI 终端联动）。只有转发**确实拿到了
        // 一个可用的 JSON-RPC 响应**才采用它；否则回退到进程内处理。
        // 此前是"拿到任何响应就原样打印"——当两个进程解析到不同的库、API Key 对不上时，
        // 服务端回 401，桥接把那段错误文本当结果吐给客户端，客户端解析失败 → 永远握不上手
        // （WorkBuddy 连不上的根因）。现在 401/403 之类会返回 None 触发回退。
        if let Some(resp_str) = try_forward_to_daemon(port, &api_key, trimmed).await {
            if !resp_str.trim().is_empty() {
                println!("{}", resp_str.trim());
            }
            continue;
        }

        if let Some(resp) = handle_mcp_request(&state, &exec_tasks, req).await {
            println!("{}", serde_json::to_string(&resp)?);
        }
    }
    Ok(())
}

/// 把一条请求转发给常驻主进程。返回 `Some(body)` 仅当对方回了一个可用的 JSON-RPC 响应
/// （HTTP 200 + 含 `jsonrpc` 字段），或 `Some("")` 表示通知类已被接收（202 无响应体）。
/// 其余情况（连不上、401/403、响应体不是 JSON-RPC）一律返回 `None`，让调用方回退到进程内处理。
async fn try_forward_to_daemon(port: u16, api_key: &str, raw_json: &str) -> Option<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let addr = format!("127.0.0.1:{port}");
    let mut stream = tokio::time::timeout(
        Duration::from_millis(80),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .ok()?
    .ok()?;

    let req_bytes = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAuthorization: Bearer {api_key}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        raw_json.len(),
        raw_json
    );
    stream.write_all(req_bytes.as_bytes()).await.ok()?;
    let mut resp = Vec::new();
    stream.read_to_end(&mut resp).await.ok()?;
    let resp_str = String::from_utf8_lossy(&resp);
    let (head, body) = resp_str.split_once("\r\n\r\n")?;
    let status_line = head.lines().next().unwrap_or("");
    if status_line.contains(" 202 ") {
        // 通知类请求：主进程已接收、本就不产生响应体
        return Some(String::new());
    }
    if !status_line.contains(" 200 ") {
        return None;
    }
    let body = body.trim();
    if !body.contains("\"jsonrpc\"") {
        return None;
    }
    Some(body.to_string())
}

// ==================== 自动写入 Claude Code (~/.claude.json) 与 Codex (~/.codex/config.toml) ====================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSyncStatus {
    pub exe_path: String,
    pub port: u16,
    pub env: std::collections::BTreeMap<String, String>,
    pub claude_code_path: String,
    pub claude_code_synced: bool,
    pub codex_path: String,
    pub codex_synced: bool,
}

fn user_home_dir() -> Option<std::path::PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(std::path::PathBuf::from)
}

pub fn sync_cli_mcp_configs(port: u16) -> CliSyncStatus {
    let exe_path = std::env::current_exe()
        .ok()
        .map(|p| {
            let s = p.to_string_lossy().to_string();
            s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
        })
        .unwrap_or_else(|| "vps-switch".to_string());

    let home = user_home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let claude_path = home.join(".claude.json");
    let codex_path = home.join(".codex").join("config.toml");

    let data_dir = std::path::absolute(crate::resolve_data_dir())
        .unwrap_or_else(|_| crate::resolve_data_dir());
    let env = std::collections::BTreeMap::from([
        ("PORT".to_string(), port.to_string()),
        (
            "DATA_DIR".to_string(),
            data_dir.to_string_lossy().to_string(),
        ),
    ]);
    let claude_ok = sync_claude_json(&claude_path, &exe_path, &env).unwrap_or(false);
    let codex_ok = sync_codex_toml(&codex_path, &exe_path, &env).unwrap_or(false);

    CliSyncStatus {
        exe_path,
        port,
        env,
        claude_code_path: claude_path.to_string_lossy().to_string(),
        claude_code_synced: claude_ok,
        codex_path: codex_path.to_string_lossy().to_string(),
        codex_synced: codex_ok,
    }
}

fn sync_claude_json(
    claude_path: &std::path::Path,
    exe_path: &str,
    env: &std::collections::BTreeMap<String, String>,
) -> Result<bool> {
    if !claude_path.exists() {
        return Ok(false);
    }
    let raw = std::fs::read_to_string(claude_path)?;
    let mut root: Value = serde_json::from_str(&raw)?;
    let Some(obj) = root.as_object_mut() else {
        return Ok(false);
    };

    let desired = json!({
        "type": "stdio",
        "command": exe_path,
        "args": ["--mcp-stdio"],
        "env": env,
    });

    let mcp_servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
    if let Some(servers_obj) = mcp_servers.as_object_mut() {
        if servers_obj.get("vps-switch") == Some(&desired) {
            return Ok(true);
        }
        servers_obj.insert("vps-switch".to_string(), desired);
    }

    let tmp_path = claude_path.with_extension("json.vps-switch-tmp");
    std::fs::write(&tmp_path, serde_json::to_string_pretty(&root)?)?;
    std::fs::rename(&tmp_path, claude_path)?;
    Ok(true)
}

fn sync_codex_toml(
    codex_path: &std::path::Path,
    exe_path: &str,
    env: &std::collections::BTreeMap<String, String>,
) -> Result<bool> {
    if !codex_path.exists() {
        return Ok(false);
    }
    let raw = std::fs::read_to_string(codex_path)?;
    let posix_exe = exe_path.replace('\\', "/");
    let desired_block = format!(
        "[mcp_servers.vps-switch]\ncommand = {}\nargs = [\"--mcp-stdio\"]\nenv = {{ DATA_DIR = {}, PORT = {} }}",
        serde_json::to_string(&posix_exe)?,
        serde_json::to_string(&env["DATA_DIR"] )?,
        serde_json::to_string(&env["PORT"] )?,
    );

    let updated = if let Some(start_idx) = raw.find("[mcp_servers.vps-switch]") {
        let after_header = start_idx + "[mcp_servers.vps-switch]".len();
        let next_section_rel = raw[after_header..].find("\n[");
        match next_section_rel {
            Some(rel) => {
                let end_idx = after_header + rel;
                format!("{}{}{}", &raw[..start_idx], desired_block, &raw[end_idx..])
            }
            None => format!("{}{}\n", &raw[..start_idx], desired_block),
        }
    } else {
        let mut s = raw;
        if !s.ends_with('\n') {
            s.push('\n');
        }
        s.push('\n');
        s.push_str(&desired_block);
        s.push('\n');
        s
    };

    let tmp_path = codex_path.with_extension("toml.vps-switch-tmp");
    if updated == std::fs::read_to_string(codex_path)? {
        return Ok(true);
    }
    std::fs::write(&tmp_path, updated)?;
    std::fs::rename(&tmp_path, codex_path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_pid_header_handles_split_frames_and_preserves_command_stderr() {
        let mut buffer = Vec::new();
        let mut pending = true;
        let mut pid = None;
        let prefix = "\u{1e}vps-exec:test-id:";
        assert!(consume_remote_pid(
            &mut buffer,
            &mut pending,
            &mut pid,
            prefix,
            b"startup warning\n\x1evps-exec:test-id:12"
        )
        .is_empty());
        let output = consume_remote_pid(
            &mut buffer,
            &mut pending,
            &mut pid,
            prefix,
            b"34\x1fcommand stderr\n",
        );
        assert_eq!(pid, Some(1234));
        assert!(!pending);
        assert_eq!(output, b"startup warning\ncommand stderr\n");
    }

    #[test]
    fn invalid_remote_pid_does_not_remove_stderr_or_target_a_process_group() {
        let mut buffer = Vec::new();
        let mut pending = true;
        let mut pid = None;
        let prefix = "\u{1e}vps-exec:test-id:";
        let mut raw = b"\x1evps-exec:test-id:0\x1f".to_vec();
        raw.extend(vec![b'A'; 4096]);
        let output = consume_remote_pid(&mut buffer, &mut pending, &mut pid, prefix, &raw);
        assert_eq!(output, raw);
        assert!(pid.is_none());
        assert!(!pending);
    }

    #[test]
    fn output_limit_reports_loss_at_exact_chunk_boundary() {
        let mut output = vec![b'A'; MAX_OUTPUT_BYTES];
        append_capped(&mut output, &[]);
        assert_eq!(output.len(), MAX_OUTPUT_BYTES);
        append_capped(&mut output, b"lost bytes");
        assert!(String::from_utf8_lossy(&output).ends_with("[输出超过 1 MiB，已截断]"));
        let marked_size = output.len();
        append_capped(&mut output, b"more lost bytes");
        assert_eq!(output.len(), marked_size);
    }

    #[test]
    fn output_limit_marks_crossing_chunk_and_preserves_short_output() {
        let mut output = "完整 output".as_bytes().to_vec();
        append_capped(&mut output, b"\n");
        assert_eq!(String::from_utf8(output).unwrap(), "完整 output\n");
        let mut output = vec![b'A'; MAX_OUTPUT_BYTES - 2];
        append_capped(&mut output, b"BCDE");
        assert_eq!(&output[MAX_OUTPUT_BYTES - 2..MAX_OUTPUT_BYTES], b"BC");
        assert!(output.len() > MAX_OUTPUT_BYTES);
    }

    #[test]
    fn cli_configs_keep_daemon_environment_and_neighbor_entries() {
        let dir = std::env::temp_dir().join(format!("vps-switch-config-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let claude = dir.join("claude.json");
        let codex = dir.join("codex.toml");
        std::fs::write(&claude, r#"{"mcpServers":{"other":{"command":"keep"}}}"#).unwrap();
        std::fs::write(&codex, "model = \"keep\"\n[mcp_servers.vps-switch]\ncommand = \"old\"\nargs = []\n[mcp_servers.other]\ncommand = \"keep\"\n").unwrap();
        let env = std::collections::BTreeMap::from([
            ("PORT".to_string(), "18881".to_string()),
            ("DATA_DIR".to_string(), "C:\\数据\\quoted\"dir".to_string()),
        ]);
        sync_claude_json(&claude, "C:\\tools\\vps-switch.exe", &env).unwrap();
        sync_codex_toml(&codex, "C:\\tools\\vps-switch.exe", &env).unwrap();
        let json: Value = serde_json::from_str(&std::fs::read_to_string(&claude).unwrap()).unwrap();
        assert_eq!(json["mcpServers"]["other"]["command"], "keep");
        assert_eq!(json["mcpServers"]["vps-switch"]["env"], json!(env));
        let first = std::fs::read_to_string(&codex).unwrap();
        assert!(first.contains("PORT = \"18881\""));
        assert!(first.contains(&serde_json::to_string(&env["DATA_DIR"]).unwrap()));
        assert!(first.contains("[mcp_servers.other]\ncommand = \"keep\""));
        sync_codex_toml(&codex, "C:\\tools\\vps-switch.exe", &env).unwrap();
        assert_eq!(std::fs::read_to_string(&codex).unwrap(), first);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_host_enforces_mcp_exposed_and_hides_unexposed_from_available_list() {
        use crate::crypto::MasterKey;
        use crate::models::{HostMetaPatch, HostUpsertPayload};

        let dir = std::env::temp_dir().join(format!("vps-switch-mcp-expose-{}", Uuid::new_v4()));
        let key = MasterKey::load_or_create(&dir).unwrap();
        let db = Database::open(&dir, key).unwrap();

        let h1 = db
            .create_host(HostUpsertPayload {
                name: "hk-prod".into(),
                addr: "203.0.113.10".into(),
                port: 22,
                username: "root".into(),
                auth_type: "password".into(),
                password: None,
                private_key: None,
                passphrase: None,
                jump_host_id: None,
                role: Some("primary".into()),
                tags: vec![],
                links: vec![],
                pinned: false,
                favorited: false,
                archived: false,
                mcp_exposed: true,
            })
            .unwrap();
        let h2 = db
            .create_host(HostUpsertPayload {
                name: "hk-secret".into(),
                addr: "203.0.113.99".into(),
                port: 22,
                username: "root".into(),
                auth_type: "password".into(),
                password: None,
                private_key: None,
                passphrase: None,
                jump_host_id: None,
                role: Some("primary".into()),
                tags: vec![],
                links: vec![],
                pinned: false,
                favorited: false,
                archived: false,
                mcp_exposed: false,
            })
            .unwrap();

        assert!(h1.mcp_exposed);
        assert!(!h2.mcp_exposed);

        // 1. 已暴露的 hk-prod 可以正常解析，且子串 "hk" 只命中唯一的已暴露主机 hk-prod
        assert_eq!(resolve_host(&db, "hk-prod").unwrap().id, h1.id);
        assert_eq!(resolve_host(&db, "hk").unwrap().id, h1.id);

        // 2. 未暴露的 hk-secret 无论是按名称、IP 还是唯一子串 "secret" 访问，均明确拒绝
        let err_name = resolve_host(&db, "hk-secret").unwrap_err().to_string();
        assert!(err_name.contains("未在 MCP 中暴露"));
        let err_ip = resolve_host(&db, "203.0.113.99").unwrap_err().to_string();
        assert!(err_ip.contains("未在 MCP 中暴露"));
        let err_sub = resolve_host(&db, "secret").unwrap_err().to_string();
        assert!(err_sub.contains("未在 MCP 中暴露"));

        // 3. 查询不存在的主机时，报错列出的可用主机名绝不能泄露未暴露的 hk-secret
        let err_missing = resolve_host(&db, "non-existent").unwrap_err().to_string();
        assert!(err_missing.contains("hk-prod"));
        assert!(!err_missing.contains("hk-secret"));

        // 4. patch_meta 切换本机不暴露后，resolve_host("local") 同样被拦截
        db.patch_meta(
            LOCAL_HOST_ID,
            HostMetaPatch {
                pinned: None,
                favorited: None,
                archived: None,
                mcp_exposed: Some(false),
                role: None,
                tags: None,
                links: None,
            },
        )
        .unwrap();
        assert!(resolve_host(&db, "local").is_err());

        // 5. 批量全选恢复暴露
        db.set_mcp_exposed_batch(true, None).unwrap();
        assert!(resolve_host(&db, "local").is_ok());
        assert_eq!(resolve_host(&db, "hk-secret").unwrap().id, h2.id);

        // 6. 工具开放范围：默认全部 7 个工具开启；设置为仅开放 vps_list/vps_inspect 后，tools/list 严格只返回 2 个
        let default_tools = db.get_enabled_mcp_tools();
        assert_eq!(default_tools.len(), 7);
        assert_eq!(
            mcp_tool_definitions_filtered(&default_tools)
                .as_array()
                .unwrap()
                .len(),
            7
        );
        let saved_tools = db
            .set_enabled_mcp_tools(&[
                "vps_list".into(),
                "vps_inspect".into(),
                "invalid_tool".into(),
            ])
            .unwrap();
        assert_eq!(saved_tools, vec!["vps_list", "vps_inspect"]);
        let filtered_defs = mcp_tool_definitions_filtered(&saved_tools);
        let names: Vec<&str> = filtered_defs
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(names, vec!["vps_list", "vps_inspect"]);

        drop(db);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
