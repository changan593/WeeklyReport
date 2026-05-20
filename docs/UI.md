# UI / UX 规格 (UI.md)

> 本文档定义每个页面的布局、交互、视觉风格。
> Claude Code 实现 React 组件时严格遵循本文档。

---

## 1. 视觉系统

### 1.1 配色（Tailwind stone 系列）

| 用途         | Tailwind 类                      | 备注                    |
| ------------ | -------------------------------- | ----------------------- |
| 页面背景     | `bg-stone-50`                    | 极浅 warm gray          |
| 卡片背景     | `bg-white`                       |                         |
| 边框         | `border-stone-200`               | hover 时变 stone-300    |
| 主文字       | `text-stone-900`                 |                         |
| 次文字       | `text-stone-600` / `text-stone-500` |                      |
| 弱文字       | `text-stone-400`                 |                         |
| 主操作按钮   | `bg-stone-900 text-white`        |                         |
| 状态-成功    | `bg-emerald-50 text-emerald-700` |                         |
| 状态-错误    | `bg-rose-50 text-rose-700`       |                         |
| 状态-警告    | `bg-amber-50 text-amber-700`     |                         |
| 高亮 (默认)  | `bg-amber-50 text-amber-700` + star icon | 用于 default 标记 |

### 1.2 排版

- 字体：`-apple-system, "SF Pro Display", "PingFang SC", "Microsoft YaHei", sans-serif`
- antialiased
- 默认字号：`text-[13px]` 或 `text-[12.5px]`
- 标题：`text-2xl font-medium` （页面标题）
- 卡片标题：`text-[14px] font-medium`
- 标签：`text-[11.5px]` 或 `text-[11px]`

### 1.3 圆角与间距

- 卡片：`rounded-lg` (8px)
- 按钮：`rounded-md` (6px)
- Input：`rounded` (4px)
- 卡片内边距：`p-5` (20px)
- 卡片间距：`space-y-3` (12px)

### 1.4 图标

使用 inline SVG，统一 16-18px，line stroke 风格（不要 filled icon）。`stroke-width: 1.6`。

---

## 2. 布局结构

```
┌─────────────────────────────────────────────────────────┐
│  Sidebar (w-56)  │       Main Content (flex-1)          │
│  ──────────────  │  ──────────────────────────────────  │
│  [Logo]          │                                       │
│                  │   <Page>                              │
│  · 工作区        │                                       │
│  · LLM 源        │                                       │
│  · 周报模板      │                                       │
│  · 历史周报      │                                       │
│  · 定时任务      │                                       │
│  · 设置          │                                       │
│                  │                                       │
│  ──────────────  │                                       │
│  [生成周报] ← 底部固定按钮                                  │
└─────────────────────────────────────────────────────────┘
```

- Sidebar：`w-56` (224px)，`bg-white`，右侧 `border-r border-stone-200`
- Main：`flex-1 overflow-auto`，内容居中 `max-w-4xl mx-auto px-10 py-10`
- 整体高度 `h-screen`，无 body 滚动条

---

## 3. 共用组件 (`ui.jsx`)

实现以下 primitives：

```jsx
<Icon name="..." size={18} className="..." />     // 内置 SVG 图标库
<IconButton title="..." onClick={...}>            // 28×28 圆角按钮
<Modal onClose={...} width="max-w-lg">            // 模态对话框
<FormField label="..." hint="...">                // 表单字段包装
<Input value={...} onChange={...} />              // 标准 text input
<Mono value={...} onChange={...} />               // 等宽字体 input
<Toggle on={true} onChange={...} />               // 开关
<PrimaryButton onClick={...} disabled={...}>      // 主操作
<SecondaryButton onClick={...} disabled={...}>    // 次操作
<StatusBanner status={{type:'success',msg:'...'}} />  // 状态提示
```

`Icon` 必须支持以下名字（按需扩展）：
```
workspace, template, report, settings, llm, schedule, plus, sparkle,
edit, trash, check, chevronR, server, laptop, download, copy,
play, clock, mail, x, refresh, star
```

---

## 4. 各页面规格

### 4.1 工作区页 (Workspaces.jsx)

**布局：**

