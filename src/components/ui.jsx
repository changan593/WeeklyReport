// 共用 UI primitives + Icon 库（详见 docs/UI.md#3-共用组件 uijsx）。
//
// 视觉规范：stone 系列 warm gray，圆角 lg/md/sm，line-stroke 风格 SVG。
// 所有组件都是无状态函数组件。

import { useEffect, useRef } from 'react';
import { useTranslation } from '../i18n/index.jsx';

// ============================================================
// Icon 库
// ============================================================

const ICONS = {
  // 侧边栏
  workspace:
    'M3 7h18M3 12h18M3 17h18',
  llm:
    'M12 2v4M12 18v4M2 12h4M18 12h4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83',
  template:
    'M4 4h16v4H4zM4 12h10v8H4zM18 12h2v8h-2z',
  report:
    'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6M16 13H8M16 17H8M10 9H8',
  schedule:
    'M12 22a10 10 0 1 1 0-20 10 10 0 0 1 0 20zM12 6v6l4 2',
  settings:
    'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h0a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z',
  // 操作
  plus: 'M12 5v14M5 12h14',
  sparkle:
    'M12 3l1.5 4.5L18 9l-4.5 1.5L12 15l-1.5-4.5L6 9l4.5-1.5z M19 14l.75 2.25L22 17l-2.25.75L19 20l-.75-2.25L16 17l2.25-.75z',
  edit: 'M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7 M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4z',
  trash:
    'M3 6h18 M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6 M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2 M10 11v6 M14 11v6',
  check: 'M20 6 9 17l-5-5',
  x: 'M18 6 6 18M6 6l12 12',
  refresh:
    'M23 4v6h-6 M1 20v-6h6 M3.51 9a9 9 0 0 1 14.85-3.36L23 10 M1 14l4.64 4.36A9 9 0 0 0 20.49 15',
  star: 'M12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26z',
  server:
    'M2 4h20v6H2zM2 14h20v6H2z M6 7h.01 M6 17h.01',
  laptop:
    'M3 5h18v11H3zM2 20h20',
  download: 'M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4 M7 10l5 5 5-5 M12 15V3',
  copy: 'M20 9h-9a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2v-9a2 2 0 0 0-2-2z M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1',
  play: 'M5 3l14 9-14 9z',
  clock: 'M12 22a10 10 0 1 1 0-20 10 10 0 0 1 0 20zM12 6v6l4 2',
  mail: 'M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2zM22 6l-10 7L2 6',
  chevronR: 'M9 18l6-6-6-6',
};

export function Icon({ name, size = 18, className = '' }) {
  const d = ICONS[name];
  if (!d) {
    return (
      <span
        className={`inline-block ${className}`}
        style={{ width: size, height: size }}
      />
    );
  }
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {/* path 可能含多段；统一作为单个 path 渲染 */}
      <path d={d} />
    </svg>
  );
}

// ============================================================
// Buttons
// ============================================================

export function IconButton({ title, onClick, children, disabled, className = '' }) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      disabled={disabled}
      className={`inline-flex h-7 w-7 items-center justify-center rounded text-stone-500 hover:bg-stone-100 hover:text-stone-900 disabled:opacity-40 ${className}`}
    >
      {children}
    </button>
  );
}

export function PrimaryButton({ onClick, disabled, children, className = '', type = 'button' }) {
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={disabled}
      className={`inline-flex items-center gap-1.5 rounded-md bg-stone-900 px-3 py-1.5 text-[12.5px] text-white hover:bg-stone-800 disabled:opacity-40 ${className}`}
    >
      {children}
    </button>
  );
}

export function SecondaryButton({ onClick, disabled, children, className = '', type = 'button' }) {
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={disabled}
      className={`inline-flex items-center gap-1.5 rounded-md border border-stone-200 bg-white px-3 py-1.5 text-[12.5px] text-stone-700 hover:border-stone-300 hover:bg-stone-50 disabled:opacity-40 ${className}`}
    >
      {children}
    </button>
  );
}

// ============================================================
// Form fields
// ============================================================

export function FormField({ label, hint, children, className = '' }) {
  return (
    <div className={`flex flex-col gap-1 ${className}`}>
      {label && (
        <label className="text-[11.5px] font-medium text-stone-600">{label}</label>
      )}
      {children}
      {hint && <p className="text-[11px] text-stone-500">{hint}</p>}
    </div>
  );
}

export function Input({ value, onChange, placeholder, type = 'text', disabled, className = '' }) {
  return (
    <input
      type={type}
      value={value ?? ''}
      onChange={(e) => onChange?.(e.target.value)}
      placeholder={placeholder}
      disabled={disabled}
      className={`rounded border border-stone-200 bg-white px-2.5 py-1.5 text-[13px] text-stone-900 focus:border-stone-400 disabled:bg-stone-50 disabled:text-stone-400 ${className}`}
    />
  );
}

