# CLAUDE.md

This file tells Claude Code how to work on the **WeeklyReport** project.
Read this before every significant change.

---

## 项目目标

一个跨平台桌面应用，自动从 Claude Code / Codex CLI 的本地 JSONL 日志生成 AI 周报，并支持定时邮件发送。技术栈：**Tauri 2 + Rust + React 18 + Tailwind CSS**。

**核心要求（写代码时反复回到这四个词）：**
1. **轻量** — 无数据库，依赖最少，二进制 < 10 MB
2. **流畅** — UI 异步加载，长操作不阻塞
3. **稳定** — 文件原子写、错误向上传播、不 panic、明确的失败信号
4. **高商业可用** — 代码质量达到 MIT 开源项目水平

## 必读文档（按顺序）

1. [docs/SPEC.md](./docs/SPEC.md) — 功能规格（必读）
2. [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) — 模块划分、数据模型
3. [docs/LLM.md](./docs/LLM.md) — 多 LLM 源协议抽象（核心机制之一）
4. [docs/UI.md](./docs/UI.md) — UI/UX 规格
5. [docs/TASKS.md](./docs/TASKS.md) — 分阶段任务（按顺序实现）
6. [docs/DECISIONS.md](./docs/DECISIONS.md) — 关键决策与不要做的事

## 工作约定

### 代码风格

**Rust**
- 模块按职责切分，每个模块 < 400 行；超过就拆分
- 错误用 `anyhow::Result` 在应用层，库代码用 `thiserror` 定义具体错误
- 禁止 `unwrap()` 和 `expect()` 在非启动初始化路径之外的代码
- 公共函数必须有文档注释（`///`），说明输入输出和失败条件
- 异步：默认 `tokio`，避免阻塞操作出现在 `async fn` 里
- 文件 I/O 必须原子写（先写 `.tmp` 再 `rename`）
- 必须通过 `cargo fmt` 和 `cargo clippy -- -D warnings`

**React / JS**
- 只用函数组件 + Hooks，不写 class component
- 状态管理：本地 `useState` / `useReducer` 优先，避免引入 Redux/Zustand
- 不引入 CSS-in-JS、styled-components、Emotion；只用 Tailwind 工具类
- 组件 < 200 行；超过拆分
- 异步操作必须有 loading 和 error 状态，不允许只显示空白
- 不写 TypeScript（项目用 JSX，保持轻量）

**通用**
- 注释用中文，代码标识符用英文
- 测试关键路径，但不追求覆盖率
- 不引入新的运行时依赖，除非 `docs/DECISIONS.md` 已记录

### 文件组织（不要偏离）

```
WeeklyReport/
├── README.md
├── CLAUDE.md                    # 本文件
├── LICENSE                      # MIT
├── .gitignore
├── package.json
├── vite.config.js
├── tailwind.config.js
├── postcss.config.js
├── index.html
├── docs/
│   ├── SPEC.md
│   ├── ARCHITECTURE.md
│   ├── LLM.md
│   ├── UI.md
│   ├── TASKS.md
│   └── DECISIONS.md
├── src/                         # 前端 React
│   ├── main.jsx
│   ├── App.jsx
│   ├── api.js                   # Tauri command 包装层
│   ├── index.css                # Tailwind 入口
│   └── components/
│       ├── ui.jsx               # 共用 UI 元件
│       ├── Workspaces.jsx       # 工作区页
│       ├── Providers.jsx        # LLM 源页
│       ├── Templates.jsx        # 模板页
│       ├── Reports.jsx          # 报告页
│       ├── Schedules.jsx        # 定时任务页
│       ├── Settings.jsx         # 设置页
│       └── GenerateDialog.jsx   # 生成对话框
└── src-tauri/                   # 后端 Rust
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── build.rs
    ├── icons/                   # 应用图标
    └── src/
        ├── main.rs              # Tauri 入口 + 命令注册
        ├── store.rs             # JSON 文件原子读写
        ├── state.rs             # 高层 CRUD（在 store 之上）
        ├── workspace.rs         # Workspace 模型 + 连接测试
        ├── ssh.rs               # SSH/rsync 远程同步
        ├── logs.rs              # JSONL 解析 + token 压缩
        ├── llm.rs               # 多 LLM 源协议
        ├── report.rs            # 周报 prompt 构造
        ├── email.rs             # SMTP + Markdown→HTML
        └── scheduler.rs         # cron 定时调度
```

### 不要做的事

- ❌ 不要引入数据库（SQLite、SurrealDB 等）—— JSON 文件就够
- ❌ 不要引入 CSS-in-JS 库 —— 只用 Tailwind
- ❌ 不要引入 TypeScript —— 保持 JSX 轻量
- ❌ 不要在 prompt 里发送工具返回结果 —— token 压缩策略见 [docs/SPEC.md](./docs/SPEC.md#token-压缩策略)
- ❌ 不要硬编码任何 LLM 源 —— 都通过 `LlmProvider` 抽象
- ❌ 不要存明文 API key 之外的任何远程数据 —— 全部本地
- ❌ 不要在 panic 路径上做 I/O —— 用 `Result` 传播

### 测试与验证

每完成一个阶段（见 [docs/TASKS.md](./docs/TASKS.md)），必须：

1. `cargo build --release` 通过
2. `cargo clippy -- -D warnings` 无警告
3. `npm run build` 通过
4. 手动跑通该阶段在 TASKS.md 中标注的验证步骤
5. 该阶段相关的关键路径写至少一个测试

### Git 提交

- 一次提交一个逻辑变更
- 使用 Conventional Commits：`feat:` `fix:` `docs:` `refactor:` `test:` `chore:`
- 提交信息用中文或英文均可，但保持一致

---

## 常用命令

```bash
# 开发
npm run tauri:dev

# 类型检查 / lint
cd src-tauri && cargo clippy -- -D warnings && cargo fmt --check
cd .. && npm run lint    # 如配置了

# 构建
npm run tauri:build

# 测试
cd src-tauri && cargo test
```

## 遇到问题怎么办

1. 检查是否违反「不要做的事」清单
2. 检查相关 docs/*.md 是否已经规定了做法
3. 如果是新决策，写入 [docs/DECISIONS.md](./docs/DECISIONS.md) 再实现
4. 不确定时停下来问用户，不要猜

---

最后一条：**这是一个开源项目，代码质量是核心 KPI**。宁可慢一点写好，不要急着写完。
