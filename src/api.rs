use crate::{
    db::Database,
    files::{
        self, join_remote_path, TransferManager,
    },
    inspector::InspectorEngine,
    mcp::{handle_mcp_request, sync_cli_mcp_configs, ExecTaskManager, JsonRpcRequest},
    models::{HostMetaPatch, HostUpsertPayload, ReorderPayload, LOCAL_HOST_ID},
    ssh::SshPool,
    terminal::{PaneBroadcast, PaneCommand, PaneHandle, TerminalManager},
};
use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Path, Query, Request, State,
    },
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use futures::{
    sink::SinkExt,
    stream::{self, StreamExt},
    TryStreamExt,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::io::AsyncReadExt;
use tokio_util::io::StreamReader;
use uuid::Uuid;

pub struct AppState {
    pub db: Arc<Database>,
    pub ssh_pool: Arc<SshPool>,
    pub terminals: Arc<TerminalManager>,
    pub transfers: Arc<TransferManager>,
    pub inspector: Arc<InspectorEngine>,
    pub exec_tasks: Arc<ExecTaskManager>,
    pub sessions: Mutex<HashSet<String>>,
    pub port: u16,
}

pub fn build_api_router(state: Arc<AppState>) -> Router {
    let protected_api = Router::new()
        .route("/api/mcp/status", get(api_mcp_status).post(api_mcp_sync))
        .route("/api/settings", get(api_get_settings))
        .route("/api/settings/auth", put(api_update_auth_settings))
        .route("/api/settings/mcp", put(api_update_mcp_settings))
        .route("/api/events", get(api_sse_events))
        .route("/api/hosts", get(list_hosts).post(create_host))
        .route("/api/hosts/reorder", put(reorder_hosts))
        .route("/api/hosts/import-1shell", post(import_from_1shell))
        .route("/api/hosts/check-all", post(check_all_hosts))
        .route("/api/hosts/:id", put(update_host).delete(delete_host))
        .route("/api/hosts/:id/meta", patch(patch_host_meta))
        .route("/api/hosts/:id/check", post(check_single_host))
        .route("/api/hosts/:id/inspect", get(inspect_single_host))
        .route("/api/terminals", get(list_terminals).post(create_terminal))
        .route("/api/terminals/:pane_id", delete(close_terminal))
        .route("/api/ws/terminal/:pane_id", get(ws_terminal_handler))
        .route("/api/files/list", get(api_list_dir))
        .route("/api/files/prewarm", post(api_prewarm_files))
        .route("/api/files/read", get(api_read_file))
        .route("/api/files/write", post(api_write_file))
        .route("/api/files/mkdir", post(api_mkdir))
        .route("/api/files/rename", post(api_rename_item))
        .route("/api/files/remove", post(api_remove_item))
        .route("/api/files/compress", post(api_compress_item))
        .route("/api/files/extract", post(api_extract_item))
        .route(
            "/api/files/upload",
            post(api_stream_upload).layer(DefaultBodyLimit::disable()),
        )
        .route("/api/files/download", get(api_stream_download))
        .route(
            "/api/transfers",
            get(api_list_transfers)
                .post(api_create_local_remote_transfer)
                .delete(api_clear_transfers),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_web_auth,
        ));

    Router::new()
        .route("/mcp", post(api_mcp_rpc))
        .route("/api/mcp", post(api_mcp_rpc))
        .route("/api/window/show", post(api_show_window))
        .route("/api/window/open-terminal", post(api_open_terminal_window))
        .route("/api/auth/status", get(api_auth_status))
        .route("/api/auth/login", post(api_auth_login))
        .route("/api/auth/logout", post(api_auth_logout))
        .merge(protected_api)
        .with_state(state)
}

#[derive(Deserialize)]
struct OpenTerminalWindowPayload {
    #[serde(alias = "hostId")]
    host_id: String,
    #[serde(alias = "paneId")]
    pane_id: Option<String>,
}

async fn api_open_terminal_window(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<OpenTerminalWindowPayload>,
) -> impl IntoResponse {
    state
        .terminals
        .notify_open_standalone_terminal(&payload.host_id, payload.pane_id.as_deref());
    (StatusCode::OK, Json(json!({ "ok": true })))
}

