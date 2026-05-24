//! 日志解析与压缩（核心模块）。
//!
//! 实现 `docs/SPEC.md#3-核心算法token-压缩策略`，schema 详见
//! `docs/JSONL.md`，解析策略详见
//! `docs/DECISIONS.md#adr-011jsonl-解析使用-serde_jsonvalue-而非强类型-struct`。
//!
//! 子模块：
//! - [`claude`]：Claude Code `history.jsonl` + `projects/<encoded>/*.jsonl`
//! - [`codex`]：Codex CLI `~/.codex/sessions/.../rollout-*.jsonl`
//! - [`compress`]：clip_text / path_basename 等工具
//!
//! 阶段 6 会在 [`collect_messages`] 里加 SSH 分支：先 ssh+tar 流式拉到本地缓存再走本机逻辑。
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use uuid::Uuid;

use crate::workspace::{expand_tilde, Workspace, WorkspaceKind};

pub mod claude;
pub mod codex;
pub mod compress;

// ============================================================
// 数据模型
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    ClaudeCode,
    Codex,
}

impl Tool {
    pub fn as_str(self) -> &'static str {
        match self {
            Tool::ClaudeCode => "claude-code",
            Tool::Codex => "codex",
        }
    }
}

/// 一条工作记录。`text` 已按角色完成压缩（user 全文 / assistant clipped）。
///
/// 详见 `docs/ARCHITECTURE.md#35-logsrs--日志解析与压缩-核心`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
    pub ts: Option<DateTime<Local>>,
    pub project: String,
    /// 项目真实路径（cwd）。来自 Claude session / Codex 日志时是完整路径；
    /// 来自 Claude `history.jsonl` 时为 `None`（该文件只存编码过的 project 名，
    /// 无法可靠还原成真实路径）。用于后续读取项目根目录的 md 文档作背景。
    pub project_path: Option<String>,
    /// 仅对 `role == User` 有意义：该指令引发的 AI 回复（已裁剪到末尾结论段）。
    /// 由 `pair_replies` 在 session 内配对填充。
    pub reply: Option<String>,
    pub tool: Tool,
    pub server: String,
}

/// 用户指令的最小单元（aggregate 后送往前端编辑 / 持久化为 draft / 喂给 prompt）。
///
/// 来源：
/// - `source = "claude-code"` / `"codex"`：从 JSONL 解析
/// - `source = "manual"` 且 `manual = true`：用户在编辑步骤里手动新增的
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogItem {
    /// 前端识别用，不持久化语义；后端只读不写
    pub id: String,
    /// ISO 8601 时间戳；少数日志可能无 ts → None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    pub text: String,
    /// `"claude-code"` / `"codex"` / `"manual"`
    pub source: String,
    /// 来源 workspace 名（= `Message.server`，用户给工作区起的名）。手动新增条目为空。
    #[serde(default)]
    pub server: String,
    /// 该指令引发的 AI 回复（已裁剪到末尾结论段，≤ 200 字符）。
    /// 用户连发两条、中间无回复时为 `None`；手动新增条目也为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    /// 用户手动新增的（不来自日志）
    #[serde(default)]
    pub manual: bool,
}

/// 聚合后的工作摘要，喂给 LLM 用，也是前端 review 步骤的中间数据。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// 项目名 → 工作指令条目列表（已按时序排序、相邻去重）
    pub by_project: HashMap<String, Vec<LogItem>>,
    /// 项目名 → 真实路径（cwd）。仅含能确定路径的项目；用于读项目 md 文档。
    /// 同名项目有多个 cwd 时取出现次数最多的。
    #[serde(default)]
    pub project_paths: HashMap<String, String>,
    /// 项目名 → 项目根目录 md 文档合并文本（README 等）。作为 LLM 的项目背景。
    /// 由 `collect_summary` 在聚合后填充；`aggregate` 本身不读文件，留空。
    #[serde(default)]
    pub project_docs: HashMap<String, String>,
    /// 少量助手回复片段（≤ 10 条），用于让 LLM 把握风格
    pub ai_snippets: Vec<String>,
    pub stats: SummaryStats,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryStats {
    pub total_prompts: u32,
    pub active_days: u32,
    pub project_count: u32,
    pub main_project: Option<String>,
    pub servers: Vec<String>,
    pub tools: Vec<String>,
    /// JSONL 行解析失败计数（JSON 不合法 / 数据损坏）。
    /// 用于让用户知道"扫了 N 行但 M 行没认出来"，避免静默丢数据。
    pub skipped_lines: u32,
    /// 文件级失败计数（IO 失败 / 整文件不可读）。
    pub skipped_files: u32,
}

/// 解析时累计的统计（线程内单次收集用，可在 spawn_blocking 内传递）。
///
/// `skipped_lines` 只累 **JSON 解析失败** 的行 —— "type 未知"、"用户被识别为
/// tool_result 反灌"等是设计内丢弃，不算 skip。
#[derive(Debug, Clone, Default)]
pub struct ParseStats {
    pub skipped_lines: u32,
    pub skipped_files: u32,
}

impl ParseStats {
    pub fn merge(&mut self, other: &ParseStats) {
        self.skipped_lines += other.skipped_lines;
        self.skipped_files += other.skipped_files;
    }
}

/// `ai_snippets` 上限；超过这个数后按时间排序取最新 N 条。
const AI_SNIPPETS_LIMIT: usize = 10;

/// 每条 user 指令配对的 AI 回复裁剪后保留的最大字符数（取末尾结论段）。
const REPLY_MAX_CHARS: usize = 200;

// ============================================================
// 公共入口
// ============================================================

