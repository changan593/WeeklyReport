//! Workspace 实体的 list / save / delete + 首次启动默认工作区创建。

use anyhow::{bail, Result};
use uuid::Uuid;

use crate::store;
use crate::workspace::{Workspace, WorkspaceKind};

use super::F_WORKSPACES;

pub fn list_workspaces() -> Result<Vec<Workspace>> {
    store::read_json(F_WORKSPACES)
}

/// 保存 Workspace（新增或更新）。id 为空时自动生成 UUID。
pub fn save_workspace(mut ws: Workspace) -> Result<Workspace> {
    if ws.id.is_empty() {
        ws.id = Uuid::new_v4().to_string();
    }
    if ws.name.trim().is_empty() {
        bail!("工作区名称不能为空");
    }
    let mut list = list_workspaces()?;
    match list.iter().position(|w| w.id == ws.id) {
        Some(pos) => list[pos] = ws.clone(),
        None => list.push(ws.clone()),
    }
    store::write_json(F_WORKSPACES, &list)?;
    Ok(ws)
}

pub fn delete_workspace(id: &str) -> Result<()> {
    let mut list = list_workspaces()?;
    let before = list.len();
    list.retain(|w| w.id != id);
    if list.len() == before {
        return Ok(());
    }
    store::write_json(F_WORKSPACES, &list)
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
