//! 守护进程的 HTTP 客户端与数据模型。
//!
//! 网络跑在专用线程的 current_thread tokio 运行时上；UI 线程只通过两个
//! 通道说话：命令用 `async_channel` 发进去，事件用 `std::sync::mpsc` 收回来，
//! 由 Workspace 的定时器批量 drain。

use std::collections::HashMap;
use std::sync::mpsc;

/// `/api/hosts` 里的一台主机（字段与旧前端 `PublicHost` 对齐）。
#[derive(Clone, Debug)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub addr: String,
    pub port: u16,
    pub username: String,
    pub kind: String,
    pub status: String,
    pub os: Option<String>,
    pub pretty_name: Option<String>,
    pub distro_id: Option<String>,
    pub kernel: Option<String>,
    pub arch: Option<String>,
    pub cpu_cores: Option<u32>,
    pub cpu_model: Option<String>,
    pub pinned: bool,
    pub favorited: bool,
    pub archived: bool,
    pub role: Option<String>,
    pub tags: Vec<String>,
    pub jump_host_name: Option<String>,
    pub latency_ms: Option<i64>,
    pub last_error: Option<String>,
}

/// 探针快照。
#[derive(Clone, Debug, Default)]
pub struct Inspection {
    pub cpu_cores: Option<u32>,
    pub cpu_usage_pct: f64,
    pub cpu_iowait_pct: f64,
    pub cpu_steal_pct: f64,
    pub cpu_model: Option<String>,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub mem_total_bytes: f64,
    pub mem_used_bytes: f64,
    pub mem_cached_bytes: f64,
    pub swap_total_bytes: f64,
    pub swap_used_bytes: f64,
    pub disk_mount: Option<String>,
    pub disk_total_bytes: f64,
    pub disk_used_bytes: f64,
    pub disks: Vec<DiskUsage>,
    pub net_iface: Option<String>,
    pub net_rx_bps: f64,
    pub net_tx_bps: f64,
    pub net_rx_bytes_total: f64,
    pub net_tx_bytes_total: f64,
    pub uptime_seconds: f64,
    pub tcp_cc: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct DiskUsage {
    pub mount: String,
    pub total_bytes: f64,
    pub used_bytes: f64,
}

/// 网络吞吐历史（每台主机保留 20 个采样）。
pub type NetHistory = HashMap<String, Vec<(f64, f64)>>;

/// 网络/解析线程交给 UI 的事件。
pub enum ApiEvent {
    Hosts(Vec<Host>),
    HostsFailed(String),
    Inspection {
        host_id: String,
        inspection: Inspection,
        host: Option<Host>,
    },
    CheckedAll(Vec<Host>),
    HostUpdated(Host),
    Failed(String),
    /// 通用 JSON 响应（元数据 PATCH 等）。
    Json(serde_json::Value),
}

/// UI 交给网络线程的命令。
pub enum ApiCommand {
    LoadHosts,
    Inspect(String),
    CheckAll,
    CheckHost(String),
    PatchMeta { host_id: String, patch: serde_json::Value },
    Post { path: String, body: serde_json::Value },
}

pub struct Api {
    commands: async_channel::Sender<ApiCommand>,
    events: mpsc::Receiver<ApiEvent>,
}

/// 守护进程地址。本机优先定位下固定回环；端口留环境变量覆盖以便隔离测试。
pub fn base_url() -> String {
    format!("http://127.0.0.1:{}", port())
}

pub fn port() -> String {
    std::env::var("VPS_SWITCH_PORT").unwrap_or_else(|_| "18800".into())
}

impl Api {
    pub fn spawn() -> Self {
        let (command_tx, command_rx) = async_channel::unbounded::<ApiCommand>();
        let (event_tx, event_rx) = mpsc::channel::<ApiEvent>();
        std::thread::Builder::new()
            .name("vps-switch-api".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                runtime.block_on(async move {
                    let client = reqwest::Client::new();
                    while let Ok(command) = command_rx.recv().await {
                        let event = handle(&client, command).await;
                        if let Some(event) = event
                            && event_tx.send(event).is_err()
                        {
                            break;
                        }
                    }
                });
            })
            .expect("api thread");
        Self {
            commands: command_tx,
            events: event_rx,
        }
    }

