# LLM 源抽象 (LLM.md)

> 本文档定义 WeeklyReport 如何支持多种 LLM 源。
> Claude Code 实现 `llm.rs` 时严格遵循本文档。

---

## 1. 设计目标

- 一份代码同时支持 **OpenAI 兼容、Anthropic 原生、Google Gemini** 三大类 API
- 新增同类型 API（如阿里 Qwen、Moonshot Kimi 等）**只需加一条预设，不写代码**
- 用户可配置 N 个 provider，**选一个为默认**
- 每个模板可单独绑定 provider；定时任务执行时也可指定
- 生成失败时**清晰报错**，便于排查（HTTP 码、body 片段都给出）

---

## 2. 数据模型

```rust
pub struct LlmProvider {
    pub id:           String,                    // UUID
    pub name:         String,                    // 用户可读名
    pub kind:         LlmKind,                   // 协议类型
    pub base_url:     String,                    // API 根地址，不带路径
    pub api_key:      String,                    // 本地存储；本地 LLM 可填占位
    pub model:        String,                    // 模型 ID
    pub max_tokens:   u32,                       // 生成长度上限
    pub temperature:  Option<f32>,               // 0~1，None 时不传
    pub is_default:   bool,                      // 默认 provider 标记
    pub extra_headers: HashMap<String, String>,  // 额外请求头（如 OpenRouter 的 HTTP-Referer）
}

pub enum LlmKind {
    OpenAiCompatible,
    Anthropic,
    Gemini,
}
```

---

## 3. 协议适配

### 3.1 OpenAI 兼容

适用：OpenAI、DeepSeek、OpenRouter、Kimi、Qwen、Groq、SiliconFlow、本地 Ollama / vLLM / LM Studio。

**请求：**

```
POST {base_url}/v1/chat/completions
Authorization: Bearer {api_key}           # 若 api_key 非空
Content-Type: application/json
{ ...extra_headers }

{
  "model": "{model}",
  "messages": [{"role": "user", "content": "{prompt}"}],
  "max_tokens": {max_tokens},
  "temperature": {temperature}              # 仅当非 None
}
```

**响应解析：**

```
text   = response["choices"][0]["message"]["content"]
tokens = response["usage"]["total_tokens"]
```

### 3.2 Anthropic 原生

适用：Claude 官方 API、所有 Claude 中转代理（只要兼容 `/v1/messages`）。

**请求：**

```
POST {base_url}/v1/messages
x-api-key: {api_key}
anthropic-version: 2023-06-01
Content-Type: application/json
{ ...extra_headers }

{
  "model": "{model}",
  "max_tokens": {max_tokens},
  "messages": [{"role": "user", "content": "{prompt}"}],
  "temperature": {temperature}              # 仅当非 None
}
```

**响应解析：**

```
text   = response["content"][0]["text"]
tokens = response["usage"]["input_tokens"] + response["usage"]["output_tokens"]
```

### 3.3 Google Gemini 原生

适用：Google AI Studio。

**请求：**

```
POST {base_url}/v1beta/models/{model}:generateContent?key={api_key}
Content-Type: application/json

{
  "contents": [{"parts": [{"text": "{prompt}"}]}],
  "generationConfig": {
    "maxOutputTokens": {max_tokens},
    "temperature": {temperature}            # 仅当非 None
  }
}
```

**响应解析：**

```
text   = response["candidates"][0]["content"]["parts"][0]["text"]
tokens = response["usageMetadata"]["totalTokenCount"]
```

---

## 4. 核心 API

```rust
/// 主入口：统一接口，内部分发到对应协议
pub async fn complete(provider: &LlmProvider, prompt: &str) -> Result<CompletionResult>;

/// 测试连接：发一个超短 prompt，验证认证和连通性
pub async fn test_connection(provider: &LlmProvider) -> Result<String>;

/// 预设列表：给前端 UI 提供「快速选择」
pub fn presets() -> Vec<(&'static str, LlmProvider)>;

pub struct CompletionResult {
    pub text:        String,
    pub tokens_used: u32,
    pub duration_ms: u64,
}
```

### 错误处理要求

任何失败必须包含足够信息便于排查：

- HTTP 状态码（非 2xx 时）
- 服务端响应 body 的前 500 字符
- 网络错误（超时、连接被拒）的原始描述

错误示例：

```
LLM API 返回 401: {"error":{"message":"Incorrect API key provided: sk-xxx","type":"invalid_request_error"}}
LLM API 请求超时（120s）
```

---

## 5. 预设清单

`presets()` 必须返回以下 10 个预设，按此顺序：

