# 功能规格 (SPEC.md)

> 本文档定义 WeeklyReport 的**功能行为**。Claude Code 实现时以本文档为准。
> 不涉及具体代码实现，只描述「应当做什么」。

---

## 1. 总览

WeeklyReport 是一个跨平台桌面应用，提供以下核心能力：

1. **读取日志**：从本机和远程服务器的 Claude Code、Codex CLI 日志中提取用户工作记录
2. **压缩聚合**：用 token 压缩策略把海量日志压成可喂给 LLM 的精简版
3. **生成周报**：调用用户配置的任意 LLM 源，按用户选定的模板生成 Markdown 周报
4. **存档历史**：所有生成的周报本地存档，作为下次生成的风格参考
5. **定时邮件**：按 cron 表达式定时执行「生成 → 发邮件」流程

用户使用一个桌面应用即可完成上述全部操作。

---

## 2. 数据实体

应用维护 6 类核心数据：

### 2.1 Workspace（工作区）

代表一个日志来源：本机或远程 SSH 服务器。

| 字段           | 类型              | 说明                                              |
| -------------- | ----------------- | ------------------------------------------------- |
| `id`           | string (UUID)     | 唯一标识                                          |
| `name`         | string            | 用户可读名                                        |
| `type`         | `"local"` \| `"ssh"` | 类型                                          |
| `host`         | string?           | SSH only                                          |
| `user`         | string?           | SSH only                                          |
| `port`         | u16?              | SSH only，默认 22                                 |
| `auth_method`  | `"key"` \| `"password"` | SSH 认证方式，默认 `"key"`                  |
| `ssh_key`      | string?           | SSH 私钥路径，留空使用系统默认；`auth_method=key` 时使用 |
| `ssh_password` | string?           | SSH 登录密码；`auth_method=password` 时使用，明文存储 |
| `claude_path`  | string?           | Claude Code 日志根目录，默认 `~/.claude`          |
| `codex_path`   | string?           | Codex CLI 日志根目录，默认 `~/.codex`             |
| `tools`        | string[]          | 启用的工具：`["claude-code", "codex"]` 子集       |

### 2.2 Template（周报模板）

定义周报的结构和风格。系统内置 3 个，用户可新建自定义。

| 字段           | 类型      | 说明                                              |
| -------------- | --------- | ------------------------------------------------- |
| `id`           | string    | UUID 或固定 `builtin-*` 前缀                      |
| `name`         | string    | 用户可读名                                        |
| `style`        | string    | `tech` / `exec` / `simple` / `custom`             |
| `sections`     | string[]  | 章节标题列表，顺序即输出顺序                      |
| `provider_id`  | string?   | 指定 LLM 源 ID，留空使用默认源                    |
| `extra_prompt` | string    | 用户自定义的额外 prompt 要求                      |
| `builtin`      | bool      | 内置模板标记，true 时不可修改不可删除             |

**内置模板：**

- `builtin-tech` — 技术周报：本周 TL;DR / 各项目进展 / 技术亮点 / 下周计划
- `builtin-exec` — 管理层汇报：执行摘要 / 关键进展 / 风险阻塞 / 下周重点
- `builtin-simple` — 简洁日报：做了啥 / 问题 / 下周

### 2.3 LlmProvider（LLM 源）

详见 [LLM.md](./LLM.md)。用户可配置 N 个，标记一个为默认。

### 2.4 Schedule（定时任务）

| 字段             | 类型            | 说明                                              |
| ---------------- | --------------- | ------------------------------------------------- |
| `id`             | string (UUID)   | 唯一标识                                          |
| `name`           | string          | 用户可读名                                        |
| `cron`           | string          | 7 段 cron：`秒 分 时 日 月 星期 年`               |
| `enabled`        | bool            | 启用状态                                          |
| `workspace_ids`  | string[]        | 涉及的工作区 ID                                   |
| `template_id`    | string          | 使用的模板 ID                                     |
| `days`           | u32             | 时间范围，最近 N 天                               |
| `recipients`     | string[]        | 收件人邮箱                                        |
| `cc`             | string[]        | 抄送邮箱                                          |
| `subject_tpl`    | string          | 邮件主题模板，支持 `{date}` `{week}` 变量         |
| `last_run`       | string?         | 上次运行时间 (ISO 8601)                           |
| `last_status`    | string?         | 上次运行状态：`success` 或 `failed: <reason>`     |
| `next_run`       | string?         | 下次预计运行时间（运行时计算，不持久化）           |

### 2.5 SmtpConfig（SMTP 配置）

单例。

| 字段        | 类型   | 说明                                                  |
| ----------- | ------ | ----------------------------------------------------- |
| `host`      | string | SMTP 主机                                             |
| `port`      | u16    | 端口                                                  |
| `username`  | string | 用户名（一般是邮箱地址）                              |
| `password`  | string | 密码或授权码                                          |
| `from_name` | string | 发件人显示名                                          |
| `use_ssl`   | bool   | true=SSL/465, false=STARTTLS/587                      |

