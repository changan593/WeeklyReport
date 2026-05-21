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
    close: 'Close',
    copy: 'Copy',
    copied: 'Copied',
    copy_failed: 'Copy failed',
    deleting: 'Deleting…',
  },
  ui: {
    modal: { close: 'Close' },
    empty: { default: 'Nothing here yet' },
    loading: { default: 'Loading…' },
  },
  reports: {
    title: 'Reports',
    subtitle: 'Every generated report is archived locally and can be used as a style reference for the next generation.',
    empty: 'No reports generated yet',
    columns: {
      range: 'Date range',
      template: 'Template',
      provider: 'LLM',
      projects: 'Projects',
      tokens: 'Tokens',
      generated_at: 'Generated at',
    },
    copy_failed: 'Copy failed',
    confirm_delete: 'Delete report "{name}"?',
    detail: {
      title: 'Report details',
      template: 'Template: {name}',
      provider: 'LLM: {name}',
      projects: 'Projects: {count}',
      tokens: 'Tokens: {count}',
    },
    actions: {
      copy_markdown: 'Copy Markdown',
    },
  },
  generate: {
    title: 'Generate Report',
    errors: {
      no_workspace: 'Please select at least one workspace',
      no_template: 'Please select a template',
      unknown: 'Unknown error',
      copy_failed: 'Copy failed',
      hint: 'Common causes: LLM source not configured / wrong API key / network issue / selected workspace has no logs.',
    },
    actions: {
      start: 'Start generation',
      cancel: 'Cancel',
      back: 'Back',
      close: 'Close',
      done: 'Done',
      copy: 'Copy',
      background: 'Continue in background, close window',
      background_tooltip: 'The backend keeps generating; the report will still be archived when finished.',
    },
    config: {
      workspace: 'Workspaces',
      workspace_empty: 'No workspaces configured. Add one on the Workspaces page first.',
      local: 'local',
      template: 'Template',
      template_empty: 'No templates available (there should be 3 built-in templates; check the backend).',
      builtin: 'built-in',
      llm: 'LLM source',
      llm_default: 'Use default / template-specified',
      default_suffix: ' (default)',
      days: 'Date range',
      days_unit: '{n} days',
    },
    generating: {
      title: 'Generating report…',
      subtitle: 'Scan logs → compress → call LLM (up to 120 s)',
      hint: 'You can close this window; generation continues in background and the report is archived to "Reports" when done.',
    },
    done: {
      success: 'Generated successfully ({provider})',
      skipped: '⚠ Skipped {lines} line(s) / {files} file(s) while parsing (corrupt or unreadable JSON). The report may be incomplete; set RUST_LOG=debug for details.',
    },
    footer: {
      tokens: 'tokens {n} · {sec}s',
    },
  },
};
