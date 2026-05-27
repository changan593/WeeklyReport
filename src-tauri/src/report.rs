//! 周报模型 + prompt 构造 + 生成入口。
//!
//! - 数据模型：`Template` / `ReportRecord`
//! - prompt 设计：见 [`build_prompt`] 的 doc comment（v0.1.2 重构）
//! - 历史报告默认**不注入**；由 `Settings.inject_past_reports` 控制，启用时
//!   默认仅取同模板生成的，避免污染 LLM 的格式判断
#![allow(dead_code)]

use crate::i18n;
use anyhow::{anyhow, bail, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use crate::email;
use crate::llm::{self, LlmProvider};
use crate::logs::{self, LogItem, ParseStats, Summary};
use crate::projectdocs;
use crate::state;
use crate::store;
use crate::workspace::{Workspace, WorkspaceKind};
use chrono::Duration;

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
///
/// `meta` 字段不下发到前端（前端用不到），仅供后端 scheduler→email 路径
/// 串联，让定时邮件复用美化版 HTML（带统计卡 + 按项目条形图）。
#[derive(Debug, Clone, Serialize)]
pub struct GenerationOutput {
    pub record: ReportRecord,
    pub content: String,
    pub duration_ms: u64,
    pub skipped_lines: u32,
    pub skipped_files: u32,
    #[serde(skip)]
    pub meta: email::ReportMeta,
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

    // `since` 用于判定文档强/弱信号：本期内修改过的非 README 文档算强信号，
    // 本期未更新的算弱信号（仅供领域词汇背景）。详见 projectdocs 模块文档。
    let since = chrono::Local::now() - Duration::days(days as i64);

    let mut messages = Vec::new();
    let mut parse_stats = ParseStats::default();
    // 项目名 → 根目录 + doc(s) 子目录 md 文档合并文本（带强/弱分区标记）。
    let mut project_docs: HashMap<String, String> = HashMap::new();
    for ws in &workspaces {
        match logs::collect_messages(ws, days, clip).await {
            Ok((part, s)) => {
                // 读各项目的 md 文档作背景：
                // 本机直接读文件系统；SSH 用 ssh+tar 把项目根 + doc(s) 子目录的 *.md 拉到缓存再读。
                let ws_paths = logs::project_paths_of(&part);
                match ws.kind {
                    WorkspaceKind::Local => {
                        for (project, path) in ws_paths {
                            if project_docs.contains_key(&project) {
                                continue;
                            }
                            if let Some(doc) = projectdocs::read_local_project_docs(&path, since) {
                                project_docs.insert(project, doc);
                            }
                        }
                    }
                    WorkspaceKind::Ssh => {
                        match crate::ssh::sync_project_docs(ws, &ws_paths, since).await {
                            Ok(docs) => {
                                for (project, doc) in docs {
                                    project_docs.entry(project).or_insert(doc);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("workspace {} 读项目 md 失败: {:#}", ws.name, e)
                            }
                        }
                    }
                }
                messages.extend(part);
                parse_stats.merge(&s);
            }
            Err(e) => tracing::warn!("workspace {} 收集日志失败: {:#}", ws.name, e),
        }
    }
    let mut summary = logs::aggregate_with_stats(messages, parse_stats);
    summary.project_docs = project_docs;
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
    // 默认不注入历史报告（避免污染 LLM 格式判断）；只有 settings 显式开启才取。
    let past = if settings.inject_past_reports {
        let same_tpl = if settings.past_reports_same_template_only {
            Some(template.id.as_str())
        } else {
            None
        };
        load_past_reports(settings.past_reports_context as usize, same_tpl)?
    } else {
        Vec::new()
    };

    // 重新统计：用户可能在编辑步骤里增删条目，原 stats 会失真
    let (total_prompts, project_count, main_project) = recompute_stats(summary);
    let mut summary_for_prompt = summary.clone();
    summary_for_prompt.stats.total_prompts = total_prompts;
    summary_for_prompt.stats.project_count = project_count;
    summary_for_prompt.stats.main_project = main_project;

    // 按 generation_mode 分派：单轮 vs 两轮（提炼-渲染）
    let (markdown, tokens, duration_ms) = match settings.generation_mode {
        state::GenerationMode::OneRound => {
            generate(&summary_for_prompt, &template, &past, &provider).await?
        }
        state::GenerationMode::TwoRoundSilent => {
            generate_two_round(&summary_for_prompt, &template, &past, &provider).await?
        }
    };

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
    let meta = build_report_meta(&saved, &summary_for_prompt);
    let html = email::render_html_with_meta(&markdown, &meta);
    if let Err(e) = store::save_report_html_file(&saved.id, &html) {
        tracing::warn!("保存报告 HTML 失败 {}: {:#}", saved.id, e);
    }

    Ok(GenerationOutput {
        record: saved,
        content: markdown,
        duration_ms,
        skipped_lines: summary_for_prompt.stats.skipped_lines,
        skipped_files: summary_for_prompt.stats.skipped_files,
        meta,
    })
}

/// 取报告 HTML：优先用磁盘上的 `<id>.html`，没有就从 `.md` 现场渲染。
///
/// 用于 Reports 详情的 HTML 预览 + 复制 HTML 功能。旧报告（本 PR 之前生成）
/// 没有 .html 文件，按需 fallback 现场渲染：尝试用 `ReportRecord` 元数据
/// 渲染美化版（无按项目条形图，因为旧报告未存 by_project 快照）；
/// 元数据查不到时退回最简版。
pub fn get_report_html(id: &str) -> Result<String> {
    if let Some(cached) = store::load_report_html_file(id)? {
        return Ok(cached);
    }
    let md = store::load_report_file(id)?;
    let record = state::list_reports()?.into_iter().find(|r| r.id == id);
    match record {
        Some(r) => {
            let meta = email::ReportMeta {
                week: r.week,
                project_count: r.project_count,
                tokens_used: r.tokens_used,
                provider_name: r.provider_name,
                generated_at: r.generated_at,
                project_breakdown: Vec::new(),
            };
            Ok(email::render_html_with_meta(&md, &meta))
        }
        None => Ok(email::render_html(&md)),
    }
}

/// 从 `ReportRecord` + `Summary` 构造邮件用的 `ReportMeta`（含按项目条形图数据）。
///
/// 公开仅为 scheduler/email 路径复用，外部不应依赖。
pub fn build_report_meta(record: &ReportRecord, summary: &Summary) -> email::ReportMeta {
    let mut breakdown: Vec<(String, u32)> = summary
        .by_project
        .iter()
        .map(|(k, v)| (k.clone(), v.len() as u32))
        .collect();
    // 排序由 render_meta_header 内部再做一遍（容错），这里先排稳定一下。
    breakdown.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    email::ReportMeta {
        week: record.week.clone(),
        project_count: record.project_count,
        tokens_used: record.tokens_used,
        provider_name: record.provider_name.clone(),
        generated_at: record.generated_at.clone(),
        project_breakdown: breakdown,
    }
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
///
/// `same_template_id`：若为 `Some(id)`，只取 `template_id == id` 的历史报告
/// （避免把另一个模板的格式带过来污染当前模板的输出）。
fn load_past_reports(n: usize, same_template_id: Option<&str>) -> Result<Vec<String>> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut records = state::list_reports()?;
    records.sort_by(|a, b| b.generated_at.cmp(&a.generated_at));
    if let Some(tid) = same_template_id {
        records.retain(|r| r.template_id == tid);
    }
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

/// 把 Summary + Template + 历史报告拼成最终 prompt 字符串。**纯函数**，便于单测。
///
/// # 设计原则（v0.1.2 重构，详见 commit message）
///
/// 旧版本里 `Template.extra_prompt` 被放到 prompt 最末段「模板额外要求」小节，
/// 优先级远低于前面塞的 7 条通用规则、风格指引、历史报告。结果：用户配置自定义
/// 模板（如 `style=custom` + extra_prompt 提供格式示例）也没用，LLM 仍照搬历史
/// 报告或者技术周报的章节结构。
///
/// 重写后：
///
/// 1. **`extra_prompt` 上提到顶部 `## 输出格式` 区域**，加强约束 ——
///    它就是格式权威，不与其他指令冲突。
/// 2. **删除 7 条通用规则 + 禁止用语清单**。保留 3 条不可妥协的硬约束
///    （禁前言/禁编造数字/禁「推断」标签）。
/// 3. **`style=custom` 时不注入任何风格指引**，让 extra_prompt 唯一说了算；
///    其它 style 时也仅在 extra_prompt 为空才回退到内置风格指引。
/// 4. **历史报告默认不注入**（由 Settings 控制）。即便启用，也只在 prompt
///    末段单独包成 `<format_reference_only>`，附「仅参考语气/节奏，不要照抄
///    章节结构」的强约束。
/// 5. **章节列表 `sections` 加严格约束**：「仅输出以下章节，不允许新增/合并/重命名」。
/// 6. **工作日志保留** `[YYYY-MM-DD]` 前缀和 AI 回复（这部分已被验证有用）。
/// 7. **项目背景文档**直接复用 `summary.project_docs` —— 该字段已由
///    [`crate::projectdocs::compose_for_prompt`] 合并好强/弱信号分区。
pub fn build_prompt(summary: &Summary, template: &Template, past_reports: &[String]) -> String {
    let mut out = String::new();

    // ─── 头部：角色 + 3 条硬约束 ───
    out.push_str(
        "你是工程师周报助手。基于下方的工作日志，按「输出格式」指定的形式，\
                  直接输出 Markdown 周报正文。\n\n",
    );
    out.push_str("# 必须遵守的 3 条硬约束\n");
    out.push_str(
        "1. **直接输出周报正文**。第一字符必须是 `#` 或编号（如 `1、`），\
         **绝对禁止任何前言/开场白/客套话**（如「好的」「这是...」「以下是...」「请查收」）。\n",
    );
    out.push_str(
        "2. **不要编造任何数字**。只有当原始工作日志中出现明确数字（PR 数、文件数、\
         耗时、百分比等）时才量化；否则用动词性描述（「完成」「重构」「修复」）即可，\
         不要写「完成 N 个 PR」「修复若干 bug」之类。\n",
    );
    out.push_str(
        "3. **不要输出本指令的任何元信息**（如「活跃天数」「项目数」「<work_logs>」标签、\
         统计字符数等）；也不要写「（推断）」一类的标签，下周计划没有明确依据时直接省略\
         该项即可。\n\n",
    );

    // ─── 输出格式（核心）：extra_prompt > 章节列表 > 内置风格指引 ───
    out.push_str("# 输出格式\n\n");
    let extra = template.extra_prompt.trim();
    let custom_only = template.style == "custom";

    if !extra.is_empty() {
        out.push_str(
            "**以下是用户指定的输出格式与示例，必须严格按此输出**\
             （照搬其句式、长度、编号风格、标点习惯；不要套用任何其它结构）：\n\n",
        );
        out.push_str("<format_spec must_follow=\"true\">\n");
        out.push_str(extra);
        out.push_str("\n</format_spec>\n\n");
    }

    if !template.sections.is_empty() {
        out.push_str(&format!(
            "**章节约束**：仅输出以下 {} 个章节，不允许新增/合并/重命名/调整顺序：\n",
            template.sections.len()
        ));
        for (i, sec) in template.sections.iter().enumerate() {
            out.push_str(&format!("  {}. {sec}\n", i + 1));
        }
        out.push('\n');
    }

    // 仅当用户没给 extra_prompt 且非 custom 时，才注入内置风格指引
    if extra.is_empty() && !custom_only {
        out.push_str(&format!("**风格**：{}\n\n", style_label(&template.style)));
        out.push_str("**风格细则**：\n");
        out.push_str(style_guide(&template.style));
        out.push_str("\n\n");
    }

    // ─── 本期数据 ───
    let stats = &summary.stats;
    out.push_str("# 本期工作概况（仅供你了解上下文，不要在周报中复述）\n");
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

    // 项目背景文档（如有）：text 已含「## 强信号 / ## 弱信号」分区
    if !summary.project_docs.is_empty() {
        out.push_str("# 项目背景文档\n");
        out.push_str(
            "下面是按项目分组的说明文档。**强信号区**（本期相关 / 核心文档）可作为\
             具体技术名词、模块名、设计目标的来源；**弱信号区**仅供识别领域词汇，\
             不要据此推断本期成果。\n\n",
        );
        let mut docs: Vec<(&String, &String)> = summary.project_docs.iter().collect();
        docs.sort_by(|a, b| a.0.cmp(b.0));
        for (project, doc) in docs {
            out.push_str(&format!("## 项目【{project}】的文档\n\n{doc}\n\n"));
        }
    }

    // ─── 工作日志（必读核心）───
    out.push_str("# 工作日志（按项目分组，已用户编辑确认；这是周报的核心信息源）\n");
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
                if let Some(reply) = &item.reply {
                    let r = reply.replace('\n', " ");
                    out.push_str(&format!("    ↳ AI 回复：{r}\n"));
                }
            }
            out.push('\n');
        }
    }
    out.push_str("</work_logs>\n\n");

    // ─── 历史报告（仅在 Settings 开启时由调用方传入；放最后并强约束）───
    if !past_reports.is_empty() {
        out.push_str("# 历史周报（仅参考语气和叙述节奏，**不要照抄章节结构或格式**）\n");
        out.push_str(
            "下面是同模板的历史周报。**只参考它的叙述节奏和用词风格**；章节结构与格式\
             以上面「# 输出格式」为准，**不要因为历史报告章节多/章节少而改变本次输出**。\n\n",
        );
        for (i, r) in past_reports.iter().enumerate() {
            let idx = i + 1;
            out.push_str(&format!(
                "<format_reference_only no_copy_structure=\"true\" idx=\"{idx}\">\n\
                 {r}\n\
                 </format_reference_only>\n\n"
            ));
        }
    }

    out
}

