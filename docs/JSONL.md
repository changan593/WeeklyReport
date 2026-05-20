# JSONL 格式适配 (JSONL.md)

> 本文档定义 WeeklyReport 如何解析 Claude Code 和 OpenAI Codex CLI 的日志文件。
> Claude Code 实现 `logs.rs` 时严格遵循本文档。
> 字段定义来自两个项目的源码与社区适配器（见 §11 来源）。

---

## 1. 设计目标

- 同一份 `collect_messages` 同时吃 Claude Code 和 Codex CLI 的日志
- Schema 在不同 CLI 版本间会漂移（Codex 已知有 ≥0.44 / mid / 2025-08 三套 session meta 格式），实现必须容错
- 不丢字段、不误算 token：用户指令保留、工具结果丢弃
- 无法解析的行（未知 `type` / 损坏 JSON）静默跳过，不阻塞整个文件

---

## 2. 日志文件位置

### Claude Code

- 全局 prompt 历史：`~/.claude/history.jsonl`
- 每个 session 完整记录：`~/.claude/projects/<encoded-cwd>/<sessionId>.jsonl`
  - `<encoded-cwd>`：把 cwd 中所有 `/` 和 `_` 替换为 `-`，**有损编码**，无法可靠反推
  - `<sessionId>`：UUID

### Codex CLI

- 每个 session 完整记录：`~/.codex/sessions/YYYY/MM/DD/rollout-<ISO-ts>-<UUID>.jsonl`
- 用户 prompt 历史：`~/.codex/history.jsonl`（可选，可被 config 关掉）

`claude_path` / `codex_path` 字段可被 `Workspace` 覆盖，默认 `~/.claude` 和 `~/.codex`。

读取时按文件 **mtime** 过滤，只保留 `since` 之后修改过的文件。

---

## 3. Claude Code — `history.jsonl`

**仅用户 prompt，无 AI 回复。** 是 session JSONL 不可用时的降级数据源。

```json
{
  "display": "帮我把 SQLite 换成 JSON",
  "pastedContents": {},
  "timestamp": 1747476225000,
  "project": "-Users-me-weekly-report"
}
```

| 字段             | 类型              | 说明                                                |
| ---------------- | ----------------- | --------------------------------------------------- |
| `display`        | string            | 用户输入的 prompt 全文                              |
| `pastedContents` | object            | 粘贴块（图片/文件等），WeeklyReport 不使用          |
| `timestamp`      | **number (ms)**   | Unix epoch 毫秒，不是 ISO 字符串                    |
| `project`        | string            | 项目标识，通常是 encoded cwd（编码不可逆）          |

**解析要点：**
- `timestamp` 按 `i64`（ms）处理；防御性：若值 < 1e12 视为秒
- `project` 反推不可行 —— 只能取尾段做项目名（如 `-Users-me-weekly-report` → `weekly-report`）

---

## 4. Claude Code — 项目 session JSONL

每行一条事件。

### 4.1 顶层字段

| 字段            | 类型     | 说明                                                  |
| --------------- | -------- | ----------------------------------------------------- |
| `type`          | string   | `user` / `assistant` / `summary` / `git-commit` 等   |
| `timestamp`     | string   | ISO 8601                                              |
| `uuid`          | string   | 这一行的 UUID                                         |
| `parentUuid`    | string?  | 父节点 UUID（用于 thread）                            |
| `sessionId`    | string   | session UUID                                          |
| `cwd`           | string?  | 工作目录 — **真值，infer_project 用它**               |
| `gitBranch`     | string?  | 当前分支                                              |
| `isMeta`        | bool?    | 元消息标记                                            |
| `message`       | object?  | 实际消息体（见 §4.2）                                 |
| `toolUseResult` | any?     | **存在即代表本行是 tool result**，非真实用户消息      |
| `costUSD`       | number?  | 成本（仅 assistant）                                  |
| `durationMs`    | number?  | 耗时（仅 assistant）                                  |
| `summary`       | string?  | 仅 `type:"summary"` 行有                              |

