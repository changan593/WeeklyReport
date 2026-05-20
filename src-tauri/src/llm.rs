//! LLM 源数据模型。
//!
//! 协议适配（OpenAI 兼容 / Anthropic / Gemini）、`complete()`、`test_connection()`
//! 与 `presets()` 由阶段 3 实现，详见 `docs/LLM.md`。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// LLM API 协议类型。
///
/// JSON 形式：
/// ```json
/// "OpenAiCompatible" | "Anthropic" | "Gemini"
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LlmKind {
    #[default]
    OpenAiCompatible,
    Anthropic,
    Gemini,
}

/// 用户配置的一个 LLM 源。详细字段语义见 `docs/LLM.md#2-数据模型`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LlmProvider {
    pub id: String,
    pub name: String,
    pub kind: LlmKind,
    pub base_url: String,
    /// 本地明文存储；本地 LLM 可填占位（如 Ollama 用 "ollama"）
    #[serde(default)]
    pub api_key: String,
    pub model: String,
    /// 生成长度上限；预设默认 2048
    pub max_tokens: u32,
    /// 0~1，None 时不传给 API（Anthropic 预设为 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// 默认 provider 标记。新增第一个 provider 时自动 true。
    #[serde(default)]
    pub is_default: bool,
    /// 额外请求头（如 OpenRouter 的 `HTTP-Referer`）。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra_headers: HashMap<String, String>,
}

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
        assert!(
            json.get("temperature").is_none(),
            "None temperature 应被省略"
        );
    }
}
