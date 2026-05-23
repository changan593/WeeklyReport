# 变更记录 (CHANGELOG.md)

所有重要变更都记录在此。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/)，
版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

---

## [Unreleased]

一次 UI/UX 打磨：工作区与 LLM 源主页加状态徽标；报告 HTML（详情页 / 定时邮件）重做，
带统计卡片与按项目条形图；其余页面做轻量产品向优化。

### Added
- **README 发版速览表**：英文与中文 README 都新增 "Releases / 发版速览" 板块，
  每版一行 highlight，便于点开仓库 30 秒掌握当前版本边界。
- **HTML 报告美化（`email.rs`）**：
  - 新增 `ReportMeta` 数据结构 + `render_html_with_meta(md, meta)` 渲染函数
  - 顶部统计卡片：项目数 / Tokens / LLM 源 / 生成时间，4 个 stat tile
  - 按项目工作量条形图（Top 8，CSS-only，无 JS，邮件客户端兼容）
  - 视觉重做：H1 黑色下划线 / H2 翠绿色左色条 / 改良 code blockquote 样式
  - `tokens_used` 大数字自动 `1.2k / 3.4M` 压缩显示
- **会话级状态徽标**：
  - 工作区卡片：本地→"本地就绪"（emerald）/ SSH→"未测试 → 测试中 → 连接正常 / 失败"
    四态切换；卡片右上加内联"测试"按钮（spinning refresh icon）
  - LLM 源卡片：自动检测配置完整度（`api_key` / `model` / `base_url`），
    显示"缺少 API key"等具体短语；测试成功/失败状态覆盖；卡片右上加内联"测试"按钮
- **次级页面打磨**：
  - 报告列表标题加总数计数；空态加 "生成第一份周报" CTA 按钮
  - 定时任务：SMTP 未配置横幅加 "去设置" 跳转按钮；卡片状态由文本改为
    StatusPill（已停用 / 已调度 / 上次成功 / 上次失败）
  - 模板：卡片自动统计 "已被 N 个定时任务引用"，删除前一目了然

### Changed
- `EmailRequest` 新增可选 `body_meta: Option<ReportMeta>` 字段；scheduler 经
  `GenerationOutput.meta` 传过来，让定时邮件复用美化 HTML
- `report::get_report_html` 对旧报告（无 cached html）自动构造 `ReportMeta`
  从 `ReportRecord` 读取统计信息（无 by_project 快照，所以不渲染条形图）

### Fixed
- 既有单元测试两处编译错误（`LogItem` 缺 `reply/server`、`Settings` 缺 `language`），
  原来阻止 `cargo test` 直接通过，现在 243 个测试全绿

### 设计取舍
- 渲染器仍按 ADR-007 保持自写极简版，新功能只是在 `wrap_html` 外层包装 +
  追加一个 header HTML 片段，未引入 `pulldown-cmark` 等第三方库
- 状态徽标走会话级 React state，**不持久化**——避免给"曾测过"造成假信号；
  下次启动应用，所有 SSH 工作区都重新显示为"未测试"

---

## [0.1.1] - 2026-05-21

发布后第一个修复版，针对 Windows 用户三个开箱即坏的问题，加上 SSH 工作区文档增补。

### Fixed
- **Windows 生成周报弹出黑色控制台窗口**：Tauri 是 GUI 应用，spawn `ssh.exe` / `tar.exe`
  时缺 `CREATE_NO_WINDOW` 标志，Windows 默认会弹控制台（即使已重定向 stdio）。
  新增 `hide_console` helper，apply 到 `build_ssh_command` / `local_tar_command` /
  `ensure_sshpass_installed` 所有子进程入口；非 Windows 平台 no-op。
- **Modal 弹窗"拖选关闭"误触**：在 input 内开始拖选文本，鼠标松开点落到 backdrop
  上时，浏览器把 backdrop 当作 click target 触发 `onClose()`，弹窗被误关。
  改用 `onMouseDown` + `onMouseUp` 双重判断 + `useRef` 跨事件传递标记，
  只有"按下和松开都在 backdrop"才关闭。
