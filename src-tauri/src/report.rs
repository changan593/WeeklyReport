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

use crate::email;
use crate::llm::{self, LlmProvider};
use crate::logs::{self, LogItem, ParseStats, Summary};
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

/// 第一步「收集」的输出。送往前端的 review 步骤，让用户编辑 `summary`。
/// 也可序列化保存为 draft，供下次继续编辑。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionOutput {
    pub summary: Summary,
    /// 这次收集是基于哪些 workspace，让 draft 恢复时能匹配
    pub workspace_ids: Vec<String>,
    pub days: u32,
    pub skipped_lines: u32,
    pub skipped_files: u32,
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

/// 第一步：扫日志 → 聚合 → 返回带 timestamp 的 `Summary`。
/// 不调 LLM、不存档，可被前端编辑。
pub async fn collect_summary(workspace_ids: &[String], days: u32) -> Result<CollectionOutput> {
    let all_ws = state::list_workspaces()?;
    let workspaces: Vec<Workspace> = all_ws
        .into_iter()
        .filter(|w| workspace_ids.iter().any(|id| id == &w.id))
        .collect();
    if workspaces.is_empty() {
        bail!(i18n::t("err.report.no_workspaces"));
    }

    let settings = state::get_settings()?;
    let clip = settings.prompt_clip_chars as usize;

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
    let summary = logs::aggregate_with_stats(messages, parse_stats);
    Ok(CollectionOutput {
        skipped_lines: summary.stats.skipped_lines,
        skipped_files: summary.stats.skipped_files,
        summary,
        workspace_ids: workspace_ids.to_vec(),
        days,
    })
}

/// 第二步：用（可能已被用户编辑过的）`Summary` 渲染 prompt → 调 LLM → 存档。
pub async fn render_from_summary(
    summary: &Summary,
    template_id: &str,
    days: u32,
    provider_id: Option<&str>,
) -> Result<GenerationOutput> {
    let template = state::list_templates()?
        .into_iter()
        .find(|t| t.id == template_id)
        .ok_or_else(|| {
            anyhow!(i18n::t_var(
                "err.report.template_not_found",
                &[("id", template_id)]
            ))
        })?;
    let provider = resolve_provider(provider_id, template.provider_id.as_deref())?;
    let settings = state::get_settings()?;
    let past = load_past_reports(settings.past_reports_context as usize)?;

    // 重新统计：用户可能在编辑步骤里增删条目，原 stats 会失真
    let (total_prompts, project_count, main_project) = recompute_stats(summary);
    let mut summary_for_prompt = summary.clone();
    summary_for_prompt.stats.total_prompts = total_prompts;
    summary_for_prompt.stats.project_count = project_count;
    summary_for_prompt.stats.main_project = main_project;

    let (markdown, tokens, duration_ms) =
        generate(&summary_for_prompt, &template, &past, &provider).await?;

    let record = ReportRecord {
        id: String::new(),
        week: format!("最近 {days} 天"),
        template_id: template.id.clone(),
        template_name: template.name.clone(),
        provider_id: Some(provider.id.clone()),
        provider_name: Some(provider.name.clone()),
        tokens_used: tokens,
        project_count: summary_for_prompt.stats.project_count,
        generated_at: Local::now().to_rfc3339(),
    };
    let saved = state::save_report(record, &markdown)?;

    // 同时存 HTML（供 Reports 详情 HTML 预览 / 复制 HTML）。
    // 渲染失败不阻塞主路径 —— 旧报告 / 渲染失败时 get_report_html 会现场再渲染。
    let html = email::render_html(&markdown);
    if let Err(e) = store::save_report_html_file(&saved.id, &html) {
        tracing::warn!("保存报告 HTML 失败 {}: {:#}", saved.id, e);
    }

    Ok(GenerationOutput {
        record: saved,
        content: markdown,
        duration_ms,
        skipped_lines: summary_for_prompt.stats.skipped_lines,
        skipped_files: summary_for_prompt.stats.skipped_files,
    })
}

/// 取报告 HTML：优先用磁盘上的 `<id>.html`，没有就从 `.md` 现场渲染。
///
/// 用于 Reports 详情的 HTML 预览 + 复制 HTML 功能。旧报告（本 PR 之前生成）
/// 没有 .html 文件，按需 fallback 现场渲染。
pub fn get_report_html(id: &str) -> Result<String> {
    if let Some(cached) = store::load_report_html_file(id)? {
        return Ok(cached);
    }
    let md = store::load_report_file(id)?;
    Ok(email::render_html(&md))
}

/// 编辑后用户可能删/增条目，按当前 by_project 重算 prompt 数 / 项目数 / 主项目。
fn recompute_stats(summary: &Summary) -> (u32, u32, Option<String>) {
    let total: u32 = summary.by_project.values().map(|v| v.len() as u32).sum();
    let count = summary.by_project.len() as u32;
    let main = summary
        .by_project
        .iter()
        .max_by_key(|(_, v)| v.len())
        .map(|(k, _)| k.clone());
    (total, count, main)
}

