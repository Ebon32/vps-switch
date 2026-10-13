use crate::{
    crypto::MasterKey,
    db::Database,
    models::HostRecord,
};
use anyhow::{anyhow, Context, Result};
use russh::{
    client::{self, AuthResult, Config, Handle, KeyboardInteractiveAuthResponse, Msg},
    keys::{decode_secret_key, key::PrivateKeyWithHashAlg, PublicKeyOrCertificate},
    Channel, ChannelMsg,
};
use russh_sftp::client::{Config as SftpConfig, SftpSession};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub struct SshClientHandler;

impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        Ok(true)
    }
}

/// 单主机 SSH 与 SFTP 连接池：
/// - 终端、执行、SFTP 文件浏览、高速上传下载、探针全部共用同一条底层 SSH 连接；
/// - 通道窗口拉大到 16 MiB + SFTP 64 路并发流水线（255 KiB 大包），彻底跑满跨境高 RTT 带宽。
#[derive(Default)]
pub struct SshPool {
    sessions: Mutex<HashMap<String, Arc<Handle<SshClientHandler>>>>,
    sftps: Mutex<HashMap<String, Arc<SftpSession>>>,
    home_dirs: Mutex<HashMap<String, String>>,
    dir_cache: Mutex<HashMap<(String, String), (Instant, crate::files::DirListing)>>,
}

impl SshPool {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            sftps: Mutex::new(HashMap::new()),
            home_dirs: Mutex::new(HashMap::new()),
            dir_cache: Mutex::new(HashMap::new()),
        }
    }

    pub async fn get_cached_dir(
        &self,
        host_id: &str,
        path: &str,
        max_age: Duration,
    ) -> Option<crate::files::DirListing> {
        let guard = self.dir_cache.lock().await;
        guard
            .get(&(host_id.to_string(), path.to_string()))
            .and_then(|(ts, listing)| {
                if ts.elapsed() <= max_age {
                    Some(listing.clone())
                } else {
                    None
                }
            })
    }

    pub async fn put_cached_dir(
        &self,
        host_id: &str,
        req_path: &str,
        listing: &crate::files::DirListing,
    ) {
        let mut guard = self.dir_cache.lock().await;
        let now = Instant::now();
        guard.insert(
            (host_id.to_string(), listing.path.clone()),
            (now, listing.clone()),
        );
        if req_path != listing.path {
            guard.insert(
                (host_id.to_string(), req_path.to_string()),
                (now, listing.clone()),
            );
        }
    }

    pub async fn invalidate_dir_cache(&self, host_id: &str) {
        let mut guard = self.dir_cache.lock().await;
        guard.retain(|(h, _), _| h != host_id);
    }

    pub async fn get_home_dir(&self, sftp: &SftpSession, host_id: &str) -> String {
        {
            let guard = self.home_dirs.lock().await;
            if let Some(h) = guard.get(host_id) {
                return h.clone();
            }
        }
        let resolved = sftp
            .canonicalize(".")
            .await
            .unwrap_or_else(|_| "/root".to_string());
        self.home_dirs
            .lock()
            .await
            .insert(host_id.to_string(), resolved.clone());
        resolved
    }

    pub async fn get_or_connect(
        &self,
        host: &HostRecord,
        jump_host: Option<&HostRecord>,
        master_key: &MasterKey,
    ) -> Result<Arc<Handle<SshClientHandler>>> {
        {
            let guard = self.sessions.lock().await;
            if let Some(existing) = guard.get(&host.id) {
                if !existing.is_closed() {
                    return Ok(existing.clone());
                }
            }
        }

        let handle = connect_single_or_jump(host, jump_host, master_key).await?;
        let arc_handle = Arc::new(handle);
        {
            let mut guard = self.sessions.lock().await;
            guard.insert(host.id.clone(), arc_handle.clone());
        }
        Ok(arc_handle)
    }

    pub async fn open_session_channel(
        &self,
        host: &HostRecord,
        jump_host: Option<&HostRecord>,
        master_key: &MasterKey,
    ) -> Result<Channel<Msg>> {
        let handle = self.get_or_connect(host, jump_host, master_key).await?;
        match handle.channel_open_session().await {
            Ok(ch) => Ok(ch),
            Err(_) => {
                self.invalidate_host(&host.id).await;
                let fresh = self.get_or_connect(host, jump_host, master_key).await?;
                fresh
                    .channel_open_session()
                    .await
                    .context("打开 SSH 会话通道失败")
            }
        }
    }

    pub async fn invalidate_host(&self, host_id: &str) {
        self.sftps.lock().await.remove(host_id);
        self.sessions.lock().await.remove(host_id);
    }

    pub async fn invalidate_sftp(&self, host_id: &str) {
        self.sftps.lock().await.remove(host_id);
    }

    pub async fn get_or_open_sftp(
        &self,
        db: &Database,
        host_id: &str,
    ) -> Result<Arc<SftpSession>> {
        {
            let guard = self.sftps.lock().await;
            if let Some(existing) = guard.get(host_id) {
                return Ok(existing.clone());
            }
        }

        let (_record, channel) = open_host_channel(self, db, host_id).await?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .context("请求 SFTP 子系统失败")?;

        // 高速调优：利用 OpenSSH limits@openssh.com (255KiB 单包) + 64 并发读写流水线 (16 MiB 在途窗口)
        let sftp_cfg = SftpConfig {
            max_packet_len: 1024 * 1024,
            max_concurrent_reads: 64,
            max_concurrent_writes: 64,
            max_write_packet_len: 261_120,
            request_timeout_secs: 60,
        };

        let sftp = SftpSession::new_with_config(channel.into_stream(), sftp_cfg)
            .await
            .context("初始化 SFTP 会话失败")?;
        let arc_sftp = Arc::new(sftp);
        {
            let mut guard = self.sftps.lock().await;
            guard.insert(host_id.to_string(), arc_sftp.clone());
        }
        Ok(arc_sftp)
    }
}