// ============================================================
// 两轮 LLM 生成（GenerationMode::TwoRoundSilent）
// ============================================================
//
// 两轮策略：
//   1. 第一轮 `extract_achievements`：让 LLM 从原始 work_logs 中提取「实际完成的事项」
//      JSON 数组（去掉过程性问询/调试/试错，合并相关指令）。
//   2. 第二轮 `render_from_achievements`：用提取出的事项 + 用户模板格式，
//      让 LLM 渲染成符合用户 extra_prompt 的 Markdown。
//
// 优势：第二轮看到的输入已经是"事实"，不再有「为啥」「再试一次」干扰。
// 代价：token 用量翻倍、延迟更长。第一轮 JSON 解析失败时自动 fallback 单轮。

/// 第一轮提炼出的单条「完成事项」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Achievement {
    /// 项目名（取自 work_logs 的【】内）
    pub project: String,
    /// 8-25 字标题，动词开头
    pub title: String,
    /// 50-150 字细节描述（做了什么 / 关键技术点 / 模块名）
    pub detail: String,
    /// ≤ 80 字事实依据，引自原始指令或 AI 回复
    pub evidence: String,
    /// `YYYY-MM-DD` 或 `YYYY-MM-DD~YYYY-MM-DD`
    #[serde(default)]
    pub date_range: String,
}