/// 一站式：收集 → 直接渲染（向后兼容旧 `generate_report` 命令 + scheduler 调用）。
pub async fn run_generation(
    workspace_ids: &[String],
    template_id: &str,
    days: u32,
    provider_id: Option<&str>,
) -> Result<GenerationOutput> {
    let collected = collect_summary(workspace_ids, days).await?;
    render_from_summary(&collected.summary, template_id, days, provider_id).await
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
///
/// 设计目标（PR #6）：
/// - 每条工作记录带 `[YYYY-MM-DD]` 日期前缀，让 LLM 能按时间组织叙述
/// - 风格指引细化为具体可执行的规则（避免"口语化"、"做了一些"等弱表达）
/// - 7 条通用要求 + 禁止短语清单，约束输出朝企业可用方向
pub fn build_prompt(summary: &Summary, template: &Template, past_reports: &[String]) -> String {
    let mut out = String::new();
    out.push_str("你是工程师周报助手，请基于以下工作日志生成一份 Markdown 格式的周报。\n\n");

    out.push_str(&format!("# 风格\n{}\n\n", style_label(&template.style)));
    out.push_str("# 风格指引（务必遵循）\n");
    out.push_str(style_guide(&template.style));
    out.push_str("\n\n");

    let stats = &summary.stats;
    out.push_str("# 本期工作概况\n");
    out.push_str(&format!(
        "- 活跃天数：{} | 项目数：{} | 主项目：{}\n",
        stats.active_days,
        stats.project_count,
        stats.main_project.as_deref().unwrap_or("无")
    ));
    if !stats.servers.is_empty() {
        out.push_str(&format!("- 服务器：{}\n", stats.servers.join("、")));
    }
    if !stats.tools.is_empty() {
        out.push_str(&format!("- 工具：{}\n", stats.tools.join("、")));
    }
    out.push('\n');

    // 用户工作指令分组（按指令数从多到少排序，便于 LLM 优先处理重点项目）
    // 每条带 [YYYY-MM-DD] 日期前缀，让 LLM 可按时间组织叙述
    out.push_str("# 工作日志（按项目分组，已用户编辑确认）\n");
    out.push_str("<work_logs>\n");
    let mut projects: Vec<(&String, &Vec<LogItem>)> = summary.by_project.iter().collect();
    projects.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
    if projects.is_empty() {
        out.push_str("(本期未提取到任何用户指令)\n");
    } else {
        for (project, items) in projects {
            out.push_str(&format!("【{}】({} 条)\n", project, items.len()));
            for item in items {
                let date = item
                    .timestamp
                    .as_deref()
                    .and_then(|s| s.get(0..10))
                    .unwrap_or("无日期");
                let line = item.text.replace('\n', " ");
                out.push_str(&format!("  · [{date}] {line}\n"));
            }
            out.push('\n');
        }
    }
    out.push_str("</work_logs>\n\n");

    // 历史报告作为风格参考
    if !past_reports.is_empty() {
        out.push_str("# 历史周报（仅参考结构和语气，不要照抄）\n");
        for (i, r) in past_reports.iter().enumerate() {
            let idx = i + 1;
            out.push_str(&format!(
                "<past_report_{idx}>\n{r}\n</past_report_{idx}>\n\n"
            ));
        }
    }

    // 章节顺序
    out.push_str("# 输出要求\n\n");
    if !template.sections.is_empty() {
        out.push_str(&format!(
            "## 章节顺序\n请按以下章节顺序输出：{}\n\n",
            template.sections.join(" / ")
        ));
    }

    // 7 条通用要求
    out.push_str("## 内容规则\n");
    out.push_str("1. **提炼成果**：基于用户指令推断实际完成的工作，不要照抄原始指令文本\n");
    out.push_str("2. **去口语化**：把「做一下」「看看」「弄了下」等口语词替换为具体动词，如「实现 / 重构 / 修复 / 接入 / 调研 / 优化」\n");
    out.push_str("3. **量化输出**：能数清的优先量化（如「完成 N 个 PR」「修复 M 个 bug」「减少 X% 体积」）\n");
    out.push_str("4. **聚焦影响**：每项工作说清「做了什么」+「解决了什么问题/带来什么价值」\n");
    out.push_str("5. **合并相似**：同主题的多条指令合并为一句话；保留时间线信息（如「5/17 起完成 X，5/19 接入 Y」）\n");
    out.push_str("6. **下周计划**基于趋势合理推断；所有非事实陈述必须标注「（推断）」\n");
    out.push_str("7. **格式**：章节用 Markdown 二级标题（`##`）；列表用 `-`，不嵌套；不要输出本指令中的元信息（活跃天数 / `<work_logs>` 标签等）\n\n");

    // 禁止短语
    out.push_str("## 禁止用语\n");
    out.push_str("以下模糊表达**不允许出现**，每条都要具体到模块/文件/功能：\n");
    out.push_str("- 「做了一些工作」「写了一些代码」「优化了体验」\n");
    out.push_str("- 「修复了若干 bug」「处理了一些问题」\n");
    out.push_str("- 「持续推进」「稳步进行」「逐步完善」\n\n");

    // 模板的额外要求
    let extra = template.extra_prompt.trim();
    if !extra.is_empty() {
        out.push_str("## 模板额外要求\n");
        out.push_str(extra);
        out.push('\n');
    }

    out
}

fn style_label(style: &str) -> &'static str {
    match style {
        "tech" => "技术向 —— 面向工程师同事；重视代码实现、bug 修复、技术选型",
        "exec" => "管理层汇报向 —— 面向不写代码的领导；重视业务影响、关键产出、风险与阻塞",
        "simple" => "简洁日报向 —— 要点列出即可，不展开细节，全文 < 300 字",
        _ => "自定义",
    }
}

