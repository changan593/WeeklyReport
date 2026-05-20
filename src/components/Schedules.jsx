// 定时任务页（详见 docs/UI.md#45-定时任务页-schedulesjsx）。
//
// Cron 用 7 段格式（秒 分 时 日 月 星期 年），与系统 cron 不同。
// SMTP 未配置时顶部 amber banner 提示去设置页配置。

import { useEffect, useMemo, useState } from 'react';
import {
  deleteSchedule,
  getSmtpConfig,
  listProviders,
  listSchedules,
  listTemplates,
  listWorkspaces,
  runScheduleNow,
  saveSchedule,
  useAsyncState,
  formatError,
} from '../api.js';
import {
  EmptyState,
  FormField,
  Icon,
  IconButton,
  Input,
  LoadingState,
  Modal,
  ModalBody,
  ModalFooter,
  ModalHeader,
  Mono,
  PrimaryButton,
  SecondaryButton,
  Select,
  StatusBanner,
  Textarea,
  Toggle,
} from './ui.jsx';

// 4 个常用 cron 预设（7 段格式）
const CRON_PRESETS = [
  { label: '每周五 17:30', cron: '0 30 17 ? * FRI *' },
  { label: '每周一 09:00', cron: '0 0 9 ? * MON *' },
  { label: '工作日 18:00', cron: '0 0 18 ? * MON-FRI *' },
  { label: '每周日 21:00', cron: '0 0 21 ? * SUN *' },
];

const DAY_OPTIONS = [3, 7, 14, 30];

export default function Schedules() {
  const [items, loading, reload] = useAsyncState(listSchedules, []);
  const [workspaces, setWorkspaces] = useState([]);
  const [templates, setTemplates] = useState([]);
  const [providers, setProviders] = useState([]);
  const [smtpConfigured, setSmtpConfigured] = useState(true);
  const [editing, setEditing] = useState(null);

  useEffect(() => {
    Promise.all([listWorkspaces(), listTemplates(), listProviders(), getSmtpConfig()])
      .then(([ws, tpl, prov, smtp]) => {
        setWorkspaces(ws || []);
        setTemplates(tpl || []);
        setProviders(prov || []);
        setSmtpConfigured(!!(smtp && smtp.host && smtp.username));
      })
      .catch(() => {});
  }, []);

  async function handleDelete(view) {
    if (!confirm(`删除定时任务「${view.name}」？`)) return;
    try {
      await deleteSchedule(view.id);
      reload();
    } catch (e) {
      alert(formatError(e));
    }
  }

  async function handleToggle(view) {
    try {
      await saveSchedule({ ...view, enabled: !view.enabled });
      reload();
    } catch (e) {
      alert(formatError(e));
    }
  }

  async function handleRunNow(view) {
    try {
      const msg = await runScheduleNow(view.id);
      alert(msg);
      reload();
    } catch (e) {
      alert(formatError(e));
    }
  }

  return (
    <div>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <h1 className="text-2xl font-medium text-stone-900">定时任务</h1>
          <p className="mt-1 text-[13px] text-stone-500">
            按 cron 表达式定时生成周报并发送邮件
          </p>
        </div>
        <PrimaryButton onClick={() => setEditing(emptySchedule())}>
          <Icon name="plus" size={15} /> 新建定时任务
        </PrimaryButton>
      </header>

      {!smtpConfigured && (
        <div className="mb-4 rounded border border-amber-200 bg-amber-50 px-3 py-2 text-[12.5px] text-amber-700">
          ⚠ 尚未配置 SMTP（无法发送邮件），请到「设置」页填写。
        </div>
      )}

      {loading && <LoadingState />}
      {!loading && items && items.length === 0 && (
        <EmptyState iconName="schedule" message="还没有定时任务" />
      )}
      {!loading && items && items.length > 0 && (
        <div className="space-y-3">
          {items.map((view) => (
            <ScheduleCard
              key={view.id}
              view={view}
              onEdit={() => setEditing(view)}
              onDelete={() => handleDelete(view)}
              onToggle={() => handleToggle(view)}
              onRunNow={() => handleRunNow(view)}
            />
          ))}
        </div>
      )}

      {editing && (
        <ScheduleEditor
          initial={editing}
          workspaces={workspaces}
          templates={templates}
          providers={providers}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            reload();
          }}
        />
      )}
    </div>
  );
}

