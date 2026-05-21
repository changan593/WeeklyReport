//! Google Gemini 原生协议：Google AI Studio。
//!
//! 协议细节见 `docs/LLM.md#33-google-gemini-原生`。
//! Gemini 的 API key **走 URL query 参数**而非 header，model 也拼在 path 里。

use crate::i18n;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;

use super::{HttpRequest, LlmProvider};

pub fn build_request(p: &LlmProvider, prompt: &str) -> HttpRequest {
    let base = p.base_url.trim_end_matches('/');
    let url = format!("{base}/v1beta/models/{}:generateContent", p.model);

    let mut headers = HashMap::new();
    headers.insert("Content-Type".into(), "application/json".into());
    for (k, v) in &p.extra_headers {
        headers.insert(k.clone(), v.clone());
    }

    // api_key 走 query 参数；由 reqwest 负责 URL 编码
    let mut query: Vec<(String, String)> = Vec::new();
    if !p.api_key.is_empty() {
        query.push(("key".into(), p.api_key.clone()));
    }

    let mut gen_config = serde_json::json!({
        "maxOutputTokens": p.max_tokens,
    });
    if let Some(t) = p.temperature {
        gen_config["temperature"] = serde_json::json!(t);
    }
    let body = serde_json::json!({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": gen_config,
    });

    HttpRequest {
        url,
        query,
        headers,
        body,
    }
}

pub fn parse_response(body: &Value) -> Result<(String, u32)> {
    let text = body
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!(i18n::t("err.llm.gemini.no_text")))?
        .to_string();
    let tokens = body
        .get("usageMetadata")
        .and_then(|u| u.get("totalTokenCount"))
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
            name: "Gemini".into(),
            kind: LlmKind::Gemini,
            base_url: "https://generativelanguage.googleapis.com".into(),
            api_key: "AIza-xxx".into(),
            model: "gemini-1.5-pro".into(),
            max_tokens: 2048,
            temperature: Some(0.7),
            is_default: false,
            extra_headers: HashMap::new(),
        }
    }

    #[test]
    fn url_includes_model_and_generate_content() {
        let r = build_request(&sample(), "hi");
        assert_eq!(
            r.url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-pro:generateContent"
        );
    }

    #[test]
    fn api_key_goes_to_query_not_headers() {
        let r = build_request(&sample(), "hi");
        assert_eq!(r.query, vec![("key".to_string(), "AIza-xxx".to_string())]);
        assert!(!r.headers.contains_key("Authorization"));
        assert!(!r.headers.contains_key("x-api-key"));
    }

    #[test]
    fn key_query_omitted_when_empty() {
        let mut p = sample();
        p.api_key = String::new();
        let r = build_request(&p, "hi");
        assert!(r.query.is_empty());
    }

    #[test]
    fn body_structure_correct() {
        let r = build_request(&sample(), "你好");
        assert_eq!(r.body["contents"][0]["parts"][0]["text"], "你好");
        assert_eq!(r.body["generationConfig"]["maxOutputTokens"], 2048);
        assert_eq!(r.body["generationConfig"]["temperature"], 0.7);
    }

    #[test]
    fn temperature_omitted_when_none() {
        let mut p = sample();
        p.temperature = None;
        let r = build_request(&p, "hi");
        assert!(r.body["generationConfig"].get("temperature").is_none());
    }

    #[test]
    fn parse_response_typical() {
        let body = serde_json::json!({
            "candidates": [{
                "content": {"parts": [{"text": "Hello"}]}
            }],
            "usageMetadata": {"totalTokenCount": 42}
        });
        let (text, tokens) = parse_response(&body).unwrap();
        assert_eq!(text, "Hello");
        assert_eq!(tokens, 42);
    }

    #[test]
    fn parse_response_missing_metadata_zero_tokens() {
        let body = serde_json::json!({
            "candidates": [{"content": {"parts": [{"text": "Hi"}]}}]
        });
        let (_, tokens) = parse_response(&body).unwrap();
        assert_eq!(tokens, 0);
    }

    #[test]
    fn parse_response_missing_text_errors() {
        let body = serde_json::json!({"candidates": []});
        assert!(parse_response(&body).is_err());
    }
}