/// 按 style 返回更具体的风格指引（注入 prompt 让 LLM 真正风格分明）。
fn style_guide(style: &str) -> &'static str {
    match style {
        "tech" => {
            "\
- 用具体技术术语：「实现 / 重构 / 修复 / 接入 / 调试 / 性能优化」
- 描述工作时说清「在哪个模块（文件路径 / 函数名 / 功能名）做了什么改动」
- 优先量化：完成 N 个 PR / 修 M 个 bug / 体积 -X% / 接入 Y 个 API
- 技术亮点部分列出本周的关键设计选择、性能提升或安全改进
- 可以出现少量代码引用（`backtick`）和文件路径"
        }
        "exec" => {
            "\
- 用非技术听众也能懂的语言；不放代码、不堆术语缩写
- 每项必带「业务影响」：上线了 X 功能 → 用户可以 Y / 节省了 Z 小时
- 风险阻塞部分清楚说明阻塞点 + 等待动作 + 影响范围
- 下周重点用 2-4 条要点列出，每条 < 25 字
- 不写背景、不写过程；只写「做成了什么 → 产生了什么价值」"
        }
        "simple" => {
            "\
- 每个 section 用 3-5 行要点，每行 < 30 字
- 用动词开头：「实现 / 重构 / 修复 / 优化 / 调研」
- 不写背景、不写细节、不写过程
- 全文 < 300 字"
        }
        _ => "- 按模板章节直接输出；保持简洁、具体、可量化",
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

    fn fake_item(text: &str) -> LogItem {
        LogItem {
            id: "test-id".into(),
            timestamp: Some("2026-05-17T10:00:00+08:00".into()),
            source: "claude-code".into(),
            text: text.into(),
            manual: false,
        }
    }

    fn sample_summary() -> Summary {
        let mut by_project = HashMap::new();
        by_project.insert(
            "weekly-report".to_string(),
            vec![
                fake_item("实现 LLM provider 抽象"),
                fake_item("把 SQLite 换成 JSON 文件"),
            ],
        );
        by_project.insert(
            "chat-bot".to_string(),
            vec![fake_item("调试 stream API 的中断问题")],
        );
        Summary {
            by_project,
            project_paths: HashMap::new(),
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
        assert!(p.contains("【weekly-report】(2 条)"));
        // 工作记录带 [YYYY-MM-DD] 日期前缀
        assert!(p.contains("· [2026-05-17] 实现 LLM provider 抽象"));
    }

    #[test]
    fn prompt_includes_style_guide_for_tech() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        // 风格指引包含技术向特有的关键词
        assert!(p.contains("风格指引"));
        assert!(p.contains("具体技术术语"));
        assert!(p.contains("文件路径"));
    }

    #[test]
    fn prompt_includes_anti_patterns() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        // 禁止短语清单
        assert!(p.contains("禁止用语"));
        assert!(p.contains("做了一些工作"));
        assert!(p.contains("修复了若干 bug"));
    }

    #[test]
    fn prompt_handles_item_without_timestamp() {
        let mut s = sample_summary();
        let item_no_ts = LogItem {
            id: "x".into(),
            timestamp: None,
            source: "manual".into(),
            text: "用户手动新增的内容".into(),
            manual: true,
        };
        s.by_project.insert("misc".into(), vec![item_no_ts]);
        let p = build_prompt(&s, &tech_template(), &[]);
        assert!(p.contains("· [无日期] 用户手动新增的内容"));
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
        assert!(p.contains("仅参考结构和语气"));
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
        assert!(p.contains("(本期未提取到任何用户指令)"));
    }

    #[test]
    fn prompt_appends_template_extra_prompt() {
        let mut t = tech_template();
        t.extra_prompt = "  请只输出三个章节，每章不超过 3 句。  ".into();
        let p = build_prompt(&sample_summary(), &t, &[]);
        assert!(p.contains("模板额外要求"));
        assert!(p.contains("请只输出三个章节"));
    }

    #[test]
    fn prompt_strips_extra_prompt_whitespace_only() {
        let mut t = tech_template();
        t.extra_prompt = "   \n   ".into();
        let p = build_prompt(&sample_summary(), &t, &[]);
        assert!(!p.contains("模板额外要求"));
    }

    #[test]
    fn prompt_replaces_newlines_in_user_prompts() {
        let mut s = sample_summary();
        s.by_project
            .insert("multi".into(), vec![fake_item("第一行\n第二行")]);
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