function ScheduleCard({ view, onEdit, onDelete, onToggle, onRunNow }) {
  const enabled = view.enabled;
  const lastStatus = view.last_status || '';
  const isFail = lastStatus.startsWith('failed');
  return (
    <div className="flex items-start gap-3 rounded-lg border border-stone-200 bg-white p-5 hover:border-stone-300">
      <div
        className={`mt-0.5 flex h-9 w-9 items-center justify-center rounded-lg ${
          enabled ? 'bg-emerald-50 text-emerald-700' : 'bg-stone-100 text-stone-400'
        }`}
      >
        <Icon name="clock" size={18} />
      </div>
      <div className="flex-1 overflow-hidden">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[14px] font-medium text-stone-900">{view.name}</span>
          <code className="rounded bg-stone-100 px-1.5 py-0.5 font-mono text-[11px] text-stone-600">
            {view.cron}
          </code>
        </div>
        <div className="mt-1 text-[12px] text-stone-500">
          {view.recipients?.length || 0} 个收件人
          {view.cc?.length > 0 && ` · ${view.cc.length} 个抄送`}
        </div>
        <div className="mt-1 flex flex-wrap gap-x-3 text-[11.5px] text-stone-500">
          {view.next_run_computed && enabled && (
            <span>下次：{formatIso(view.next_run_computed)}</span>
          )}
          {view.last_run && <span>上次：{formatIso(view.last_run)}</span>}
          {lastStatus && (
            <span className={isFail ? 'text-rose-700' : 'text-emerald-700'}>
              {lastStatus.slice(0, 60)}
            </span>
          )}
        </div>
      </div>
      <div className="flex flex-col items-end gap-2">
        <Toggle on={enabled} onChange={onToggle} />
        <div className="flex gap-0.5">
          <IconButton title="立即执行" onClick={onRunNow}>
            <Icon name="play" size={14} />
          </IconButton>
          <IconButton title="编辑" onClick={onEdit}>
            <Icon name="edit" size={14} />
          </IconButton>
          <IconButton title="删除" onClick={onDelete}>
            <Icon name="trash" size={14} />
          </IconButton>
        </div>
      </div>
    </div>
  );
}

function emptySchedule() {
  return {
    id: '',
    name: '',
    cron: '0 30 17 ? * FRI *',
    enabled: true,
    workspace_ids: [],
    template_id: '',
    provider_id: null,
    days: 7,
    recipients: [],
    cc: [],
    subject_tpl: '周报 {date}',
    last_run: null,
    last_status: null,
    next_run: null,
  };
}

