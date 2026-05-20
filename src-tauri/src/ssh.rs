//! SSH 客户端：测试连接 + `ssh + tar` 流式同步 *.jsonl 到本地缓存。
//!
//! 不引入 `ssh2` crate，改用系统 `ssh` 和 `tar` 命令子进程（见
//! `docs/DECISIONS.md#adr-010ssh-使用系统命令而非-ssh2-crate`）。
//!
//! 同步走 `ssh ... 'tar c ...' | tar x` 单向流，而不是 rsync 双向协议
//! （见 `docs/DECISIONS.md#adr-013-放弃-rsync-改用-ssh--tar-单向流`）。
//!
//! 公钥认证（默认）所有 SSH 调用都加：
//! - `BatchMode=yes`（禁止任何交互式密码输入，避免卡进程）
//! - `StrictHostKeyChecking=no`（不卡 known_hosts，初次连接也能跑）
//! - `ConnectTimeout=8`（连接 8 秒超时）
//!
//! 密码认证使用 `sshpass -e ssh ...`，密码通过 `SSHPASS` 环境变量传入（避免
//! 出现在 `ps` 输出里）。密码方式下：
//! - 不设 `BatchMode=yes`（否则 sshpass 无法注入密码）
//! - `PreferredAuthentications=password,keyboard-interactive` 强制走密码
//! - `PubkeyAuthentication=no` 跳过公钥试探
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::workspace::{expand_tilde, SshAuthMethod, Workspace, WorkspaceKind};

const CONNECT_TIMEOUT_SECS: u32 = 8;
const SSHPASS_ENV: &str = "SSHPASS";

// ============================================================
// test_connection
// ============================================================

