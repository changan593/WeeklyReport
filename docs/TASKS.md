# 分阶段实现计划 (TASKS.md)

> 按顺序实现。每阶段完成后必须通过该阶段的「验证」才进入下一阶段。
> Claude Code 应在每阶段结束时提交一次 git commit。

---

## 阶段 0：项目骨架

**目标：** 建立空项目并能跑起来一个空白 Tauri 窗口。

**任务：**

1. 创建项目根目录文件：
   - `README.md`
   - `LICENSE` (MIT)
   - `.gitignore`（忽略 `node_modules/`, `target/`, `dist/`, `*.log`, `.DS_Store`）
   - `CLAUDE.md`（拷贝本仓库提供的内容）
   - `docs/` 全套文档
2. 初始化 `package.json`、`vite.config.js`、`tailwind.config.js`、`postcss.config.js`、`index.html`
3. 初始化 `src/main.jsx`、`src/App.jsx`（一个空白页就行）、`src/index.css`
4. 初始化 `src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/build.rs`、`src-tauri/src/main.rs`
5. 创建占位图标到 `src-tauri/icons/`（v0.1.0 用纯色 PNG，正式发布时替换）

**Cargo.toml 依赖：** 见 [ARCHITECTURE.md#关键依赖rust](./ARCHITECTURE.md#6-关键依赖rust)
**package.json 依赖：** 见 [ARCHITECTURE.md#关键依赖js](./ARCHITECTURE.md#7-关键依赖js)

**验证：**

- [ ] `npm install` 成功
- [ ] `npm run tauri:dev` 启动一个空白窗口
- [ ] `cargo build` 成功
- [ ] `cargo clippy -- -D warnings` 无警告

**提交：** `chore: initial project scaffold`

---

## 阶段 1：存储层 + 数据模型

**目标：** 实现 `store.rs` + `state.rs` + 所有数据实体。

**任务：**

1. 实现 `store.rs`：
   - `init()` 创建数据目录
   - `data_dir()` 跨平台获取路径
   - `read_json` / `write_json` 通用读写
   - 原子写入（`.tmp` + `rename`）
   - 损坏文件备份机制
   - `save_report_file` / `load_report_file` / `delete_report_file`
2. 定义所有数据结构（在各自的模块里）：
   - `workspace.rs`: `Workspace`
   - `report.rs`: `Template`, `ReportRecord`
   - `llm.rs`: `LlmProvider`, `LlmKind`
   - `scheduler.rs`: `Schedule`
   - `email.rs`: `SmtpConfig`, `EmailRequest`
   - `state.rs`: `Settings`
3. 实现 `state.rs`：
   - 各实体的 `list / save / delete`
   - 内置模板始终注入到列表
   - 默认 LlmProvider 切换逻辑
   - 首次启动时创建本机工作区

**验证：**

- [ ] 写单元测试：保存后读出与原对象 equal
- [ ] 写单元测试：损坏的 JSON 文件能恢复为默认值并备份
- [ ] 写单元测试：删除当前 default provider 时自动选举新默认
- [ ] `cargo test` 通过

**提交：** `feat: storage layer with atomic JSON files`

---

## 阶段 2：日志解析与压缩

**目标：** 实现 `logs.rs`，能从本机日志生成 `Summary`。

> **必读**：实现前先读完 [JSONL.md](./JSONL.md) —— 它是 schema 与字段的唯一事实源。
> 解析策略遵循 [DECISIONS.md ADR-011](./DECISIONS.md#adr-011jsonl-解析使用-serde_jsonvalue-而非强类型-struct)：全部用 `serde_json::Value` 容错解析。

**任务：**

1. 实现 `logs::collect_messages` 本机分支：
   - 扫描 `claude_path` 和 `codex_path`（默认 `~/.claude`、`~/.codex`，可被 workspace 覆盖；用 `expand_tilde`）
   - 按文件 mtime 过滤到 since 之后
   - 三个数据源各自有 reader（不要混在一个函数里）：
     - `~/.claude/history.jsonl`（用户 prompt 历史）
     - `~/.claude/projects/<encoded>/*.jsonl`（完整 session）
     - `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`（完整 session）
   - 单行 JSON 解析失败 `warn!` + continue；单文件失败 `warn!` + continue
2. 实现 token 压缩规则（[SPEC.md#3](./SPEC.md#3-核心算法token-压缩策略) + [JSONL.md#8](./JSONL.md#8-token-压缩规则汇总与-spec3-对齐)）：
   - 真实用户 prompt：全文保留
   - AI text / Codex `output_text`：首尾各 N 字符（默认 N=200）
   - Tool use / Codex `function_call`：`[name: 关键参数(≤60)]`；Codex `arguments` 需二次 JSON 解析
   - Tool result / `function_call_output`：丢弃
   - Thinking / Reasoning：丢弃
   - Claude `summary` / `git-commit` / `isMeta`：丢弃
   - Codex `session_meta` / `event_msg` / `compacted` / `turn_context`：丢弃
   - **关键**：Claude `type:"user"` + `toolUseResult`/数组 content 视为 tool result 丢弃
   - 连续相似指令去重（前 30 字符匹配）
3. 实现 `infer_project()`（[JSONL.md#7](./JSONL.md#7-项目名推断-infer_project)）：
   - Claude session JSONL：优先取任一行 `cwd` 末段
   - Claude history.jsonl：取 `project` 字段末段
   - Codex rollout JSONL：取 `session_meta.payload.cwd` 末段
   - 退路：文件名 stem / "Claude Code" / "Codex"
4. 实现 `aggregate()`：把消息分组成 `Summary`（`by_project` / `ai_snippets` / `stats`）

**fixture 测试**（每个数据源至少一个 fixture）：

- `tests/fixtures/claude_history.jsonl` —— 含 1 行真实 prompt + 1 行 number 时间戳
- `tests/fixtures/claude_session_string.jsonl` —— `type:"user"` 字符串 content
- `tests/fixtures/claude_session_array.jsonl` —— assistant 含 text/thinking/tool_use 数组
- `tests/fixtures/claude_session_tool_result.jsonl` —— `type:"user"` + `toolUseResult`（应被丢弃）
- `tests/fixtures/codex_rollout.jsonl` —— `session_meta` + `response_item(message)` + `function_call`

**验证：**

- [ ] fixture 测试：每种 schema 输出符合预期的 `Summary`
- [ ] Claude `type:"user"` + `toolUseResult` 的行不出现在用户指令列表中
- [ ] Codex `function_call.arguments` 内嵌 JSON 串能解析出关键参数
- [ ] 损坏 JSON 行不阻塞整文件
- [ ] 在真实 `~/.claude/history.jsonl` 上运行不崩溃
- [ ] `by_project` 的 key 是真实 cwd 末段（不是 encoded 路径）
- [ ] 输出可读字符串总长度 < 100k 字符（实际工作 7 天）
- [ ] `cargo test` 通过
- [ ] [JSONL.md#10 验证清单](./JSONL.md#10-验证清单) 全部勾选

**提交：** `feat: log parsing and token compression`

---

## 阶段 3：LLM 抽象 + 基础生成

**目标：** 实现 `llm.rs` + `report.rs`，能用任意 LLM 源生成周报。

**任务：**

1. 实现 `llm.rs`：
   - 数据结构 `LlmProvider`、`LlmKind`、`CompletionResult`
   - `complete()` 入口分发到三种协议（详见 [LLM.md](./LLM.md)）
   - 每种协议的请求构造 + 响应解析
   - `test_connection()`
   - `presets()` 返回 10 个预设
2. 实现 `report.rs`：
   - `generate()` 构造 prompt，调用 `llm::complete`
   - prompt 模板见 [SPEC.md#输出格式](./SPEC.md#输出格式)
   - 历史报告作为上下文注入

**验证：**

- [ ] 调用 OpenAI 真实 API 成功（用户提供 key）
- [ ] 调用 Anthropic 真实 API 成功
- [ ] 调用 Gemini 真实 API 成功
- [ ] 错误 key / 错误 model / 网络超时各自报错清晰
- [ ] 见 [LLM.md#11-验证清单](./LLM.md#11-验证清单) 全部勾选

**提交：** `feat: multi-LLM provider abstraction`

---

## 阶段 4：前端基础 + 工作区页 + LLM 源页

**目标：** UI 能用，能配置工作区和 LLM 源。

**任务：**

1. 实现 `src/api.js`：所有 Tauri command 包装
2. 实现 `src/components/ui.jsx`：共用 primitives
3. 实现 `src/App.jsx`：sidebar + 路由切换
4. 实现 `src/components/Workspaces.jsx`：详见 [UI.md#41](./UI.md#41-工作区页-workspacesjsx)
5. 实现 `src/components/Providers.jsx`：详见 [UI.md#42](./UI.md#42-llm-源页-providersjsx)
6. 后端注册对应的 Tauri command（见 [ARCHITECTURE.md#tauri-command-列表](./ARCHITECTURE.md#310-mainrs--tauri-入口)）

**验证：**

- [ ] 启动应用，能添加/编辑/删除工作区
- [ ] 能测试本机工作区（显示路径是否存在）
- [ ] 能添加/编辑/删除 LLM 源
- [ ] 能用 10 个预设一键填充
- [ ] 能测试 LLM 源连接（用真实 API key）
- [ ] 默认 LLM 源切换逻辑正常

**提交：** `feat: workspaces and LLM providers UI`

---

## 阶段 5：模板页 + 历史报告页 + 生成对话框

**目标：** 闭环：能从 UI 触发生成一份完整周报。

**任务：**

1. 实现 `src/components/Templates.jsx`：详见 [UI.md#43](./UI.md#43-周报模板页-templatesjsx)
2. 实现 `src/components/Reports.jsx`：详见 [UI.md#44](./UI.md#44-历史周报页-reportsjsx)
3. 实现 `src/components/GenerateDialog.jsx`：详见 [UI.md#47](./UI.md#47-生成对话框-generatedialogjsx)
4. 后端 `generate_report` command：
   - 接收 GenerateRequest（workspace_ids, template_id, days, provider_id?）
   - 调用 logs → llm → 存档
   - 返回 Markdown + 元数据
5. 集成历史报告作为风格参考（取最近 2 份）

**验证：**

- [ ] 能在生成对话框选工作区/模板/LLM/天数生成周报
- [ ] 生成时显示进度
- [ ] 生成完成后展示 Markdown 预览
- [ ] 复制按钮工作
- [ ] 历史周报页能列出所有报告
- [ ] 点击列表行能查看详情
- [ ] 删除报告时同时删 .md 文件和 index.json 中的条目
- [ ] 第二次生成时上一份报告作为参考被注入 prompt

**提交：** `feat: templates, reports, and generation workflow`

---

## 阶段 6：SSH 远程工作区

**目标：** 支持远程服务器日志。

**任务：**

1. 实现 `ssh.rs`：
   - `test()`：用系统 `ssh` 命令测试连接
   - `sync_to_cache()`：用 `rsync` 同步 *.jsonl 到本地缓存
2. 在 `logs::collect_messages` 加入 SSH 分支：先同步到缓存再走本机解析逻辑
3. 工作区编辑器支持 SSH 字段

**验证：**

- [ ] 添加 SSH 工作区，测试连接成功
- [ ] 测试连接失败时报错清晰（超时 / 认证失败 / host 不通分别给出不同提示）
- [ ] 生成报告时能从远程读到日志
- [ ] 远程缓存目录位置正确（`cache_dir()` 跨平台）

**提交：** `feat: SSH remote workspace support`

---

## 阶段 7：SMTP 邮件 + 设置页

**目标：** 能发邮件。

**任务：**

1. 实现 `email.rs`：
   - `send()` SMTP 发邮件
   - `test_smtp()` 测试连接
   - Markdown → HTML 渲染器（零依赖）
2. 实现 `src/components/Settings.jsx`：详见 [UI.md#46](./UI.md#46-设置页-settingsjsx)
3. 后端注册 SMTP 相关 command

**验证：**

- [ ] 选预设（QQ/Gmail/163）能一键填充
- [ ] 测试连接成功
- [ ] 发测试邮件成功
- [ ] 错密码时报错清晰
- [ ] 测试邮件 HTML 在主流邮箱客户端（Gmail web / iOS Mail）显示正常

**提交：** `feat: SMTP email and settings page`

---

## 阶段 8：定时任务

**目标：** 能定时自动生成 + 发邮件。

**任务：**

1. 实现 `scheduler.rs`：
   - `SchedulerState`、`reload_all`、`add_job`、`remove_job`、`refresh_job`
   - `execute_schedule` 完整流程
   - `next_run_time` 计算
2. 实现 `src/components/Schedules.jsx`：详见 [UI.md#45](./UI.md#45-定时任务页-schedulesjsx)
3. `main.rs` 启动时初始化 scheduler 并 `reload_all`
4. 后端注册定时任务 command

**验证：**

- [ ] 新建一个 1 分钟后执行的任务，1 分钟内自动触发
- [ ] 立即执行按钮工作
- [ ] 任务执行失败时 last_status 显示错误
- [ ] 关闭应用再打开，已启用任务自动恢复
- [ ] 修改任务后下次执行用新参数
- [ ] 启用/禁用 toggle 立即生效

**提交：** `feat: scheduled tasks with cron and email`

---

## 阶段 9：打磨与文档

**目标：** 达到可发布质量。

**任务：**

1. 错误信息全面 review：所有用户可见的错误都人类可读
2. Loading 状态全面 review：所有异步操作都有反馈
3. 空状态全面 review：所有列表为空时有友好提示
4. README.md 完善：截图、徽章、使用说明
5. 写 `docs/CONTRIBUTING.md`
6. 写 `docs/CHANGELOG.md`
7. `cargo fmt` + `cargo clippy -- -D warnings` 全过
8. 性能验证：冷启动 < 2s，生成端到端 < 60s

**验证：**

- [ ] [SPEC.md#6-性能要求](./SPEC.md#6-性能要求) 全部达标
- [ ] 所有阶段的验证项再过一遍
- [ ] 在三个平台（macOS / Linux / Windows）至少各跑一次 dev 模式
- [ ] 创建一个真实周报并通过邮件发送验收

**提交：** `chore: v0.1.0 polish and docs`

---

## 阶段 10：发布准备

**目标：** 能发布 GitHub release。

**任务：**

1. 替换占位图标为真实图标（使用 `cargo tauri icon`）
2. 配置 GitHub Actions（`.github/workflows/build.yml`）：
   - 三平台并行构建
   - PR 触发：cargo test + clippy
   - tag 触发：build + 上传 release artifacts
3. 写 release notes
4. tag `v0.1.0` 触发发布

**验证：**

- [ ] CI 通过
- [ ] 三平台的 release artifact 可下载并运行
- [ ] release notes 完整列出 v0.1.0 功能

**提交：** `release: v0.1.0`

---

## 总览

```
阶段 0  项目骨架            ← 半天
阶段 1  存储 + 数据模型      ← 1 天
阶段 2  日志解析压缩        ← 1.5 天
阶段 3  LLM 抽象            ← 1 天
阶段 4  Workspaces + LLM UI ← 1.5 天
阶段 5  模板 + 报告 + 生成   ← 2 天
阶段 6  SSH 远程            ← 1 天
阶段 7  SMTP + 设置         ← 1 天
阶段 8  定时任务            ← 1.5 天
阶段 9  打磨                ← 1 天
阶段 10 发布                ← 0.5 天
────────────────────────────
总计                        ← 约 12 天 (单人，含调试)
```

Claude Code 通常一次能完成 1-2 个阶段。建议每个阶段单独开会话，避免上下文过长。
