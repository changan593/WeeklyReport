//! 项目背景文档扫描器。
//!
//! 作为周报生成的「项目背景」喂给 LLM，帮助理解项目用途、技术栈、领域术语。
//!
//! ## 扫描范围
//!
//! - 项目根目录 `<root>/*.md`
//! - `doc / docs / Doc / Docs / DOC / DOCS` 子目录下：再深 2 层的 `*.md`
//!   即 `<root>/docs/*.md` 和 `<root>/docs/<sub>/*.md`
//!
//! 子目录名大小写不敏感；`.md` 扩展名同样大小写不敏感。
//!
//! ## 强/弱信号分类（由调用方传入 `since` 决定）
//!
//! - **强信号**（完整内容进 prompt）：
//!   - README* / CLAUDE* / CHANGELOG* / ROADMAP* 等核心文档（无视 mtime）
//!   - 其它 md 且 `mtime ≥ since`（本期内修改过）
//! - **弱信号**（仅文件名 + 首行标题 + mtime 进 prompt）：
//!   - 其它 md 且 `mtime < since`（本期未修改，仅作领域词汇背景）
//!
//! ## prompt 输出格式
//!
//! 合并后的文本带显式分区标记，便于 LLM 区分对待：
//!
//! ```text
//! ## 强信号文档（本期相关 / 核心说明）
//! ### README.md
//! <完整内容>
//!
//! ### docs/architecture.md  (修改于 2026-05-22)
//! <完整内容>
//!
//! ## 弱信号文档（仅作背景，本期未更新）
//! - docs/api-v1.md  — # API Reference v1  (修改于 2025-09-10)
//! - docs/old-design.md  — # 旧版架构  (修改于 2025-06-15)
//! ```
#![allow(dead_code)]

use chrono::{DateTime, Local};
use std::fs;
use std::path::Path;
use std::time::SystemTime;
use walkdir::WalkDir;

/// 单个强信号文档进 prompt 时的字符上限（按 char 计）。
pub const MAX_DOC_CHARS_PER_FILE: usize = 2000;
/// 一个项目所有强信号文档合并后的字符上限。
pub const MAX_TOTAL_CHARS_PER_PROJECT: usize = 6000;

/// doc 子目录名（小写后比较，故大小写不敏感）。
const DOC_SUBDIR_NAMES: &[&str] = &["doc", "docs"];

/// 文件名（不含扩展名）前缀匹配则永远算强信号。比较时统一大写。
const ALWAYS_STRONG_PREFIXES: &[&str] = &["README", "CLAUDE", "CHANGELOG", "ROADMAP"];

/// 单个被扫到的 md 文档。
///
/// 调用方通常不直接拿这个结构，而是用 [`read_local_project_docs`] 拿合并后的字符串。
/// 暴露出来主要为了单元测试和给 SSH 路径复用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocFile {
    /// 相对项目根的路径（用 `/` 分隔，跨平台一致），如 `"README.md"` 或 `"docs/architecture.md"`
    pub rel_path: String,
    /// 文件正文（已 trim）
    pub content: String,
    /// 修改时间；元数据读取失败时为 `None`
    pub mtime: Option<DateTime<Local>>,
    /// 强信号 = 完整内容进 prompt；弱信号 = 仅文件名+标题进 prompt
    pub strength: DocStrength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocStrength {
    Strong,
    Weak,
}

// ============================================================
// 公共入口
// ============================================================

/// 读取本机项目的所有相关 md 文档，按强/弱分区合并成一段 prompt 用的文本。
///
/// 返回 `None`：目录不存在 / 没有 md 文件 / 全部读取失败。
///
/// `since` 用来决定弱信号：非核心文档且 `mtime < since` 算弱。
pub fn read_local_project_docs(project_path: &str, since: DateTime<Local>) -> Option<String> {
    let files = scan_local_project_docs(project_path, since);
    if files.is_empty() {
        return None;
    }
    let composed = compose_for_prompt(&files);
    if composed.trim().is_empty() {
        None
    } else {
        Some(composed)
    }
}

