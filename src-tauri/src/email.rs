//! SMTP 邮件 + 极简 Markdown→HTML 渲染。
//!
//! - 数据模型：`SmtpConfig` 单例 / `EmailRequest` 一次发送
//! - 发送走 `lettre`（smtp + rustls + builder）；不引入 markdown 第三方库
//!   （见 ADR-007）
//! - 详细 HTML 渲染语法：见 `docs/DECISIONS.md#adr-007不写复杂-markdown-渲染器`
#![allow(dead_code)]

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
}

// ============================================================
// 发送
// ============================================================

pub async fn send(cfg: &SmtpConfig, req: &EmailRequest) -> Result<()> {
    if cfg.host.trim().is_empty() {
        return Err(anyhow!("SMTP host 未配置"));
    }
    if req.to.is_empty() {
        return Err(anyhow!("收件人列表为空"));
    }
    // 校验主题不含 CRLF（兜底；lettre 也会拒绝，但我们提供更友好的错误）
    crate::validate::mail_header_text(&req.subject)?;
    // 校验所有收件人/抄送地址格式
    for addr in req.to.iter().chain(req.cc.iter()) {
        crate::validate::email(addr)?;
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

    let html = render_html(&req.body_markdown);
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

    let msg = builder.multipart(body).context("构造邮件失败")?;
    transport
        .send(msg)
        .await
        .map_err(|e| anyhow!("SMTP 发送失败：{e}"))?;
    Ok(())
}

/// 测试 SMTP 连接（不发送任何邮件）。
pub async fn test_smtp(cfg: &SmtpConfig) -> Result<String> {
    if cfg.host.trim().is_empty() {
        return Err(anyhow!("SMTP host 未配置"));
    }
    let transport = build_transport(cfg)?;
    let ok = transport.test_connection().await.map_err(|e| {
        anyhow!("SMTP 连接失败：{e}\n常见原因：密码/授权码错误、被防火墙拦截、端口与加密方式不匹配")
    })?;
    if !ok {
        return Err(anyhow!("SMTP 服务器未响应有效问候"));
    }
    Ok(format!(
        "✓ SMTP 连接成功：{}:{}（{}）",
        cfg.host,
        cfg.port,
        if cfg.use_ssl { "SSL/TLS" } else { "STARTTLS" }
    ))
}

fn build_transport(cfg: &SmtpConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let builder = if cfg.use_ssl {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)
    }
    .map_err(|e| anyhow!("构造 SMTP 传输失败：{e}"))?;
    let creds = Credentials::new(cfg.username.clone(), cfg.password.clone());
    Ok(builder.port(cfg.port).credentials(creds).build())
}

fn parse_from(cfg: &SmtpConfig) -> Result<Mailbox> {
    // from_name 走 mail_header 校验；防止 CRLF 注入到 From: 头
    let trimmed = cfg.from_name.trim();
    if !trimmed.is_empty() {
        crate::validate::mail_header_text(trimmed)?;
    }
    crate::validate::email(&cfg.username)?;
    let s = if trimmed.is_empty() {
        cfg.username.clone()
    } else {
        format!("{} <{}>", trimmed, cfg.username)
    };
    parse_mailbox(&s)
}

fn parse_mailbox(s: &str) -> Result<Mailbox> {
    s.parse::<Mailbox>()
        .map_err(|e| anyhow!("无效邮箱地址 \"{s}\"：{e}"))
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
    wrap_html(&body)
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

/// 用极简 inline CSS 包裹 body，适配 Gmail / iOS Mail 等主流客户端。
fn wrap_html(body: &str) -> String {
    format!(
        "<!doctype html>
<html lang=\"zh-CN\"><head>
<meta charset=\"utf-8\">
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">
<style>
  body {{ font-family: -apple-system, \"PingFang SC\", \"Microsoft YaHei\", sans-serif; color: #1c1917; line-height: 1.6; max-width: 720px; margin: 24px auto; padding: 0 16px; background: #ffffff; }}
  h1, h2, h3 {{ color: #1c1917; margin-top: 24px; }}
  h1 {{ font-size: 22px; }}
  h2 {{ font-size: 18px; border-bottom: 1px solid #e7e5e4; padding-bottom: 6px; }}
  h3 {{ font-size: 15px; }}
  p {{ margin: 8px 0; }}
  ul {{ padding-left: 22px; }}
  li {{ margin: 4px 0; }}
  blockquote {{ border-left: 3px solid #d6d3d1; color: #57534e; padding: 4px 12px; margin: 8px 0; background: #fafaf9; }}
  code {{ background: #f5f5f4; padding: 1px 5px; border-radius: 3px; font-family: ui-monospace, \"SF Mono\", Menlo, monospace; font-size: 0.92em; color: #44403c; }}
  hr {{ border: none; border-top: 1px solid #e7e5e4; margin: 16px 0; }}
  strong {{ font-weight: 600; }}
</style></head><body>
{body}</body></html>"
    )
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
