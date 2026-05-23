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
import { useTranslation } from '../i18n/index.jsx';
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
  StatusPill,
  Textarea,
  Toggle,
} from './ui.jsx';
import { splitEmails, formatIsoMinute as formatIso } from '../utils.js';

// 4 个常用 cron 预设（7 段格式，**按 UTC 解释**）
// 注：这些是 UTC 时间。中国大陆用户实际本地触发时刻 +8h（如 17:30 UTC = 北京 01:30 次日）。
const CRON_PRESETS = [
  { i18nKey: 'schedules.preset.friday', cron: '0 30 17 ? * FRI *' },
  { i18nKey: 'schedules.preset.monday', cron: '0 0 9 ? * MON *' },
  { i18nKey: 'schedules.preset.weekdays', cron: '0 0 18 ? * MON-FRI *' },
  { i18nKey: 'schedules.preset.sunday', cron: '0 0 21 ? * SUN *' },
];

const DAY_OPTIONS = [3, 7, 14, 30];

export default function Schedules({ navigate }) {
  const { t } = useTranslation();
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
    if (!confirm(t('schedules.confirm_delete', { name: view.name }))) return;
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
          <h1 className="text-2xl font-medium text-stone-900">{t('schedules.title')}</h1>
          <p className="mt-1 text-[13px] text-stone-500">{t('schedules.subtitle')}</p>
        </div>
        <PrimaryButton onClick={() => setEditing(emptySchedule(t))}>
          <Icon name="plus" size={15} /> {t('schedules.add')}
        </PrimaryButton>
      </header>

      {!smtpConfigured && (
        <div className="mb-4 flex items-start justify-between gap-3 rounded border border-amber-200 bg-amber-50 px-3 py-2 text-[12.5px] text-amber-700">
          <span>{t('schedules.no_smtp_warning')}</span>
          {navigate && (
            <button
              type="button"
              onClick={() => navigate('settings')}
              className="shrink-0 rounded border border-amber-300 bg-white px-2 py-0.5 text-[11.5px] text-amber-700 hover:border-amber-400"
            >
              {t('schedules.go_settings')}
            </button>
          )}
        </div>
      )}

      {loading && <LoadingState />}
      {!loading && items && items.length === 0 && (
        <EmptyState iconName="schedule" message={t('schedules.empty')} />
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
  const { t } = useTranslation();
  const enabled = view.enabled;
  const lastStatus = view.last_status || '';
  const isFail = lastStatus.startsWith('failed');
  const pill = (() => {
    if (!enabled) return { tone: 'neutral', label: t('schedules.card.status.disabled') };
    if (isFail) return { tone: 'error', label: t('schedules.card.status.failed'), title: lastStatus };
    if (lastStatus) return { tone: 'success', label: t('schedules.card.status.ok'), title: lastStatus };
    return { tone: 'info', label: t('schedules.card.status.scheduled') };
  })();
  return (
    <div className="flex items-start gap-3 rounded-lg border border-stone-200 bg-white p-5 hover:border-stone-300">
      <div
        className={`mt-0.5 flex h-9 w-9 items-center justify-center rounded-lg ${
          enabled
            ? isFail
              ? 'bg-rose-50 text-rose-600'
              : 'bg-emerald-50 text-emerald-700'
            : 'bg-stone-100 text-stone-400'
        }`}
      >
        <Icon name="clock" size={18} />
      </div>
      <div className="flex-1 overflow-hidden">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[14px] font-medium text-stone-900">{view.name}</span>
          <StatusPill tone={pill.tone} label={pill.label} title={pill.title} />
          <code className="rounded bg-stone-100 px-1.5 py-0.5 font-mono text-[11px] text-stone-600">
            {view.cron}
          </code>
        </div>
        <div className="mt-1 text-[12px] text-stone-500">
          {t('schedules.card.recipients', { n: view.recipients?.length || 0 })}
          {view.cc?.length > 0 && ` · ${t('schedules.card.cc', { n: view.cc.length })}`}
        </div>
        <div className="mt-1 flex flex-wrap gap-x-3 text-[11.5px] text-stone-500">
          {view.next_run_computed && enabled && (
            <span>{t('schedules.card.next', { time: formatIso(view.next_run_computed) })}</span>
          )}
          {view.last_run && <span>{t('schedules.card.last', { time: formatIso(view.last_run) })}</span>}
        </div>
        {isFail && lastStatus && (
          <div className="mt-1 line-clamp-2 text-[11.5px] text-rose-700">{lastStatus}</div>
        )}
      </div>
      <div className="flex flex-col items-end gap-2">
        <Toggle on={enabled} onChange={onToggle} />
        <div className="flex gap-0.5">
          <IconButton title={t('schedules.card.run_now')} onClick={onRunNow}>
            <Icon name="play" size={14} />
          </IconButton>
          <IconButton title={t('schedules.card.edit')} onClick={onEdit}>
            <Icon name="edit" size={14} />
          </IconButton>
          <IconButton title={t('schedules.card.delete')} onClick={onDelete}>
            <Icon name="trash" size={14} />
          </IconButton>
        </div>
      </div>
    </div>
  );
}

function emptySchedule(t) {
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
    subject_tpl: t('schedules.editor.subject_tpl_default'),
    last_run: null,
    last_status: null,
    next_run: null,
  };
}

function ScheduleEditor({ initial, workspaces, templates, providers, onClose, onSaved }) {
  const { t } = useTranslation();
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
  const [subjectTpl, setSubjectTpl] = useState(
    initial.subject_tpl || t('schedules.editor.subject_tpl_default'),
  );
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
    if (!name.trim()) return setError(t('schedules.editor.name_required'));
    if (!cron.trim()) return setError(t('schedules.editor.cron_required'));
    if (!tplId) return setError(t('schedules.editor.template_required'));
    if (wsIds.length === 0) return setError(t('schedules.editor.workspace_required'));
    if (recipients.length === 0) return setError(t('schedules.editor.recipients_required'));

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
      <ModalHeader
        title={initial.id ? t('schedules.editor.title_edit') : t('schedules.editor.title_new')}
        onClose={onClose}
      />
      <ModalBody className="space-y-4">
        <FormField label={t('schedules.editor.name')}>
          <Input value={name} onChange={setName} placeholder={t('schedules.editor.name_placeholder')} />
        </FormField>

        <FormField label={t('schedules.editor.cron')} hint={t('schedules.editor.cron_hint')}>
          <Mono value={cron} onChange={setCron} />
          <div className="mt-1.5 flex flex-wrap gap-1.5">
            {CRON_PRESETS.map((p, i) => (
              <button
                key={i}
                type="button"
                onClick={() => setCron(p.cron)}
                className="rounded border border-stone-200 bg-white px-2.5 py-0.5 text-[11.5px] text-stone-600 hover:border-stone-300 hover:text-stone-900"
              >
                {t(p.i18nKey)}
              </button>
            ))}
          </div>
        </FormField>

        <div className="grid grid-cols-2 gap-3">
          <FormField label={t('schedules.editor.template')}>
            <Select value={tplId} onChange={setTplId}>
              <option value="">{t('schedules.editor.template_placeholder')}</option>
              {templates.map((tpl) => (
                <option key={tpl.id} value={tpl.id}>
                  {tpl.name}
                </option>
              ))}
            </Select>
          </FormField>
          <FormField label={t('schedules.editor.days')}>
            <Select value={String(days)} onChange={(v) => setDays(Number(v))}>
              {DAY_OPTIONS.map((n) => (
                <option key={n} value={String(n)}>
                  {t('schedules.editor.days_unit', { n })}
                </option>
              ))}
            </Select>
          </FormField>
        </div>

        <FormField label={t('schedules.editor.llm')}>
          <Select value={provId} onChange={setProvId}>
            <option value="">{t('schedules.editor.llm_default')}</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
                {p.is_default ? t('schedules.editor.llm_default_suffix') : ''}
              </option>
            ))}
          </Select>
        </FormField>

        <FormField label={t('schedules.editor.workspace')}>
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
              <p className="text-[12.5px] text-stone-500">{t('schedules.editor.workspace_empty')}</p>
            )}
          </div>
        </FormField>

        <FormField label={t('schedules.editor.subject_tpl')} hint={t('schedules.editor.subject_tpl_hint')}>
          <Input value={subjectTpl} onChange={setSubjectTpl} />
        </FormField>

        <FormField label={t('schedules.editor.recipients')} hint={t('schedules.editor.recipients_hint')}>
          <Textarea value={recipientsText} onChange={setRecipientsText} rows={2} />
        </FormField>

        <FormField label={t('schedules.editor.cc')} hint={t('schedules.editor.cc_hint')}>
          <Textarea value={ccText} onChange={setCcText} rows={2} />
        </FormField>

        <label className="flex items-center gap-2 text-[13px]">
          <Toggle on={enabled} onChange={setEnabled} />
          <span>{t('schedules.editor.enabled')}</span>
        </label>

        {error && (
          <div className="rounded border border-rose-200 bg-rose-50 px-3 py-2 text-[12.5px] text-rose-700">
            {error}
          </div>
        )}
        <StatusBanner status={null} />
      </ModalBody>
      <ModalFooter>
        <div />
        <div className="flex gap-2">
          <SecondaryButton onClick={onClose} disabled={saving}>
            {t('common.cancel')}
          </SecondaryButton>
          <PrimaryButton onClick={handleSave} disabled={saving}>
            {saving ? t('common.saving') : t('common.save')}
          </PrimaryButton>
        </div>
      </ModalFooter>
    </Modal>
  );
}

// splitEmails / formatIso 已迁移到 src/utils.js（便于单测）。导入在文件顶部。
