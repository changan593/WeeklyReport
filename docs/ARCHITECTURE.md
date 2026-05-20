# 架构 (ARCHITECTURE.md)

> 本文档描述 WeeklyReport 的技术架构和模块设计。
> 实现细节由 Claude Code 决定，但本文档定义的边界不可越过。

---

## 1. 总体架构

```
┌────────────────────────────────────────────────────────────┐
│                        前端 (React)                        │
│   sidebar nav + 6 pages + generate dialog                  │
└──────────────────┬─────────────────────────────────────────┘
                   │ Tauri IPC (invoke)
┌──────────────────▼─────────────────────────────────────────┐
│                    后端 (Rust / Tauri)                     │
│  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌──────────┐ │
│  │workspace│ │ logs   │ │  llm   │ │ report │ │scheduler │ │
│  └────────┘ └────────┘ └────────┘ └────────┘ └──────────┘ │
│  ┌────────┐ ┌────────┐ ┌────────┐                          │
│  │  ssh   │ │ email  │ │  state │                          │
│  └────────┘ └────────┘ └────────┘                          │
│                  ▲                                          │
│                  │                                          │
│  ┌───────────────┴────────────────┐                        │
│  │    store (JSON 文件原子读写)    │                        │
│  └────────────────────────────────┘                        │
└────────────────────────────────────────────────────────────┘
                   ▲
                   │
┌──────────────────┴─────────────────────────────────────────┐
│        OS 文件系统 (用户配置目录 / 缓存目录)                │
└────────────────────────────────────────────────────────────┘
```

## 2. 技术栈

| 层      | 选择                       | 备注                                |
| ------- | -------------------------- | ----------------------------------- |
| 桌面框架| **Tauri 2**                | 比 Electron 小 10x，原生窗口        |
| 后端语言| **Rust** (edition 2021)    | 安全、快、单二进制                   |
| 前端框架| **React 18**               | 函数组件 + Hooks                    |
| 样式    | **Tailwind CSS 3**         | 工具类，无 CSS-in-JS                |
| 构建    | **Vite 5**                 | 前端打包                            |
| 状态存储| **JSON 文件**              | 无数据库                            |

## 3. Rust 后端模块

每个模块单一职责，文件 < 400 行。

### 3.1 `store.rs` — 文件存储基础层

**职责：** JSON 文件的原子读写。

**核心 API：**

```rust
pub fn init() -> Result<()>;                       // 初始化数据目录
pub fn data_dir() -> Result<PathBuf>;              // 获取数据目录
pub fn read_json<T: DeserializeOwned + Default>(filename: &str) -> Result<T>;
pub fn write_json<T: Serialize>(filename: &str, value: &T) -> Result<()>;
pub fn save_report_file(id: &str, content: &str) -> Result<PathBuf>;
pub fn load_report_file(id: &str) -> Result<String>;
pub fn delete_report_file(id: &str) -> Result<()>;
```

**关键实现要点：**

