//! SSH 客户端：测试连接 + rsync 同步 *.jsonl 到本地缓存。
//!
//! 不引入 `ssh2` crate，改用系统 `ssh` 和 `rsync` 命令子进程（见
//! `docs/DECISIONS.md#adr-010ssh-使用系统命令而非-ssh2-crate`）。
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

/// 用 rsync 把远端 `*.jsonl` 同步到本地缓存目录。
///
/// 返回：tool name (`"claude-code"` / `"codex"`) → 本地缓存路径。
/// 只同步 workspace `tools` 中启用的工具；只拉 `*.jsonl` 文件（保留目录结构）。
pub async fn sync_to_cache(ws: &Workspace) -> Result<HashMap<String, PathBuf>> {
    require_ssh(ws)?;
    let user = ws.user.as_deref().unwrap_or("root");
    let host = ws.host.as_deref().unwrap_or_default();
    let password = require_password_if_needed(ws)?;
    if password.is_some() {
        ensure_sshpass_installed().await?;
    }
    let ssh_e_arg = build_rsync_ssh_arg(ws);

    let root = cache_root(&ws.id)?;
    std::fs::create_dir_all(&root)?;

    let mut out = HashMap::new();
    if ws.tools.iter().any(|t| t == "claude-code") {
        let remote = ws.claude_path.as_deref().unwrap_or("~/.claude");
        let local = root.join("claude");
        std::fs::create_dir_all(&local)?;
        rsync_jsonl(&ssh_e_arg, user, host, remote, &local, password.as_deref()).await?;
        out.insert("claude-code".into(), local);
    }
    if ws.tools.iter().any(|t| t == "codex") {
        let remote = ws.codex_path.as_deref().unwrap_or("~/.codex");
        let local = root.join("codex");
        std::fs::create_dir_all(&local)?;
        rsync_jsonl(&ssh_e_arg, user, host, remote, &local, password.as_deref()).await?;
        out.insert("codex".into(), local);
    }
    Ok(out)
}

