// 历史周报页（详见 docs/UI.md#44-历史周报页-reportsjsx）。
//
// 列出所有已存档报告（表格视图）；点击行进入详情；详情可在 Markdown / HTML 间切换，
// 复制按钮按当前 tab 复制对应内容；可删除。

import { useEffect, useState } from 'react';
import {
  deleteReport,
  getReport,
  getReportHtml,
  listReports,
  useAsyncState,
  formatError,
} from '../api.js';
import { useTranslation } from '../i18n/index.jsx';
import {
  EmptyState,
  Icon,
  IconButton,
  LoadingState,
  Modal,
  ModalBody,
  ModalFooter,
  ModalHeader,
  PrimaryButton,
  SecondaryButton,
} from './ui.jsx';
import { formatIsoMinute as formatTs } from '../utils.js';

export default function Reports() {
  const { t } = useTranslation();
  const [items, loading, reload] = useAsyncState(listReports, []);
  const [openId, setOpenId] = useState(null);

  return (
    <div>
      <header className="mb-6">
        <h1 className="text-2xl font-medium text-stone-900">{t('reports.title')}</h1>
        <p className="mt-1 text-[13px] text-stone-500">{t('reports.subtitle')}</p>
      </header>

      {loading && <LoadingState />}
      {!loading && items && items.length === 0 && (
        <EmptyState iconName="report" message={t('reports.empty')} />
      )}
      {!loading && items && items.length > 0 && (
        <ReportTable items={sortByDateDesc(items)} onOpen={setOpenId} />
      )}

      {openId && (
        <ReportDetail
          id={openId}
          onClose={() => setOpenId(null)}
          onDeleted={() => {
            setOpenId(null);
            reload();
          }}
        />
      )}
    </div>
  );
}

function sortByDateDesc(items) {
  return [...items].sort((a, b) =>
    (b.generated_at || '').localeCompare(a.generated_at || ''),
  );
}

