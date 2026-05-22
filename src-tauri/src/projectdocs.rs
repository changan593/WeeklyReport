//! 读取项目根目录的 Markdown 文档，作为周报生成时的「项目背景」。
//!
//! 本机项目直接读文件系统；SSH 项目的读取见 `ssh` 模块。
//! 每个项目所有 md 合并后按 [`MAX_DOC_CHARS`] 截断，避免 README 过长撑爆 prompt。
#![allow(dead_code)]

use std::fs;
use std::path::Path;

/// 单个项目所有 md 合并后的字符上限。
pub const MAX_DOC_CHARS: usize = 3000;

/// 读取本机项目根目录（仅根层，不递归）的所有 `.md` 文件，合并成一段文本。
///
/// 返回 `None` 的情况：目录不存在 / 没有 md 文件 / 全部读取失败。
/// 合并文本总长截断到 [`MAX_DOC_CHARS`]。
pub fn read_local_project_docs(project_path: &str) -> Option<String> {
    let dir = Path::new(project_path);
    if !dir.is_dir() {
        return None;
    }
    let mut files: Vec<(String, String)> = Vec::new();
    for entry in fs::read_dir(dir).ok()?.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_md = path
            .extension()
            .and_then(|s| s.to_str())
            .map_or(false, |ext| ext.eq_ignore_ascii_case("md"));
        if !is_md {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if let Ok(content) = fs::read_to_string(&path) {
            files.push((name, content));
        }
    }
    if files.is_empty() {
        return None;
    }
    // 文件名排序保证输出稳定
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Some(merge_and_clip(&files, MAX_DOC_CHARS))
}

/// 把多个 `(文件名, 内容)` 合并成一段文本，总长（按 char 计）截断到 `max_chars`。
pub fn merge_and_clip(files: &[(String, String)], max_chars: usize) -> String {
    let mut out = String::new();
    for (name, content) in files {
        if out.chars().count() >= max_chars {
            break;
        }
        out.push_str(&format!("### {name}\n"));
        let remaining = max_chars.saturating_sub(out.chars().count());
        let body: String = content.trim().chars().take(remaining).collect();
        out.push_str(&body);
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("wr-docs-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn read_returns_none_for_missing_dir() {
        assert!(read_local_project_docs("/nonexistent/xyz/123").is_none());
    }

    #[test]
    fn read_collects_only_md_files() {
        let dir = temp_dir("collect");
        fs::write(dir.join("README.md"), "# Project\nHello world").unwrap();
        fs::write(dir.join("NOTES.md"), "some notes here").unwrap();
        fs::write(dir.join("ignore.txt"), "not markdown").unwrap();
        let docs = read_local_project_docs(dir.to_str().unwrap()).unwrap();
        assert!(docs.contains("# Project"));
        assert!(docs.contains("some notes here"));
        assert!(!docs.contains("not markdown"));
        assert!(docs.contains("### README.md"));
        assert!(docs.contains("### NOTES.md"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_returns_none_when_no_md() {
        let dir = temp_dir("nomd");
        fs::write(dir.join("a.txt"), "x").unwrap();
        assert!(read_local_project_docs(dir.to_str().unwrap()).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_matches_md_case_insensitively() {
        let dir = temp_dir("case");
        fs::write(dir.join("README.MD"), "uppercase ext").unwrap();
        let docs = read_local_project_docs(dir.to_str().unwrap()).unwrap();
        assert!(docs.contains("uppercase ext"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_and_clip_truncates_to_limit() {
        let files = vec![("big.md".to_string(), "x".repeat(5000))];
        let out = merge_and_clip(&files, 100);
        // header「### big.md\n」约 12 字符 + 截断后的 body，不应远超 limit
        assert!(out.chars().count() <= 130, "实际 {}", out.chars().count());
    }

    #[test]
    fn merge_and_clip_keeps_filename_headers() {
        let files = vec![
            ("CLAUDE.md".to_string(), "claude doc".to_string()),
            ("README.md".to_string(), "readme doc".to_string()),
        ];
        let out = merge_and_clip(&files, 3000);
        assert!(out.contains("### CLAUDE.md"));
        assert!(out.contains("### README.md"));
        assert!(out.contains("claude doc"));
        assert!(out.contains("readme doc"));
    }
}
