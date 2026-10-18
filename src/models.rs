use serde::{Deserialize, Serialize};

pub const LOCAL_HOST_ID: &str = "local";
pub const LOCAL_PROXY_JUMP_ID: &str = "local-proxy";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HostLink {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OsInfo {
    pub os: Option<String>,
    pub distro_id: Option<String>,
    pub version_id: Option<String>,
    pub pretty_name: Option<String>,
    pub arch: Option<String>,
    pub kernel: Option<String>,
    pub cpu_model: Option<String>,
    pub cpu_cores: Option<u32>,
    pub detected_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HostRecord {
    pub id: String,
    pub name: String,
    pub kind: String, // "local" | "ssh"
    pub addr: String,
    pub port: u16,
    pub username: String,
    pub auth_type: String, // "local" | "password" | "privateKey"
    pub encrypted_password: Option<String>,
    pub encrypted_private_key: Option<String>,
    pub encrypted_passphrase: Option<String>,
    pub jump_host_id: Option<String>,
    pub role: Option<String>,
    pub tags: Vec<String>,
    pub links: Vec<HostLink>,
    pub os_info: Option<OsInfo>,
    pub pinned: bool,
    pub favorited: bool,
    pub archived: bool,
    pub sort_order: i64,
    pub status: String, // "online" | "offline" | "unknown"
    pub latency_ms: Option<i64>,
    pub last_connected_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicHost {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub addr: String,
    pub port: u16,
    pub username: String,
    pub auth_type: String,
    pub has_password: bool,
    pub has_private_key: bool,
    pub has_passphrase: bool,
    pub jump_host_id: Option<String>,
    pub jump_host_name: Option<String>,
    pub role: Option<String>,
    pub tags: Vec<String>,
    pub links: Vec<HostLink>,
    pub os_info: Option<OsInfo>,
    pub pinned: bool,
    pub favorited: bool,
    pub archived: bool,
    pub sort_order: i64,
    pub status: String,
    pub latency_ms: Option<i64>,
    pub last_connected_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostUpsertPayload {
    pub name: String,
    #[serde(default)]
    pub addr: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_username")]
    pub username: String,
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
    pub jump_host_id: Option<String>,
    pub role: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<HostLink>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub favorited: bool,
    #[serde(default)]
    pub archived: bool,
}

fn default_port() -> u16 {
    22
}
fn default_username() -> String {
    "root".to_string()
}
fn default_auth_type() -> String {
    "password".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostMetaPatch {
    pub pinned: Option<bool>,
    pub favorited: Option<bool>,
    pub archived: Option<bool>,
    pub role: Option<String>,
    pub tags: Option<Vec<String>>,
    pub links: Option<Vec<HostLink>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderPayload {
    pub ordered_ids: Vec<String>,
}