async fn api_show_window(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.terminals.notify_show_main_window();
    (StatusCode::OK, Json(json!({ "ok": true })))
}

fn err_response(status: StatusCode, msg: impl ToString) -> impl IntoResponse {
    (
        status,
        Json(json!({
            "ok": false,
            "error": msg.to_string(),
        })),
    )
}

async fn list_hosts(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.list_public_hosts() {
        Ok(hosts) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "hosts": hosts,
                "inspections": state.inspector.get_all_cached(),
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn create_host(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HostUpsertPayload>,
) -> impl IntoResponse {
    match state.db.create_host(payload) {
        Ok(host) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "host": host,
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, e).into_response(),
    }
}

async fn update_host(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<HostUpsertPayload>,
) -> impl IntoResponse {
    state.ssh_pool.invalidate_host(&id).await;
    match state.db.update_host(&id, payload) {
        Ok(host) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "host": host,
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, e).into_response(),
    }
}

async fn patch_host_meta(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(patch): Json<HostMetaPatch>,
) -> impl IntoResponse {
    match state.db.patch_meta(&id, patch) {
        Ok(host) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "host": host,
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, e).into_response(),
    }
}

async fn reorder_hosts(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ReorderPayload>,
) -> impl IntoResponse {
    match state.db.reorder(&payload.ordered_ids) {
        Ok(()) => match state.db.list_public_hosts() {
            Ok(hosts) => (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "hosts": hosts,
                })),
            )
                .into_response(),
            Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        },
        Err(e) => err_response(StatusCode::BAD_REQUEST, e).into_response(),
    }
}

async fn delete_host(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    state.ssh_pool.invalidate_host(&id).await;
    match state.db.delete_host(&id) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, e).into_response(),
    }
}

async fn import_from_1shell(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.import_from_1shell() {
        Ok(imported) => match state.db.list_public_hosts() {
            Ok(hosts) => (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "imported": imported,
                    "hosts": hosts,
                })),
            )
                .into_response(),
            Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        },
        Err(e) => err_response(StatusCode::BAD_REQUEST, e).into_response(),
    }
}

async fn check_single_host(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let inspection = state
        .inspector
        .inspect_host(&state.ssh_pool, &state.db, &id)
        .await
        .ok();
    match state.db.get_public_host(&id) {
        Ok(Some(host)) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "host": host,
                "inspection": inspection,
            })),
        )
            .into_response(),
        Ok(None) => err_response(StatusCode::NOT_FOUND, "主机不存在").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn inspect_single_host(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state
        .inspector
        .inspect_host(&state.ssh_pool, &state.db, &id)
        .await
    {
        Ok(inspection) => {
            let host = state.db.get_public_host(&id).ok().flatten();
            (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "inspection": inspection,
                    "host": host,
                })),
            )
                .into_response()
        }
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

async fn check_all_hosts(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let records = match state.db.list_records() {
        Ok(r) => r,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };

    let active_hosts: Vec<_> = records.into_iter().filter(|r| !r.archived).collect();

    let db = state.db.clone();
    let pool = state.ssh_pool.clone();
    let inspector = state.inspector.clone();
    stream::iter(active_hosts)
        .for_each_concurrent(8, |record| {
            let db = db.clone();
            let pool = pool.clone();
            let inspector = inspector.clone();
            async move {
                let _ = inspector.inspect_host(&pool, &db, &record.id).await;
            }
        })
        .await;

    match state.db.list_public_hosts() {
        Ok(hosts) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "hosts": hosts,
                "inspections": state.inspector.get_all_cached(),
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ==================== 板块二：终端 API 与二进制 WebSocket ====================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListTerminalsQuery {
    host_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTerminalPayload {
    host_id: String,
    #[serde(default = "default_cols")]
    cols: u16,
    #[serde(default = "default_rows")]
    rows: u16,
    cwd: Option<String>,
}

fn default_cols() -> u16 {
    100
}
fn default_rows() -> u16 {
    30
}

async fn list_terminals(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListTerminalsQuery>,
) -> impl IntoResponse {
    let panes = state
        .terminals
        .list_panes(query.host_id.as_deref(), false);
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "panes": panes,
        })),
    )
}

