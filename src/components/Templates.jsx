// 周报模板页（详见 docs/UI.md#43-周报模板页-templatesjsx）。
//
// 内置 3 个模板（builtin: true）不可修改不可删除；用户可新建自定义模板。
// 编辑器支持选择关联 LLM 源 / 章节增删 / 额外要求 prompt。

import { useEffect, useState } from 'react';
import {
  deleteTemplate,
  listProviders,
  listTemplates,
  saveTemplate,
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
  PrimaryButton,
  SecondaryButton,
  Select,
  Textarea,
} from './ui.jsx';

const STYLE_LABELS = {
  tech: { label: '技术向', color: 'bg-blue-50 text-blue-700' },
  exec: { label: '管理层', color: 'bg-purple-50 text-purple-700' },
  simple: { label: '简洁', color: 'bg-amber-50 text-amber-700' },
  custom: { label: '自定义', color: 'bg-stone-100 text-stone-600' },
};

export default function Templates() {
  const [items, loading, reload] = useAsyncState(listTemplates, []);
  const [providers, setProviders] = useState([]);
  const [editing, setEditing] = useState(null);

  useEffect(() => {
    listProviders().then(setProviders).catch(() => setProviders([]));
  }, []);

  async function handleDelete(t) {
    if (!confirm(`删除模板「${t.name}」？`)) return;
    try {
      await deleteTemplate(t.id);
      reload();
    } catch (e) {
      alert(formatError(e));
    }
  }

  return (
    <div>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <h1 className="text-2xl font-medium text-stone-900">周报模板</h1>
          <p className="mt-1 text-[13px] text-stone-500">
            定义周报的结构与风格；可绑定特定 LLM 源
          </p>
        </div>
        <PrimaryButton onClick={() => setEditing(emptyTemplate())}>
          <Icon name="plus" size={15} /> 新建模板
        </PrimaryButton>
      </header>

      {loading && <LoadingState />}
      {!loading && items && items.length === 0 && (
        <EmptyState iconName="template" message="还没有模板" />
      )}
      {!loading && items && items.length > 0 && (
        <div className="grid grid-cols-2 gap-3">
          {items.map((t) => (
            <TemplateCard
              key={t.id}
              template={t}
              onEdit={() => setEditing(t)}
              onDelete={() => handleDelete(t)}
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

function TemplateCard({ template, onEdit, onDelete }) {
  const meta = STYLE_LABELS[template.style] || STYLE_LABELS.custom;
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-stone-200 bg-white p-5 hover:border-stone-300">
      <div className="flex items-center gap-2">
        <span className="text-[14px] font-medium text-stone-900">{template.name}</span>
        <span className={`rounded px-1.5 py-0.5 text-[11px] ${meta.color}`}>
          {meta.label}
        </span>
      </div>
      <div className="text-[11.5px] text-stone-500">
        {template.sections?.length || 0} 个章节
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
          {template.builtin ? '内置' : '自定义'}
        </span>
        <div className="flex gap-0.5">
          <IconButton
            title={template.builtin ? '内置模板不可编辑' : '编辑'}
            onClick={onEdit}
            disabled={template.builtin}
          >
            <Icon name="edit" size={14} />
          </IconButton>
          <IconButton
            title={template.builtin ? '内置模板不可删除' : '删除'}
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

function emptyTemplate() {
  return {
    id: '',
    name: '',
    style: 'custom',
    sections: ['本周概览', '下周计划'],
    provider_id: null,
    extra_prompt: '',
    builtin: false,
  };
}

function TemplateEditor({ initial, providers, onClose, onSaved }) {
  const [t, setT] = useState({
    ...initial,
    provider_id: initial.provider_id ?? null,
    sections: Array.isArray(initial.sections) ? initial.sections : [],
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState(null);
  const disabled = !!t.builtin;

  function set(field, value) {
    setT((prev) => ({ ...prev, [field]: value }));
  }

  function updateSection(idx, value) {
    setT((prev) => {
      const next = [...prev.sections];
      next[idx] = value;
      return { ...prev, sections: next };
    });
  }

  function removeSection(idx) {
    setT((prev) => ({
      ...prev,
      sections: prev.sections.filter((_, i) => i !== idx),
    }));
  }

  function addSection() {
    setT((prev) => ({ ...prev, sections: [...prev.sections, ''] }));
  }

  async function handleSave() {
    setError(null);
    if (!t.name.trim()) {
      setError('模板名称不能为空');
      return;
    }
    const sections = t.sections.map((s) => s.trim()).filter((s) => s.length > 0);
    if (sections.length === 0) {
      setError('至少需要一个章节');
      return;
    }
    setSaving(true);
    try {
      await saveTemplate({
        ...t,
        sections,
        provider_id: t.provider_id || null,
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
        title={disabled ? '查看模板（内置）' : initial.id ? '编辑模板' : '新建模板'}
        onClose={onClose}
      />
      <ModalBody className="space-y-4">
        <FormField label="模板名称">
          <Input value={t.name} onChange={(v) => set('name', v)} disabled={disabled} />
        </FormField>

        <div className="grid grid-cols-2 gap-3">
          <FormField label="风格">
            <Select value={t.style} onChange={(v) => set('style', v)} disabled={disabled}>
              <option value="tech">技术向</option>
              <option value="exec">管理层</option>
              <option value="simple">简洁</option>
              <option value="custom">自定义</option>
            </Select>
          </FormField>
          <FormField label="指定 LLM 源" hint="留空使用全局默认源">
            <Select
              value={t.provider_id || ''}
              onChange={(v) => set('provider_id', v || null)}
              disabled={disabled}
            >
              <option value="">使用默认 LLM 源</option>
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </Select>
          </FormField>
        </div>

        <FormField label="章节列表" hint="顺序即输出顺序">
          <div className="space-y-1.5">
            {t.sections.map((s, i) => (
              <div key={i} className="flex items-center gap-2">
                <span className="w-5 text-right text-[12px] text-stone-400">{i + 1}.</span>
                <Input
                  value={s}
                  onChange={(v) => updateSection(i, v)}
                  disabled={disabled}
                  className="flex-1"
                />
                {!disabled && (
                  <IconButton title="删除此章节" onClick={() => removeSection(i)}>
                    <Icon name="trash" size={14} />
                  </IconButton>
                )}
              </div>
            ))}
            {!disabled && (
              <SecondaryButton onClick={addSection} className="mt-1">
                <Icon name="plus" size={14} />
                添加章节
              </SecondaryButton>
            )}
          </div>
        </FormField>

        <FormField label="额外要求（可选）" hint="会拼接到 prompt 末尾">
          <Textarea
            value={t.extra_prompt}
            onChange={(v) => set('extra_prompt', v)}
            disabled={disabled}
            rows={3}
            placeholder="如：风格要正式；只输出三个段落；用代码块展示命令"
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
            {disabled ? '关闭' : '取消'}
          </SecondaryButton>
          {!disabled && (
            <PrimaryButton onClick={handleSave} disabled={saving}>
              {saving ? '保存中…' : '保存'}
            </PrimaryButton>
          )}
        </div>
      </ModalFooter>
    </Modal>
  );
}
