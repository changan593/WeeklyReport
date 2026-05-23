//! SMTP 邮件 + 极简 Markdown→HTML 渲染。
//!
//! - 数据模型：`SmtpConfig` 单例 / `EmailRequest` 一次发送
//! - 发送走 `lettre`（smtp + rustls + builder）；不引入 markdown 第三方库
//!   （见 ADR-007）
//! - 详细 HTML 渲染语法：见 `docs/DECISIONS.md#adr-007不写复杂-markdown-渲染器`
#![allow(dead_code)]

use crate::i18n;
use anyhow::{anyhow, Context, Result};
use lettre::message::header::ContentType;
use lettre::message::{Mailbox, Message, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::{AsyncTransport, Tokio1Executor};
use serde::{Deserialize, Serialize};

// ============================================================
// 数据模型
// ============================================================

/// SMTP 配置（单例，保存到 `smtp.json`）。
///
/// `use_ssl = true` → 隐式 TLS（典型端口 465）；`false` → STARTTLS（典型 587）。
///
/// `Debug` 自定义：`password` 永远以 `***` 输出，避免日志意外泄露。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SmtpConfig {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub from_name: String,
    #[serde(default)]
    pub use_ssl: bool,
}

impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &crate::llm::mask_secret(&self.password))
            .field("from_name", &self.from_name)
            .field("use_ssl", &self.use_ssl)
            .finish()
    }
}

/// 一封待发送邮件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EmailRequest {
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    pub subject: String,
    /// 正文 Markdown；发送时由 [`render_html`] 转 HTML，同时附 plain text 副本。
    pub body_markdown: String,
    /// 可选：报告元数据。提供时邮件 HTML 会带顶部统计卡片 + 按项目条形图。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_meta: Option<ReportMeta>,
}

/// 报告元数据，用于美化 HTML 顶部统计区与按项目条形图。
/// 与 `report::ReportRecord` 解耦（避免 email→report 循环依赖），
/// 由调用方按需 `From<&ReportRecord>` 构造（见 `report.rs::ReportMeta::from_record`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReportMeta {
    /// 时间范围，如 "最近 7 天"
    pub week: String,
    /// 项目数
    pub project_count: u32,
    /// 已用 token
    pub tokens_used: u32,
    /// LLM 源显示名（可选）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    /// 生成时间（RFC3339）
    pub generated_at: String,
    /// 按项目工作记录数；为空则不渲染条形图
    #[serde(default)]
    pub project_breakdown: Vec<(String, u32)>,
}

// ============================================================
// 发送
// ============================================================

pub async fn send(cfg: &SmtpConfig, req: &EmailRequest) -> Result<()> {
    if cfg.host.trim().is_empty() {
        return Err(anyhow!(i18n::t("err.email.no_host")));
    }
    if req.to.is_empty() {
        return Err(anyhow!(i18n::t("err.email.no_recipients")));
    }

    let transport = build_transport(cfg)?;
    let from = parse_from(cfg)?;

    let mut builder = Message::builder().from(from).subject(req.subject.clone());
    for to in &req.to {
        builder = builder.to(parse_mailbox(to)?);
    }
    for cc in &req.cc {
        builder = builder.cc(parse_mailbox(cc)?);
    }

    let html = match &req.body_meta {
        Some(meta) => render_html_with_meta(&req.body_markdown, meta),
        None => render_html(&req.body_markdown),
    };
    let plain = req.body_markdown.clone();

    let body = MultiPart::alternative()
        .singlepart(
            SinglePart::builder()
                .header(ContentType::TEXT_PLAIN)
                .body(plain),
        )
        .singlepart(
            SinglePart::builder()
                .header(ContentType::TEXT_HTML)
                .body(html),
        );

    let msg = builder
        .multipart(body)
        .with_context(|| i18n::t("err.email.build_message_failed"))?;
    transport.send(msg).await.map_err(|e| {
        anyhow!(i18n::t_var(
            "err.email.smtp_send_failed",
            &[("err", &e.to_string())]
        ))
    })?;
    Ok(())
}