### 4.2 `message.content` 的两种形态

**字符串形态**（典型用户 prompt）：

```json
{
  "type": "user",
  "timestamp": "2026-05-17T10:23:45Z",
  "uuid": "...",
  "sessionId": "...",
  "cwd": "/Users/me/weekly-report",
  "message": {"role": "user", "content": "修一下 schedule 的 cron 解析"}
}
```

**数组形态**（assistant 几乎总是 / user 在反灌 tool result 时）：

```json
{
  "type": "assistant",
  "timestamp": "...",
  "message": {
    "role": "assistant",
    "content": [
      {"type": "text", "text": "我先看看 scheduler.rs"},
      {"type": "thinking", "thinking": "..."},
      {"type": "tool_use", "id": "tu_01", "name": "Read", "input": {"file_path": "src/scheduler.rs"}}
    ]
  },
  "costUSD": 0.012,
  "durationMs": 1480
}
```

**Tool result 反灌**（注意 `type:"user"` 但带 `toolUseResult`）：

```json
{
  "type": "user",
  "toolUseResult": "...",
  "message": {
    "role": "user",
    "content": [
      {"type": "tool_result", "tool_use_id": "tu_01", "content": [{"type": "text", "text": "..."}]}
    ]
  }
}
```

### 4.3 内容块类型

| 块 `type`     | 字段                                         | WeeklyReport 处理         |
| ------------- | -------------------------------------------- | ------------------------- |
| `text`        | `text: string`                               | 拼接为 assistant 正文     |
| `thinking`    | `thinking: string`                           | **丢弃**                  |
| `tool_use`    | `id`, `name`, `input: object`                | `[name: 关键参数]`        |
| `tool_result` | `tool_use_id`, `content: string \| array`    | **丢弃**                  |

### 4.4 真实用户消息判别（关键陷阱）

`type:"user"` **不一定**是真实用户指令。判别条件：

```text
isRealUserPrompt =
    type == "user"
    && typeof message.content == "string"
    && message.content is non-empty
    && toolUseResult is absent
    && isMeta is not true
```

否则按 tool result 或元数据丢弃。

---

## 5. Codex CLI — rollout JSONL

每行是一条 `RolloutLine`，**两段嵌套**结构，与 Claude Code 完全不同。源码定义（`codex-rs/protocol/src/protocol.rs`）：

```rust
pub struct RolloutLine {
    pub timestamp: String,        // ISO 8601
    #[serde(flatten)]
    pub item: RolloutItem,        // 提供 type + payload
}

#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum RolloutItem {
    SessionMeta(SessionMetaLine),
    ResponseItem(ResponseItem),
    Compacted(CompactedItem),
    TurnContext(TurnContextItem),
    EventMsg(EventMsg),
}
```

JSON 形式：

```json
{"timestamp":"2026-05-17T10:23:45.123Z","type":"session_meta","payload":{...}}
{"timestamp":"2026-05-17T10:23:50.456Z","type":"response_item","payload":{...}}
```

### 5.1 `session_meta`（每个文件第一行）

```json
{
  "timestamp": "2026-05-17T10:23:45Z",
  "type": "session_meta",
  "payload": {
    "id": "thread-uuid",
    "timestamp": "2026-05-17T10:23:45Z",
    "cwd": "/Users/me/weekly-report",
    "originator": "codex",
    "cli_version": "0.50.0",
    "source": "cli",
    "model_provider": "openai",
    "git": {"branch": "main", "commit_hash": "abc123"}
  }
}
```

`payload.cwd` 是 infer_project 的真值来源。三版兼容：老版本可能缺 `source` / `model_provider` / `git` 等字段 —— 用 `Value::get(...)` 容错读取，**不要硬声明 struct**。

### 5.2 `response_item` 内层 `type`

源码：`#[serde(tag = "type", rename_all = "snake_case")]` 在 `ResponseItem` 上：

