//! Anthropic 原生协议：Claude 官方 API + 兼容 `/v1/messages` 的代理。
//!
//! 协议细节见 `docs/LLM.md#32-anthropic-原生`。

use crate::i18n;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;

use super::{HttpRequest, LlmProvider};

pub fn build_request(p: &LlmProvider, prompt: &str) -> HttpRequest {
    let base = p.base_url.trim_end_matches('/');
    let url = format!("{base}/v1/messages");

    let mut headers = HashMap::new();
    headers.insert("Content-Type".into(), "application/json".into());
    headers.insert("anthropic-version".into(), "2023-06-01".into());
    if !p.api_key.is_empty() {
        headers.insert("x-api-key".into(), p.api_key.clone());
    }
    for (k, v) in &p.extra_headers {
        headers.insert(k.clone(), v.clone());
    }

    let mut body = serde_json::json!({
        "model": p.model,
        "max_tokens": p.max_tokens,
        "messages": [{"role": "user", "content": prompt}],
    });
    if let Some(t) = p.temperature {
        body["temperature"] = serde_json::json!(t);
    }

    HttpRequest {
        url,
        query: Vec::new(),
        headers,
        body,
    }
}

/// Anthropic 的 tokens = input_tokens + output_tokens（usage 字段）。
pub fn parse_response(body: &Value) -> Result<(String, u32)> {
    let text = body
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!(i18n::t("err.llm.anthropic.no_text")))?
        .to_string();
    let usage = body.get("usage");
    let input = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    let output = usage
        .and_then(|u| u.get("output_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    Ok((text, (input + output) as u32))
}

#[cfg(test)]
mod tests {
    use super::super::{LlmKind, LlmProvider};
    use super::*;

    fn sample() -> LlmProvider {
        LlmProvider {
            id: "p".into(),
            name: "Claude".into(),
            kind: LlmKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: "sk-ant-xxx".into(),
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 2048,
            temperature: None,
            is_default: true,
            extra_headers: HashMap::new(),
        }
    }

    #[test]
    fn url_appends_messages_path() {
        let r = build_request(&sample(), "hi");
        assert_eq!(r.url, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn headers_have_anthropic_version() {
        let r = build_request(&sample(), "hi");
        assert_eq!(r.headers["anthropic-version"], "2023-06-01");
    }

    #[test]
    fn headers_use_x_api_key_not_bearer() {
        let r = build_request(&sample(), "hi");
        assert_eq!(r.headers["x-api-key"], "sk-ant-xxx");
        assert!(!r.headers.contains_key("Authorization"));
    }

    #[test]
    fn x_api_key_omitted_when_empty() {
        let mut p = sample();
        p.api_key = String::new();
        let r = build_request(&p, "hi");
        assert!(!r.headers.contains_key("x-api-key"));
    }

    #[test]
    fn body_structure_correct() {
        let r = build_request(&sample(), "你好");
        assert_eq!(r.body["model"], "claude-sonnet-4-20250514");
        assert_eq!(r.body["max_tokens"], 2048);
        assert_eq!(r.body["messages"][0]["role"], "user");
        assert_eq!(r.body["messages"][0]["content"], "你好");
        assert!(r.body.get("temperature").is_none(), "默认 None 不应序列化");
    }

    #[test]
    fn temperature_when_some() {
        let mut p = sample();
        p.temperature = Some(0.5);
        let r = build_request(&p, "hi");
        assert_eq!(r.body["temperature"], 0.5);
    }

    #[test]
    fn parse_response_typical() {
        let body = serde_json::json!({
            "content": [{"type": "text", "text": "Hello"}],
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let (text, tokens) = parse_response(&body).unwrap();
        assert_eq!(text, "Hello");
        assert_eq!(tokens, 15);
    }

    #[test]
    fn parse_response_missing_usage_zero_tokens() {
        let body = serde_json::json!({
            "content": [{"text": "Hi"}]
        });
        let (_, tokens) = parse_response(&body).unwrap();
        assert_eq!(tokens, 0);
    }

    #[test]
    fn parse_response_missing_content_errors() {
        let body = serde_json::json!({});
        assert!(parse_response(&body).is_err());
    }
}