/// 从一个 workspace 收集最近 `days` 天的 Messages。
///
/// `clip_chars` 是 assistant 文本首尾保留字符数（默认 200，来自 Settings）。
///
/// 本机分支立刻可用；SSH 分支会在阶段 6 接上。任何一个数据源失败都不会
/// 阻塞其他源 —— 单文件/单行错误只 `warn!` 然后继续。
pub async fn collect_messages(
    ws: &Workspace,
    days: u32,
    clip_chars: usize,
) -> Result<(Vec<Message>, ParseStats)> {
    let (claude_root, codex_root) = match ws.kind {
        WorkspaceKind::Local => local_roots(ws),
        WorkspaceKind::Ssh => {
            // 先 ssh+tar 流式拉到本地缓存，再走与本机相同的解析逻辑。
            let cache = crate::ssh::sync_to_cache(ws).await?;
            (
                cache.get("claude-code").cloned(),
                cache.get("codex").cloned(),
            )
        }
    };

    let ws_name = ws.name.clone();
    tokio::task::spawn_blocking(move || {
        let since = Local::now() - Duration::days(days as i64);
        Ok(collect_from_paths(
            &ws_name,
            claude_root.as_deref(),
            codex_root.as_deref(),
            since,
            clip_chars,
        ))
    })
    .await
    .map_err(|e| {
        anyhow!(crate::i18n::t_var(
            "err.logs.collect_panic",
            &[("err", &e.to_string())]
        ))
    })?
}

/// 本机工作区根据 tools 决定哪些路径要扫；未启用的工具返回 None。
fn local_roots(ws: &Workspace) -> (Option<PathBuf>, Option<PathBuf>) {
    let claude = if ws.tools.iter().any(|t| t == "claude-code") {
        let raw = ws.claude_path.as_deref().unwrap_or("~/.claude");
        Some(PathBuf::from(expand_tilde(raw)))
    } else {
        None
    };
    let codex = if ws.tools.iter().any(|t| t == "codex") {
        let raw = ws.codex_path.as_deref().unwrap_or("~/.codex");
        Some(PathBuf::from(expand_tilde(raw)))
    } else {
        None
    };
    (claude, codex)
}

/// 给定 Claude / Codex 根目录（本机或 SSH 缓存），收集所有 since 之后的 Message。
fn collect_from_paths(
    server_name: &str,
    claude_root: Option<&Path>,
    codex_root: Option<&Path>,
    since: DateTime<Local>,
    clip_chars: usize,
) -> (Vec<Message>, ParseStats) {
    let mut out = Vec::new();
    let mut stats = ParseStats::default();
    if let Some(p) = claude_root {
        if p.is_dir() {
            let (msgs, s) = claude::collect(p, server_name, since, clip_chars);
            out.extend(msgs);
            stats.merge(&s);
        }
    }
    if let Some(p) = codex_root {
        if p.is_dir() {
            let (msgs, s) = codex::collect(p, server_name, since, clip_chars);
            out.extend(msgs);
            stats.merge(&s);
        }
    }
    (out, stats)
}

// ============================================================
// 聚合
// ============================================================

/// 把 Messages 聚合成 Summary，并合入解析阶段的 ParseStats（跳过行/文件计数）。
///
/// - 按时间排序；同项目内前 30 字符相同的相邻用户 prompt 视为重复，去重
/// - 助手文本取**最新** AI_SNIPPETS_LIMIT 条
/// - stats：总指令数、活跃天数（按 Local 日期去重）、项目数、主项目、servers、tools、
///   skipped_lines / skipped_files
pub fn aggregate(messages: Vec<Message>) -> Summary {
    aggregate_with_stats(messages, ParseStats::default())
}

