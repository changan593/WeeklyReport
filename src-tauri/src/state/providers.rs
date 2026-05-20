//! LlmProvider 实体：CRUD + 默认源管理 + get_default_provider 优先级解析。

use anyhow::{bail, Result};
use uuid::Uuid;

use crate::llm::LlmProvider;
use crate::store;

use super::F_PROVIDERS;

pub fn list_providers() -> Result<Vec<LlmProvider>> {
    store::read_json(F_PROVIDERS)
}

/// 保存 provider（新增或更新）。维护「同时只有一个 default」不变量。
///
/// 规则：
/// - 新增且列表原本为空 → 自动设为 default
/// - 入参 `is_default = true` → 其他全部置 false
/// - 最终列表非空但没有 default → 第一个升级为 default
pub fn save_provider(mut p: LlmProvider) -> Result<LlmProvider> {
    if p.name.trim().is_empty() {
        bail!("LLM 源名称不能为空");
    }
    let new_id = p.id.is_empty();
    if new_id {
        p.id = Uuid::new_v4().to_string();
    }

    let mut list = list_providers()?;
    let existed = list.iter().any(|x| x.id == p.id);

    if new_id && list.is_empty() {
        p.is_default = true;
    }

    if p.is_default {
        for x in list.iter_mut() {
            if x.id != p.id {
                x.is_default = false;
            }
        }
    }

    if existed {
        if let Some(pos) = list.iter().position(|x| x.id == p.id) {
            list[pos] = p.clone();
        }
    } else {
        list.push(p.clone());
    }

    if !list.is_empty() && !list.iter().any(|x| x.is_default) {
        list[0].is_default = true;
        if list[0].id == p.id {
            p.is_default = true;
        }
    }

    store::write_json_secret(F_PROVIDERS, &list)?;
    Ok(p)
}

/// 删除 provider。若删除的是当前 default，则把剩余的第一个升级为 default。
pub fn delete_provider(id: &str) -> Result<()> {
    let mut list = list_providers()?;
    let pos = match list.iter().position(|x| x.id == id) {
        Some(i) => i,
        None => return Ok(()),
    };
    let was_default = list[pos].is_default;
    list.remove(pos);
    if was_default {
        if let Some(first) = list.first_mut() {
            first.is_default = true;
        }
    }
    store::write_json_secret(F_PROVIDERS, &list)
}

/// 按优先级返回当前应使用的 provider（参考 `docs/LLM.md#6-优先级解析`）。
///
/// 此函数只覆盖 3-5 步：default → 第一个 → 报错。调用方处理 1-2 步。
pub fn get_default_provider() -> Result<LlmProvider> {
    let list = list_providers()?;
    if let Some(p) = list.iter().find(|x| x.is_default) {
        return Ok(p.clone());
    }
    if let Some(p) = list.first() {
        return Ok(p.clone());
    }
    bail!("未配置任何 LLM 源")
}