/// 测试 SMTP 连接（不发送任何邮件）。
pub async fn test_smtp(cfg: &SmtpConfig) -> Result<String> {
    if cfg.host.trim().is_empty() {
        return Err(anyhow!(i18n::t("err.email.no_host")));
    }
    let transport = build_transport(cfg)?;
    let ok = transport.test_connection().await.map_err(|e| {
        anyhow!(i18n::t_var(
            "err.email.smtp_connect_failed",
            &[("err", &e.to_string())],
        ))
    })?;
    if !ok {
        return Err(anyhow!(i18n::t("err.email.smtp_no_greeting")));
    }
    let enc = if cfg.use_ssl { "SSL/TLS" } else { "STARTTLS" };
    let port = cfg.port.to_string();
    Ok(i18n::t_var(
        "msg.smtp_conn_ok",
        &[
            ("host", cfg.host.as_str()),
            ("port", port.as_str()),
            ("enc", enc),
        ],
    ))
}

fn build_transport(cfg: &SmtpConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let builder = if cfg.use_ssl {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)
    }
    .map_err(|e| {
        anyhow!(i18n::t_var(
            "err.email.smtp_transport_failed",
            &[("err", &e.to_string())]
        ))
    })?;
    let creds = Credentials::new(cfg.username.clone(), cfg.password.clone());
    Ok(builder.port(cfg.port).credentials(creds).build())
}

fn parse_from(cfg: &SmtpConfig) -> Result<Mailbox> {
    let s = if cfg.from_name.trim().is_empty() {
        cfg.username.clone()
    } else {
        format!("{} <{}>", cfg.from_name.trim(), cfg.username)
    };
    parse_mailbox(&s)
}

fn parse_mailbox(s: &str) -> Result<Mailbox> {
    s.parse::<Mailbox>().map_err(|e| {
        anyhow!(i18n::t_var(
            "err.email.invalid_address",
            &[("addr", s), ("err", &e.to_string())],
        ))
    })
}

// ============================================================
// Markdown → HTML（极简，详见 ADR-007）
// ============================================================

/// 渲染 Markdown 为完整 HTML（含 <head><style>），适配邮件客户端。
///
/// 支持：`# / ## / ###` 标题、`**bold**`、`` `code` ``、`- / *` 列表、
/// `> quote` 引用、`---` 分隔线、空行→段落。
/// 不支持：链接、图片、表格、嵌套列表、HTML 内嵌。
pub fn render_html(md: &str) -> String {
    let body = render_body(md);
    wrap_html(None, &body)
}

/// 同 [`render_html`]，但在正文上方追加由 `meta` 渲染的统计卡片与按项目条形图。
///
/// 用于 Reports 详情页与定时邮件，让收件人一眼看到「时间范围 / 项目数 / tokens / 生成时间」
/// 以及各项目工作量分布。
pub fn render_html_with_meta(md: &str, meta: &ReportMeta) -> String {
    let body = render_body(md);
    let header = render_meta_header(meta);
    let combined = format!("{header}\n{body}");
    wrap_html(Some(meta), &combined)
}

/// 仅渲染 body 部分（不带 html/head 包裹），便于单元测试。
fn render_body(md: &str) -> String {
    let mut out = String::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut in_list = false;
    let mut in_quote = false;

    for raw in md.lines() {
        let line = raw.trim_end_matches(['\r']);
        let trimmed = line.trim();

        // 空行：刷新所有打开的块
        if trimmed.is_empty() {
            flush_paragraph(&mut out, &mut paragraph);
            close_list(&mut out, &mut in_list);
            close_quote(&mut out, &mut in_quote);
            continue;
        }

        // 分隔线
        if trimmed == "---" {
            flush_paragraph(&mut out, &mut paragraph);
            close_list(&mut out, &mut in_list);
            close_quote(&mut out, &mut in_quote);
            out.push_str("<hr>\n");
            continue;
        }

        // 标题
        if let Some((tag, rest)) = parse_heading(trimmed) {
            flush_paragraph(&mut out, &mut paragraph);
            close_list(&mut out, &mut in_list);
            close_quote(&mut out, &mut in_quote);
            out.push_str(&format!("<{tag}>{}</{tag}>\n", render_inline(rest)));
            continue;
        }

        // 列表
        if let Some(item) = parse_list_item(trimmed) {
            flush_paragraph(&mut out, &mut paragraph);
            close_quote(&mut out, &mut in_quote);
            if !in_list {
                out.push_str("<ul>\n");
                in_list = true;
            }
            out.push_str(&format!("  <li>{}</li>\n", render_inline(item)));
            continue;
        }

        // 引用
        if let Some(rest) = trimmed
            .strip_prefix("> ")
            .or_else(|| trimmed.strip_prefix(">"))
        {
            flush_paragraph(&mut out, &mut paragraph);
            close_list(&mut out, &mut in_list);
            if !in_quote {
                out.push_str("<blockquote>\n");
                in_quote = true;
            }
            out.push_str(&format!("  {}<br>\n", render_inline(rest)));
            continue;
        }

        // 默认：累积到段落
        close_list(&mut out, &mut in_list);
        close_quote(&mut out, &mut in_quote);
        paragraph.push(trimmed.to_string());
    }

    flush_paragraph(&mut out, &mut paragraph);
    close_list(&mut out, &mut in_list);
    close_quote(&mut out, &mut in_quote);
    out
}

