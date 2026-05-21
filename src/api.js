// Tauri command 包装层 + 通用异步加载 hook。
//
// 所有 Tauri command 在此处统一封装，便于前端只 import 这一个文件，
// 也便于将来切换 IPC 机制（如 mock）时只动一处。

import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';

// ============================================================
// Workspaces
// ============================================================

export function listWorkspaces() {
  return invoke('list_workspaces');
}

export function saveWorkspace(workspace) {
  return invoke('save_workspace', { workspace });
}

export function deleteWorkspace(id) {
  return invoke('delete_workspace', { id });
}

export function testWorkspaceConnection(workspace) {
  return invoke('test_workspace_connection', { workspace });
}

// ============================================================
// LLM Providers
// ============================================================

export function listProviders() {
  return invoke('list_providers');
}

export function saveProvider(provider) {
  return invoke('save_provider', { provider });
}

export function deleteProvider(id) {
  return invoke('delete_provider', { id });
}

export function testProvider(provider) {
  return invoke('test_provider', { provider });
}

export function llmPresets() {
  return invoke('llm_presets');
}

// ============================================================
// Templates
// ============================================================

export function listTemplates() {
  return invoke('list_templates');
}

export function saveTemplate(template) {
  return invoke('save_template', { template });
}

export function deleteTemplate(id) {
  return invoke('delete_template', { id });
}

// ============================================================
// Reports & Generation
// ============================================================

export function listReports() {
  return invoke('list_reports');
}

export function getReport(id) {
  return invoke('get_report', { id });
}

/// 取报告的 HTML 版本（用于详情页 HTML 预览 / 复制 HTML）。
export function getReportHtml(id) {
  return invoke('get_report_html', { id });
}

export function deleteReport(id) {
  return invoke('delete_report', { id });
}

/// req: { workspace_ids, template_id, days, provider_id? }
/// 返回: { record, content, duration_ms }
export function generateReport(req) {
  return invoke('generate_report', { req });
}

// ============================================================
// Settings
// ============================================================

export function getSettings() {
  return invoke('get_settings');
}

export function saveSettings(settings) {
  return invoke('save_settings', { settings });
}

/// 通知后端 i18n 切换语言（让 anyhow! 错误消息和邮件模板立即跟上 UI）。
export function setAppLanguage(lang) {
  return invoke('set_app_language', { lang });
}

// ============================================================
// 两步生成（详见 docs/SPEC.md "两步生成"）
// ============================================================

/// 第一步：收集日志 → 返回带 timestamp 的 Summary（CollectionOutput）。
export function collectLogs({ workspace_ids, days }) {
  return invoke('collect_logs', { req: { workspace_ids, days } });
}

/// 第二步：用（可能已被用户编辑过的）Summary 调 LLM + 存档。
export function renderReport({ summary, template_id, days, provider_id }) {
  return invoke('render_report', {
    req: { summary, template_id, days, provider_id },
  });
}

/// 草稿摘要持久化（用户编辑到一半关掉对话框，下次能继续）。
export function saveDraftSummary(draft) {
  return invoke('save_draft_summary', { draft });
}

export function loadDraftSummary() {
  return invoke('load_draft_summary');
}

export function clearDraftSummary() {
  return invoke('clear_draft_summary');
}

// ============================================================
// Schedules
// ============================================================

/// 返回 ScheduleView 数组（每条 schedule + next_run_computed）。
export function listSchedules() {
  return invoke('list_schedules');
}

export function saveSchedule(schedule) {
  return invoke('save_schedule', { schedule });
}

export function deleteSchedule(id) {
  return invoke('delete_schedule', { id });
}

export function runScheduleNow(id) {
  return invoke('run_schedule_now', { id });
}

// ============================================================
// SMTP
// ============================================================

export function getSmtpConfig() {
  return invoke('get_smtp_config');
}

export function saveSmtpConfig(config) {
  return invoke('save_smtp_config', { config });
}

export function testSmtpConfig(config) {
  return invoke('test_smtp_config', { config });
}

export function sendTestEmail(config, to) {
  return invoke('send_test_email', { req: { config, to } });
}

// ============================================================
// Misc
// ============================================================

export function dataDirPath() {
  return invoke('data_dir_path');
}

// ============================================================
// useAsyncState hook
//
// 统一处理 loading / error / reload 模式：
//
//   const [data, loading, reload, error] = useAsyncState(listWorkspaces, []);
//
// `loader` 必须返回 Promise。`deps` 变化时自动重新加载。
// ============================================================

export function useAsyncState(loader, deps = []) {
  const [data, setData] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const aliveRef = useRef(true);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const v = await loader();
      if (aliveRef.current) setData(v);
    } catch (e) {
      if (aliveRef.current) setError(formatError(e));
    } finally {
      if (aliveRef.current) setLoading(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  useEffect(() => {
    aliveRef.current = true;
    reload();
    return () => {
      aliveRef.current = false;
    };
  }, [reload]);

  return [data, loading, reload, error];
}

// formatError 已迁移到 src/utils.js（便于单测且与 Tauri 解耦）。这里 re-export
// 让既有调用方 `import { formatError } from '../api.js'` 继续工作。
export { formatError } from './utils.js';
