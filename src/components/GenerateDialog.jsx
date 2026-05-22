// 生成对话框（详见 docs/UI.md#47-生成对话框-generatedialogjsx）。
//
// 5 个 step：config → collecting → review → generating → done | error
// review：按项目折叠展示，用户可勾选/编辑/删除/手动新增条目；
//         编辑过程实时 debounce 自动保存到后端 draft_summary.json
// 启动时自动检测 draft，提示用户继续/丢弃

import { useEffect, useMemo, useRef, useState } from 'react';
import {
  clearDraftSummary,
  collectLogs,
  formatError,
  listProviders,
  listTemplates,
  listWorkspaces,
  loadDraftSummary,
  renderReport,
  saveDraftSummary,
} from '../api.js';
import { useTranslation } from '../i18n/index.jsx';
import {
  FormField,
  Icon,
  IconButton,
  Modal,
  ModalBody,
  ModalFooter,
  ModalHeader,
  PrimaryButton,
  SecondaryButton,
  Select,
  Textarea,
} from './ui.jsx';
import { formatIsoMinute } from '../utils.js';

const DAY_OPTIONS = [3, 7, 14, 30];
const AUTOSAVE_DEBOUNCE_MS = 800;

export default function GenerateDialog({ onClose, onGenerated }) {
  const { t } = useTranslation();
  const [workspaces, setWorkspaces] = useState([]);
  const [templates, setTemplates] = useState([]);
  const [providers, setProviders] = useState([]);
  const [bootError, setBootError] = useState(null);

  // config 状态
  const [wsIds, setWsIds] = useState([]);
  const [tplId, setTplId] = useState('');
  const [provId, setProvId] = useState('');
  const [days, setDays] = useState(7);

  // 流程状态：config → collecting → review → generating → done | error
  const [step, setStep] = useState('config');
  const [collection, setCollection] = useState(null);
  const [selectedIds, setSelectedIds] = useState(() => new Set());
  const [draftSavedAt, setDraftSavedAt] = useState(null);
  const [draftSaving, setDraftSaving] = useState(false);
  const [draftDetected, setDraftDetected] = useState(null);

  const [result, setResult] = useState(null);
  const [errMsg, setErrMsg] = useState(null);
  const [copied, setCopied] = useState(false);

  // 初次加载 + 检测 draft
  useEffect(() => {
    Promise.all([listWorkspaces(), listTemplates(), listProviders()])
      .then(([wsList, tplList, provList]) => {
        setWorkspaces(wsList || []);
        setTemplates(tplList || []);
        setProviders(provList || []);
        setWsIds((wsList || []).map((w) => w.id));
        if ((tplList || []).length > 0) {
          setTplId(tplList[0].id);
        }
      })
      .catch((e) => setBootError(formatError(e)));

    loadDraftSummary()
      .then((draft) => {
        if (draft) setDraftDetected(draft);
      })
      .catch(() => {
        /* 草稿加载失败不阻塞 */
      });
  }, []);

  function toggleWs(id) {
    setWsIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  }

  // === 第一步：collect ===
  async function handleCollect() {
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
    setStep('collecting');
    setErrMsg(null);
    try {
      const out = await collectLogs({ workspace_ids: wsIds, days });
      setCollection(out);
      setSelectedIds(allItemIds(out));
      setStep('review');
    } catch (e) {
      setErrMsg(formatError(e));
      setStep('error');
    }
  }

  // === 草稿自动保存（debounce）===
  // collection 变化（编辑文字 / 删除 / 添加）就 800ms 后保存一次完整 collection。
  // 勾选状态不持久化（重启后默认全选未删除项）—— 这是"勾选 = 临时筛选"的语义。
  const draftDebounceRef = useRef(null);
  useEffect(() => {
    if (step !== 'review' || !collection) return undefined;
    if (draftDebounceRef.current) clearTimeout(draftDebounceRef.current);
    setDraftSaving(true);
    draftDebounceRef.current = setTimeout(async () => {
      try {
        await saveDraftSummary(collection);
        setDraftSavedAt(new Date());
      } catch {
        /* 草稿保存失败不阻塞 UI */
      } finally {
        setDraftSaving(false);
      }
    }, AUTOSAVE_DEBOUNCE_MS);
    return () => {
      if (draftDebounceRef.current) clearTimeout(draftDebounceRef.current);
    };
  }, [step, collection]);

  // === 草稿提示：继续 / 丢弃 ===
  function handleContinueDraft() {
    if (!draftDetected) return;
    setCollection(draftDetected);
    setSelectedIds(allItemIds(draftDetected));
    setWsIds(draftDetected.workspace_ids || []);
    setDays(draftDetected.days || 7);
    setDraftDetected(null);
    setStep('review');
  }

  async function handleDiscardDraft() {
    setDraftDetected(null);
    try {
      await clearDraftSummary();
    } catch {
      /* 静默 */
    }
  }

  // === Review 操作 ===
  function toggleSelected(id) {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleProjectAll(projectName) {
    const items = collection.summary.by_project[projectName] || [];
    const allSelected = items.length > 0 && items.every((it) => selectedIds.has(it.id));
    setSelectedIds((prev) => {
      const next = new Set(prev);
      items.forEach((it) => {
        if (allSelected) next.delete(it.id);
        else next.add(it.id);
      });
      return next;
    });
  }

  function updateItemText(projectName, itemId, newText) {
    setCollection((prev) => {
      const items = prev.summary.by_project[projectName].map((it) =>
        it.id === itemId ? { ...it, text: newText } : it,
      );
      return {
        ...prev,
        summary: {
          ...prev.summary,
          by_project: { ...prev.summary.by_project, [projectName]: items },
        },
      };
    });
  }

  function deleteItem(projectName, itemId) {
    setCollection((prev) => {
      const items = prev.summary.by_project[projectName].filter((it) => it.id !== itemId);
      const next = { ...prev.summary.by_project };
      if (items.length === 0) {
        delete next[projectName];
      } else {
        next[projectName] = items;
      }
      return { ...prev, summary: { ...prev.summary, by_project: next } };
    });
    setSelectedIds((prev) => {
      const n = new Set(prev);
      n.delete(itemId);
      return n;
    });
  }

  function addItem(projectName, text) {
    const trimmed = text.trim();
    if (!trimmed) return;
    const newItem = {
      id: `manual-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      timestamp: new Date().toISOString(),
      text: trimmed,
      source: 'manual',
      server: '',
      manual: true,
    };
    setCollection((prev) => {
      const items = [...(prev.summary.by_project[projectName] || []), newItem];
      return {
        ...prev,
        summary: {
          ...prev.summary,
          by_project: { ...prev.summary.by_project, [projectName]: items },
        },
      };
    });
    setSelectedIds((prev) => new Set([...prev, newItem.id]));
  }

  // === 项目背景文档编辑 ===
  function updateProjectDoc(projectName, text) {
    setCollection((prev) => ({
      ...prev,
      summary: {
        ...prev.summary,
        project_docs: { ...(prev.summary.project_docs || {}), [projectName]: text },
      },
    }));
  }

  function deleteProjectDoc(projectName) {
    setCollection((prev) => {
      const next = { ...(prev.summary.project_docs || {}) };
      delete next[projectName];
      return { ...prev, summary: { ...prev.summary, project_docs: next } };
    });
  }

  // === 第二步：render ===
  async function handleRender() {
    if (selectedIds.size === 0) {
      setErrMsg(t('generate.review.errors.no_selected'));
      setStep('error');
      return;
    }
    // 过滤未勾选条目；project 全空则删
    const filtered = {};
    Object.entries(collection.summary.by_project).forEach(([proj, items]) => {
      const kept = items.filter((it) => selectedIds.has(it.id));
      if (kept.length > 0) filtered[proj] = kept;
    });
    // project_docs 只保留仍有勾选条目的项目，避免「有背景无日志」的项目进 prompt
    const keptProjects = new Set(Object.keys(filtered));
    const filteredDocs = {};
    Object.entries(collection.summary.project_docs || {}).forEach(([proj, doc]) => {
      if (keptProjects.has(proj)) filteredDocs[proj] = doc;
    });
    const finalSummary = {
      ...collection.summary,
      by_project: filtered,
      project_docs: filteredDocs,
      // stats 后端 recompute_stats 会重算
    };

    setStep('generating');
    setErrMsg(null);
    try {
      const r = await renderReport({
        summary: finalSummary,
        template_id: tplId,
        days: collection.days,
        provider_id: provId || null,
      });
      setResult(r);
      try {
        await clearDraftSummary();
      } catch {
        /* 静默 */
      }
      setStep('done');
      onGenerated?.();
    } catch (e) {
      setErrMsg(formatError(e));
      setStep('error');
    }
  }

  async function handleRefetch() {
    setCollection(null);
    setSelectedIds(new Set());
    try {
      await clearDraftSummary();
    } catch {
      /* 静默 */
    }
    setStep('config');
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
    <Modal onClose={onClose} width={step === 'review' ? 'max-w-5xl' : 'max-w-3xl'}>
      <ModalHeader title={t('generate.title')} onClose={onClose} />
      <ModalBody className="space-y-4">
        {bootError && <ErrorBox message={bootError} />}
        {draftDetected && step === 'config' && (
          <DraftBanner
            draft={draftDetected}
            onContinue={handleContinueDraft}
            onDiscard={handleDiscardDraft}
          />
        )}

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

        {step === 'collecting' && <CollectingStep />}

        {step === 'review' && collection && (
          <ReviewStep
            collection={collection}
            selectedIds={selectedIds}
            draftSaving={draftSaving}
            draftSavedAt={draftSavedAt}
            onToggleItem={toggleSelected}
            onToggleProject={toggleProjectAll}
            onUpdateText={updateItemText}
            onDeleteItem={deleteItem}
            onAddItem={addItem}
            onUpdateDoc={updateProjectDoc}
            onDeleteDoc={deleteProjectDoc}
            onRefetch={handleRefetch}
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
              <PrimaryButton onClick={handleCollect}>
                <Icon name="sparkle" size={14} /> {t('generate.actions.start')}
              </PrimaryButton>
            </>
          )}
          {step === 'collecting' && (
            <SecondaryButton onClick={onClose}>{t('generate.actions.cancel')}</SecondaryButton>
          )}
          {step === 'review' && (
            <>
              <SecondaryButton onClick={() => setStep('config')}>
                {t('generate.actions.back')}
              </SecondaryButton>
              <PrimaryButton onClick={handleRender}>
                <Icon name="sparkle" size={14} /> {t('generate.actions.continue_render')}
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
              <SecondaryButton onClick={() => setStep(collection ? 'review' : 'config')}>
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

/** 收集所有 item id 形成 Set，用于"默认全选"。 */
function allItemIds(collectionOutput) {
  const s = new Set();
  Object.values(collectionOutput.summary.by_project || {}).forEach((items) => {
    items.forEach((it) => s.add(it.id));
  });
  return s;
}

// ============================================================
// ConfigStep
// ============================================================

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

// ============================================================
// CollectingStep（新）
// ============================================================

function CollectingStep() {
  const { t } = useTranslation();
  return (
    <div className="py-12 text-center">
      <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-xl bg-stone-100 text-stone-500">
        <Icon name="sparkle" size={22} />
      </div>
      <p className="text-[14px] font-medium text-stone-900">{t('generate.collecting.title')}</p>
      <p className="mt-1 text-[12px] text-stone-500">{t('generate.collecting.subtitle')}</p>
    </div>
  );
}

// ============================================================
// ReviewStep（新，最复杂）
// ============================================================

function ReviewStep({
  collection,
  selectedIds,
  draftSaving,
  draftSavedAt,
  onToggleItem,
  onToggleProject,
  onUpdateText,
  onDeleteItem,
  onAddItem,
  onUpdateDoc,
  onDeleteDoc,
  onRefetch,
}) {
  const { t } = useTranslation();

  const totalCount = useMemo(() => {
    return Object.values(collection.summary.by_project).reduce(
      (s, items) => s + items.length,
      0,
    );
  }, [collection]);

  const selectedCount = useMemo(() => {
    let n = 0;
    Object.values(collection.summary.by_project).forEach((items) => {
      items.forEach((it) => {
        if (selectedIds.has(it.id)) n += 1;
      });
    });
    return n;
  }, [collection, selectedIds]);

  // 按条目数从多到少排
  const projects = useMemo(() => {
    return Object.entries(collection.summary.by_project).sort(
      ([, a], [, b]) => b.length - a.length,
    );
  }, [collection]);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h3 className="text-[14px] font-medium text-stone-900">{t('generate.review.title')}</h3>
          <p className="text-[12px] text-stone-500">{t('generate.review.subtitle')}</p>
        </div>
        <div className="flex items-center gap-2 text-[11.5px] text-stone-500">
          {draftSaving && <span>{t('generate.review.saving')}</span>}
          {!draftSaving && draftSavedAt && (
            <span>
              {t('generate.review.saved_at', {
                time: formatIsoMinute(draftSavedAt.toISOString()),
              })}
            </span>
          )}
          <SecondaryButton onClick={onRefetch}>
            <Icon name="refresh" size={12} /> {t('generate.review.refetch')}
          </SecondaryButton>
        </div>
      </div>

      <div className="rounded-md bg-stone-50 px-3 py-1.5 text-[12px] text-stone-600">
        {t('generate.review.summary', { total: totalCount, selected: selectedCount })}
      </div>

      {projects.length === 0 ? (
        <p className="py-8 text-center text-[12.5px] text-stone-500">
          {t('generate.review.empty')}
        </p>
      ) : (
        <div className="space-y-2">
          {projects.map(([name, items]) => (
            <ProjectSection
              key={name}
              name={name}
              items={items}
              doc={collection.summary.project_docs?.[name]}
              selectedIds={selectedIds}
              onToggleItem={onToggleItem}
              onToggleAll={() => onToggleProject(name)}
              onUpdateText={(id, text) => onUpdateText(name, id, text)}
              onDeleteItem={(id) => onDeleteItem(name, id)}
              onAddItem={(text) => onAddItem(name, text)}
              onUpdateDoc={(text) => onUpdateDoc(name, text)}
              onDeleteDoc={() => onDeleteDoc(name)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ProjectSection({
  name,
  items,
  doc,
  selectedIds,
  onToggleItem,
  onToggleAll,
  onUpdateText,
  onDeleteItem,
  onAddItem,
  onUpdateDoc,
  onDeleteDoc,
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(true);
  const [adding, setAdding] = useState(false);
  const [newText, setNewText] = useState('');

  const sel = items.filter((it) => selectedIds.has(it.id)).length;
  const allSelected = items.length > 0 && sel === items.length;
  const indeterminate = sel > 0 && sel < items.length;

  return (
    <div className="rounded-lg border border-stone-200 bg-white">
      <div className="flex items-center gap-2 border-b border-stone-100 px-3 py-2">
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          className="text-stone-500 hover:text-stone-900"
          aria-label={open ? 'collapse' : 'expand'}
        >
          <Icon name={open ? 'chevronD' : 'chevronR'} size={14} />
        </button>
        <input
          type="checkbox"
          checked={allSelected}
          ref={(el) => {
            if (el) el.indeterminate = indeterminate;
          }}
          onChange={onToggleAll}
        />
        <span className="flex-1 text-[13px] font-medium text-stone-900">{name}</span>
        <span className="text-[11.5px] text-stone-500">
          {t('generate.review.project_summary', { selected: sel, total: items.length })}
        </span>
      </div>

      {open && (
        <div className="divide-y divide-stone-100">
          <ProjectDocPanel doc={doc} onUpdate={onUpdateDoc} onDelete={onDeleteDoc} />
          {items.map((it) => (
            <ItemRow
              key={it.id}
              item={it}
              selected={selectedIds.has(it.id)}
              onToggle={() => onToggleItem(it.id)}
              onUpdate={(text) => onUpdateText(it.id, text)}
              onDelete={() => onDeleteItem(it.id)}
            />
          ))}
          {adding ? (
            <div className="space-y-2 px-3 py-2">
              <Textarea
                value={newText}
                onChange={setNewText}
                rows={2}
                placeholder={t('generate.review.add_placeholder')}
              />
              <div className="flex gap-1.5">
                <PrimaryButton
                  onClick={() => {
                    onAddItem(newText);
                    setNewText('');
                    setAdding(false);
                  }}
                  disabled={!newText.trim()}
                >
                  {t('common.add')}
                </PrimaryButton>
                <SecondaryButton
                  onClick={() => {
                    setAdding(false);
                    setNewText('');
                  }}
                >
                  {t('common.cancel')}
                </SecondaryButton>
              </div>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setAdding(true)}
              className="flex w-full items-center gap-1.5 px-3 py-2 text-left text-[12px] text-stone-500 hover:bg-stone-50 hover:text-stone-900"
            >
              <Icon name="plus" size={12} />
              {t('generate.review.add_item')}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

// 项目背景文档面板：展示 / 编辑 / 清空项目根目录的 md 文档（README 等）。
// doc 为 undefined 表示该项目没有背景文档，显示「添加」入口。
function ProjectDocPanel({ doc, onUpdate, onDelete }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  if (typeof doc !== 'string') {
    return (
      <button
        type="button"
        onClick={() => onUpdate('')}
        className="flex w-full items-center gap-1.5 px-3 py-2 text-left text-[12px] text-stone-500 hover:bg-stone-50 hover:text-stone-900"
      >
        <Icon name="plus" size={12} />
        {t('generate.review.project_docs.add')}
      </button>
    );
  }
  return (
    <div className="bg-stone-50/60">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-1.5 px-3 py-2 text-left text-[12px] text-stone-600 hover:text-stone-900"
      >
        <Icon name={open ? 'chevronD' : 'chevronR'} size={12} />
        <span className="font-medium">{t('generate.review.project_docs.label')}</span>
        <span className="text-[11px] text-stone-400">
          {t('generate.review.project_docs.chars', { n: doc.length })}
        </span>
      </button>
      {open && (
        <div className="space-y-2 px-3 pb-3">
          <p className="text-[11px] text-stone-400">
            {t('generate.review.project_docs.hint')}
          </p>
          <Textarea
            value={doc}
            onChange={onUpdate}
            rows={6}
            placeholder={t('generate.review.project_docs.placeholder')}
          />
          <SecondaryButton onClick={onDelete}>
            <Icon name="trash" size={12} /> {t('generate.review.project_docs.clear')}
          </SecondaryButton>
        </div>
      )}
    </div>
  );
}

function ItemRow({ item, selected, onToggle, onUpdate, onDelete }) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [draftText, setDraftText] = useState(item.text);

  function saveEdit() {
    onUpdate(draftText);
    setEditing(false);
  }

  function cancelEdit() {
    setDraftText(item.text);
    setEditing(false);
  }

  function handleDelete() {
    if (confirm(t('generate.review.confirm_delete_item'))) {
      onDelete();
    }
  }

  const sourceLabel = t(`generate.review.source.${item.source}`);
  const timeLabel = item.timestamp ? formatIsoMinute(item.timestamp) : '';
  const sourceClass =
    item.source === 'claude-code'
      ? 'bg-orange-50 text-orange-700'
      : item.source === 'codex'
        ? 'bg-emerald-50 text-emerald-700'
        : 'bg-stone-100 text-stone-600';

  return (
    <div className="flex items-start gap-2 px-3 py-2">
      <input type="checkbox" checked={selected} onChange={onToggle} className="mt-1" />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap gap-x-2 gap-y-0.5 text-[11px] text-stone-400">
          {timeLabel && <span>{timeLabel}</span>}
          <span className={`rounded px-1 ${sourceClass}`}>{sourceLabel}</span>
          {item.server && (
            <span className="rounded bg-stone-100 px-1 text-stone-600">{item.server}</span>
          )}
        </div>
        {editing ? (
          <div className="mt-1 space-y-1.5">
            <Textarea value={draftText} onChange={setDraftText} rows={3} />
            <div className="flex gap-1.5">
              <PrimaryButton onClick={saveEdit}>{t('generate.actions.save_edit')}</PrimaryButton>
              <SecondaryButton onClick={cancelEdit}>
                {t('generate.actions.cancel_edit')}
              </SecondaryButton>
            </div>
          </div>
        ) : (
          <p className="mt-0.5 whitespace-pre-wrap break-words text-[12.5px] text-stone-700">
            {item.text}
          </p>
        )}
      </div>
      {!editing && (
        <div className="flex gap-0.5">
          <IconButton title={t('generate.actions.edit')} onClick={() => setEditing(true)}>
            <Icon name="edit" size={13} />
          </IconButton>
          <IconButton title={t('generate.actions.delete')} onClick={handleDelete}>
            <Icon name="trash" size={13} />
          </IconButton>
        </div>
      )}
    </div>
  );
}

// ============================================================
// DraftBanner（新）
// ============================================================

function DraftBanner({ draft, onContinue, onDiscard }) {
  const { t } = useTranslation();
  // 后端 CollectionOutput 没存 saved_at，显示条目数代替时间提示
  const itemCount = Object.values(draft.summary.by_project || {}).reduce(
    (s, items) => s + items.length,
    0,
  );
  const desc = t('generate.draft.detected_desc', { time: `${itemCount} items` });
  return (
    <div className="rounded border border-amber-200 bg-amber-50 px-3 py-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="text-[12.5px] text-amber-900">
          <div className="font-medium">{t('generate.draft.detected_title')}</div>
          <div className="text-[11.5px] text-amber-700">{desc}</div>
        </div>
        <div className="flex gap-1.5">
          <SecondaryButton onClick={onDiscard}>{t('generate.draft.discard')}</SecondaryButton>
          <PrimaryButton onClick={onContinue}>{t('generate.draft.continue')}</PrimaryButton>
        </div>
      </div>
    </div>
  );
}

// ============================================================
// GeneratingStep / DoneStep / ErrorBox
// ============================================================

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
      <pre className="max-h-80 overflow-auto whitespace-pre-wrap rounded-lg bg-stone-50 p-4 font-mono text-[12px] text-stone-800">
        {result.content}
      </pre>
    </div>
  );
}

function ErrorBox({ message }) {
  const { t } = useTranslation();
  return (
    <div className="whitespace-pre-wrap rounded border border-rose-200 bg-rose-50 px-3 py-2 text-[12.5px] text-rose-700">
      {message || t('generate.errors.unknown')}
    </div>
  );
}
