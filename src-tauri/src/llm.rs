//! LLM 源协议抽象 + 调用入口。
//!
//! 三类协议（OpenAI 兼容 / Anthropic / Gemini）的实现分别在子模块。
//! 设计目标与字段定义详见 `docs/LLM.md`。
//!
//! 为便于单元测试，把"构造请求"和"解析响应"做成纯函数（不接触网络），
//! HTTP 发送由 [`post_json`] 这一层薄壳子负责。
#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

pub mod anthropic;
pub mod gemini;
pub mod openai;

// ============================================================
// 数据模型
// ============================================================

/// LLM API 协议类型。
///
/// JSON 形式： `"OpenAiCompatible" | "Anthropic" | "Gemini"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LlmKind {
    #[default]
    OpenAiCompatible,
    Anthropic,
    Gemini,
}

/// 用户配置的一个 LLM 源。字段语义见 `docs/LLM.md#2-数据模型`。
///
/// `Debug` 自定义实现：`api_key` 永远以 `***` 输出，避免 `tracing::warn!("{:?}", p)`
/// 这类日志意外把密钥喷到 stderr / 文件日志 / 崩溃报告里。
#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LlmProvider {
    pub id: String,
    pub name: String,
    pub kind: LlmKind,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra_headers: HashMap<String, String>,
}

impl std::fmt::Debug for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmProvider")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("api_key", &mask_secret(&self.api_key))
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("temperature", &self.temperature)
            .field("is_default", &self.is_default)
            .field("extra_headers", &self.extra_headers)
            .finish()
    }
}

/// 把密钥脱敏为 `<empty>` 或 `sk-xx***5 chars`。
/// 短密钥（<8）只露前 1 后 1；长的露前 4 后 2。
pub(crate) fn mask_secret(s: &str) -> String {
    let n = s.chars().count();
    if n == 0 {
        return "<empty>".into();
    }
    if n <= 8 {
        let first: String = s.chars().take(1).collect();
        let last: String = s.chars().rev().take(1).collect();
        return format!("{first}***{last}");
    }
    let first: String = s.chars().take(4).collect();
    let last: String = s.chars().skip(n.saturating_sub(2)).collect();
    format!("{first}***{last}")
}

/// LLM 调用返回。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompletionResult {
    pub text: String,
    pub tokens_used: u32,
    pub duration_ms: u64,
}

/// 协议中立的 HTTP 请求规格。
///
/// 子模块只产出 `HttpRequest`，不发起网络；网络层由 [`post_json`] 统一处理。
/// 这样请求构造逻辑可以做纯函数单测。
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub url: String,
    pub query: Vec<(String, String)>,
    pub headers: HashMap<String, String>,
    pub body: Value,
}

const HTTP_TIMEOUT: Duration = Duration::from_secs(120);
/// 单次 LLM 响应允许的最大字节数。
///
/// 防御恶意 / 故障 LLM 返回 GB 级响应导致 OOM。10 MB 对于周报生成完全够用
/// （LLM 输出 token 上限通常 8K–32K，对应 ~100 KB 文本）。
const HTTP_MAX_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

// ============================================================
// 入口
// ============================================================

/// 主入口：根据 provider.kind 分发到对应协议。
///
/// 失败时错误信息包含 HTTP 状态码与响应 body 前 500 字符，便于排查。
pub async fn complete(provider: &LlmProvider, prompt: &str) -> Result<CompletionResult> {
    let req = build_request(provider, prompt);
    let started = std::time::Instant::now();
    let (status, body_text) = post_json(&req).await?;
    let duration_ms = started.elapsed().as_millis() as u64;

    if !(200..300).contains(&status) {
        return Err(anyhow!(
            "LLM API 返回 {}: {}",
            status,
            truncate_chars(&body_text, 500)
        ));
    }

    let body: Value = serde_json::from_str(&body_text)
        .with_context(|| format!("响应 JSON 解析失败: {}", truncate_chars(&body_text, 500)))?;
    let (text, tokens) = parse_response(provider.kind, &body)?;
    Ok(CompletionResult {
        text,
        tokens_used: tokens,
        duration_ms,
    })
}

