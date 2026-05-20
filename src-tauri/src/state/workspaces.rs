//! Workspace 实体的 list / save / delete + 首次启动默认工作区创建。

use anyhow::Result;
use uuid::Uuid;

use crate::store;
use crate::workspace::{self, Workspace, WorkspaceKind};

use super::F_WORKSPACES;

pub fn list_workspaces() -> Result<Vec<Workspace>> {
    store::read_json(F_WORKSPACES)
}

/// 保存 Workspace（新增或更新）。id 为空时自动生成 UUID。
///
/// SSH 工作区的 host / user 等字段会被 [`workspace::validate`] 严格校验，
/// 防止被当作 OpenSSH / rsync 命令行选项注入。
/// 文件以 0600 权限写入（含 SSH 私钥路径、内部主机名等半敏感字段）。
pub fn save_workspace(mut ws: Workspace) -> Result<Workspace> {
    if ws.id.is_empty() {
        ws.id = Uuid::new_v4().to_string();
    } else {
        crate::validate::id(&ws.id)?;
    }
    workspace::validate(&ws)?;

    let mut list = list_workspaces()?;
    match list.iter().position(|w| w.id == ws.id) {
        Some(pos) => list[pos] = ws.clone(),
        None => list.push(ws.clone()),
    }
    store::write_json_secret(F_WORKSPACES, &list)?;
    Ok(ws)
}

/// 删除工作区。同时：
/// 1. 校验 ID 格式（防路径穿越）
/// 2. 从持久化列表移除
/// 3. 级联清理本地缓存目录（远端 rsync 拉回的全部 jsonl）
///
/// 不阻止"删除被 schedule 引用的工作区"——schedule 执行时会自然报错并
/// 在 last_status 中显示，避免硬阻断用户的合理操作。
pub fn delete_workspace(id: &str) -> Result<()> {
    crate::validate::id(id)?;
    let mut list = list_workspaces()?;
    let before = list.len();
    list.retain(|w| w.id != id);
    if list.len() == before {
        return Ok(());
    }
    store::write_json_secret(F_WORKSPACES, &list)?;
    // 清理可能存在的 SSH 缓存目录（含敏感日志数据）
    crate::ssh::cleanup_cache(id);
    Ok(())
}

/// 首次启动时若没有任何 Workspace，则创建一个指向 `~/.claude` / `~/.codex` 的本机工作区。
///
/// 已有 workspace 时不做任何事（幂等）。
pub fn ensure_default_workspace() -> Result<()> {
    let existing = list_workspaces()?;
    if !existing.is_empty() {
        return Ok(());
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法定位用户家目录"))?;
    let ws = Workspace {
        id: Uuid::new_v4().to_string(),
        name: "本机".to_string(),
        kind: WorkspaceKind::Local,
        host: None,
        user: None,
        port: None,
        ssh_key: None,
        claude_path: Some(home.join(".claude").to_string_lossy().into_owned()),
        codex_path: Some(home.join(".codex").to_string_lossy().into_owned()),
        tools: vec!["claude-code".to_string(), "codex".to_string()],
    };
    save_workspace(ws)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 注意：save_workspace 依赖全局 DATA_ROOT；此处不重复 state.rs 顶层
    // 已有的端到端测试，只针对新加的"校验拦截"路径做轻量回归。

    #[test]
    fn save_rejects_dash_prefix_ssh_host() {
        // 即使绕过前端直接给后端送恶意 host，也应在 save 时被拒绝
        let ws = Workspace {
            id: String::new(),
            name: "evil".into(),
            kind: WorkspaceKind::Ssh,
            host: Some("-oProxyCommand=/bin/sh".into()),
            user: Some("alice".into()),
            ..Default::default()
        };
        let r = save_workspace(ws);
        assert!(r.is_err());
    }

    #[test]
    fn save_rejects_malformed_id() {
        let ws = Workspace {
            id: "../../etc".into(),
            name: "x".into(),
            kind: WorkspaceKind::Local,
            ..Default::default()
        };
        assert!(save_workspace(ws).is_err());
    }

    #[test]
    fn delete_rejects_malformed_id() {
        assert!(delete_workspace("../foo").is_err());
        assert!(delete_workspace("a/b").is_err());
    }
}
