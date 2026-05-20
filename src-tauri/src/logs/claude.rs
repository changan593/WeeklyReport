//! Claude Code 日志解析（`~/.claude/history.jsonl` + `projects/<encoded>/*.jsonl`）。
//!
//! 详细 schema 与陷阱见 `docs/JSONL.md#3-claude-code--historyjsonl`
//! 和 `docs/JSONL.md#4-claude-code--项目-session-jsonl`。

use chrono::{DateTime, Local};
use serde_json::Value;
use std::path::Path;

use super::compress::{clip_text, path_basename, value_to_short_str};
use super::{parse_ts, Message, ParseStats, Role, Tool};

/// 扫描 Claude Code 根目录，输出 since 之后的所有 Messages + 跳过统计。
///
/// 失败的单个文件 / 单行 `warn!` 后跳过且计数，不阻塞整体收集。
pub fn collect(
    root: &Path,
    server: &str,
    since: DateTime<Local>,
    clip_chars: usize,
) -> (Vec<Message>, ParseStats) {
    let mut out = Vec::new();
    let mut stats = ParseStats::default();

    // 1. ~/.claude/history.jsonl
    let history = root.join("history.jsonl");
    if history.is_file() {
        match std::fs::metadata(&history) {
            Ok(meta) if super::mtime_after(&meta, since) => {
                let (msgs, s) = parse_history_file(&history, server, since);
                out.extend(msgs);
                stats.merge(&s);
            }
            Ok(_) => {} // 文件太旧
            Err(e) => {
                tracing::warn!("history.jsonl 元数据读取失败: {e}");
                stats.skipped_files += 1;
            }
        }
    }

    // 2. ~/.claude/projects/<encoded>/*.jsonl
    let projects_dir = root.join("projects");
    if projects_dir.is_dir() {
        for entry in walkdir::WalkDir::new(&projects_dir)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            match std::fs::metadata(path) {
                Ok(meta) if super::mtime_after(&meta, since) => {
                    let (msgs, s) = parse_session_file(path, server, since, clip_chars);
                    out.extend(msgs);
                    stats.merge(&s);
                }
                _ => {}
            }
        }
    }

    (out, stats)
}

// ============================================================
// history.jsonl
// ============================================================

fn parse_history_file(
    path: &Path,
    server: &str,
    since: DateTime<Local>,
) -> (Vec<Message>, ParseStats) {
    let mut stats = ParseStats::default();
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("读取 {} 失败: {}", path.display(), e);
            stats.skipped_files += 1;
            return (Vec::new(), stats);
        }
    };
    let mut msgs = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match parse_history_line(line, server) {
            Some(m) if m.ts.map_or(true, |ts| ts >= since) => msgs.push(m),
            Some(_) => {} // 在窗口外，正常丢弃
            None => {
                // 区分"行不合法 JSON"（应计数）vs"display 字段不存在"（设计内丢弃）
                if serde_json::from_str::<Value>(line.trim()).is_err() {
                    stats.skipped_lines += 1;
                }
            }
        }
    }
    (msgs, stats)
}

/// 解析 `~/.claude/history.jsonl` 一行。
///
/// 字段：`display` / `timestamp(ms)` / `project` / `pastedContents`。
/// 详见 `docs/JSONL.md#3`。
pub(crate) fn parse_history_line(line: &str, server: &str) -> Option<Message> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let text = v.get("display")?.as_str()?.trim().to_string();
    if text.is_empty() {
        return None;
    }
    let ts = v.get("timestamp").and_then(parse_ts);
    let project = v
        .get("project")
        .and_then(|p| p.as_str())
        .map(path_basename)
        .unwrap_or_else(|| "Claude Code".to_string());
    Some(Message {
        role: Role::User,
        text,
        ts,
        project,
        tool: Tool::ClaudeCode,
        server: server.to_string(),
    })
}

// ============================================================
// projects/<encoded>/<sessionId>.jsonl
// ============================================================

fn parse_session_file(
    path: &Path,
    server: &str,
    since: DateTime<Local>,
    clip_chars: usize,
) -> (Vec<Message>, ParseStats) {
    let mut stats = ParseStats::default();
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("读取 {} 失败: {}", path.display(), e);
            stats.skipped_files += 1;
            return (Vec::new(), stats);
        }
    };

    // 第一遍：找 cwd 作为 project 名。
    let project = content
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|v| v.get("cwd").and_then(|c| c.as_str()).map(path_basename))
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });

    // 第二遍：按行解析。
    let mut messages = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // 先 JSON 校验：失败计入 skipped_lines；后续业务判断（如 type 未知）不计数
        if serde_json::from_str::<Value>(trimmed).is_err() {
            stats.skipped_lines += 1;
            continue;
        }
        for m in parse_session_line(line, server, &project, clip_chars) {
            if m.ts.map_or(true, |ts| ts >= since) {
                messages.push(m);
            }
        }
    }
    (messages, stats)
}