function ScheduleEditor({ initial, workspaces, templates, providers, onClose, onSaved }) {
  // 输入字段 normalize：recipients / cc 用文本 textarea
  const [name, setName] = useState(initial.name || '');
  const [cron, setCron] = useState(initial.cron || '');
  const [enabled, setEnabled] = useState(!!initial.enabled);
  const [wsIds, setWsIds] = useState(initial.workspace_ids || []);
  const [tplId, setTplId] = useState(initial.template_id || templates[0]?.id || '');
  const [provId, setProvId] = useState(initial.provider_id || '');
  const [days, setDays] = useState(initial.days || 7);
  const [recipientsText, setRecipientsText] = useState((initial.recipients || []).join(', '));
  const [ccText, setCcText] = useState((initial.cc || []).join(', '));
  const [subjectTpl, setSubjectTpl] = useState(initial.subject_tpl || '周报 {date}');
  const [error, setError] = useState(null);
  const [saving, setSaving] = useState(false);

  const recipients = useMemo(() => splitEmails(recipientsText), [recipientsText]);
  const cc = useMemo(() => splitEmails(ccText), [ccText]);

  function toggleWs(id) {
    setWsIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  }

  async function handleSave() {
    setError(null);
    if (!name.trim()) return setError('任务名称不能为空');
    if (!cron.trim()) return setError('cron 表达式不能为空');
    if (!tplId) return setError('请选择模板');
    if (wsIds.length === 0) return setError('请选择至少一个工作区');
    if (recipients.length === 0) return setError('请填写至少一个收件人');

    setSaving(true);
    try {
      await saveSchedule({
        id: initial.id || '',
        name,
        cron: cron.trim(),
        enabled,
        workspace_ids: wsIds,
        template_id: tplId,
        provider_id: provId || null,
        days: Number(days) || 7,
        recipients,
        cc,
        subject_tpl: subjectTpl,
        last_run: initial.last_run || null,
        last_status: initial.last_status || null,
        next_run: null,
      });
      onSaved();
    } catch (e) {
      setError(formatError(e));
      setSaving(false);
    }
  }

  return (
    <Modal onClose={onClose} width="max-w-2xl">
      <ModalHeader title={initial.id ? '编辑定时任务' : '新建定时任务'} onClose={onClose} />
      <ModalBody className="space-y-4">
        <FormField label="任务名称">
          <Input value={name} onChange={setName} placeholder="如：周五技术周报" />
        </FormField>

        <FormField
          label="cron 表达式（7 段：秒 分 时 日 月 星期 年）"
          hint="与系统 cron 不一样，UTC 时区由 tokio-cron-scheduler 处理"
        >
          <Mono value={cron} onChange={setCron} />
          <div className="mt-1.5 flex flex-wrap gap-1.5">
            {CRON_PRESETS.map((p, i) => (
              <button
                key={i}
                type="button"
                onClick={() => setCron(p.cron)}
                className="rounded border border-stone-200 bg-white px-2.5 py-0.5 text-[11.5px] text-stone-600 hover:border-stone-300 hover:text-stone-900"
              >
                {p.label}
              </button>
            ))}
          </div>
        </FormField>

        <div className="grid grid-cols-2 gap-3">
          <FormField label="模板">
            <Select value={tplId} onChange={setTplId}>
              <option value="">（请选择）</option>
              {templates.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                </option>
              ))}
            </Select>
          </FormField>
          <FormField label="时间范围">
            <Select value={String(days)} onChange={(v) => setDays(Number(v))}>
              {DAY_OPTIONS.map((n) => (
                <option key={n} value={String(n)}>
                  最近 {n} 天
                </option>
              ))}
            </Select>
          </FormField>
        </div>

        <FormField label="LLM 源（可选，留空则按模板 / 默认源）">
          <Select value={provId} onChange={setProvId}>
            <option value="">按模板 / 默认</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
                {p.is_default ? '（默认）' : ''}
              </option>
            ))}
          </Select>
        </FormField>

        <FormField label="工作区">
          <div className="space-y-1">
            {workspaces.map((w) => (
              <label key={w.id} className="flex items-center gap-2 text-[13px]">
                <input
                  type="checkbox"
                  checked={wsIds.includes(w.id)}
                  onChange={() => toggleWs(w.id)}
                />
                <span>{w.name}</span>
              </label>
            ))}
            {workspaces.length === 0 && (
              <p className="text-[12.5px] text-stone-500">还没有工作区</p>
            )}
          </div>
        </FormField>

        <FormField label="邮件主题模板" hint="支持 {date} 和 {week} 变量">
          <Input value={subjectTpl} onChange={setSubjectTpl} />
        </FormField>

        <FormField label="收件人邮箱" hint="多个用逗号、空格或分号分隔">
          <Textarea value={recipientsText} onChange={setRecipientsText} rows={2} />
        </FormField>

        <FormField label="抄送（可选）" hint="多个用逗号、空格或分号分隔">
          <Textarea value={ccText} onChange={setCcText} rows={2} />
        </FormField>

        <label className="flex items-center gap-2 text-[13px]">
          <Toggle on={enabled} onChange={setEnabled} />
          <span>启用</span>
        </label>

        {error && (
          <div className="rounded border border-rose-200 bg-rose-50 px-3 py-2 text-[12.5px] text-rose-700">
            {error}
          </div>
        )}
      </ModalBody>
      <ModalFooter>
        <div />
        <div className="flex gap-2">
          <SecondaryButton onClick={onClose} disabled={saving}>
            取消
          </SecondaryButton>
          <PrimaryButton onClick={handleSave} disabled={saving}>
            {saving ? '保存中…' : '保存'}
          </PrimaryButton>
        </div>
      </ModalFooter>
    </Modal>
  );
}

function splitEmails(text) {
  return (text || '')
    .split(/[,;\s]+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

function formatIso(s) {
  if (!s) return '';
  return s.replace('T', ' ').slice(0, 16);
}