/// 两轮 LLM 生成入口。返回 `(markdown, total_tokens, total_duration_ms)`。
///
/// 第一轮 JSON 解析失败时自动 fallback 到单轮（详见 [`extract_achievements`]）。
async fn generate_two_round(
    summary: &Summary,
    template: &Template,
    past_reports: &[String],
    provider: &LlmProvider,
) -> Result<(String, u32, u64)> {
    // 第一轮：提炼事项
    let extract_result = extract_achievements(summary, provider).await;
    let (achievements, t1_tokens, t1_ms) = match extract_result {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("两轮模式第一轮提炼失败，回退到单轮：{:#}", e);
            return generate(summary, template, past_reports, provider).await;
        }
    };
    if achievements.is_empty() {
        tracing::warn!("两轮模式第一轮提炼出 0 条事项，回退到单轮");
        return generate(summary, template, past_reports, provider).await;
    }

    // 第二轮：用事项渲染 Markdown
    let prompt = build_prompt_from_achievements(&achievements, summary, template, past_reports);
    let r = llm::complete(provider, &prompt).await?;
    Ok((
        r.text,
        t1_tokens.saturating_add(r.tokens_used),
        t1_ms.saturating_add(r.duration_ms),
    ))
}

/// 第一轮：让 LLM 从 work_logs 中提炼出 [`Achievement`] 列表。
///
/// 返回 `(achievements, tokens, duration_ms)`。LLM 输出无法解析为 JSON 数组时返回 Err，
/// 调用方可决定 fallback 单轮。
async fn extract_achievements(
    summary: &Summary,
    provider: &LlmProvider,
) -> Result<(Vec<Achievement>, u32, u64)> {
    let prompt = build_extract_prompt(summary);
    let r = llm::complete(provider, &prompt).await?;
    let achievements = parse_achievements_json(&r.text)?;
    Ok((achievements, r.tokens_used, r.duration_ms))
}

