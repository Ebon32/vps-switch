use crate::{
    db::Database,
    models::{OsInfo, LOCAL_HOST_ID},
    ssh::{exec_short_command, SshPool},
};
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::Instant,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageItem {
    pub mount: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostInspection {
    pub host_id: String,
    pub sampled_at: String,
    pub uptime_seconds: u64,
    pub load_1: f64,
    pub load_5: f64,
    pub load_15: f64,
    pub tcp_cc: Option<String>,
    pub cpu_model: Option<String>,
    pub cpu_cores: u32,
    pub cpu_usage_pct: f64,
    pub cpu_iowait_pct: f64,
    pub cpu_steal_pct: f64,
    pub mem_total_bytes: u64,
    pub mem_used_bytes: u64,
    pub mem_cached_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub disk_mount: String,
    pub disk_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disks: Vec<DiskUsageItem>,
    pub net_iface: String,
    pub net_rx_bytes_total: u64,
    pub net_tx_bytes_total: u64,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
}

#[derive(Clone, Copy)]
struct CpuTicks {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
    steal: u64,
}

impl CpuTicks {
    fn total(&self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
    }

    fn idle_all(&self) -> u64 {
        self.idle + self.iowait
    }
}

struct PrevHostSample {
    at: Instant,
    cpu: CpuTicks,
    net_iface: String,
    net_rx: u64,
    net_tx: u64,
    last_cpu_pct: f64,
    last_iowait_pct: f64,
    last_steal_pct: f64,
}

#[derive(Default)]
pub struct InspectorEngine {
    prev_samples: Mutex<HashMap<String, PrevHostSample>>,
    latest_snapshots: Mutex<HashMap<String, HostInspection>>,
}

/// 纯内核虚拟文件读取脚本：
/// 1. 开头先用纯 shell builtin `read < /proc/stat` 取样 CPU1，在读取其他内核文件后再取 CPU2，
///    保证即使首次点开也能通过真实 `/proc/stat` 差分算出准确 CPU%，绝不用容器里失真的 loadavg 估算；
/// 2. 磁盘统一使用 POSIX 标准 `df -Pk`（1024-byte blocks，兼容 Alpine BusyBox 与所有发行版）。
const LINUX_INSPECT_SCRIPT: &str = r#"export LC_ALL=C
read -r _ u1 n1 s1 i1 w1 q1 sq1 st1 _ < /proc/stat 2>/dev/null
CPU1="$u1 $n1 $s1 $i1 $w1 $q1 $sq1 $st1"
read -r UP _ < /proc/uptime 2>/dev/null || UP=0
read -r L1 L5 L15 _ < /proc/loadavg 2>/dev/null || { L1=0; L5=0; L15=0; }
CC=$(cat /proc/sys/net/ipv4/tcp_congestion_control 2>/dev/null || echo "")
IFACE=$(awk '$2 == "00000000" {print $1; exit}' /proc/net/route 2>/dev/null)
if [ -z "$IFACE" ]; then
  IFACE=$(awk -F: 'NR>2 {gsub(/^[ \t]+/,"",$1); if ($1 !~ /^(lo|docker|veth|br-|tun|tap|wg|tailscale|flannel|cni|virbr)/) {print $1; exit}}' /proc/net/dev 2>/dev/null)
fi
NET=$(awk -F'[: ]+' -v dev="$IFACE" '$0 ~ "^[ \t]*"dev":" {
  for(i=1;i<=NF;i++) if($i == dev) { print $(i+1), $(i+9); exit }
}' /proc/net/dev 2>/dev/null)
MEM=$(awk '
  /^MemTotal:/ {mt=$2}
  /^MemFree:/ {mf=$2}
  /^MemAvailable:/ {ma=$2}
  /^Buffers:/ {buf=$2}
  /^Cached:/ {ca=$2}
  /^SReclaimable:/ {sr=$2}
  /^SwapTotal:/ {st=$2}
  /^SwapFree:/ {sf=$2}
  END {
    avail = (ma > 0) ? ma : (mf + buf + ca);
    used = (mt > avail) ? (mt - avail) : 0;
    cache = buf + ca + sr;
    sw_used = (st > sf) ? (st - sf) : 0;
    printf "%.0f %.0f %.0f %.0f %.0f", mt*1024, used*1024, cache*1024, st*1024, sw_used*1024
  }
' /proc/meminfo 2>/dev/null)
DISKS=$(df -Pk 2>/dev/null | awk 'NR>1 && $1 !~ /^(tmpfs|devtmpfs|udev|none|shm|overlay)$/ && $6 !~ /^\/(dev|proc|sys|run|boot\/efi)/ {printf "%s:%.0f:%.0f|", $6, $2*1024, $3*1024}')
if [ -z "$DISKS" ]; then
  DISKS=$(df -Pk / 2>/dev/null | awk 'NR==2 {printf "%s:%.0f:%.0f|", $6, $2*1024, $3*1024}')
fi
MODEL=$(awk -F: '/model name|Model name/ {gsub(/^[ \t]+/,"",$2); print $2; exit}' /proc/cpuinfo 2>/dev/null)
CORES=$(grep -c '^processor' /proc/cpuinfo 2>/dev/null || echo 1)
if [ -f /etc/os-release ]; then . /etc/os-release; fi
UNAME_R=$(uname -r 2>/dev/null)
UNAME_M=$(uname -m 2>/dev/null)
sleep 0.12 2>/dev/null || true
read -r _ u2 n2 s2 i2 w2 q2 sq2 st2 _ < /proc/stat 2>/dev/null
CPU2="$u2 $n2 $s2 $i2 $w2 $q2 $sq2 $st2"
printf "UP=%s\nLOAD=%s %s %s\nCC=%s\nCPU1=%s\nCPU2=%s\nIFACE=%s\nNET=%s\nMEM=%s\nDISKS=%s\nMODEL=%s\nCORES=%s\nOS_ID=%s\nOS_VER=%s\nOS_PRETTY=%s\nKERNEL=%s\nARCH=%s\n" \
  "$UP" "$L1" "$L5" "$L15" "$CC" "$CPU1" "$CPU2" "$IFACE" "$NET" "$MEM" "$DISKS" "$MODEL" "$CORES" "${ID:-linux}" "${VERSION_ID:-}" "${PRETTY_NAME:-Linux}" "$UNAME_R" "$UNAME_M"
"#;

impl InspectorEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_all_cached(&self) -> HashMap<String, HostInspection> {
        self.latest_snapshots.lock().unwrap().clone()
    }

    pub async fn inspect_host(
        &self,
        pool: &SshPool,
        db: &Database,
        host_id: &str,
    ) -> Result<HostInspection> {
        if host_id == LOCAL_HOST_ID {
            let ins = self.inspect_local_windows().await?;
            self.latest_snapshots
                .lock()
                .unwrap()
                .insert(host_id.to_string(), ins.clone());
            return Ok(ins);
        }

        let record = db
            .get_record(host_id)?
            .ok_or_else(|| anyhow!("主机不存在: {host_id}"))?;
        let jump = match record.jump_host_id.as_deref() {
            Some(jid) => db.get_record(jid)?,
            None => None,
        };

        let started = Instant::now();
        let session = match pool
            .get_or_connect(&record, jump.as_ref(), db.master_key())
            .await
        {
            Ok(s) => s,
            Err(err) => {
                let _ = db.update_check_result(
                    host_id,
                    false,
                    None,
                    None,
                    Some(format!("{err:#}")),
                );
                return Err(err);
            }
        };

        let raw = match exec_short_command(&session, LINUX_INSPECT_SCRIPT).await {
            Ok(r) => r,
            Err(err) => {
                pool.invalidate_host(host_id).await;
                let _ = db.update_check_result(
                    host_id,
                    false,
                    None,
                    None,
                    Some(format!("{err:#}")),
                );
                return Err(err);
            }
        };
        let latency_ms = started.elapsed().as_millis() as i64;

        let (inspection, os_info) = self.parse_linux_inspection(host_id, &raw)?;
        let _ = db.update_check_result(host_id, true, Some(latency_ms), Some(os_info), None);
        self.latest_snapshots
            .lock()
            .unwrap()
            .insert(host_id.to_string(), inspection.clone());

        Ok(inspection)
    }

    fn parse_linux_inspection(
        &self,
        host_id: &str,
        raw: &str,
    ) -> Result<(HostInspection, OsInfo)> {
        let mut map = HashMap::new();
        for line in raw.lines() {
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim(), v.trim());
            }
        }

        let uptime_seconds = map
            .get("UP")
            .and_then(|s| s.split('.').next())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let mut load_parts = map
            .get("LOAD")
            .copied()
            .unwrap_or("0 0 0")
            .split_whitespace();
        let load_1 = load_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let load_5 = load_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let load_15 = load_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);

        let tcp_cc = map
            .get("CC")
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());

        let cpu1 = parse_cpu_ticks(map.get("CPU1").copied().unwrap_or(""));
        let cpu2 = parse_cpu_ticks(map.get("CPU2").copied().unwrap_or(""));

        let net_iface = map
            .get("IFACE")
            .copied()
            .filter(|s| !s.is_empty())
            .unwrap_or("eth0")
            .to_string();

        let mut net_parts = map.get("NET").copied().unwrap_or("0 0").split_whitespace();
        let net_rx = net_parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let net_tx = net_parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);

        let mut mem_parts = map
            .get("MEM")
            .copied()
            .unwrap_or("0 0 0 0 0")
            .split_whitespace();
        let mem_total_bytes = mem_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let mem_used_bytes = mem_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let mem_cached_bytes = mem_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let swap_total_bytes = mem_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let swap_used_bytes = mem_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);

        let mut disks = Vec::new();
        let mut seen_mounts = std::collections::HashSet::new();
        for item in map.get("DISKS").copied().unwrap_or("").split('|') {
            if item.trim().is_empty() {
                continue;
            }
            let mut p = item.split(':');
            if let (Some(mount), Some(tot), Some(used)) = (
                p.next(),
                p.next().and_then(|s| s.parse::<u64>().ok()),
                p.next().and_then(|s| s.parse::<u64>().ok()),
            ) {
                if tot > 0 && seen_mounts.insert(mount.to_string()) {
                    disks.push(DiskUsageItem {
                        mount: mount.to_string(),
                        total_bytes: tot,
                        used_bytes: used,
                    });
                }
            }
        }
        disks.sort_by(|a, b| {
            (a.mount != "/")
                .cmp(&(b.mount != "/"))
                .then_with(|| a.mount.cmp(&b.mount))
        });

        let primary_disk = disks.first().cloned().unwrap_or(DiskUsageItem {
            mount: "/".to_string(),
            total_bytes: 0,
            used_bytes: 0,
        });

        let cpu_model = map
            .get("MODEL")
            .copied()
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let cpu_cores = map
            .get("CORES")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1)
            .max(1);

        let now_inst = Instant::now();
        let (cpu_usage_pct, cpu_iowait_pct, cpu_steal_pct, net_rx_bps, net_tx_bps) = {
            let mut guard = self.prev_samples.lock().unwrap();
            if let Some(prev) = guard.get(host_id) {
                let elapsed = now_inst.duration_since(prev.at).as_secs_f64();
                // 若距离上次采样在 15 秒内，直接用跨周期的精确硬件计数差值；否则使用本次脚本内 CPU1->CPU2 的差值
                let base_cpu = if elapsed <= 15.0 { prev.cpu } else { cpu1 };
                let (cpu_pct, iowait_pct, steal_pct) = compute_cpu_diff(&base_cpu, &cpu2)
                    .unwrap_or((prev.last_cpu_pct, prev.last_iowait_pct, prev.last_steal_pct));

                let (rx_bps, tx_bps) = if elapsed > 0.2 && elapsed <= 15.0 && prev.net_iface == net_iface {
                    let drx = net_rx.saturating_sub(prev.net_rx);
                    let dtx = net_tx.saturating_sub(prev.net_tx);
                    ((drx as f64 / elapsed) as u64, (dtx as f64 / elapsed) as u64)
                } else {
                    (0, 0)
                };

                guard.insert(
                    host_id.to_string(),
                    PrevHostSample {
                        at: now_inst,
                        cpu: cpu2,
                        net_iface: net_iface.clone(),
                        net_rx,
                        net_tx,
                        last_cpu_pct: cpu_pct,
                        last_iowait_pct: iowait_pct,
                        last_steal_pct: steal_pct,
                    },
                );

                (cpu_pct, iowait_pct, steal_pct, rx_bps, tx_bps)
            } else {
                // 首次采样：直接用脚本内 CPU1 -> CPU2 的真实 /proc/stat 差值，绝不用 loadavg 瞎猜！
                let (cpu_pct, iowait_pct, steal_pct) =
                    compute_cpu_diff(&cpu1, &cpu2).unwrap_or((0.0, 0.0, 0.0));
                guard.insert(
                    host_id.to_string(),
                    PrevHostSample {
                        at: now_inst,
                        cpu: cpu2,
                        net_iface: net_iface.clone(),
                        net_rx,
                        net_tx,
                        last_cpu_pct: cpu_pct,
                        last_iowait_pct: iowait_pct,
                        last_steal_pct: steal_pct,
                    },
                );
                (cpu_pct, iowait_pct, steal_pct, 0, 0)
            }
        };

        let now_iso = Utc::now().to_rfc3339();
        let os_info = OsInfo {
            os: Some("linux".to_string()),
            distro_id: map.get("OS_ID").map(|s| s.to_lowercase()),
            version_id: map.get("OS_VER").filter(|s| !s.is_empty()).map(|s| s.to_string()),
            pretty_name: map.get("OS_PRETTY").filter(|s| !s.is_empty()).map(|s| s.to_string()),
            arch: map.get("ARCH").filter(|s| !s.is_empty()).map(|s| s.to_string()),
            kernel: map.get("KERNEL").filter(|s| !s.is_empty()).map(|s| s.to_string()),
            cpu_model: cpu_model.clone(),
            cpu_cores: Some(cpu_cores),
            detected_at: Some(now_iso.clone()),
        };

        let inspection = HostInspection {
            host_id: host_id.to_string(),
            sampled_at: now_iso,
            uptime_seconds,
            load_1,
            load_5,
            load_15,
            tcp_cc,
            cpu_model,
            cpu_cores,
            cpu_usage_pct: (cpu_usage_pct * 10.0).round() / 10.0,
            cpu_iowait_pct: (cpu_iowait_pct * 10.0).round() / 10.0,
            cpu_steal_pct: (cpu_steal_pct * 10.0).round() / 10.0,
            mem_total_bytes,
            mem_used_bytes,
            mem_cached_bytes,
            swap_total_bytes,
            swap_used_bytes,
            disk_mount: primary_disk.mount,
            disk_total_bytes: primary_disk.total_bytes,
            disk_used_bytes: primary_disk.used_bytes,
            disks,
            net_iface,
            net_rx_bytes_total: net_rx,
            net_tx_bytes_total: net_tx,
            net_rx_bps,
            net_tx_bps,
        };

        Ok((inspection, os_info))
    }

    async fn inspect_local_windows(&self) -> Result<HostInspection> {
        let ps_script = r#"
$os = Get-CimInstance Win32_OperatingSystem
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$disks = Get-CimInstance Win32_LogicalDisk -Filter "DriveType=3" | ForEach-Object { "$($_.DeviceID)/:$($_.Size):$($_.Size - $_.FreeSpace)" }
$net = Get-NetAdapterStatistics | Where-Object { $_.ReceivedBytes -gt 0 } | Sort-Object ReceivedBytes -Descending | Select-Object -First 1
$up = [int]((Get-Date) - $os.LastBootUpTime).TotalSeconds
$mt = [uint64]$os.TotalVisibleMemorySize * 1024
$mf = [uint64]$os.FreePhysicalMemory * 1024
$mu = if ($mt -gt $mf) { $mt - $mf } else { 0 }
Write-Output "UP=$up"
Write-Output "CPU_PCT=$($cpu.LoadPercentage)"
Write-Output "CPU_NAME=$($cpu.Name)"
Write-Output "CORES=$([Environment]::ProcessorCount)"
Write-Output "MEM=$mt $mu"
Write-Output "DISKS=$($disks -join '|')"
Write-Output "NET=$($net.Name)|$($net.ReceivedBytes)|$($net.SentBytes)"
"#;
        let mut cmd = tokio::process::Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", ps_script]);
        #[cfg(windows)]
        cmd.creation_flags(0x08000000);
        let out = cmd.output().await?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut map = HashMap::new();
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim(), v.trim());
            }
        }

        let uptime_seconds = map.get("UP").and_then(|s| s.parse().ok()).unwrap_or(0);
        let cpu_usage_pct = map.get("CPU_PCT").and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let cpu_model = map.get("CPU_NAME").map(|s| s.to_string());
        let cpu_cores = map.get("CORES").and_then(|s| s.parse().ok()).unwrap_or(8);

        let mut mem_it = map.get("MEM").copied().unwrap_or("0 0").split_whitespace();
        let mem_total_bytes = mem_it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let mem_used_bytes = mem_it.next().and_then(|s| s.parse().ok()).unwrap_or(0);

        let mut disks = Vec::new();
        for item in map.get("DISKS").copied().unwrap_or("").split('|') {
            if item.trim().is_empty() {
                continue;
            }
            let mut p = item.split(':');
            // Windows mount has C:/ so first two parts are "C" and "/"
            let parts: Vec<&str> = item.split(':').collect();
            if parts.len() >= 4 {
                let mount = format!("{}:{}", parts[0], parts[1]);
                let tot = parts[2].parse::<u64>().unwrap_or(0);
                let used = parts[3].parse::<u64>().unwrap_or(0);
                if tot > 0 {
                    disks.push(DiskUsageItem {
                        mount,
                        total_bytes: tot,
                        used_bytes: used,
                    });
                }
            } else if let (Some(m), Some(tot), Some(used)) = (
                p.next(),
                p.next().and_then(|s| s.parse::<u64>().ok()),
                p.next().and_then(|s| s.parse::<u64>().ok()),
            ) {
                if tot > 0 {
                    disks.push(DiskUsageItem {
                        mount: m.to_string(),
                        total_bytes: tot,
                        used_bytes: used,
                    });
                }
            }
        }
        let primary_disk = disks.first().cloned().unwrap_or(DiskUsageItem {
            mount: "C:/".to_string(),
            total_bytes: 0,
            used_bytes: 0,
        });

        let mut net_it = map.get("NET").copied().unwrap_or("Ethernet|0|0").split('|');
        let net_iface = net_it.next().unwrap_or("Ethernet").to_string();
        let net_rx: u64 = net_it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let net_tx: u64 = net_it.next().and_then(|s| s.parse().ok()).unwrap_or(0);

        let now_inst = Instant::now();
        let (net_rx_bps, net_tx_bps) = {
            let mut guard = self.prev_samples.lock().unwrap();
            let rates = if let Some(prev) = guard.get(LOCAL_HOST_ID) {
                let elapsed = now_inst.duration_since(prev.at).as_secs_f64();
                if elapsed > 0.2 {
                    (
                        (net_rx.saturating_sub(prev.net_rx) as f64 / elapsed) as u64,
                        (net_tx.saturating_sub(prev.net_tx) as f64 / elapsed) as u64,
                    )
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            };
            guard.insert(
                LOCAL_HOST_ID.to_string(),
                PrevHostSample {
                    at: now_inst,
                    cpu: CpuTicks {
                        user: 0,
                        nice: 0,
                        system: 0,
                        idle: 0,
                        iowait: 0,
                        irq: 0,
                        softirq: 0,
                        steal: 0,
                    },
                    net_iface: net_iface.clone(),
                    net_rx,
                    net_tx,
                    last_cpu_pct: cpu_usage_pct,
                    last_iowait_pct: 0.0,
                    last_steal_pct: 0.0,
                },
            );
            rates
        };

        Ok(HostInspection {
            host_id: LOCAL_HOST_ID.to_string(),
            sampled_at: Utc::now().to_rfc3339(),
            uptime_seconds,
            load_1: 0.0,
            load_5: 0.0,
            load_15: 0.0,
            tcp_cc: None,
            cpu_model,
            cpu_cores,
            cpu_usage_pct,
            cpu_iowait_pct: 0.0,
            cpu_steal_pct: 0.0,
            mem_total_bytes,
            mem_used_bytes,
            mem_cached_bytes: 0,
            swap_total_bytes: 0,
            swap_used_bytes: 0,
            disk_mount: primary_disk.mount,
            disk_total_bytes: primary_disk.total_bytes,
            disk_used_bytes: primary_disk.used_bytes,
            disks,
            net_iface,
            net_rx_bytes_total: net_rx,
            net_tx_bytes_total: net_tx,
            net_rx_bps,
            net_tx_bps,
        })
    }
}

