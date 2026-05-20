//! SMTP 邮件相关的数据模型。
//!
//! `send()`、`test_smtp()` 与 Markdown→HTML 渲染器由阶段 7 实现，
//! 详见 `docs/ARCHITECTURE.md#38-emailrs`。
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// SMTP 配置（单例，保存到 `smtp.json`）。
///
/// `use_ssl=true` 表示 SSL/TLS（一般端口 465），`false` 表示 STARTTLS（一般端口 587）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
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

/// 一封待发送邮件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EmailRequest {
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    pub subject: String,
    /// 正文 Markdown；发送时由 `email::render_html` 转 HTML。
    pub body_markdown: String,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn smtp_default_empty() {
        let cfg = SmtpConfig::default();
        assert_eq!(cfg.host, "");
        assert_eq!(cfg.port, 0);
        assert!(!cfg.use_ssl);
    }

    #[test]
    fn email_request_round_trip() {
        let req = EmailRequest {
            to: vec!["a@x.com".into(), "b@x.com".into()],
            cc: vec!["c@x.com".into()],
            subject: "周报 2026-W20".into(),
            body_markdown: "# 标题\n- 一\n- 二\n".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: EmailRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }
}
