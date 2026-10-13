#![cfg_attr(
    all(windows, not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod api;
mod crypto;
mod db;
#[cfg(windows)]
mod desktop;
mod files;
mod inspector;
mod mcp;
mod models;
mod ssh;
mod terminal;

use anyhow::Result;
use api::{build_api_router, AppState};
use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use crypto::MasterKey;
use db::Database;
use files::TransferManager;
use inspector::InspectorEngine;
use mcp::{run_stdio_bridge, sync_cli_mcp_configs, ExecTaskManager};
use rust_embed::RustEmbed;
use ssh::SshPool;
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use terminal::TerminalManager;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
struct EmbeddedFrontend;

fn resolve_data_dir() -> PathBuf {
    let cwd_data = PathBuf::from("data");
    if cwd_data.join("vps-switch.db").exists() {
        return cwd_data;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let exe_data = dir.join("data");
            if exe_data.join("vps-switch.db").exists() {
                return exe_data;
            }
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("vps-switch").join("data");
    }
    cwd_data
}

/// 静态前端资源服务：优先读本机 frontend/dist（方便开发热更新），不存在时直接走二进制内嵌的 EmbeddedFrontend（单文件打包免外部依赖）
async fn serve_frontend_asset(uri: Uri) -> Response {
    let raw_path = uri.path().trim_start_matches('/');
    let rel_path = if raw_path.is_empty() {
        "index.html"
    } else {
        raw_path
    };

    let disk_candidate = PathBuf::from("frontend/dist").join(rel_path);
    if let Ok(bytes) = tokio::fs::read(&disk_candidate).await {
        let mime = mime_guess::from_path(rel_path).first_or_octet_stream();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(bytes))
            .unwrap();
    }

    if let Some(content) = EmbeddedFrontend::get(rel_path) {
        let mime = mime_guess::from_path(rel_path).first_or_octet_stream();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(content.data.into_owned()))
            .unwrap();
    }

    if let Some(index) = EmbeddedFrontend::get("index.html") {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(index.data.into_owned()))
            .unwrap();
    }

    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

fn try_wakeup_existing_instance(port: u16) -> bool {
    use std::io::{Read, Write};
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(120)) else {
        return false;
    };
    let req = format!(
        "POST /api/window/show HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 128];
    let _ = stream.read(&mut buf);
    true
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let is_mcp_stdio = args.iter().any(|a| a == "--mcp-stdio" || a == "mcp");
    let is_headless = args.iter().any(|a| a == "--headless" || a == "--server");

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(18800);

    // 桌面模式下若已有实例在运行，直接唤醒已有主窗口并退出
    #[cfg(windows)]
    if !is_mcp_stdio && !is_headless && try_wakeup_existing_instance(port) {
        return Ok(());
    }

    if !is_mcp_stdio {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "vps_switch=info,tower_http=warn".into()),
            )
            .init();
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let data_dir = resolve_data_dir();
    let master_key = MasterKey::load_or_create(&data_dir)?;
    let db = Arc::new(Database::open(&data_dir, master_key)?);

    if db.count_ssh_hosts().unwrap_or(0) == 0 {
        match db.import_from_1shell() {
            Ok(n) if n > 0 && !is_mcp_stdio => info!("已自动从 1Shell 导入 {n} 台 VPS 主机"),
            _ => {}
        }
    }

    let exec_tasks = Arc::new(ExecTaskManager::new());
    let state = Arc::new(AppState {
        db,
        ssh_pool: Arc::new(SshPool::new()),
        terminals: Arc::new(TerminalManager::new()),
        transfers: Arc::new(TransferManager::new()),
        inspector: Arc::new(InspectorEngine::new()),
        exec_tasks: exec_tasks.clone(),
        sessions: std::sync::Mutex::new(std::collections::HashSet::new()),
        port,
    });

    if is_mcp_stdio {
        return rt.block_on(run_stdio_bridge(state, exec_tasks, port));
    }

    let sync_status = sync_cli_mcp_configs(port);
    info!(
        "MCP 自动配置同步: Claude Code={}, Codex={}",
        sync_status.claude_code_synced, sync_status.codex_synced
    );

    // 启动时后台并发执行全机队快照预热
    {
        let bg_state = state.clone();
        rt.spawn(async move {
            use futures::stream::{self, StreamExt};
            if let Ok(records) = bg_state.db.list_records() {
                let active: Vec<_> = records.into_iter().filter(|r| !r.archived).collect();
                stream::iter(active)
                    .for_each_concurrent(8, |rec| {
                        let st = bg_state.clone();
                        async move {
                            let _ = st
                                .inspector
                                .inspect_host(&st.ssh_pool, &st.db, &rec.id)
                                .await;
                        }
                    })
                    .await;
            }
        });
    }

    let bind_ip: std::net::IpAddr = std::env::var("HOST")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));
    let addr = SocketAddr::new(bind_ip, port);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = build_api_router(state.clone())
        .fallback(serve_frontend_asset)
        .layer(cors);

    #[cfg(windows)]
    if !is_headless {
        let rt_handle = rt.handle().clone();
        rt.spawn(async move {
            if let Ok(listener) = tokio::net::TcpListener::bind(addr).await {
                info!("vps-switch desktop server listening on http://{addr}");
                let _ = axum::serve(listener, app).await;
            }
        });
        return desktop::run_desktop_window(state, rt_handle, port);
    }

    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        info!("vps-switch headless server listening on http://{addr}");
        axum::serve(listener, app).await?;
        Ok::<(), anyhow::Error>(())
    })
}
