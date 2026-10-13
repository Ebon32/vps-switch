//! 命令黑名单护栏。
//!
//! 只做一件事：MCP 的 agent 通道里，命中黑名单的命令**拒绝执行**。
//! 没有白名单、没有确认流程、没有风险分级、没有安全挡位——那些要么得靠 agent 自觉，
//! 要么是另一套系统。这里就是一份名单。
//!
//! 为什么需要：MCP 工具一旦被外部 agent 放行，参数里的命令字符串就不再被 agent 自身的
//! 权限系统检查（Claude Code 的 MCP 权限规则只按工具名匹配、不带参数内容）。所以
//! `vps_exec(host, "rm -rf /")` 在 agent 侧零提示，而同一条命令敲进它自己的 Bash 工具会被拦。
//!
//! 边界：**这份名单防手滑，不防蓄意绕过**。`r\m -rf /`、`$(echo rm) -rf /` 这类形态
//! 纯模式匹配必然漏——要堵住得把命令 tokenize 成 argv 再解命令替换，是另一个量级的东西。

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// 名单文件名，落在数据目录（与 vps-switch.db 同级）。
pub const DENY_LIST_FILE: &str = "deny-list.txt";

/// 首启写出的默认名单。用户可直接编辑该文件增删，改完立即生效（每次判定都重读）。
const DEFAULT_DENY_LIST: &str = r#"# vps-switch 命令黑名单
#
# 命中即拒绝执行，agent 拿不到任何执行结果。这里只放不可逆的灾难，正常运维不该出现。
#
# 写法同 Claude Code 的 Bash(...) 规则：
#   - 一行一条，字面文本 + * 通配（* 可放任意位置）；没有 * 就是精确匹配
#   - 按词边界匹配，所以 `rm -rf /` 不会误伤 `rm -rf /var/log`
#   - 匹配前统一归一化：剥引号、\/ 还原成 /、折叠空白、转小写、短选项排序
#     （故 -rf / -fr / -r -f / -f -r 四种写法等价，rm -rf "/" 也拦得住）
#
# 删掉任何一条即失去那条保护；在下面按同样写法加自己的。
# 这份名单防手滑，不防蓄意绕过（r\m -rf /、$(echo rm) -rf / 这类必然漏）。

# ── 删根与系统目录（递归删除不可逆）────────────────────────────
rm -rf /
rm -rf /etc
rm -rf /usr
rm -rf /bin
rm -rf /sbin
rm -rf /lib
rm -rf /lib64
rm -rf /boot
rm -rf /dev
rm -rf /proc
rm -rf /sys
rm -rf /root
rm --no-preserve-root

