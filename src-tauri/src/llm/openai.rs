//! OpenAI 兼容协议：OpenAI、DeepSeek、OpenRouter、Kimi、Qwen、Groq、Ollama、vLLM、LM Studio。
//!
//! 协议细节见 `docs/LLM.md#31-openai-兼容`。

use crate::i18n;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;

use super::{HttpRequest, LlmProvider};

/// 构造请求体 + headers + URL。**不发起网络**，便于单测断言。
pub fn build_request(p: &LlmProvider, prompt: &str) -> HttpRequest {
    let base = p.base_url.trim_end_matches('/');
    let url = format!("{base}/v1/chat/completions");

    let mut headers = HashMap::new();
    headers.insert("Content-Type".into(), "application/json".into());
    if !p.api_key.is_empty() {
        headers.insert("Authorization".into(), format!("Bearer {}", p.api_key));
    }
    for (k, v) in &p.extra_headers {
        headers.insert(k.clone(), v.clone());
    }

    let mut body = serde_json::json!({
        "model": p.model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": p.max_tokens,
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

/// 从响应体解析出 (text, total_tokens)。
pub fn parse_response(body: &Value) -> Result<(String, u32)> {
    let text = body
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!(i18n::t("err.llm.openai.no_content")))?
        .to_string();
    let tokens = body
        .get("usage")
        .and_then(|u| u.get("total_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0) as u32;
    Ok((text, tokens))
}

#[cfg(test)]
mod tests {
    use super::super::{LlmKind, LlmProvider};
    use super::*;

    fn sample() -> LlmProvider {
        LlmProvider {
            id: "p".into(),
            name: "OpenAI".into(),
            kind: LlmKind::OpenAiCompatible,
            base_url: "https://api.openai.com".into(),
            api_key: "sk-xxx".into(),
            model: "gpt-4o-mini".into(),
            max_tokens: 2048,
            temperature: Some(0.7),
            is_default: false,
            extra_headers: HashMap::new(),
        }
    }

    #[test]
    fn url_appends_chat_completions_path() {
        let r = build_request(&sample(), "hi");
        assert_eq!(r.url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn url_strips_trailing_slash() {
        let mut p = sample();
        p.base_url = "https://api.openai.com/".into();
        let r = build_request(&p, "hi");
        assert_eq!(r.url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn authorization_header_set_when_key_present() {
        let r = build_request(&sample(), "hi");
        assert_eq!(r.headers["Authorization"], "Bearer sk-xxx");
    }

    #[test]
    fn authorization_header_omitted_when_key_empty() {
        let mut p = sample();
        p.api_key = String::new();
        let r = build_request(&p, "hi");
        assert!(!r.headers.contains_key("Authorization"));
    }

    #[test]
    fn extra_headers_merged() {
        let mut p = sample();
        p.extra_headers
            .insert("HTTP-Referer".into(), "https://example.com".into());
        let r = build_request(&p, "hi");
        assert_eq!(r.headers["HTTP-Referer"], "https://example.com");
    }

    #[test]
    fn body_includes_model_messages_max_tokens() {
        let r = build_request(&sample(), "你好");
        assert_eq!(r.body["model"], "gpt-4o-mini");
        assert_eq!(r.body["max_tokens"], 2048);
        assert_eq!(r.body["messages"][0]["role"], "user");
        assert_eq!(r.body["messages"][0]["content"], "你好");
    }

    #[test]
    fn temperature_omitted_when_none() {
        let mut p = sample();
        p.temperature = None;
        let r = build_request(&p, "hi");
        assert!(r.body.get("temperature").is_none());
    }

    #[test]
    fn temperature_included_when_some() {
        let r = build_request(&sample(), "hi");
        assert_eq!(r.body["temperature"], 0.7);
    }

    #[test]
    fn parse_response_typical() {
        let body = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "Hi"}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });
        let (text, tokens) = parse_response(&body).unwrap();
        assert_eq!(text, "Hi");
        assert_eq!(tokens, 15);
    }

    #[test]
    fn parse_response_missing_usage_returns_zero_tokens() {
        let body = serde_json::json!({
            "choices": [{"message": {"content": "Hi"}}]
        });
        let (_, tokens) = parse_response(&body).unwrap();
        assert_eq!(tokens, 0);
    }

    #[test]
    fn parse_response_missing_content_errors() {
        let body = serde_json::json!({"choices": []});
        assert!(parse_response(&body).is_err());
    }
}