**预设：** Gmail / Outlook 365 / QQ / 163 / 企业微信邮箱，UI 提供一键填充。

### 2.6 Report（历史周报）

每生成一次保存一份。

| 字段             | 类型            | 说明                                              |
| ---------------- | --------------- | ------------------------------------------------- |
| `id`             | string (UUID)   | 唯一标识                                          |
| `week`           | string          | 时间范围标签（如 `"最近 7 天"` 或具体日期范围）   |
| `template_id`    | string          | 使用的模板                                        |
| `template_name`  | string          | 模板名（冗余，便于查看）                          |
| `provider_id`    | string?         | 使用的 LLM 源                                     |
| `provider_name`  | string?         | LLM 源名（冗余）                                  |
| `tokens_used`    | u32             | 消耗的 token 数                                   |
| `project_count`  | u32             | 涉及的项目数                                      |
| `generated_at`   | string          | 生成时间 ISO 8601                                 |
| `content`        | string (文件)   | Markdown 正文，单独存为 `<id>.md`                 |

---

## 3. 核心算法：Token 压缩策略

**这是项目的核心价值，必须严格按此实现。**

输入：原始 JSONL 日志文件（可能几百万字符）。
输出：精简版的「工作记录摘要」字符串（通常 < 30k tokens）。

### 压缩规则

| 内容类型                            | 处理方式                                          |
| ----------------------------------- | ------------------------------------------------- |
| 用户指令 (真实 user prompt)         | **全文保留**（这是核心工作信号，不可丢失）        |
| AI 文本回复                         | 保留首 N 字符 + 末 N 字符，中间用 `…` 替代       |
| Tool use                            | 仅保留工具名 + 一个关键参数（如 file_path）       |
| Tool result                         | **完全丢弃**                                      |
| Thinking / Reasoning blocks         | 完全丢弃                                          |
| Meta / summary / git-commit / event_msg / compacted | 完全丢弃                          |
| 连续相似指令                        | 前 30 字符相同的相邻指令视为重复，自动去重        |

`N` 默认为 200，可在设置中调整。