export function Mono({ value, onChange, placeholder, type = 'text', disabled, className = '' }) {
  return (
    <Input
      value={value}
      onChange={onChange}
      placeholder={placeholder}
      type={type}
      disabled={disabled}
      className={`font-mono text-[12px] ${className}`}
    />
  );
}

export function Textarea({ value, onChange, placeholder, rows = 4, disabled, className = '' }) {
  return (
    <textarea
      value={value ?? ''}
      onChange={(e) => onChange?.(e.target.value)}
      placeholder={placeholder}
      rows={rows}
      disabled={disabled}
      className={`rounded border border-stone-200 bg-white px-2.5 py-1.5 text-[13px] text-stone-900 focus:border-stone-400 disabled:bg-stone-50 disabled:text-stone-400 ${className}`}
    />
  );
}

export function Select({ value, onChange, children, disabled, className = '' }) {
  return (
    <select
      value={value ?? ''}
      onChange={(e) => onChange?.(e.target.value)}
      disabled={disabled}
      className={`rounded border border-stone-200 bg-white px-2.5 py-1.5 text-[13px] text-stone-900 focus:border-stone-400 disabled:bg-stone-50 disabled:text-stone-400 ${className}`}
    >
      {children}
    </select>
  );
}

export function Toggle({ on, onChange, disabled }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      onClick={() => !disabled && onChange?.(!on)}
      disabled={disabled}
      className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors disabled:opacity-40 ${
        on ? 'bg-stone-900' : 'bg-stone-300'
      }`}
    >
      <span
        className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
          on ? 'translate-x-4' : 'translate-x-0.5'
        }`}
      />
    </button>
  );
}

// ============================================================
// Modal
// ============================================================

export function Modal({ onClose, children, width = 'max-w-lg' }) {
  // ESC 关闭
  useEffect(() => {
    function onKey(e) {
      if (e.key === 'Escape') onClose?.();
    }
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  // 仅当 mousedown 和 mouseup 都发生在 backdrop 时才关闭，避免在输入框
  // 内开始拖选文本、鼠标松开点落到 backdrop 上而误触发的"drag-to-close"
  const downOnBackdrop = useRef(false);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-stone-900/30 p-4"
      onMouseDown={(e) => {
        downOnBackdrop.current = e.target === e.currentTarget;
      }}
      onMouseUp={(e) => {
        if (downOnBackdrop.current && e.target === e.currentTarget) {
          onClose?.();
        }
        downOnBackdrop.current = false;
      }}
    >
      <div
        className={`w-full ${width} max-h-[calc(100vh-2rem)] overflow-hidden rounded-lg border border-stone-200 bg-white shadow-xl`}
      >
        {children}
      </div>
    </div>
  );
}

export function ModalHeader({ title, onClose }) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center justify-between border-b border-stone-200 px-5 py-3">
      <h2 className="text-[14px] font-medium text-stone-900">{title}</h2>
      <IconButton title={t('ui.modal.close')} onClick={onClose}>
        <Icon name="x" size={16} />
      </IconButton>
    </div>
  );
}

export function ModalBody({ children, className = '' }) {
  return (
    <div className={`max-h-[60vh] overflow-y-auto px-5 py-4 ${className}`}>{children}</div>
  );
}

export function ModalFooter({ children, className = '' }) {
  return (
    <div className={`flex items-center justify-between border-t border-stone-200 px-5 py-3 ${className}`}>
      {children}
    </div>
  );
}

// ============================================================
// StatusBanner
// ============================================================

export function StatusBanner({ status }) {
  if (!status) return null;
  const styles = {
    success: 'bg-emerald-50 text-emerald-700 border-emerald-200',
    error: 'bg-rose-50 text-rose-700 border-rose-200',
    warning: 'bg-amber-50 text-amber-700 border-amber-200',
    info: 'bg-stone-50 text-stone-700 border-stone-200',
  };
  const cls = styles[status.type] || styles.info;
  return (
    <div
      className={`whitespace-pre-wrap rounded border px-3 py-2 text-[12.5px] ${cls}`}
    >
      {status.msg}
    </div>
  );
}

// ============================================================
// EmptyState
// ============================================================

export function EmptyState({ iconName = 'sparkle', message, children }) {
  const { t } = useTranslation();
  return (
    <div className="py-16 text-center">
      <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-xl bg-stone-100 text-stone-400">
        <Icon name={iconName} size={22} />
      </div>
      <p className="text-[13px] text-stone-500">{message ?? t('ui.empty.default')}</p>
      {children && <div className="mt-4">{children}</div>}
    </div>
  );
}

// ============================================================
// LoadingState
// ============================================================

export function LoadingState({ message }) {
  const { t } = useTranslation();
  return (
    <div className="py-12 text-center text-[13px] text-stone-400">
      {message ?? t('ui.loading.default')}
    </div>
  );
}