- **SSH key 字段填公钥 `.pub` 导致认证失败**：ssh 报 `invalid format` + `Permission
  denied`。前端字段 hint 由 "留空使用系统默认 ~/.ssh/id_rsa" 改为明确点出
  "填私钥路径（不是 .pub 公钥）；留空使用系统默认 ~/.ssh/id_ed25519"。

### Added
- `docs/SSH.md`：SSH 工作区配置完整指南，覆盖 Windows / macOS / Linux 三平台。
  - 公钥免密：`ssh-keygen` 命令、`ssh-copy-id` (Unix) vs PowerShell scp+ssh 两步法
  - 密码认证：sshpass 安装、Windows 不推荐的原因
  - 应用内字段对照表
  - 7 条常见问题（含 §6 Q1.5 专门解释 `Load key "...pub": invalid format`）
- README 与 CLAUDE.md 文档目录加链接到 `docs/SSH.md`。

---

## [0.1.0] - 2026-05-20

首个公开版本，覆盖完整端到端流程：日志收集 → 压缩聚合 → LLM 生成 → 本地存档 → 定时邮件。

### Added

**存储与数据模型（阶段 1）**
- `store.rs` 跨平台 JSON 文件原子读写，损坏文件自动备份为 `.broken-<时间戳>` 并恢复默认值
- `state.rs` 各实体 CRUD：Workspace / Template / LlmProvider / Schedule / SmtpConfig / Settings / ReportRecord
- 默认 LlmProvider 自动选举（新增第一个 → 默认；删除当前默认 → 剩余第一个升级）
- 内置 3 模板始终注入到列表，不持久化（technical / executive / simple）
- 首次启动自动创建本机工作区（指向 `~/.claude` 和 `~/.codex`）
- 报告正文单独存为 `reports/<id>.md`，方便直接打开 / 复制 / 转发

**日志解析与压缩（阶段 2）**
- `logs.rs` 同时支持 Claude Code 和 Codex CLI 两种 schema，按真实源码字段（见 `docs/JSONL.md`）
- 容错策略：单行 / 单文件 / 未知 type 失败 `warn!` 后继续，不阻塞整体
- 时间戳解析：兼容 ISO 8601 字符串 与 Unix epoch（秒 / 毫秒）数字
- 用户指令去重：连续两条前 30 字符相同视为重复，命中"重发"与"上箭头改一改"两种场景
- 项目名推断 `infer_project()`：优先 `cwd` 末段，退路文件名 stem
- Token 压缩规则：用户全文保留、AI 文本首尾 N 字符、tool_use 渲染为 `[name: 关键参数]`、tool_result/thinking/reasoning 完全丢弃
- 关键陷阱：Claude `type:"user"` + `toolUseResult` 是 tool result 反灌，必须丢弃

**多 LLM 源协议抽象（阶段 3）**
- `llm.rs` + 三子模块（OpenAI 兼容 / Anthropic / Gemini）
- 10 个预设：Anthropic / OpenAI / DeepSeek / OpenRouter / Kimi / Qwen / Gemini / 本地 Ollama / 本地 vLLM / 自定义
- `complete()` 120 秒超时；错误信息含 HTTP 状态码与响应 body 前 500 字符
- `test_connection()` 发短 prompt 验证认证 + 模型 ID + 网络
- `extra_headers` 字段支持 OpenRouter 等自定义请求头
- 设计：build_request / parse_response 是**纯函数**，HTTP 发送薄壳子，便于单测

**报告生成工作流（阶段 5）**
- `report::run_generation()` 完整流程编排：模板 / provider 解析 / 工作区收日志 / 聚合 / 历史报告注入 / LLM / 存档
- `report::build_prompt()` 严格按 SPEC#输出格式拼 prompt；项目按指令数倒序，便于 LLM 优先重点项目
- 历史报告作为风格参考（默认最近 2 份）