| 内层 `type`              | 字段                                          | 处理                       |
| ------------------------ | --------------------------------------------- | -------------------------- |
| `message`                | `role`, `content: ContentItem[]`              | 见 §5.3                    |
| `reasoning`              | `summary[]`, `content[]?`                     | **丢弃**                   |
| `function_call`          | `name`, `arguments: string`, `call_id`        | `[name: 关键参数]`         |
| `function_call_output`   | `call_id`, `output`                           | **丢弃**                   |
| `local_shell_call`       | `action`, `status`                            | 按 tool 处理               |
| `custom_tool_call*`      | `name`, `input: string`                       | 按 tool 处理               |
| `web_search_call`        | `action`                                      | 丢弃或按 tool              |
| `image_generation_call`  | `result`, `revised_prompt?`                   | 丢弃                       |
| `compaction*`            | `encrypted_content`                           | 丢弃                       |

### 5.3 Codex `message.content`（与 Claude 不同）

```json
{
  "type": "response_item",
  "payload": {
    "type": "message",
    "role": "user",
    "content": [{"type": "input_text", "text": "重构这个模块"}]
  }
}
```

```json
{
  "type": "response_item",
  "payload": {
    "type": "message",
    "role": "assistant",
    "content": [{"type": "output_text", "text": "好的，我先看一下结构。"}]
  }
}
```

| 块 `type`      | 字段                | WeeklyReport 处理        |
| -------------- | ------------------- | ------------------------ |
| `input_text`   | `text: string`      | 用户指令，**全文保留**   |
| `output_text`  | `text: string`      | 助手回复，首尾各 N 字符  |
| `input_image`  | `image_url: string` | 丢弃                     |

**注意：Codex 的 user message content 永远是数组**，没有字符串形态。这点与 Claude Code 相反。

### 5.4 Codex `function_call.arguments` 的嵌套

```json
{
  "type": "response_item",
  "payload": {
    "type": "function_call",
    "name": "shell",
    "call_id": "call_01",
    "arguments": "{\"command\":[\"ls\",\"-la\"]}"
  }
}
```

`arguments` 是 **JSON 字符串**（再嵌套一层），不是 object。提炼时先 `serde_json::from_str` 转 `Value`，再挑关键参数（如 `command[0]` / `file_path`）。

### 5.5 其他 `RolloutItem` 顶层 `type`

- `compacted`：上下文压缩点，丢弃
- `turn_context`：每轮 trace_id，丢弃
- `event_msg`：流式事件，丢弃（事件信息在 response_item 已有）

---

## 6. 版本兼容矩阵

| CLI            | 版本                | 注意点                                                        |
| -------------- | ------------------- | ------------------------------------------------------------- |
| Claude Code    | 全部                | `type:"user"` + `toolUseResult` 必须当 tool result 处理       |
| Claude Code    | 老版本              | `cwd` / `gitBranch` 字段可能缺                                |
| Codex          | ≥ 0.44              | 完整 `RolloutLine` schema                                     |
| Codex          | mid (2025-08 前后)  | `session_meta` 字段不全                                       |
| Codex          | 老版本              | 可能没有 `payload` 包裹，需要 fallback 直接读 `RolloutLine.item` |

实现策略：**全部用 `serde_json::Value`** + 按字符串 `type` 分支；不硬声明 struct；任何 `Value::get(...)` 缺失走默认值。

---

## 7. 项目名推断 `infer_project()`

按优先级：

| 数据源                              | 真值字段              | 退路                           |
| ----------------------------------- | --------------------- | ------------------------------ |
| Claude Code session JSONL           | 任一行的 `cwd` 末段   | 文件名 stem                    |
| Claude Code history.jsonl           | `project` 末段        | "Claude Code"                  |
| Codex rollout JSONL                 | `session_meta.cwd` 末段 | 文件名 stem                  |
| Codex history.jsonl                 | 无                    | "Codex"                        |

例：

```
/Users/me/weekly-report   →   weekly-report
C:\dev\my-app             →   my-app
-Users-me-weekly-report   →   weekly-report   (history.jsonl 退路)
```

---

