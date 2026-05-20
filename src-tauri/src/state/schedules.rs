//! Schedule 实体：CRUD + update_schedule_run_status（仅更新两字段）。

use anyhow::{bail, Result};
use uuid::Uuid;

use crate::scheduler::Schedule;
use crate::store;

use super::F_SCHEDULES;

pub fn list_schedules() -> Result<Vec<Schedule>> {
    store::read_json(F_SCHEDULES)
}

pub fn save_schedule(mut s: Schedule) -> Result<Schedule> {
    if s.name.trim().is_empty() {
        bail!("任务名称不能为空");
    }
    if s.cron.trim().is_empty() {
        bail!("cron 表达式不能为空");
    }
    if s.id.is_empty() {
        s.id = Uuid::new_v4().to_string();
    }
    // 不持久化 next_run（运行时计算）
    s.next_run = None;

    let mut list = list_schedules()?;
    match list.iter().position(|x| x.id == s.id) {
        Some(pos) => list[pos] = s.clone(),
        None => list.push(s.clone()),
    }
    store::write_json(F_SCHEDULES, &list)?;
    Ok(s)
}

pub fn delete_schedule(id: &str) -> Result<()> {
    let mut list = list_schedules()?;
    list.retain(|x| x.id != id);
    store::write_json(F_SCHEDULES, &list)
}

/// 只更新 schedule 的 `last_run` / `last_status` 字段；其他字段原样保留。
///
/// 给 scheduler 在任务完成（成功或失败）后调用。
pub fn update_schedule_run_status(
    id: &str,
    last_run: Option<String>,
    last_status: Option<String>,
) -> Result<()> {
    let mut list = list_schedules()?;
    if let Some(s) = list.iter_mut().find(|s| s.id == id) {
        s.last_run = last_run;
        s.last_status = last_status;
        store::write_json(F_SCHEDULES, &list)?;
    }
    Ok(())
}