/// 第一轮的 prompt：让 LLM 输出 JSON 数组。**纯函数**，便于单测。
pub fn build_extract_prompt(summary: &Summary) -> String {
    let mut out = String::new();
    out.push_str(
        "你是工程师工作日志分析助手。从下方「工作日志」中提取**实际完成的工作事项**，\
         以 JSON 数组形式输出。\n\n",
    );

    out.push_str("# 规则\n");
    out.push_str(
        "1. 一个事项 = 一段连贯的工作（可能由多条用户指令组成）；请合并相关指令成一条事项。\n",
    );
    out.push_str("2. **排除**过程性指令：纯试错（「再试一次」）、纯调整（「调一下颜色」）、纯问询（「为啥」）。\n");
    out.push_str("3. 每条事项必须有 `evidence` —— 一段引自原始指令或 AI 回复的事实依据。\n");
    out.push_str("4. **不要编造任何事项**。如果原始日志不足以证明某事项完成，宁可不写。\n");
    out.push_str("5. 每条事项 5 个字段：\n");
    out.push_str("   - `project`：项目名（取自 work_logs 的【】内）\n");
    out.push_str("   - `title`：8-25 字标题，动词开头（如「实现 X 模块的 Y 功能」）\n");
    out.push_str("   - `detail`：50-150 字细节（做了什么 / 关键模块名 / 技术点）\n");
    out.push_str("   - `evidence`：≤ 80 字引文/事实依据\n");
    out.push_str("   - `date_range`：`YYYY-MM-DD` 或 `YYYY-MM-DD~YYYY-MM-DD`\n");
    out.push_str("6. 全部按时间顺序排（最早在前）。\n\n");

    out.push_str("# 输出\n");
    out.push_str(
        "**直接输出 JSON 数组**：以 `[` 开始、以 `]` 结束。\
         **不要任何 Markdown 代码块包裹**（不要 ` ```json `）、**不要任何解释文字**。\n\n",
    );

    // 项目背景（如有）：帮助 LLM 理解领域术语
    if !summary.project_docs.is_empty() {
        out.push_str("# 项目背景\n\n");
        let mut docs: Vec<(&String, &String)> = summary.project_docs.iter().collect();
        docs.sort_by(|a, b| a.0.cmp(b.0));
        for (project, doc) in docs {
            out.push_str(&format!("## 【{project}】\n\n{doc}\n\n"));
        }
    }

    // work_logs 段（与 build_prompt 一致）
    out.push_str("# 工作日志\n<work_logs>\n");
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
                if let Some(reply) = &item.reply {
                    let r = reply.replace('\n', " ");
                    out.push_str(&format!("    ↳ AI 回复：{r}\n"));
                }
            }
            out.push('\n');
        }
    }
    out.push_str("</work_logs>\n");

    out
}

/// 解析第一轮 LLM 输出为 [`Achievement`] 列表。
///
/// 容错策略：
/// 1. 优先直接 `serde_json::from_str` 整段
/// 2. 若失败，尝试剥离 markdown 代码块包裹（` ```json` / ` ``` `）
/// 3. 若失败，尝试从首个 `[` 到末个 `]` 截取后再解析
/// 4. 都失败 → 返回 Err（调用方 fallback 单轮）
///
/// **纯函数**，便于单测。
pub fn parse_achievements_json(s: &str) -> Result<Vec<Achievement>> {
    let trimmed = s.trim();
    // 1. 直接解析
    if let Ok(v) = serde_json::from_str::<Vec<Achievement>>(trimmed) {
        return Ok(v);
    }
    // 2. 去 markdown 代码块包裹
    let stripped = strip_markdown_codeblock(trimmed);
    if let Ok(v) = serde_json::from_str::<Vec<Achievement>>(&stripped) {
        return Ok(v);
    }
    // 3. 取首 [ ... 末 ] 截段
    if let (Some(lo), Some(hi)) = (stripped.find('['), stripped.rfind(']')) {
        if hi > lo {
            let slice = &stripped[lo..=hi];
            if let Ok(v) = serde_json::from_str::<Vec<Achievement>>(slice) {
                return Ok(v);
            }
        }
    }
    Err(anyhow!(
        "LLM 第一轮提炼输出无法解析为 JSON 数组：{}",
        &trimmed.chars().take(200).collect::<String>()
    ))
}

/// 去掉 ` ```json ... ``` ` 或 ` ``` ... ``` ` 包裹。其它情况原样返回。
fn strip_markdown_codeblock(s: &str) -> String {
    let t = s.trim();
    if !t.starts_with("```") {
        return t.to_string();
    }
    // 跳过开头 ``` 和可选的 language tag 行
    let after_open = t.trim_start_matches('`').trim_start();
    let after_open = after_open
        .split_once('\n')
        .map(|x| x.1)
        .unwrap_or(after_open);
    // 去掉末尾 ```
    let body = after_open.trim_end();
    let body = body.trim_end_matches('`').trim_end();
    body.to_string()
}

