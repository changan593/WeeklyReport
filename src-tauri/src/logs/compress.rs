//! 文本压缩工具：clip_text 等。
//!
//! 详见 `docs/SPEC.md#3-核心算法token-压缩策略` 与
//! `docs/JSONL.md#8-token-压缩规则汇总与-spec3-对齐`。

/// 把过长文本压缩为「首 N 字 + … + 末 N 字」形式。
///
/// 按 **char**（Unicode 字符）数量计算，不按 byte，避免 UTF-8 中段切断。
/// 当原文长度 ≤ 2N 时原样返回，不插入省略号。
pub fn clip_text(text: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= n * 2 {
        return text.to_string();
    }
    let head: String = chars[..n].iter().collect();
    let tail: String = chars[chars.len() - n..].iter().collect();
    format!("{head}…{tail}")
}

/// 把 path 字符串切到末段（cross-platform：同时认 `/` 与 `\`）。
///
/// 用于 `infer_project()` —— 从 cwd 推断项目名。
pub fn path_basename(p: &str) -> String {
    let trimmed = p.trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    if last.is_empty() {
        p.to_string()
    } else {
        last.to_string()
    }
}

/// 把任意 JSON Value 转为短字符串（用于 tool_use 关键参数显示）。
///
/// - 字符串 → 原样
/// - 数组 → 元素以空格连接（仅取 str 元素）
/// - 其他 → debug 形式
///
/// 总长度按 char 截到 `max`，超过时尾部加 `…`。
pub fn value_to_short_str(v: &serde_json::Value, max: usize) -> String {
    let s = match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    };
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s
    } else {
        let head: String = chars[..max].iter().collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_under_threshold_returns_original() {
        let s = "hello world";
        assert_eq!(clip_text(s, 200), s);
    }

    #[test]
    fn clip_over_threshold_inserts_ellipsis() {
        let s = "a".repeat(500);
        let out = clip_text(&s, 200);
        assert!(out.contains('…'));
        let len = out.chars().count();
        assert_eq!(len, 200 + 1 + 200);
    }

    #[test]
    fn clip_respects_unicode_boundary() {
        let s: String = "中".repeat(500);
        let out = clip_text(&s, 100);
        assert_eq!(out.chars().count(), 100 + 1 + 100);
        // 全是中文字符 + 一个省略号
        assert!(out.chars().all(|c| c == '中' || c == '…'));
    }

    #[test]
    fn clip_n_zero_returns_empty() {
        assert_eq!(clip_text("anything", 0), "");
    }

    #[test]
    fn basename_unix_path() {
        assert_eq!(path_basename("/Users/me/weekly-report"), "weekly-report");
        assert_eq!(path_basename("/Users/me/weekly-report/"), "weekly-report");
    }

    #[test]
    fn basename_windows_path() {
        assert_eq!(path_basename("C:\\dev\\my-app"), "my-app");
        assert_eq!(path_basename("C:\\dev\\my-app\\"), "my-app");
    }

    #[test]
    fn basename_encoded_path() {
        // Claude history.jsonl 中的 project 字段已是 encoded 形式
        assert_eq!(
            path_basename("-Users-me-weekly-report"),
            "-Users-me-weekly-report"
        );
    }

    #[test]
    fn basename_mixed_separators() {
        assert_eq!(path_basename("C:/dev/my-app"), "my-app");
    }

    #[test]
    fn value_to_short_str_for_string() {
        let v = serde_json::json!("src/scheduler.rs");
        assert_eq!(value_to_short_str(&v, 60), "src/scheduler.rs");
    }

    #[test]
    fn value_to_short_str_truncates() {
        let v = serde_json::json!("a".repeat(100));
        let out = value_to_short_str(&v, 10);
        assert_eq!(out.chars().count(), 11); // 10 + …
        assert!(out.ends_with('…'));
    }

    #[test]
    fn value_to_short_str_for_array() {
        let v = serde_json::json!(["ls", "-la", "/tmp"]);
        assert_eq!(value_to_short_str(&v, 60), "ls -la /tmp");
    }
}