/// 测试 SSH 工作区：用 `ssh ... echo OK && test -d <path>` 验证连通性 + 路径存在。
///
/// 返回多行可读字符串，每行 `✓` / `✗` 标记，可直接渲染到 UI StatusBanner。
pub async fn test(ws: &Workspace) -> Result<String> {
    require_ssh(ws)?;
    if ws.auth_method == SshAuthMethod::Password {
        // 提前给出 sshpass 缺失的友好提示；同时校验密码非空
        require_password_if_needed(ws)?;
        ensure_sshpass_installed().await?;
    }
    let user = ws.user.as_deref().unwrap_or("root");
    let host = ws.host.as_deref().unwrap_or_default();
    let port = ws.port.unwrap_or(22);

    let want_claude = ws.tools.iter().any(|t| t == "claude-code");
    let want_codex = ws.tools.iter().any(|t| t == "codex");
    let claude_path = ws.claude_path.as_deref().unwrap_or("~/.claude");
    let codex_path = ws.codex_path.as_deref().unwrap_or("~/.codex");

    // 远端 shell 脚本：echo OK + 各 path 的存在性检查。
    // 路径必须 POSIX shell 安全转义，否则用户填的 `claude_path = "\"; rm -rf ~"`
    // 会在远端执行任意命令。
    let mut script = String::from("echo OK");
    if want_claude {
        script.push_str(&format!(
            " && (test -d {} && echo CLAUDE_OK || echo CLAUDE_MISSING)",
            sh_quote_remote_path(claude_path)
        ));
    }
    if want_codex {
        script.push_str(&format!(
            " && (test -d {} && echo CODEX_OK || echo CODEX_MISSING)",
            sh_quote_remote_path(codex_path)
        ));
    }

    let mut cmd = build_ssh_command(ws)?;
    cmd.arg(format!("{user}@{host}"));
    cmd.arg(&script);
    let output = cmd
        .output()
        .await
        .map_err(|e| anyhow!("无法启动 ssh 命令：{e}\n请确认系统已安装 OpenSSH"))?;

    if !output.status.success() {
        return Err(format_ssh_error(
            &String::from_utf8_lossy(&output.stderr),
            output.status.code(),
            ws.auth_method,
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = vec![format!("✓ SSH 连接成功：{user}@{host}:{port}")];
    if want_claude {
        lines.push(if stdout.contains("CLAUDE_OK") {
            format!("✓ Claude Code 路径存在：{claude_path}")
        } else {
            format!("✗ Claude Code 路径不存在：{claude_path}")
        });
    }
    if want_codex {
        lines.push(if stdout.contains("CODEX_OK") {
            format!("✓ Codex 路径存在：{codex_path}")
        } else {
            format!("✗ Codex 路径不存在：{codex_path}")
        });
    }
    Ok(lines.join("\n"))
}

/// 把 ssh stderr 翻成用户可读的中文错误。
fn format_ssh_error(stderr: &str, exit_code: Option<i32>, auth: SshAuthMethod) -> anyhow::Error {
    let lower = stderr.to_lowercase();
    // sshpass 退出码 5 = 密码错误，6 = host key 不匹配
    let auth_hint = match auth {
        SshAuthMethod::Key => {
            "SSH 认证失败：检查 ssh_key 路径或确认公钥已添加到服务端 ~/.ssh/authorized_keys"
        }
        SshAuthMethod::Password => "SSH 认证失败：检查用户名和密码是否正确",
    };
    let hint = if lower.contains("connection timed out") || lower.contains("operation timed out") {
        format!("SSH 连接超时（{CONNECT_TIMEOUT_SECS} 秒）：检查 host 和网络")
    } else if lower.contains("permission denied") || lower.contains("publickey") {
        auth_hint.into()
    } else if lower.contains("could not resolve") || lower.contains("name or service not known") {
        "无法解析主机名：检查 host 拼写".into()
    } else if lower.contains("no route to host") || lower.contains("network is unreachable") {
        "无法连接到 host：检查网络与防火墙".into()
    } else if lower.contains("host key verification failed") {
        "Host key 校验失败（罕见，因为我们已设 StrictHostKeyChecking=no）".into()
    } else if exit_code == Some(5) && matches!(auth, SshAuthMethod::Password) {
        // sshpass 文档定义：exit 5 = 密码错误
        "SSH 密码错误：检查密码是否正确".into()
    } else {
        "SSH 命令失败".into()
    };
    let stderr_snippet = stderr
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join("\n");
    anyhow!(
        "{hint}（exit={}）\n{stderr_snippet}",
        exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".into())
    )
}

// ============================================================
// sync_to_cache
// ============================================================

/// 用 `ssh + tar` 把远端 `*.jsonl` 同步到本地缓存目录。
///
/// 返回：tool name (`"claude-code"` / `"codex"`) → 本地缓存路径。
/// 只同步 workspace `tools` 中启用的工具；只拉 `*.jsonl` 文件（保留目录结构）。
pub async fn sync_to_cache(ws: &Workspace) -> Result<HashMap<String, PathBuf>> {
    require_ssh(ws)?;
    if ws.auth_method == SshAuthMethod::Password {
        require_password_if_needed(ws)?;
        ensure_sshpass_installed().await?;
    }

    let root = cache_root(&ws.id)?;
    std::fs::create_dir_all(&root)?;

    let mut out = HashMap::new();
    if ws.tools.iter().any(|t| t == "claude-code") {
        let remote = ws.claude_path.as_deref().unwrap_or("~/.claude");
        let local = root.join("claude");
        std::fs::create_dir_all(&local)?;
        tar_pull_jsonl(ws, remote, &local).await?;
        out.insert("claude-code".into(), local);
    }
    if ws.tools.iter().any(|t| t == "codex") {
        let remote = ws.codex_path.as_deref().unwrap_or("~/.codex");
        let local = root.join("codex");
        std::fs::create_dir_all(&local)?;
        tar_pull_jsonl(ws, remote, &local).await?;
        out.insert("codex".into(), local);
    }
    Ok(out)
}

/// 构造远端 shell 命令：进入 `remote` 目录，用 `find` 选出所有 `*.jsonl`，交给
/// `tar` 打包到 stdout（被 ssh channel 转发到本地 stdout）。
///
/// 命令结构：`cd <quoted> && find . -name '*.jsonl' -print0 | tar --null -cf - -T -`
/// - `cd` 后 `find .` 用相对路径，让 archive 中的文件路径相对 remote 根
/// - `find -print0` + `tar --null -T -` 用 NUL 分隔，安全处理含空格/特殊字符的文件名
/// - 远端路径走 [`sh_quote_remote_path`] 转义，抵御命令注入
fn build_remote_tar_cmd(remote: &str) -> String {
    format!(
        "cd {} && find . -name '*.jsonl' -print0 | tar --null -cf - -T -",
        sh_quote_remote_path(remote)
    )
}

/// 选择本地 `tar` 可执行文件。
///
/// Windows 上 PATH 第一个 `tar.exe` 可能是 MSYS2/Cygwin 版本，它的 stdio 用
/// Cygwin pipe 句柄，跟 Win32 OpenSSH spawn 的 anonymous pipe 不兼容（读时报
/// "Unknown error"）。优先用 `%SystemRoot%\System32\tar.exe`（Windows 10 1803+
/// 自带的 bsdtar），避免选到 MSYS2 tar。其他平台保持 PATH 解析的 `tar`。
fn local_tar_command() -> Command {
    #[cfg(target_os = "windows")]
    {
        if let Ok(sysroot) = std::env::var("SystemRoot") {
            let p = PathBuf::from(&sysroot).join("System32").join("tar.exe");
            if p.exists() {
                return Command::new(p);
            }
        }
    }
    Command::new("tar")
}

/// 远端 `ssh + tar c` 打包 → 本地 `tar x` 解包，单向流式同步。
///
/// 相较 rsync：
/// - 只用单向 stdio（ssh.stdout → tar.stdin），不依赖 rsync 协议的双向握手，
///   避开 Windows 上 MSYS2 rsync ↔ Win32 OpenSSH 的 pipe 不兼容（详见
///   `docs/DECISIONS.md#adr-013`）
/// - 全量同步而非增量；对 jsonl 日志（典型 <10MB/工作区）代价可忽略
async fn tar_pull_jsonl(ws: &Workspace, remote: &str, local: &Path) -> Result<()> {
    let user = ws.user.as_deref().unwrap_or("root");
    let host = ws.host.as_deref().unwrap_or_default();
    let local_str = local
        .to_str()
        .ok_or_else(|| anyhow!("缓存路径不是 UTF-8: {}", local.display()))?;

    // 1. 启动 ssh：远端跑 tar c，stdout 接 Rust 创建的 pipe
    let mut ssh_cmd = build_ssh_command(ws)?;
    ssh_cmd.arg(format!("{user}@{host}"));
    ssh_cmd.arg(build_remote_tar_cmd(remote));
    ssh_cmd.stdin(Stdio::null());
    ssh_cmd.stdout(Stdio::piped());
    ssh_cmd.stderr(Stdio::piped());

    let mut ssh = ssh_cmd
        .spawn()
        .map_err(|e| anyhow!("无法启动 ssh 命令：{e}\n请确认系统已安装 OpenSSH"))?;

    // 2. 启动本地 tar x，stdin 从 ssh stdout 接管
    let mut tar_cmd = local_tar_command();
    tar_cmd
        .args(["xf", "-", "-C", local_str])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped());

    let mut tar = tar_cmd.spawn().map_err(|e| {
        anyhow!(
            "无法启动 tar 命令：{e}\n请确认系统已安装 tar\n\
             （Windows 10+ / macOS / Linux 默认都自带）"
        )
    })?;

    let mut ssh_stdout = ssh.stdout.take().expect("ssh stdout piped");
    let mut ssh_stderr = ssh.stderr.take().expect("ssh stderr piped");
    let mut tar_stdin = tar.stdin.take().expect("tar stdin piped");
    let mut tar_stderr = tar.stderr.take().expect("tar stderr piped");

    // 3. 并发：两端 stderr 后台收集 + ssh stdout 搬到 tar stdin
    let ssh_err_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = ssh_stderr.read_to_end(&mut buf).await;
        buf
    });
    let tar_err_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = tar_stderr.read_to_end(&mut buf).await;
        buf
    });
    let copy_task = tokio::spawn(async move {
        let res = tokio::io::copy(&mut ssh_stdout, &mut tar_stdin).await;
        // 显式 drop 让 tar 看到 stdin EOF
        drop(tar_stdin);
        res
    });

    // 4. 等结束 + 统一错误处理
    let ssh_status = ssh.wait().await?;
    let _ = copy_task.await;
    let tar_status = tar.wait().await?;
    let ssh_err = ssh_err_task.await.unwrap_or_default();
    let tar_err = tar_err_task.await.unwrap_or_default();

    if !ssh_status.success() {
        let stderr_str = String::from_utf8_lossy(&ssh_err);
        return Err(format_ssh_error(
            &stderr_str,
            ssh_status.code(),
            ws.auth_method,
        ));
    }
    if !tar_status.success() {
        let stderr_str = String::from_utf8_lossy(&tar_err);
        let snippet = stderr_str
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(3)
            .collect::<Vec<_>>()
            .join("\n");
        return Err(anyhow!(
            "本地 tar 解包失败（exit={}）：\n{snippet}",
            tar_status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".into())
        ));
    }
    Ok(())
}