async fn rsync_jsonl(
    ssh_e_arg: &str,
    user: &str,
    host: &str,
    remote: &str,
    local: &Path,
    password: Option<&str>,
) -> Result<()> {
    // 远端路径末尾加 `/` 让 rsync 按子树同步而非顶层目录。
    let trimmed = remote.trim_end_matches('/');
    let src = format!("{user}@{host}:{trimmed}/");
    let local_str = local_path_for_rsync(local)?;

    let mut cmd = Command::new("rsync");
    cmd.args([
        "-az",
        "--include=*/",
        "--include=*.jsonl",
        "--exclude=*",
        "-e",
        ssh_e_arg,
        &src,
        &local_str,
    ]);
    if let Some(pw) = password {
        cmd.env(SSHPASS_ENV, pw);
    }
    let output = cmd.output().await.map_err(|e| {
        anyhow!("无法启动 rsync 命令：{e}\n请确认系统已安装 rsync（Windows 用户需单独安装）")
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let snippet = stderr
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(3)
            .collect::<Vec<_>>()
            .join("\n");
        return Err(anyhow!(
            "rsync 失败（exit={}）：\n{snippet}",
            output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_default()
        ));
    }
    Ok(())
}

// ============================================================
// 工具
// ============================================================

/// 把本地缓存目录转成 rsync 可识别的路径字符串。
///
/// rsync 用冒号区分本地/远端（`user@host:path`）。Windows 上的本地路径
/// `C:\Users\foo` 会被 rsync 误判成远端 `host=C` + `path=\Users\foo`，
/// 导致与远端源同时使用时报错 `source and destination cannot both be remote`。
/// MSYS2 编译的 rsync 期望本地路径用 POSIX 风格 `/c/Users/foo`。
///
/// 在非 Windows 平台原样返回。
fn local_path_for_rsync(p: &Path) -> Result<String> {
    let s = p
        .to_str()
        .ok_or_else(|| anyhow!("缓存路径不是 UTF-8: {}", p.display()))?;
    #[cfg(target_os = "windows")]
    {
        return Ok(windows_to_msys_path(s));
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(s.to_string())
    }
}

/// 把 Windows 路径转成 MSYS2 POSIX 风格：`C:\foo\bar` → `/c/foo/bar`。
///
/// 仅当字符串符合 `<盘符>:<分隔符>...` 时转换，否则原样返回。
/// 即使在非 Windows 平台编译，也保留此函数以便单元测试覆盖纯字符串逻辑。
fn windows_to_msys_path(s: &str) -> String {
    let bytes = s.as_bytes();
    let looks_like_drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    if !looks_like_drive {
        return s.to_string();
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    let rest: String = s[2..]
        .chars()
        .map(|c| if c == '\\' { '/' } else { c })
        .collect();
    format!("/{drive}{rest}")
}

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

/// 拼成 `rsync -e "ssh -o ... -p ... -i ..."` 所需的单参数字符串。
///
/// 密码方式时返回 `sshpass -e ssh ...`，rsync 进程必须同时设置
/// `SSHPASS` 环境变量（在 `rsync_jsonl` 中处理）。
fn build_rsync_ssh_arg(ws: &Workspace) -> String {
    let prefix = match ws.auth_method {
        SshAuthMethod::Key => String::from("ssh"),
        SshAuthMethod::Password => String::from("sshpass -e ssh"),
    };
    let mut s =
        format!("{prefix} -o StrictHostKeyChecking=no -o ConnectTimeout={CONNECT_TIMEOUT_SECS}");
    match ws.auth_method {
        SshAuthMethod::Key => {
            s.push_str(" -o BatchMode=yes");
            if let Some(k) = ws.ssh_key.as_deref() {
                if !k.is_empty() {
                    // 简单引用：路径中有空格时会出问题，但用户的私钥路径几乎不会有空格
                    s.push_str(&format!(" -i {}", expand_tilde(k)));
                }
            }
        }
        SshAuthMethod::Password => {
            s.push_str(" -o PreferredAuthentications=password,keyboard-interactive");
            s.push_str(" -o PubkeyAuthentication=no");
        }
    }
    if let Some(p) = ws.port {
        if p != 22 {
            s.push_str(&format!(" -p {p}"));
        }
    }
    s
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

    #[test]
    fn rsync_ssh_arg_format() {
        let s = build_rsync_ssh_arg(&ssh_ws());
        assert!(s.starts_with("ssh "));
        assert!(s.contains("BatchMode=yes"));
        assert!(s.contains("-p 2200"));
        assert!(s.contains("-i "));
    }

    #[test]
    fn rsync_ssh_arg_password_uses_sshpass() {
        let s = build_rsync_ssh_arg(&ssh_pw_ws());
        assert!(s.starts_with("sshpass -e ssh "));
        assert!(!s.contains("BatchMode=yes"));
        assert!(s.contains("PreferredAuthentications=password"));
        assert!(s.contains("PubkeyAuthentication=no"));
        assert!(s.contains("-p 2200"));
        // 密码本身绝不能出现在 rsync -e 参数里
        assert!(!s.contains("s3cret"));
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

    // -------- windows_to_msys_path（Windows 本地路径转 MSYS2 POSIX 风格） --------

    #[test]
    fn windows_to_msys_path_converts_backslash_drive() {
        assert_eq!(windows_to_msys_path(r"C:\Users\me\foo"), "/c/Users/me/foo");
        assert_eq!(windows_to_msys_path(r"D:\path\to\dir"), "/d/path/to/dir");
    }

    #[test]
    fn windows_to_msys_path_converts_forward_slash_drive() {
        // Rust 的 Path 在 Windows 上也接受正斜杠
        assert_eq!(windows_to_msys_path("C:/Users/me"), "/c/Users/me");
    }

    #[test]
    fn windows_to_msys_path_lowercases_drive_letter() {
        assert_eq!(windows_to_msys_path(r"G:\proj"), "/g/proj");
    }

    #[test]
    fn windows_to_msys_path_leaves_posix_paths_alone() {
        assert_eq!(windows_to_msys_path("/home/user/cache"), "/home/user/cache");
        assert_eq!(windows_to_msys_path("relative/path"), "relative/path");
    }

    #[test]
    fn windows_to_msys_path_leaves_unc_alone() {
        // UNC 路径 \\server\share —— 不符合「盘符 + 冒号」格式，原样返回
        assert_eq!(
            windows_to_msys_path(r"\\server\share\file"),
            r"\\server\share\file"
        );
    }

    #[test]
    fn windows_to_msys_path_requires_separator_after_colon() {
        // `C:foo`（无分隔符）不是有效的绝对盘符路径，保持原样不强行转换
        assert_eq!(windows_to_msys_path("C:foo"), "C:foo");
    }
}