- 标题区：「工作区」+ 副标题 + 右上角「添加工作区」按钮
- 卡片列表：每个工作区一卡片，垂直堆叠

**工作区卡片内容：**

- 左：图标（local → laptop / ssh → server）
- 中：
  - 标题：`name`
  - 子标题：`local` 显示 "本地机器"；`ssh` 显示 `{user}@{host}:{port}`
  - 标签行：显示启用的工具（橙色 Claude Code / 绿色 Codex）和路径
- 右：编辑、删除按钮

**新建/编辑对话框 (Modal)：**

- 类型 segment：本地 / SSH 远程
- 名称（必填）
- SSH 时：host、port、user、ssh_key 路径
- 工具勾选 + 路径配置（每个工具一行）
- 底部：左下「测试连接」+ 右下「取消 / 保存」
- 测试连接结果用 StatusBanner 显示

### 4.2 LLM 源页 (Providers.jsx)

**布局：**

- 标题区：「LLM 源」+ 副标题（说明用途）+ 右上角「添加 LLM 源」按钮
- 卡片列表

**LLM 源卡片内容：**

- 左：LLM 图标
- 中：
  - 标题：`name` + 默认标记（amber 小标签 + star 图标）+ 协议类型小标签
  - 副：`base_url`（mono 字体）
  - 标签行：`模型: {model}` (橙色 code) + `max_tokens: N` + `temp: 0.7`
- 右：
  - 非默认 → 显示「设为默认」按钮
  - 编辑、删除

**空状态：** 显示预设按钮组（前 6 个），点击直接进入编辑器并预填该预设。

**新建/编辑对话框：**

- 快速预设按钮组（10 个）：点击后预填 base_url / model / temperature
- 名称
- 协议类型 select：OpenAI 兼容 / Anthropic / Gemini
- Base URL（mono）
- API Key（密码框，mono）
- 模型 ID（mono）
- max_tokens + temperature（一行两列）
- 「设为默认」勾选
- 底部：左下「测试连接」+ 右下「取消 / 保存」

### 4.3 周报模板页 (Templates.jsx)

**布局：**

- 标题区：「周报模板」+ 副标题 + 右上「新建模板」
- 卡片网格 `grid grid-cols-2 gap-3`

**模板卡片内容：**

- 顶部：标题 + 风格标签（不同色：tech 蓝 / exec 紫 / simple 琥珀 / custom 灰）
- 中：`N 个章节`
- 章节列表：编号 + 标题
- 底部：「内置」/「自定义」标记 + 编辑/删除按钮（内置不可删）

**编辑对话框：**

- 模板名称
- 风格 select
- 指定 LLM 源 select（含「使用默认 LLM 源」选项）
- 额外要求（textarea）
- 章节列表（每行一个 input + 删除按钮 + 末尾「添加章节」）
- 内置模板下所有字段 disabled

### 4.4 历史周报页 (Reports.jsx)

**布局：**

- 标题区：「历史周报」+ 副标题
- 表格（不用卡片，列表数据用表格更合适）

**表格列：**

| 时间范围 | 模板 | LLM 源 | 项目 | Tokens | 生成时间 | → |
|----------|------|--------|------|--------|----------|---|

行点击 → 弹出详情模态。

**详情对话框：**

- 顶部：周次 + 元数据（模板 / 项目数 / tokens / 时间 / LLM 源）
- 中：Markdown 正文，`bg-stone-50 rounded-lg p-5 font-mono text-[12px]`
- 底部：左「删除」+ 右「复制 / 关闭」

### 4.5 定时任务页 (Schedules.jsx)

**布局：**

- 标题区 + 「新建定时任务」按钮
- 若 SMTP 未配置：顶部 amber banner 提示去设置页配置
- 卡片列表

**定时任务卡片内容：**

- 左：时钟图标（启用绿/未启用灰）
- 中：
  - 标题 + cron code 小标签
  - 描述：cron 预设的可读标签（如「每周五 17:30」）+ 收件人数 + 抄送数
  - 元数据：下次执行 + 上次执行 + 状态（成功绿 / 失败红显示前 60 字符）
- 右：Toggle 开关 + 立即执行 + 编辑 + 删除

**新建/编辑对话框：**

