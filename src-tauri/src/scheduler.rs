//! 定时任务 `Schedule` 数据模型。
//!
//! 实际的调度执行（`SchedulerState`、`execute_schedule`、`next_run_time`）
//! 由阶段 8 实现，详见 `docs/ARCHITECTURE.md#39-schedulerrs`。

use serde::{Deserialize, Serialize};

/// 一条定时任务配置。
///
/// `cron` 使用 7 段格式（秒 分 时 日 月 星期 年），与系统 cron (5/6 段) 不同，
/// UI 必须提示这一点。详见 `docs/DECISIONS.md#adr-009`。
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
    /// 可选地指定本任务使用的 LLM 源；不指定则按模板 → 默认源回退。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub days: u32,
    #[serde(default)]
    pub recipients: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    /// 邮件主题模板，支持 `{date}` `{week}` 变量。
    #[serde(default)]
    pub subject_tpl: String,
    /// 上次运行时间 ISO 8601。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    /// `success` 或 `failed: <原因>`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    /// 运行时计算，不持久化（save 前清空）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run: Option<String>,
}

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
}