**SSH 远程工作区（阶段 6）**
- `ssh.rs` 使用系统 `ssh` + `tar`（ADR-010 + ADR-013）
- 安全选项：`BatchMode=yes` + `StrictHostKeyChecking=no` + `ConnectTimeout=8`
- `sync_to_cache()` 用 `ssh ... 'tar c' | tar x` 单向流，只拉 `*.jsonl` 文件到 OS 缓存目录，保留目录结构
- Windows 本地 tar 优先 `%SystemRoot%\System32\tar.exe`，规避 MSYS2 tar 与 Win32 ssh 的 pipe 不兼容
- 错误信息中文化：超时 / 认证失败 / DNS / 网络不可达分别给出不同提示

**SMTP 邮件（阶段 7）**
- `email.rs` 用 `lettre` 发送；同时附 plain text + HTML 副本（multipart/alternative）
- 自动按 `use_ssl` 选 SSL/TLS 或 STARTTLS
- 零依赖 Markdown→HTML 渲染器（ADR-007，< 250 行）：标题 / 列表 / 引用 / 粗体 / 代码 / 分隔线
- HTML 含内联 CSS `<style>`，适配 Gmail / iOS Mail 等主流客户端

**定时任务（阶段 8）**
- `scheduler.rs` 包装 `tokio-cron-scheduler`；启动时 `reload_all()` 自动恢复所有 enabled 任务
- Cron 用 **7 段**格式（秒 分 时 日 月 星期 年），ADR-009
- `execute_schedule()` 完整流程：生成 → 邮件 → 更新 `last_run` / `last_status`；失败不阻塞调度器，下次仍触发
- `render_subject()` 支持 `{date}` / `{week}` 变量
- 立即执行按钮 + 启用/禁用 Toggle 实时生效

**前端 UI（阶段 4 / 5 / 7 / 8）**
- React 18 + Tailwind CSS 3 + Vite 5
- 6 个一级页面 + 生成对话框（4 步 step：config / generating / done / error）
- 22 个内置 SVG 图标（line-stroke 风格，stroke-width 1.6）
- 共用 primitives：Modal / FormField / Input / Mono / Select / Toggle / StatusBanner / EmptyState / LoadingState
- 严格按 `docs/UI.md` 规范实现（stone 色系，warm gray）
- `useAsyncState` 统一处理 loading / error / reload

**文档**
- `docs/SPEC.md` 完整功能规格
- `docs/ARCHITECTURE.md` 技术架构与模块划分
- `docs/LLM.md` 多 LLM 源协议抽象与 10 个预设清单
- `docs/JSONL.md` Claude Code / Codex CLI 日志 schema（基于真实源码 / 社区适配器）
- `docs/UI.md` UI/UX 规格
- `docs/TASKS.md` 分阶段实现计划（10 阶段）
- `docs/DECISIONS.md` 11 条 ADR
- `docs/CONTRIBUTING.md` 贡献指南

### 不在 v0.1.0 范围

详见 `docs/DECISIONS.md` 路线图（SE-001 ~ SE-007）：

- 系统托盘最小化常驻
- 多语言支持 / WebDAV / iCloud 同步
- Slack / 钉钉 / 飞书机器人推送
- 失败重试 + 系统通知
- API key keyring 加密（v0.1.0 接受明文存储，见 ADR-008）
- Web 部署版本

### 测试

- 177 单元测试，覆盖：
  - 存储层 round-trip / 损坏 JSON 恢复
  - 日志解析（5 个 fixture 文件：Claude history / session 字符串 / session 数组 / tool_result 反灌 / Codex rollout）
  - Token 压缩 / 用户指令去重
  - LLM 三协议 build_request + parse_response 纯函数
  - 报告 prompt 拼装（统计 / 项目排序 / past_reports 注入 / 边界 case）
  - SSH 命令参数构造 + 错误信息识别
  - 邮件 Markdown→HTML 渲染（标题 / 列表 / 引用 / 内联粗体 / 代码 / 转义）
  - 定时任务 cron 解析 / subject 模板替换 / SchedulerState 基础
- 4 个 live API 测试（`#[ignore]`，需 env var）：Anthropic / OpenAI / Gemini / bad key

---

[Unreleased]: https://github.com/changan593/WeeklyReport/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/changan593/WeeklyReport/releases/tag/v0.1.0
