//! 定时任务：数据模型 `Schedule` + `SchedulerState`（tokio-cron-scheduler 包装）
//! + `execute_schedule` 完整流程 + `next_run_time` 计算。
//!
//! Cron 用 **7 段** 格式（秒 分 时 日 月 星期 年），见 ADR-009。
//! 详见 `docs/ARCHITECTURE.md#39-schedulerrs`。
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use chrono::{Datelike, Local, Utc};
use cron::Schedule as CronSchedule;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_cron_scheduler::{Job, JobScheduler};
use uuid::Uuid;

use crate::email::{self, EmailRequest};
use crate::report::{self, ReportRecord};
use crate::state;

// ============================================================
// 数据模型
// ============================================================

/// 一条定时任务配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Schedule {
    pub id: String,
    pub name: String,
    pub cron: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub workspace_ids: Vec<String>,
    pub template_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub days: u32,
    #[serde(default)]
    pub recipients: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub subject_tpl: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run: Option<String>,
}

// ============================================================
// SchedulerState
// ============================================================

/// 调度器实例，包装 tokio-cron-scheduler。
///
/// 用 `Arc<Mutex<HashMap>>` 追踪 `schedule_id → job_uuid`，便于按 schedule id
/// 增删改任务。`JobScheduler` 自身已经是 Arc，但显式包一层可读性更好。
pub struct SchedulerState {
    inner: JobScheduler,
    /// schedule_id → tokio-cron-scheduler job uuid
    jobs: Arc<Mutex<HashMap<String, Uuid>>>,
}

