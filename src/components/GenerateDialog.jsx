// 生成对话框（详见 docs/UI.md#47-生成对话框-generatedialogjsx）。
//
// 4 个 step：config → generating → done | error。

import { useEffect, useState } from 'react';
import {
  generateReport,
  listProviders,
  listTemplates,
  listWorkspaces,
  formatError,
} from '../api.js';
import { useTranslation } from '../i18n/index.jsx';
import {
  FormField,
  Icon,
  LoadingState,
  Modal,
  ModalBody,
  ModalFooter,
  ModalHeader,
  PrimaryButton,
  SecondaryButton,
  Select,
} from './ui.jsx';

const DAY_OPTIONS = [3, 7, 14, 30];

export default function GenerateDialog({ onClose, onGenerated }) {
  const { t } = useTranslation();
  const [workspaces, setWorkspaces] = useState([]);
  const [templates, setTemplates] = useState([]);
  const [providers, setProviders] = useState([]);
  const [bootError, setBootError] = useState(null);

  // 表单状态
  const [wsIds, setWsIds] = useState([]);
  const [tplId, setTplId] = useState('');
  const [provId, setProvId] = useState('');
  const [days, setDays] = useState(7);

  // 流程状态
  const [step, setStep] = useState('config'); // config | generating | done | error
  const [result, setResult] = useState(null);
  const [errMsg, setErrMsg] = useState(null);
  const [copied, setCopied] = useState(false);

  // 初次加载
  useEffect(() => {
    Promise.all([listWorkspaces(), listTemplates(), listProviders()])
      .then(([wsList, tplList, provList]) => {
        setWorkspaces(wsList || []);
        setTemplates(tplList || []);
        setProviders(provList || []);
        // 默认全选 workspaces
        setWsIds((wsList || []).map((w) => w.id));
        // 默认第一个模板
        if ((tplList || []).length > 0) {
          setTplId(tplList[0].id);
        }
      })
      .catch((e) => setBootError(formatError(e)));
  }, []);

  function toggleWs(id) {
    setWsIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  }

  async function handleGenerate() {
    if (wsIds.length === 0) {
      setErrMsg(t('generate.errors.no_workspace'));
      setStep('error');
      return;
    }
    if (!tplId) {
      setErrMsg(t('generate.errors.no_template'));
      setStep('error');
      return;
    }
    setStep('generating');
    setErrMsg(null);
    try {
      const r = await generateReport({
        workspace_ids: wsIds,
        template_id: tplId,
        days,
        provider_id: provId || null,
      });
      setResult(r);
      setStep('done');
      onGenerated?.();
    } catch (e) {
      setErrMsg(formatError(e));
      setStep('error');
    }
  }

  async function handleCopy() {
    if (!result?.content) return;
    try {
      await navigator.clipboard.writeText(result.content);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      alert(`${t('generate.errors.copy_failed')}: ${formatError(e)}`);
    }
  }

  return (
    <Modal onClose={onClose} width="max-w-3xl">
      <ModalHeader title={t('generate.title')} onClose={onClose} />
      <ModalBody className="space-y-4">
        {bootError && <ErrorBox message={bootError} />}

        {step === 'config' && (
          <ConfigStep
            workspaces={workspaces}
            templates={templates}
            providers={providers}
            wsIds={wsIds}
            tplId={tplId}
            provId={provId}
            days={days}
            onToggleWs={toggleWs}
            onTplChange={setTplId}
            onProvChange={setProvId}
            onDaysChange={setDays}
          />
        )}

        {step === 'generating' && <GeneratingStep />}

        {step === 'error' && (
          <div className="space-y-3">
            <ErrorBox message={errMsg} />
            <p className="text-[12.5px] text-stone-500">{t('generate.errors.hint')}</p>
          </div>
        )}

        {step === 'done' && result && (
          <DoneStep
            result={result}
            providerName={
              providers.find((p) => p.id === result.record.provider_id)?.name ||
              result.record.provider_name ||
              '—'
            }
          />
        )}
      </ModalBody>
      <ModalFooter>
        <div className="text-[11.5px] text-stone-400">
          {step === 'done' &&
            result &&
            t('generate.footer.tokens', {
              n: result.record.tokens_used,
              sec: (result.duration_ms / 1000).toFixed(1),
            })}
        </div>
        <div className="flex gap-2">
          {step === 'config' && (
            <>
              <SecondaryButton onClick={onClose}>{t('generate.actions.cancel')}</SecondaryButton>
              <PrimaryButton onClick={handleGenerate}>
                <Icon name="sparkle" size={14} /> {t('generate.actions.start')}
              </PrimaryButton>
            </>
          )}
          {step === 'generating' && (
            <SecondaryButton onClick={onClose} title={t('generate.actions.background_tooltip')}>
              {t('generate.actions.background')}
            </SecondaryButton>
          )}
          {step === 'error' && (
            <>
              <SecondaryButton onClick={() => setStep('config')}>
                {t('generate.actions.back')}
              </SecondaryButton>
              <PrimaryButton onClick={onClose}>{t('generate.actions.close')}</PrimaryButton>
            </>
          )}
          {step === 'done' && (
            <>
              <SecondaryButton onClick={handleCopy}>
                <Icon name="copy" size={14} />{' '}
                {copied ? t('common.copied') : t('generate.actions.copy')}
              </SecondaryButton>
              <PrimaryButton onClick={onClose}>{t('generate.actions.done')}</PrimaryButton>
            </>
          )}
        </div>
      </ModalFooter>
    </Modal>
  );
}