// ============================================================
// 工具
// ============================================================

fn require_ssh(ws: &Workspace) -> Result<()> {
    if ws.kind != WorkspaceKind::Ssh {
        return Err(anyhow!("不是 SSH 工作区"));
    }
    if ws.host.as_deref().unwrap_or("").is_empty() {
        return Err(anyhow!("SSH 工作区缺少 host"));
    }
    Ok(())
}

/// 校验密码方式时 `ssh_password` 必须非空，并返回密码值。
///
/// 返回 `Ok(None)` 表示使用公钥方式（不需要密码）；
/// 返回 `Ok(Some(pw))` 表示密码方式且密码已填；
/// 返回 `Err` 表示密码方式但未填写密码。
fn require_password_if_needed(ws: &Workspace) -> Result<Option<String>> {
    if ws.auth_method != SshAuthMethod::Password {
        return Ok(None);
    }
    let pw = ws.ssh_password.as_deref().unwrap_or("");
    if pw.is_empty() {
        return Err(anyhow!("SSH 密码方式：ssh_password 不能为空"));
    }
    Ok(Some(pw.to_string()))
}

/// 检查系统是否安装了 `sshpass`。密码认证依赖此工具。
async fn ensure_sshpass_installed() -> Result<()> {
    // `sshpass -V` 在 stdout 输出版本号并退出 0；命令缺失时 spawn 会失败
    let res = Command::new("sshpass").arg("-V").output().await;
    match res {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(anyhow!(
            "sshpass 命令异常（exit={}）：{}",
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stderr)
                .lines()
                .next()
                .unwrap_or("")
        )),
        Err(_) => Err(anyhow!(
            "未找到 sshpass 命令。密码方式 SSH 需要先安装 sshpass：\n\
             - macOS：brew install hudochenkov/sshpass/sshpass\n\
             - Debian/Ubuntu：sudo apt install sshpass\n\
             - CentOS/RHEL：sudo yum install sshpass\n\
             - Windows：建议改用公钥方式，或在 WSL 内安装 sshpass"
        )),
    }
}