    pub fn send(&self, command: ApiCommand) {
        let _ = self.commands.send_blocking(command);
    }

    pub fn try_recv(&self) -> Vec<ApiEvent> {
        self.events.try_iter().collect()
    }
}

async fn handle(client: &reqwest::Client, command: ApiCommand) -> Option<ApiEvent> {
    match command {
        ApiCommand::LoadHosts => match get_json(client, "/api/hosts").await {
            Ok(body) => {
                let hosts = hosts_from_json(&body);
                Some(ApiEvent::Hosts(hosts))
            }
            Err(error) => Some(ApiEvent::HostsFailed(error)),
        },
        ApiCommand::Inspect(host_id) => {
            let path = format!("/api/hosts/{}/inspect", encode(&host_id));
            match get_json(client, &path).await {
                Ok(body) => {
                    let inspection = body
                        .get("inspection")
                        .map(inspection_from_json)
                        .unwrap_or_default();
                    let host = body.get("host").and_then(host_from_json);
                    Some(ApiEvent::Inspection {
                        host_id,
                        inspection,
                        host,
                    })
                }
                Err(error) => Some(ApiEvent::Failed(error)),
            }
        }
        ApiCommand::CheckAll => match post_json(client, "/api/hosts/check-all", None).await {
            Ok(body) => Some(ApiEvent::CheckedAll(hosts_from_json(&body))),
            Err(error) => Some(ApiEvent::Failed(error)),
        },
        ApiCommand::CheckHost(host_id) => {
            let path = format!("/api/hosts/{}/check", encode(&host_id));
            match post_json(client, &path, None).await {
                Ok(body) => match body.get("host").and_then(host_from_json) {
                    Some(host) => Some(ApiEvent::HostUpdated(host)),
                    None => Some(ApiEvent::Failed("检测响应缺少主机".into())),
                },
                Err(error) => Some(ApiEvent::Failed(error)),
            }
        }
        ApiCommand::PatchMeta { host_id, patch } => {
            let path = format!("/api/hosts/{}/meta", encode(&host_id));
            match patch_json(client, &path, &patch).await {
                Ok(body) => match body.get("host").and_then(host_from_json) {
                    Some(host) => Some(ApiEvent::HostUpdated(host)),
                    None => Some(ApiEvent::Json(body)),
                },
                Err(error) => Some(ApiEvent::Failed(error)),
            }
        }
        ApiCommand::Post { path, body } => match post_json(client, &path, Some(&body)).await {
            Ok(body) => Some(ApiEvent::Json(body)),
            Err(error) => Some(ApiEvent::Failed(error)),
        },
    }
}

async fn get_json(client: &reqwest::Client, path: &str) -> Result<serde_json::Value, String> {
    let response = client
        .get(format!("{}{path}", base_url()))
        .send()
        .await
        .map_err(|error| format!("连接守护进程失败：{error}"))?;
    read_json(response).await
}

async fn post_json(
    client: &reqwest::Client,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let mut request = client.post(format!("{}{path}", base_url()));
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("连接守护进程失败：{error}"))?;
    read_json(response).await
}

async fn patch_json(
    client: &reqwest::Client,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let response = client
        .patch(format!("{}{path}", base_url()))
        .json(body)
        .send()
        .await
        .map_err(|error| format!("连接守护进程失败：{error}"))?;
    read_json(response).await
}

async fn read_json(response: reqwest::Response) -> Result<serde_json::Value, String> {
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("解析响应失败：{error}"))?;
    if !status.is_success() {
        let reason = body
            .get("error")
            .and_then(|error| error.as_str())
            .unwrap_or("请求失败");
        return Err(reason.to_owned());
    }
    Ok(body)
}

