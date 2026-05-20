// 阶段 0 占位：仅渲染一个空白窗口的欢迎页。
// 真实的 sidebar + 页面在阶段 4 起逐步落地，见 docs/TASKS.md。
export default function App() {
  return (
    <div className="flex h-full w-full items-center justify-center bg-stone-50">
      <div className="text-center">
        <h1 className="text-xl font-semibold text-stone-900">WeeklyReport</h1>
        <p className="mt-2 text-sm text-stone-500">
          项目骨架已就绪，等待后续阶段填充功能。
        </p>
      </div>
    </div>
  );
}
