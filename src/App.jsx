// App shell：侧边栏 + 路由切换 + 生成对话框入口。
//
// 详见 docs/UI.md#2-布局结构。

import { useState } from 'react';
import GenerateDialog from './components/GenerateDialog.jsx';
import Providers from './components/Providers.jsx';
import Reports from './components/Reports.jsx';
import Templates from './components/Templates.jsx';
import { Icon } from './components/ui.jsx';
import Workspaces from './components/Workspaces.jsx';

const NAV = [
  { key: 'workspaces', label: '工作区', icon: 'workspace' },
  { key: 'providers', label: 'LLM 源', icon: 'llm' },
  { key: 'templates', label: '周报模板', icon: 'template' },
  { key: 'reports', label: '历史周报', icon: 'report' },
  { key: 'schedules', label: '定时任务', icon: 'schedule' },
  { key: 'settings', label: '设置', icon: 'settings' },
];

export default function App() {
  const [page, setPage] = useState('workspaces');
  const [genOpen, setGenOpen] = useState(false);
  // 用一个 nonce 触发 Reports 页面重载（生成完报告后）
  const [reportsNonce, setReportsNonce] = useState(0);

  return (
    <div className="flex h-screen w-full bg-stone-50 text-stone-900">
      {/* 侧边栏 */}
      <aside className="flex w-56 flex-col border-r border-stone-200 bg-white">
        <div className="flex h-14 items-center px-5">
          <div className="text-[15px] font-semibold tracking-tight text-stone-900">
            WeeklyReport
          </div>
        </div>
        <nav className="flex-1 px-2 py-2">
          {NAV.map((item) => (
            <button
              key={item.key}
              type="button"
              onClick={() => setPage(item.key)}
              className={`mb-0.5 flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-[13px] transition-colors ${
                page === item.key
                  ? 'bg-stone-100 text-stone-900'
                  : 'text-stone-600 hover:bg-stone-50 hover:text-stone-900'
              }`}
            >
              <Icon name={item.icon} size={16} />
              <span>{item.label}</span>
            </button>
          ))}
        </nav>
        <div className="border-t border-stone-200 p-3">
          <button
            type="button"
            onClick={() => setGenOpen(true)}
            className="flex w-full items-center justify-center gap-1.5 rounded-md bg-stone-900 px-3 py-2 text-[12.5px] text-white hover:bg-stone-800"
          >
            <Icon name="sparkle" size={15} />
            <span>生成周报</span>
          </button>
        </div>
      </aside>

      {/* 主内容 */}
      <main className="flex-1 overflow-auto">
        <div className="mx-auto max-w-4xl px-10 py-10">
          <Page page={page} reportsNonce={reportsNonce} />
        </div>
      </main>

      {genOpen && (
        <GenerateDialog
          onClose={() => setGenOpen(false)}
          onGenerated={() => setReportsNonce((n) => n + 1)}
        />
      )}
    </div>
  );
}

function Page({ page, reportsNonce }) {
  if (page === 'workspaces') return <Workspaces />;
  if (page === 'providers') return <Providers />;
  if (page === 'templates') return <Templates />;
  if (page === 'reports') return <Reports key={reportsNonce} />;
  return (
    <Placeholder
      title={NAV.find((n) => n.key === page)?.label || ''}
      description="此页面由后续阶段实现。"
    />
  );
}

function Placeholder({ title, description }) {
  return (
    <div>
      <h1 className="text-2xl font-medium text-stone-900">{title}</h1>
      <p className="mt-2 text-[13px] text-stone-500">{description}</p>
    </div>
  );
}