# ── 格式化与向块设备裸写 ───────────────────────────────────────
# mkfs 后面必然跟设备路径，写成「mkfs…/dev/…」才不会误伤 which mkfs / man mkfs 这类提及
mkfs*/dev/*
dd*of=/dev/*

# ── fork 炸弹 ──────────────────────────────────────────────────
:|:&

# ── Windows：递归删盘根或系统目录 ──────────────────────────────
Remove-Item -Recurse -Force C:\
Remove-Item -Recurse -Force C:\Windows
rd /s /q C:\
rd /s /q C:\Windows

# ── Windows：格式化 / 抹盘 / 毁引导 / 删卷影副本 ───────────────
format C:
Format-Volume
Clear-Disk
diskpart clean
diskpart /s
reg delete HKLM
bcdedit /delete
vssadmin delete shadows
"#;

/// 判定结果。只有两种：放行，或拒绝并说明命中了哪条。
pub enum Verdict {
    Allow,
    Deny { why: String },
}

pub struct Guard {
    path: PathBuf,
}

impl Guard {
    /// 打开名单；文件不存在则写出默认名单。名单路径固定，用户改的就是这个文件。
    pub fn load_or_create(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join(DENY_LIST_FILE);
        if !path.exists() {
            std::fs::write(&path, DEFAULT_DENY_LIST)
                .with_context(|| format!("写出默认命令黑名单失败: {}", path.display()))?;
        }
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 名单原文（供界面展示与编辑）。
    pub fn read_list(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap_or_else(|_| DEFAULT_DENY_LIST.to_string())
    }

    /// 覆盖写名单。内容不做校验——用户想拦什么就拦什么；写坏了下次判定退回内置默认。
    pub fn write_list(&self, content: &str) -> Result<()> {
        std::fs::write(&self.path, content)
            .with_context(|| format!("写入命令黑名单失败: {}", self.path.display()))
    }

    /// 判定一条命令。每次重读名单文件，所以用户改完立刻生效。
    pub fn check(&self, command: &str) -> Verdict {
        let raw = command.trim();
        if raw.is_empty() {
            return Verdict::Allow;
        }

        // 命令被摘要截断过就拒绝：`…` / `[truncated]` 说明 agent 手里已经不是完整命令，
        // 匹配它没有意义（真命令未知），放行则等于无护栏。
        if raw.contains('\u{2026}') || raw.to_lowercase().contains("[truncated") {
            return Verdict::Deny {
                why: "命令疑似被截断（含 … 或 [truncated]），无法确认完整命令，已拒绝执行。请重新给出完整命令。"
                    .to_string(),
            };
        }

        let hay = normalize(raw);
        if hay.is_empty() {
            return Verdict::Allow;
        }

        for pattern in self.patterns() {
            if pattern_matches(&pattern, &hay) {
                return Verdict::Deny {
                    why: format!(
                        "命中命令黑名单「{}」。该规则在 {}，确认要放开请自行编辑该文件。",
                        pattern,
                        self.path.display()
                    ),
                };
            }
        }
        Verdict::Allow
    }

    /// 读名单。读不到就退回内置默认——护栏宁可多拦，不能因为文件没了就静默失效。
    fn patterns(&self) -> Vec<String> {
        parse_patterns(&self.read_list())
    }
}

/// 解析名单：一行一条，`#` 起头与空行跳过。
fn parse_patterns(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(normalize)
        .filter(|p| !p.is_empty())
        .collect()
}

/// 归一化：剥引号 → `\/` 还原成 `/` → 转小写 → 折叠空白 → 短选项聚簇排序。
/// 名单里的模式与待判命令走同一条归一化，所以两边写法天然对齐。
fn normalize(text: &str) -> String {
    let no_quotes: String = text.chars().filter(|c| *c != '"' && *c != '\'').collect();
    let unescaped = no_quotes.replace("\\/", "/");
    let lowered = unescaped.to_lowercase();

    let mut out: Vec<String> = Vec::new();
    let mut pending: Vec<char> = Vec::new();
    for token in lowered.split_whitespace() {
        if is_short_flag_cluster(token) {
            // `-rf` / `-fr` / `-r -f` / `-f -r` 全部塌成同一个 `-fr`
            pending.extend(token[1..].chars());
        } else {
            flush_flags(&mut pending, &mut out);
            out.push(token.to_string());
        }
    }
    flush_flags(&mut pending, &mut out);
    out.join(" ")
}

/// 短选项聚簇：`-` 后 1~4 个纯字母。`--recursive`、`-Recurse` 这类长旗标不动它。
fn is_short_flag_cluster(token: &str) -> bool {
    let Some(body) = token.strip_prefix('-') else {
        return false;
    };
    if body.is_empty() || body.starts_with('-') {
        return false;
    }
    body.len() <= 4 && body.chars().all(|c| c.is_ascii_alphabetic())
}

fn flush_flags(pending: &mut Vec<char>, out: &mut Vec<String>) {
    if pending.is_empty() {
        return;
    }
    pending.sort_unstable();
    pending.dedup();
    out.push(format!("-{}", pending.iter().collect::<String>()));
    pending.clear();
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 模式匹配：`*` 通配，无 `*` 即精确匹配；首尾按词边界卡住（模式自带首尾 `*` 则放开那一侧）。
/// 词边界只认 [A-Za-z0-9_]，所以 `/`、空格、`;`、`|`、`&`、`.`、`\` 都算边界——
/// 这正是 `rm -rf /` 不误伤 `rm -rf /var/log`、`mkfs` 不误伤 `mkfsomething` 的原因。
fn pattern_matches(pattern: &str, hay: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let head_anchored = !pattern.starts_with('*');
    let tail_anchored = !pattern.ends_with('*');
    let parts: Vec<&str> = pattern.split('*').filter(|p| !p.is_empty()).collect();

    if parts.is_empty() {
        // 模式就是 "*"：命中任何非空命令
        return !hay.is_empty();
    }

    for (start, _) in hay.match_indices(parts[0]) {
        if head_anchored
            && start != 0
            && hay[..start].chars().next_back().map(is_word_char).unwrap_or(false)
        {
            continue;
        }
        let mut pos = start + parts[0].len();
        let mut ok = true;
        for part in &parts[1..] {
            match hay[pos..].find(part) {
                Some(offset) => pos += offset + part.len(),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        if tail_anchored
            && pos != hay.len()
            && hay[pos..].chars().next().map(is_word_char).unwrap_or(false)
        {
            continue;
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> Guard {
        Guard {
            path: PathBuf::from("does-not-exist-deny-list.txt"),
        }
    }

    fn denied(cmd: &str) -> bool {
        matches!(guard().check(cmd), Verdict::Deny { .. })
    }

    fn allowed(cmd: &str) -> bool {
        matches!(guard().check(cmd), Verdict::Allow)
    }

    /// 必须拦：不可逆灾难。含 1Shell 2026-07-04 云电脑事故那条命令。
    #[test]
    fn denies_catastrophic() {
        for cmd in [
            // POSIX
            "rm -rf /",
            "rm -rf /*",
            "rm --no-preserve-root -rf /",
            "sudo rm -rf /",
            "rm -rf /etc",
            "rm -rf /usr",
            "rm -rf /boot",
            "mkfs.ext4 /dev/sda1",
            "mkfs -t ext4 /dev/nvme0n1",
            "dd if=/dev/zero of=/dev/sda bs=1M",
            "dd bs=1M of=/dev/nvme0n1",
            ":(){ :|:& };:",
            ":(){:|:&};:",
            // Windows
            "Remove-Item -Recurse -Force C:\\",
            "Remove-Item -Recurse -Force C:\\Windows",
            "rd /s /q C:\\",
            "rd /s /q C:\\Windows",
            "format C: /y",
            "Format-Volume -DriveLetter C -Confirm:$false",
            "Clear-Disk -Number 0 -RemoveData -Confirm:$false",
            "diskpart /s wipe.txt",
            "reg delete HKLM\\SOFTWARE /f",
            "bcdedit /delete {default} /f",
            "vssadmin delete shadows /all /quiet",
        ] {
            assert!(denied(cmd), "应拦截但放行了: {cmd}");
        }
    }

    /// 必须拦住手滑写法的变体：引号、反斜杠转义、大小写、短选项顺序与拆写。
    #[test]
    fn denies_sloppy_spellings() {
        for cmd in [
            "rm -rf \"/\"",
            "rm -rf '/'",
            "rm -rf \\/",
            "RM -RF /",
            "rm -fr /",
            "rm -r -f /",
            "rm -f -r /",
            "rm  -rf   /",
            "sudo -u root rm -rf /",
            "/usr/bin/rm -rf /",
            "bash -c \"rm -rf /\"",
        ] {
            assert!(denied(cmd), "应拦截但放行了: {cmd}");
        }
    }

    /// 必须放行：日常运维。误拦了这几条，这个功能就不该存在。
    #[test]
    fn allows_ordinary_ops() {
        for cmd in [
            "ls -la",
            "cat /etc/nginx/nginx.conf",
            "systemctl status nginx",
            "docker compose up -d",
            "docker ps -a --format '{{.Names}}'",
            "df -h",
            "free -m",
            "tail -n 100 /var/log/auth.log",
            "grep -r nginx /etc/nginx",
            // 删子目录是正常清理，不是灾难
            "rm -rf /var/log/nginx.old",
            "rm -rf /tmp/build",
            "rm -rf /home/deploy/app/dist",
            "rm -rf ./node_modules",
            "rm -rf /opt/app/cache",
            // 读块设备不是写块设备
            "dd if=/dev/sda of=/root/backup.img bs=4M",
            "dd if=/dev/zero of=/tmp/zero.bin bs=1M count=10",
            "mkfs_report=$(which mkfs)",
            "which mkfs",
            "man mkfs",
            "lsblk",
            "apt-get install -y nginx",
            "ufw allow 443",
            // Windows 日常
            "Get-ChildItem C:\\Windows\\System32",
            "Get-Content C:\\ProgramData\\ssh\\sshd_config",
            "reg query HKLM\\SOFTWARE\\OpenSSH",
            "Remove-Item C:\\temp\\one-file.txt",
            "Remove-Item -Recurse D:\\build\\out",
            "New-Item -ItemType Directory C:\\temp\\work",
            "Get-Service sshd",
            "Restart-Service sshd",
            "systeminfo",
            "git log --format=%H -1",
            "docker system prune -af",
        ] {
            assert!(allowed(cmd), "不应拦截但拦了: {cmd}");
        }
    }

    /// 边界：模式两侧的词边界必须成立，短模式不能吃掉长命令。
    #[test]
    fn word_boundaries_hold() {
        assert!(allowed("rm -rf /var/log/nginx.old"));
        assert!(allowed("lsblk"));
        assert!(allowed("mkfsomething --help"));
        assert!(allowed("reg query HKLM\\SOFTWARE"));
        assert!(allowed("docker run --rm alpine echo hi"));
        assert!(allowed("format-json --in x"));
        assert!(allowed("git log --format=%H"));
    }

    /// 截断标记：命令被摘要过就拒绝，不能拿半截命令去匹配。
    #[test]
    fn denies_truncated_command() {
        assert!(denied("rm -rf /very/long/path…"));
        assert!(denied("echo hello [truncated 120 chars]"));
    }

    /// 名单文件解析：注释与空行跳过，用户增删立即生效。
    #[test]
    fn parses_user_list() {
        let list = "# 注释\n\nrm -rf /\n\n  my-dangerous-cmd  \n";
        let pats = parse_patterns(list);
        assert_eq!(pats, vec!["rm -fr /".to_string(), "my-dangerous-cmd".to_string()]);
    }
}