pub fn aggregate_with_stats(mut messages: Vec<Message>, parse_stats: ParseStats) -> Summary {
    messages.sort_by_key(|m| (m.ts, m.project.clone()));

    let mut by_project: HashMap<String, Vec<LogItem>> = HashMap::new();
    let mut servers: BTreeSet<String> = BTreeSet::new();
    let mut tools: BTreeSet<String> = BTreeSet::new();
    let mut active_days: BTreeSet<String> = BTreeSet::new();
    let mut last_text_per_project: HashMap<String, String> = HashMap::new();
    let mut total_prompts: u32 = 0;
    // 每个项目的 cwd 投票：project → (path → 出现次数)；同名项目取票数最高的路径
    let mut path_votes: HashMap<String, HashMap<String, u32>> = HashMap::new();

    // assistant snippets：先全收集，最后按 ts 排序取最新
    let mut assistant_pool: Vec<(Option<DateTime<Local>>, String)> = Vec::new();

    for m in messages {
        servers.insert(m.server.clone());
        tools.insert(m.tool.as_str().to_string());
        if let Some(ts) = m.ts {
            active_days.insert(ts.format("%Y-%m-%d").to_string());
        }
        if let Some(pp) = &m.project_path {
            if !pp.trim().is_empty() {
                *path_votes
                    .entry(m.project.clone())
                    .or_default()
                    .entry(pp.clone())
                    .or_default() += 1;
            }
        }
        match m.role {
            Role::User => {
                // 工具注入的噪音（environment_context / turn_aborted /
                // automation 触发块）不是用户真实工作内容，直接跳过
                if is_noise_prompt(&m.text) {
                    continue;
                }
                if let Some(prev) = last_text_per_project.get(&m.project) {
                    if dedup_match(prev, &m.text, 30) {
                        // 相邻重复，跳过当前条 —— 但要把它的 reply 抢救给前一条。
                        //
                        // 背景：Claude Code 同一条用户指令同时存在于 history.jsonl
                        // （无 assistant 配对）和 projects/<sid>/<id>.jsonl
                        // （有 assistant 配对）。原逻辑「先入者赢」会让 history 的
                        // 无回复版本占座，session 的有回复版本被当重复扔掉 —— 表现
                        // 就是 Claude Code 条目永远没 AI 回复。
                        //
                        // 这里在丢弃前用 `bubble up reply` 策略：前一条没 reply、
                        // 当前条有 reply → 写到前一条上。Codex 单源不受影响。
                        if m.reply.is_some() {
                            if let Some(prev_item) = by_project
                                .get_mut(&m.project)
                                .and_then(|items| items.last_mut())
                            {
                                if prev_item.reply.is_none() {
                                    prev_item.reply = m.reply.clone();
                                }
                            }
                        }
                        continue;
                    }
                }
                last_text_per_project.insert(m.project.clone(), m.text.clone());
                let item = LogItem {
                    id: Uuid::new_v4().to_string(),
                    timestamp: m.ts.map(|t| t.to_rfc3339()),
                    source: m.tool.as_str().to_string(),
                    server: m.server.clone(),
                    reply: m.reply,
                    text: m.text,
                    manual: false,
                };
                by_project.entry(m.project).or_default().push(item);
                total_prompts += 1;
            }
            Role::Assistant => {
                if !m.text.trim().is_empty() {
                    assistant_pool.push((m.ts, m.text));
                }
            }
        }
    }

    // 取最新 N 条 assistant 片段
    assistant_pool.sort_by_key(|(ts, _)| std::cmp::Reverse(*ts));
    let ai_snippets: Vec<String> = assistant_pool
        .into_iter()
        .take(AI_SNIPPETS_LIMIT)
        .map(|(_, s)| s)
        .collect();

    let project_count = by_project.len() as u32;
    let main_project = by_project
        .iter()
        .max_by_key(|(_, v)| v.len())
        .map(|(k, _)| k.clone());

    // 每个项目取票数最高的 cwd 作为代表路径
    let project_paths: HashMap<String, String> = path_votes
        .into_iter()
        .filter_map(|(project, votes)| {
            votes
                .into_iter()
                .max_by_key(|(_, c)| *c)
                .map(|(path, _)| (project, path))
        })
        .collect();

    let stats = SummaryStats {
        total_prompts,
        active_days: active_days.len() as u32,
        project_count,
        main_project,
        servers: servers.into_iter().collect(),
        tools: tools.into_iter().collect(),
        skipped_lines: parse_stats.skipped_lines,
        skipped_files: parse_stats.skipped_files,
    };

    Summary {
        by_project,
        project_paths,
        project_docs: HashMap::new(),
        ai_snippets,
        stats,
    }
}

/// 从一批 Message 提取 项目名 → 真实路径（每个项目取第一个非空 path）。
///
/// 与 `Summary.project_paths` 的区别：这个不做投票，直接取首个，
/// 用于 `collect_summary` 按 workspace 即时读 md（同 workspace 内路径基本一致）。
pub fn project_paths_of(messages: &[Message]) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    for m in messages {
        if let Some(p) = &m.project_path {
            if !p.trim().is_empty() {
                out.entry(m.project.clone()).or_insert_with(|| p.clone());
            }
        }
    }
    out
}

// ============================================================
// 通用工具（子模块共享）
// ============================================================

/// 解析 timestamp 字段：兼容 ISO 8601 字符串 与 Unix epoch（秒/毫秒）数字。
///
/// 详见 JSONL.md §3（Claude history 是 ms 数字）与 §5（Codex 是 ISO 字符串）。
pub(crate) fn parse_ts(v: &serde_json::Value) -> Option<DateTime<Local>> {
    if let Some(s) = v.as_str() {
        return DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.with_timezone(&Local));
    }
    if let Some(n) = v.as_i64() {
        let ms = if n < 1_000_000_000_000 {
            n.saturating_mul(1000)
        } else {
            n
        };
        return DateTime::from_timestamp_millis(ms).map(|dt| dt.with_timezone(&Local));
    }
    if let Some(f) = v.as_f64() {
        let ms = if f < 1e12 {
            (f * 1000.0) as i64
        } else {
            f as i64
        };
        return DateTime::from_timestamp_millis(ms).map(|dt| dt.with_timezone(&Local));
    }
    None
}

/// 用户 prompt 去重：取两条文本的前 `n` 字符（按 Unicode char），
/// 比较到「较短者的长度」为止；若一方为空则不视为重复。
///
/// 既能命中"完全相同的指令重发"，也能命中"在原 prompt 后追加内容的复发"（典型场景：
/// 用户按上箭头改一改再发）。
fn prefix_match(prev: &str, current: &str, n: usize) -> bool {
    let p: Vec<char> = prev.chars().take(n).collect();
    let c: Vec<char> = current.chars().take(n).collect();
    let min = p.len().min(c.len());
    if min == 0 {
        return false;
    }
    p[..min] == c[..min]
}

/// 去重比对：在 [`prefix_match`] 之上，对带「占位符」的 Claude / Codex 文本
/// 做归一化，避免双源同一条指令因占位符内容不同而漏匹配。
///
/// 典型场景：Claude Code 的 `~/.claude/history.jsonl` 会把粘贴块压成
/// `[Pasted text #2 +74 lines]`，session jsonl 则保留完整内容。两条只在
/// 占位符那一段不同，前 30 字符对不上，原逻辑误判为不同指令。这里把占位符
/// 整段抹掉再比较，能正确把这对识别为重复。
fn dedup_match(prev: &str, current: &str, n: usize) -> bool {
    if prefix_match(prev, current, n) {
        return true;
    }
    let p_norm = strip_paste_placeholders(prev);
    let c_norm = strip_paste_placeholders(current);
    // 抹掉占位符后任何一边变空就别再比了，避免无内容的瞎匹配
    if p_norm.trim().is_empty() || c_norm.trim().is_empty() {
        return false;
    }
    prefix_match(&p_norm, &c_norm, n)
}

