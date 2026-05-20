// App shell：侧边栏 + 路由切换。
//
// 阶段 4 接入 Workspaces / Providers 两页；其余页面占位，留待后续阶段填充。
// 详见 docs/UI.md#2-布局结构。

import { useState } from 'react';
import { Icon } from './components/ui.jsx';
import Workspaces from './components/Workspaces.jsx';
import Providers from './components/Providers.jsx';

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
            disabled
            title="生成对话框由阶段 5 实现"
            className="flex w-full items-center justify-center gap-1.5 rounded-md bg-stone-200 px-3 py-2 text-[12.5px] text-stone-400"
          >
            <Icon name="sparkle" size={15} />
            <span>生成周报</span>
          </button>
        </div>
      </aside>

      {/* 主内容 */}
      <main className="flex-1 overflow-auto">
        <div className="mx-auto max-w-4xl px-10 py-10">
          <Page page={page} />
        </div>
      </main>
    </div>
  );
}

function Page({ page }) {
  if (page === 'workspaces') return <Workspaces />;
  if (page === 'providers') return <Providers />;
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