/// 解析 Claude Code 项目 session JSONL 的一行。
///
/// 返回 0 条（应丢弃）或多条 Messages（用户 prompt + 助手文本各算一条）。
/// 行解析失败 `warn!` 并返回空。
pub(crate) fn parse_session_line(
    line: &str,
    server: &str,
    project: &str,
    clip_chars: usize,
) -> Vec<Message> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("解析 Claude session 行失败: {e}");
            return Vec::new();
        }
    };

    let ts = v.get("timestamp").and_then(parse_ts);
    let t = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match t {
        "user" => parse_user_line(&v, server, project, ts),
        "assistant" => parse_assistant_line(&v, server, project, ts, clip_chars),
        // summary / git-commit / 其他未知 type 全部丢弃
        _ => Vec::new(),
    }
}

/// 真实用户 prompt 必须满足（详见 JSONL.md §4.4）：
/// - `type == "user"`
/// - `message.content` 是非空字符串
/// - `toolUseResult` 不存在
/// - `isMeta` 不为 true
fn parse_user_line(
    v: &Value,
    server: &str,
    project: &str,
    ts: Option<DateTime<Local>>,
) -> Vec<Message> {
    if v.get("toolUseResult").is_some() {
        return Vec::new();
    }
    if v.get("isMeta").and_then(|b| b.as_bool()) == Some(true) {
        return Vec::new();
    }
    let content = match v.get("message").and_then(|m| m.get("content")) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let text = match content.as_str() {
        Some(s) => s.trim().to_string(),
        None => return Vec::new(), // 数组形态 = tool_result 反灌，丢弃
    };
    if text.is_empty() {
        return Vec::new();
    }
    vec![Message {
        role: Role::User,
        text,
        ts,
        project: project.to_string(),
        tool: Tool::ClaudeCode,
        server: server.to_string(),
    }]
}

/// 助手消息：从 `message.content` 数组里抽出所有 `text` 块拼接、clip。
/// `thinking` / `tool_use` / `tool_result` 块全部丢弃（tool_use 信号在 v0.1 不入 Summary）。
fn parse_assistant_line(
    v: &Value,
    server: &str,
    project: &str,
    ts: Option<DateTime<Local>>,
    clip_chars: usize,
) -> Vec<Message> {
    let content = match v.get("message").and_then(|m| m.get("content")) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let arr = match content.as_array() {
        Some(a) => a,
        None => return Vec::new(), // 字符串形态在 assistant 罕见，跳过
    };

    let mut text_parts: Vec<&str> = Vec::new();
    for block in arr {
        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(s) = block.get("text").and_then(|t| t.as_str()) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    text_parts.push(trimmed);
                }
            }
        }
    }

    if text_parts.is_empty() {
        return Vec::new();
    }
    let combined = text_parts.join("\n");
    let clipped = clip_text(&combined, clip_chars);
    vec![Message {
        role: Role::Assistant,
        text: clipped,
        ts,
        project: project.to_string(),
        tool: Tool::ClaudeCode,
        server: server.to_string(),
    }]
}

// ============================================================
// 工具函数（暂未使用，预留给阶段 9 打磨时把 tool_use 加回 Summary）
// ============================================================

