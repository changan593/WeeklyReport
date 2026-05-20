//! ReportRecord 元数据列表 + Markdown 正文文件 CRUD。
//!
//! 所有接受 `id` 的函数都通过 [`crate::validate::id`] 校验，防止
//! 通过 IPC 传 `../../etc/passwd` 实施任意文件删除。

use anyhow::{anyhow, Result};
use uuid::Uuid;

use crate::report::ReportRecord;
use crate::store;
use crate::validate;

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
    } else {
        validate::id(&record.id)?;
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
    validate::id(id)?;
    let list = list_reports()?;
    let record = list
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| anyhow!("报告不存在: {id}"))?;
    let content = store::load_report_file(id)?;
    Ok((record, content))
}

/// 删除报告：同时删除 .md 文件和 index.json 中的条目。
///
/// 顺序：先确认 id 在 index 中存在 → 再删 .md → 最后写 index。
/// 这样即便最后一步崩溃也只是留下空索引，不会出现"index 已删但 .md 还在"
/// 的 orphan 情况。
pub fn delete_report(id: &str) -> Result<()> {
    validate::id(id)?;
    let mut list = list_reports()?;
    if !list.iter().any(|r| r.id == id) {
        // 索引中没有 → 静默成功（容忍重复点击）
        return Ok(());
    }
    // 先删文件，失败时不修改 index，避免半成品状态
    store::delete_report_file(id)?;
    list.retain(|r| r.id != id);
    store::write_json(F_REPORT_INDEX, &list)?;
    Ok(())
}
