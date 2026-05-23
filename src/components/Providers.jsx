// LLM 源页（详见 docs/UI.md#42-llm-源页-providersjsx）。
//
// 功能：列出 / 添加 / 编辑 / 删除 LLM 源；测试连接；10 个预设一键填充；
// 切换默认源（保存 is_default=true 时其他自动取消，由后端 state.rs 维护）。

import { useEffect, useState } from 'react';
import {
  deleteProvider,
  listProviders,
  llmPresets,
  saveProvider,
  testProvider,
  useAsyncState,
  formatError,
} from '../api.js';
import { withTimeout } from '../utils.js';
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
} from './ui.jsx';

const KIND_I18N_KEYS = {
  OpenAiCompatible: 'providers.kind.openai',
  Anthropic: 'providers.kind.anthropic',
  Gemini: 'providers.kind.gemini',
};

export default function Providers() {
  const { t } = useTranslation();
  const [items, loading, reload] = useAsyncState(listProviders, []);
  const [presets, setPresets] = useState([]);
  const [editing, setEditing] = useState(null);
  // 会话级测试状态：id → { state: 'idle'|'testing'|'ok'|'failed', message }
  const [testStatus, setTestStatus] = useState({});

  useEffect(() => {
    llmPresets()
      .then(setPresets)
      .catch(() => setPresets([]));
  }, []);

  async function handleDelete(p) {
    if (!confirm(t('providers.confirm_delete', { name: p.name }))) return;
    try {
      await deleteProvider(p.id);
      reload();
    } catch (e) {
      alert(formatError(e));
    }
  }

  async function setAsDefault(p) {
    try {
      await saveProvider({ ...p, is_default: true });
      reload();
    } catch (e) {
      alert(formatError(e));
    }
  }

  async function handleTest(p) {
    setTestStatus((prev) => ({ ...prev, [p.id]: { state: 'testing' } }));
    try {
      // 后端 LLM 超时是 120s；前端给 130s 兜底，避免极端网络下 UI 永远卡在「测试中」。
      const msg = await withTimeout(testProvider(p), 130_000);
      setTestStatus((prev) => ({ ...prev, [p.id]: { state: 'ok', message: msg } }));
    } catch (e) {
      // eslint-disable-next-line no-console
      console.error('[provider test failed]', p?.id, e);
      setTestStatus((prev) => ({
        ...prev,
        [p.id]: { state: 'failed', message: formatError(e) },
      }));
    }
  }

  return (
    <div>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <h1 className="text-2xl font-medium text-stone-900">{t('providers.title')}</h1>
          <p className="mt-1 text-[13px] text-stone-500">{t('providers.subtitle')}</p>
        </div>
        <PrimaryButton onClick={() => setEditing(emptyProvider())}>
          <Icon name="plus" size={15} /> {t('providers.add')}
        </PrimaryButton>
      </header>

      {loading && <LoadingState />}
      {!loading && items && items.length === 0 && (
        <EmptyState iconName="llm" message={t('providers.empty')}>
          <div className="flex flex-wrap justify-center gap-2 px-4">
            {presets.slice(0, 6).map((p, i) => (
              <button
                key={i}
                type="button"
                onClick={() => setEditing({ ...p })}
                className="rounded border border-stone-200 bg-white px-3 py-1.5 text-[12px] text-stone-600 hover:border-stone-300 hover:text-stone-900"
              >
                {p.name}
              </button>
            ))}
          </div>
        </EmptyState>
      )}
      {!loading && items && items.length > 0 && (
        <div className="space-y-3">
          {items.map((p) => (
            <ProviderCard
              key={p.id}
              provider={p}
              status={testStatus[p.id]}
              onEdit={() => setEditing(p)}
              onDelete={() => handleDelete(p)}
              onSetDefault={() => setAsDefault(p)}
              onTest={() => handleTest(p)}
            />
          ))}
        </div>
      )}

      {editing && (
        <ProviderEditor
          initial={editing}
          presets={presets}
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

function ProviderCard({ provider, status, onEdit, onDelete, onSetDefault, onTest }) {
  const { t } = useTranslation();
  const pill = computeProviderPill(provider, status, t);
  const testing = status?.state === 'testing';
  return (
    <div className="flex items-start gap-3 rounded-lg border border-stone-200 bg-white p-5 hover:border-stone-300">
      <div
        className={`mt-0.5 flex h-9 w-9 items-center justify-center rounded-lg ${
          provider.is_default ? 'bg-amber-50 text-amber-700' : 'bg-stone-100 text-stone-500'
        }`}
      >
        <Icon name="llm" size={18} />
      </div>
      <div className="flex-1 overflow-hidden">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[14px] font-medium text-stone-900">{provider.name}</span>
          <StatusPill tone={pill.tone} label={pill.label} title={pill.title} pulse={pill.pulse} />
          {provider.is_default && (
            <span className="inline-flex items-center gap-1 rounded bg-amber-50 px-1.5 py-0.5 text-[11px] text-amber-700">
              <Icon name="star" size={11} /> {t('providers.card.default_badge')}
            </span>
          )}
          <span className="rounded bg-stone-100 px-1.5 py-0.5 text-[11px] text-stone-600">
            {KIND_I18N_KEYS[provider.kind] ? t(KIND_I18N_KEYS[provider.kind]) : provider.kind}
          </span>
        </div>
        <div className="mt-1 truncate font-mono text-[12px] text-stone-500">
          {provider.base_url}
        </div>
        <div className="mt-2 flex flex-wrap gap-1.5 text-[11px]">
          <span className="rounded bg-orange-50 px-1.5 py-0.5 font-mono text-orange-700">
            {provider.model || t('providers.card.no_model')}
          </span>
          <span className="rounded bg-stone-100 px-1.5 py-0.5 text-stone-600">
            max_tokens: {provider.max_tokens}
          </span>
          {provider.temperature !== null && provider.temperature !== undefined && (
            <span className="rounded bg-stone-100 px-1.5 py-0.5 text-stone-600">
              temp: {provider.temperature}
            </span>
          )}
        </div>
        {status?.state === 'failed' && status.message && (
          <div className="mt-2 line-clamp-2 text-[11.5px] text-rose-700">{status.message}</div>
        )}
      </div>
      <div className="flex flex-col items-end gap-1">
        {!provider.is_default && (
          <button
            type="button"
            onClick={onSetDefault}
            className="rounded px-2 py-0.5 text-[11.5px] text-stone-500 hover:text-stone-900"
          >
            {t('providers.card.set_default')}
          </button>
        )}
        <div className="flex gap-0.5">
          <IconButton title={t('providers.card.test')} onClick={onTest} disabled={testing}>
            <Icon name="refresh" size={15} className={testing ? 'animate-spin' : ''} />
          </IconButton>
          <IconButton title={t('providers.card.edit')} onClick={onEdit}>
            <Icon name="edit" size={15} />
          </IconButton>
          <IconButton title={t('providers.card.delete')} onClick={onDelete}>
            <Icon name="trash" size={15} />
          </IconButton>
        </div>
      </div>
    </div>
  );
}

/// 决定 LLM 源卡片的状态徽标：
/// - 缺 api_key / model / base_url → warning
/// - 测试中 → info（pulse）
/// - 测过成功 → success；失败 → error
/// - 否则 → success（已配置但未测试）
function computeProviderPill(provider, status, t) {
  const st = status?.state;
  if (st === 'testing') {
    return { tone: 'info', label: t('providers.card.status.testing'), pulse: true };
  }
  if (st === 'ok') {
    return {
      tone: 'success',
      label: t('providers.card.status.ok'),
      title: status.message,
    };
  }
  if (st === 'failed') {
    return {
      tone: 'error',
      label: t('providers.card.status.failed'),
      title: status.message,
    };
  }
  // 未测过：检查配置完整度
  if (!provider.api_key) {
    return { tone: 'warning', label: t('providers.card.status.missing_key') };
  }
  if (!provider.model) {
    return { tone: 'warning', label: t('providers.card.status.missing_model') };
  }
  if (!provider.base_url) {
    return { tone: 'warning', label: t('providers.card.status.missing_url') };
  }
  return { tone: 'success', label: t('providers.card.status.configured') };
}

function emptyProvider() {
  return {
    id: '',
    name: '',
    kind: 'OpenAiCompatible',
    base_url: '',
    api_key: '',
    model: '',
    max_tokens: 2048,
    temperature: 0.7,
    is_default: false,
    extra_headers: {},
  };
}

function ProviderEditor({ initial, presets, onClose, onSaved }) {
  const { t } = useTranslation();
  const [p, setP] = useState({ ...initial });
  const [status, setStatus] = useState(null);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);

  function set(field, value) {
    setP((prev) => ({ ...prev, [field]: value }));
  }

  function applyPreset(preset) {
    setP((prev) => ({
      ...prev,
      // 保留 id / name / api_key（如果用户已填）；其他字段取预设
      name: prev.name || preset.name,
      kind: preset.kind,
      base_url: preset.base_url,
      model: preset.model,
      max_tokens: preset.max_tokens,
      temperature: preset.temperature,
      api_key: prev.api_key || preset.api_key,
    }));
    setStatus({ type: 'info', msg: t('providers.editor.preset_applied', { name: preset.name }) });
  }

  async function handleTest() {
    setTesting(true);
    setStatus({ type: 'info', msg: t('providers.editor.testing') });
    try {
      const payload = normalize(p);
      const msg = await testProvider(payload);
      setStatus({ type: 'success', msg });
    } catch (e) {
      setStatus({ type: 'error', msg: formatError(e) });
    } finally {
      setTesting(false);
    }
  }

  async function handleSave() {
    if (!p.name.trim()) {
      setStatus({ type: 'error', msg: t('providers.editor.name_required') });
      return;
    }
    setSaving(true);
    try {
      await saveProvider(normalize(p));
      onSaved();
    } catch (e) {
      setStatus({ type: 'error', msg: formatError(e) });
      setSaving(false);
    }
  }

  return (
    <Modal onClose={onClose} width="max-w-2xl">
      <ModalHeader
        title={initial.id ? t('providers.editor.title_edit') : t('providers.editor.title_new')}
        onClose={onClose}
      />
      <ModalBody className="space-y-4">
        {/* 预设按钮组 */}
        {presets.length > 0 && (
          <div>
            <div className="mb-1.5 text-[11.5px] font-medium text-stone-600">
              {t('providers.editor.preset_label')}
            </div>
            <div className="flex flex-wrap gap-1.5">
              {presets.map((preset, i) => (
                <button
                  key={i}
                  type="button"
                  onClick={() => applyPreset(preset)}
                  className="rounded border border-stone-200 bg-white px-2.5 py-1 text-[11.5px] text-stone-600 hover:border-stone-300 hover:text-stone-900"
                >
                  {preset.name}
                </button>
              ))}
            </div>
          </div>
        )}

        <FormField label={t('providers.editor.name')}>
          <Input
            value={p.name}
            onChange={(v) => set('name', v)}
            placeholder={t('providers.editor.name_placeholder')}
          />
        </FormField>

        <div className="grid grid-cols-2 gap-3">
          <FormField label={t('providers.editor.kind')}>
            <Select value={p.kind} onChange={(v) => set('kind', v)}>
              <option value="OpenAiCompatible">{t('providers.kind.openai')}</option>
              <option value="Anthropic">{t('providers.kind.anthropic')}</option>
              <option value="Gemini">{t('providers.kind.gemini')}</option>
            </Select>
          </FormField>
          <FormField label={t('providers.editor.model')}>
            <Mono
              value={p.model}
              onChange={(v) => set('model', v)}
              placeholder={t('providers.editor.model_placeholder')}
            />
          </FormField>
        </div>

        <FormField label={t('providers.editor.base_url')}>
          <Mono
            value={p.base_url}
            onChange={(v) => set('base_url', v)}
            placeholder="https://api.openai.com"
          />
        </FormField>

        <FormField label={t('providers.editor.api_key')} hint={t('providers.editor.api_key_hint')}>
          <Mono
            value={p.api_key}
            onChange={(v) => set('api_key', v)}
            placeholder="sk-..."
            type="password"
          />
        </FormField>

        <div className="grid grid-cols-2 gap-3">
          <FormField label="max_tokens">
            <Input
              value={String(p.max_tokens ?? 2048)}
              onChange={(v) => set('max_tokens', v)}
            />
          </FormField>
          <FormField label="temperature" hint={t('providers.editor.temperature_hint')}>
            <Input
              value={p.temperature === null || p.temperature === undefined ? '' : String(p.temperature)}
              onChange={(v) => set('temperature', v)}
              placeholder="0.7"
            />
          </FormField>
        </div>

        <label className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={!!p.is_default}
            onChange={(e) => set('is_default', e.target.checked)}
          />
          <span>{t('providers.editor.set_default')}</span>
        </label>

        <StatusBanner status={status} />
      </ModalBody>
      <ModalFooter>
        <SecondaryButton onClick={handleTest} disabled={testing}>
          <Icon name="refresh" size={14} />
          {testing ? t('providers.editor.test_running') : t('providers.editor.test_btn')}
        </SecondaryButton>
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

/// 把表单字段强制转成后端期望的类型。
function normalize(p) {
  const maxTokens = Number(p.max_tokens) || 2048;
  // 空字符串 / null / undefined → null（API 跳过 temperature）
  const tStr = typeof p.temperature === 'string' ? p.temperature.trim() : p.temperature;
  let temperature = null;
  if (tStr !== '' && tStr !== null && tStr !== undefined) {
    const n = Number(tStr);
    temperature = Number.isFinite(n) ? n : null;
  }
  return {
    ...p,
    max_tokens: maxTokens,
    temperature,
  };
}