fn encode(value: &str) -> String {
    // host id 只含字母数字与 -_，直接拼接即可。
    value.to_owned()
}

pub fn hosts_from_json(body: &serde_json::Value) -> Vec<Host> {
    body.get("hosts")
        .and_then(|hosts| hosts.as_array())
        .map(|items| items.iter().filter_map(host_from_json).collect())
        .unwrap_or_default()
}

pub fn host_from_json(value: &serde_json::Value) -> Option<Host> {
    let field = |name: &str| value.get(name).and_then(|item| item.as_str());
    let os_info = value.get("osInfo");
    let os_field = |name: &str| {
        os_info
            .and_then(|info| info.get(name))
            .and_then(|item| item.as_str())
    };
    Some(Host {
        id: field("id")?.to_owned(),
        name: field("name")?.to_owned(),
        addr: field("addr").unwrap_or_default().to_owned(),
        port: value.get("port").and_then(|v| v.as_u64()).unwrap_or(22) as u16,
        username: field("username").unwrap_or_default().to_owned(),
        kind: field("kind").unwrap_or("ssh").to_owned(),
        status: field("status").unwrap_or("unknown").to_owned(),
        os: os_field("os").map(str::to_owned),
        pretty_name: os_field("prettyName").map(str::to_owned),
        distro_id: os_field("distroId").map(str::to_owned),
        kernel: os_field("kernel").map(str::to_owned),
        arch: os_field("arch").map(str::to_owned),
        cpu_cores: os_info
            .and_then(|info| info.get("cpuCores"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        cpu_model: os_field("cpuModel").map(str::to_owned),
        pinned: value.get("pinned").and_then(|v| v.as_bool()).unwrap_or(false),
        favorited: value
            .get("favorited")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        archived: value
            .get("archived")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        role: field("role").map(str::to_owned),
        tags: value
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        jump_host_name: field("jumpHostName").map(str::to_owned),
        latency_ms: value.get("latencyMs").and_then(|v| v.as_i64()),
        last_error: field("lastError").map(str::to_owned),
    })
}

fn inspection_from_json(value: &serde_json::Value) -> Inspection {
    let num = |name: &str| value.get(name).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let text = |name: &str| {
        value
            .get(name)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };
    Inspection {
        cpu_cores: value
            .get("cpuCores")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        cpu_usage_pct: num("cpuUsagePct"),
        cpu_iowait_pct: num("cpuIowaitPct"),
        cpu_steal_pct: num("cpuStealPct"),
        cpu_model: text("cpuModel"),
        load1: num("load1"),
        load5: num("load5"),
        load15: num("load15"),
        mem_total_bytes: num("memTotalBytes"),
        mem_used_bytes: num("memUsedBytes"),
        mem_cached_bytes: num("memCachedBytes"),
        swap_total_bytes: num("swapTotalBytes"),
        swap_used_bytes: num("swapUsedBytes"),
        disk_mount: text("diskMount"),
        disk_total_bytes: num("diskTotalBytes"),
        disk_used_bytes: num("diskUsedBytes"),
        disks: value
            .get("disks")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(DiskUsage {
                            mount: item.get("mount")?.as_str()?.to_owned(),
                            total_bytes: item.get("totalBytes")?.as_f64().unwrap_or(0.0),
                            used_bytes: item.get("usedBytes")?.as_f64().unwrap_or(0.0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        net_iface: text("netIface"),
        net_rx_bps: num("netRxBps"),
        net_tx_bps: num("netTxBps"),
        net_rx_bytes_total: num("netRxBytesTotal"),
        net_tx_bytes_total: num("netTxBytesTotal"),
        uptime_seconds: num("uptimeSeconds"),
        tcp_cc: text("tcpCc"),
    }
}