/// 扫描项目目录下所有符合范围的 md 文件，返回 [`DocFile`] 列表。
///
/// 扫描范围见模块文档。失败的单个文件 `warn!` 后跳过。
pub fn scan_local_project_docs(project_path: &str, since: DateTime<Local>) -> Vec<DocFile> {
    let root = Path::new(project_path);
    if !root.is_dir() {
        return Vec::new();
    }
    let mut out: Vec<DocFile> = Vec::new();
    // max_depth=3：根=0，根下文件=1，子目录文件=2，再深一层=3
    for entry in WalkDir::new(root)
        .max_depth(3)
        .into_iter()
        .filter_entry(|e| keep_walk_entry(root, e))
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if !is_md_path(entry.path()) {
            continue;
        }
        let depth = entry.depth();
        if !path_in_scope(root, entry.path(), depth) {
            continue;
        }
        let rel = match rel_path(root, entry.path()) {
            Some(p) => p,
            None => continue,
        };
        let (content, mtime) = match read_with_mtime(entry.path()) {
            Some(t) => t,
            None => {
                tracing::warn!("读项目文档失败：{}", entry.path().display());
                continue;
            }
        };
        let strength = classify_strength(&rel, mtime, since);
        out.push(DocFile {
            rel_path: rel,
            content,
            mtime,
            strength,
        });
    }
    // 稳定排序：强信号优先；同强弱内 README* 类优先；再按路径
    out.sort_by(|a, b| {
        let ka = (
            strength_order(a.strength),
            !is_always_strong(&a.rel_path),
            a.rel_path.clone(),
        );
        let kb = (
            strength_order(b.strength),
            !is_always_strong(&b.rel_path),
            b.rel_path.clone(),
        );
        ka.cmp(&kb)
    });
    out
}

