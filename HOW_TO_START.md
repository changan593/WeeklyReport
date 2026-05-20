# 如何用 Claude Code 启动这个项目

> 这份文档不属于项目文档体系，是给你（项目发起人）看的。
> 项目真正落地后可以删掉。

---

## 1. 准备工作

### 1.1 在 GitHub 创建空仓库

```
https://github.com/new
仓库名: WeeklyReport
描述: AI-powered weekly report generator from Claude Code / Codex CLI logs
可见性: Public
初始化: 不勾任何选项（README/.gitignore/LICENSE 都不要勾）
```

### 1.2 本地准备

```bash
# 选个工作目录
mkdir -p ~/code && cd ~/code

# 解压本规格文档（假设你下载到了 ~/Downloads/WeeklyReport-spec.zip）
unzip ~/Downloads/WeeklyReport-spec.zip -d WeeklyReport
cd WeeklyReport

# 此时目录里应该有：
# README.md  CLAUDE.md  docs/  HOW_TO_START.md（本文件）

# 初始化 git
git init
git add .
git commit -m "docs: initial specification"
git branch -M main
git remote add origin git@github.com:changan593/WeeklyReport.git
git push -u origin main
```

### 1.3 安装 Claude Code

如果还没有：

```bash
# 参考 https://docs.claude.com/en/docs/claude-code
curl -fsSL https://claude.ai/install.sh | bash
```

---

## 2. 启动 Claude Code

```bash
cd ~/code/WeeklyReport
claude
```

第一次进来时，Claude Code 会读取项目根目录的 `CLAUDE.md`，了解项目背景和约定。

---

## 3. 推荐的对话流程

### 第一次会话：阶段 0 + 阶段 1

```
你好，请按 docs/TASKS.md 完成阶段 0 和阶段 1。

完成后告诉我：
- 你做了什么
- 验证清单的每一项是否通过
- 有没有任何疑问需要我确认

不要跳到阶段 2。
```

### 后续会话：每次 1-2 个阶段

每开一个新会话（避免上下文过长）：

```
请按 docs/TASKS.md 完成阶段 X 和阶段 Y。
阶段 0 到 X-1 已完成，代码在仓库里。
完成后告诉我验证结果。
```

### 关键策略

1. **每阶段单独提交**：让 Claude Code 在每阶段结束时 git commit。这样你能 review 增量代码。
2. **每个新阶段开新会话**：避免上下文太长导致它「忘记」前面的约定。每开一个新会话，让它先读 `CLAUDE.md`。
3. **遇到分歧立即停下**：如果 Claude Code 想引入新依赖或者背离 SPEC，立即让它停，先讨论。
4. **每阶段都跑验证**：不要相信「已完成」，要看到验证清单全勾。

---

## 4. 常见对话模板

### 让 Claude Code 重新对齐规格

```
请重新读取 CLAUDE.md 和 docs/SPEC.md，然后告诉我当前实现是否完全符合规格。
如有偏差，列出来后逐一修正。
```

### 让 Claude Code 重构

```
当前 src-tauri/src/logs.rs 已经超过 400 行。
按照 CLAUDE.md 的约定，拆分成多个模块。
拆分前先告诉我你的拆分方案，等我确认。
```

### 让 Claude Code review 自己的代码

```
请用 docs/CLAUDE.md 中的代码质量标准 review 你刚写的代码。
列出每个不符合标准的地方，然后修正。
```

### 让 Claude Code 写测试

```
为 src-tauri/src/logs.rs 写单元测试，覆盖：
1. 解析 Claude Code history.jsonl 格式
2. 解析 Codex CLI rollout JSONL 格式
3. AI 文本压缩（首尾各 200 字符）
4. 连续相似指令去重

用 fixture 数据，放在 src-tauri/tests/ 下。
```

---

## 5. 何时停下来调整

如果你看到 Claude Code 写出以下代码，立即停下：

- 引入 SQLite（违反 ADR-001）
- 引入 TypeScript（违反 ADR-004）
- 引入 styled-components / Emotion（违反 ADR-005）
- 引入 Redux / Zustand（违反 ADR-006）
- 引入 pulldown-cmark 等 markdown 库（违反 ADR-007）
- 在 prompt 里塞 tool_result（违反 SPEC.md token 压缩规则）
- 任何模块超过 500 行（违反 CLAUDE.md 模块规模约定）
- 任何 `.unwrap()` 在非启动路径

直接说：「这违反了 ADR-X，请改回 SPEC.md 中的方案。」

---

## 6. 验收节奏

每阶段完成后，你自己跑一遍：

```bash
# 后端
cd src-tauri
cargo fmt --check
cargo clippy -- -D warnings
cargo test

# 前端 + 整体
cd ..
npm run tauri:dev
```

然后用应用走一遍该阶段对应的用户流程。OK 才让 Claude Code 进下一阶段。

---

## 7. 何时手动接管

以下情况建议你自己写而不让 Claude Code 写：

- **图标设计**：让设计师做或用 AI 工具，不要让 Claude Code 写 SVG 生成
- **CI 配置**：GitHub Actions 配置比较敏感，自己看模板写
- **Release notes**：必须有人的判断
- **README 截图**：跑起来后自己截

---

## 8. 项目完成后

- [ ] 删除本文件 `HOW_TO_START.md`
- [ ] 在 README.md 中添加截图
- [ ] 打 v0.1.0 tag
- [ ] 在 GitHub 发布 Release
- [ ] 写一篇博客 / Twitter 介绍项目

---

祝项目顺利。如果有任何阶段卡住，回到这个 spec 文档体系，让它指导 Claude Code。
