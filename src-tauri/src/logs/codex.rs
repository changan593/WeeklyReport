//! OpenAI Codex CLI 日志解析（`~/.codex/sessions/.../rollout-*.jsonl`）。
//!
//! 详细 schema 见 `docs/JSONL.md#5-codex-cli--rollout-jsonl`。
//!
//! 关键差异（与 Claude 不同）：
//! - 两段嵌套：`{timestamp, type, payload}`
//! - `response_item.payload.type` 再分 message / function_call / reasoning / ...
//! - message content **永远是数组**（input_text / output_text / input_image）
//! - 已知 3 套兼容格式，用 `serde_json::Value` 容错。

use chrono::{DateTime, Local};
use serde_json::Value;
use std::path::Path;

use super::compress::{clip_text, path_basename};
use super::{parse_ts, Message, ParseStats, Role, Tool};

/// 扫描 Codex CLI 根目录，返回 Messages + 跳过统计。
pub fn collect(
    root: &Path,
    server: &str,
    since: DateTime<Local>,
    clip_chars: usize,
) -> (Vec<Message>, ParseStats) {
    let mut out = Vec::new();
    let mut stats = ParseStats::default();
    let sessions = root.join("sessions");
    if !sessions.is_dir() {
        return (out, stats);
    }
    for entry in walkdir::WalkDir::new(&sessions)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("rollout-") {
            continue;
        }
        match std::fs::metadata(path) {
            Ok(meta) if super::mtime_after(&meta, since) => {
                let (msgs, s) = parse_rollout_file(path, server, since, clip_chars);
                out.extend(msgs);
                stats.merge(&s);
            }
            _ => {}
        }
    }
    (out, stats)
}

fn parse_rollout_file(
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

    // 第一遍：找 session_meta.payload.cwd。完整 cwd 留作 project_path，basename 作 project 名。
    let cwd_full: Option<String> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|v| {
            let outer = v.get("type").and_then(|t| t.as_str())?;
            if outer != "session_meta" {
                return None;
            }
            // 兼容老版本：payload 可能缺失，直接在顶层带 cwd
            v.get("payload")
                .and_then(|p| p.get("cwd"))
                .or_else(|| v.get("cwd"))
                .and_then(|c| c.as_str())
                .map(|s| s.to_string())
        });
    let project = cwd_full.as_deref().map(path_basename).unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Codex".to_string())
    });

    // 第二遍：按行解析。
    let mut messages = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if serde_json::from_str::<Value>(trimmed).is_err() {
            stats.skipped_lines += 1;
            continue;
        }
        for m in parse_rollout_line(line, server, &project, cwd_full.as_deref(), clip_chars) {
            if m.ts.map_or(true, |ts| ts >= since) {
                messages.push(m);
            }
        }
    }
    // 同 session 内把每条 user 指令配上紧跟的 AI 回复
    super::pair_replies(&mut messages);
    (messages, stats)
}

/// 解析 Codex rollout JSONL 一行。
///
/// 顶层 `type` 只关心 `response_item`，其他（`session_meta` / `event_msg` / `compacted`
/// / `turn_context`）全部丢弃。
pub(crate) fn parse_rollout_line(
    line: &str,
    server: &str,
    project: &str,
    project_path: Option<&str>,
    clip_chars: usize,
) -> Vec<Message> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("解析 Codex rollout 行失败: {e}");
            return Vec::new();
        }
    };

    let ts = v.get("timestamp").and_then(parse_ts);
    let outer = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    if outer != "response_item" {
        return Vec::new();
    }
    let payload = match v.get("payload") {
        Some(p) => p,
        None => return Vec::new(),
    };
    let inner = payload.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match inner {
        "message" => parse_message(payload, server, project, project_path, ts, clip_chars),
        // function_call / function_call_output / reasoning / local_shell_call / web_search_call /
        // image_generation_call / custom_tool_call* / compaction* → 全部丢弃（v0.1）
        _ => Vec::new(),
    }
}