fn compute_cpu_diff(a: &CpuTicks, b: &CpuTicks) -> Option<(f64, f64, f64)> {
    let total_delta = b.total().saturating_sub(a.total());
    if total_delta == 0 {
        return None;
    }
    let idle_delta = b.idle_all().saturating_sub(a.idle_all());
    let busy_delta = total_delta.saturating_sub(idle_delta);
    let io_delta = b.iowait.saturating_sub(a.iowait);
    let st_delta = b.steal.saturating_sub(a.steal);
    Some((
        ((busy_delta as f64 / total_delta as f64) * 100.0).clamp(0.0, 100.0),
        ((io_delta as f64 / total_delta as f64) * 100.0).clamp(0.0, 100.0),
        ((st_delta as f64 / total_delta as f64) * 100.0).clamp(0.0, 100.0),
    ))
}

fn parse_cpu_ticks(s: &str) -> CpuTicks {
    let mut it = s.split_whitespace().filter_map(|x| x.parse::<u64>().ok());
    CpuTicks {
        user: it.next().unwrap_or(0),
        nice: it.next().unwrap_or(0),
        system: it.next().unwrap_or(0),
        idle: it.next().unwrap_or(0),
        iowait: it.next().unwrap_or(0),
        irq: it.next().unwrap_or(0),
        softirq: it.next().unwrap_or(0),
        steal: it.next().unwrap_or(0),
    }
}
