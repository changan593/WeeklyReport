# WeeklyReport

> 跨平台桌面应用：从 Claude Code / Codex CLI 的对话日志，自动生成 AI 周报，定时发送邮件。

<p align="center">
  <img alt="license" src="https://img.shields.io/badge/license-MIT-blue.svg">
  <img alt="platform" src="https://img.shields.io/badge/platform-macOS%20|%20Linux%20|%20Windows-lightgrey">
  <img alt="tauri" src="https://img.shields.io/badge/tauri-2-orange">
  <img alt="rust" src="https://img.shields.io/badge/rust-1.75+-dea584">
  <img alt="react" src="https://img.shields.io/badge/react-18-61dafb">
</p>

## 为什么做这个

每周写周报很痛苦，但你每天与 AI 编程工具（Claude Code、Codex CLI）的交互记录其实已经是你工作的真实信号——只是这些日志分散在不同机器的 JSONL 文件里，又长又杂。这个工具把它们聚合、压缩、用 LLM 生成成稿，每周自动发到老板邮箱。

## 特性

- 🪶 **极致轻量** — 无数据库依赖，JSON 文件存储，发布包 < 10 MB
- 🌐 **多 LLM 源** — OpenAI / Anthropic / Gemini / DeepSeek / OpenRouter / Kimi / Qwen / 本地 Ollama / vLLM，内置 10 个预设
- 🖥️ **多工作区** — 本机 + N 个 SSH 远程服务器，统一聚合日志
- 🧠 **风格记忆** — 历史周报作为下次生成的参考，保持风格连贯
- ⏰ **定时邮件** — cron 表达式 + SMTP，每周自动生成并发送
- 🔒 **数据自主** — 全部本地存储；可备份、可 Git 同步、可迁移
- 🌍 **跨平台** — macOS / Linux / Windows，Tauri 2 原生构建

## 快速开始

### 环境要求

- [Node.js](https://nodejs.org/) ≥ 18
- [Rust](https://rustup.rs/) ≥ 1.75
- Linux 额外依赖：

  ```bash
  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget \
                   file libssl-dev libgtk-3-dev libayatana-appindicator3-dev \
                   librsvg2-dev
  ```

### 安装与运行

```bash
git clone git@github.com:changan593/WeeklyReport.git
cd WeeklyReport
npm install
npm run tauri:dev          # 开发模式
npm run tauri:build        # 打包发布版
```

发布版产物位于 `src-tauri/target/release/bundle/`，按平台输出 `.dmg` / `.AppImage` / `.deb` / `.msi`。

## 使用流程

1. **配置 LLM 源** — 在「LLM 源」页面选预设（DeepSeek / OpenAI / 本地 Ollama 等），填入 API key
2. **添加工作区** — 默认自动加一个本机工作区；需要远程则添加 SSH 工作区
3. **配置 SMTP（可选）** — 「设置」页面填邮箱信息，预设了 Gmail / QQ / 163 / 企业微信
4. **手动生成测试** — 侧边栏「生成周报」按钮，确认效果
5. **定时任务** — 「定时任务」页面新建任务，cron 时间 + 收件人 + 模板，启用即可

## 数据存储

所有数据存在本地，按平台位置：

| 平台    | 路径                                          |
| ------- | --------------------------------------------- |
| macOS   | `~/Library/Application Support/WeeklyReport/` |
| Linux   | `~/.config/weekly-report/`                    |
| Windows | `%APPDATA%\WeeklyReport\`                     |

详细数据模型见 [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md#数据存储)。

## 文档

| 文档                                              | 内容                                |
| ------------------------------------------------- | ----------------------------------- |
| [docs/SPEC.md](./docs/SPEC.md)                    | 完整功能规格                         |
| [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md)    | 技术架构、模块划分、数据模型         |
| [docs/LLM.md](./docs/LLM.md)                      | 多 LLM 源协议抽象                    |
| [docs/JSONL.md](./docs/JSONL.md)                  | Claude Code / Codex CLI 日志 schema  |
| [docs/UI.md](./docs/UI.md)                        | UI / UX 规格                         |
| [docs/TASKS.md](./docs/TASKS.md)                  | 分阶段实现计划                       |
| [docs/DECISIONS.md](./docs/DECISIONS.md)          | 架构决策记录                         |
| [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md)    | 贡献指南（开发流程、规范）           |
| [docs/CHANGELOG.md](./docs/CHANGELOG.md)          | 版本变更记录                         |

## 贡献

欢迎 Issue 和 PR。代码风格：

- **Rust**：`cargo fmt` + `cargo clippy -- -D warnings`
- **React**：函数组件 + Hooks，Tailwind 工具类，不引入额外 CSS-in-JS
- **提交信息**：[Conventional Commits](https://www.conventionalcommits.org/)

## License

[MIT](./LICENSE)
