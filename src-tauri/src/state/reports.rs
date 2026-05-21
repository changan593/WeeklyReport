//! ReportRecord 元数据列表 + Markdown 正文文件 CRUD。

use anyhow::{anyhow, Result};
use uuid::Uuid;

use crate::report::ReportRecord;
use crate::store;

use super::F_REPORT_INDEX;

pub fn list_reports() -> Result<Vec<ReportRecord>> {
    store::read_json(F_REPORT_INDEX)
}

/// 保存一份新的报告：写 Markdown 文件 + 更新 index。
///
/// id 为空时自动生成 UUID。原子写。
pub fn save_report(mut record: ReportRecord, content: &str) -> Result<ReportRecord> {
    if record.id.is_empty() {
        record.id = Uuid::new_v4().to_string();
    }
    store::save_report_file(&record.id, content)?;

    let mut list = list_reports()?;
    match list.iter().position(|r| r.id == record.id) {
        Some(pos) => list[pos] = record.clone(),
        None => list.push(record.clone()),
    }
    store::write_json(F_REPORT_INDEX, &list)?;
    Ok(record)
}

/// 返回 (元数据, Markdown 正文)。
pub fn get_report(id: &str) -> Result<(ReportRecord, String)> {
    let list = list_reports()?;
    let record = list.into_iter().find(|r| r.id == id).ok_or_else(|| {
        anyhow!(crate::i18n::t_var(
            "err.state.report_not_found",
            &[("id", id)]
        ))
    })?;
    let content = store::load_report_file(id)?;
    Ok((record, content))
}

/// 删除报告：同时删除 .md 文件和 index.json 中的条目。
pub fn delete_report(id: &str) -> Result<()> {
    let mut list = list_reports()?;
    list.retain(|r| r.id != id);
    store::write_json(F_REPORT_INDEX, &list)?;
    store::delete_report_file(id)?;
    Ok(())
}