- 任务名称
- cron 表达式（mono input）+ 4 个预设按钮
- 模板 select + 时间范围 select（一行两列）
- 工作区多选（checkbox list）
- 邮件主题模板 input
- 收件人 textarea（多个用逗号/空格/分号分隔）
- 抄送 textarea
- 启用开关
- 底部：取消 / 保存

### 4.6 设置页 (Settings.jsx)

**布局：**

- 标题区
- 分区列表：每区一个 Section（小灰标题 + 白底卡片）

**Section 1: SMTP 邮箱配置**

- 快速选择按钮组（5 个邮箱预设）
- 选中预设时显示对应 hint 信息（amber 提示）
- SMTP Host + Port（一行）
- 加密方式 segment：STARTTLS / SSL/TLS
- 用户名、密码、发件人显示名
- StatusBanner（保存/测试结果）
- 底部操作行：测试邮箱 input + 「发测试」「测连接」「保存」按钮

**Section 2: 数据存储**

- 显示当前数据目录路径
- 说明：完全无数据库依赖，可备份/迁移

**（v0.1.0 不实现的功能）**

预留位置：`AI 参数（clip 字符数、参考报告数量）`、主题、语言。

### 4.7 生成对话框 (GenerateDialog.jsx)

从侧边栏「生成周报」按钮触发，是 Modal。

**4 个 step：**

1. **config**：配置生成参数
   - 工作区 checkbox list
   - 模板 radio
   - LLM 源 select（默认「使用默认」）
   - 时间范围 button segment
   - 底部：取消 / 开始生成
2. **generating**：加载状态
   - 居中：图标 + "正在生成周报…" + 步骤描述
3. **error**：失败
   - rose banner 显示错误信息
   - 底部：返回 / 关闭
4. **done**：成功
   - 顶部：✓ 成功 + 用了哪个 LLM
   - 中：Markdown 预览（mono 字体，max-h-80 + scroll）
   - 底部：元数据（tokens / 耗时） + 复制 / 完成

---

## 5. 交互模式

### 5.1 加载状态

任何异步加载必须显示 loading 状态。统一样式：

```jsx
<div className="text-center py-12 text-stone-400 text-[13px]">加载中…</div>
```

### 5.2 空状态

数据为空时显示友好提示：

```jsx
<div className="text-center py-16">
  <div className="w-12 h-12 mx-auto rounded-xl bg-stone-100 flex items-center justify-center text-stone-400 mb-3">
    <Icon name="..." size={22} />
  </div>
  <p className="text-[13px] text-stone-500">还没有 XXX</p>
</div>
```

### 5.3 删除确认

所有删除操作 **必须** 用 `confirm()` 二次确认：

```jsx
if (!confirm(`删除「${item.name}」？`)) return;
```

### 5.4 状态反馈

成功操作显示 StatusBanner 2.5 秒：

```jsx
setStatus({ type: 'success', msg: '已保存' });
setTimeout(() => setStatus(null), 2500);
```

### 5.5 复制操作

使用 `@tauri-apps/plugin-clipboard-manager`：

```jsx
navigator.clipboard.writeText(text).then(() => alert('已复制'));
```

---

## 6. 响应式

- 最小窗口：900 × 600（在 `tauri.conf.json` 中配置）
- 不需要适配手机/平板
- 内容区 max-width 限制：`max-w-4xl`（约 896px）

---

## 7. 可访问性

- 所有 IconButton 必须有 `title` 属性
- 所有按钮 disabled 状态用 `disabled:opacity-40` 视觉提示
- 配色对比度满足 WCAG AA（stone 系列默认达标）

---

## 8. 验证清单

实现完成后逐项确认：

- [ ] 6 个一级页面 + 生成对话框全部可访问
- [ ] 侧边栏 active 状态高亮
- [ ] 每个页面都有空状态显示
- [ ] 每个页面的加载状态显示「加载中…」
- [ ] 所有删除操作有确认对话框
- [ ] 所有 Modal 点击外部关闭
- [ ] 默认 provider 在卡片上有 amber star 标记
- [ ] 内置模板有「内置」标签且不可删
- [ ] 失败状态在定时任务卡片上红色显示前 60 字符
- [ ] SMTP 预设选中时显示对应 hint
- [ ] 生成对话框 4 个 step 切换正常
