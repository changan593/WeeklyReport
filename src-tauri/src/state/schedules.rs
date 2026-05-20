//! Schedule 实体：CRUD + update_schedule_run_status（仅更新两字段）。
//!
//! 文件以 0600 权限写入：含收件人邮箱、cc、SMTP 触发等隐私字段。

use anyhow::{bail, Result};
use uuid::Uuid;

use crate::scheduler::Schedule;
use crate::store;
use crate::validate;

use super::F_SCHEDULES;

pub fn list_schedules() -> Result<Vec<Schedule>> {
    store::read_json(F_SCHEDULES)
}

pub fn save_schedule(mut s: Schedule) -> Result<Schedule> {
    if s.name.trim().is_empty() {
        bail!("任务名称不能为空");
    }
    if s.name.len() > 128 {
        bail!("任务名称过长");
    }
    if s.cron.trim().is_empty() {
        bail!("cron 表达式不能为空");
    }
    if s.cron.len() > 200 {
        bail!("cron 表达式过长");
    }
    if s.id.is_empty() {
        s.id = Uuid::new_v4().to_string();
    } else {
        validate::id(&s.id)?;
    }
    // 模板 id / provider id 必须是合法 ID 形态（防被注入到将来的路径拼接）
    validate::id(&s.template_id)?;
    if let Some(pid) = s.provider_id.as_deref() {
        if !pid.is_empty() {
            validate::id(pid)?;
        }
    }
    for wid in &s.workspace_ids {
        validate::id(wid)?;
    }
    // 校验所有邮箱（防 CRLF 头注入）
    for addr in s.recipients.iter().chain(s.cc.iter()) {
        validate::email(addr)?;
    }
    // 主题模板：禁止换行（subject 是单行）
    if !s.subject_tpl.is_empty() {
        validate::mail_header_text(&s.subject_tpl)?;
    }

    // 不持久化 next_run（运行时计算）
    s.next_run = None;

    let mut list = list_schedules()?;
    match list.iter().position(|x| x.id == s.id) {
        Some(pos) => list[pos] = s.clone(),
        None => list.push(s.clone()),
    }
    store::write_json_secret(F_SCHEDULES, &list)?;
    Ok(s)
}

pub fn delete_schedule(id: &str) -> Result<()> {
    validate::id(id)?;
    let mut list = list_schedules()?;
    list.retain(|x| x.id != id);
    store::write_json_secret(F_SCHEDULES, &list)
}

/// 只更新 schedule 的 `last_run` / `last_status` 字段；其他字段原样保留。
///
/// 给 scheduler 在任务完成（成功或失败）后调用。
pub fn update_schedule_run_status(
    id: &str,
    last_run: Option<String>,
    last_status: Option<String>,
) -> Result<()> {
    validate::id(id)?;
    let mut list = list_schedules()?;
    if let Some(s) = list.iter_mut().find(|s| s.id == id) {
        s.last_run = last_run;
        s.last_status = last_status;
        store::write_json_secret(F_SCHEDULES, &list)?;
    }
    Ok(())
}
