// i18n resources: English.
//
// Key convention: `{page/module}.{field}`, keep it flat, avoid deep nesting.
// Not translated: data field names, JSON file names, internal error code keys, URLs.

export default {
  nav: {
    workspaces: 'Workspaces',
    providers: 'LLM Sources',
    templates: 'Templates',
    reports: 'Reports',
    schedules: 'Schedules',
    settings: 'Settings',
  },
  sidebar: {
    generate: 'Generate Report',
  },
  settings: {
    title: 'Settings',
    subtitle: 'Configure SMTP (for scheduled email delivery); view data directory',
    sections: {
      language: 'Language / 语言',
      smtp: 'SMTP Mail Configuration',
      data: 'Data Storage',
    },
    language: {
      hint: 'Switch UI language, applied immediately. Backend error messages and email templates will be translated in a future release.',
      zh_cn: '简体中文',
      en: 'English',
    },
    data: {
      desc: 'All configuration is stored as JSON files in the directory below; you can back up or migrate freely.',
    },
  },
  common: {
    loading: 'Loading…',
    save: 'Save',
    saved: 'Saved',
    saving: 'Saving…',
    cancel: 'Cancel',
    confirm: 'Confirm',
    delete: 'Delete',
    edit: 'Edit',
    add: 'Add',
  },
};
