//! LlmProvider 实体：CRUD + 默认源管理 + get_default_provider 优先级解析。

use anyhow::{bail, Result};
use uuid::Uuid;

use crate::llm::LlmProvider;
use crate::store;
use crate::validate;

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
    if p.name.len() > 64 {
        bail!("LLM 源名称过长");
    }
    let new_id = p.id.is_empty();
    if new_id {
        p.id = Uuid::new_v4().to_string();
    } else {
        validate::id(&p.id)?;
    }
    // base_url 不允许换行/控制字符（防止意外注入到将来的 URL 拼接逻辑）
    if p.base_url.chars().any(|c| c.is_control()) {
        bail!("base_url 含控制字符");
    }
    // API key 不允许换行（lettre/HTTP 头部 CRLF 注入兜底）
    if p.api_key.contains('\r') || p.api_key.contains('\n') {
        bail!("API key 不能含换行");
    }
    // model 不允许换行/控制字符；它会被 Gemini 协议直接拼到 URL 路径中
    if p.model.chars().any(|c| c.is_control()) {
        bail!("model 含控制字符");
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
///
/// 安全：删除前检查是否被 template / schedule 引用，若有则报错并列出引用方，
/// 让用户先解除引用再删。
pub fn delete_provider(id: &str) -> Result<()> {
    validate::id(id)?;
    // 检查引用方
    let templates = super::templates::list_templates()?;
    let used_by_templates: Vec<String> = templates
        .iter()
        .filter(|t| t.provider_id.as_deref() == Some(id))
        .map(|t| t.name.clone())
        .collect();
    let schedules = super::schedules::list_schedules()?;
    let used_by_schedules: Vec<String> = schedules
        .iter()
        .filter(|s| s.provider_id.as_deref() == Some(id))
        .map(|s| s.name.clone())
        .collect();
    if !used_by_templates.is_empty() || !used_by_schedules.is_empty() {
        let mut msgs = Vec::new();
        if !used_by_templates.is_empty() {
            msgs.push(format!("模板: {}", used_by_templates.join(", ")));
        }
        if !used_by_schedules.is_empty() {
            msgs.push(format!("定时任务: {}", used_by_schedules.join(", ")));
        }
        bail!("无法删除：以下条目正在引用此 LLM 源 - {}", msgs.join("; "));
    }

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
