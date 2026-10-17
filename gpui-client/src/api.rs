//! 守护进程的 HTTP 客户端。
//!
//! 网络跑在专用线程的 current_thread tokio 运行时上；UI 线程只通过两个
//! 通道说话：请求用 `async_channel` 发进去，事件用 `std::sync::mpsc` 收回来，
//! 由 Workspace 的定时器批量 drain（与 shell-rs 的终端引擎同一种模式——
//! 后台线程永远不直接唤醒 GPUI 的 executor）。

use std::sync::mpsc;

/// `/api/hosts` 里客户端需要的字段子集。
#[derive(Clone, Debug)]
pub struct HostSummary {
    pub id: String,
    pub name: String,
    pub addr: String,
    /// `local` 或 `ssh`。
    pub kind: String,
    pub os: Option<String>,
    pub pretty_name: Option<String>,
    pub pinned: bool,
    pub favorited: bool,
    pub latency_ms: Option<i64>,
    pub last_error: Option<String>,
}

/// 网络层交给 UI 的事件。
pub enum ApiEvent {
    Hosts(Vec<HostSummary>),
    Failed(String),
}

enum Request {
    LoadHosts,
}

pub struct Api {
    requests: async_channel::Sender<Request>,
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
        let (request_tx, request_rx) = async_channel::unbounded::<Request>();
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
                    while let Ok(request) = request_rx.recv().await {
                        let event = match request {
                            Request::LoadHosts => fetch_hosts(&client).await,
                        };
                        if event_tx.send(event).is_err() {
                            break;
                        }
                    }
                });
            })
            .expect("api thread");
        Self {
            requests: request_tx,
            events: event_rx,
        }
    }

    pub fn load_hosts(&self) {
        let _ = self.requests.send_blocking(Request::LoadHosts);
    }

    pub fn try_recv(&self) -> Option<ApiEvent> {
        self.events.try_recv().ok()
    }
}

async fn fetch_hosts(client: &reqwest::Client) -> ApiEvent {
    let url = format!("{}/api/hosts", base_url());
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(error) => return ApiEvent::Failed(format!("连接守护进程失败：{error}")),
    };
    let response = match response.error_for_status() {
        Ok(response) => response,
        Err(error) => return ApiEvent::Failed(format!("守护进程返回错误：{error}")),
    };
    let body: serde_json::Value = match response.json().await {
        Ok(body) => body,
        Err(error) => return ApiEvent::Failed(format!("解析响应失败：{error}")),
    };
    let Some(items) = body.get("hosts").and_then(|hosts| hosts.as_array()) else {
        return ApiEvent::Failed("响应缺少 hosts 字段".into());
    };
    ApiEvent::Hosts(items.iter().filter_map(host_from_json).collect())
}

fn host_from_json(value: &serde_json::Value) -> Option<HostSummary> {
    let field = |name: &str| value.get(name).and_then(|item| item.as_str());
    let os_info = value.get("osInfo");
    Some(HostSummary {
        id: field("id")?.to_owned(),
        name: field("name")?.to_owned(),
        addr: field("addr").unwrap_or_default().to_owned(),
        kind: field("kind").unwrap_or("ssh").to_owned(),
        os: os_info
            .and_then(|info| info.get("os"))
            .and_then(|item| item.as_str())
            .map(str::to_owned),
        pretty_name: os_info
            .and_then(|info| info.get("prettyName"))
            .and_then(|item| item.as_str())
            .map(str::to_owned),
        pinned: value.get("pinned").and_then(|item| item.as_bool()).unwrap_or(false),
        favorited: value
            .get("favorited")
            .and_then(|item| item.as_bool())
            .unwrap_or(false),
        latency_ms: value.get("latencyMs").and_then(|item| item.as_i64()),
        last_error: field("lastError").map(str::to_owned),
    })
}