/// 把 [`DocFile`] 列表合并成 prompt 用的字符串。
///
/// 强信号区：每个文件 `### path (mtime)\n<完整内容截断>\n\n`，总字符截到
/// [`MAX_TOTAL_CHARS_PER_PROJECT`]。
/// 弱信号区：每个文件一行 `- path — <首行标题> (mtime)`。
pub fn compose_for_prompt(files: &[DocFile]) -> String {
    let mut strong = String::new();
    let mut weak_lines: Vec<String> = Vec::new();
    let mut strong_used = 0usize;

    for f in files {
        match f.strength {
            DocStrength::Strong => {
                if strong_used >= MAX_TOTAL_CHARS_PER_PROJECT {
                    // 强信号溢出 → 降级为弱信号一行
                    weak_lines.push(format_weak_line(f));
                    continue;
                }
                let header = format!("### {}{}\n", f.rel_path, format_mtime_suffix(f.mtime));
                strong.push_str(&header);
                strong_used += header.chars().count();
                let remaining_total = MAX_TOTAL_CHARS_PER_PROJECT.saturating_sub(strong_used);
                let allow = remaining_total.min(MAX_DOC_CHARS_PER_FILE);
                let body: String = f.content.trim().chars().take(allow).collect();
                strong.push_str(&body);
                strong_used += body.chars().count();
                strong.push_str("\n\n");
                strong_used += 2;
            }
            DocStrength::Weak => weak_lines.push(format_weak_line(f)),
        }
    }

    let mut out = String::new();
    if !strong.trim().is_empty() {
        out.push_str("## 强信号文档（本期相关 / 核心说明）\n");
        out.push_str(strong.trim_end());
        out.push('\n');
    }
    if !weak_lines.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("## 弱信号文档（本期未更新，仅供领域词汇背景，不要据此推断本期成果）\n");
        for l in weak_lines {
            out.push_str(&l);
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

// ============================================================
// 内部辅助
// ============================================================

fn keep_walk_entry(root: &Path, entry: &walkdir::DirEntry) -> bool {
    let depth = entry.depth();
    if depth == 0 {
        return true; // 根本身
    }
    if entry.file_type().is_file() {
        return true; // 文件级别留给主循环判断
    }
    // 目录：只进 doc/docs 一级子目录，然后里面再深一层任意子目录
    if depth == 1 {
        return entry
            .file_name()
            .to_str()
            .map(is_doc_subdir)
            .unwrap_or(false);
    }
    if depth == 2 {
        // doc/<sub> 这一层都进，让 max_depth=3 把里面的 md 文件捞出来
        let parent_is_doc = entry
            .path()
            .parent()
            .and_then(|p| p.strip_prefix(root).ok())
            .and_then(|rel| rel.iter().next())
            .and_then(|c| c.to_str())
            .map(is_doc_subdir)
            .unwrap_or(false);
        return parent_is_doc;
    }
    false
}

fn path_in_scope(root: &Path, path: &Path, depth: usize) -> bool {
    if depth == 1 {
        return true; // 根目录直接 md
    }
    // depth=2 → 父目录必须是 doc/docs；depth=3 → 祖父必须是 doc/docs
    let rel = match path.strip_prefix(root) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let parts: Vec<_> = rel.iter().filter_map(|c| c.to_str()).collect();
    if depth == 2 {
        return parts.len() == 2 && is_doc_subdir(parts[0]);
    }
    if depth == 3 {
        return parts.len() == 3 && is_doc_subdir(parts[0]);
    }
    false
}

fn is_md_path(p: &Path) -> bool {
    p.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn is_doc_subdir(name: &str) -> bool {
    let lower = name.to_lowercase();
    DOC_SUBDIR_NAMES.contains(&lower.as_str())
}

fn rel_path(root: &Path, p: &Path) -> Option<String> {
    let r = p.strip_prefix(root).ok()?;
    // 统一用 '/'，跨平台一致
    let s = r
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/");
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn read_with_mtime(p: &Path) -> Option<(String, Option<DateTime<Local>>)> {
    let content = fs::read_to_string(p).ok()?;
    let mtime = fs::metadata(p)
        .ok()
        .and_then(|m| systime_to_local(m.modified().ok()));
    Some((content, mtime))
}

fn systime_to_local(t: Option<SystemTime>) -> Option<DateTime<Local>> {
    t.map(|st| {
        let dt: DateTime<Local> = st.into();
        dt
    })
}

fn classify_strength(
    rel_path: &str,
    mtime: Option<DateTime<Local>>,
    since: DateTime<Local>,
) -> DocStrength {
    if is_always_strong(rel_path) {
        return DocStrength::Strong;
    }
    match mtime {
        Some(t) if t >= since => DocStrength::Strong,
        Some(_) => DocStrength::Weak,
        // mtime 读不到时保守视为强，避免漏掉关键文档
        None => DocStrength::Strong,
    }
}

fn is_always_strong(rel_path: &str) -> bool {
    let stem = Path::new(rel_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let upper = stem.to_uppercase();
    ALWAYS_STRONG_PREFIXES.iter().any(|p| upper.starts_with(p))
}

fn strength_order(s: DocStrength) -> u8 {
    match s {
        DocStrength::Strong => 0,
        DocStrength::Weak => 1,
    }
}

fn format_mtime_suffix(mtime: Option<DateTime<Local>>) -> String {
    mtime
        .map(|t| format!("  (修改于 {})", t.format("%Y-%m-%d")))
        .unwrap_or_default()
}

/// 弱信号一行：文件名 + 首行标题 + mtime。
fn format_weak_line(f: &DocFile) -> String {
    let title = first_heading(&f.content).unwrap_or_else(|| "(无标题)".to_string());
    format!(
        "- {} — {}{}",
        f.rel_path,
        title,
        format_mtime_suffix(f.mtime)
    )
}

/// 从 md 内容里取首个 `#`/`##`/`###` 标题，若无则取首行非空文本截 50 字符。
fn first_heading(md: &str) -> Option<String> {
    for line in md.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(rest) = t.strip_prefix('#') {
            let stripped = rest.trim_start_matches('#').trim();
            if !stripped.is_empty() {
                return Some(truncate_chars(stripped, 80));
            }
        }
        return Some(truncate_chars(t, 80));
    }
    None
}

fn truncate_chars(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let head: String = chars[..max].iter().collect();
        format!("{head}…")
    }
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("wr-docs-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn now() -> DateTime<Local> {
        Local::now()
    }

    #[test]
    fn returns_none_for_missing_dir() {
        assert!(read_local_project_docs("/nonexistent/xyz/123", now()).is_none());
    }

    #[test]
    fn scans_root_md_files() {
        let dir = temp_dir("root");
        fs::write(dir.join("README.md"), "# Project\nHello world").unwrap();
        fs::write(dir.join("NOTES.md"), "some notes here").unwrap();
        fs::write(dir.join("ignore.txt"), "not markdown").unwrap();
        let files = scan_local_project_docs(dir.to_str().unwrap(), now() - Duration::days(7));
        let paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        assert!(
            paths.contains(&"README.md"),
            "应扫到 README.md，实际: {paths:?}"
        );
        assert!(paths.contains(&"NOTES.md"));
        assert!(!paths.contains(&"ignore.txt"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scans_doc_subdir_one_level() {
        let dir = temp_dir("doc1");
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs/arch.md"), "# Architecture").unwrap();
        let files = scan_local_project_docs(dir.to_str().unwrap(), now() - Duration::days(7));
        let paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        assert!(paths.contains(&"docs/arch.md"), "实际: {paths:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scans_doc_subdir_two_levels() {
        let dir = temp_dir("doc2");
        fs::create_dir_all(dir.join("docs/api")).unwrap();
        fs::write(dir.join("docs/api/spec.md"), "# API Spec").unwrap();
        let files = scan_local_project_docs(dir.to_str().unwrap(), now() - Duration::days(7));
        let paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        assert!(
            paths.contains(&"docs/api/spec.md"),
            "应扫到深 2 层的 md，实际: {paths:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn does_not_recurse_beyond_depth_3() {
        let dir = temp_dir("deep");
        fs::create_dir_all(dir.join("docs/api/v2")).unwrap();
        fs::write(dir.join("docs/api/v2/spec.md"), "# Too deep").unwrap();
        let files = scan_local_project_docs(dir.to_str().unwrap(), now() - Duration::days(7));
        let paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        assert!(
            !paths.iter().any(|p| p.contains("v2/spec.md")),
            "不应扫到深 3+ 层，实际: {paths:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_doc_subdir_matches_all_case_variants() {
        // FS 层面：Windows NTFS 大小写不敏感，6 个变体会合并成 2 个目录，
        // 所以单测函数本身覆盖所有大小写，FS 测试只验证两个 base name。
        for name in &["doc", "Doc", "DOC", "docs", "Docs", "DOCS"] {
            assert!(is_doc_subdir(name), "{name} 应被识别为 doc 子目录");
        }
        // 非 doc 子目录
        for name in &["src", "tests", "examples", "documents", "documentation"] {
            assert!(!is_doc_subdir(name), "{name} 不该被识别");
        }
    }

    #[test]
    fn doc_and_docs_subdirs_both_scanned() {
        let dir = temp_dir("doc-and-docs");
        for name in &["doc", "docs"] {
            let sub = dir.join(name);
            fs::create_dir_all(&sub).unwrap();
            fs::write(sub.join("x.md"), format!("# from {name}")).unwrap();
        }
        let files = scan_local_project_docs(dir.to_str().unwrap(), now() - Duration::days(7));
        let paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.starts_with("doc/")),
            "doc/ 下应扫到，实际: {paths:?}"
        );
        assert!(
            paths.iter().any(|p| p.starts_with("docs/")),
            "docs/ 下应扫到，实际: {paths:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ignores_non_doc_subdirs() {
        let dir = temp_dir("nondoc");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("tests")).unwrap();
        fs::write(dir.join("src/main.md"), "# code doc").unwrap();
        fs::write(dir.join("tests/readme.md"), "# tests").unwrap();
        fs::write(dir.join("README.md"), "# root").unwrap();
        let files = scan_local_project_docs(dir.to_str().unwrap(), now() - Duration::days(7));
        let paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["README.md"],
            "只应扫根目录 + doc(s)，不该进 src/tests"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn md_extension_case_insensitive() {
        let dir = temp_dir("ext");
        fs::write(dir.join("README.MD"), "uppercase ext").unwrap();
        let files = scan_local_project_docs(dir.to_str().unwrap(), now() - Duration::days(7));
        assert_eq!(files.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn readme_always_strong_regardless_of_mtime() {
        let dir = temp_dir("readme-strong");
        let readme = dir.join("README.md");
        fs::write(&readme, "# old readme").unwrap();
        // 模拟 README 修改时间在 since 之前（很久以前的项目说明）
        // 这里不实际改 mtime，依赖 ALWAYS_STRONG 前缀规则
        let files = scan_local_project_docs(dir.to_str().unwrap(), now() + Duration::days(1));
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0].strength,
            DocStrength::Strong,
            "README* 永远是强信号，无视 mtime"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_doc_weak_when_mtime_before_since() {
        let dir = temp_dir("weak");
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs/old.md"), "# Old design").unwrap();
        // since = 未来某天 → 文件的 mtime（现在）必然 < since
        let since = now() + Duration::days(7);
        let files = scan_local_project_docs(dir.to_str().unwrap(), since);
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0].strength,
            DocStrength::Weak,
            "非 README 且 mtime < since → 弱信号"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_doc_strong_when_mtime_after_since() {
        let dir = temp_dir("strong");
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs/new.md"), "# New design").unwrap();
        let since = now() - Duration::days(7);
        let files = scan_local_project_docs(dir.to_str().unwrap(), since);
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0].strength,
            DocStrength::Strong,
            "非 README 但 mtime ≥ since → 强信号"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compose_renders_strong_and_weak_sections() {
        let strong = DocFile {
            rel_path: "README.md".into(),
            content: "# Project\nDoes thing X".into(),
            mtime: None,
            strength: DocStrength::Strong,
        };
        let weak = DocFile {
            rel_path: "docs/old.md".into(),
            content: "# Old design\nDeprecated".into(),
            mtime: Some(Local::now() - Duration::days(180)),
            strength: DocStrength::Weak,
        };
        let out = compose_for_prompt(&[strong, weak]);
        assert!(out.contains("## 强信号文档"));
        assert!(out.contains("### README.md"));
        assert!(out.contains("Does thing X"));
        assert!(out.contains("## 弱信号文档"));
        assert!(out.contains("- docs/old.md"));
        assert!(out.contains("Old design"));
    }

    #[test]
    fn compose_omits_section_when_empty() {
        let only_strong = vec![DocFile {
            rel_path: "README.md".into(),
            content: "X".into(),
            mtime: None,
            strength: DocStrength::Strong,
        }];
        let out = compose_for_prompt(&only_strong);
        assert!(out.contains("强信号"));
        assert!(!out.contains("弱信号"), "无弱信号时不应输出该分区");

        let only_weak = vec![DocFile {
            rel_path: "docs/x.md".into(),
            content: "# X".into(),
            mtime: None,
            strength: DocStrength::Weak,
        }];
        let out = compose_for_prompt(&only_weak);
        assert!(!out.contains("## 强信号"), "无强信号时不应输出该分区");
        assert!(out.contains("弱信号"));
    }

    #[test]
    fn compose_strong_total_clipped() {
        // 4 个 2000 字符的强信号文件，总和应被截到 MAX_TOTAL_CHARS_PER_PROJECT=6000
        let files: Vec<DocFile> = (0..4)
            .map(|i| DocFile {
                rel_path: format!("doc{i}.md"),
                content: "x".repeat(MAX_DOC_CHARS_PER_FILE),
                mtime: None,
                strength: DocStrength::Strong,
            })
            .collect();
        let out = compose_for_prompt(&files);
        assert!(
            out.chars().count() <= MAX_TOTAL_CHARS_PER_PROJECT + 500,
            "总长应受控（含分区标题/弱信号兜底），实际 {}",
            out.chars().count()
        );
    }

    #[test]
    fn first_heading_extracts_h1() {
        assert_eq!(first_heading("# Title\nbody"), Some("Title".into()));
        assert_eq!(first_heading("\n\n## Sub\nbody"), Some("Sub".into()));
        assert_eq!(
            first_heading("not a heading\n# later"),
            Some("not a heading".into())
        );
        assert_eq!(first_heading(""), None);
        assert_eq!(first_heading("\n\n"), None);
    }

    #[test]
    fn read_local_returns_composed_text() {
        let dir = temp_dir("e2e");
        fs::write(dir.join("README.md"), "# Project\nDescription").unwrap();
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs/arch.md"), "# Architecture").unwrap();
        let out = read_local_project_docs(dir.to_str().unwrap(), now() - Duration::days(7))
            .expect("应返回合并文本");
        assert!(out.contains("README.md"));
        assert!(out.contains("Project"));
        assert!(out.contains("docs/arch.md"));
        let _ = fs::remove_dir_all(&dir);
    }

    // -------- 向后兼容：保留对旧 API 名的引用，以防其它模块尚未迁移 --------

    // ---------- 联通真实项目目录的手动测试（默认 ignored） ----------
    //
    // 用法（用户本地，PowerShell）：
    //   $env:WR_PROJECT="C:\Work\some-real-project"
    //   $env:WR_DAYS="7"
    //   cargo test --bin weekly-report -- --ignored projectdocs::tests::live_scan_dump
    //
    // 会把扫到的所有 md 列出来 + 合并后的最终 prompt 文本打到 stdout。
    // 用来验证：doc/docs 子目录扫到了 / mtime 拿到了 / 强弱分类符合预期。
    #[test]
    #[ignore]
    fn live_scan_dump() {
        let path = match std::env::var("WR_PROJECT") {
            Ok(p) => p,
            Err(_) => {
                eprintln!("跳过：未设置 WR_PROJECT 环境变量");
                return;
            }
        };
        let days: i64 = std::env::var("WR_DAYS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(7);
        let since = Local::now() - Duration::days(days);
        println!("\n=== 扫描 {path}（since = 最近 {days} 天）===\n");

        let files = scan_local_project_docs(&path, since);
        if files.is_empty() {
            println!("(未扫到任何 md 文件)");
            return;
        }
        println!("扫到 {} 个 md 文件：\n", files.len());
        for f in &files {
            let mt = f
                .mtime
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "?".into());
            let strength = match f.strength {
                DocStrength::Strong => "STRONG",
                DocStrength::Weak => "WEAK  ",
            };
            println!(
                "  [{strength}] {:<40} mtime={mt}  ({} 字符)",
                f.rel_path,
                f.content.chars().count()
            );
        }

        println!("\n=== 合并后的 prompt 文本 ===\n");
        println!("{}", compose_for_prompt(&files));
        println!("\n=== 末尾 ===\n");
    }

    #[test]
    fn weak_format_includes_mtime() {
        let f = DocFile {
            rel_path: "docs/api.md".into(),
            content: "# API".into(),
            mtime: Some(Local::now() - Duration::days(100)),
            strength: DocStrength::Weak,
        };
        let line = format_weak_line(&f);
        assert!(line.contains("docs/api.md"));
        assert!(line.contains("API"));
        assert!(line.contains("修改于"));
    }
}