/// 构造一个 ssh 子进程命令，根据 `auth_method` 自动套用 `sshpass`。
///
/// 调用方追加 `user@host` 和远端命令后即可 `output().await`。
fn build_ssh_command(ws: &Workspace) -> Result<Command> {
    let password = require_password_if_needed(ws)?;
    let args = base_ssh_args(ws);
    let mut cmd = match password.as_deref() {
        Some(pw) => {
            let mut c = Command::new("sshpass");
            // `-e` 让 sshpass 从 SSHPASS 环境变量读密码，避免出现在 ps 输出里
            c.arg("-e").arg("ssh").env(SSHPASS_ENV, pw);
            c
        }
        None => Command::new("ssh"),
    };
    cmd.args(args.iter().map(OsStr::new));
    Ok(cmd)
}

fn base_ssh_args(ws: &Workspace) -> Vec<String> {
    let mut args = vec![
        "-o".into(),
        "StrictHostKeyChecking=no".into(),
        "-o".into(),
        format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}"),
    ];
    match ws.auth_method {
        SshAuthMethod::Key => {
            // 公钥方式：禁止交互输入，避免卡进程
            args.push("-o".into());
            args.push("BatchMode=yes".into());
            if let Some(k) = ws.ssh_key.as_deref() {
                if !k.is_empty() {
                    args.push("-i".into());
                    args.push(expand_tilde(k));
                }
            }
        }
        SshAuthMethod::Password => {
            // 密码方式：通过 sshpass 注入；明确只允许密码认证
            args.push("-o".into());
            args.push("PreferredAuthentications=password,keyboard-interactive".into());
            args.push("-o".into());
            args.push("PubkeyAuthentication=no".into());
        }
    }
    if let Some(p) = ws.port {
        if p != 22 {
            args.push("-p".into());
            args.push(p.to_string());
        }
    }
    args
}