/// 抹掉 Claude Code / Codex 把附件压缩成的占位符（pasted text / image 引用），
/// 只用于去重比较；不改变 LogItem.text 实际存储。
///
/// 处理对象：
/// - `[Pasted text #2 +74 lines]` / `[Pasted text #1]`
/// - `[Image #1]`
/// - `<image name=[Image #1]></image>`
fn strip_paste_placeholders(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &s[i..];
        // 命中 `[Pasted text` 或 `[Image` 起始 → 跳到下一个 `]`
        let is_paste = rest.starts_with("[Pasted text");
        let is_image = rest.starts_with("[Image");
        if is_paste || is_image {
            if let Some(end_rel) = rest.find(']') {
                i += end_rel + 1;
                continue;
            }
        }
        // 命中 `<image ...>` 标签 → 跳到 `</image>` 之后；缺少闭合就跳到 `>`
        if rest.starts_with("<image") {
            if let Some(end_rel) = rest.find("</image>") {
                i += end_rel + "</image>".len();
                continue;
            }
            if let Some(end_rel) = rest.find('>') {
                i += end_rel + 1;
                continue;
            }
        }
        // 普通字符按 char 推进，避免切到 UTF-8 字节中间
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// 判断一条用户 prompt 是否为工具自动注入的噪音（非用户真实工作内容）。
///
/// 已知噪音类型（均来自 Codex CLI）：
/// - 每轮注入的 `<environment_context>` 环境信息块（cwd / shell / 日期 / 时区）
/// - 中断提示 `<turn_aborted>`
/// - automation 定时任务的触发文本（`Automation:` 元信息头 + `Automation ID:`）
pub(crate) fn is_noise_prompt(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<environment_context>")
        || t.starts_with("<turn_aborted>")
        || (t.starts_with("Automation:") && t.contains("Automation ID:"))
}

/// 取文本末尾最多 `max` 个字符（按 Unicode char）。
///
/// AI 回复的开头中间是过程叙述、末尾才是结论；故只保留末尾。
/// 超长时丢弃前面、在开头加 `…` 标记被截断。
pub(crate) fn clip_tail(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let tail: String = chars[chars.len() - max..].iter().collect();
    format!("…{}", tail.trim_start())
}

/// 在单个 session 的有序 Message 序列内，把每条 user 指令配上它引发的 AI 回复。
///
/// 收集 user[i] 之后、下一条 user 之前的**所有** assistant 文本拼起来，
/// 用 [`clip_tail`] 裁剪到 `REPLY_MAX_CHARS` 写入 `Message.reply`。
///
/// 为什么不取第一条 assistant：Claude Code 一个 user→assistant turn 因为多次
/// 工具调用（assistant→tool_use→tool_result→assistant→...）会拆成多条 assistant
/// JSONL 行。第一条往往只是「Let me check…」「我来分析一下」之类的开场白，真正
/// 的结论在后面。取末尾 N 字符既能拿到结论，也能在结论很短时回填前文上下文。
///
/// **必须**对单个 session 内的消息调用 —— 跨 session 混合会配错。
pub(crate) fn pair_replies(messages: &mut [Message]) {
    let len = messages.len();
    for i in 0..len {
        if messages[i].role != Role::User {
            continue;
        }
        // 收集 [i+1..] 段里直到下一条 user 之前的所有 assistant 文本
        let mut parts: Vec<&str> = Vec::new();
        for j in (i + 1)..len {
            match messages[j].role {
                Role::Assistant => {
                    let t = messages[j].text.trim();
                    if !t.is_empty() {
                        parts.push(t);
                    }
                }
                Role::User => break, // 下一轮 user，本轮收集结束
            }
        }
        if parts.is_empty() {
            continue;
        }
        let combined = parts.join("\n");
        let reply = clip_tail(combined.trim(), REPLY_MAX_CHARS);
        if !reply.is_empty() {
            messages[i].reply = Some(reply);
        }
    }
}

/// 判断文件 mtime 是否在 `since` 之后。元数据出错时保守返回 true（保留文件）。
pub(crate) fn mtime_after(meta: &Metadata, since: DateTime<Local>) -> bool {
    let modified: SystemTime = match meta.modified() {
        Ok(t) => t,
        Err(_) => return true,
    };
    let dt: DateTime<Local> = modified.into();
    dt >= since
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: Role, project: &str, text: &str, day_offset: i64) -> Message {
        Message {
            role,
            text: text.to_string(),
            ts: Some(Local::now() - Duration::days(day_offset)),
            project: project.to_string(),
            project_path: None,
            reply: None,
            tool: Tool::ClaudeCode,
            server: "本机".to_string(),
        }
    }

    /// 带 project_path 的 Message 构造（测 project_paths 聚合用）。
    fn msg_with_path(project: &str, path: &str, text: &str) -> Message {
        Message {
            role: Role::User,
            text: text.to_string(),
            ts: Some(Local::now()),
            project: project.to_string(),
            project_path: Some(path.to_string()),
            reply: None,
            tool: Tool::ClaudeCode,
            server: "本机".to_string(),
        }
    }

    #[test]
    fn aggregate_groups_user_prompts_by_project() {
        let msgs = vec![
            msg(Role::User, "a", "做 A 事", 1),
            msg(Role::User, "b", "做 B 事", 1),
            msg(Role::User, "a", "再做 A2", 0),
        ];
        let s = aggregate(msgs);
        assert_eq!(s.by_project.len(), 2);
        assert_eq!(s.by_project["a"].len(), 2);
        assert_eq!(s.by_project["b"].len(), 1);
        assert_eq!(s.stats.total_prompts, 3);
        assert_eq!(s.stats.project_count, 2);
    }

    #[test]
    fn aggregate_dedupes_adjacent_user_prompts_in_same_project() {
        // 前 30 字符相同 → 视为重复
        let msgs = vec![
            msg(Role::User, "a", "重构 scheduler.rs 的 cron 解析逻辑", 2),
            msg(
                Role::User,
                "a",
                "重构 scheduler.rs 的 cron 解析逻辑 又跑了一次",
                1,
            ),
            msg(Role::User, "a", "完全不同的另一个任务", 0),
        ];
        let s = aggregate(msgs);
        assert_eq!(s.by_project["a"].len(), 2, "相邻重复应去重");
    }

    #[test]
    fn aggregate_dedup_bubbles_reply_from_session_into_history_entry() {
        // 模拟 Claude Code 双源：history.jsonl 的同一条指令（无 reply）
        // 排在前面、session jsonl 的同一条指令（有 reply）排在后面。
        // 期望：保留前者（time 早），但 reply 被后者补上。
        let mut hist = msg(Role::User, "weekly-report", "把卡片状态改为绿色", 1);
        hist.reply = None;
        let mut sess = msg(Role::User, "weekly-report", "把卡片状态改为绿色", 1);
        sess.reply = Some("已把 enabled=true 的卡片背景改成 emerald-50".into());

        let s = aggregate(vec![hist, sess]);
        let items = &s.by_project["weekly-report"];
        assert_eq!(items.len(), 1, "重复应去重");
        assert_eq!(
            items[0].reply.as_deref(),
            Some("已把 enabled=true 的卡片背景改成 emerald-50"),
            "session 的 reply 应该被抢救回来"
        );
    }

    #[test]
    fn aggregate_dedup_does_not_clobber_existing_reply() {
        // 第一条已经有 reply 时不应被第二条覆盖（避免拿质量更差的回复）
        let mut a = msg(Role::User, "x", "跑一下 cargo test", 1);
        a.reply = Some("good reply".into());
        let mut b = msg(Role::User, "x", "跑一下 cargo test", 1);
        b.reply = Some("worse reply".into());

        let s = aggregate(vec![a, b]);
        let items = &s.by_project["x"];
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].reply.as_deref(), Some("good reply"));
    }

    #[test]
    fn aggregate_does_not_dedupe_across_projects() {
        let msgs = vec![
            msg(Role::User, "a", "相同前缀的指令", 1),
            msg(Role::User, "b", "相同前缀的指令", 1),
        ];
        let s = aggregate(msgs);
        assert_eq!(s.by_project["a"].len(), 1);
        assert_eq!(s.by_project["b"].len(), 1);
    }

    #[test]
    fn aggregate_ai_snippets_take_newest_capped() {
        let mut msgs = Vec::new();
        for i in 0..20 {
            msgs.push(msg(
                Role::Assistant,
                "x",
                &format!("回复 {i}"),
                20 - i as i64,
            ));
        }
        let s = aggregate(msgs);
        assert_eq!(s.ai_snippets.len(), AI_SNIPPETS_LIMIT);
        // 最新的（day_offset 最小）应在前
        assert!(s.ai_snippets[0].contains("回复 19"));
    }

    #[test]
    fn aggregate_main_project_is_top_by_prompt_count() {
        let mut msgs = Vec::new();
        // 5 条彼此 30 字内首字符就不同，保证不被去重
        for i in 0..5 {
            msgs.push(msg(
                Role::User,
                "big",
                &format!("{i} alpha 任务详细描述：执行步骤一二三"),
                1,
            ));
        }
        for i in 0..3 {
            msgs.push(msg(
                Role::User,
                "small",
                &format!("{i} beta 另一个项目的不同指令"),
                1,
            ));
        }
        let s = aggregate(msgs);
        assert_eq!(s.by_project["big"].len(), 5);
        assert_eq!(s.by_project["small"].len(), 3);
        assert_eq!(s.stats.main_project.as_deref(), Some("big"));
    }

    #[test]
    fn aggregate_dedupes_identical_repeats() {
        // 完全相同的指令重发也应只保留一条
        let msgs = vec![
            msg(Role::User, "a", "同一指令", 2),
            msg(Role::User, "a", "同一指令", 1),
            msg(Role::User, "a", "同一指令", 0),
        ];
        let s = aggregate(msgs);
        assert_eq!(s.by_project["a"].len(), 1);
    }

    #[test]
    fn prefix_match_basic_cases() {
        assert!(prefix_match("abc", "abcdef", 30), "prefix should dedup");
        assert!(prefix_match("hello world", "hello world!!!", 30));
        assert!(prefix_match("identical", "identical", 30));
        assert!(!prefix_match("abc", "xyz", 30));
        assert!(!prefix_match("", "abc", 30));
        assert!(!prefix_match("abc", "", 30));
    }

    #[test]
    fn is_noise_prompt_detects_codex_injections() {
        assert!(is_noise_prompt(
            "<environment_context>\n  <cwd>/x</cwd>\n</environment_context>"
        ));
        // 前导空白也要识别
        assert!(is_noise_prompt(
            "  <environment_context>\n<current_date>2026</current_date>"
        ));
        assert!(is_noise_prompt(
            "<turn_aborted>\nThe user interrupted the previous turn."
        ));
        assert!(is_noise_prompt(
            "Automation: companies observation\nAutomation ID: companies-observation\nLast run: x"
        ));
    }

    #[test]
    fn is_noise_prompt_keeps_real_prompts() {
        assert!(!is_noise_prompt("帮我重构 scheduler.rs 的 cron 解析"));
        // 只是提到 Automation 这个词，不是触发块
        assert!(!is_noise_prompt("Automation 这个词是什么意思"));
        assert!(!is_noise_prompt("讲一下 <environment> 标签的用法"));
    }

    #[test]
    fn aggregate_filters_noise_prompts() {
        let msgs = vec![
            msg(
                Role::User,
                "p",
                "<environment_context>\n<cwd>/x</cwd>\n</environment_context>",
                1,
            ),
            msg(Role::User, "p", "真实工作指令", 0),
        ];
        let s = aggregate(msgs);
        assert_eq!(s.by_project["p"].len(), 1, "噪音应被过滤");
        assert_eq!(s.by_project["p"][0].text, "真实工作指令");
        assert_eq!(s.stats.total_prompts, 1);
    }

    #[test]
    fn clip_tail_keeps_short_text() {
        assert_eq!(clip_tail("短文本", 200), "短文本");
    }

    #[test]
    fn clip_tail_truncates_from_end() {
        let long: String = "x".repeat(300);
        let out = clip_tail(&long, 200);
        assert!(out.starts_with('…'), "截断应在开头加 … 标记");
        assert_eq!(out.chars().count(), 201, "… + 末尾 200 字符");
    }

    #[test]
    fn pair_replies_attaches_reply_to_user() {
        let mut msgs = vec![
            msg(Role::User, "p", "做 A 功能", 0),
            msg(Role::Assistant, "p", "已完成 A 功能，新增 a.rs", 0),
        ];
        pair_replies(&mut msgs);
        assert_eq!(msgs[0].reply.as_deref(), Some("已完成 A 功能，新增 a.rs"));
    }

    #[test]
    fn pair_replies_no_reply_when_user_followed_by_user() {
        let mut msgs = vec![
            msg(Role::User, "p", "指令一", 0),
            msg(Role::User, "p", "指令二", 0),
        ];
        pair_replies(&mut msgs);
        assert_eq!(msgs[0].reply, None);
    }

    #[test]
    fn pair_replies_skips_blank_assistant() {
        let mut msgs = vec![
            msg(Role::User, "p", "指令", 0),
            msg(Role::Assistant, "p", "   ", 0),
        ];
        pair_replies(&mut msgs);
        assert_eq!(msgs[0].reply, None);
    }

    #[test]
    fn pair_replies_joins_multiple_assistants_in_one_turn() {
        // Claude Code 一个 turn 可拆成多条 assistant message（多次工具调用）。
        // 修复前只取第一条「Let me check…」开场白；修复后把后续的也拼进来。
        let mut msgs = vec![
            msg(Role::User, "p", "重构 cron 解析", 0),
            msg(Role::Assistant, "p", "Let me check.", 0),
            msg(Role::Assistant, "p", "已完成 cron 重构，新增单测", 0),
            msg(Role::User, "p", "下一条指令", 0),
        ];
        pair_replies(&mut msgs);
        let reply = msgs[0].reply.as_deref().expect("应配上 reply");
        // 必须包含末尾的真结论
        assert!(
            reply.contains("已完成 cron 重构"),
            "reply 应包含末尾结论，实际：{reply}"
        );
    }

    #[test]
    fn pair_replies_long_combined_drops_opener_keeps_conclusion() {
        // 多条 assistant 加起来超过 REPLY_MAX_CHARS=200 时，clip_tail 应该把开头
        // 的「Let me check…」开场白裁掉，只留末尾的真结论。
        let opener = "Let me check the current implementation.".to_string();
        let middle = "Analyzing.".repeat(30); // 撑长，让总长超过 200
        let conclusion = "已完成：补成 7 段，42 个单元测试全绿。";
        let mut msgs = vec![
            msg(Role::User, "p", "重构 cron 解析", 0),
            msg(Role::Assistant, "p", &opener, 0),
            msg(Role::Assistant, "p", &middle, 0),
            msg(Role::Assistant, "p", conclusion, 0),
            msg(Role::User, "p", "下一条", 0),
        ];
        pair_replies(&mut msgs);
        let reply = msgs[0].reply.as_deref().unwrap();
        assert!(reply.contains("已完成"), "应保留末尾结论：{reply}");
        assert!(!reply.contains("Let me check"), "开场白应被裁掉：{reply}");
        assert!(reply.starts_with('…'), "裁剪后应有 … 前缀");
    }

    #[test]
    fn pair_replies_long_combined_clipped_to_tail() {
        // 多条 assistant 加起来超过 REPLY_MAX_CHARS 时取末尾
        let long_first = "x".repeat(500);
        let mut msgs = vec![
            msg(Role::User, "p", "q", 0),
            msg(Role::Assistant, "p", &long_first, 0),
            msg(Role::Assistant, "p", "FINAL CONCLUSION HERE.", 0),
        ];
        pair_replies(&mut msgs);
        let reply = msgs[0].reply.as_deref().unwrap();
        assert!(reply.contains("FINAL CONCLUSION HERE."));
        assert!(reply.starts_with('…'), "末尾裁剪应带 … 前缀");
    }

    // -------- dedup_match: 占位符归一化 --------

    #[test]
    fn dedup_match_catches_pasted_text_placeholder_vs_full() {
        // 复现 Claude Code 双源问题：history 把粘贴块压成占位符，session 保留全文
        let history = "这是我新跑的日志：[Pasted text #2 +74 lines]";
        let session = "这是我新跑的日志：Total jobs run:     20 / 20\n  Completed: 18\n  ...";
        // 原始 prefix_match 应该不匹配（前 30 字符不同）
        assert!(!prefix_match(history, session, 30));
        // dedup_match 应该匹配（归一化后命中）
        assert!(dedup_match(history, session, 30));
    }

    #[test]
    fn dedup_match_catches_image_placeholder() {
        let a = "看下这个图：[Image #1]";
        let b = "看下这个图：[Image #2]";
        assert!(dedup_match(a, b, 30));
    }

    #[test]
    fn dedup_match_handles_xml_image_tag() {
        let a = "参考这个：<image name=[Image #1]></image>";
        let b = "参考这个：<image name=[Image #2]></image>";
        assert!(dedup_match(a, b, 30));
    }

    #[test]
    fn dedup_match_keeps_distinct_prompts_distinct() {
        assert!(!dedup_match(
            "重构 scheduler.rs 的 cron 解析",
            "修一下 email.rs 的 markdown 渲染",
            30,
        ));
    }

    #[test]
    fn dedup_match_does_not_match_when_both_normalize_to_empty() {
        // 全是占位符的两条不该被认作"同一条"（无信息）
        assert!(!dedup_match("[Pasted text #1]", "[Image #2]", 30));
    }

    #[test]
    fn strip_paste_placeholders_handles_common_forms() {
        assert_eq!(
            strip_paste_placeholders("这是我新跑的日志：[Pasted text #2 +74 lines]"),
            "这是我新跑的日志："
        );
        assert_eq!(
            strip_paste_placeholders("看图 [Image #1] 谢谢"),
            "看图  谢谢"
        );
        assert_eq!(
            strip_paste_placeholders("a <image name=[Image #1]></image> b"),
            "a  b"
        );
        // 不应误伤其它方括号
        assert_eq!(
            strip_paste_placeholders("数组 [1, 2, 3] 长度 3"),
            "数组 [1, 2, 3] 长度 3"
        );
    }

    #[test]
    fn aggregate_active_days_distinct() {
        let msgs = vec![
            msg(Role::User, "a", "p1", 0),
            msg(Role::User, "a", "p2", 0), // 同一天
            msg(Role::User, "a", "p3", 2), // 另一天
        ];
        let s = aggregate(msgs);
        assert_eq!(s.stats.active_days, 2);
    }

    #[test]
    fn aggregate_empty_messages_yields_empty_summary() {
        let s = aggregate(Vec::new());
        assert!(s.by_project.is_empty());
        assert!(s.ai_snippets.is_empty());
        assert!(s.project_paths.is_empty());
        assert_eq!(s.stats.total_prompts, 0);
        assert_eq!(s.stats.main_project, None);
    }

    #[test]
    fn aggregate_collects_project_paths() {
        let msgs = vec![
            msg_with_path("app", "/home/me/app", "做 A"),
            msg_with_path("app", "/home/me/app", "做 B"),
            msg(Role::User, "noPath", "无路径项目", 0),
        ];
        let s = aggregate(msgs);
        assert_eq!(
            s.project_paths.get("app").map(String::as_str),
            Some("/home/me/app")
        );
        // 无 project_path 的项目不进 project_paths
        assert!(!s.project_paths.contains_key("noPath"));
    }

    #[test]
    fn aggregate_project_path_picks_most_voted() {
        // 同名项目有两个不同 cwd，取出现次数多的
        let msgs = vec![
            msg_with_path("app", "/path/a", "x1"),
            msg_with_path("app", "/path/a", "x2"),
            msg_with_path("app", "/path/b", "x3"),
        ];
        let s = aggregate(msgs);
        assert_eq!(
            s.project_paths.get("app").map(String::as_str),
            Some("/path/a")
        );
    }

    #[test]
    fn aggregate_records_servers_and_tools() {
        let mut a = msg(Role::User, "p", "x", 0);
        a.server = "本机".into();
        a.tool = Tool::ClaudeCode;
        let mut b = msg(Role::User, "p", "y", 0);
        b.server = "GPU 服务器".into();
        b.tool = Tool::Codex;
        let s = aggregate(vec![a, b]);
        assert_eq!(s.stats.servers, vec!["GPU 服务器", "本机"]);
        assert_eq!(s.stats.tools, vec!["claude-code", "codex"]);
    }

    #[test]
    fn parse_ts_iso_string() {
        let v = serde_json::json!("2026-05-17T10:23:45Z");
        assert!(parse_ts(&v).is_some());
    }

    #[test]
    fn parse_ts_unix_ms() {
        let v = serde_json::json!(1_747_476_225_000i64);
        let ts = parse_ts(&v).unwrap();
        assert_eq!(ts.format("%Y-%m-%d").to_string(), "2025-05-17");
    }

    #[test]
    fn parse_ts_unix_seconds() {
        let v = serde_json::json!(1_747_476_225i64);
        assert!(parse_ts(&v).is_some());
    }

    #[test]
    fn parse_ts_invalid_returns_none() {
        assert!(parse_ts(&serde_json::json!("not a date")).is_none());
        assert!(parse_ts(&serde_json::json!(null)).is_none());
        assert!(parse_ts(&serde_json::json!({})).is_none());
    }

    /// 端到端：用 fixture 模拟一个 workspace 的本机日志目录，
    /// 验证 collect_messages → aggregate 输出符合预期。
    #[tokio::test]
    async fn collect_messages_end_to_end_with_fixtures() {
        // 临时目录布局：
        //   <tmp>/.claude/history.jsonl
        //   <tmp>/.claude/projects/-Users-me-weekly-report/abc.jsonl
        //   <tmp>/.codex/sessions/2026/05/17/rollout-xxx.jsonl
        let root =
            std::env::temp_dir().join(format!("weekly-report-logs-e2e-{}", uuid::Uuid::new_v4()));
        let claude = root.join(".claude");
        let codex = root.join(".codex");
        std::fs::create_dir_all(claude.join("projects/-Users-me-weekly-report")).unwrap();
        std::fs::create_dir_all(codex.join("sessions/2026/05/17")).unwrap();
        std::fs::write(
            claude.join("history.jsonl"),
            include_str!("logs/fixtures/claude_history.jsonl"),
        )
        .unwrap();
        std::fs::write(
            claude.join("projects/-Users-me-weekly-report/abc.jsonl"),
            include_str!("logs/fixtures/claude_session_string.jsonl"),
        )
        .unwrap();
        std::fs::write(
            codex.join("sessions/2026/05/17/rollout-test.jsonl"),
            include_str!("logs/fixtures/codex_rollout.jsonl"),
        )
        .unwrap();

        let ws = Workspace {
            id: "w1".into(),
            name: "测试机".into(),
            kind: WorkspaceKind::Local,
            host: None,
            user: None,
            port: None,
            auth_method: Default::default(),
            ssh_key: None,
            ssh_password: None,
            claude_path: Some(claude.to_string_lossy().into_owned()),
            codex_path: Some(codex.to_string_lossy().into_owned()),
            tools: vec!["claude-code".into(), "codex".into()],
        };

        // 用大窗口确保 fixture 时间戳都在窗口内；按文件 mtime 也保证（刚 write）
        let (messages, parse_stats) = collect_messages(&ws, 365 * 100, 200).await.unwrap();
        assert!(!messages.is_empty(), "fixtures 应至少产出一些 messages");
        assert_eq!(parse_stats.skipped_lines, 0, "fixture 应全部合法");
        assert_eq!(parse_stats.skipped_files, 0);

        let summary = aggregate_with_stats(messages, parse_stats);
        assert!(summary.stats.total_prompts > 0);
        assert!(
            summary.stats.tools.iter().any(|t| t == "claude-code"),
            "应至少有 claude-code"
        );
        assert!(summary.stats.tools.iter().any(|t| t == "codex"));
        assert!(summary.stats.servers.contains(&"测试机".to_string()));

        // 清理
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn collect_messages_ssh_without_host_errors() {
        // 阶段 6 起 SSH 分支启用：调 ssh::sync_to_cache 前先 require_ssh 校验 host
        let ws = Workspace {
            id: "w1".into(),
            name: "x".into(),
            kind: WorkspaceKind::Ssh,
            ..Default::default()
        };
        let err = collect_messages(&ws, 7, 200).await.unwrap_err().to_string();
        assert!(err.contains("host"), "应提示缺 host，实际: {err}");
    }

    #[tokio::test]
    async fn collect_messages_skips_missing_paths_silently() {
        let ws = Workspace {
            id: "w1".into(),
            name: "x".into(),
            kind: WorkspaceKind::Local,
            claude_path: Some("/non/existent/path".into()),
            codex_path: Some("/also/missing".into()),
            tools: vec!["claude-code".into(), "codex".into()],
            ..Default::default()
        };
        let (msgs, stats) = collect_messages(&ws, 7, 200).await.unwrap();
        assert!(msgs.is_empty());
        assert_eq!(stats.skipped_files, 0);
    }

    // -------- ParseStats / skipped 计数 --------

    #[tokio::test]
    async fn collect_counts_broken_lines() {
        // 临时目录中放一个含损坏行的 Claude session JSONL
        let root = std::env::temp_dir().join(format!(
            "weekly-report-logs-broken-{}",
            uuid::Uuid::new_v4()
        ));
        let claude = root.join(".claude");
        std::fs::create_dir_all(claude.join("projects/proj-a")).unwrap();
        std::fs::write(
            claude.join("projects/proj-a/sess.jsonl"),
            "{\"type\":\"user\",\"timestamp\":\"2026-05-17T10:00:00Z\",\"cwd\":\"/p\",\"message\":{\"role\":\"user\",\"content\":\"valid\"}}\n\
             { not valid json\n\
             also { broken\n",
        )
        .unwrap();

        let ws = Workspace {
            id: "w1".into(),
            name: "test".into(),
            kind: WorkspaceKind::Local,
            claude_path: Some(claude.to_string_lossy().into_owned()),
            tools: vec!["claude-code".into()],
            ..Default::default()
        };

        let (msgs, stats) = collect_messages(&ws, 365 * 100, 200).await.unwrap();
        assert_eq!(msgs.len(), 1, "应解析出 1 条合法消息");
        assert_eq!(stats.skipped_lines, 2, "应计入 2 行损坏 JSON");
        assert_eq!(stats.skipped_files, 0);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_stats_merge() {
        let mut a = ParseStats {
            skipped_lines: 3,
            skipped_files: 1,
        };
        let b = ParseStats {
            skipped_lines: 5,
            skipped_files: 2,
        };
        a.merge(&b);
        assert_eq!(a.skipped_lines, 8);
        assert_eq!(a.skipped_files, 3);
    }

    #[test]
    fn aggregate_with_stats_propagates_skipped_counts() {
        let stats = ParseStats {
            skipped_lines: 7,
            skipped_files: 2,
        };
        let s = aggregate_with_stats(Vec::new(), stats);
        assert_eq!(s.stats.skipped_lines, 7);
        assert_eq!(s.stats.skipped_files, 2);
    }
}