| 标签                    | kind              | base_url                                            | model（建议默认）             |
| ----------------------- | ----------------- | --------------------------------------------------- | ----------------------------- |
| Anthropic Claude        | Anthropic         | `https://api.anthropic.com`                         | `claude-sonnet-4-20250514`    |
| OpenAI GPT              | OpenAiCompatible  | `https://api.openai.com`                            | `gpt-4o-mini`                 |
| DeepSeek                | OpenAiCompatible  | `https://api.deepseek.com`                          | `deepseek-chat`               |
| OpenRouter              | OpenAiCompatible  | `https://openrouter.ai/api`                         | `anthropic/claude-sonnet-4`   |
| Kimi (月之暗面)         | OpenAiCompatible  | `https://api.moonshot.cn`                           | `moonshot-v1-8k`              |
| 通义千问 (Qwen)         | OpenAiCompatible  | `https://dashscope.aliyuncs.com/compatible-mode`    | `qwen-plus`                   |
| Gemini                  | Gemini            | `https://generativelanguage.googleapis.com`         | `gemini-1.5-pro`              |
| 本地 Ollama             | OpenAiCompatible  | `http://localhost:11434`                            | `qwen2.5:7b`                  |
| 本地 vLLM / LM Studio   | OpenAiCompatible  | `http://localhost:8000`                             | `Qwen/Qwen2.5-7B-Instruct`    |
| 自定义 (OpenAI 兼容)    | OpenAiCompatible  | （空）                                              | （空）                        |

预设默认值：
- `max_tokens`: 2048
- `temperature`: 0.7（Anthropic 设为 None）
- `is_default`: 第一个（Anthropic）为 true，其他 false
- `api_key`: 空（除 Ollama 填 `"ollama"`，vLLM 填 `"EMPTY"` 作为占位）

---

## 6. 优先级解析

调用方需要选 provider 时，按以下顺序：

```
1. 调用方显式传入的 provider_id（如 generate 时用户在对话框里选的）
2. Template 的 provider_id 字段
3. LlmProvider 列表中 is_default = true 的
4. LlmProvider 列表的第一个
5. 报错：「未配置任何 LLM 源」
```

`state::get_default_provider()` 实现 step 3-5。

---

## 7. 默认 provider 管理逻辑

- **新增 provider 时**：若是列表中第一个，自动 `is_default = true`
- **保存 provider 且 `is_default = true` 时**：把其他所有 provider 的 `is_default` 置 false
- **删除当前默认 provider 时**：自动把剩余的第一个设为默认

由 `state.rs` 实现。

---

## 8. 测试连接的实现

`test_connection` 发送一个**极短 prompt**，验证两件事：
1. 网络/认证可通
2. 模型 ID 正确（错的 model 会被服务端拒）

实现：

```rust
pub async fn test_connection(provider: &LlmProvider) -> Result<String> {
    let r = complete(provider, "回复一个 'OK' 即可，不要其他内容").await?;
    Ok(format!(
        "✓ 连接成功（耗时 {}ms, 用 {} tokens）\n模型回复: {}",
        r.duration_ms,
        r.tokens_used,
        r.text.chars().take(50).collect::<String>()
    ))
}
```

UI 在测试连接成功时**绿色显示**这段返回，失败时红色显示错误。

---

## 9. 超时

- HTTP 请求默认 **120 秒**超时（用于生成报告，可能需要较长）
- 测试连接也走 120 秒，但回复短，正常情况秒级返回

---

## 10. 不在范围（v0.1.0 不做）

- ❌ 流式响应（SSE）—— 后续可选
- ❌ Function calling / tool use —— 周报场景不需要
- ❌ 多轮对话 —— 一次 prompt 一次响应足够
- ❌ Vision / 图片输入 —— 周报场景不需要
- ❌ Token 计数预估 —— 不准且无价值

---

## 11. 验证清单

实现完成后必须验证：

- [ ] 调用 OpenAI 真实 API 成功（用任一可用 key）
- [ ] 调用 DeepSeek（OpenAI 兼容协议）成功
- [ ] 调用 Anthropic 真实 API 成功
- [ ] 调用 Gemini 真实 API 成功
- [ ] 调用本地 Ollama（无 API key 也能工作）成功
- [ ] 错误 API key 时报错信息清晰
- [ ] 错误 model 名时报错信息清晰
- [ ] 网络超时时报错信息清晰
- [ ] `extra_headers` 字段确实附加到请求中（用 OpenRouter 的 `HTTP-Referer` 测试）
- [ ] 默认 provider 切换逻辑正确（新加 → 自动默认；改默认 → 旧的取消；删默认 → 剩余第一个升级）