- 写入前先写到 `.tmp` 文件，然后 `rename` 到目标路径（atomic on POSIX 和 Windows）
- 读取时文件不存在返回 `T::default()`，不报错
- 解析失败时**备份**损坏文件为 `<filename>.broken-<timestamp>`，再返回默认值
- 数据目录按平台自动选择：
  - macOS: `~/Library/Application Support/WeeklyReport/`
  - Linux: `~/.config/weekly-report/`
  - Windows: `%APPDATA%\WeeklyReport\`

### 3.2 `state.rs` — 高层 CRUD

**职责：** 在 `store.rs` 之上提供业务逻辑（默认值注入、ID 生成、内置数据保护等）。

每类实体一组函数：

```rust
pub fn list_workspaces() -> Result<Vec<Workspace>>;
pub fn save_workspace(ws: Workspace) -> Result<Workspace>;
pub fn delete_workspace(id: &str) -> Result<()>;
// ...各实体同款 list/save/delete
```

**关键逻辑：**

- `list_templates` 始终把内置模板注入到列表里（即使 `templates.json` 不存在）
- 写入 `templates.json` 时过滤掉 `builtin: true` 的（内置不持久化）
- 删除模板/provider 时检查是否有依赖（被定时任务引用），有则拒绝
- 新增 LlmProvider 时若是第一个，自动设为 default
- 设置某个 provider 为 default 时，其他自动取消

### 3.3 `workspace.rs` — Workspace 模型与本地路径检测

**职责：** Workspace 数据结构 + 本地路径展开 + 连接测试。

**核心 API：**

```rust
pub fn expand_tilde(path: &str) -> String;
pub async fn test_connection(ws: &Workspace) -> Result<String>;
```

`test_connection` 行为：
- 本机：检查 claude_path 和 codex_path 是否存在，返回多行报告
- SSH：调用 `ssh::test()`

### 3.4 `ssh.rs` — SSH 客户端

**职责：** 远程 SSH 测试 + rsync 日志同步。

**实现要点：**

- **不**用 `ssh2` crate（编译复杂），改用系统 `ssh` 和 `rsync` 命令的 subprocess
- 用 `tokio::process::Command` 异步执行
- 设置 `StrictHostKeyChecking=no` + `ConnectTimeout=8` + `BatchMode=yes`
- rsync 只拉 `*.jsonl` 文件到本地缓存目录

**核心 API：**

```rust
pub async fn test(ws: &Workspace) -> Result<String>;
pub async fn sync_to_cache(ws: &Workspace) -> Result<HashMap<String, PathBuf>>;
```

### 3.5 `logs.rs` — 日志解析与压缩 (核心)

**职责：** 实现 [SPEC.md#token-压缩策略](./SPEC.md#3-核心算法token-压缩策略)。

**核心 API：**

```rust
pub async fn collect_messages(ws: &Workspace, days: u32) -> Result<Vec<Message>>;
pub fn aggregate(messages: Vec<Message>) -> Summary;
```

`Message` 结构：

```rust
pub struct Message {
    pub role:    String,           // "user" | "assistant"
    pub text:    String,
    pub ts:      Option<DateTime<Local>>,
    pub project: String,
    pub tool:    String,            // "claude-code" | "codex"
    pub server:  String,            // workspace.name
}
```

`Summary` 结构：

```rust
pub struct Summary {
    pub by_project:  HashMap<String, Vec<String>>,    // 项目名 → 用户指令列表
    pub ai_snippets: Vec<String>,                     // 少量 AI 回复片段（用于上下文）
    pub stats:       SummaryStats,                    // 总指令数、活跃天数、项目数等
}
```

**JSONL 格式适配（完整定义见 [JSONL.md](./JSONL.md)）：**

- **Claude Code `history.jsonl`**：仅用户 prompt，字段 `display` / `timestamp(ms)` / `project` / `pastedContents`
- **Claude Code 项目 session JSONL**（`projects/<encoded-cwd>/<sessionId>.jsonl`）：每行顶层 `type` + `message{role,content}`；`content` 是字符串或 typed-block 数组（`text` / `tool_use` / `tool_result` / `thinking`）；**陷阱**：`type:"user"` + `toolUseResult` 是 tool result 反灌
- **Codex rollout JSONL**：`{timestamp, type, payload}` 两段嵌套；`type` 一级（`session_meta` / `response_item` / `event_msg` / `compacted` / `turn_context`）；`response_item.payload.type` 二级（`message` / `reasoning` / `function_call` / `function_call_output` / ...）；message content 永远是数组（`input_text` / `output_text` / `input_image`）

**实现注意：**
- 用 `serde_json::Value` 灵活解析，不硬编码 struct（两边 schema 在版本间漂移，Codex 已有 3 套兼容格式）
- 文件按修改时间筛选，只保留 since 之内的；单行/单文件失败 `warn!` 后 continue，不阻塞整体
- `infer_project()` **优先取 `cwd` 末段**（Claude session JSONL 的 `cwd` 字段、Codex `session_meta.payload.cwd`）；history.jsonl 退路用 `project` 末段；最后退路用文件名 stem
- 输出的 user prompt 全文保留；assistant `text` / Codex `output_text` 截首尾各 N 字符（默认 200）
- Tool use / function_call 输出形如 `[Read: src/scheduler.rs]`，参数长度上限 60 字符
- Codex `function_call.arguments` 是 JSON 字符串，需 `serde_json::from_str` 二次解析
- Claude `type:"user"` 真伪判别：`message.content` 必须是字符串、`toolUseResult` 必须不存在、`isMeta` 必须不为 true

### 3.6 `llm.rs` — 多 LLM 源协议抽象

详见 [LLM.md](./LLM.md)。

**核心 API：**

```rust
pub async fn complete(provider: &LlmProvider, prompt: &str) -> Result<CompletionResult>;
pub async fn test_connection(provider: &LlmProvider) -> Result<String>;
pub fn presets() -> Vec<(&'static str, LlmProvider)>;
```

### 3.7 `report.rs` — 周报 prompt 构造

**职责：** 把 `Summary` + `Template` + 历史周报 → prompt 字符串，调用 `llm::complete`，返回 Markdown + 元数据。

**核心 API：**

```rust
pub async fn generate(
    summary: &Summary,
    template: &Template,
    past_reports: &[String],   // 最近 N 份历史报告，用作风格参考
    provider: &LlmProvider,
) -> Result<(String, u32, u64)>;   // (markdown, tokens_used, duration_ms)
```

**prompt 模板：** 见 [SPEC.md#输出格式](./SPEC.md#输出格式)

### 3.8 `email.rs` — SMTP 发送

**职责：** SMTP 发邮件 + Markdown → HTML 渲染。

依赖：`lettre`（仅 `smtp-transport` + `tokio1-rustls-tls` + `builder` features）。

**核心 API：**

```rust
pub async fn send(cfg: &SmtpConfig, req: &EmailRequest) -> Result<()>;
pub async fn test_smtp(cfg: &SmtpConfig) -> Result<String>;
```

**Markdown → HTML 实现：**
- **不引入第三方 markdown 库**，自己写一个极简渲染器（< 100 行）
- 支持：# / ## / ### 标题、**粗体**、`code`、列表、> 引用、空行段落、---
- 输出带 `<style>` 的完整 HTML，适配邮件客户端
- 同时附 plain text 副本（multipart/alternative）

### 3.9 `scheduler.rs` — 定时调度

**职责：** 用 `tokio-cron-scheduler` 跑后台 cron 任务。

**核心 API：**

```rust
pub struct SchedulerState { /* ... */ }
impl SchedulerState {
    pub async fn new() -> Result<Self>;
    pub async fn reload_all(&self) -> Result<()>;           // 启动时加载所有 enabled 任务
    pub async fn refresh_job(&self, sch: Schedule) -> Result<()>;
    pub async fn remove_job(&self, id: &str) -> Result<()>;
}
pub async fn execute_schedule(sch: &Schedule) -> Result<()>;
pub fn next_run_time(cron: &str) -> Option<String>;
```

**任务执行流程：**
1. 加载工作区 → 拉日志 → 压缩
2. 加载模板 → 加载 LLM provider（优先级：schedule 指定 > template 指定 > 默认）
3. 调用 `report::generate`
4. 存档 → 发邮件
5. 更新 `last_run` / `last_status`

**关键实现：**
- Cron 格式：7 段（秒 分 时 日 月 星期 年）
- 失败不阻塞调度器，下次仍触发
- 应用关闭时自动释放（tokio runtime 销毁即可）

### 3.10 `main.rs` — Tauri 入口

**职责：** 注册所有 Tauri command + 初始化 storage 和 scheduler。

**Tauri command 列表（完整）：**

```
// Workspaces
list_workspaces, save_workspace, delete_workspace, test_workspace_connection

// Templates
list_templates, save_template, delete_template

// LLM Providers
list_providers, save_provider, delete_provider, test_provider, llm_presets

// Reports
list_reports, get_report, delete_report

// Generate
generate_report

// SMTP
get_smtp_config, save_smtp_config, test_smtp_config,
send_test_email, send_report_email

// Schedules
list_schedules, save_schedule, delete_schedule, run_schedule_now

// Misc
data_dir_path, get_settings, save_settings
```

---

## 4. 前端架构

### 4.1 组件树

```
App.jsx (sidebar + content)
├── Workspaces.jsx
├── Providers.jsx
├── Templates.jsx
├── Reports.jsx
├── Schedules.jsx
├── Settings.jsx
└── GenerateDialog.jsx (modal)

ui.jsx (shared primitives, used by all pages)
api.js (Tauri invoke 包装)
```

### 4.2 状态管理

**不引入 Redux / Zustand。** 每个页面用本地 `useState` 管理自己的状态。

跨页共享的状态（如「当前选中的页面」）放在 `App.jsx` 顶层。

异步加载用统一的 hook：

```jsx
function useAsyncState(loader, deps = []) {
  // 返回 [data, loading, reload]
}
```

### 4.3 UI 设计原则

详见 [UI.md](./UI.md)。简述：

- 配色：stone 系列（warm gray），高对比度
- 排版：默认中文 UI，字号 12.5 - 14 px 为主
- 导航：左侧 56px 宽侧边栏，6 个一级页面
- 卡片：白底 + stone-200 边框，hover 变 stone-300
- 按钮：主操作 stone-900 实心，次操作描边
- 表单：text-input 用 stone-200 边框，focus 时 stone-400

---

## 5. 数据存储

```
{config_dir}/WeeklyReport/
├── workspaces.json              # 工作区数组
├── templates.json               # 自定义模板数组（内置不存）
├── schedules.json               # 定时任务数组
├── llm_providers.json           # LLM 源数组
├── smtp.json                    # SMTP 配置对象
├── settings.json                # 通用设置对象
└── reports/
    ├── index.json               # 历史报告元数据数组（不含 content）
    ├── <uuid-1>.md              # 报告 1 正文
    ├── <uuid-2>.md              # 报告 2 正文
    └── ...
```

**为什么报告正文单独存为 .md？**
- 直接可读，不需要应用打开
- 可单独复制、转发
- index.json 保持小，列表加载快

**写入策略：** 原子写（先写 `.tmp`，再 `rename`）。

**JSON 格式：** 缩进 2 空格的 pretty JSON，便于 git diff。

---

## 6. 关键依赖（Rust）

`Cargo.toml` 中**只允许**以下依赖（精简到必要）：

| Crate                   | 用途                          |
| ----------------------- | ----------------------------- |
| `tauri = "2"`           | 桌面框架                       |
| `tauri-plugin-shell`    | 打开外部链接                   |
| `tauri-plugin-dialog`   | 系统对话框                     |
| `tauri-plugin-fs`       | 文件操作                       |
| `tauri-plugin-clipboard-manager` | 剪贴板                |
| `tokio`                 | async runtime                  |
| `serde` + `serde_json`  | 序列化                         |
| `chrono`                | 时间                           |
| `dirs`                  | 跨平台目录                     |
| `walkdir`               | 递归目录扫描                   |
| `anyhow` + `thiserror`  | 错误处理                       |
| `reqwest`               | HTTP（LLM API 调用）           |
| `lettre`                | SMTP                           |
| `tokio-cron-scheduler` + `cron` | 定时任务                |
| `uuid`                  | ID 生成                        |
| `tracing` + `tracing-subscriber` | 日志                  |

**禁止引入：** SQLite (`rusqlite`), markdown 库 (`pulldown-cmark`), CSS-in-JS, Redux 等。

---

## 7. 关键依赖（JS）

`package.json` 中**只允许**：

```json
{
  "dependencies": {
    "react": "^18.3.1",
    "react-dom": "^18.3.1",
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-shell": "^2",
    "@tauri-apps/plugin-dialog": "^2",
    "@tauri-apps/plugin-clipboard-manager": "^2"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2",
    "@vitejs/plugin-react": "^4",
    "vite": "^5",
    "tailwindcss": "^3",
    "autoprefixer": "^10",
    "postcss": "^8"
  }
}
```

**不引入：** TypeScript, Redux, Zustand, Emotion, styled-components, antd, MUI, dayjs 等。

---

## 8. 构建与发布

### 8.1 开发模式

```bash
npm install
npm run tauri:dev          # 启动 Vite + Tauri 开发窗口
```

### 8.2 发布构建

```bash
npm run tauri:build        # 输出 src-tauri/target/release/bundle/
```

各平台产物：
- macOS: `.dmg` + `.app`
- Linux: `.AppImage` + `.deb`
- Windows: `.msi` + `.exe`

### 8.3 CI（v0.1.0 后续添加）

GitHub Actions 工作流（`.github/workflows/build.yml`）应在 push 到 main 时：
- 在三平台并行构建
- 运行 `cargo test` + `cargo clippy`
- 打 tag 时自动发布 release，附带产物

详见 [TASKS.md](./TASKS.md#阶段-6-发布准备)。