## 8. Token 压缩规则汇总（与 SPEC#3 对齐）

| 来源                                       | 处理                                |
| ------------------------------------------ | ----------------------------------- |
| Claude `type:"user"` + 字符串 content      | **全文保留**                        |
| Claude history.jsonl `display`             | **全文保留**                        |
| Codex `input_text`                         | **全文保留**                        |
| Claude `text` 块 / Codex `output_text`     | 首尾各 N 字符（默认 N=200）         |
| Claude `tool_use` / Codex `function_call`  | `[name: 关键参数(≤60)]`             |
| `thinking` / `reasoning`                   | 丢弃                                |
| `tool_result` / `function_call_output`     | 丢弃                                |
| Claude `summary` / `git-commit`            | 丢弃                                |
| Codex `session_meta`/`event_msg`/`compacted`/`turn_context` | 丢弃              |
| Claude `type:"user"` + `toolUseResult`     | 丢弃（实质是 tool result）          |
| Claude `type:"user"` + `isMeta=true`       | 丢弃                                |

**去重：** 连续两条用户指令，前 30 字符完全相同视为重复，保留前一条。

---

## 9. 实现策略要点

- 不为 JSONL 格式声明强类型 struct，统一用 `serde_json::Value` + `type` 字符串分支
- 单行 JSON 解析失败 → `warn!` 后 `continue`，不阻塞整文件
- 单文件失败 → 记日志后 `continue`，不阻塞整个 workspace
- 未知 `type`（如 Codex 未来新增事件类型）静默跳过
- 文件按 mtime 过滤到 since 之后再 open
- 每条 `Message` 入库时记 `(project, tool, server)` 三元组，便于 `aggregate()` 分组

---

## 10. 验证清单

实现完成后必须确认：

- [ ] 真实 `~/.claude/history.jsonl` 解析不崩
- [ ] 真实 `~/.claude/projects/<x>/<y>.jsonl` 解析不崩
- [ ] 真实 `~/.codex/sessions/.../rollout-*.jsonl` 解析不崩
- [ ] Claude `type:"user"` 但携带 `toolUseResult` 的行不被算为用户指令
- [ ] Claude `message.content` 为字符串和为数组都能正确处理
- [ ] Codex `session_meta.payload.cwd` 缺失时不 panic
- [ ] Codex `function_call.arguments` 内嵌 JSON 串能正确二次解析
- [ ] 损坏 JSON 行 `warn!` 后 `continue`
- [ ] 未知 `type` 字段静默跳过
- [ ] 输出 Summary 中 `by_project` 的 key 是真实 cwd 末段（不是编码后的路径）
- [ ] 连续相同前 30 字符的用户指令被去重
- [ ] 时间字段：Claude history.jsonl 按 ms 数字；其他按 ISO 字符串

---

## 11. 致谢与来源

本文档字段定义来自以下源头：

- OpenAI Codex 源码：[`codex-rs/protocol/src/protocol.rs`](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)（RolloutLine / RolloutItem / SessionMeta）
- OpenAI Codex 源码：[`codex-rs/protocol/src/models.rs`](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/models.rs)（ResponseItem / ContentItem）
- Codex PR [#3380 Introduce rollout items](https://github.com/openai/codex/pull/3380) 与 [#14434 RolloutLine schema](https://github.com/openai/codex/pull/14434)
- Codex discussion [#3827 Session/Rollout Files](https://github.com/openai/codex/discussions/3827)
- [PixelPaw-Labs/codex-trace](https://github.com/PixelPaw-Labs/codex-trace)（多版本 session_meta 兼容说明）
- [amac0/ClaudeCodeJSONLParser](https://github.com/amac0/ClaudeCodeJSONLParser)（Claude `type` 字段判别）
- [withLinda/claude-JSONL-browser](https://github.com/withLinda/claude-JSONL-browser)（Claude content 数组样本）
- [Dicklesworthstone/coding_agent_session_search](https://github.com/Dicklesworthstone/coding_agent_session_search)（多 agent 适配模式）