> **重要陷阱**：Claude Code 中 `type:"user"` 不一定是真实用户指令；
> 当行携带 `toolUseResult` 字段或 `message.content` 是数组时，实质上是 tool result 反灌，必须按 tool result 丢弃。
> 详见 [JSONL.md §4.4](./JSONL.md#44-真实用户消息判别关键陷阱)。

### 日志文件位置

- Claude Code：
  - 全局 `~/.claude/history.jsonl`
  - 各 session：`~/.claude/projects/<encoded-path>/<session-id>.jsonl`
- Codex CLI：
  - 各 session：`~/.codex/sessions/YYYY/MM/DD/rollout-<timestamp>-<id>.jsonl`
  - 全局（可选）：`~/.codex/history.jsonl`

读取时按文件修改时间过滤，只保留指定天数内的文件。

### JSONL 格式适配

Claude Code 和 Codex CLI 的 JSONL 结构差异较大，需要分别适配：

- **Claude Code** 用顶层 `type` 字段区分用户/助手/工具，`message.content` 是 string 或 typed-block 数组
- **Codex CLI** 用 `type` + `payload` 两段嵌套，`response_item.payload.type` 再区分 message/reasoning/function_call 等

每个 CLI 在版本间还会有 schema 漂移。**所有字段定义与版本兼容矩阵详见 [JSONL.md](./JSONL.md)。** 实现时统一用 `serde_json::Value` 容错解析，不硬声明 struct。

### 输出格式

最终喂给 LLM 的内容形如：

```
你是工程师周报助手...

风格：技术向...
活跃天数：5 | 项目数：3 | 主项目：weekly-report
服务器：MacBook Pro, GPU服务器

以下是从日志提取的用户工作指令（按项目分组）：
<work_logs>
【weekly-report】(12 条指令)
  · 帮我实现 LLM provider 抽象
  · 把 SQLite 换成 JSON 文件
  ...

【chat-bot】(5 条指令)
  · 调试 stream API 的中断问题
  ...
</work_logs>

请按以下章节顺序输出 Markdown 周报：本周 TL;DR / 各项目进展 / 技术亮点 / 下周计划

要求：1. 提炼不要照抄；2. 相似归纳合并；3. 下周计划合理推断标注（待确认）
```

---

## 4. 用户操作流程

### 4.1 首次启动

应用启动时：

1. 在 OS 配置目录下创建数据目录（不存在时）
2. 创建一个默认的本机工作区（指向 `~/.claude` 和 `~/.codex`）
3. 加载所有内置模板（始终存在，不可删除）
4. **若无 LLM 源已配置，引导用户到「LLM 源」页**

### 4.2 配置 LLM 源

详见 [LLM.md](./LLM.md)。流程：

1. 进入「LLM 源」页
2. 点「添加 LLM 源」或选一个预设
3. 填入 base_url、API key、model
4. 点「测试连接」验证
5. 保存（首次添加自动设为默认）

### 4.3 添加远程工作区

1. 进入「工作区」页
2. 点「添加工作区」，选「SSH 远程」
3. 填入 host / user / port / ssh_key 路径
4. 选择要读取的工具（Claude Code / Codex）和路径
5. 点「测试连接」验证（应当显示工具目录是否存在）
6. 保存

### 4.4 手动生成周报

1. 侧边栏底部「生成周报」按钮
2. 弹出对话框：
   - 勾选要包含的工作区（默认全选）
   - 选择模板
   - 选择 LLM 源（默认用模板指定的，或全局默认）
   - 选择时间范围（3 / 7 / 14 / 30 天）
3. 点「开始生成」
4. 显示进度（扫描日志 → 压缩 → 调用 LLM）
5. 完成后展示 Markdown 预览，可复制、下载、关闭
6. 自动存档到「历史周报」

### 4.5 配置定时任务

前提：已配置 SMTP。

1. 进入「定时任务」页
2. 点「新建定时任务」
3. 填入：
   - 任务名（用户可读）
   - cron 表达式（提供 4 个常用预设：每周一/五/工作日/周日）
   - 选择工作区、模板、时间范围
   - 邮件主题模板（支持 `{date}` `{week}`）
   - 收件人邮箱、抄送邮箱
4. 启用任务
5. 保存

应用后台自动按 cron 触发，每次执行：拉日志 → 压缩 → 生成 → 发邮件，结果记录到 `last_run` / `last_status`。

支持「立即执行一次」按钮，用于调试和验证。

### 4.6 配置 SMTP

1. 进入「设置」页
2. 选 SMTP 预设（Gmail / QQ / 163 / 企业微信邮箱），自动填 host/port/加密方式
3. 提示用户必要时使用授权码而不是登录密码
4. 填入用户名、密码
5. 点「测试连接」验证
6. 可发一封测试邮件确认
7. 保存

---

## 5. 错误处理

### 5.1 用户可见的错误

所有可预见的错误必须给出**人类可读、可操作**的提示：

- LLM 调用失败 → 显示 HTTP 状态码 + 服务端返回的错误片段
- SMTP 认证失败 → 提示「检查授权码（非登录密码）」
- SSH 连接失败 → 提示具体错误（超时/认证失败/host 不可达）
- 日志路径不存在 → 提示「路径不存在：<path>」
- 模板内容验证失败 → 提示具体字段错误

### 5.2 程序内部错误

- 后端使用 `anyhow::Result` 传播错误
- Tauri command 返回 `Result<T, String>`，错误转字符串给前端
- 前端 try-catch 所有 invoke 调用，失败时显示用户可见提示

### 5.3 持久化错误

- 文件写入失败 → 保留旧版本（原子写保证），向用户报错
- JSON 解析失败 → 备份损坏文件为 `.broken-<timestamp>`，恢复为默认值

### 5.4 定时任务失败

- 失败不阻塞调度器，下次仍按 cron 触发
- `last_status` 记录失败原因
- 失败状态在任务卡片上红色显示前 60 字符

---

## 6. 性能要求

- **应用启动到可交互：< 2s**（冷启动）
- **生成周报端到端：< 60s**（不含 LLM 网络延迟）
- **UI 响应：所有操作 < 100ms 反馈**（loading 状态算反馈）
- **二进制大小：< 10 MB**（macOS arm64 release）

---

## 7. 安全与隐私

- API key、SMTP 密码以**明文**保存在本地 JSON 文件（v0.1.0 接受此妥协，未来用 keyring 加密）
- 数据目录权限：仅当前用户可读写
- **绝不上报任何用户数据到第三方**（包括崩溃日志、telemetry）
- 网络请求仅发往：用户配置的 LLM endpoint、SMTP server，不发往任何 Anthropic / 开发者控制的地址

---

## 8. 非功能性需求

- **代码质量**：通过 `cargo clippy -- -D warnings`、`cargo fmt --check`，前端通过 ESLint（如配置）
- **可移植性**：所有路径用 `dirs` crate 自动适配 OS
- **可观测性**：使用 `tracing` 库，关键操作记录 INFO 日志
- **可测试性**：核心模块（logs、llm、email）有单元测试
- **本地化**：默认中文 UI；预留 i18n 扩展位

---

## 9. 不在范围内（v0.1.0）

以下功能**不在 v0.1.0 范围**，但架构上应留出扩展位：

- 系统托盘最小化常驻
- 多语言支持
- WebDAV / iCloud 同步
- API key 加密存储（keyring）
- Slack / 钉钉 / 飞书机器人推送
- 失败重试 + 系统通知
- Web 部署版本

详见 [DECISIONS.md](./DECISIONS.md) 中的「路线图」。