/// 跨平台缓存根目录：
/// - macOS:   `~/Library/Caches/WeeklyReport/<ws_id>/`
/// - Linux:   `~/.cache/weekly-report/<ws_id>/`
/// - Windows: `%LOCALAPPDATA%\WeeklyReport\Cache\<ws_id>\`
pub fn cache_root(ws_id: &str) -> Result<PathBuf> {
    let base = dirs::cache_dir().ok_or_else(|| anyhow!("无法定位 OS 缓存目录"))?;
    Ok(base.join(cache_app_name()).join(ws_id))
}

fn cache_app_name() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "weekly-report"
    }
    #[cfg(not(target_os = "linux"))]
    {
        "WeeklyReport"
    }
}

/// 把任意字符串安全包成 POSIX shell 单引号字符串。
///
/// 单引号内只有单引号本身是特殊字符，把它转义为 `'\''` 即可。
/// 这是抵御命令注入的标准做法，比双引号转义更可靠（双引号内 `$`、`\` 等都是
/// 特殊字符，需要分别处理，容易遗漏）。
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// 远端路径专用引用：开头的 `~/` 替换为 `"$HOME"` 让远端 shell 展开家目录，
/// 其余部分仍走 `sh_quote` 单引号转义。
///
/// 普通 `sh_quote` 把整个路径包进单引号，会顺带把 `~` 也当字面字符，导致远端
/// `test -d '~/.claude'` 找不到目录。本函数只在开头特判 `~`/`~/`，命令注入抵御
/// 能力跟 `sh_quote` 一致（注入字符仍被关在单引号里）。
fn sh_quote_remote_path(s: &str) -> String {
    if s == "~" {
        return "\"$HOME\"".to_string();
    }
    if let Some(rest) = s.strip_prefix("~/") {
        // 例如 ~/.claude -> "$HOME"'/.claude'
        return format!("\"$HOME\"{}", sh_quote(&format!("/{rest}")));
    }
    sh_quote(s)
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh_ws() -> Workspace {
        Workspace {
            id: "w1".into(),
            name: "服务器".into(),
            kind: WorkspaceKind::Ssh,
            host: Some("example.com".into()),
            user: Some("alice".into()),
            port: Some(2200),
            auth_method: SshAuthMethod::Key,
            ssh_key: Some("~/.ssh/id_ed25519".into()),
            ssh_password: None,
            claude_path: Some("/home/alice/.claude".into()),
            codex_path: Some("/home/alice/.codex".into()),
            tools: vec!["claude-code".into(), "codex".into()],
        }
    }

    fn ssh_pw_ws() -> Workspace {
        Workspace {
            auth_method: SshAuthMethod::Password,
            ssh_key: None,
            ssh_password: Some("s3cret".into()),
            ..ssh_ws()
        }
    }

    #[test]
    fn base_ssh_args_include_safe_options() {
        let args = base_ssh_args(&ssh_ws());
        let joined = args.join(" ");
        assert!(joined.contains("BatchMode=yes"));
        assert!(joined.contains("StrictHostKeyChecking=no"));
        assert!(joined.contains("ConnectTimeout=8"));
        assert!(joined.contains("-p 2200"));
        assert!(joined.contains("-i "));
    }

    #[test]
    fn base_ssh_args_skip_port_22_and_empty_key() {
        let mut ws = ssh_ws();
        ws.port = Some(22);
        ws.ssh_key = Some(String::new());
        let args = base_ssh_args(&ws);
        assert!(!args.iter().any(|a| a == "-p"));
        assert!(!args.iter().any(|a| a == "-i"));
    }

    #[test]
    fn base_ssh_args_password_disables_batchmode_and_pubkey() {
        let args = base_ssh_args(&ssh_pw_ws());
        let joined = args.join(" ");
        // 密码方式不能加 BatchMode=yes（会让 sshpass 失效）
        assert!(!joined.contains("BatchMode=yes"));
        assert!(joined.contains("PreferredAuthentications=password"));
        assert!(joined.contains("PubkeyAuthentication=no"));
        // 不应当带 -i（私钥）
        assert!(!args.iter().any(|a| a == "-i"));
    }

    // -------- 远端 tar 命令构造 --------

    #[test]
    fn remote_tar_cmd_uses_relative_path_after_cd() {
        let s = build_remote_tar_cmd("/home/alice/.claude");
        // cd 之后 find 必须用 `.` 相对路径，让 archive 路径相对 remote 根
        assert!(s.contains("cd '/home/alice/.claude'"));
        assert!(s.contains("find . -name '*.jsonl' -print0"));
        assert!(s.contains("tar --null -cf - -T -"));
    }

    #[test]
    fn remote_tar_cmd_expands_tilde_via_home() {
        // ~/.claude 必须被 sh_quote_remote_path 转为 "$HOME"'/.claude'
        let s = build_remote_tar_cmd("~/.claude");
        assert!(s.contains("cd \"$HOME\"'/.claude'"));
    }

    #[test]
    fn remote_tar_cmd_neutralizes_injection() {
        // 攻击者填的 remote_path：闭合引号 + 注入 rm
        let s = build_remote_tar_cmd("~/foo'; rm -rf ~ #");
        // 注入字符必须全部留在单引号内，find / tar 段照样在
        assert!(s.contains("find . -name '*.jsonl' -print0"));
        assert!(s.contains("rm -rf"));
        // cd 段必须以 "$HOME" 开头（家目录展开），整个尾段在单引号里
        assert!(s.contains("cd \"$HOME\""));
    }

    #[test]
    fn require_password_rejects_empty() {
        let mut ws = ssh_pw_ws();
        ws.ssh_password = Some(String::new());
        assert!(require_password_if_needed(&ws).is_err());
        ws.ssh_password = None;
        assert!(require_password_if_needed(&ws).is_err());
    }

    #[test]
    fn require_password_skipped_for_key_auth() {
        let ws = ssh_ws();
        assert!(matches!(require_password_if_needed(&ws), Ok(None)));
    }

    #[test]
    fn require_password_returns_value() {
        let ws = ssh_pw_ws();
        let pw = require_password_if_needed(&ws).unwrap();
        assert_eq!(pw.as_deref(), Some("s3cret"));
    }

    #[test]
    fn require_ssh_rejects_local() {
        let mut ws = ssh_ws();
        ws.kind = WorkspaceKind::Local;
        assert!(require_ssh(&ws).is_err());
    }

    #[test]
    fn require_ssh_rejects_missing_host() {
        let mut ws = ssh_ws();
        ws.host = None;
        assert!(require_ssh(&ws).is_err());
        ws.host = Some(String::new());
        assert!(require_ssh(&ws).is_err());
    }

    #[test]
    fn format_ssh_error_recognizes_timeout() {
        let e = format_ssh_error(
            "ssh: connect to host example.com port 22: Connection timed out",
            Some(255),
            SshAuthMethod::Key,
        );
        let s = e.to_string();
        assert!(s.contains("超时"));
    }

    #[test]
    fn format_ssh_error_recognizes_auth_key_hint() {
        let e = format_ssh_error(
            "alice@example.com: Permission denied (publickey)",
            Some(255),
            SshAuthMethod::Key,
        );
        let s = e.to_string();
        assert!(s.contains("认证失败"));
        assert!(s.contains("ssh_key"));
    }

    #[test]
    fn format_ssh_error_recognizes_auth_password_hint() {
        let e = format_ssh_error(
            "Permission denied, please try again.",
            Some(255),
            SshAuthMethod::Password,
        );
        let s = e.to_string();
        assert!(s.contains("用户名和密码"));
    }

    #[test]
    fn format_ssh_error_recognizes_sshpass_exit5() {
        // sshpass 把密码错误专门定为 exit 5
        let e = format_ssh_error("", Some(5), SshAuthMethod::Password);
        let s = e.to_string();
        assert!(s.contains("密码错误"));
    }

    #[test]
    fn format_ssh_error_recognizes_dns() {
        let e = format_ssh_error(
            "ssh: Could not resolve hostname bogus.example.com",
            Some(255),
            SshAuthMethod::Key,
        );
        let s = e.to_string();
        assert!(s.contains("无法解析主机名"));
    }

    #[test]
    fn cache_root_is_per_workspace() {
        let a = cache_root("ws-a").unwrap();
        let b = cache_root("ws-b").unwrap();
        assert_ne!(a, b);
        assert!(a.ends_with("ws-a"));
        assert!(b.ends_with("ws-b"));
    }

    // -------- 命令注入抵御 --------

    #[test]
    fn sh_quote_wraps_in_single_quotes() {
        assert_eq!(sh_quote("plain"), "'plain'");
        assert_eq!(sh_quote("/Users/me/.claude"), "'/Users/me/.claude'");
    }

    #[test]
    fn sh_quote_escapes_single_quote() {
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn sh_quote_neutralizes_injection_attempts() {
        // 经典攻击 payload：闭合引号 + 注入命令
        let evil = r#""; rm -rf ~; echo ""#;
        let quoted = sh_quote(evil);
        // 整个 evil 字符串全部在外层单引号内，shell 不会展开
        assert!(quoted.starts_with('\''));
        assert!(quoted.ends_with('\''));
        // 内部双引号原样保留（在单引号内是普通字符）
        assert!(quoted.contains('"'));
        // 关键：分号和 rm 都被关在单引号内，不会被 shell 解释
        assert!(quoted.contains("rm -rf"));
    }

    #[test]
    fn sh_quote_handles_dollar_and_backtick() {
        // $ 和 ` 在双引号内会被展开，但在单引号内是普通字符
        assert_eq!(sh_quote("$PATH"), "'$PATH'");
        assert_eq!(sh_quote("`whoami`"), "'`whoami`'");
    }

    // -------- sh_quote_remote_path（远端路径的 ~ 展开） --------

    #[test]
    fn sh_quote_remote_path_expands_leading_tilde_slash() {
        // ~/.claude -> "$HOME"'/.claude'
        assert_eq!(sh_quote_remote_path("~/.claude"), "\"$HOME\"'/.claude'");
        assert_eq!(sh_quote_remote_path("~/work/logs"), "\"$HOME\"'/work/logs'");
    }

    #[test]
    fn sh_quote_remote_path_expands_bare_tilde() {
        assert_eq!(sh_quote_remote_path("~"), "\"$HOME\"");
    }

    #[test]
    fn sh_quote_remote_path_leaves_absolute_paths_alone() {
        assert_eq!(
            sh_quote_remote_path("/home/changan/.claude"),
            "'/home/changan/.claude'"
        );
    }

    #[test]
    fn sh_quote_remote_path_leaves_mid_tilde_alone() {
        // 路径中间的 ~ 不当作家目录，保持字面意义
        assert_eq!(sh_quote_remote_path("/x/~/y"), "'/x/~/y'");
    }

    #[test]
    fn sh_quote_remote_path_blocks_injection_after_tilde() {
        // 攻击者填 ~/foo; rm -rf ~ —— $HOME 在外面，分号和 rm 仍被单引号关住
        let q = sh_quote_remote_path("~/foo; rm -rf ~");
        assert!(q.starts_with("\"$HOME\""));
        assert!(q.contains("'/foo; rm -rf ~'"));
        // 整个尾段必须仍在单引号内
        assert!(q.ends_with('\''));
    }

    #[tokio::test]
    async fn sync_to_cache_rejects_local() {
        let mut ws = ssh_ws();
        ws.kind = WorkspaceKind::Local;
        assert!(sync_to_cache(&ws).await.is_err());
    }
}
