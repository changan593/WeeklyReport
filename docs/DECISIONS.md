# 架构决策记录 (DECISIONS.md)

> 重要技术决策和它们的理由。
> 新增决策时附在文末。修改既有决策需说明原因。

---

## ADR-001：不使用数据库，用 JSON 文件存储

**状态：** ✅ 已接受

**背景：**

WeeklyReport 的数据规模很小：
- 工作区：典型 1-10 个
- 模板：典型 3-10 个
- LLM 源：典型 1-5 个
- 定时任务：典型 1-3 个
- 历史报告：典型 50-500 份/年

总数据量级：< 10 MB。

**选项：**

| 选项                  | 优点                  | 缺点                          |
| --------------------- | --------------------- | ----------------------------- |
| SQLite                | 成熟、ACID、查询灵活  | 编译复杂、+1MB 二进制、用户不可直接备份 |
| JSON 文件             | 极简、人类可读、可 Git 同步 | 并发写要小心、复杂查询难 |
| SurrealDB / sled      | 嵌入式 + 灵活         | 增加学习成本、规模过剩         |

**决策：** 用 JSON 文件。

**理由：**
- 数据量极小，不需要数据库的查询能力
- 用户可直接 `cat` / `vim` 查看修改备份
- 可以放进 Git 同步到云端
- 减少编译复杂度（不需要 C 编译器编 SQLite）
- 减少二进制体积 ~1 MB

**实现：**
- 每类数据一个文件
- 原子写入（`.tmp` + `rename`）
- 损坏时自动备份并恢复默认值
- 历史报告正文单独存 `.md` 文件，便于直接打开

**何时考虑迁移：**
- 数据量超过 100 MB
- 需要复杂查询（如「找出 2024 年所有用 OpenAI 生成的报告」）
- 多进程并发写（当前是单进程单线程，不会有竞争）

---

## ADR-002：多 LLM 源支持，三协议抽象

**状态：** ✅ 已接受

**背景：**

主流 LLM API 协议分三类：
1. OpenAI 兼容（OpenAI、DeepSeek、Kimi、Qwen、OpenRouter、Groq、Ollama、vLLM）
2. Anthropic 原生
3. Google Gemini 原生

如果只支持 Anthropic（最初的方案），用户会问「为啥不能用 OpenAI / DeepSeek」。

**选项：**

| 选项                | 描述                                      |
| ------------------- | ----------------------------------------- |
| 只支持 Anthropic    | 简单，但局限大                            |
| 一个适配器一种 API  | 每加一种 API 写一份新代码                 |
| 三协议抽象          | 三种实现，无数 API 都套用其中之一         |

**决策：** 三协议抽象。

**理由：**
- 实际上几乎所有 LLM API 都是这三协议之一
- 新增同协议 API 只需加预设，零代码
- 用户体验上，「我有 DeepSeek 想用」直接选预设
- 维护成本低，三份请求/响应代码不会再增

**实现细节：** 见 [LLM.md](./LLM.md)。

---

## ADR-003：不使用 Electron，选择 Tauri 2

**状态：** ✅ 已接受

**理由：**

| 维度       | Electron       | Tauri 2           |
| ---------- | -------------- | ----------------- |
| 二进制大小 | ~150 MB        | ~10 MB            |
| 内存占用   | ~200 MB        | ~50 MB            |
| 启动时间   | 1-2s           | < 0.5s            |
| 后端语言   | Node.js        | Rust（更适合系统编程） |
| 安全       | 默认全权限     | 默认沙箱          |
| 跨平台     | ✓              | ✓                 |

对一个目标「轻量、流畅、稳定」的项目，Tauri 是显然的选择。

**风险：** Tauri 生态小于 Electron，某些 npm 库可能不能直接用。但本项目前端依赖极简，不构成问题。

---

## ADR-004：不使用 TypeScript

**状态：** ✅ 已接受

**理由：**
- 项目前端代码量小（< 2000 行）
- Tauri command 的类型从后端自动校验（通过 Tauri IPC 序列化）
- 增加 TS 配置和编译步骤增加复杂度
- 团队（个人项目）不需要 TS 带来的类型安全收益

**何时考虑改 TS：**
- 项目变大（> 5000 行 JS）
- 多人协作
- IDE 自动补全成为生产力瓶颈

---

## ADR-005：不使用 CSS-in-JS

**状态：** ✅ 已接受

**理由：**
- Tailwind CSS 已经能解决所有样式需求
- CSS-in-JS（styled-components / Emotion）增加 ~50KB 依赖
- 运行时计算样式损耗性能
- 与「轻量」目标相悖

**保留例外：** 如果需要动态计算的样式，用内联 `style={}` 即可，不引入新依赖。

---

## ADR-006：不引入 Redux / Zustand 等全局状态库

**状态：** ✅ 已接受

**理由：**
- 每个页面状态独立
- 跨页共享的状态极少（只有「当前页签」）
- `useState` / `useReducer` 足够
- 全局状态库往往让代码更复杂而非更简单

**实现模式：**
- 页面级状态用 `useState`
- 异步加载用统一 `useAsyncState` hook
- 跨页通信通过 Tauri command（数据源是后端文件）

---

## ADR-007：不写复杂 Markdown 渲染器

**状态：** ✅ 已接受

**背景：**

邮件发送时需要把 Markdown 周报转 HTML。

**选项：**
1. 引入 `pulldown-cmark` 等成熟 markdown 库
2. 自己写极简渲染器（< 100 行）

**决策：** 自己写。

