// i18n 资源：中文（默认语言）。
//
// key 命名约定：`{页面/模块}.{字段}`，扁平就行不要嵌套过深。
// 不参与 i18n 的：数据字段名、JSON 文件名、内部错误码 key、URL。

export default {
  nav: {
    workspaces: '工作区',
    providers: 'LLM 源',
    templates: '周报模板',
    reports: '历史周报',
    schedules: '定时任务',
    settings: '设置',
  },
  sidebar: {
    generate: '生成周报',
  },
  settings: {
    title: '设置',
    subtitle: '配置 SMTP（用于定时任务发送邮件）；查看数据目录',
    sections: {
      language: '语言 / Language',
      smtp: 'SMTP 邮箱配置',
      data: '数据存储',
    },
    language: {
      hint: '切换应用界面语言，立即生效；后端错误消息与邮件模板将在后续版本完成翻译',
      zh_cn: '简体中文',
      en: 'English',
    },
    data: {
      desc: '所有配置以 JSON 文件形式存放在以下目录；可直接备份或迁移。',
    },
  },
  common: {
    loading: '加载中…',
    save: '保存',
    saved: '已保存',
    saving: '保存中…',
    cancel: '取消',
    confirm: '确认',
    delete: '删除',
    edit: '编辑',
    add: '新增',
    close: '关闭',
    copy: '复制',
    copied: '已复制',
    copy_failed: '复制失败',
    deleting: '删除中…',
  },
  ui: {
    modal: { close: '关闭' },
    empty: { default: '还没有内容' },
    loading: { default: '加载中…' },
  },
  reports: {
    title: '历史周报',
    subtitle: '所有生成的周报都会本地存档，可作为下次生成的风格参考',
    empty: '还没有生成过任何周报',
    columns: {
      range: '时间范围',
      template: '模板',
      provider: 'LLM 源',
      projects: '项目',
      tokens: 'Tokens',
      generated_at: '生成时间',
    },
    copy_failed: '复制失败',
    confirm_delete: '删除报告「{name}」？',
    detail: {
      title: '周报详情',
      template: '模板：{name}',
      provider: 'LLM：{name}',
      projects: '项目：{count}',
      tokens: 'Tokens：{count}',
    },
    actions: {
      copy_markdown: '复制 Markdown',
    },
  },
  generate: {
    title: '生成周报',
    errors: {
      no_workspace: '请至少选择一个工作区',
      no_template: '请选择模板',
      unknown: '未知错误',
      copy_failed: '复制失败',
      hint: '常见原因：LLM 源未配置 / API key 错误 / 网络不通 / 选中的工作区无日志。',
    },
    actions: {
      start: '开始生成',
      cancel: '取消',
      back: '返回',
      close: '关闭',
      done: '完成',
      copy: '复制',
      background: '后台继续，关闭窗口',
      background_tooltip: '后端会继续生成，完成后报告仍会存档',
    },
    config: {
      workspace: '工作区',
      workspace_empty: '还没有配置工作区，请先到「工作区」页添加。',
      local: '本地',
      template: '模板',
      template_empty: '没有可用模板（应至少有 3 个内置模板，请检查后端）。',
      builtin: '内置',
      llm: 'LLM 源',
      llm_default: '使用默认 / 模板指定',
      default_suffix: '（默认）',
      days: '时间范围',
      days_unit: '{n} 天',
    },
    generating: {
      title: '正在生成周报…',
      subtitle: '扫描日志 → 压缩聚合 → 调用 LLM，最长 120 秒',
      hint: '可点底部按钮关闭窗口，生成会在后台继续，完成后报告自动存档到「历史周报」',
    },
    done: {
      success: '生成成功（{provider}）',
      skipped: '⚠ 解析时跳过 {lines} 行 / {files} 个文件（JSON 损坏或不可读）。报告内容可能不完整，可用 RUST_LOG=debug 查看明细。',
    },
    footer: {
      tokens: 'tokens {n} · 耗时 {sec}s',
    },
  },
};