/// Codex `message` payload：`{role, content: [...]}`。
///
/// content block 类型：
/// - `input_text`: 用户指令，全文保留
/// - `output_text`: 助手回复，首尾 clip
/// - `input_image`: 丢弃
fn parse_message(
    payload: &Value,
    server: &str,
    project: &str,
    project_path: Option<&str>,
    ts: Option<DateTime<Local>>,
    clip_chars: usize,
) -> Vec<Message> {
    let role_str = payload.get("role").and_then(|r| r.as_str()).unwrap_or("");
    let arr = match payload.get("content").and_then(|c| c.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut text_parts: Vec<&str> = Vec::new();
    let mut is_user_text = false;
    let mut is_assistant_text = false;
    for block in arr {
        let bt = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let text = block.get("text").and_then(|t| t.as_str());
        match bt {
            "input_text" => {
                if let Some(s) = text {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        text_parts.push(trimmed);
                        is_user_text = true;
                    }
                }
            }
            "output_text" => {
                if let Some(s) = text {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        text_parts.push(trimmed);
                        is_assistant_text = true;
                    }
                }
            }
            // input_image / 其他未知类型 → 丢弃
            _ => {}
        }
    }
    if text_parts.is_empty() {
        return Vec::new();
    }

    let combined = text_parts.join("\n");

    // role 优先看 payload.role，回退到 content block 类型推断
    let role = if role_str == "user" || (role_str.is_empty() && is_user_text && !is_assistant_text)
    {
        Role::User
    } else {
        Role::Assistant
    };

    let text = if role == Role::User {
        combined
    } else {
        clip_text(&combined, clip_chars)
    };

    vec![Message {
        role,
        text,
        ts,
        project: project.to_string(),
        project_path: project_path.map(String::from),
        // user 由 pair_replies 配对填充；assistant 恒 None
        reply: None,
        tool: Tool::Codex,
        server: server.to_string(),
    }]
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    const ROLLOUT: &str = include_str!("fixtures/codex_rollout.jsonl");

    fn parse_lines(text: &str, project: &str) -> Vec<Message> {
        text.lines()
            .flat_map(|l| parse_rollout_line(l, "test-srv", project, None, 200))
            .collect()
    }

    #[test]
    fn session_meta_is_dropped() {
        let line = r#"{"timestamp":"2026-05-17T10:00:00Z","type":"session_meta","payload":{"id":"x","cwd":"/p","timestamp":"2026-05-17T10:00:00Z","originator":"codex","cli_version":"0.50.0"}}"#;
        assert!(parse_rollout_line(line, "srv", "p", None, 200).is_empty());
    }

    #[test]
    fn event_msg_compacted_turn_context_dropped() {
        for outer in ["event_msg", "compacted", "turn_context"] {
            let line = format!(
                r#"{{"timestamp":"2026-05-17T10:00:00Z","type":"{outer}","payload":{{"any":"thing"}}}}"#
            );
            assert!(parse_rollout_line(&line, "srv", "p", None, 200).is_empty());
        }
    }

    #[test]
    fn user_input_text_is_full_prompt() {
        let line = r#"{"timestamp":"2026-05-17T10:01:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"重构这个模块"}]}}"#;
        let msgs = parse_rollout_line(line, "srv", "weekly-report", None, 200);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, Role::User);
        assert_eq!(msgs[0].text, "重构这个模块");
        assert_eq!(msgs[0].tool, Tool::Codex);
        assert_eq!(msgs[0].project, "weekly-report");
    }

    #[test]
    fn assistant_output_text_is_clipped() {
        let long = "a".repeat(800);
        let line = format!(
            r#"{{"timestamp":"2026-05-17T10:02:00Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"{long}"}}]}}}}"#
        );
        let msgs = parse_rollout_line(&line, "srv", "p", None, 200);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, Role::Assistant);
        assert!(msgs[0].text.contains('…'));
    }

    #[test]
    fn input_image_is_dropped_but_text_still_kept() {
        let line = r#"{"timestamp":"2026-05-17T10:03:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_image","image_url":"data:..."},{"type":"input_text","text":"看这张图"}]}}"#;
        let msgs = parse_rollout_line(line, "srv", "p", None, 200);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "看这张图");
    }

    #[test]
    fn function_call_and_reasoning_dropped() {
        for inner_type in ["function_call", "function_call_output", "reasoning"] {
            let line = format!(
                r#"{{"timestamp":"2026-05-17T10:00:00Z","type":"response_item","payload":{{"type":"{inner_type}","name":"x","arguments":"{{}}"}}}}"#
            );
            assert!(parse_rollout_line(&line, "srv", "p", None, 200).is_empty());
        }
    }

    #[test]
    fn broken_or_empty_lines_dropped() {
        assert!(parse_rollout_line("", "srv", "p", None, 200).is_empty());
        assert!(parse_rollout_line("{ not json", "srv", "p", None, 200).is_empty());
    }

    #[test]
    fn missing_payload_dropped() {
        let line = r#"{"timestamp":"...","type":"response_item"}"#;
        assert!(parse_rollout_line(line, "srv", "p", None, 200).is_empty());
    }

    #[test]
    fn empty_content_array_dropped() {
        let line = r#"{"timestamp":"2026-05-17T10:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[]}}"#;
        assert!(parse_rollout_line(line, "srv", "p", None, 200).is_empty());
    }

    #[test]
    fn fixture_parses_message_lines() {
        let msgs = parse_lines(ROLLOUT, "weekly-report");
        let users: Vec<_> = msgs.iter().filter(|m| m.role == Role::User).collect();
        let assts: Vec<_> = msgs.iter().filter(|m| m.role == Role::Assistant).collect();
        assert!(!users.is_empty(), "应有用户指令");
        assert!(!assts.is_empty(), "应有助手回复");
    }

    #[test]
    fn fixture_session_meta_provides_cwd() {
        let project = ROLLOUT
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find_map(|v| {
                if v.get("type").and_then(|t| t.as_str()) != Some("session_meta") {
                    return None;
                }
                v.get("payload")
                    .and_then(|p| p.get("cwd"))
                    .and_then(|c| c.as_str())
                    .map(path_basename)
            });
        assert_eq!(project.as_deref(), Some("weekly-report"));
    }

    #[test]
    fn role_inferred_from_block_type_when_role_missing() {
        // 老版本 Codex 可能不带 role 字段
        let line = r#"{"timestamp":"2026-05-17T10:00:00Z","type":"response_item","payload":{"type":"message","content":[{"type":"input_text","text":"老格式 prompt"}]}}"#;
        let msgs = parse_rollout_line(line, "srv", "p", None, 200);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, Role::User);
    }
}