function ConfigStep({
  workspaces,
  templates,
  providers,
  wsIds,
  tplId,
  provId,
  days,
  onToggleWs,
  onTplChange,
  onProvChange,
  onDaysChange,
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4">
      <FormField label={t('generate.config.workspace')}>
        {workspaces.length === 0 ? (
          <p className="text-[12.5px] text-stone-500">{t('generate.config.workspace_empty')}</p>
        ) : (
          <div className="space-y-1">
            {workspaces.map((w) => (
              <label key={w.id} className="flex items-center gap-2 text-[13px]">
                <input
                  type="checkbox"
                  checked={wsIds.includes(w.id)}
                  onChange={() => onToggleWs(w.id)}
                />
                <span>{w.name}</span>
                <span className="text-[11px] text-stone-400">
                  ({w.type === 'local' ? t('generate.config.local') : `${w.user}@${w.host}`})
                </span>
              </label>
            ))}
          </div>
        )}
      </FormField>

      <FormField label={t('generate.config.template')}>
        {templates.length === 0 ? (
          <p className="text-[12.5px] text-stone-500">{t('generate.config.template_empty')}</p>
        ) : (
          <div className="space-y-1">
            {templates.map((tpl) => (
              <label key={tpl.id} className="flex items-center gap-2 text-[13px]">
                <input
                  type="radio"
                  name="template"
                  checked={tplId === tpl.id}
                  onChange={() => onTplChange(tpl.id)}
                />
                <span>{tpl.name}</span>
                {tpl.builtin && (
                  <span className="rounded bg-stone-100 px-1 py-0.5 text-[10.5px] text-stone-500">
                    {t('generate.config.builtin')}
                  </span>
                )}
              </label>
            ))}
          </div>
        )}
      </FormField>

      <div className="grid grid-cols-2 gap-3">
        <FormField label={t('generate.config.llm')}>
          <Select value={provId} onChange={onProvChange}>
            <option value="">{t('generate.config.llm_default')}</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
                {p.is_default ? t('generate.config.default_suffix') : ''}
              </option>
            ))}
          </Select>
        </FormField>
        <FormField label={t('generate.config.days')}>
          <div className="inline-flex rounded-md border border-stone-200 p-0.5">
            {DAY_OPTIONS.map((n) => (
              <button
                key={n}
                type="button"
                onClick={() => onDaysChange(n)}
                className={`rounded px-3 py-1 text-[12.5px] ${
                  days === n
                    ? 'bg-stone-900 text-white'
                    : 'text-stone-600 hover:text-stone-900'
                }`}
              >
                {t('generate.config.days_unit', { n })}
              </button>
            ))}
          </div>
        </FormField>
      </div>
    </div>
  );
}

function GeneratingStep() {
  const { t } = useTranslation();
  return (
    <div className="py-12 text-center">
      <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-xl bg-stone-100 text-stone-500">
        <Icon name="sparkle" size={22} />
      </div>
      <p className="text-[14px] font-medium text-stone-900">{t('generate.generating.title')}</p>
      <p className="mt-1 text-[12px] text-stone-500">{t('generate.generating.subtitle')}</p>
      <p className="mt-3 text-[11.5px] text-stone-400">{t('generate.generating.hint')}</p>
    </div>
  );
}

function DoneStep({ result, providerName }) {
  const { t } = useTranslation();
  const skipped = (result.skipped_lines || 0) + (result.skipped_files || 0);
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2 rounded border border-emerald-200 bg-emerald-50 px-3 py-2 text-[12.5px] text-emerald-700">
        <Icon name="check" size={14} />
        <span>{t('generate.done.success', { provider: providerName })}</span>
      </div>
      {skipped > 0 && (
        <div className="rounded border border-amber-200 bg-amber-50 px-3 py-2 text-[12px] text-amber-700">
          {t('generate.done.skipped', {
            lines: result.skipped_lines,
            files: result.skipped_files,
          })}
        </div>
      )}
      <pre className="max-h-80 overflow-auto rounded-lg bg-stone-50 p-4 font-mono text-[12px] text-stone-800 whitespace-pre-wrap">
        {result.content}
      </pre>
    </div>
  );
}

function ErrorBox({ message }) {
  const { t } = useTranslation();
  return (
    <div className="rounded border border-rose-200 bg-rose-50 px-3 py-2 text-[12.5px] text-rose-700 whitespace-pre-wrap">
      {message || t('generate.errors.unknown')}
    </div>
  );
}
