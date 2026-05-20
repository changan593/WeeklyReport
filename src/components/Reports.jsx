// 历史周报页（详见 docs/UI.md#44-历史周报页-reportsjsx）。
//
// 列出所有已存档报告（表格视图）；点击行进入详情；详情可复制 Markdown / 删除。

import { useState } from 'react';
import {
  deleteReport,
  getReport,
  listReports,
  useAsyncState,
  formatError,
} from '../api.js';
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
  const [items, loading, reload] = useAsyncState(listReports, []);
  const [openId, setOpenId] = useState(null);

  return (
    <div>
      <header className="mb-6">
        <h1 className="text-2xl font-medium text-stone-900">历史周报</h1>
        <p className="mt-1 text-[13px] text-stone-500">
          所有生成的周报都会本地存档，可作为下次生成的风格参考
        </p>
      </header>

      {loading && <LoadingState />}
      {!loading && items && items.length === 0 && (
        <EmptyState
          iconName="report"
          message="还没有生成过任何周报"
        />
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
  return (
    <div className="overflow-hidden rounded-lg border border-stone-200 bg-white">
      <table className="w-full text-[12.5px]">
        <thead className="border-b border-stone-200 bg-stone-50 text-stone-500">
          <tr>
            <th className="px-4 py-2 text-left font-medium">时间范围</th>
            <th className="px-4 py-2 text-left font-medium">模板</th>
            <th className="px-4 py-2 text-left font-medium">LLM 源</th>
            <th className="px-4 py-2 text-right font-medium">项目</th>
            <th className="px-4 py-2 text-right font-medium">Tokens</th>
            <th className="px-4 py-2 text-left font-medium">生成时间</th>
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
              <td className="px-4 py-2 text-stone-500">
                {r.provider_name || '—'}
              </td>
              <td className="px-4 py-2 text-right text-stone-700">{r.project_count}</td>
              <td className="px-4 py-2 text-right text-stone-500">{r.tokens_used}</td>
              <td className="px-4 py-2 text-stone-500">
                {formatTs(r.generated_at)}
              </td>
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
  const [data, loading, , error] = useAsyncState(() => getReport(id), [id]);
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(data?.content || '');
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      alert('复制失败：' + formatError(e));
    }
  }

  async function handleDelete() {
    if (!data?.record) return;
    if (!confirm(`删除报告「${data.record.week} - ${data.record.template_name}」？`)) {
      return;
    }
    try {
      await deleteReport(id);
      onDeleted();
    } catch (e) {
      alert(formatError(e));
    }
  }

  return (
    <Modal onClose={onClose} width="max-w-3xl">
      <ModalHeader title="周报详情" onClose={onClose} />
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
              <span>模板：{data.record.template_name}</span>
              {data.record.provider_name && (
                <span>LLM：{data.record.provider_name}</span>
              )}
              <span>项目：{data.record.project_count}</span>
              <span>Tokens：{data.record.tokens_used}</span>
              <span>{formatTs(data.record.generated_at)}</span>
            </div>
            <pre className="max-h-[60vh] overflow-auto rounded-lg bg-stone-50 p-5 font-mono text-[12px] text-stone-800 whitespace-pre-wrap">
              {data.content}
            </pre>
          </>
        )}
      </ModalBody>
      <ModalFooter>
        <SecondaryButton onClick={handleDelete}>
          <Icon name="trash" size={14} /> 删除
        </SecondaryButton>
        <div className="flex gap-2">
          <SecondaryButton onClick={handleCopy} disabled={!data?.content}>
            <Icon name="copy" size={14} />
            {copied ? '已复制' : '复制 Markdown'}
          </SecondaryButton>
          <PrimaryButton onClick={onClose}>关闭</PrimaryButton>
        </div>
      </ModalFooter>
    </Modal>
  );
}
