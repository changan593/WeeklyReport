// 周报模板页（详见 docs/UI.md#43-周报模板页-templatesjsx）。
//
// 内置 3 个模板（builtin: true）不可修改不可删除；用户可新建自定义模板。
// 编辑器支持选择关联 LLM 源 / 章节增删 / 额外要求 prompt。

import { useEffect, useState } from 'react';
import {
  deleteTemplate,
  listProviders,
  listSchedules,
  listTemplates,
  saveTemplate,
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
  PrimaryButton,
  SecondaryButton,
  Select,
  Textarea,
} from './ui.jsx';

const STYLE_COLOR = {
  tech: 'bg-blue-50 text-blue-700',
  exec: 'bg-purple-50 text-purple-700',
  simple: 'bg-amber-50 text-amber-700',
  custom: 'bg-stone-100 text-stone-600',
};

function styleLabelKey(style) {
  return `templates.style.${STYLE_COLOR[style] ? style : 'custom'}`;
}

export default function Templates() {
  const { t } = useTranslation();
  const [items, loading, reload] = useAsyncState(listTemplates, []);
  const [providers, setProviders] = useState([]);
  const [scheduleUsage, setScheduleUsage] = useState({});
  const [editing, setEditing] = useState(null);

  useEffect(() => {
    listProviders().then(setProviders).catch(() => setProviders([]));
    // 取每个模板被定时任务引用的次数，用于卡片角标提示"已在 N 个任务中使用"
    listSchedules()
      .then((schedules) => {
        const counts = {};
        for (const s of schedules || []) {
          if (s.template_id) counts[s.template_id] = (counts[s.template_id] || 0) + 1;
        }
        setScheduleUsage(counts);
      })
      .catch(() => setScheduleUsage({}));
  }, []);

  async function handleDelete(tpl) {
    if (!confirm(t('templates.confirm_delete', { name: tpl.name }))) return;
    try {
      await deleteTemplate(tpl.id);
      reload();
    } catch (e) {
      alert(formatError(e));
    }
  }

  return (
    <div>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <h1 className="text-2xl font-medium text-stone-900">{t('templates.title')}</h1>
          <p className="mt-1 text-[13px] text-stone-500">{t('templates.subtitle')}</p>
        </div>
        <PrimaryButton onClick={() => setEditing(emptyTemplate(t))}>
          <Icon name="plus" size={15} /> {t('templates.add')}
        </PrimaryButton>
      </header>

      {loading && <LoadingState />}
      {!loading && items && items.length === 0 && (
        <EmptyState iconName="template" message={t('templates.empty')} />
      )}
      {!loading && items && items.length > 0 && (
        <div className="grid grid-cols-2 gap-3">
          {items.map((tpl) => (
            <TemplateCard
              key={tpl.id}
              template={tpl}
              usageCount={scheduleUsage[tpl.id] || 0}
              onEdit={() => setEditing(tpl)}
              onDelete={() => handleDelete(tpl)}
            />
          ))}
        </div>
      )}

      {editing && (
        <TemplateEditor
          initial={editing}
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

function TemplateCard({ template, usageCount = 0, onEdit, onDelete }) {
  const { t } = useTranslation();
  const color = STYLE_COLOR[template.style] || STYLE_COLOR.custom;
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-stone-200 bg-white p-5 hover:border-stone-300">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[14px] font-medium text-stone-900">{template.name}</span>
        <span className={`rounded px-1.5 py-0.5 text-[11px] ${color}`}>
          {t(styleLabelKey(template.style))}
        </span>
        {usageCount > 0 && (
          <span
            className="inline-flex items-center gap-1 rounded bg-sky-50 px-1.5 py-0.5 text-[11px] text-sky-700"
            title={t('templates.card.used_by_tip')}
          >
            <Icon name="schedule" size={11} />
            {t('templates.card.used_by', { n: usageCount })}
          </span>
        )}
      </div>
      <div className="text-[11.5px] text-stone-500">
        {t('templates.card.sections_count', { n: template.sections?.length || 0 })}
      </div>
      <ol className="space-y-0.5 text-[12.5px] text-stone-700">
        {template.sections?.map((s, i) => (
          <li key={i}>
            <span className="mr-1.5 text-stone-400">{i + 1}.</span>
            {s}
          </li>
        ))}
      </ol>
      <div className="mt-1 flex items-center justify-between border-t border-stone-100 pt-2">
        <span className="text-[11px] text-stone-500">
          {template.builtin ? t('templates.card.builtin') : t('templates.card.custom')}
        </span>
        <div className="flex gap-0.5">
          <IconButton
            title={template.builtin ? t('templates.card.edit_disabled') : t('templates.card.edit')}
            onClick={onEdit}
            disabled={template.builtin}
          >
            <Icon name="edit" size={14} />
          </IconButton>
          <IconButton
            title={template.builtin ? t('templates.card.delete_disabled') : t('templates.card.delete')}
            onClick={onDelete}
            disabled={template.builtin}
          >
            <Icon name="trash" size={14} />
          </IconButton>
        </div>
      </div>
    </div>
  );
}

function emptyTemplate(t) {
  return {
    id: '',
    name: '',
    style: 'custom',
    sections: [t('templates.default_sections.overview'), t('templates.default_sections.plan')],
    provider_id: null,
    extra_prompt: '',
    builtin: false,
  };
}

function TemplateEditor({ initial, providers, onClose, onSaved }) {
  const { t } = useTranslation();
  const [tpl, setTpl] = useState({
    ...initial,
    provider_id: initial.provider_id ?? null,
    sections: Array.isArray(initial.sections) ? initial.sections : [],
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState(null);
  const disabled = !!tpl.builtin;

  function set(field, value) {
    setTpl((prev) => ({ ...prev, [field]: value }));
  }

  function updateSection(idx, value) {
    setTpl((prev) => {
      const next = [...prev.sections];
      next[idx] = value;
      return { ...prev, sections: next };
    });
  }

  function removeSection(idx) {
    setTpl((prev) => ({
      ...prev,
      sections: prev.sections.filter((_, i) => i !== idx),
    }));
  }

  function addSection() {
    setTpl((prev) => ({ ...prev, sections: [...prev.sections, ''] }));
  }

  async function handleSave() {
    setError(null);
    if (!tpl.name.trim()) {
      setError(t('templates.editor.name_required'));
      return;
    }
    const sections = tpl.sections.map((s) => s.trim()).filter((s) => s.length > 0);
    if (sections.length === 0) {
      setError(t('templates.editor.sections_required'));
      return;
    }
    setSaving(true);
    try {
      await saveTemplate({
        ...tpl,
        sections,
        provider_id: tpl.provider_id || null,
      });
      onSaved();
    } catch (e) {
      setError(formatError(e));
      setSaving(false);
    }
  }

  const title = disabled
    ? t('templates.editor.title_view')
    : initial.id
      ? t('templates.editor.title_edit')
      : t('templates.editor.title_new');

  return (
    <Modal onClose={onClose} width="max-w-2xl">
      <ModalHeader title={title} onClose={onClose} />
      <ModalBody className="space-y-4">
        <FormField label={t('templates.editor.name')}>
          <Input value={tpl.name} onChange={(v) => set('name', v)} disabled={disabled} />
        </FormField>

        <div className="grid grid-cols-2 gap-3">
          <FormField label={t('templates.editor.style')}>
            <Select value={tpl.style} onChange={(v) => set('style', v)} disabled={disabled}>
              <option value="tech">{t('templates.style.tech')}</option>
              <option value="exec">{t('templates.style.exec')}</option>
              <option value="simple">{t('templates.style.simple')}</option>
              <option value="custom">{t('templates.style.custom')}</option>
            </Select>
          </FormField>
          <FormField label={t('templates.editor.llm')} hint={t('templates.editor.llm_hint')}>
            <Select
              value={tpl.provider_id || ''}
              onChange={(v) => set('provider_id', v || null)}
              disabled={disabled}
            >
              <option value="">{t('templates.editor.llm_default')}</option>
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </Select>
          </FormField>
        </div>

        <FormField label={t('templates.editor.sections')} hint={t('templates.editor.sections_hint')}>
          <div className="space-y-1.5">
            {tpl.sections.map((s, i) => (
              <div key={i} className="flex items-center gap-2">
                <span className="w-5 text-right text-[12px] text-stone-400">{i + 1}.</span>
                <Input
                  value={s}
                  onChange={(v) => updateSection(i, v)}
                  disabled={disabled}
                  className="flex-1"
                />
                {!disabled && (
                  <IconButton title={t('templates.editor.section_delete')} onClick={() => removeSection(i)}>
                    <Icon name="trash" size={14} />
                  </IconButton>
                )}
              </div>
            ))}
            {!disabled && (
              <SecondaryButton onClick={addSection} className="mt-1">
                <Icon name="plus" size={14} />
                {t('templates.editor.section_add')}
              </SecondaryButton>
            )}
          </div>
        </FormField>

        <FormField label={t('templates.editor.extra')} hint={t('templates.editor.extra_hint')}>
          <Textarea
            value={tpl.extra_prompt}
            onChange={(v) => set('extra_prompt', v)}
            disabled={disabled}
            rows={3}
            placeholder={t('templates.editor.extra_placeholder')}
          />
        </FormField>

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
            {disabled ? t('templates.editor.close') : t('common.cancel')}
          </SecondaryButton>
          {!disabled && (
            <PrimaryButton onClick={handleSave} disabled={saving}>
              {saving ? t('common.saving') : t('common.save')}
            </PrimaryButton>
          )}
        </div>
      </ModalFooter>
    </Modal>
  );
}