async fn create_terminal(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateTerminalPayload>,
) -> impl IntoResponse {
    match state
        .terminals
        .create_pane(
            &state.ssh_pool,
            &state.db,
            &payload.host_id,
            payload.cols,
            payload.rows,
            payload.cwd,
            false,
        )
        .await
    {
        Ok(pane) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "pane": pane,
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

async fn close_terminal(
    State(state): State<Arc<AppState>>,
    Path(pane_id): Path<String>,
) -> impl IntoResponse {
    let closed = state.terminals.close_pane(&pane_id);
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "closed": closed,
        })),
    )
}

async fn ws_terminal_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(pane_id): Path<String>,
) -> impl IntoResponse {
    let Some(pane) = state.terminals.get_pane(&pane_id) else {
        return err_response(StatusCode::NOT_FOUND, "终端 Pane 不存在或已关闭").into_response();
    };
    ws.on_upgrade(move |socket| handle_terminal_ws(socket, pane))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum WsClientControl {
    Resize { cols: u16, rows: u16 },
    Paste { text: String },
}

async fn handle_terminal_ws(socket: WebSocket, pane: Arc<PaneHandle>) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut bcast_rx = pane.broadcast_tx.subscribe();

    let replay = pane.replay_bytes();
    if !replay.is_empty() {
        let mut frame = Vec::with_capacity(1 + replay.len());
        frame.push(0x02);
        frame.extend_from_slice(&replay);
        if ws_tx.send(Message::Binary(frame)).await.is_err() {
            return;
        }
    }

    let meta_msg = json!({
        "type": "meta",
        "pane": pane.snapshot_meta(),
    })
    .to_string();
    if ws_tx.send(Message::Text(meta_msg)).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            bcast = bcast_rx.recv() => {
                match bcast {
                    Ok(PaneBroadcast::Output(chunk)) => {
                        let mut frame = Vec::with_capacity(1 + chunk.len());
                        frame.push(0x00);
                        frame.extend_from_slice(&chunk);
                        if ws_tx.send(Message::Binary(frame)).await.is_err() {
                            break;
                        }
                    }
                    Ok(PaneBroadcast::Meta(meta)) => {
                        let text = json!({
                            "type": "meta",
                            "pane": meta,
                        }).to_string();
                        if ws_tx.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let snapshot = pane.replay_bytes();
                        let mut frame = Vec::with_capacity(1 + snapshot.len());
                        frame.push(0x02);
                        frame.extend_from_slice(&snapshot);
                        if ws_tx.send(Message::Binary(frame)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = ws_rx.next() => {
                match incoming {
                    Some(Ok(Message::Binary(data))) => {
                        if data.first() == Some(&0x01) && data.len() > 1 {
                            let _ = pane.cmd_tx.send(PaneCommand::Input(data[1..].to_vec()));
                        } else if !data.is_empty() {
                            let _ = pane.cmd_tx.send(PaneCommand::Input(data));
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(ctrl) = serde_json::from_str::<WsClientControl>(&text) {
                            match ctrl {
                                WsClientControl::Resize { cols, rows } => {
                                    let _ = pane.cmd_tx.send(PaneCommand::Resize(cols, rows));
                                }
                                WsClientControl::Paste { text } => {
                                    let _ = pane.cmd_tx.send(PaneCommand::Paste(text));
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {}
                }
            }
        }
    }
}

// ==================== 板块三 & 四：文件浏览与满速流式传输 API ====================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileQuery {
    host_id: String,
    path: Option<String>,
    #[serde(default)]
    refresh: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrewarmPayload {
    host_id: String,
    cwd: Option<String>,
}

async fn api_prewarm_files(
    State(state): State<Arc<AppState>>,
    Json(p): Json<PrewarmPayload>,
) -> impl IntoResponse {
    let pool = state.ssh_pool.clone();
    let db = state.db.clone();
    let host_id = p.host_id.clone();
    let cwd = p.cwd.clone();
    tokio::spawn(async move {
        if let Ok(listing) = files::list_dir(&pool, &db, &host_id, cwd.as_deref(), false).await {
            if let Some(ref parent) = listing.parent {
                let _ = files::list_dir(&pool, &db, &host_id, Some(parent), false).await;
            }
        }
    });
    (StatusCode::OK, Json(json!({ "ok": true })))
}

async fn api_list_dir(
    State(state): State<Arc<AppState>>,
    Query(q): Query<FileQuery>,
) -> impl IntoResponse {
    match files::list_dir(
        &state.ssh_pool,
        &state.db,
        &q.host_id,
        q.path.as_deref(),
        q.refresh,
    )
    .await
    {
        Ok(listing) => {
            // 后台自动预热上级目录与前 6 个子目录，让后续点击子目录或返回上级 0ms 瞬开
            if q.host_id != LOCAL_HOST_ID {
                let pool = state.ssh_pool.clone();
                let db = state.db.clone();
                let host_id = q.host_id.clone();
                let parent = listing.parent.clone();
                let subdirs: Vec<String> = listing
                    .items
                    .iter()
                    .filter(|i| i.is_dir && !i.name.starts_with('.'))
                    .take(6)
                    .map(|i| i.path.clone())
                    .collect();
                tokio::spawn(async move {
                    if let Some(p) = parent {
                        let _ = files::list_dir(&pool, &db, &host_id, Some(&p), false).await;
                    }
                    for sub in subdirs {
                        let _ = files::list_dir(&pool, &db, &host_id, Some(&sub), false).await;
                    }
                });
            }

            (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "path": listing.path,
                    "parent": listing.parent,
                    "items": listing.items,
                })),
            )
                .into_response()
        }
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

async fn api_read_file(
    State(state): State<Arc<AppState>>,
    Query(q): Query<FileQuery>,
) -> impl IntoResponse {
    let Some(path) = q.path.as_deref().filter(|s| !s.trim().is_empty()) else {
        return err_response(StatusCode::BAD_REQUEST, "缺少文件路径").into_response();
    };
    match files::read_text_file(&state.ssh_pool, &state.db, &q.host_id, path).await {
        Ok(res) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "path": res.path,
                "content": res.content,
                "size": res.size,
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteFilePayload {
    host_id: String,
    path: String,
    content: String,
}

async fn api_write_file(
    State(state): State<Arc<AppState>>,
    Json(p): Json<WriteFilePayload>,
) -> impl IntoResponse {
    match files::write_text_file(&state.ssh_pool, &state.db, &p.host_id, &p.path, &p.content).await
    {
        Ok(size) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "path": p.path,
                "size": size,
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MkdirPayload {
    host_id: String,
    path: String,
}

async fn api_mkdir(
    State(state): State<Arc<AppState>>,
    Json(p): Json<MkdirPayload>,
) -> impl IntoResponse {
    match files::create_dir(&state.ssh_pool, &state.db, &p.host_id, &p.path).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenamePayload {
    host_id: String,
    old_path: String,
    new_path: String,
}

async fn api_rename_item(
    State(state): State<Arc<AppState>>,
    Json(p): Json<RenamePayload>,
) -> impl IntoResponse {
    match files::rename_item(&state.ssh_pool, &state.db, &p.host_id, &p.old_path, &p.new_path).await
    {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemovePayload {
    host_id: String,
    path: String,
    #[serde(default)]
    is_dir: bool,
}

async fn api_remove_item(
    State(state): State<Arc<AppState>>,
    Json(p): Json<RemovePayload>,
) -> impl IntoResponse {
    match files::remove_item(&state.ssh_pool, &state.db, &p.host_id, &p.path, p.is_dir).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveActionPayload {
    host_id: String,
    path: String,
}

async fn api_compress_item(
    State(state): State<Arc<AppState>>,
    Json(p): Json<ArchiveActionPayload>,
) -> impl IntoResponse {
    match files::compress_item(&state.ssh_pool, &state.db, &p.host_id, &p.path).await {
        Ok(archive_path) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "path": archive_path,
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

async fn api_extract_item(
    State(state): State<Arc<AppState>>,
    Json(p): Json<ArchiveActionPayload>,
) -> impl IntoResponse {
    match files::extract_item(&state.ssh_pool, &state.db, &p.host_id, &p.path).await {
        Ok(out_dir) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "path": out_dir,
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadQuery {
    host_id: String,
    dir_path: String,
    filename: String,
    #[serde(default)]
    size: u64,
}

/// 浏览器直传：纯二进制 Body 流直写 SFTP 64 路并发流水线（零 multipart 解析损耗）
async fn api_stream_upload(
    State(state): State<Arc<AppState>>,
    Query(q): Query<UploadQuery>,
    body: Body,
) -> impl IntoResponse {
    let host_name = state
        .db
        .get_record(&q.host_id)
        .ok()
        .flatten()
        .map(|h| h.name)
        .unwrap_or_else(|| q.host_id.clone());

    let remote_target = join_remote_path(&q.dir_path, &q.filename);
    let task_id = state.transfers.create_task(
        &q.host_id,
        &host_name,
        "upload",
        &q.filename,
        None,
        &remote_target,
        q.size,
    );

    let byte_stream = body
        .into_data_stream()
        .map_err(|err| std::io::Error::other(err.to_string()));
    let reader = StreamReader::new(byte_stream);

    match files::stream_upload_to_host(
        &state.ssh_pool,
        &state.db,
        &state.transfers,
        &task_id,
        &q.host_id,
        &q.dir_path,
        &q.filename,
        q.size,
        reader,
    )
    .await
    {
        Ok(final_path) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "taskId": task_id,
                "path": final_path,
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

/// 浏览器高速流式下载：直接把 64 路并发 SFTP 预读流作为 HTTP 响应输出，同时实时更新传输队列速率
async fn api_stream_download(
    State(state): State<Arc<AppState>>,
    Query(q): Query<FileQuery>,
) -> Response {
    let Some(file_path) = q.path.filter(|s| !s.trim().is_empty()) else {
        return err_response(StatusCode::BAD_REQUEST, "缺少下载路径").into_response();
    };

    let host_name = state
        .db
        .get_record(&q.host_id)
        .ok()
        .flatten()
        .map(|h| h.name)
        .unwrap_or_else(|| q.host_id.clone());

    let filename = file_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("download.bin")
        .to_string();

    if q.host_id == LOCAL_HOST_ID {
        let meta = match tokio::fs::metadata(&file_path).await {
            Ok(m) => m,
            Err(e) => return err_response(StatusCode::NOT_FOUND, e).into_response(),
        };
        let file = match tokio::fs::File::open(&file_path).await {
            Ok(f) => f,
            Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        };
        let stream = tokio_util::io::ReaderStream::with_capacity(file, 512 * 1024);
        return build_download_response(&filename, meta.len(), Body::from_stream(stream));
    }

    let sftp = match state.ssh_pool.get_or_open_sftp(&state.db, &q.host_id).await {
        Ok(s) => s,
        Err(e) => return err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    };

    let meta = match sftp.metadata(&file_path).await {
        Ok(m) => m,
        Err(e) => return err_response(StatusCode::NOT_FOUND, format!("{e:#}")).into_response(),
    };
    if meta.is_dir() {
        return err_response(StatusCode::BAD_REQUEST, "不能直接下载文件夹").into_response();
    }
    let total_size = meta.size.unwrap_or(0);

    let remote_file = match sftp.open(&file_path).await {
        Ok(f) => f,
        Err(e) => return err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    };

    let task_id = state.transfers.create_task(
        &q.host_id,
        &host_name,
        "download",
        &filename,
        None,
        &file_path,
        total_size,
    );

    let transfers = state.transfers.clone();
    let started = Instant::now();

    let stream = futures::stream::unfold(
        (remote_file, 0u64, task_id, transfers, false),
        move |(mut rfile, mut transferred, tid, tm, done)| async move {
            if done {
                return None;
            }
            let mut buf = vec![0u8; 512 * 1024];
            match rfile.read(&mut buf).await {
                Ok(0) => {
                    let _ = rfile.close().await;
                    tm.finish_task(&tid, transferred, started.elapsed().as_secs_f64());
                    None
                }
                Ok(n) => {
                    buf.truncate(n);
                    transferred += n as u64;
                    tm.update_progress(
                        &tid,
                        transferred,
                        total_size,
                        started.elapsed().as_secs_f64(),
                    );
                    Some((
                        Ok::<bytes::Bytes, std::io::Error>(bytes::Bytes::from(buf)),
                        (rfile, transferred, tid, tm, false),
                    ))
                }
                Err(err) => {
                    tm.fail_task(&tid, &err);
                    Some((Err(err), (rfile, transferred, tid, tm, true)))
                }
            }
        },
    );

    build_download_response(&filename, total_size, Body::from_stream(stream))
}

fn build_download_response(filename: &str, size: u64, body: Body) -> Response {
    let safe_ascii: String = filename
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let disposition = format!("attachment; filename=\"{safe_ascii}\"");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, size.to_string())
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(body)
        .unwrap()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListTransfersQuery {
    host_id: Option<String>,
}

async fn api_list_transfers(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListTransfersQuery>,
) -> impl IntoResponse {
    let tasks = state.transfers.list_tasks(q.host_id.as_deref());
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "tasks": tasks,
        })),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalRemoteTransferPayload {
    host_id: String,
    direction: String, // "upload" | "download"
    local_path: String,
    remote_path: String,
}

async fn api_create_local_remote_transfer(
    State(state): State<Arc<AppState>>,
    Json(p): Json<LocalRemoteTransferPayload>,
) -> impl IntoResponse {
    match files::transfer_local_remote_file(
        &state.ssh_pool,
        &state.db,
        &state.transfers,
        &p.host_id,
        &p.direction,
        &p.local_path,
        &p.remote_path,
    )
    .await
    {
        Ok(task) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "task": task,
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

async fn api_clear_transfers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.transfers.clear_finished();
    (StatusCode::OK, Json(json!({ "ok": true })))
}

// ==================== 登录鉴权、设置页与 MCP JSON-RPC 2.0 端点 ====================

const SESSION_COOKIE_NAME: &str = "vps_switch_session";

fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    if let Some(auth) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(tok) = auth.strip_prefix("Bearer ") {
            if !tok.trim().is_empty() {
                return Some(tok.trim().to_string());
            }
        }
    }
    if let Some(cookie_hdr) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in cookie_hdr.split(';') {
            let trimmed = part.trim();
            if let Some(val) = trimmed.strip_prefix(&format!("{SESSION_COOKIE_NAME}=")) {
                if !val.trim().is_empty() {
                    return Some(val.trim().to_string());
                }
            }
        }
    }
    None
}

fn is_request_authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    let auth_enabled = state
        .db
        .get_setting("auth_enabled")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    if !auth_enabled {
        return true;
    }
    let Some(token) = extract_session_token(headers) else {
        return false;
    };
    state.sessions.lock().unwrap().contains(&token)
}

async fn require_web_auth(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    if !is_request_authenticated(&state, req.headers()) {
        return err_response(StatusCode::UNAUTHORIZED, "请先登录").into_response();
    }
    next.run(req).await
}

async fn api_auth_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let settings = state.db.get_app_settings().ok();
    let auth_enabled = settings.as_ref().map(|s| s.auth_enabled).unwrap_or(false);
    let username = settings
        .as_ref()
        .map(|s| s.admin_username.clone())
        .unwrap_or_else(|| "admin".to_string());
    let authenticated = is_request_authenticated(&state, &headers);
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "authEnabled": auth_enabled,
            "authenticated": authenticated,
            "username": username,
        })),
    )
}

#[derive(Debug, Deserialize)]
struct LoginPayload {
    username: String,
    password: String,
}

async fn api_auth_login(
    State(state): State<Arc<AppState>>,
    Json(p): Json<LoginPayload>,
) -> Response {
    match state
        .db
        .verify_admin_credentials(&p.username, &p.password)
    {
        Ok(true) => {
            let token = format!("sess_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
            state.sessions.lock().unwrap().insert(token.clone());
            let cookie = format!(
                "{SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000"
            );
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::SET_COOKIE, cookie)
                .body(Body::from(
                    json!({
                        "ok": true,
                        "token": token,
                        "username": p.username.trim(),
                    })
                    .to_string(),
                ))
                .unwrap()
        }
        Ok(false) => err_response(StatusCode::UNAUTHORIZED, "账号或密码错误").into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn api_auth_logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    if let Some(tok) = extract_session_token(&headers) {
        state.sessions.lock().unwrap().remove(&tok);
    }
    let clear_cookie = format!("{SESSION_COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::SET_COOKIE, clear_cookie)
        .body(Body::from(json!({ "ok": true }).to_string()))
        .unwrap()
}

async fn api_get_settings(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.get_app_settings() {
        Ok(settings) => {
            let cli_sync = sync_cli_mcp_configs(state.port);
            (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "settings": settings,
                    "cliSync": cli_sync,
                })),
            )
                .into_response()
        }
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAuthSettingsPayload {
    auth_enabled: bool,
    username: String,
    password: Option<String>,
}

async fn api_update_auth_settings(
    State(state): State<Arc<AppState>>,
    Json(p): Json<UpdateAuthSettingsPayload>,
) -> Response {
    match state
        .db
        .update_auth_settings(p.auth_enabled, &p.username, p.password.as_deref())
    {
        Ok(settings) => {
            // 若刚开启了登录保护，自动为当前管理员签发会话 Cookie，避免改完设置立刻被踢出
            let token = format!("sess_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
            state.sessions.lock().unwrap().insert(token.clone());
            let cookie = format!(
                "{SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000"
            );
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::SET_COOKIE, cookie)
                .body(Body::from(
                    json!({
                        "ok": true,
                        "settings": settings,
                    })
                    .to_string(),
                ))
                .unwrap()
        }
        Err(e) => err_response(StatusCode::BAD_REQUEST, e).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMcpSettingsPayload {
    mcp_enabled: bool,
    mcp_require_api_key: bool,
    #[serde(default)]
    regenerate_key: bool,
}

async fn api_update_mcp_settings(
    State(state): State<Arc<AppState>>,
    Json(p): Json<UpdateMcpSettingsPayload>,
) -> impl IntoResponse {
    match state.db.update_mcp_settings(
        p.mcp_enabled,
        p.mcp_require_api_key,
        p.regenerate_key,
    ) {
        Ok(settings) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "settings": settings,
            })),
        )
            .into_response(),
        Err(e) => err_response(StatusCode::BAD_REQUEST, e).into_response(),
    }
}

fn extract_mcp_api_key(headers: &HeaderMap) -> Option<String> {
    if let Some(auth) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(k) = auth.strip_prefix("Bearer ") {
            if !k.trim().is_empty() {
                return Some(k.trim().to_string());
            }
        }
    }
    if let Some(k) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        if !k.trim().is_empty() {
            return Some(k.trim().to_string());
        }
    }
    None
}

async fn api_mcp_rpc(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<JsonRpcRequest>,
) -> Response {
    let Ok(settings) = state.db.get_app_settings() else {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, "读取设置失败").into_response();
    };

    if !settings.mcp_enabled {
        return err_response(StatusCode::FORBIDDEN, "MCP Server 当前已在设置中关闭").into_response();
    }

    // 本机 --mcp-stdio 内部桥接请求带 x-vps-switch-stdio: 1；其余外部 HTTP 请求若开启了 API Key 校验则必须匹配
    let is_local_stdio = headers
        .get("x-vps-switch-stdio")
        .and_then(|v| v.to_str().ok())
        == Some("1");
    if settings.mcp_require_api_key && !is_local_stdio {
        let provided = extract_mcp_api_key(&headers);
        if provided.as_deref() != Some(settings.mcp_api_key.as_str()) {
            return err_response(
                StatusCode::UNAUTHORIZED,
                "MCP API Key 无效或缺失（请在 Authorization: Bearer <API_KEY> 或 X-API-Key 请求头中提供）",
            )
            .into_response();
        }
    }

    match handle_mcp_request(&state, &state.exec_tasks, req).await {
        Some(val) => (StatusCode::OK, Json(val)).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn api_mcp_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status = sync_cli_mcp_configs(state.port);
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "status": status,
        })),
    )
}

async fn api_mcp_sync(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status = sync_cli_mcp_configs(state.port);
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "status": status,
        })),
    )
}

async fn api_sse_events(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let rx = state.terminals.subscribe_ui_events();
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let payload = serde_json::to_string(&ev).unwrap_or_default();
                    return Some((
                        Ok::<SseEvent, std::convert::Infallible>(SseEvent::default().data(payload)),
                        rx,
                    ));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}

