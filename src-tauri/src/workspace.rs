//! Workspace 数据模型 + 路径工具 + 输入校验。
//!
//! 连接测试逻辑（`test_connection`）：
//! - 本机：检查 claude_path / codex_path 是否存在
//! - SSH：调用 `crate::ssh::test`
//!
//! 详见 `docs/ARCHITECTURE.md#33-workspacers`。
#![allow(dead_code)]

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::validate;

/// Workspace 类型：本机或 SSH 远程。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceKind {
    #[default]
    Local,
    Ssh,
}

/// 一个日志来源（本机或远程 SSH 服务器）。
///
/// JSON 字段 `type` 对应 Rust 字段 `kind`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: WorkspaceKind,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_path: Option<String>,

    #[serde(default)]
    pub tools: Vec<String>,
}

/// 校验 Workspace 字段。在 `save_workspace` 与 `test_connection` 前都应调用。
///
/// SSH 工作区强校验 `host` / `user`（防 OpenSSH 参数注入）；本机不需要。
pub fn validate(ws: &Workspace) -> Result<()> {
    if ws.name.trim().is_empty() {
        bail!("工作区名称不能为空");
    }
    if ws.name.len() > 64 {
        bail!("工作区名称过长（最多 64 字符）");
    }
    match ws.kind {
        WorkspaceKind::Local => {}
        WorkspaceKind::Ssh => {
            let host = ws
                .host
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("SSH 工作区缺少 host"))?;
            validate::ssh_host(host)?;
            // user 缺省时 ssh 用客户端当前用户；填了就校验
            if let Some(u) = ws.user.as_deref() {
                if !u.is_empty() {
                    validate::ssh_user(u)?;
                }
            }
            // ssh_key 路径不在白名单内（用户输入），仅做基本健全性检查
            if let Some(k) = ws.ssh_key.as_deref() {
                if k.chars().any(|c| c.is_control()) {
                    bail!("SSH 私钥路径不能含控制字符");
                }
            }
        }
    }
    Ok(())
}

/// 测试 workspace 连接。
///
/// - `Local`：检查 claude_path / codex_path 是否存在，返回多行报告
/// - `Ssh`：调用 `crate::ssh::test`
///
/// 返回的字符串可直接渲染到 UI 的 StatusBanner。
pub async fn test_connection(ws: &Workspace) -> anyhow::Result<String> {
    validate(ws)?;
    match ws.kind {
        WorkspaceKind::Local => Ok(test_local(ws)),
        WorkspaceKind::Ssh => crate::ssh::test(ws).await,
    }
}

fn test_local(ws: &Workspace) -> String {
    let mut lines = Vec::new();
    lines.push(format!("✓ 本机工作区「{}」", ws.name));

    let want_claude = ws.tools.iter().any(|t| t == "claude-code");
    let want_codex = ws.tools.iter().any(|t| t == "codex");

    if !want_claude && !want_codex {
        lines.push("⚠ 未启用任何工具，请至少勾选 Claude Code 或 Codex".into());
        return lines.join("\n");
    }

    if want_claude {
        let raw = ws.claude_path.as_deref().unwrap_or("~/.claude");
        let expanded = expand_tilde(raw);
        let exists = std::path::Path::new(&expanded).is_dir();
        let mark = if exists { "✓" } else { "✗" };
        lines.push(format!(
            "{mark} Claude Code 路径{}：{expanded}",
            if exists { "存在" } else { "不存在" }
        ));
    }

    if want_codex {
        let raw = ws.codex_path.as_deref().unwrap_or("~/.codex");
        let expanded = expand_tilde(raw);
        let exists = std::path::Path::new(&expanded).is_dir();
        let mark = if exists { "✓" } else { "✗" };
        lines.push(format!(
            "{mark} Codex 路径{}：{expanded}",
            if exists { "存在" } else { "不存在" }
        ));
    }

    lines.join("\n")
}

/// 把路径中的 `~` 展开为用户家目录。失败时原样返回。
///
/// 仅处理形如 `~`、`~/foo` 的路径，不支持 `~user/` 这种 POSIX 形式。
pub fn expand_tilde(path: &str) -> String {
    if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.to_string_lossy().into_owned();
        }
        return path.to_string();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_kind_serializes_lowercase() {
        let json = serde_json::to_string(&WorkspaceKind::Local).unwrap();
        assert_eq!(json, "\"local\"");
        let json = serde_json::to_string(&WorkspaceKind::Ssh).unwrap();
        assert_eq!(json, "\"ssh\"");
    }

    #[test]
    fn workspace_serde_uses_type_field() {
        let ws = Workspace {
            id: "id1".into(),
            name: "本机".into(),
            kind: WorkspaceKind::Local,
            tools: vec!["claude-code".into()],
            ..Default::default()
        };
        let json = serde_json::to_value(&ws).unwrap();
        assert_eq!(json["type"], "local");
        assert_eq!(json["id"], "id1");
        // 反序列化回来
        let back: Workspace = serde_json::from_value(json).unwrap();
        assert_eq!(back, ws);
    }

    #[test]
    fn expand_tilde_no_tilde() {
        assert_eq!(expand_tilde("/usr/local/bin"), "/usr/local/bin");
        assert_eq!(expand_tilde("relative/path"), "relative/path");
        assert_eq!(expand_tilde(""), "");
    }

    #[test]
    fn expand_tilde_replaces_home_prefix() {
        if let Some(home) = dirs::home_dir() {
            let home_s = home.to_string_lossy();
            assert_eq!(expand_tilde("~"), home_s);
            let expanded = expand_tilde("~/.claude");
            assert!(expanded.starts_with(home_s.as_ref()));
            assert!(expanded.ends_with(".claude"));
        }
    }

    // -------- validate --------

    fn ssh_ws() -> Workspace {
        Workspace {
            id: "w".into(),
            name: "S".into(),
            kind: WorkspaceKind::Ssh,
            host: Some("example.com".into()),
            user: Some("alice".into()),
            port: Some(22),
            ssh_key: None,
            claude_path: None,
            codex_path: None,
            tools: vec![],
        }
    }

    #[test]
    fn validate_accepts_normal_ssh_workspace() {
        assert!(validate(&ssh_ws()).is_ok());
    }

    #[test]
    fn validate_rejects_dash_prefix_host() {
        let mut ws = ssh_ws();
        ws.host = Some("-oProxyCommand=evil".into());
        assert!(validate(&ws).is_err(), "应拒绝 OpenSSH 选项注入");
    }

    #[test]
    fn validate_rejects_dash_prefix_user() {
        let mut ws = ssh_ws();
        ws.user = Some("-oUser=root".into());
        assert!(validate(&ws).is_err());
    }

    #[test]
    fn validate_rejects_shell_metachars_in_host() {
        let mut ws = ssh_ws();
        ws.host = Some("example.com;rm -rf /".into());
        assert!(validate(&ws).is_err());
    }

    #[test]
    fn validate_rejects_empty_name() {
        let mut ws = ssh_ws();
        ws.name = "  ".into();
        assert!(validate(&ws).is_err());
    }

    #[test]
    fn validate_local_only_checks_name() {
        let ws = Workspace {
            id: String::new(),
            name: "本机".into(),
            kind: WorkspaceKind::Local,
            ..Default::default()
        };
        assert!(validate(&ws).is_ok());
    }

    #[test]
    fn validate_rejects_control_chars_in_ssh_key() {
        let mut ws = ssh_ws();
        ws.ssh_key = Some("~/.ssh/id_rsa\n".into());
        assert!(validate(&ws).is_err());
    }
}
