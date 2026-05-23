// 工作区页（详见 docs/UI.md#41-工作区页-workspacesjsx）。
//
// 功能：列出 / 添加 / 编辑 / 删除 workspace；测试连接（本机 + SSH）。
// SSH 字段虽然在编辑器里也可填，但 SSH 连接测试由阶段 6 接通。

import { useState } from 'react';
import {
  deleteWorkspace,
  listWorkspaces,
  saveWorkspace,
  testWorkspaceConnection,
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
  StatusBanner,
  StatusPill,
} from './ui.jsx';

const TOOLS = [
  { key: 'claude-code', label: 'Claude Code', color: 'bg-orange-50 text-orange-700' },
  { key: 'codex', label: 'Codex', color: 'bg-emerald-50 text-emerald-700' },
];

export default function Workspaces() {
  const { t } = useTranslation();
  const [items, loading, reload] = useAsyncState(listWorkspaces, []);
  const [editing, setEditing] = useState(null); // null | {} (new) | workspace object
  // 会话级连接状态：id → { state: 'idle'|'testing'|'ok'|'failed', message }
  // 不持久化（重启清空），避免给"曾测过"造成假信号。
  const [testStatus, setTestStatus] = useState({});

  async function handleDelete(ws) {
    if (!confirm(t('workspaces.confirm_delete', { name: ws.name }))) return;
    try {
      await deleteWorkspace(ws.id);
      reload();
    } catch (e) {
      alert(formatError(e));
    }
  }

  async function handleTest(ws) {
    setTestStatus((prev) => ({ ...prev, [ws.id]: { state: 'testing' } }));
    try {
      // 前端超时兜底：避免后端 ssh 子进程异常时 UI 卡死在「测试中」。
      // 后端本身有 ConnectTimeout=8s，理论 ≤ 20s 必返；这里给 45s 留足余量。
      const msg = await withTimeout(testWorkspaceConnection(ws), 45_000);
      setTestStatus((prev) => ({ ...prev, [ws.id]: { state: 'ok', message: msg } }));
    } catch (e) {
      // 把原始错误也输出到 DevTools console，方便排查
      // （UI 上只展示精简一行；详细 stack 在控制台里看）
      // eslint-disable-next-line no-console
      console.error('[workspace test failed]', ws?.id, e);
      setTestStatus((prev) => ({
        ...prev,
        [ws.id]: { state: 'failed', message: formatError(e) },
      }));
    }
  }

  return (
    <div>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <h1 className="text-2xl font-medium text-stone-900">{t('workspaces.title')}</h1>
          <p className="mt-1 text-[13px] text-stone-500">{t('workspaces.subtitle')}</p>
        </div>
        <PrimaryButton onClick={() => setEditing(emptyWorkspace())}>
          <Icon name="plus" size={15} /> {t('workspaces.add')}
        </PrimaryButton>
      </header>

      {loading && <LoadingState />}
      {!loading && items && items.length === 0 && (
        <EmptyState iconName="workspace" message={t('workspaces.empty')} />
      )}
      {!loading && items && items.length > 0 && (
        <div className="space-y-3">
          {items.map((ws) => (
            <WorkspaceCard
              key={ws.id}
              workspace={ws}
              status={testStatus[ws.id]}
              onEdit={() => setEditing(ws)}
              onDelete={() => handleDelete(ws)}
              onTest={() => handleTest(ws)}
            />
          ))}
        </div>
      )}

      {editing && (
        <WorkspaceEditor
          initial={editing}
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

function WorkspaceCard({ workspace, status, onEdit, onDelete, onTest }) {
  const { t } = useTranslation();
  const isLocal = workspace.type === 'local';
  // 本地工作区始终视为就绪（无网络可断）；SSH 默认"未测试"，由 status 覆盖。
  const pill = (() => {
    const st = status?.state;
    if (isLocal) {
      return { tone: 'success', label: t('workspaces.card.status.local') };
    }
    if (st === 'testing') {
      return { tone: 'info', label: t('workspaces.card.status.testing'), pulse: true };
    }
    if (st === 'ok') {
      return { tone: 'success', label: t('workspaces.card.status.ok'), title: status.message };
    }
    if (st === 'failed') {
      return { tone: 'error', label: t('workspaces.card.status.failed'), title: status.message };
    }
    return { tone: 'neutral', label: t('workspaces.card.status.untested') };
  })();
  const testing = status?.state === 'testing';
  return (
    <div className="flex items-start gap-3 rounded-lg border border-stone-200 bg-white p-5 hover:border-stone-300">
      <div
        className={`mt-0.5 flex h-9 w-9 items-center justify-center rounded-lg ${
          isLocal ? 'bg-emerald-50 text-emerald-700' : 'bg-stone-100 text-stone-500'
        }`}
      >
        <Icon name={isLocal ? 'laptop' : 'server'} size={18} />
      </div>
      <div className="flex-1 overflow-hidden">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[14px] font-medium text-stone-900">{workspace.name}</span>
          <StatusPill tone={pill.tone} label={pill.label} title={pill.title} pulse={pill.pulse} />
        </div>
        <div className="mt-0.5 text-[12px] text-stone-500">
          {isLocal
            ? t('workspaces.card.local_label')
            : `${workspace.user || 'root'}@${workspace.host || '?'}:${workspace.port ?? 22}`}
        </div>
        <div className="mt-2 flex flex-wrap gap-1.5">
          {workspace.tools?.map((tool) => {
            const meta = TOOLS.find((x) => x.key === tool);
            return (
              <span
                key={tool}
                className={`rounded px-1.5 py-0.5 text-[11px] ${meta?.color || 'bg-stone-100 text-stone-600'}`}
              >
                {meta?.label || tool}
              </span>
            );
          })}
          {workspace.tools?.includes('claude-code') && workspace.claude_path && (
            <span className="font-mono text-[11px] text-stone-500">{workspace.claude_path}</span>
          )}
        </div>
        {status?.state === 'failed' && status.message && (
          <div className="mt-2 line-clamp-2 text-[11.5px] text-rose-700">{status.message}</div>
        )}
      </div>
      <div className="flex flex-col gap-1">
        <IconButton
          title={t('workspaces.card.test')}
          onClick={onTest}
          disabled={testing}
        >
          <Icon name="refresh" size={15} className={testing ? 'animate-spin' : ''} />
        </IconButton>
        <IconButton title={t('workspaces.card.edit')} onClick={onEdit}>
          <Icon name="edit" size={15} />
        </IconButton>
        <IconButton title={t('workspaces.card.delete')} onClick={onDelete}>
          <Icon name="trash" size={15} />
        </IconButton>
      </div>
    </div>
  );
}

function emptyWorkspace() {
  return {
    id: '',
    name: '',
    type: 'local',
    host: '',
    user: '',
    port: 22,
    auth_method: 'key',
    ssh_key: '',
    ssh_password: '',
    claude_path: '~/.claude',
    codex_path: '~/.codex',
    tools: ['claude-code', 'codex'],
  };
}

function WorkspaceEditor({ initial, onClose, onSaved }) {
  const { t } = useTranslation();
  const [ws, setWs] = useState({ ...initial });
  const [status, setStatus] = useState(null);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);

  function set(field, value) {
    setWs((prev) => ({ ...prev, [field]: value }));
  }

  function toggleTool(tool) {
    setWs((prev) => {
      const tools = prev.tools || [];
      const next = tools.includes(tool)
        ? tools.filter((x) => x !== tool)
        : [...tools, tool];
      return { ...prev, tools: next };
    });
  }

  async function handleTest() {
    setTesting(true);
    setStatus({ type: 'info', msg: t('workspaces.editor.testing') });
    try {
      const msg = await testWorkspaceConnection(ws);
      setStatus({ type: 'success', msg });
    } catch (e) {
      setStatus({ type: 'error', msg: formatError(e) });
    } finally {
      setTesting(false);
    }
  }

  async function handleSave() {
    if (!ws.name.trim()) {
      setStatus({ type: 'error', msg: t('workspaces.editor.name_required') });
      return;
    }
    setSaving(true);
    try {
      // port 字段需要是数字
      const payload = { ...ws, port: Number(ws.port) || 22 };
      await saveWorkspace(payload);
      onSaved();
    } catch (e) {
      setStatus({ type: 'error', msg: formatError(e) });
      setSaving(false);
    }
  }

  return (
    <Modal onClose={onClose} width="max-w-xl">
      <ModalHeader
        title={initial.id ? t('workspaces.editor.title_edit') : t('workspaces.editor.title_new')}
        onClose={onClose}
      />
      <ModalBody className="space-y-4">
        {/* 类型 segment */}
        <div className="inline-flex rounded-md border border-stone-200 p-0.5">
          {['local', 'ssh'].map((typeKey) => (
            <button
              key={typeKey}
              type="button"
              onClick={() => set('type', typeKey)}
              className={`rounded px-3 py-1 text-[12.5px] ${
                ws.type === typeKey
                  ? 'bg-stone-900 text-white'
                  : 'text-stone-600 hover:text-stone-900'
              }`}
            >
              {typeKey === 'local'
                ? t('workspaces.editor.type.local')
                : t('workspaces.editor.type.ssh')}
            </button>
          ))}
        </div>

        <FormField label={t('workspaces.editor.name')}>
          <Input
            value={ws.name}
            onChange={(v) => set('name', v)}
            placeholder={t('workspaces.editor.name_placeholder')}
          />
        </FormField>

        {ws.type === 'ssh' && (
          <div className="grid grid-cols-2 gap-3">
            <FormField label={t('workspaces.editor.host')} className="col-span-2">
              <Input
                value={ws.host}
                onChange={(v) => set('host', v)}
                placeholder="example.com"
              />
            </FormField>
            <FormField label={t('workspaces.editor.user')}>
              <Input value={ws.user} onChange={(v) => set('user', v)} placeholder="root" />
            </FormField>
            <FormField label={t('workspaces.editor.port')}>
              <Input
                value={String(ws.port ?? 22)}
                onChange={(v) => set('port', v)}
                placeholder="22"
              />
            </FormField>
            <FormField label={t('workspaces.editor.auth')} className="col-span-2">
              <div className="inline-flex rounded-md border border-stone-200 p-0.5">
                {[
                  { v: 'key', label: t('workspaces.editor.auth_key') },
                  { v: 'password', label: t('workspaces.editor.auth_password') },
                ].map((opt) => (
                  <button
                    key={opt.v}
                    type="button"
                    onClick={() => set('auth_method', opt.v)}
                    className={`rounded px-3 py-1 text-[12.5px] ${
                      (ws.auth_method || 'key') === opt.v
                        ? 'bg-stone-900 text-white'
                        : 'text-stone-600 hover:text-stone-900'
                    }`}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </FormField>
            {(ws.auth_method || 'key') === 'key' ? (
              <FormField
                label={t('workspaces.editor.ssh_key_label')}
                className="col-span-2"
                hint={t('workspaces.editor.ssh_key_hint')}
              >
                <Mono
                  value={ws.ssh_key}
                  onChange={(v) => set('ssh_key', v)}
                  placeholder="~/.ssh/id_ed25519"
                />
              </FormField>
            ) : (
              <FormField
                label={t('workspaces.editor.ssh_password_label')}
                className="col-span-2"
                hint={t('workspaces.editor.ssh_password_hint')}
              >
                <Input
                  type="password"
                  value={ws.ssh_password}
                  onChange={(v) => set('ssh_password', v)}
                  placeholder={t('workspaces.editor.ssh_password_placeholder')}
                />
              </FormField>
            )}
          </div>
        )}

        {/* Tools */}
        <FormField label={t('workspaces.editor.tools')}>
          <div className="space-y-2">
            {TOOLS.map((tool) => {
              const on = ws.tools?.includes(tool.key);
              return (
                <div key={tool.key} className="flex items-center gap-3">
                  <label className="flex items-center gap-2 text-[13px]">
                    <input
                      type="checkbox"
                      checked={!!on}
                      onChange={() => toggleTool(tool.key)}
                    />
                    <span>{tool.label}</span>
                  </label>
                  {on && (
                    <Mono
                      value={tool.key === 'claude-code' ? ws.claude_path : ws.codex_path}
                      onChange={(v) =>
                        set(tool.key === 'claude-code' ? 'claude_path' : 'codex_path', v)
                      }
                      placeholder={tool.key === 'claude-code' ? '~/.claude' : '~/.codex'}
                      className="flex-1"
                    />
                  )}
                </div>
              );
            })}
          </div>
        </FormField>

        <StatusBanner status={status} />
      </ModalBody>
      <ModalFooter>
        <SecondaryButton onClick={handleTest} disabled={testing}>
          <Icon name="refresh" size={14} />
          {testing ? t('workspaces.editor.test_running') : t('workspaces.editor.test_btn')}
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
