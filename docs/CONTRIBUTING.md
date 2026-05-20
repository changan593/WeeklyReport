# 贡献指南 (CONTRIBUTING.md)

欢迎为 WeeklyReport 贡献代码。本文档说明开发流程、规范、提交要求。

---

## 1. 环境

| 依赖                | 版本               | 说明                                              |
| ------------------- | ------------------ | ------------------------------------------------- |
| Node.js             | ≥ 18               | 前端构建                                          |
| Rust                | ≥ 1.75（edition 2021） | 后端                                          |
| `ssh` / `tar`       | 系统命令           | SSH 远程工作区使用；Windows 10 1803+ / macOS / Linux 默认都自带 |
| Linux 系统库        | 见 README          | Tauri 在 Linux 上依赖 webkit2gtk / gtk3           |

---

## 2. 起步

```bash
git clone git@github.com:changan593/WeeklyReport.git
cd WeeklyReport
npm install
npm run tauri:dev          # 启动开发窗口（带 HMR）
```

如果只想验证前端：

```bash
npm run dev                # vite dev server (无 Tauri)
npm run build              # 生产构建到 dist/
```

只想验证后端：

```bash
cd src-tauri
cargo test                 # 单元测试（< 1 秒）
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

---

## 3. 代码规范

### Rust

- 模块按职责切分，每个 `.rs` 文件 < 400 行；超过就拆分到子目录（参考 `logs/` 和 `llm/`）
- 应用层错误用 `anyhow::Result`；库代码用 `thiserror` 定义具体错误类型
- 非启动初始化路径**禁止** `unwrap()` / `expect()`
- 公共函数必须有 `///` 文档注释，说明输入输出和失败条件
- async：默认 `tokio`；阻塞 I/O 用 `tokio::task::spawn_blocking` 包装
- 文件 I/O 必须原子写（先写 `.tmp` 再 `rename`）
- 提交前过：`cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`

### React / JS

- 只用函数组件 + Hooks，不写 class 组件
- 状态管理：本地 `useState` / `useReducer` 优先；跨页面共享放 `App.jsx` 顶层
- **不引入** Redux / Zustand / CSS-in-JS / styled-components / TypeScript / dayjs
- 只用 Tailwind 工具类（stone 色系；详见 `docs/UI.md`）
- 组件 < 200 行；超过拆分
- 所有异步必须有 loading + error 状态，禁止只显示空白

### 通用

- **注释中文 / 标识符英文**
- 测试关键路径，但**不追求覆盖率**
- 不引入新的运行时依赖，除非 `docs/DECISIONS.md` 已记录（dev-dep 例外）
- 不在 panic 路径上做 I/O，用 `Result` 传播

---

## 4. 分阶段开发

实现严格按 `docs/TASKS.md` 的 10 个阶段执行。每个阶段完成后必须：

1. `cargo build --release` 通过
2. `cargo clippy --all-targets -- -D warnings` 无警告
3. `cargo fmt --check` 无 diff
4. `npm run build` 通过
5. 该阶段在 TASKS.md 中列出的验证项全部勾选
6. 关键路径至少一个单元测试 / fixture 测试
7. Git 一次提交，使用 [Conventional Commits](https://www.conventionalcommits.org/)：
   - `feat:` 新功能
   - `fix:` bug 修复
   - `docs:` 文档变更
   - `refactor:` 重构（行为不变）
   - `test:` 新增 / 修改测试
   - `chore:` 构建 / CI / 依赖等

---

## 5. PR 流程

1. Fork 仓库，从 `main` 切分支：`feat/<topic>` 或 `fix/<topic>`
2. 一次 PR 一个逻辑变更（除非依赖紧耦合）
3. PR 描述请说明：
   - 修了什么 / 加了什么
   - 关联 issue（如有）
   - 验证步骤（如何手动复现 / 测试）
4. 通过 CI 后由维护者 review
5. 通常 squash merge 到 main

---

## 6. 不要做的事

详见 `CLAUDE.md`「不要做的事」一节，简述：

- ❌ 引入数据库（SQLite 等）—— JSON 文件够用
- ❌ 引入 CSS-in-JS 库 —— 只用 Tailwind
- ❌ 引入 TypeScript —— 项目刻意保持 JSX 轻量
- ❌ 在 prompt 里发送工具返回结果 —— 见 `docs/JSONL.md` 压缩规则
- ❌ 硬编码 LLM 源 —— 通过 `LlmProvider` 抽象
- ❌ 存任何远程数据 —— 全部本地，包括 API key（明文 JSON，见 ADR-008）
- ❌ 在 panic 路径上做 I/O —— 用 `Result` 传播

---

## 7. 报告 bug

提交 issue 时包含：

- 操作系统（macOS / Linux / Windows）+ 版本
- 复现步骤
- 期望行为 vs 实际行为
- 相关日志（`RUST_LOG=debug npm run tauri:dev` 输出）
- 如能脱敏分享，附上一段触发问题的 JSONL 样本

---

## 8. 项目愿景

WeeklyReport 是一个**有边界**的工具：

- 专注「从 Coding Agent 日志生成周报」一件事
- 拒绝 telemetry、远程同步、协作功能
- 二进制 < 10 MB、冷启动 < 2s、生成 < 60s
- 代码质量达到 MIT 开源项目水平

如果你想加的功能在 `docs/DECISIONS.md` 的路线图里（SE-001 ~ SE-007），欢迎；
不在路线图里、又与上述原则相悖的，请先开 issue 讨论后再写代码。