async fn authenticate_session(
    session: &mut Handle<SshClientHandler>,
    host: &HostRecord,
    master_key: &MasterKey,
) -> Result<()> {
    let user = if host.username.trim().is_empty() {
        "root"
    } else {
        host.username.trim()
    };

    if host.auth_type == "privateKey" {
        let key_pem = master_key
            .decrypt(host.encrypted_private_key.as_deref())?
            .ok_or_else(|| anyhow!("该主机未配置私钥"))?;
        let passphrase = master_key.decrypt(host.encrypted_passphrase.as_deref())?;
        let key_pair =
            decode_secret_key(&key_pem, passphrase.as_deref()).context("解析 SSH 私钥失败")?;
        let best_hash = session.best_supported_rsa_hash().await?.flatten();
        let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key_pair), best_hash);
        match session.authenticate_publickey(user, key_with_alg).await? {
            AuthResult::Success => return Ok(()),
            AuthResult::Failure { .. } => return Err(anyhow!("SSH 私钥认证被服务器拒绝")),
        }
    }

    let password = master_key
        .decrypt(host.encrypted_password.as_deref())?
        .ok_or_else(|| anyhow!("该主机未配置密码"))?;

    match session.authenticate_password(user, &password).await? {
        AuthResult::Success => return Ok(()),
        AuthResult::Failure { .. } => {}
    }

    let mut ki = session
        .authenticate_keyboard_interactive_start(user, None::<String>)
        .await?;
    for _ in 0..3 {
        match ki {
            KeyboardInteractiveAuthResponse::Success => return Ok(()),
            KeyboardInteractiveAuthResponse::Failure { .. } => {
                return Err(anyhow!("SSH 密码认证失败"));
            }
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let responses = prompts.iter().map(|_| password.clone()).collect();
                ki = session
                    .authenticate_keyboard_interactive_respond(responses)
                    .await?;
            }
        }
    }

    Err(anyhow!("SSH 认证未通过"))
}

pub async fn connect_single_or_jump(
    host: &HostRecord,
    jump_host: Option<&HostRecord>,
    master_key: &MasterKey,
) -> Result<Handle<SshClientHandler>> {
    let mut cfg = Config::default();
    // 关键性能优化：将 SSH Channel 窗口从默认 2MiB 提升至 16MiB，单包上限拉满 65535，
    // 彻底消除跨境高 RTT 线路上传下载时等待 SSH_MSG_CHANNEL_WINDOW_ADJUST 造成的断流限速
    cfg.window_size = 16 * 1024 * 1024;
    cfg.maximum_packet_size = 65535;
    cfg.channel_buffer_size = 1024;
    cfg.inactivity_timeout = Some(Duration::from_secs(3600));
    cfg.keepalive_interval = Some(Duration::from_secs(25));
    cfg.nodelay = true;
    let config = Arc::new(cfg);

    if let Some(jump) = jump_host {
        let jump_addr = format!("{}:{}", jump.addr.trim(), jump.port);
        let mut jump_session = tokio::time::timeout(
            Duration::from_secs(10),
            client::connect(config.clone(), jump_addr.as_str(), SshClientHandler),
        )
        .await
        .context("连接跳板机超时")?
        .context("连接跳板机失败")?;

        authenticate_session(&mut jump_session, jump, master_key)
            .await
            .context("跳板机认证失败")?;

        let direct_channel = tokio::time::timeout(
            Duration::from_secs(10),
            jump_session.channel_open_direct_tcpip(
                host.addr.trim(),
                host.port as u32,
                "127.0.0.1",
                0,
            ),
        )
        .await
        .context("通过跳板机建立隧道超时")?
        .context("通过跳板机建立隧道失败")?;

        let stream = direct_channel.into_stream();
        let mut target_session = tokio::time::timeout(
            Duration::from_secs(10),
            client::connect_stream(config, stream, SshClientHandler),
        )
        .await
        .context("目标主机 SSH 握手超时")?
        .context("目标主机 SSH 握手失败")?;

        authenticate_session(&mut target_session, host, master_key).await?;
        Ok(target_session)
    } else {
        let addr = format!("{}:{}", host.addr.trim(), host.port);
        let mut session = tokio::time::timeout(
            Duration::from_secs(8),
            client::connect(config, addr.as_str(), SshClientHandler),
        )
        .await
        .context("SSH 连接超时")?
        .context("SSH 连接失败")?;

        authenticate_session(&mut session, host, master_key).await?;
        Ok(session)
    }
}

pub async fn exec_short_command(session: &Handle<SshClientHandler>, cmd: &str) -> Result<String> {
    let mut channel = session.channel_open_session().await?;
    channel.exec(true, cmd).await?;
    let mut out = Vec::new();
    let collect = async {
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => out.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, .. } => out.extend_from_slice(&data),
                ChannelMsg::Eof | ChannelMsg::Close => break,
                _ => {}
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(5), collect).await;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

pub async fn open_host_channel(
    pool: &SshPool,
    db: &Database,
    host_id: &str,
) -> Result<(HostRecord, Channel<Msg>)> {
    let record = db
        .get_record(host_id)?
        .ok_or_else(|| anyhow!("主机不存在: {host_id}"))?;
    let jump = match record.jump_host_id.as_deref() {
        Some(jid) => db.get_record(jid)?,
        None => None,
    };
    let channel = pool
        .open_session_channel(&record, jump.as_ref(), db.master_key())
        .await?;
    let _ = db.update_check_result(&record.id, true, None, None, None);
    Ok((record, channel))
}