/// 第二轮的 prompt：把 [`Achievement`] 列表 + 模板格式拼成最终 prompt。
///
/// 与 [`build_prompt`] 共享头部硬约束 + 输出格式段；区别是输入从原始 work_logs
/// 换成已提炼的 achievements JSON。**纯函数**，便于单测。
pub fn build_prompt_from_achievements(
    achievements: &[Achievement],
    summary: &Summary,
    template: &Template,
    past_reports: &[String],
) -> String {
    let mut out = String::new();

    // 同 build_prompt 的头部 3 条硬约束
    out.push_str(
        "你是工程师周报助手。基于下方已提炼的「完成事项」（事实依据），\
         按「输出格式」指定的形式，直接输出 Markdown 周报正文。\n\n",
    );
    out.push_str("# 必须遵守的 3 条硬约束\n");
    out.push_str(
        "1. **直接输出周报正文**。第一字符必须是 `#` 或编号（如 `1、`），\
         **绝对禁止任何前言/开场白/客套话**（如「好的」「这是...」「以下是...」「请查收」）。\n",
    );
    out.push_str(
        "2. **不要编造任何数字**。只有当 achievement 的 detail/evidence 中出现明确数字时\
         才量化；否则用动词性描述即可，不要写「完成 N 个 PR」「修复若干 bug」之类。\n",
    );
    out.push_str(
        "3. **不要输出本指令的任何元信息**（如 evidence 引文标签、本说明文字）；\
         也不要写「（推断）」一类的标签，下周计划没有明确依据时直接省略该项即可。\n\n",
    );

    // 输出格式：与 build_prompt 一致
    out.push_str("# 输出格式\n\n");
    let extra = template.extra_prompt.trim();
    let custom_only = template.style == "custom";

    if !extra.is_empty() {
        out.push_str(
            "**以下是用户指定的输出格式与示例，必须严格按此输出**\
             （照搬其句式、长度、编号风格、标点习惯；不要套用任何其它结构）：\n\n",
        );
        out.push_str("<format_spec must_follow=\"true\">\n");
        out.push_str(extra);
        out.push_str("\n</format_spec>\n\n");
    }

    if !template.sections.is_empty() {
        out.push_str(&format!(
            "**章节约束**：仅输出以下 {} 个章节，不允许新增/合并/重命名/调整顺序：\n",
            template.sections.len()
        ));
        for (i, sec) in template.sections.iter().enumerate() {
            out.push_str(&format!("  {}. {sec}\n", i + 1));
        }
        out.push('\n');
    }

    if extra.is_empty() && !custom_only {
        out.push_str(&format!("**风格**：{}\n\n", style_label(&template.style)));
        out.push_str("**风格细则**：\n");
        out.push_str(style_guide(&template.style));
        out.push_str("\n\n");
    }

    // 本期数据概况（同 build_prompt）
    let stats = &summary.stats;
    out.push_str("# 本期工作概况（仅供你了解上下文，不要在周报中复述）\n");
    out.push_str(&format!(
        "- 活跃天数：{} | 项目数：{} | 主项目：{}\n",
        stats.active_days,
        stats.project_count,
        stats.main_project.as_deref().unwrap_or("无")
    ));
    if !stats.servers.is_empty() {
        out.push_str(&format!("- 服务器：{}\n", stats.servers.join("、")));
    }
    out.push('\n');

    // 已提炼的事项（替代原始 work_logs）—— 第二轮的核心信息源
    out.push_str("# 本期完成事项（已从原始日志中提炼，是周报的核心信息源；可直接采用）\n");
    out.push_str("<achievements>\n");
    // pretty JSON 让 LLM 易读
    match serde_json::to_string_pretty(achievements) {
        Ok(json) => out.push_str(&json),
        Err(_) => out.push_str("[]"),
    }
    out.push_str("\n</achievements>\n\n");

    out.push_str("# 整理要求\n");
    out.push_str("- 把上面 achievements 整理成符合「输出格式」的周报\n");
    out.push_str("- 可以按项目 / 主题归类、合并相似事项，但**不要遗漏任何 achievement**\n");
    out.push_str("- evidence 字段是给你做事实依据参考的，**不要直接把 evidence 写进周报**\n\n");

    // 项目背景（如有，复用同样的强/弱信号分区文本）
    if !summary.project_docs.is_empty() {
        out.push_str("# 项目背景文档\n");
        out.push_str("强信号区可作为具体技术名词、模块名的来源；弱信号区仅供识别领域词汇。\n\n");
        let mut docs: Vec<(&String, &String)> = summary.project_docs.iter().collect();
        docs.sort_by(|a, b| a.0.cmp(b.0));
        for (project, doc) in docs {
            out.push_str(&format!("## 项目【{project}】的文档\n\n{doc}\n\n"));
        }
    }

    // 历史报告（仅启用时）
    if !past_reports.is_empty() {
        out.push_str("# 历史周报（仅参考语气和叙述节奏，**不要照抄章节结构或格式**）\n\n");
        for (i, r) in past_reports.iter().enumerate() {
            let idx = i + 1;
            out.push_str(&format!(
                "<format_reference_only no_copy_structure=\"true\" idx=\"{idx}\">\n\
                 {r}\n\
                 </format_reference_only>\n\n"
            ));
        }
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
            server: "本机".into(),
            reply: None,
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
            project_docs: HashMap::new(),
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

    /// 构造一个 custom 模板（无 extra_prompt）—— deepexi 等用户模板典型形态
    fn custom_template_with_extra(extra: &str) -> Template {
        Template {
            id: "user-custom".into(),
            name: "deepexi".into(),
            style: "custom".into(),
            sections: vec!["本周重点".into(), "下周计划".into()],
            provider_id: None,
            extra_prompt: extra.into(),
            builtin: false,
        }
    }

    // -------- 头部硬约束 --------

    #[test]
    fn prompt_starts_with_role_and_forbids_preface() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(p.starts_with("你是工程师周报助手"), "应以角色定位开头");
        assert!(
            p.contains("绝对禁止任何前言/开场白/客套话"),
            "必须含禁前言规则"
        );
        assert!(
            p.contains("好的") && p.contains("以下是"),
            "禁前言规则应给出具体反面示例"
        );
    }

    #[test]
    fn prompt_forbids_fabricating_numbers() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(p.contains("不要编造任何数字"));
        assert!(
            !p.contains("完成 N 个 PR") || p.contains("不要写「完成 N 个 PR」"),
            "禁编数字的反例应包裹在禁令上下文里"
        );
    }

    #[test]
    fn prompt_forbids_inference_tag() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(p.contains("「（推断）」"));
        assert!(p.contains("没有明确依据时直接省略"));
    }

    // -------- 输出格式优先级（核心） --------

    #[test]
    fn extra_prompt_appears_at_top_with_format_spec_tag() {
        // 用户的 extra_prompt 必须以 <format_spec must_follow="true"> 包裹放在 prompt 顶部，
        // 优先级压倒一切（解决 deepexi 模板被技术周报格式覆盖的核心问题）
        let t = custom_template_with_extra("本周重点\n1、完成 X\n下周计划\n1、调研 Y");
        let p = build_prompt(&sample_summary(), &t, &[]);
        let spec_pos = p
            .find("<format_spec must_follow=\"true\">")
            .expect("应有 format_spec 标签");
        // 注意：硬约束区把 `<work_logs>` 作为反面示例引用了一次，所以用唯一的
        // 章节标题 `# 工作日志` 来定位真正的工作日志区
        let work_logs_pos = p.find("# 工作日志").expect("应有 # 工作日志 章节标题");
        assert!(
            spec_pos < work_logs_pos,
            "format_spec 必须排在 work_logs 之前（顶部权威）：spec_pos={spec_pos} work_logs_pos={work_logs_pos}"
        );
        assert!(p.contains("1、完成 X"), "extra_prompt 原文应保留");
        assert!(p.contains("严格按此输出"));
    }

    #[test]
    fn custom_style_does_not_inject_style_guide() {
        // style=custom 时不该出现内置的「风格指引」「风格细则」段落
        let t = custom_template_with_extra("用户的格式示例");
        let p = build_prompt(&sample_summary(), &t, &[]);
        assert!(
            !p.contains("**风格细则**"),
            "custom 模板不该注入内置风格细则"
        );
        assert!(
            !p.contains("**风格**：技术向"),
            "custom 模板不该注入风格标签"
        );
    }

    #[test]
    fn tech_style_with_extra_prompt_omits_style_guide() {
        // 即使是 tech 模板，只要用户给了 extra_prompt，就以 extra_prompt 为准
        // 不再叠加内置风格细则（避免规则打架）
        let mut t = tech_template();
        t.extra_prompt = "用户的格式说明".into();
        let p = build_prompt(&sample_summary(), &t, &[]);
        assert!(
            !p.contains("**风格细则**"),
            "extra_prompt 非空时不叠加内置风格细则"
        );
    }

    #[test]
    fn tech_style_without_extra_prompt_keeps_style_guide() {
        // tech 模板且无 extra_prompt → 内置风格指引兜底
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(p.contains("**风格细则**"));
        assert!(p.contains("具体技术术语"));
        assert!(p.contains("文件路径"));
    }

    #[test]
    fn section_constraint_is_strict() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(p.contains("不允许新增/合并/重命名/调整顺序"));
        // 章节按编号列出
        assert!(p.contains("1. 本周 TL;DR"));
        assert!(p.contains("2. 各项目进展"));
        assert!(p.contains("3. 技术亮点"));
        assert!(p.contains("4. 下周计划"));
    }

    #[test]
    fn empty_sections_yield_no_section_constraint() {
        let t = Template {
            sections: vec![],
            ..tech_template()
        };
        let p = build_prompt(&sample_summary(), &t, &[]);
        assert!(!p.contains("章节约束"));
    }

    // -------- 旧的「7 条规则 + 禁止用语」必须被移除 --------

    #[test]
    fn anti_pattern_list_removed() {
        // 旧版本会列「做了一些工作 / 修复了若干 bug / 持续推进」等禁止短语
        // 重构后这些被移除（实证表明这种 anti-list 会让 LLM 反向把这些词写进去）
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(!p.contains("## 禁止用语"), "禁止用语清单应被移除");
        assert!(!p.contains("做了一些工作"), "禁止短语 anti-prompt 应被移除");
        assert!(!p.contains("持续推进"));
    }

    #[test]
    fn legacy_seven_rules_removed() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        // 旧的「## 内容规则」标题应被移除（合并到顶部 3 条硬约束）
        assert!(!p.contains("## 内容规则"));
        // 旧的「## 模板额外要求」标题应被移除（extra_prompt 已上提到顶部）
        assert!(!p.contains("## 模板额外要求"));
    }

    // -------- 本期数据 + 工作日志 --------

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
        assert!(p.contains("· [2026-05-17] 实现 LLM provider 抽象"));
    }

    #[test]
    fn stats_section_marked_as_context_only() {
        // 统计数据明确标为「仅供上下文，不要在周报中复述」
        // 避免 LLM 把「活跃天数 5」「项目数 2」之类的元信息写进周报
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(p.contains("仅供你了解上下文，不要在周报中复述"));
    }

    #[test]
    fn prompt_includes_project_docs_when_present() {
        let mut s = sample_summary();
        s.project_docs.insert(
            "weekly-report".into(),
            "## 强信号文档\n### README.md\n这是周报生成项目".into(),
        );
        let p = build_prompt(&s, &tech_template(), &[]);
        assert!(p.contains("# 项目背景文档"));
        assert!(p.contains("## 项目【weekly-report】的文档"));
        assert!(p.contains("这是周报生成项目"));
        // 强弱信号说明
        assert!(p.contains("强信号区"));
        assert!(p.contains("弱信号区"));
    }

    #[test]
    fn prompt_omits_project_docs_section_when_empty() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(!p.contains("# 项目背景文档"));
    }

    #[test]
    fn prompt_includes_ai_reply_when_present() {
        let mut s = sample_summary();
        if let Some(items) = s.by_project.get_mut("weekly-report") {
            items[0].reply = Some("已完成 LLM 抽象层重构,新增 llm.rs".to_string());
        }
        let p = build_prompt(&s, &tech_template(), &[]);
        assert!(p.contains("↳ AI 回复：已完成 LLM 抽象层重构"));
    }

    #[test]
    fn prompt_handles_item_without_timestamp() {
        let mut s = sample_summary();
        let item_no_ts = LogItem {
            id: "x".into(),
            timestamp: None,
            source: "manual".into(),
            server: String::new(),
            text: "用户手动新增的内容".into(),
            reply: None,
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

    // -------- 历史报告（默认不注入，注入时强约束）--------

    #[test]
    fn prompt_omits_past_reports_section_when_empty() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(!p.contains("past_report"));
        assert!(!p.contains("format_reference_only"));
    }

    #[test]
    fn past_reports_wrapped_with_no_copy_structure_tag() {
        let past = vec![
            "# 上周周报\n\n做了 A 和 B。".to_string(),
            "# 上上周周报\n\n做了 C。".to_string(),
        ];
        let p = build_prompt(&sample_summary(), &tech_template(), &past);
        // 必须用 format_reference_only 包裹（而非旧的 past_report_N）
        assert!(p.contains("<format_reference_only"));
        assert!(p.contains("no_copy_structure=\"true\""));
        assert!(p.contains("做了 A 和 B"));
        // 必须明示不要照抄章节结构
        assert!(p.contains("不要照抄章节结构或格式"));
        assert!(p.contains("不要因为历史报告章节多/章节少而改变本次输出"));
    }

    #[test]
    fn past_reports_appear_at_end_after_work_logs() {
        // 即便注入历史报告，也应在 work_logs 之后（避免 LLM 受历史报告"先入为主"）
        let past = vec!["# 历史".to_string()];
        let p = build_prompt(&sample_summary(), &tech_template(), &past);
        // 用唯一的章节标题 `# 工作日志` 定位（硬约束区引用过 `<work_logs>` 占位）
        let work_pos = p.find("# 工作日志").unwrap();
        let past_pos = p.find("<format_reference_only").unwrap();
        assert!(work_pos < past_pos, "历史报告应排在 work_logs 之后");
    }

    // -------- extra_prompt 边界 --------

    #[test]
    fn empty_extra_prompt_does_not_emit_format_spec_tag() {
        let p = build_prompt(&sample_summary(), &tech_template(), &[]);
        assert!(!p.contains("<format_spec"));
    }

    #[test]
    fn whitespace_only_extra_prompt_treated_as_empty() {
        let mut t = tech_template();
        t.extra_prompt = "   \n   ".into();
        let p = build_prompt(&sample_summary(), &t, &[]);
        assert!(!p.contains("<format_spec"));
    }

    #[test]
    fn extra_prompt_overrides_style_guide_priority() {
        // 双重保险：tech 模板 + extra_prompt → format_spec 在前，无 style 细则
        let mut t = tech_template();
        t.extra_prompt = "1、xxx\n2、yyy".into();
        let p = build_prompt(&sample_summary(), &t, &[]);
        let spec_pos = p.find("<format_spec").unwrap();
        // style 细则不该出现
        assert!(!p.contains("**风格细则**"));
        // sections 约束仍要在 spec 之后（次优先级）
        let sec_pos = p.find("章节约束").unwrap();
        assert!(spec_pos < sec_pos);
    }

    // -------- 空 Summary / 换行处理 --------

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

    // ============================================================
    // 两轮 LLM 提炼-渲染（GenerationMode::TwoRoundSilent）
    // ============================================================

    fn sample_achievement() -> Achievement {
        Achievement {
            project: "weekly-report".into(),
            title: "实现 LLM provider 抽象层".into(),
            detail: "新增 llm.rs 模块，支持 OpenAI/Anthropic/Gemini 三种协议".into(),
            evidence: "用户指令：「实现 LLM provider 抽象」".into(),
            date_range: "2026-05-17".into(),
        }
    }

    #[test]
    fn achievement_round_trip_json() {
        let a = sample_achievement();
        let s = serde_json::to_string(&a).unwrap();
        let back: Achievement = serde_json::from_str(&s).unwrap();
        assert_eq!(a, back);
    }

    #[test]
    fn achievement_deserialization_allows_missing_date_range() {
        let json = r#"{
            "project": "x",
            "title": "做 X",
            "detail": "细节",
            "evidence": "证据"
        }"#;
        let a: Achievement = serde_json::from_str(json).unwrap();
        assert_eq!(a.date_range, "");
    }

    // -------- parse_achievements_json --------

    #[test]
    fn parse_clean_json_array() {
        let json = r#"[{"project":"p","title":"t","detail":"d","evidence":"e","date_range":"2026-05-17"}]"#;
        let v = parse_achievements_json(json).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].title, "t");
    }

    #[test]
    fn parse_json_wrapped_in_markdown_codeblock() {
        let wrapped = "```json\n[{\"project\":\"p\",\"title\":\"t\",\"detail\":\"d\",\"evidence\":\"e\"}]\n```";
        let v = parse_achievements_json(wrapped).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn parse_json_with_leading_explanation() {
        // LLM 有时会在 JSON 前面加一行解释 —— 用首 [ 末 ] 兜底
        let s =
            "这是结果：\n[{\"project\":\"p\",\"title\":\"t\",\"detail\":\"d\",\"evidence\":\"e\"}]";
        let v = parse_achievements_json(s).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn parse_invalid_returns_err() {
        assert!(parse_achievements_json("not json at all").is_err());
        assert!(parse_achievements_json("").is_err());
        assert!(parse_achievements_json("[invalid json").is_err());
    }

    #[test]
    fn parse_empty_array_ok() {
        let v = parse_achievements_json("[]").unwrap();
        assert!(v.is_empty());
    }

    // -------- build_extract_prompt --------

    #[test]
    fn extract_prompt_requests_json_array() {
        let p = build_extract_prompt(&sample_summary());
        assert!(p.contains("JSON 数组"));
        assert!(p.contains("直接输出 JSON 数组"));
        assert!(p.contains("不要任何 Markdown 代码块包裹"));
        // 必须列出 5 个字段
        assert!(p.contains("project"));
        assert!(p.contains("title"));
        assert!(p.contains("detail"));
        assert!(p.contains("evidence"));
        assert!(p.contains("date_range"));
    }

    #[test]
    fn extract_prompt_includes_work_logs() {
        let p = build_extract_prompt(&sample_summary());
        assert!(p.contains("<work_logs>"));
        assert!(p.contains("【weekly-report】"));
        assert!(p.contains("· [2026-05-17] 实现 LLM provider 抽象"));
    }

    #[test]
    fn extract_prompt_warns_against_fabrication() {
        let p = build_extract_prompt(&sample_summary());
        assert!(p.contains("不要编造任何事项"));
        assert!(p.contains("排除") && p.contains("过程性指令"));
    }

    #[test]
    fn extract_prompt_includes_project_docs_when_present() {
        let mut s = sample_summary();
        s.project_docs.insert(
            "weekly-report".into(),
            "## 强信号文档\n### README.md\n这是周报项目".into(),
        );
        let p = build_extract_prompt(&s);
        assert!(p.contains("项目背景"));
        assert!(p.contains("这是周报项目"));
    }

    // -------- build_prompt_from_achievements --------

    #[test]
    fn round2_prompt_uses_achievements_as_source() {
        let acks = vec![sample_achievement()];
        let p = build_prompt_from_achievements(&acks, &sample_summary(), &tech_template(), &[]);
        assert!(p.contains("<achievements>"));
        assert!(p.contains("</achievements>"));
        assert!(p.contains("实现 LLM provider 抽象层"));
        // 不应再含原始 work_logs 段
        assert!(!p.contains("<work_logs>"));
    }

    #[test]
    fn round2_prompt_keeps_hard_constraints_and_format_spec() {
        let acks = vec![sample_achievement()];
        let t = custom_template_with_extra("1、X\n2、Y");
        let p = build_prompt_from_achievements(&acks, &sample_summary(), &t, &[]);
        // 同样的 3 条硬约束
        assert!(p.contains("绝对禁止任何前言"));
        assert!(p.contains("不要编造任何数字"));
        assert!(p.contains("「（推断）」"));
        // 用户 extra_prompt 仍以 format_spec 包裹在顶部
        let spec_pos = p
            .find("<format_spec must_follow=\"true\">")
            .expect("format_spec");
        let ack_pos = p.find("<achievements>").expect("achievements");
        assert!(spec_pos < ack_pos, "format_spec 应在 achievements 之前");
    }

    #[test]
    fn round2_prompt_tells_llm_not_to_quote_evidence() {
        let acks = vec![sample_achievement()];
        let p = build_prompt_from_achievements(&acks, &sample_summary(), &tech_template(), &[]);
        assert!(p.contains("不要直接把 evidence 写进周报"));
        assert!(p.contains("不要遗漏任何 achievement"));
    }

    #[test]
    fn round2_prompt_handles_empty_achievements() {
        let p = build_prompt_from_achievements(&[], &sample_summary(), &tech_template(), &[]);
        assert!(p.contains("<achievements>"));
        assert!(p.contains("[]"), "空数组应渲染为 []");
    }

    #[test]
    fn round2_prompt_renders_achievements_as_pretty_json() {
        let acks = vec![sample_achievement()];
        let p = build_prompt_from_achievements(&acks, &sample_summary(), &tech_template(), &[]);
        // pretty JSON 缩进让 LLM 易读
        assert!(p.contains("\"project\""));
        assert!(p.contains("\"title\""));
    }

    #[test]
    fn round2_prompt_omits_style_guide_for_custom() {
        let acks = vec![sample_achievement()];
        let t = custom_template_with_extra("用户格式");
        let p = build_prompt_from_achievements(&acks, &sample_summary(), &t, &[]);
        // custom + extra_prompt 时不该有内置风格细则
        assert!(!p.contains("**风格细则**"));
    }

    #[test]
    fn round2_prompt_keeps_section_constraint() {
        let acks = vec![sample_achievement()];
        let p = build_prompt_from_achievements(&acks, &sample_summary(), &tech_template(), &[]);
        assert!(p.contains("章节约束"));
        assert!(p.contains("1. 本周 TL;DR"));
    }

    #[test]
    fn round2_prompt_includes_past_reports_when_provided() {
        let acks = vec![sample_achievement()];
        let past = vec!["# 旧报告".to_string()];
        let p = build_prompt_from_achievements(&acks, &sample_summary(), &tech_template(), &past);
        assert!(p.contains("<format_reference_only"));
        assert!(p.contains("不要照抄章节结构或格式"));
    }
}