/// 从 `tool_use.input` 抽出一个关键参数；常见键优先，缺则取首个键。
#[allow(dead_code)]
pub(crate) fn extract_tool_use_key_arg(input: Option<&Value>) -> Option<String> {
    let obj = input?.as_object()?;
    for key in [
        "file_path",
        "path",
        "pattern",
        "command",
        "url",
        "query",
        "prompt",
    ] {
        if let Some(val) = obj.get(key) {
            return Some(value_to_short_str(val, 60));
        }
    }
    obj.iter().next().map(|(_, v)| value_to_short_str(v, 60))
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    const HISTORY: &str = include_str!("fixtures/claude_history.jsonl");
    const SESSION_STRING: &str = include_str!("fixtures/claude_session_string.jsonl");
    const SESSION_ARRAY: &str = include_str!("fixtures/claude_session_array.jsonl");
    const SESSION_TOOL_RESULT: &str = include_str!("fixtures/claude_session_tool_result.jsonl");

    fn parse_session_lines(text: &str, project: &str) -> Vec<Message> {
        text.lines()
            .flat_map(|l| parse_session_line(l, "test-srv", project, 200))
            .collect()
    }

    #[test]
    fn history_line_parses_basic() {
        let line = r#"{"display":"hello","timestamp":1747476225000,"project":"-Users-me-app"}"#;
        let m = parse_history_line(line, "srv").unwrap();
        assert_eq!(m.text, "hello");
        assert_eq!(m.role, Role::User);
        assert_eq!(m.tool, Tool::ClaudeCode);
        assert_eq!(m.server, "srv");
        assert_eq!(m.project, "-Users-me-app");
        assert!(m.ts.is_some());
    }

    #[test]
    fn history_line_handles_second_timestamps() {
        // 部分老版本可能用秒（< 1e12）
        let line = r#"{"display":"hi","timestamp":1747476225,"project":"x"}"#;
        let m = parse_history_line(line, "srv").unwrap();
        assert!(m.ts.is_some());
    }

    #[test]
    fn history_line_skips_empty_display() {
        let line = r#"{"display":"   ","timestamp":1,"project":"x"}"#;
        assert!(parse_history_line(line, "srv").is_none());
    }

    #[test]
    fn history_line_skips_broken_json() {
        assert!(parse_history_line("{ not json", "srv").is_none());
    }

    #[test]
    fn history_fixture_parses_all_real_prompts() {
        let msgs: Vec<Message> = HISTORY
            .lines()
            .filter_map(|l| parse_history_line(l, "srv"))
            .collect();
        assert!(
            msgs.len() >= 3,
            "应解析至少 3 条 prompt, 实际 {}",
            msgs.len()
        );
        assert!(msgs.iter().all(|m| m.role == Role::User));
    }

    #[test]
    fn session_string_user_is_real_prompt() {
        let msgs = parse_session_lines(SESSION_STRING, "weekly-report");
        let user_msgs: Vec<_> = msgs.iter().filter(|m| m.role == Role::User).collect();
        assert!(!user_msgs.is_empty());
        assert!(user_msgs.iter().all(|m| !m.text.is_empty()));
    }

    #[test]
    fn session_array_assistant_text_is_clipped() {
        let msgs = parse_session_lines(SESSION_ARRAY, "weekly-report");
        let asst: Vec<_> = msgs.iter().filter(|m| m.role == Role::Assistant).collect();
        assert!(!asst.is_empty(), "应至少有一条 assistant text 消息");
    }

    #[test]
    fn session_array_drops_thinking_and_tool_use() {
        let msgs = parse_session_lines(SESSION_ARRAY, "weekly-report");
        // 不应出现 thinking 内容
        assert!(!msgs.iter().any(|m| m.text.contains("我在思考")));
        // 不应有 role=tool 或 [Read 这种渲染（v0.1 不入 Summary）
        assert!(!msgs.iter().any(|m| m.text.starts_with('[')));
    }

    #[test]
    fn tool_result_line_with_tool_use_result_field_is_dropped() {
        // type:"user" + toolUseResult → 实质 tool result，必须不被算作用户指令
        let msgs = parse_session_lines(SESSION_TOOL_RESULT, "weekly-report");
        let user_msgs: Vec<_> = msgs.iter().filter(|m| m.role == Role::User).collect();
        // fixture 里只有 1 行真实用户 prompt，其他都是 tool result 反灌
        assert_eq!(
            user_msgs.len(),
            1,
            "tool result 反灌不应被算作用户 prompt，实际 {} 条",
            user_msgs.len()
        );
        assert_eq!(user_msgs[0].text, "运行测试看看");
    }

    #[test]
    fn tool_result_array_content_dropped() {
        // type:"user" + message.content 是数组（即使没有 toolUseResult）也算 tool result
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"out"}]},"timestamp":"2026-05-17T10:00:00Z"}"#;
        let msgs = parse_session_line(line, "srv", "p", 200);
        assert!(msgs.is_empty());
    }

    #[test]
    fn ismeta_user_dropped() {
        let line = r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"系统提示"},"timestamp":"2026-05-17T10:00:00Z"}"#;
        let msgs = parse_session_line(line, "srv", "p", 200);
        assert!(msgs.is_empty());
    }

    #[test]
    fn unknown_type_silently_dropped() {
        let line = r#"{"type":"summary","summary":"会话摘要","timestamp":"2026-05-17T10:00:00Z"}"#;
        let msgs = parse_session_line(line, "srv", "p", 200);
        assert!(msgs.is_empty());

        let line = r#"{"type":"git-commit","timestamp":"2026-05-17T10:00:00Z"}"#;
        let msgs = parse_session_line(line, "srv", "p", 200);
        assert!(msgs.is_empty());
    }

    #[test]
    fn broken_line_silently_dropped() {
        let msgs = parse_session_line("{ broken", "srv", "p", 200);
        assert!(msgs.is_empty());
        let msgs = parse_session_line("", "srv", "p", 200);
        assert!(msgs.is_empty());
    }

    #[test]
    fn infer_project_from_cwd_in_session() {
        // collect 通过两遍扫描提取 cwd；这里直接用 fixture
        let project = SESSION_STRING
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find_map(|v| v.get("cwd").and_then(|c| c.as_str()).map(path_basename));
        assert_eq!(project.as_deref(), Some("weekly-report"));
    }

    #[test]
    fn extract_tool_use_key_arg_prefers_file_path() {
        let v = serde_json::json!({"file_path": "src/scheduler.rs", "limit": 50});
        let arg = extract_tool_use_key_arg(Some(&v));
        assert_eq!(arg.as_deref(), Some("src/scheduler.rs"));
    }

    #[test]
    fn extract_tool_use_key_arg_falls_back_to_first() {
        let v = serde_json::json!({"weird_key": "weird value"});
        let arg = extract_tool_use_key_arg(Some(&v));
        assert_eq!(arg.as_deref(), Some("weird value"));
    }
}
