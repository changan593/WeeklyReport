//! 周报相关的数据模型：`Template` 与 `ReportRecord`。
//!
//! 报告正文（Markdown）单独存为 `reports/<id>.md`，不在 `index.json` 中携带，
//! 详见 `docs/ARCHITECTURE.md#5-数据存储`。

use serde::{Deserialize, Serialize};

/// 周报模板。系统内置 3 个（`builtin: true`），用户可新建自定义模板。
///
/// 内置模板由 [`crate::state::list_templates`] 始终注入到列表里，不持久化到
/// `templates.json`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Template {
    pub id: String,
    pub name: String,
    /// `tech` / `exec` / `simple` / `custom`
    pub style: String,
    /// 章节标题，顺序即输出顺序
    pub sections: Vec<String>,
    /// 绑定的 LLM 源 ID；为 None 时使用默认源
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// 用户附加的 prompt 要求
    #[serde(default)]
    pub extra_prompt: String,
    /// 内置模板标记；true 时不可修改不可删除
    #[serde(default)]
    pub builtin: bool,
}

/// 历史周报元数据（不含 Markdown 正文）。
///
/// 正文存为 `reports/<id>.md`，元数据列表存为 `reports/index.json`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReportRecord {
    pub id: String,
    /// 时间范围标签，如 "最近 7 天" 或 "2026-05-13 ~ 2026-05-19"
    pub week: String,
    pub template_id: String,
    /// 冗余字段：模板名（便于在列表中显示，不依赖模板是否还存在）
    pub template_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    pub tokens_used: u32,
    pub project_count: u32,
    /// 生成时间 ISO 8601
    pub generated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_round_trip() {
        let t = Template {
            id: "t1".into(),
            name: "我的模板".into(),
            style: "tech".into(),
            sections: vec!["TL;DR".into(), "进展".into()],
            provider_id: Some("p1".into()),
            extra_prompt: "请用要点".into(),
            builtin: false,
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: Template = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn report_record_default_optional_provider() {
        let json = r#"{
            "id": "r1",
            "week": "最近 7 天",
            "template_id": "builtin-tech",
            "template_name": "技术周报",
            "tokens_used": 1500,
            "project_count": 3,
            "generated_at": "2026-05-20T12:00:00+08:00"
        }"#;
        let r: ReportRecord = serde_json::from_str(json).unwrap();
        assert_eq!(r.provider_id, None);
        assert_eq!(r.provider_name, None);
        assert_eq!(r.tokens_used, 1500);
    }
}