/// 测试连接：发一个极短 prompt 验证认证与连通性。
pub async fn test_connection(provider: &LlmProvider) -> Result<String> {
    let r = complete(provider, "回复一个 'OK' 即可，不要其他内容").await?;
    let preview: String = r.text.chars().take(50).collect();
    Ok(format!(
        "✓ 连接成功（耗时 {}ms, 用 {} tokens）\n模型回复: {}",
        r.duration_ms, r.tokens_used, preview
    ))
}

fn build_request(provider: &LlmProvider, prompt: &str) -> HttpRequest {
    match provider.kind {
        LlmKind::OpenAiCompatible => openai::build_request(provider, prompt),
        LlmKind::Anthropic => anthropic::build_request(provider, prompt),
        LlmKind::Gemini => gemini::build_request(provider, prompt),
    }
}

fn parse_response(kind: LlmKind, body: &Value) -> Result<(String, u32)> {
    match kind {
        LlmKind::OpenAiCompatible => openai::parse_response(body),
        LlmKind::Anthropic => anthropic::parse_response(body),
        LlmKind::Gemini => gemini::parse_response(body),
    }
}

async fn post_json(req: &HttpRequest) -> Result<(u16, String)> {
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .context("HTTP client 初始化失败")?;

    let mut builder = client.post(&req.url);
    if !req.query.is_empty() {
        builder = builder.query(&req.query);
    }
    for (k, v) in &req.headers {
        builder = builder.header(k, v);
    }
    builder = builder.json(&req.body);

    let response = builder
        .send()
        .await
        .with_context(|| format!("HTTP 请求失败: {}", req.url))?;
    let status = response.status().as_u16();

    // Content-Length 早拒：如果响应头声明的长度就超限，直接报错，连 body 都不读。
    if let Some(len) = response.content_length() {
        if len > HTTP_MAX_RESPONSE_BYTES {
            return Err(anyhow!(
                "LLM 响应过大（声明 {len} 字节，上限 {HTTP_MAX_RESPONSE_BYTES}）。\
                 可能是端点配置错误或被劫持。"
            ));
        }
    }

    // 流式读取（用 reqwest 自带的 `chunk()`，避免新增 futures-util 直接依赖）
    // 累计到上限立即中断，保护内存。
    let mut response = response;
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    while let Some(chunk) = response.chunk().await.context("读取响应体失败")? {
        if (buf.len() as u64) + (chunk.len() as u64) > HTTP_MAX_RESPONSE_BYTES {
            return Err(anyhow!(
                "LLM 响应超过 {HTTP_MAX_RESPONSE_BYTES} 字节上限（已读 {} 字节）",
                buf.len()
            ));
        }
        buf.extend_from_slice(&chunk);
    }
    let body = String::from_utf8(buf).context("响应不是合法 UTF-8")?;
    Ok((status, body))
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
// 预设
// ============================================================

/// 返回 10 个预设。详见 `docs/LLM.md#5-预设清单`。
///
/// 顺序固定。UI 会以此渲染"快速选择"按钮组。
pub fn presets() -> Vec<(&'static str, LlmProvider)> {
    fn p(
        name: &'static str,
        kind: LlmKind,
        base_url: &str,
        model: &str,
        api_key: &str,
        temperature: Option<f64>,
        is_default: bool,
    ) -> (&'static str, LlmProvider) {
        (
            name,
            LlmProvider {
                id: String::new(),
                name: name.to_string(),
                kind,
                base_url: base_url.to_string(),
                api_key: api_key.to_string(),
                model: model.to_string(),
                max_tokens: 2048,
                temperature,
                is_default,
                extra_headers: HashMap::new(),
            },
        )
    }

    vec![
        p(
            "Anthropic Claude",
            LlmKind::Anthropic,
            "https://api.anthropic.com",
            "claude-sonnet-4-20250514",
            "",
            None,
            true,
        ),
        p(
            "OpenAI GPT",
            LlmKind::OpenAiCompatible,
            "https://api.openai.com",
            "gpt-4o-mini",
            "",
            Some(0.7),
            false,
        ),
        p(
            "DeepSeek",
            LlmKind::OpenAiCompatible,
            "https://api.deepseek.com",
            "deepseek-chat",
            "",
            Some(0.7),
            false,
        ),
        p(
            "OpenRouter",
            LlmKind::OpenAiCompatible,
            "https://openrouter.ai/api",
            "anthropic/claude-sonnet-4",
            "",
            Some(0.7),
            false,
        ),
        p(
            "Kimi (月之暗面)",
            LlmKind::OpenAiCompatible,
            "https://api.moonshot.cn",
            "moonshot-v1-8k",
            "",
            Some(0.7),
            false,
        ),
        p(
            "通义千问 (Qwen)",
            LlmKind::OpenAiCompatible,
            "https://dashscope.aliyuncs.com/compatible-mode",
            "qwen-plus",
            "",
            Some(0.7),
            false,
        ),
        p(
            "Gemini",
            LlmKind::Gemini,
            "https://generativelanguage.googleapis.com",
            "gemini-1.5-pro",
            "",
            Some(0.7),
            false,
        ),
        p(
            "本地 Ollama",
            LlmKind::OpenAiCompatible,
            "http://localhost:11434",
            "qwen2.5:7b",
            "ollama",
            Some(0.7),
            false,
        ),
        p(
            "本地 vLLM / LM Studio",
            LlmKind::OpenAiCompatible,
            "http://localhost:8000",
            "Qwen/Qwen2.5-7B-Instruct",
            "EMPTY",
            Some(0.7),
            false,
        ),
        p(
            "自定义 (OpenAI 兼容)",
            LlmKind::OpenAiCompatible,
            "",
            "",
            "",
            Some(0.7),
            false,
        ),
    ]
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_kind_serialization() {
        assert_eq!(
            serde_json::to_string(&LlmKind::OpenAiCompatible).unwrap(),
            "\"OpenAiCompatible\""
        );
        assert_eq!(
            serde_json::to_string(&LlmKind::Anthropic).unwrap(),
            "\"Anthropic\""
        );
        assert_eq!(
            serde_json::to_string(&LlmKind::Gemini).unwrap(),
            "\"Gemini\""
        );
    }

    #[test]
    fn provider_round_trip_with_headers() {
        let mut headers = HashMap::new();
        headers.insert("HTTP-Referer".into(), "https://example.com".into());
        let p = LlmProvider {
            id: "p1".into(),
            name: "OpenRouter".into(),
            kind: LlmKind::OpenAiCompatible,
            base_url: "https://openrouter.ai/api".into(),
            api_key: "sk-xx".into(),
            model: "anthropic/claude-sonnet-4".into(),
            max_tokens: 2048,
            temperature: Some(0.7),
            is_default: true,
            extra_headers: headers,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: LlmProvider = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn provider_with_none_temperature_omits_field() {
        let p = LlmProvider {
            id: "p1".into(),
            name: "Claude".into(),
            kind: LlmKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: String::new(),
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 2048,
            temperature: None,
            is_default: true,
            extra_headers: HashMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("temperature").is_none());
    }

    // -------- presets --------

    #[test]
    fn presets_returns_ten_in_documented_order() {
        let list = presets();
        assert_eq!(list.len(), 10);
        let names: Vec<&str> = list.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec![
                "Anthropic Claude",
                "OpenAI GPT",
                "DeepSeek",
                "OpenRouter",
                "Kimi (月之暗面)",
                "通义千问 (Qwen)",
                "Gemini",
                "本地 Ollama",
                "本地 vLLM / LM Studio",
                "自定义 (OpenAI 兼容)",
            ]
        );
    }

    #[test]
    fn presets_anthropic_is_default_and_temperature_none() {
        let list = presets();
        let (_, claude) = &list[0];
        assert!(claude.is_default);
        assert_eq!(claude.temperature, None);
        assert_eq!(claude.kind, LlmKind::Anthropic);
    }

    #[test]
    fn presets_others_not_default() {
        let list = presets();
        for (_, p) in list.iter().skip(1) {
            assert!(!p.is_default);
        }
    }

    #[test]
    fn presets_local_providers_have_placeholder_keys() {
        let list = presets();
        let by_name: HashMap<&str, &LlmProvider> = list.iter().map(|(n, p)| (*n, p)).collect();
        assert_eq!(by_name["本地 Ollama"].api_key, "ollama");
        assert_eq!(by_name["本地 vLLM / LM Studio"].api_key, "EMPTY");
    }

    #[test]
    fn presets_max_tokens_2048() {
        for (_, p) in presets() {
            assert_eq!(p.max_tokens, 2048);
        }
    }

    // -------- 顶层 build_request / parse_response 分发 --------

    #[test]
    fn build_request_dispatches_by_kind() {
        let mut p = LlmProvider {
            id: "p".into(),
            name: "x".into(),
            kind: LlmKind::OpenAiCompatible,
            base_url: "https://example.com".into(),
            api_key: "k".into(),
            model: "m".into(),
            max_tokens: 100,
            temperature: None,
            is_default: false,
            extra_headers: HashMap::new(),
        };
        let r = build_request(&p, "hi");
        assert!(r.url.ends_with("/v1/chat/completions"));

        p.kind = LlmKind::Anthropic;
        let r = build_request(&p, "hi");
        assert!(r.url.ends_with("/v1/messages"));

        p.kind = LlmKind::Gemini;
        let r = build_request(&p, "hi");
        assert!(r.url.contains(":generateContent"));
    }

    #[test]
    fn parse_response_dispatches_by_kind() {
        let openai_body = serde_json::json!({
            "choices":[{"message":{"content":"o"}}],
            "usage":{"total_tokens":1}
        });
        assert_eq!(
            parse_response(LlmKind::OpenAiCompatible, &openai_body).unwrap(),
            ("o".into(), 1)
        );
        let anthropic_body = serde_json::json!({
            "content":[{"text":"a"}],
            "usage":{"input_tokens":1,"output_tokens":2}
        });
        assert_eq!(
            parse_response(LlmKind::Anthropic, &anthropic_body).unwrap(),
            ("a".into(), 3)
        );
        let gemini_body = serde_json::json!({
            "candidates":[{"content":{"parts":[{"text":"g"}]}}],
            "usageMetadata":{"totalTokenCount":7}
        });
        assert_eq!(
            parse_response(LlmKind::Gemini, &gemini_body).unwrap(),
            ("g".into(), 7)
        );
    }

    #[test]
    fn debug_masks_api_key() {
        let p = LlmProvider {
            id: "p1".into(),
            name: "x".into(),
            kind: LlmKind::OpenAiCompatible,
            base_url: "https://api.openai.com".into(),
            api_key: "sk-secret-abcd1234".into(),
            model: "gpt-4o-mini".into(),
            max_tokens: 100,
            temperature: None,
            is_default: false,
            extra_headers: HashMap::new(),
        };
        let s = format!("{p:?}");
        assert!(s.contains("***"), "Debug 应含 ***，实际：{s}");
        assert!(!s.contains("secret-abcd"), "Debug 不应含完整密钥：{s}");
        // 但其他字段应可见
        assert!(s.contains("gpt-4o-mini"));
    }

    #[test]
    fn mask_secret_empty() {
        assert_eq!(mask_secret(""), "<empty>");
    }

    #[test]
    fn mask_secret_short() {
        assert_eq!(mask_secret("abc"), "a***c");
        assert_eq!(mask_secret("abcdefgh"), "a***h");
    }

    #[test]
    fn mask_secret_long() {
        // 长度 > 8：前 4 后 2
        assert_eq!(mask_secret("sk-1234567890"), "sk-1***90");
    }

    #[test]
    fn mask_secret_unicode() {
        // 中文 4 个字符
        let r = mask_secret("密钥很长很长很长");
        assert!(r.contains("***"));
    }

    #[test]
    fn truncate_chars_works() {
        assert_eq!(truncate_chars("hello", 100), "hello");
        let s = "a".repeat(600);
        let out = truncate_chars(&s, 500);
        assert_eq!(out.chars().count(), 501);
        assert!(out.ends_with('…'));
    }

    // ---------- 真实 API 联通测试（默认 ignored，需 env var） ----------
    //
    // 用法（用户本地）：
    //   ANTHROPIC_API_KEY=... cargo test --bin weekly-report \
    //       llm::tests::live_anthropic -- --ignored
    //   OPENAI_API_KEY=...    cargo test ... live_openai -- --ignored
    //   GEMINI_API_KEY=...    cargo test ... live_gemini -- --ignored
    //
    // 这些测试会发起真实 HTTP 请求，CI 不跑。

    #[tokio::test]
    #[ignore]
    async fn live_anthropic() {
        let key = match std::env::var("ANTHROPIC_API_KEY") {
            Ok(k) => k,
            Err(_) => return, // 没设就静默跳过
        };
        let p = LlmProvider {
            id: "live".into(),
            name: "live-anthropic".into(),
            kind: LlmKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: key,
            model: std::env::var("ANTHROPIC_MODEL")
                .unwrap_or_else(|_| "claude-sonnet-4-20250514".into()),
            max_tokens: 64,
            temperature: None,
            is_default: false,
            extra_headers: HashMap::new(),
        };
        let r = test_connection(&p).await.expect("Anthropic 连接应成功");
        assert!(r.contains("✓"));
    }

    #[tokio::test]
    #[ignore]
    async fn live_openai() {
        let key = match std::env::var("OPENAI_API_KEY") {
            Ok(k) => k,
            Err(_) => return,
        };
        let p = LlmProvider {
            id: "live".into(),
            name: "live-openai".into(),
            kind: LlmKind::OpenAiCompatible,
            base_url: "https://api.openai.com".into(),
            api_key: key,
            model: std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
            max_tokens: 64,
            temperature: Some(0.0),
            is_default: false,
            extra_headers: HashMap::new(),
        };
        let r = test_connection(&p).await.expect("OpenAI 连接应成功");
        assert!(r.contains("✓"));
    }

    #[tokio::test]
    #[ignore]
    async fn live_gemini() {
        let key = match std::env::var("GEMINI_API_KEY") {
            Ok(k) => k,
            Err(_) => return,
        };
        let p = LlmProvider {
            id: "live".into(),
            name: "live-gemini".into(),
            kind: LlmKind::Gemini,
            base_url: "https://generativelanguage.googleapis.com".into(),
            api_key: key,
            model: std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-1.5-pro".into()),
            max_tokens: 64,
            temperature: Some(0.0),
            is_default: false,
            extra_headers: HashMap::new(),
        };
        let r = test_connection(&p).await.expect("Gemini 连接应成功");
        assert!(r.contains("✓"));
    }

    #[tokio::test]
    #[ignore]
    async fn live_bad_key_returns_clear_error() {
        let p = LlmProvider {
            id: "live".into(),
            name: "bad-key".into(),
            kind: LlmKind::OpenAiCompatible,
            base_url: "https://api.openai.com".into(),
            api_key: "sk-definitely-invalid".into(),
            model: "gpt-4o-mini".into(),
            max_tokens: 16,
            temperature: None,
            is_default: false,
            extra_headers: HashMap::new(),
        };
        let err = complete(&p, "hi").await.unwrap_err().to_string();
        // 不强求状态码，但必须含有 HTTP 状态信息或 OpenAI 错误体
        assert!(
            err.contains("401") || err.contains("invalid") || err.contains("Incorrect"),
            "错误信息应清晰，实际：{err}"
        );
    }
}