function ReportTable({ items, onOpen }) {
  const { t } = useTranslation();
  return (
    <div className="overflow-hidden rounded-lg border border-stone-200 bg-white">
      <table className="w-full text-[12.5px]">
        <thead className="border-b border-stone-200 bg-stone-50 text-stone-500">
          <tr>
            <th className="px-4 py-2 text-left font-medium">{t('reports.columns.range')}</th>
            <th className="px-4 py-2 text-left font-medium">{t('reports.columns.template')}</th>
            <th className="px-4 py-2 text-left font-medium">{t('reports.columns.provider')}</th>
            <th className="px-4 py-2 text-right font-medium">{t('reports.columns.projects')}</th>
            <th className="px-4 py-2 text-right font-medium">{t('reports.columns.tokens')}</th>
            <th className="px-4 py-2 text-left font-medium">{t('reports.columns.generated_at')}</th>
            <th className="px-4 py-2 w-8" />
          </tr>
        </thead>
        <tbody>
          {items.map((r) => (
            <tr
              key={r.id}
              onClick={() => onOpen(r.id)}
              className="cursor-pointer border-t border-stone-100 hover:bg-stone-50"
            >
              <td className="px-4 py-2 text-stone-900">{r.week}</td>
              <td className="px-4 py-2 text-stone-700">{r.template_name}</td>
              <td className="px-4 py-2 text-stone-500">{r.provider_name || '—'}</td>
              <td className="px-4 py-2 text-right text-stone-700">{r.project_count}</td>
              <td className="px-4 py-2 text-right text-stone-500">{r.tokens_used}</td>
              <td className="px-4 py-2 text-stone-500">{formatTs(r.generated_at)}</td>
              <td className="px-4 py-2 text-stone-400">
                <Icon name="chevronR" size={14} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// formatTs 复用 src/utils.js 的 formatIsoMinute（导入在文件顶部）

function ReportDetail({ id, onClose, onDeleted }) {
  const { t } = useTranslation();
  const [data, loading, , error] = useAsyncState(() => getReport(id), [id]);
  const [tab, setTab] = useState('md'); // 'md' | 'html'
  const [html, setHtml] = useState(null);
  const [htmlLoading, setHtmlLoading] = useState(false);
  const [htmlError, setHtmlError] = useState(null);
  const [copied, setCopied] = useState(false);

  // 第一次切到 HTML tab 时才 lazy load（避免不必要的渲染调用）
  useEffect(() => {
    if (tab !== 'html' || html != null || htmlLoading) return;
    setHtmlLoading(true);
    setHtmlError(null);
    getReportHtml(id)
      .then(setHtml)
      .catch((e) => setHtmlError(formatError(e)))
      .finally(() => setHtmlLoading(false));
  }, [tab, html, htmlLoading, id]);

  async function handleCopy() {
    try {
      const text = tab === 'html' ? html || '' : data?.content || '';
      if (!text) return;
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      alert(`${t('reports.copy_failed')}: ${formatError(e)}`);
    }
  }

  async function handleDelete() {
    if (!data?.record) return;
    const name = `${data.record.week} - ${data.record.template_name}`;
    if (!confirm(t('reports.confirm_delete', { name }))) {
      return;
    }
    try {
      await deleteReport(id);
      onDeleted();
    } catch (e) {
      alert(formatError(e));
    }
  }

  const copyLabel = copied
    ? t('common.copied')
    : tab === 'html'
      ? t('reports.actions.copy_html')
      : t('reports.actions.copy_markdown');

  return (
    <Modal onClose={onClose} width="max-w-3xl">
      <ModalHeader title={t('reports.detail.title')} onClose={onClose} />
      <ModalBody className="space-y-3">
        {loading && <LoadingState />}
        {error && (
          <div className="rounded border border-rose-200 bg-rose-50 px-3 py-2 text-[12.5px] text-rose-700">
            {error}
          </div>
        )}
        {data?.record && (
          <>
            <div className="flex flex-wrap gap-x-4 gap-y-1 text-[12px] text-stone-500">
              <span>{data.record.week}</span>
              <span>{t('reports.detail.template', { name: data.record.template_name })}</span>
              {data.record.provider_name && (
                <span>{t('reports.detail.provider', { name: data.record.provider_name })}</span>
              )}
              <span>{t('reports.detail.projects', { count: data.record.project_count })}</span>
              <span>{t('reports.detail.tokens', { count: data.record.tokens_used })}</span>
              <span>{formatTs(data.record.generated_at)}</span>
            </div>

            <TabBar tab={tab} onChange={setTab} />

            {tab === 'md' && (
              <pre className="max-h-[60vh] overflow-auto whitespace-pre-wrap rounded-lg bg-stone-50 p-5 font-mono text-[12px] text-stone-800">
                {data.content}
              </pre>
            )}
            {tab === 'html' && (
              <HtmlPreview html={html} loading={htmlLoading} error={htmlError} />
            )}
          </>
        )}
      </ModalBody>
      <ModalFooter>
        <SecondaryButton onClick={handleDelete}>
          <Icon name="trash" size={14} /> {t('common.delete')}
        </SecondaryButton>
        <div className="flex gap-2">
          <SecondaryButton onClick={handleCopy} disabled={tab === 'html' ? !html : !data?.content}>
            <Icon name="copy" size={14} />
            {copyLabel}
          </SecondaryButton>
          <PrimaryButton onClick={onClose}>{t('common.close')}</PrimaryButton>
        </div>
      </ModalFooter>
    </Modal>
  );
}

function TabBar({ tab, onChange }) {
  const { t } = useTranslation();
  const tabs = [
    { key: 'md', label: t('reports.tab.markdown') },
    { key: 'html', label: t('reports.tab.html') },
  ];
  return (
    <div className="inline-flex rounded-md border border-stone-200 p-0.5">
      {tabs.map((x) => (
        <button
          key={x.key}
          type="button"
          onClick={() => onChange(x.key)}
          className={`rounded px-3 py-1 text-[12.5px] ${
            tab === x.key
              ? 'bg-stone-900 text-white'
              : 'text-stone-600 hover:text-stone-900'
          }`}
        >
          {x.label}
        </button>
      ))}
    </div>
  );
}

/// HTML 预览用 iframe + srcDoc + sandbox（无脚本、无同源），避免污染全局样式 + XSS。
function HtmlPreview({ html, loading, error }) {
  if (loading) return <LoadingState />;
  if (error) {
    return (
      <div className="rounded border border-rose-200 bg-rose-50 px-3 py-2 text-[12.5px] text-rose-700">
        {error}
      </div>
    );
  }
  if (!html) return null;
  return (
    <iframe
      srcDoc={html}
      title="HTML preview"
      sandbox=""
      className="h-[60vh] w-full rounded-lg border border-stone-200 bg-white"
    />
  );
}
