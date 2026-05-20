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
//! 阶段 6 会在 [`collect_messages`] 里加 SSH 分支：先 rsync 到本地缓存再走本机逻辑。
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Local};
use std::collections::{BTreeSet, HashMap};
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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
    pub tool: Tool,
    pub server: String,
}

/// 聚合后的工作摘要，喂给 LLM 用。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    /// 项目名 → 用户指令列表（已按时序排序、相邻去重）
    pub by_project: HashMap<String, Vec<String>>,
    /// 少量助手回复片段（≤ 10 条），用于让 LLM 把握风格
    pub ai_snippets: Vec<String>,
    pub stats: SummaryStats,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
            // 先 rsync 到本地缓存，再走与本机相同的解析逻辑。
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
    .map_err(|e| anyhow!("收集任务 panic: {e}"))?
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

    let mut by_project: HashMap<String, Vec<String>> = HashMap::new();
    let mut servers: BTreeSet<String> = BTreeSet::new();
    let mut tools: BTreeSet<String> = BTreeSet::new();
    let mut active_days: BTreeSet<String> = BTreeSet::new();
    let mut last_text_per_project: HashMap<String, String> = HashMap::new();
    let mut total_prompts: u32 = 0;

    // assistant snippets：先全收集，最后按 ts 排序取最新
    let mut assistant_pool: Vec<(Option<DateTime<Local>>, String)> = Vec::new();

    for m in messages {
        servers.insert(m.server.clone());
        tools.insert(m.tool.as_str().to_string());
        if let Some(ts) = m.ts {
            active_days.insert(ts.format("%Y-%m-%d").to_string());
        }
        match m.role {
            Role::User => {
                if let Some(prev) = last_text_per_project.get(&m.project) {
                    if prefix_match(prev, &m.text, 30) {
                        continue; // 相邻重复，跳过
                    }
                }
                last_text_per_project.insert(m.project.clone(), m.text.clone());
                by_project.entry(m.project).or_default().push(m.text);
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
        ai_snippets,
        stats,
    }
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
        assert_eq!(s.stats.total_prompts, 0);
        assert_eq!(s.stats.main_project, None);
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
            ssh_key: None,
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
