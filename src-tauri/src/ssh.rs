//! SSH 客户端：测试连接 + rsync 同步 *.jsonl 到本地缓存。
//!
//! 不引入 `ssh2` crate，改用系统 `ssh` 和 `rsync` 命令子进程（见
//! `docs/DECISIONS.md#adr-010ssh-使用系统命令而非-ssh2-crate`）。
//!
//! 所有 SSH 调用都加：
//! - `BatchMode=yes`（禁止任何交互式密码输入）
//! - `StrictHostKeyChecking=no`（不卡 known_hosts，初次连接也能跑）
//! - `ConnectTimeout=8`（连接 8 秒超时）
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::workspace::{expand_tilde, Workspace, WorkspaceKind};

const CONNECT_TIMEOUT_SECS: u32 = 8;

// ============================================================
// test_connection
// ============================================================

/// 测试 SSH 工作区：用 `ssh ... echo OK && test -d <path>` 验证连通性 + 路径存在。
///
/// 返回多行可读字符串，每行 `✓` / `✗` 标记，可直接渲染到 UI StatusBanner。
pub async fn test(ws: &Workspace) -> Result<String> {
    require_ssh(ws)?;
    let user = ws.user.as_deref().unwrap_or("root");
    let host = ws.host.as_deref().unwrap_or_default();
    let port = ws.port.unwrap_or(22);

    let want_claude = ws.tools.iter().any(|t| t == "claude-code");
    let want_codex = ws.tools.iter().any(|t| t == "codex");
    let claude_path = ws.claude_path.as_deref().unwrap_or("~/.claude");
    let codex_path = ws.codex_path.as_deref().unwrap_or("~/.codex");

    // 在远端执行的 shell 脚本：echo OK + 各 path 的存在性检查
    let mut script = String::from("echo OK");
    if want_claude {
        script.push_str(&format!(
            " && (test -d \"{claude_path}\" && echo CLAUDE_OK || echo CLAUDE_MISSING)"
        ));
    }
    if want_codex {
        script.push_str(&format!(
            " && (test -d \"{codex_path}\" && echo CODEX_OK || echo CODEX_MISSING)"
        ));
    }

    let mut cmd = Command::new("ssh");
    cmd.args(base_ssh_args(ws));
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
fn format_ssh_error(stderr: &str, exit_code: Option<i32>) -> anyhow::Error {
    let lower = stderr.to_lowercase();
    let hint = if lower.contains("connection timed out") || lower.contains("operation timed out") {
        format!("SSH 连接超时（{CONNECT_TIMEOUT_SECS} 秒）：检查 host 和网络")
    } else if lower.contains("permission denied") || lower.contains("publickey") {
        "SSH 认证失败：检查 ssh_key 路径或确认公钥已添加到服务端 ~/.ssh/authorized_keys".into()
    } else if lower.contains("could not resolve") || lower.contains("name or service not known") {
        "无法解析主机名：检查 host 拼写".into()
    } else if lower.contains("no route to host") || lower.contains("network is unreachable") {
        "无法连接到 host：检查网络与防火墙".into()
    } else if lower.contains("host key verification failed") {
        "Host key 校验失败（罕见，因为我们已设 StrictHostKeyChecking=no）".into()
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
    let ssh_e_arg = build_rsync_ssh_arg(ws);

    let root = cache_root(&ws.id)?;
    std::fs::create_dir_all(&root)?;

    let mut out = HashMap::new();
    if ws.tools.iter().any(|t| t == "claude-code") {
        let remote = ws.claude_path.as_deref().unwrap_or("~/.claude");
        let local = root.join("claude");
        std::fs::create_dir_all(&local)?;
        rsync_jsonl(&ssh_e_arg, user, host, remote, &local).await?;
        out.insert("claude-code".into(), local);
    }
    if ws.tools.iter().any(|t| t == "codex") {
        let remote = ws.codex_path.as_deref().unwrap_or("~/.codex");
        let local = root.join("codex");
        std::fs::create_dir_all(&local)?;
        rsync_jsonl(&ssh_e_arg, user, host, remote, &local).await?;
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
) -> Result<()> {
    // 远端路径末尾加 `/` 让 rsync 按子树同步而非顶层目录。
    let trimmed = remote.trim_end_matches('/');
    let src = format!("{user}@{host}:{trimmed}/");
    let local_str = local
        .to_str()
        .ok_or_else(|| anyhow!("缓存路径不是 UTF-8: {}", local.display()))?;

    let output = Command::new("rsync")
        .args([
            "-az",
            "--include=*/",
            "--include=*.jsonl",
            "--exclude=*",
            "-e",
            ssh_e_arg,
            &src,
            local_str,
        ])
        .output()
        .await
        .map_err(|e| {
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

fn require_ssh(ws: &Workspace) -> Result<()> {
    if ws.kind != WorkspaceKind::Ssh {
        return Err(anyhow!("不是 SSH 工作区"));
    }
    if ws.host.as_deref().unwrap_or("").is_empty() {
        return Err(anyhow!("SSH 工作区缺少 host"));
    }
    Ok(())
}

fn base_ssh_args(ws: &Workspace) -> Vec<String> {
    let mut args = vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "StrictHostKeyChecking=no".into(),
        "-o".into(),
        format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}"),
    ];
    if let Some(p) = ws.port {
        if p != 22 {
            args.push("-p".into());
            args.push(p.to_string());
        }
    }
    if let Some(k) = ws.ssh_key.as_deref() {
        if !k.is_empty() {
            args.push("-i".into());
            args.push(expand_tilde(k));
        }
    }
    args
}

/// 拼成 `rsync -e "ssh -o ... -p ... -i ..."` 所需的单参数字符串。
fn build_rsync_ssh_arg(ws: &Workspace) -> String {
    let mut s = format!(
        "ssh -o BatchMode=yes -o StrictHostKeyChecking=no -o ConnectTimeout={CONNECT_TIMEOUT_SECS}"
    );
    if let Some(p) = ws.port {
        if p != 22 {
            s.push_str(&format!(" -p {p}"));
        }
    }
    if let Some(k) = ws.ssh_key.as_deref() {
        if !k.is_empty() {
            // 简单引用：路径中有空格时会出问题，但用户的私钥路径几乎不会有空格
            s.push_str(&format!(" -i {}", expand_tilde(k)));
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
            ssh_key: Some("~/.ssh/id_ed25519".into()),
            claude_path: Some("/home/alice/.claude".into()),
            codex_path: Some("/home/alice/.codex".into()),
            tools: vec!["claude-code".into(), "codex".into()],
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
    fn rsync_ssh_arg_format() {
        let s = build_rsync_ssh_arg(&ssh_ws());
        assert!(s.starts_with("ssh "));
        assert!(s.contains("BatchMode=yes"));
        assert!(s.contains("-p 2200"));
        assert!(s.contains("-i "));
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
        );
        let s = e.to_string();
        assert!(s.contains("超时"));
    }

    #[test]
    fn format_ssh_error_recognizes_auth() {
        let e = format_ssh_error(
            "alice@example.com: Permission denied (publickey)",
            Some(255),
        );
        let s = e.to_string();
        assert!(s.contains("认证失败"));
    }

    #[test]
    fn format_ssh_error_recognizes_dns() {
        let e = format_ssh_error(
            "ssh: Could not resolve hostname bogus.example.com",
            Some(255),
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

    #[tokio::test]
    async fn sync_to_cache_rejects_local() {
        let mut ws = ssh_ws();
        ws.kind = WorkspaceKind::Local;
        assert!(sync_to_cache(&ws).await.is_err());
    }
}