impl SchedulerState {
    /// 创建并 **启动** scheduler。返回后即可 `reload_all()` 加载已存任务。
    pub async fn new() -> Result<Self> {
        let scheduler = JobScheduler::new()
            .await
            .map_err(|e| anyhow!("初始化 scheduler 失败: {e}"))?;
        scheduler
            .start()
            .await
            .map_err(|e| anyhow!("启动 scheduler 失败: {e}"))?;
        Ok(Self {
            inner: scheduler,
            jobs: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 从 `schedules.json` 加载所有 `enabled` 的任务。
    ///
    /// 先清空当前已注册任务再加载，便于"修改设置文件后让 UI 触发重载"场景。
    /// 单条任务加载失败 `warn!` 后继续，不中断整体。
    pub async fn reload_all(&self) -> Result<()> {
        // 清空
        let old_ids: Vec<Uuid> = {
            let mut jobs = self.jobs.lock().await;
            let v: Vec<_> = jobs.values().copied().collect();
            jobs.clear();
            v
        };
        for uuid in old_ids {
            let _ = self.inner.remove(&uuid).await;
        }

        // 重新加载
        let all = state::list_schedules()?;
        for sch in all.into_iter().filter(|s| s.enabled) {
            if let Err(e) = self.add_job(&sch).await {
                tracing::warn!("加载定时任务 {} 失败: {:#}", sch.name, e);
            }
        }
        Ok(())
    }

    /// 更新或新增任务。若 enabled=false，则只移除现有 job。
    pub async fn refresh_job(&self, sch: &Schedule) -> Result<()> {
        self.remove_job(&sch.id).await?;
        if sch.enabled {
            self.add_job(sch).await?;
        }
        Ok(())
    }

    /// 移除任务。任务不存在时静默成功。
    pub async fn remove_job(&self, id: &str) -> Result<()> {
        let uuid = self.jobs.lock().await.remove(id);
        if let Some(u) = uuid {
            let _ = self.inner.remove(&u).await;
        }
        Ok(())
    }

    async fn add_job(&self, sch: &Schedule) -> Result<()> {
        let sch_owned = sch.clone();
        let job = Job::new_async(sch.cron.as_str(), move |_uuid, _lock| {
            let s = sch_owned.clone();
            Box::pin(async move {
                if let Err(e) = execute_schedule(&s).await {
                    tracing::error!("schedule {} 执行失败: {:#}", s.name, e);
                }
            })
        })
        .map_err(|e| anyhow!("cron 表达式无效或 job 构造失败: {e}"))?;
        let uuid = self
            .inner
            .add(job)
            .await
            .map_err(|e| anyhow!("添加 job 失败: {e}"))?;
        self.jobs.lock().await.insert(sch.id.clone(), uuid);
        Ok(())
    }
}

// ============================================================
// 执行
// ============================================================

/// 完整任务流程：生成报告 → 发邮件 → 更新 last_run / last_status。
///
/// 即使失败也会更新 `last_status`，便于 UI 显示原因。
pub async fn execute_schedule(sch: &Schedule) -> Result<()> {
    let started = Local::now();
    let outcome = run_once(sch).await;

    let now_iso = Local::now().to_rfc3339();
    let status = match &outcome {
        Ok(detail) => format!("success: {detail}"),
        Err(e) => format!("failed: {e:#}"),
    };
    if let Err(e) = state::update_schedule_run_status(&sch.id, Some(now_iso), Some(status)) {
        tracing::warn!("写回 schedule {} 状态失败: {:#}", sch.id, e);
    }
    let dt_ms = (Local::now() - started).num_milliseconds();
    tracing::info!("schedule {} 完成（{} ms）", sch.name, dt_ms);
    outcome.map(|_| ())
}

async fn run_once(sch: &Schedule) -> Result<String> {
    if sch.workspace_ids.is_empty() {
        return Err(anyhow!("workspace_ids 为空"));
    }
    if sch.recipients.is_empty() {
        return Err(anyhow!("recipients 为空，没有发件目标"));
    }

    // 1. 生成报告
    let output = report::run_generation(
        &sch.workspace_ids,
        &sch.template_id,
        sch.days,
        sch.provider_id.as_deref(),
    )
    .await?;

    // 2. 发邮件
    let smtp = state::get_smtp_config()?;
    let subject = render_subject(&sch.subject_tpl, &output.record);
    let email_req = EmailRequest {
        to: sch.recipients.clone(),
        cc: sch.cc.clone(),
        subject,
        body_markdown: output.content,
    };
    email::send(&smtp, &email_req).await?;

    Ok(format!(
        "report={} tokens={} duration={}ms",
        output.record.id, output.record.tokens_used, output.duration_ms
    ))
}

/// 渲染邮件主题模板：替换 `{date}` 和 `{week}` 占位符。
pub fn render_subject(tpl: &str, _record: &ReportRecord) -> String {
    let now = Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let week = format!("W{:02}", now.iso_week().week());

    let raw = if tpl.trim().is_empty() {
        "周报 {date}"
    } else {
        tpl
    };
    raw.replace("{date}", &date).replace("{week}", &week)
}

// ============================================================
// next_run_time
// ============================================================

/// 计算给定 cron 表达式的下一次运行时间，返回 **本地时区** ISO 8601。
///
/// **关键**：tokio-cron-scheduler 内部按 UTC 解释 cron 表达式（即 `0 0 9 * * *`
/// 意为 UTC 9 点）。本函数为保持显示与实际触发一致，**也按 UTC 解析**，再把
/// 结果换算成 Local 显示给用户。
///
/// 这意味着：
/// - 用户填的 cron 表达式应该按 UTC 思考（UI 必须明示）
/// - "下次执行" 显示的是该 cron 实际触发时刻在用户当地时区的对应时刻
///
/// 解析失败时返回 None；不抛错，便于 UI 兜底显示"—"。
pub fn next_run_time(cron: &str) -> Option<String> {
    let sch = CronSchedule::from_str(cron).ok()?;
    let next_utc = sch.upcoming(Utc).next()?;
    let next_local = next_utc.with_timezone(&Local);
    Some(next_local.to_rfc3339())
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_round_trip() {
        let s = Schedule {
            id: "s1".into(),
            name: "周五汇报".into(),
            cron: "0 30 17 ? * FRI *".into(),
            enabled: true,
            workspace_ids: vec!["w1".into(), "w2".into()],
            template_id: "builtin-tech".into(),
            provider_id: None,
            days: 7,
            recipients: vec!["boss@example.com".into()],
            cc: vec![],
            subject_tpl: "周报 {date}".into(),
            last_run: Some("2026-05-15T17:30:00Z".into()),
            last_status: Some("success".into()),
            next_run: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Schedule = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn schedule_partial_json_uses_defaults() {
        let json = r#"{
            "id": "s2",
            "name": "x",
            "cron": "0 0 9 * * MON *",
            "template_id": "builtin-simple",
            "days": 7
        }"#;
        let s: Schedule = serde_json::from_str(json).unwrap();
        assert!(!s.enabled);
        assert!(s.workspace_ids.is_empty());
        assert!(s.recipients.is_empty());
        assert_eq!(s.last_status, None);
    }

    // -------- next_run_time --------

    #[test]
    fn next_run_time_valid_7segment() {
        // 每个工作日早 9:00
        let r = next_run_time("0 0 9 ? * MON,TUE,WED,THU,FRI *");
        assert!(r.is_some(), "应能解析 7 段 cron");
    }

    #[test]
    fn next_run_time_invalid_returns_none() {
        assert_eq!(next_run_time("not a cron"), None);
        assert_eq!(next_run_time(""), None);
    }

    // -------- render_subject --------

    fn fake_record() -> ReportRecord {
        ReportRecord {
            id: "r".into(),
            week: "最近 7 天".into(),
            template_id: "builtin-tech".into(),
            template_name: "技术周报".into(),
            provider_id: None,
            provider_name: None,
            tokens_used: 0,
            project_count: 0,
            generated_at: Local::now().to_rfc3339(),
        }
    }

    #[test]
    fn render_subject_empty_uses_default() {
        let s = render_subject("", &fake_record());
        assert!(s.starts_with("周报 "));
        // {date} 被替换为 YYYY-MM-DD
        assert!(s.contains(&Local::now().format("%Y-%m-%d").to_string()));
    }

    #[test]
    fn render_subject_substitutes_date_and_week() {
        let s = render_subject("{date} / {week} 工作汇报", &fake_record());
        let today = Local::now().format("%Y-%m-%d").to_string();
        let week = format!("W{:02}", Local::now().iso_week().week());
        assert!(s.contains(&today));
        assert!(s.contains(&week));
    }

    #[test]
    fn render_subject_no_placeholders() {
        let s = render_subject("固定主题不替换", &fake_record());
        assert_eq!(s, "固定主题不替换");
    }

    // -------- SchedulerState 基础（不跑真实 job） --------

    #[tokio::test]
    async fn scheduler_new_starts_clean() {
        let st = SchedulerState::new().await.unwrap();
        assert!(st.jobs.lock().await.is_empty());
    }

    #[tokio::test]
    async fn refresh_job_with_invalid_cron_errors() {
        let st = SchedulerState::new().await.unwrap();
        let sch = Schedule {
            id: "s".into(),
            name: "x".into(),
            cron: "definitely not a cron".into(),
            enabled: true,
            template_id: "builtin-tech".into(),
            days: 7,
            ..Default::default()
        };
        let err = st.refresh_job(&sch).await.unwrap_err().to_string();
        assert!(err.contains("cron"));
    }

    #[tokio::test]
    async fn refresh_job_disabled_only_removes() {
        let st = SchedulerState::new().await.unwrap();
        let mut sch = Schedule {
            id: "s".into(),
            name: "x".into(),
            cron: "0 0 9 * * * *".into(),
            enabled: true,
            template_id: "builtin-tech".into(),
            days: 7,
            ..Default::default()
        };
        st.refresh_job(&sch).await.unwrap();
        assert_eq!(st.jobs.lock().await.len(), 1);
        sch.enabled = false;
        st.refresh_job(&sch).await.unwrap();
        assert_eq!(st.jobs.lock().await.len(), 0);
    }
}
