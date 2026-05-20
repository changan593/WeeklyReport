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
      setErrMsg('请至少选择一个工作区');
      setStep('error');
      return;
    }
    if (!tplId) {
      setErrMsg('请选择模板');
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
      alert('复制失败：' + formatError(e));
    }
  }

  return (
    <Modal onClose={onClose} width="max-w-3xl">
      <ModalHeader title="生成周报" onClose={onClose} />
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
            <p className="text-[12.5px] text-stone-500">
              常见原因：LLM 源未配置 / API key 错误 / 网络不通 / 选中的工作区无日志。
            </p>
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
            `tokens ${result.record.tokens_used} · 耗时 ${(result.duration_ms / 1000).toFixed(1)}s`}
        </div>
        <div className="flex gap-2">
          {step === 'config' && (
            <>
              <SecondaryButton onClick={onClose}>取消</SecondaryButton>
              <PrimaryButton onClick={handleGenerate}>
                <Icon name="sparkle" size={14} /> 开始生成
              </PrimaryButton>
            </>
          )}
          {step === 'generating' && (
            <SecondaryButton onClick={onClose} title="后端会继续生成，完成后报告仍会存档">
              后台继续，关闭窗口
            </SecondaryButton>
          )}
          {step === 'error' && (
            <>
              <SecondaryButton onClick={() => setStep('config')}>
                返回
              </SecondaryButton>
              <PrimaryButton onClick={onClose}>关闭</PrimaryButton>
            </>
          )}
          {step === 'done' && (
            <>
              <SecondaryButton onClick={handleCopy}>
                <Icon name="copy" size={14} /> {copied ? '已复制' : '复制'}
              </SecondaryButton>
              <PrimaryButton onClick={onClose}>完成</PrimaryButton>
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
  return (
    <div className="space-y-4">
      <FormField label="工作区">
        {workspaces.length === 0 ? (
          <p className="text-[12.5px] text-stone-500">
            还没有配置工作区，请先到「工作区」页添加。
          </p>
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
                  ({w.type === 'local' ? '本地' : `${w.user}@${w.host}`})
                </span>
              </label>
            ))}
          </div>
        )}
      </FormField>

      <FormField label="模板">
        {templates.length === 0 ? (
          <p className="text-[12.5px] text-stone-500">
            没有可用模板（应至少有 3 个内置模板，请检查后端）。
          </p>
        ) : (
          <div className="space-y-1">
            {templates.map((t) => (
              <label key={t.id} className="flex items-center gap-2 text-[13px]">
                <input
                  type="radio"
                  name="template"
                  checked={tplId === t.id}
                  onChange={() => onTplChange(t.id)}
                />
                <span>{t.name}</span>
                {t.builtin && (
                  <span className="rounded bg-stone-100 px-1 py-0.5 text-[10.5px] text-stone-500">
                    内置
                  </span>
                )}
              </label>
            ))}
          </div>
        )}
      </FormField>

      <div className="grid grid-cols-2 gap-3">
        <FormField label="LLM 源">
          <Select value={provId} onChange={onProvChange}>
            <option value="">使用默认 / 模板指定</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
                {p.is_default ? '（默认）' : ''}
              </option>
            ))}
          </Select>
        </FormField>
        <FormField label="时间范围">
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
                {n} 天
              </button>
            ))}
          </div>
        </FormField>
      </div>
    </div>
  );
}

function GeneratingStep() {
  return (
    <div className="py-12 text-center">
      <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-xl bg-stone-100 text-stone-500">
        <Icon name="sparkle" size={22} />
      </div>
      <p className="text-[14px] font-medium text-stone-900">正在生成周报…</p>
      <p className="mt-1 text-[12px] text-stone-500">
        扫描日志 → 压缩聚合 → 调用 LLM，最长 120 秒
      </p>
      <p className="mt-3 text-[11.5px] text-stone-400">
        可点底部按钮关闭窗口，生成会在后台继续，完成后报告自动存档到「历史周报」
      </p>
    </div>
  );
}

function DoneStep({ result, providerName }) {
  const skipped = (result.skipped_lines || 0) + (result.skipped_files || 0);
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2 rounded border border-emerald-200 bg-emerald-50 px-3 py-2 text-[12.5px] text-emerald-700">
        <Icon name="check" size={14} />
        <span>生成成功（{providerName}）</span>
      </div>
      {skipped > 0 && (
        <div className="rounded border border-amber-200 bg-amber-50 px-3 py-2 text-[12px] text-amber-700">
          ⚠ 解析时跳过 {result.skipped_lines} 行 / {result.skipped_files} 个文件（JSON 损坏或不可读）。
          报告内容可能不完整，可用 <code className="font-mono">RUST_LOG=debug</code> 查看明细。
        </div>
      )}
      <pre className="max-h-80 overflow-auto rounded-lg bg-stone-50 p-4 font-mono text-[12px] text-stone-800 whitespace-pre-wrap">
        {result.content}
      </pre>
    </div>
  );
}

function ErrorBox({ message }) {
  return (
    <div className="rounded border border-rose-200 bg-rose-50 px-3 py-2 text-[12.5px] text-rose-700 whitespace-pre-wrap">
      {message || '未知错误'}
    </div>
  );
}
