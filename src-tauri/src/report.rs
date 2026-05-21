//! 周报模型 + prompt 构造 + 生成入口。
//!
//! - 数据模型：`Template` / `ReportRecord`
//! - prompt 模板：见 `docs/SPEC.md#输出格式`
//! - 历史报告作为风格参考注入（默认最近 2 份）
#![allow(dead_code)]

use crate::i18n;
use anyhow::{anyhow, bail, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::llm::{self, LlmProvider};
use crate::logs::{self, ParseStats, Summary};
use crate::state;
use crate::store;
use crate::workspace::Workspace;

// ============================================================
// 数据模型
// ============================================================

/// 周报模板。系统内置 3 个（`builtin: true`），用户可新建自定义模板。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Template {
    pub id: String,
    pub name: String,
    /// `tech` / `exec` / `simple` / `custom`
    pub style: String,
    pub sections: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub extra_prompt: String,
    #[serde(default)]
    pub builtin: bool,
}

/// 历史周报元数据（不含 Markdown 正文）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReportRecord {
    pub id: String,
    pub week: String,
    pub template_id: String,
    pub template_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    pub tokens_used: u32,
    pub project_count: u32,
    pub generated_at: String,
}

// ============================================================
// 生成入口
// ============================================================

/// 构造 prompt + 调 LLM + 返回 (Markdown 正文, tokens 数, 耗时 ms)。
///
/// `past_reports` 是最近 N 份历史报告 Markdown（默认 2 份，由调用方提供）。
/// 详见 `docs/ARCHITECTURE.md#37-reportrs`。
pub async fn generate(
    summary: &Summary,
    template: &Template,
    past_reports: &[String],
    provider: &LlmProvider,
) -> Result<(String, u32, u64)> {
    let prompt = build_prompt(summary, template, past_reports);
    let r = llm::complete(provider, &prompt).await?;
    Ok((r.text, r.tokens_used, r.duration_ms))
}

/// 完整生成流程：解析 provider → 收日志 → 聚合 → 注入历史 → 调 LLM → 存档。
///
/// 被 Tauri 的 `generate_report` command 与 `scheduler::execute_schedule`
/// 共同调用，避免两边重复实现。
pub async fn run_generation(
    workspace_ids: &[String],
    template_id: &str,
    days: u32,
    provider_id: Option<&str>,
) -> Result<GenerationOutput> {
    // 1. 模板
    let template = state::list_templates()?
        .into_iter()
        .find(|t| t.id == template_id)
        .ok_or_else(|| {
            anyhow!(i18n::t_var(
                "err.report.template_not_found",
                &[("id", template_id)]
            ))
        })?;

    // 2. 解析 provider（按 LLM.md §6 优先级 显式 > 模板 > 默认 > 第一个）
    let provider = resolve_provider(provider_id, template.provider_id.as_deref())?;

    // 3. 工作区
    let all_ws = state::list_workspaces()?;
    let workspaces: Vec<Workspace> = all_ws
        .into_iter()
        .filter(|w| workspace_ids.iter().any(|id| id == &w.id))
        .collect();
    if workspaces.is_empty() {
        bail!("未选中任何工作区");
    }

    // 4. 设置
    let settings = state::get_settings()?;
    let clip = settings.prompt_clip_chars as usize;

    // 5. 收集 messages（单 workspace 失败不阻塞其他）
    let mut messages = Vec::new();
    let mut parse_stats = ParseStats::default();
    for ws in &workspaces {
        match logs::collect_messages(ws, days, clip).await {
            Ok((part, s)) => {
                messages.extend(part);
                parse_stats.merge(&s);
            }
            Err(e) => tracing::warn!("workspace {} 收集日志失败: {:#}", ws.name, e),
        }
    }

    // 6. 聚合
    let summary = logs::aggregate_with_stats(messages, parse_stats);

    // 7. 历史报告作为风格参考
    let past = load_past_reports(settings.past_reports_context as usize)?;

    // 8. 生成
    let (markdown, tokens, duration_ms) = generate(&summary, &template, &past, &provider).await?;

    // 9. 存档
    let record = ReportRecord {
        id: String::new(),
        week: format!("最近 {days} 天"),
        template_id: template.id.clone(),
        template_name: template.name.clone(),
        provider_id: Some(provider.id.clone()),
        provider_name: Some(provider.name.clone()),
        tokens_used: tokens,
        project_count: summary.stats.project_count,
        generated_at: Local::now().to_rfc3339(),
    };
    let saved = state::save_report(record, &markdown)?;

    Ok(GenerationOutput {
        record: saved,
        content: markdown,
        duration_ms,
        skipped_lines: summary.stats.skipped_lines,
        skipped_files: summary.stats.skipped_files,
    })
}

