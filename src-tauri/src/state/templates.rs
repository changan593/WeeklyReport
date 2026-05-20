//! Template 实体：CRUD + 内置 3 模板始终注入。

use anyhow::{bail, Result};
use uuid::Uuid;

use crate::report::Template;
use crate::store;

use super::{schedules, F_TEMPLATES};

/// 列出所有模板：内置 3 个 + 用户自定义。
///
/// 内置始终在前，顺序固定（tech / exec / simple）。
pub fn list_templates() -> Result<Vec<Template>> {
    let mut customs: Vec<Template> = store::read_json(F_TEMPLATES)?;
    customs.retain(|t| !t.builtin && !t.id.starts_with("builtin-"));
    let mut result = builtin_templates();
    result.extend(customs);
    Ok(result)
}

/// 保存自定义模板。拒绝写入内置模板。
pub fn save_template(mut t: Template) -> Result<Template> {
    if t.builtin || t.id.starts_with("builtin-") {
        bail!("不可修改内置模板");
    }
    if t.name.trim().is_empty() {
        bail!("模板名称不能为空");
    }
    if t.sections.is_empty() {
        bail!("模板至少需要一个章节");
    }
    if t.id.is_empty() {
        t.id = Uuid::new_v4().to_string();
    }
    let mut customs: Vec<Template> = store::read_json(F_TEMPLATES)?;
    customs.retain(|x| !x.builtin && !x.id.starts_with("builtin-"));
    match customs.iter().position(|x| x.id == t.id) {
        Some(pos) => customs[pos] = t.clone(),
        None => customs.push(t.clone()),
    }
    store::write_json(F_TEMPLATES, &customs)?;
    Ok(t)
}

/// 删除自定义模板。拒绝删除内置；若被定时任务引用则拒绝。
pub fn delete_template(id: &str) -> Result<()> {
    if id.starts_with("builtin-") {
        bail!("不可删除内置模板");
    }
    let schedules = schedules::list_schedules()?;
    let users: Vec<String> = schedules
        .iter()
        .filter(|s| s.template_id == id)
        .map(|s| s.name.clone())
        .collect();
    if !users.is_empty() {
        bail!("无法删除：以下定时任务正在使用此模板: {}", users.join(", "));
    }
    let mut customs: Vec<Template> = store::read_json(F_TEMPLATES)?;
    customs.retain(|t| t.id != id);
    store::write_json(F_TEMPLATES, &customs)
}

/// 3 个内置模板。
fn builtin_templates() -> Vec<Template> {
    vec![
        Template {
            id: "builtin-tech".into(),
            name: "技术周报".into(),
            style: "tech".into(),
            sections: vec![
                "本周 TL;DR".into(),
                "各项目进展".into(),
                "技术亮点".into(),
                "下周计划".into(),
            ],
            provider_id: None,
            extra_prompt: String::new(),
            builtin: true,
        },
        Template {
            id: "builtin-exec".into(),
            name: "管理层汇报".into(),
            style: "exec".into(),
            sections: vec![
                "执行摘要".into(),
                "关键进展".into(),
                "风险阻塞".into(),
                "下周重点".into(),
            ],
            provider_id: None,
            extra_prompt: String::new(),
            builtin: true,
        },
        Template {
            id: "builtin-simple".into(),
            name: "简洁日报".into(),
            style: "simple".into(),
            sections: vec!["做了啥".into(), "问题".into(), "下周".into()],
            provider_id: None,
            extra_prompt: String::new(),
            builtin: true,
        },
    ]
}