**理由：**
- 周报 Markdown 用法非常有限（标题/列表/粗体/code）
- 引入完整库增加二进制 ~500KB
- 自写代码可读，bug 易修
- 不追求完整 CommonMark 兼容

**支持的语法：**
- `# / ## / ###` 标题
- `**bold**` 粗体
- `` `code` `` 代码
- `- / *` 列表
- `> quote` 引用
- `---` 分隔线
- 空行 → 段落

**不支持：** 链接、图片、表格、嵌套列表、HTML 内嵌。

---

## ADR-008：API key 明文存储（v0.1.0）

**状态：** ⚠️ 临时接受，路线图中改

**背景：**

LLM API key 和 SMTP 密码需要持久化。

**选项：**

| 方案                          | 安全性 | 复杂度 | 跨平台 |
| ----------------------------- | ------ | ------ | ------ |
| 明文 JSON                     | 低     | 零     | ✓      |
| 简单加密（AES，密钥硬编码）   | 假安全 | 低     | ✓      |
| OS keyring (macOS Keychain etc.) | 高 | 中     | ✓      |

**决策（v0.1.0）：** 明文存储。

**理由：**
- 数据已经在用户自己的电脑上、用户自己的目录里
- 假加密反而误导用户
- keyring 涉及平台特定 API，v0.1.0 范围太大
- 用户文档明确说明：「这些 key 以明文存在 `~/.config/weekly-report/llm_providers.json`」

**路线图：** v0.2.0 用 `keyring` crate 加密存储敏感字段。

---

## ADR-009：定时任务调度器使用 tokio-cron-scheduler

**状态：** ✅ 已接受

**背景：**

需要后台 cron-style 定时任务。

**选项：**
| 库                       | 优点                  | 缺点                          |
| ------------------------ | --------------------- | ----------------------------- |
| `tokio-cron-scheduler`   | tokio 原生，活跃维护  | API 稍重                      |
| 自己写 sleep 循环        | 零依赖                | 失去 cron 表达式能力          |
| 系统 cron                | 最稳                  | 跨平台是噩梦                  |

**决策：** `tokio-cron-scheduler` + `cron` 解析。

**注意：** 7 段 cron 格式（含秒和年），与系统 cron 的 5/6 段不一样。前端 UI 必须明确显示这一点。

---

## ADR-010：SSH 使用系统命令而非 ssh2 crate

**状态：** ✅ 已接受

**理由：**
- `ssh2` crate 依赖 libssh2 + libssl 编译，麻烦且经常出问题
- 用户的电脑必然已有 `ssh` 和 `rsync` 命令
- subprocess 调用简单可靠
- 缺点：Windows 默认没有 rsync（但有 OpenSSH）→ Windows 用户需要自己装 rsync，README 中说明

---

## ADR-011：JSONL 解析使用 `serde_json::Value` 而非强类型 struct

**状态：** ✅ 已接受

**背景：**

实现 `logs.rs` 时需要解析 Claude Code 和 OpenAI Codex CLI 的 JSONL 日志。
通过读 [openai/codex 源码](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)
与 Claude Code 社区适配器，我们发现：

- 两边 schema 完全不同：Claude 顶层 `type` + `message{content}`；Codex `RolloutLine { timestamp, type, payload }` 两段嵌套
- 同一 CLI 在版本间 schema 会漂移（Codex 已知 ≥0.44 / mid / 2025-08 三套 session_meta 格式）
- 字段呈"半结构化"：Claude `message.content` 可以是 string 也可以是 typed-block 数组；Codex `function_call.arguments` 是嵌套 JSON 字符串
- 每个 CLI 都有罕见 `type` 值（Claude `summary` / `git-commit`、Codex `event_msg` / `compaction*`），未来还可能增加

**选项：**

| 方案                              | 优点                | 缺点                            |
| --------------------------------- | ------------------- | ------------------------------- |
| 为每种格式定义强类型 struct       | 类型安全、IDE 自动补全 | 任何 schema 变动都要改代码并重新发版 |
| 引入 codex/claude 各自的 protocol crate | 直接复用上游定义 | 引入大量重型依赖，违反"轻量"目标 |
| 全部用 `serde_json::Value` 容错读取 | 一次写完，版本漂移无感 | 失去编译期类型检查              |

**决策：** 用 `serde_json::Value` + 按 `type` 字符串分支。

**理由：**
- 解析失败不应阻塞整个收集过程；强类型 struct 一旦上游加字段就反序列化失败
- 我们只关心一小部分字段（user prompt、assistant text、tool_use 名+参数），其他全丢
- 单行/单文件失败 `warn!` 后 `continue`，整体仍能产出可用 Summary
- 当 Codex 或 Claude Code 升级 schema 时，多半不需要改我们的代码

**实现约束（强制）：**
- 任何 `Value::get(...)` 缺失时走默认值，不 unwrap
- 未知 `type` 静默跳过，不报错
- 单行 JSON 解析失败 → `warn!` + continue
- 详细字段表 + 版本兼容矩阵见 [JSONL.md](./JSONL.md)

---

## 路线图（未来决策）

以下条目尚未做正式决策，待 v0.2.0+ 评估：

- **SE-001** 系统托盘最小化常驻
- **SE-002** WebDAV / iCloud 配置同步
- **SE-003** 多语言支持（i18n）
- **SE-004** Slack / 钉钉 / 飞书机器人推送
- **SE-005** 失败重试 + 系统通知
- **SE-006** API key keyring 加密
- **SE-007** Web 部署版本

每条立项时新建 ADR 并更新本文件。