/// 完整生成流程的输出。`skipped_*` 是解析阶段计数，用于让 UI 提示用户
/// "扫了 N 行但 M 行损坏没认出来"，避免静默丢数据。
#[derive(Debug, Clone, Serialize)]
pub struct GenerationOutput {
    pub record: ReportRecord,
    pub content: String,
    pub duration_ms: u64,
    pub skipped_lines: u32,
    pub skipped_files: u32,
}

/// 按 LLM.md §6 优先级解析 provider：显式 > 模板 > 默认 > 第一个 > 报错。
fn resolve_provider(explicit: Option<&str>, template_pid: Option<&str>) -> Result<LlmProvider> {
    let providers = state::list_providers()?;

    if let Some(id) = explicit {
        if !id.is_empty() {
            if let Some(p) = providers.iter().find(|p| p.id == id) {
                return Ok(p.clone());
            }
            return Err(anyhow!(i18n::t_var(
                "err.report.provider_not_found",
                &[("id", id)]
            )));
        }
    }
    if let Some(id) = template_pid {
        if !id.is_empty() {
            if let Some(p) = providers.iter().find(|p| p.id == id) {
                return Ok(p.clone());
            }
            // 模板指定的 provider 已被删除 → 回退到默认（不报错）
        }
    }
    state::get_default_provider()
}

/// 取最近 `n` 份历史报告的 Markdown 正文。失败的单条 warn! 后跳过。
fn load_past_reports(n: usize) -> Result<Vec<String>> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut records = state::list_reports()?;
    records.sort_by(|a, b| b.generated_at.cmp(&a.generated_at));
    let mut out = Vec::new();
    for r in records.into_iter().take(n) {
        match store::load_report_file(&r.id) {
            Ok(s) => out.push(s),
            Err(e) => tracing::warn!("加载历史报告 {} 失败: {:#}", r.id, e),
        }
    }
    Ok(out)
}

// ============================================================
// prompt 构造
// ============================================================

/// 把 Summary + Template + 历史报告拼成最终 prompt 字符串。
///
/// 输出结构对应 `docs/SPEC.md#输出格式`。**纯函数**，便于单测。
pub fn build_prompt(summary: &Summary, template: &Template, past_reports: &[String]) -> String {
    let mut out = String::new();
    out.push_str("你是工程师周报助手，请基于以下工作日志生成一份 Markdown 格式的周报。\n\n");

    out.push_str(&format!("风格：{}\n", style_label(&template.style)));

    let stats = &summary.stats;
    out.push_str(&format!(
        "活跃天数：{} | 项目数：{} | 主项目：{}\n",
        stats.active_days,
        stats.project_count,
        stats.main_project.as_deref().unwrap_or("无")
    ));
    if !stats.servers.is_empty() {
        out.push_str(&format!("服务器：{}\n", stats.servers.join("、")));
    }
    if !stats.tools.is_empty() {
        out.push_str(&format!("工具：{}\n", stats.tools.join("、")));
    }
    out.push('\n');

    // 用户工作指令分组（按指令数从多到少排序，便于 LLM 优先处理重点项目）
    out.push_str("以下是从日志提取的用户工作指令（按项目分组）：\n");
    out.push_str("<work_logs>\n");
    let mut projects: Vec<(&String, &Vec<String>)> = summary.by_project.iter().collect();
    projects.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
    if projects.is_empty() {
        out.push_str("（本期未提取到任何用户指令）\n");
    } else {
        for (project, prompts) in projects {
            out.push_str(&format!("【{}】({} 条指令)\n", project, prompts.len()));
            for p in prompts {
                let line = p.replace('\n', " ");
                out.push_str(&format!("  · {line}\n"));
            }
            out.push('\n');
        }
    }
    out.push_str("</work_logs>\n\n");

    // 历史报告作为风格参考
    if !past_reports.is_empty() {
        out.push_str("以下是最近的历史周报，请**仅参考其结构和语气**，不要照抄具体内容：\n");
        for (i, r) in past_reports.iter().enumerate() {
            let idx = i + 1;
            out.push_str(&format!(
                "<past_report_{idx}>\n{r}\n</past_report_{idx}>\n\n"
            ));
        }
    }

    // 章节顺序
    if !template.sections.is_empty() {
        out.push_str(&format!(
            "请按以下章节顺序输出 Markdown 周报：{}\n\n",
            template.sections.join(" / ")
        ));
    } else {
        out.push_str("请输出一份结构清晰的 Markdown 周报。\n\n");
    }

    // 通用要求
    out.push_str("要求：\n");
    out.push_str("1. **提炼总结**，不要逐条照抄原始用户指令\n");
    out.push_str("2. 相似指令应**归纳合并**为一句话\n");
    out.push_str("3. 下周计划可基于趋势合理推断，但所有非事实陈述都要标注「（推断）」\n");
    out.push_str("4. 每个章节用 Markdown 二级标题（`##`）开头\n");
    out.push_str("5. 不要包含本指令中提到的元信息（如 `活跃天数`、`<work_logs>` 标签）\n");

    // 模板的额外要求
    let extra = template.extra_prompt.trim();
    if !extra.is_empty() {
        out.push_str("\n额外要求：\n");
        out.push_str(extra);
        out.push('\n');
    }

    out
}

