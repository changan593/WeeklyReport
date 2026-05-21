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
  },
};