fn parse_heading(s: &str) -> Option<(&'static str, &str)> {
    for (prefix, tag) in [("# ", "h1"), ("## ", "h2"), ("### ", "h3")] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return Some((tag, rest));
        }
    }
    None
}

fn parse_list_item(s: &str) -> Option<&str> {
    s.strip_prefix("- ").or_else(|| s.strip_prefix("* "))
}

fn flush_paragraph(out: &mut String, paragraph: &mut Vec<String>) {
    if !paragraph.is_empty() {
        let joined = paragraph.join(" ");
        out.push_str(&format!("<p>{}</p>\n", render_inline(&joined)));
        paragraph.clear();
    }
}

fn close_list(out: &mut String, in_list: &mut bool) {
    if *in_list {
        out.push_str("</ul>\n");
        *in_list = false;
    }
}

fn close_quote(out: &mut String, in_quote: &mut bool) {
    if *in_quote {
        out.push_str("</blockquote>\n");
        *in_quote = false;
    }
}

/// 行内：HTML 转义 → 替换成对 ** → 替换成对 `。
///
/// 不支持嵌套（**`x`** 不会渲染为粗体内嵌代码），刻意保持简单。
fn render_inline(s: &str) -> String {
    let escaped = html_escape(s);
    let with_bold = replace_pair(&escaped, "**", "<strong>", "</strong>");
    replace_pair(&with_bold, "`", "<code>", "</code>")
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// 把成对的 `delim` 替换成 `<open>...</open>`；奇数个 delim 时尾部原样保留。
fn replace_pair(s: &str, delim: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(first) = rest.find(delim) {
        let after = &rest[first + delim.len()..];
        match after.find(delim) {
            Some(rel) => {
                let second_rel_to_rest = first + delim.len() + rel;
                out.push_str(&rest[..first]);
                out.push_str(open);
                out.push_str(&rest[first + delim.len()..second_rel_to_rest]);
                out.push_str(close);
                rest = &rest[second_rel_to_rest + delim.len()..];
            }
            None => {
                // 没有配对的 delim，剩余原样输出
                out.push_str(rest);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 用 inline CSS 包裹 body，适配 Gmail / iOS Mail 等主流客户端。
///
/// 设计目标：
/// - 视觉层次清晰：H1 / H2 都有视觉锚点（H1 渐变下划线 / H2 左色条）
/// - 邮件客户端兼容：避免 flexbox（用 table），所有动态值（如 bar width）走 inline style
/// - 暗色友好：色板基于 stone + emerald 暖灰，不与系统暗色冲突
fn wrap_html(meta: Option<&ReportMeta>, body: &str) -> String {
    let lang = i18n::current_language();
    // 邮件正文最大宽度；统计卡内部用 table 自适应。
    let max_w = if meta.is_some() { 760 } else { 720 };
    format!(
        "<!doctype html>
<html lang=\"{lang}\"><head>
<meta charset=\"utf-8\">
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">
<style>
  body {{ font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", \"PingFang SC\", \"Microsoft YaHei\", \"Helvetica Neue\", sans-serif; color: #1c1917; line-height: 1.65; max-width: {max_w}px; margin: 28px auto; padding: 0 20px; background: #ffffff; -webkit-font-smoothing: antialiased; }}
  h1, h2, h3 {{ color: #1c1917; margin-top: 28px; margin-bottom: 12px; font-weight: 600; letter-spacing: -0.01em; }}
  h1 {{ font-size: 24px; padding-bottom: 10px; border-bottom: 2px solid #1c1917; display: inline-block; }}
  h2 {{ font-size: 18px; padding-left: 12px; border-left: 3px solid #10b981; line-height: 1.4; }}
  h3 {{ font-size: 14.5px; color: #44403c; }}
  p {{ margin: 10px 0; }}
  ul {{ padding-left: 22px; }}
  li {{ margin: 5px 0; }}
  blockquote {{ border-left: 3px solid #a8a29e; color: #57534e; padding: 8px 14px; margin: 12px 0; background: #fafaf9; border-radius: 0 4px 4px 0; }}
  code {{ background: #f5f5f4; padding: 1.5px 6px; border-radius: 4px; font-family: ui-monospace, \"SF Mono\", Menlo, monospace; font-size: 0.88em; color: #1f2937; border: 1px solid #e7e5e4; }}
  hr {{ border: none; border-top: 1px solid #e7e5e4; margin: 20px 0; }}
  strong {{ font-weight: 600; color: #0c0a09; }}
  .wr-meta {{ margin: 0 0 28px; background: linear-gradient(135deg, #fafaf9 0%, #f5f5f4 100%); border: 1px solid #e7e5e4; border-radius: 12px; padding: 18px 20px; }}
  .wr-meta-title {{ font-size: 12px; text-transform: uppercase; letter-spacing: 0.1em; color: #78716c; margin: 0 0 12px; font-weight: 600; }}
  .wr-meta-week {{ font-size: 19px; font-weight: 600; color: #0c0a09; margin: 0 0 14px; letter-spacing: -0.01em; }}
  .wr-stats {{ width: 100%; border-collapse: separate; border-spacing: 8px 0; table-layout: fixed; }}
  .wr-stat {{ background: #ffffff; border: 1px solid #e7e5e4; border-radius: 8px; padding: 12px 14px; vertical-align: top; }}
  .wr-stat-label {{ font-size: 11px; color: #78716c; text-transform: uppercase; letter-spacing: 0.06em; margin: 0 0 4px; font-weight: 600; }}
  .wr-stat-value {{ font-size: 18px; font-weight: 600; color: #0c0a09; margin: 0; letter-spacing: -0.01em; }}
  .wr-stat-suffix {{ font-size: 11.5px; color: #78716c; font-weight: 400; margin-left: 3px; }}
  .wr-chart {{ margin: 0 0 28px; background: #ffffff; border: 1px solid #e7e5e4; border-radius: 12px; padding: 16px 18px; }}
  .wr-chart-title {{ font-size: 12px; text-transform: uppercase; letter-spacing: 0.1em; color: #78716c; margin: 0 0 12px; font-weight: 600; }}
  .wr-chart-table {{ width: 100%; border-collapse: collapse; }}
  .wr-chart-row td {{ padding: 5px 0; vertical-align: middle; }}
  .wr-chart-name {{ font-size: 13px; color: #292524; padding-right: 12px; max-width: 200px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
  .wr-chart-bar-track {{ width: 100%; height: 8px; background: #f5f5f4; border-radius: 4px; overflow: hidden; }}
  .wr-chart-bar-fill {{ height: 8px; background: linear-gradient(90deg, #10b981 0%, #059669 100%); border-radius: 4px; }}
  .wr-chart-count {{ font-size: 12px; color: #57534e; font-variant-numeric: tabular-nums; text-align: right; padding-left: 12px; min-width: 36px; }}
  @media (max-width: 540px) {{
    .wr-stats {{ border-spacing: 0; }}
    .wr-stat {{ display: block; margin-bottom: 8px; }}
    .wr-chart-name {{ max-width: 120px; }}
  }}
</style></head><body>
{body}</body></html>"
    )
}

/// 渲染顶部统计卡片 + （若有）按项目工作量条形图。
fn render_meta_header(meta: &ReportMeta) -> String {
    let mut out = String::new();
    out.push_str("<div class=\"wr-meta\">\n");
    out.push_str("  <div class=\"wr-meta-title\">");
    out.push_str(&html_escape(&i18n::t("email.html.meta_title")));
    out.push_str("</div>\n");
    out.push_str("  <div class=\"wr-meta-week\">");
    out.push_str(&html_escape(&meta.week));
    out.push_str("</div>\n");

    // 三~四个统计 tile：项目数 / tokens / LLM / 生成时间
    out.push_str("  <table class=\"wr-stats\" role=\"presentation\" cellspacing=\"0\" cellpadding=\"0\"><tr>\n");

    out.push_str("    <td class=\"wr-stat\">\n");
    out.push_str("      <div class=\"wr-stat-label\">");
    out.push_str(&html_escape(&i18n::t("email.html.stat.projects")));
    out.push_str("</div>\n");
    out.push_str(&format!(
        "      <div class=\"wr-stat-value\">{}</div>\n",
        meta.project_count
    ));
    out.push_str("    </td>\n");

    out.push_str("    <td class=\"wr-stat\">\n");
    out.push_str("      <div class=\"wr-stat-label\">");
    out.push_str(&html_escape(&i18n::t("email.html.stat.tokens")));
    out.push_str("</div>\n");
    out.push_str(&format!(
        "      <div class=\"wr-stat-value\">{}</div>\n",
        format_compact(meta.tokens_used as u64)
    ));
    out.push_str("    </td>\n");

    if let Some(name) = &meta.provider_name {
        if !name.trim().is_empty() {
            out.push_str("    <td class=\"wr-stat\">\n");
            out.push_str("      <div class=\"wr-stat-label\">");
            out.push_str(&html_escape(&i18n::t("email.html.stat.llm")));
            out.push_str("</div>\n");
            out.push_str(&format!(
                "      <div class=\"wr-stat-value\" style=\"font-size:14px;\">{}</div>\n",
                html_escape(name)
            ));
            out.push_str("    </td>\n");
        }
    }

    let date_display = format_generated_at(&meta.generated_at);
    out.push_str("    <td class=\"wr-stat\">\n");
    out.push_str("      <div class=\"wr-stat-label\">");
    out.push_str(&html_escape(&i18n::t("email.html.stat.generated")));
    out.push_str("</div>\n");
    out.push_str(&format!(
        "      <div class=\"wr-stat-value\" style=\"font-size:14px;\">{}</div>\n",
        html_escape(&date_display)
    ));
    out.push_str("    </td>\n");

    out.push_str("  </tr></table>\n");
    out.push_str("</div>\n");

    // 项目分布条形图：取 Top 8，按数量降序
    if !meta.project_breakdown.is_empty() {
        let mut rows: Vec<(String, u32)> = meta.project_breakdown.clone();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        let max_val = rows.iter().map(|(_, v)| *v).max().unwrap_or(1).max(1);
        let limit = rows.len().min(8);
        let rows = &rows[..limit];

        out.push_str("<div class=\"wr-chart\">\n");
        out.push_str("  <div class=\"wr-chart-title\">");
        out.push_str(&html_escape(&i18n::t("email.html.chart_title")));
        out.push_str("</div>\n");
        out.push_str("  <table class=\"wr-chart-table\" role=\"presentation\" cellspacing=\"0\" cellpadding=\"0\">\n");
        for (name, count) in rows {
            let pct = (*count as f64 / max_val as f64 * 100.0).round() as u32;
            // 至少给一个可见的最小宽度
            let pct = pct.max(4);
            out.push_str("    <tr class=\"wr-chart-row\">\n");
            out.push_str(&format!(
                "      <td class=\"wr-chart-name\" style=\"width: 32%;\">{}</td>\n",
                html_escape(name)
            ));
            out.push_str("      <td>\n");
            out.push_str(&format!(
                "        <div class=\"wr-chart-bar-track\"><div class=\"wr-chart-bar-fill\" style=\"width: {pct}%;\"></div></div>\n"
            ));
            out.push_str("      </td>\n");
            out.push_str(&format!(
                "      <td class=\"wr-chart-count\" style=\"width: 56px;\">{count}</td>\n"
            ));
            out.push_str("    </tr>\n");
        }
        out.push_str("  </table>\n");
        out.push_str("</div>\n");
    }

    out
}

/// 把大数字压成 "1.2k / 3.4M" 形式，节省统计卡空间。
fn format_compact(n: u64) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    if n < 1_000_000 {
        let v = n as f64 / 1_000.0;
        return format!("{v:.1}k");
    }
    let v = n as f64 / 1_000_000.0;
    format!("{v:.1}M")
}

/// 把 RFC3339 时间裁成 "YYYY-MM-DD HH:MM"；解析失败时原样返回前 16 字符。
fn format_generated_at(s: &str) -> String {
    use chrono::DateTime;
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return dt.format("%Y-%m-%d %H:%M").to_string();
    }
    s.chars().take(16).collect()
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------- 数据模型 round-trip --------

    #[test]
    fn smtp_round_trip() {
        let cfg = SmtpConfig {
            host: "smtp.example.com".into(),
            port: 465,
            username: "u@example.com".into(),
            password: "secret".into(),
            from_name: "Me".into(),
            use_ssl: true,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: SmtpConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn email_request_round_trip() {
        let req = EmailRequest {
            to: vec!["a@x.com".into()],
            cc: vec!["c@x.com".into()],
            subject: "x".into(),
            body_markdown: "# h\n- a\n".into(),
            body_meta: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: EmailRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn email_request_round_trip_with_meta() {
        let req = EmailRequest {
            to: vec!["a@x.com".into()],
            cc: vec![],
            subject: "x".into(),
            body_markdown: "# h\n".into(),
            body_meta: Some(ReportMeta {
                week: "最近 7 天".into(),
                project_count: 3,
                tokens_used: 12345,
                provider_name: Some("DeepSeek".into()),
                generated_at: "2026-05-23T10:00:00+08:00".into(),
                project_breakdown: vec![("a".into(), 5), ("b".into(), 3)],
            }),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: EmailRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    // -------- 内联 --------

    #[test]
    fn html_escape_special() {
        assert_eq!(
            html_escape("<a> & \"b\" 'c'"),
            "&lt;a&gt; &amp; &quot;b&quot; &#39;c&#39;"
        );
    }

    #[test]
    fn inline_bold_paired() {
        assert_eq!(
            render_inline("hello **world** done"),
            "hello <strong>world</strong> done"
        );
    }

    #[test]
    fn inline_bold_unpaired_left_alone() {
        // 奇数个 ** 不应产生跨行错误，保留原样
        assert_eq!(render_inline("** stray"), "** stray");
    }

    #[test]
    fn inline_code_paired() {
        assert_eq!(
            render_inline("run `cargo test` ok"),
            "run <code>cargo test</code> ok"
        );
    }

    #[test]
    fn inline_escapes_html_inside_bold() {
        let r = render_inline("**<script>**");
        assert!(r.contains("&lt;script&gt;"));
        assert!(r.contains("<strong>"));
    }

    // -------- 块级 --------

    #[test]
    fn renders_headings() {
        let html = render_body("# H1\n## H2\n### H3\n");
        assert!(html.contains("<h1>H1</h1>"));
        assert!(html.contains("<h2>H2</h2>"));
        assert!(html.contains("<h3>H3</h3>"));
    }

    #[test]
    fn renders_unordered_list() {
        let html = render_body("- one\n- two\n- three\n");
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>one</li>"));
        assert!(html.contains("<li>two</li>"));
        assert!(html.contains("</ul>"));
    }

    #[test]
    fn renders_star_list_item() {
        let html = render_body("* hello\n* world\n");
        assert!(html.contains("<li>hello</li>"));
    }

    #[test]
    fn renders_blockquote() {
        let html = render_body("> quote me\n> still quoted\n");
        assert!(html.contains("<blockquote>"));
        assert!(html.contains("quote me"));
        assert!(html.contains("</blockquote>"));
    }

    #[test]
    fn renders_hr() {
        let html = render_body("a\n\n---\n\nb\n");
        assert!(html.contains("<hr>"));
    }

    #[test]
    fn empty_line_splits_paragraph() {
        let html = render_body("para 1\nstill para 1\n\npara 2\n");
        let count = html.matches("<p>").count();
        assert_eq!(count, 2, "应有两个 <p> 段落，实际 HTML：{html}");
    }

    #[test]
    fn list_then_paragraph_closes_ul() {
        let html = render_body("- a\n- b\n\nparagraph\n");
        // </ul> 应在 paragraph 之前
        let ul_close = html.find("</ul>").unwrap();
        let p_open = html.find("<p>").unwrap();
        assert!(ul_close < p_open);
    }

    #[test]
    fn full_html_has_doctype_and_style() {
        let html = render_html("# Hi\n\nbody");
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<style>"));
        assert!(html.contains("<h1>Hi</h1>"));
        assert!(html.contains("</body></html>"));
    }

    // -------- replace_pair 单元 --------

    #[test]
    fn replace_pair_basic() {
        assert_eq!(replace_pair("a**b**c", "**", "<s>", "</s>"), "a<s>b</s>c");
    }

    #[test]
    fn replace_pair_multiple_pairs() {
        assert_eq!(
            replace_pair("**x** and **y**", "**", "<s>", "</s>"),
            "<s>x</s> and <s>y</s>"
        );
    }

    #[test]
    fn replace_pair_single_delim_kept() {
        assert_eq!(replace_pair("a**b", "**", "<s>", "</s>"), "a**b");
    }

    // -------- send 输入校验（不发起网络） --------

    #[tokio::test]
    async fn send_rejects_empty_host() {
        let cfg = SmtpConfig::default();
        let req = EmailRequest {
            to: vec!["a@b.com".into()],
            subject: "x".into(),
            body_markdown: "x".into(),
            ..Default::default()
        };
        let err = send(&cfg, &req).await.unwrap_err().to_string();
        assert!(err.contains("host"));
    }

    // -------- ReportMeta / render_html_with_meta --------

    #[test]
    fn format_compact_works() {
        assert_eq!(format_compact(0), "0");
        assert_eq!(format_compact(999), "999");
        assert_eq!(format_compact(1_000), "1.0k");
        assert_eq!(format_compact(1_234), "1.2k");
        assert_eq!(format_compact(12_345), "12.3k");
        assert_eq!(format_compact(1_500_000), "1.5M");
    }

    #[test]
    fn format_generated_at_iso8601() {
        let r = format_generated_at("2026-05-23T10:30:00+08:00");
        assert_eq!(r, "2026-05-23 10:30");
    }

    #[test]
    fn format_generated_at_invalid_fallbacks_to_prefix() {
        let r = format_generated_at("not a date");
        assert_eq!(r, "not a date");
    }

    #[test]
    fn render_meta_header_includes_stats() {
        let m = ReportMeta {
            week: "最近 7 天".into(),
            project_count: 3,
            tokens_used: 12_345,
            provider_name: Some("DeepSeek".into()),
            generated_at: "2026-05-23T10:00:00+08:00".into(),
            project_breakdown: vec![],
        };
        let html = render_meta_header(&m);
        assert!(html.contains("最近 7 天"));
        assert!(html.contains(">3<")); // project_count
        assert!(html.contains("12.3k")); // tokens
        assert!(html.contains("DeepSeek"));
        assert!(html.contains("2026-05-23 10:00"));
        // 无 breakdown 时不应渲染图表
        assert!(!html.contains("wr-chart"));
    }

    #[test]
    fn render_meta_header_includes_chart_when_breakdown_present() {
        let m = ReportMeta {
            week: "x".into(),
            project_count: 2,
            tokens_used: 1000,
            provider_name: None,
            generated_at: "2026-01-01T00:00:00+00:00".into(),
            project_breakdown: vec![("alpha".into(), 10), ("beta".into(), 3)],
        };
        let html = render_meta_header(&m);
        assert!(html.contains("wr-chart"), "应渲染图表块");
        assert!(html.contains(">alpha<"));
        assert!(html.contains(">beta<"));
        // alpha 是最大值，应该是 100%
        assert!(html.contains("width: 100%"));
    }

    #[test]
    fn render_meta_header_xss_safe() {
        let m = ReportMeta {
            week: "<script>alert(1)</script>".into(),
            project_count: 1,
            tokens_used: 0,
            provider_name: Some("<img>".into()),
            generated_at: "x".into(),
            project_breakdown: vec![("<b>".into(), 1)],
        };
        let html = render_meta_header(&m);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&lt;img&gt;"));
        assert!(html.contains("&lt;b&gt;"));
    }

    #[test]
    fn full_html_with_meta_has_meta_block_and_body() {
        let m = ReportMeta {
            week: "最近 7 天".into(),
            project_count: 2,
            tokens_used: 500,
            provider_name: Some("Local".into()),
            generated_at: "2026-05-23T10:00:00+08:00".into(),
            project_breakdown: vec![("p1".into(), 4), ("p2".into(), 2)],
        };
        let html = render_html_with_meta("# 周报\n\n正文。", &m);
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("wr-meta"));
        assert!(html.contains("wr-chart"));
        assert!(html.contains("<h1>周报</h1>"));
        // meta 块应在 h1 之前
        let meta_pos = html.find("wr-meta").unwrap();
        let h1_pos = html.find("<h1>").unwrap();
        assert!(
            meta_pos < h1_pos,
            "meta block should appear before report body"
        );
    }

    #[test]
    fn full_html_without_meta_unchanged() {
        let html = render_html("# Hi\n\nbody");
        assert!(html.starts_with("<!doctype html>"));
        // CSS 类名一定在 <style> 里；这里检查实际渲染的 div 是否存在。
        assert!(!html.contains("<div class=\"wr-meta\">"));
        assert!(!html.contains("<div class=\"wr-chart\">"));
        assert!(html.contains("<h1>Hi</h1>"));
    }

    /// 手动验证用：`cargo test --bin weekly-report -- --ignored dump_sample_html --nocapture`
    /// 会把示例 HTML 写到 /tmp/sample-meta.html，便于人工预览（浏览器打开看效果）。
    #[test]
    #[ignore]
    fn dump_sample_html() {
        let meta = ReportMeta {
            week: "最近 7 天 (2026-05-17 → 2026-05-23)".into(),
            project_count: 4,
            tokens_used: 18_426,
            provider_name: Some("DeepSeek (deepseek-chat)".into()),
            generated_at: "2026-05-23T18:30:00+08:00".into(),
            project_breakdown: vec![
                ("weekly-report".into(), 42),
                ("dashboard-v2".into(), 28),
                ("infra/terraform".into(), 17),
                ("docs-site".into(), 9),
            ],
        };
        let md = "# 本周周报\n\n## weekly-report\n\n本周完成了 HTML 邮件渲染的美化与统计卡片，新增按项目工作量条形图。重构 `render_html_with_meta`，与原 `render_html` 并存以保持向后兼容。\n\n- 新增 `ReportMeta` 数据结构传递元数据\n- CSS 调整为更现代化的视觉风格（左色条 H2、卡片背景）\n- 邮件正文兼容 Gmail / iOS Mail（table-layout 排版）\n\n## dashboard-v2\n\n推进了**前端状态徽标**的设计与实现，覆盖工作区与 LLM 源主页。每张卡片右上角现在能一眼看到 `已配置 / 测试中 / 可用 / 失败` 等状态。\n\n> 设计原则：状态信息要可读，但不抢眼。\n\n## infra/terraform\n\n升级 `provider aws` 到 5.x，跑通 plan/apply。新增 staging 环境的 IAM 角色定义。\n\n## docs-site\n\n补齐 SSH 配置文档，覆盖 Windows / macOS / Linux 三平台。\n\n## 下周计划\n\n- 完成定时任务的失败重试与系统通知\n- 实验 API key keyring 存储（替换明文）\n";
        let html = render_html_with_meta(md, &meta);
        std::fs::write("/tmp/sample-meta.html", &html).unwrap();
        eprintln!(
            "Sample HTML written to /tmp/sample-meta.html ({} bytes)",
            html.len()
        );
    }

    #[tokio::test]
    async fn send_rejects_empty_to() {
        let cfg = SmtpConfig {
            host: "smtp.example.com".into(),
            ..Default::default()
        };
        let req = EmailRequest::default();
        let err = send(&cfg, &req).await.unwrap_err().to_string();
        assert!(err.contains("收件人"));
    }

    #[tokio::test]
    async fn test_smtp_rejects_empty_host() {
        let cfg = SmtpConfig::default();
        let err = test_smtp(&cfg).await.unwrap_err().to_string();
        assert!(err.contains("host"));
    }

    #[test]
    fn parse_from_with_name() {
        let cfg = SmtpConfig {
            username: "u@example.com".into(),
            from_name: "我".into(),
            ..Default::default()
        };
        let mb = parse_from(&cfg).unwrap();
        assert_eq!(mb.email.to_string(), "u@example.com");
    }

    #[test]
    fn parse_from_empty_name() {
        let cfg = SmtpConfig {
            username: "u@example.com".into(),
            from_name: "  ".into(),
            ..Default::default()
        };
        let mb = parse_from(&cfg).unwrap();
        assert_eq!(mb.email.to_string(), "u@example.com");
    }
}