fn style_label(style: &str) -> &'static str {
    match style {
        "tech" => "技术向 —— 重视代码实现、bug 修复、技术选型",
        "exec" => "管理层汇报向 —— 重视业务影响、关键产出、风险与阻塞",
        "simple" => "简洁日报向 —— 要点列出即可，不展开细节",
        _ => "自定义",
    }
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::SummaryStats;
    use std::collections::HashMap;

    fn sample_summary() -> Summary {
        let mut by_project = HashMap::new();
        by_project.insert(
            "weekly-report".to_string(),
            vec![
                "实现 LLM provider 抽象".to_string(),
                "把 SQLite 换成 JSON 文件".to_string(),
            ],
        );
        by_project.insert(
            "chat-bot".to_string(),
            vec!["调试 stream API 的中断问题".to_string()],
        );
        Summary {
            by_project,
            ai_snippets: vec![],
            stats: SummaryStats {
                total_prompts: 3,
                active_days: 5,
                project_count: 2,
                main_project: Some("weekly-report".to_string()),
                servers: vec!["本机".to_string()],
                tools: vec!["claude-code".to_string()],
                skipped_lines: 0,
                skipped_files: 0,
            },
        }
    }

    fn tech_template() -> Template {
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
        }
    }

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

    #[test]
    fn prompt_contains_stats_and_work_logs() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(p.contains("活跃天数：5"));
        assert!(p.contains("项目数：2"));
        assert!(p.contains("主项目：weekly-report"));
        assert!(p.contains("服务器：本机"));
        assert!(p.contains("<work_logs>"));
        assert!(p.contains("</work_logs>"));
        assert!(p.contains("【weekly-report】(2 条指令)"));
        assert!(p.contains("· 实现 LLM provider 抽象"));
    }

    #[test]
    fn prompt_sorts_projects_by_prompt_count_desc() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        let weekly_pos = p.find("【weekly-report】").unwrap();
        let chat_pos = p.find("【chat-bot】").unwrap();
        assert!(weekly_pos < chat_pos, "指令多的项目应排在前面");
    }

    #[test]
    fn prompt_includes_section_order() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(p.contains("本周 TL;DR / 各项目进展 / 技术亮点 / 下周计划"));
    }

    #[test]
    fn prompt_injects_past_reports_when_present() {
        let past = vec![
            "# 上周周报\n\n做了 A 和 B。".to_string(),
            "# 上上周周报\n\n做了 C。".to_string(),
        ];
        let p = build_prompt(&sample_summary(), &tech_template(), &past);
        assert!(p.contains("<past_report_1>"));
        assert!(p.contains("<past_report_2>"));
        assert!(p.contains("做了 A 和 B"));
        assert!(p.contains("仅参考其结构和语气"));
    }

    #[test]
    fn prompt_omits_past_reports_section_when_empty() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(!p.contains("past_report"));
    }

    #[test]
    fn prompt_handles_empty_summary_gracefully() {
        let empty = Summary::default();
        let p = build_prompt(&empty, &tech_template(), &[]);
        assert!(p.contains("活跃天数：0"));
        assert!(p.contains("项目数：0"));
        assert!(p.contains("主项目：无"));
        assert!(p.contains("（本期未提取到任何用户指令）"));
    }

    #[test]
    fn prompt_appends_template_extra_prompt() {
        let mut t = tech_template();
        t.extra_prompt = "  请只输出三个章节，每章不超过 3 句。  ".into();
        let p = build_prompt(&sample_summary(), &t, &[]);
        assert!(p.contains("额外要求："));
        assert!(p.contains("请只输出三个章节"));
    }

    #[test]
    fn prompt_strips_extra_prompt_whitespace_only() {
        let mut t = tech_template();
        t.extra_prompt = "   \n   ".into();
        let p = build_prompt(&sample_summary(), &t, &[]);
        assert!(!p.contains("额外要求"));
    }

    #[test]
    fn prompt_replaces_newlines_in_user_prompts() {
        let mut s = sample_summary();
        s.by_project
            .insert("multi".into(), vec!["第一行\n第二行".to_string()]);
        let p = build_prompt(&s, &tech_template(), &[]);
        // 行内换行被替换为空格，避免 prompt 结构被破坏
        assert!(p.contains("第一行 第二行"));
        assert!(!p.contains("· 第一行\n第二行"));
    }

    #[test]
    fn style_labels_known() {
        assert!(style_label("tech").contains("技术向"));
        assert!(style_label("exec").contains("管理层"));
        assert!(style_label("simple").contains("简洁"));
        assert_eq!(style_label("custom"), "自定义");
        assert_eq!(style_label("unknown"), "自定义");
    }
}
