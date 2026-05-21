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
} from './ui.jsx';

const TOOLS = [
  { key: 'claude-code', label: 'Claude Code', color: 'bg-orange-50 text-orange-700' },
  { key: 'codex', label: 'Codex', color: 'bg-emerald-50 text-emerald-700' },
];

export default function Workspaces() {
  const [items, loading, reload] = useAsyncState(listWorkspaces, []);
  const [editing, setEditing] = useState(null); // null | {} (new) | workspace object

  async function handleDelete(ws) {
    if (!confirm(`删除工作区「${ws.name}」？`)) return;
    try {
      await deleteWorkspace(ws.id);
      reload();
    } catch (e) {
      alert(formatError(e));
    }
  }

  return (
    <div>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <h1 className="text-2xl font-medium text-stone-900">工作区</h1>
          <p className="mt-1 text-[13px] text-stone-500">
            管理本机和远程服务器的日志来源
          </p>
        </div>
        <PrimaryButton onClick={() => setEditing(emptyWorkspace())}>
          <Icon name="plus" size={15} /> 添加工作区
        </PrimaryButton>
      </header>

      {loading && <LoadingState />}
      {!loading && items && items.length === 0 && (
        <EmptyState iconName="workspace" message="还没有工作区" />
      )}
      {!loading && items && items.length > 0 && (
        <div className="space-y-3">
          {items.map((ws) => (
            <WorkspaceCard
              key={ws.id}
              workspace={ws}
              onEdit={() => setEditing(ws)}
              onDelete={() => handleDelete(ws)}
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

function WorkspaceCard({ workspace, onEdit, onDelete }) {
  const isLocal = workspace.type === 'local';
  return (
    <div className="flex items-start gap-3 rounded-lg border border-stone-200 bg-white p-5 hover:border-stone-300">
      <div className="mt-0.5 flex h-9 w-9 items-center justify-center rounded-lg bg-stone-100 text-stone-500">
        <Icon name={isLocal ? 'laptop' : 'server'} size={18} />
      </div>
      <div className="flex-1 overflow-hidden">
        <div className="text-[14px] font-medium text-stone-900">{workspace.name}</div>
        <div className="mt-0.5 text-[12px] text-stone-500">
          {isLocal
            ? '本地机器'
            : `${workspace.user || 'root'}@${workspace.host || '?'}:${workspace.port ?? 22}`}
        </div>
        <div className="mt-2 flex flex-wrap gap-1.5">
          {workspace.tools?.map((t) => {
            const meta = TOOLS.find((x) => x.key === t);
            return (
              <span
                key={t}
                className={`rounded px-1.5 py-0.5 text-[11px] ${meta?.color || 'bg-stone-100 text-stone-600'}`}
              >
                {meta?.label || t}
              </span>
            );
          })}
          {workspace.tools?.includes('claude-code') && workspace.claude_path && (
            <span className="font-mono text-[11px] text-stone-500">
              {workspace.claude_path}
            </span>
          )}
        </div>
      </div>
      <div className="flex flex-col gap-1">
        <IconButton title="编辑" onClick={onEdit}>
          <Icon name="edit" size={15} />
        </IconButton>
        <IconButton title="删除" onClick={onDelete}>
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
        ? tools.filter((t) => t !== tool)
        : [...tools, tool];
      return { ...prev, tools: next };
    });
  }

  async function handleTest() {
    setTesting(true);
    setStatus({ type: 'info', msg: '正在测试连接…' });
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
      setStatus({ type: 'error', msg: '工作区名称不能为空' });
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
        title={initial.id ? '编辑工作区' : '添加工作区'}
        onClose={onClose}
      />
      <ModalBody className="space-y-4">
        {/* 类型 segment */}
        <div className="inline-flex rounded-md border border-stone-200 p-0.5">
          {['local', 'ssh'].map((t) => (
            <button
              key={t}
              type="button"
              onClick={() => set('type', t)}
              className={`rounded px-3 py-1 text-[12.5px] ${
                ws.type === t
                  ? 'bg-stone-900 text-white'
                  : 'text-stone-600 hover:text-stone-900'
              }`}
            >
              {t === 'local' ? '本地' : 'SSH 远程'}
            </button>
          ))}
        </div>

        <FormField label="名称">
          <Input
            value={ws.name}
            onChange={(v) => set('name', v)}
            placeholder="如：本机、GPU 服务器"
          />
        </FormField>

        {ws.type === 'ssh' && (
          <div className="grid grid-cols-2 gap-3">
            <FormField label="Host" className="col-span-2">
              <Input
                value={ws.host}
                onChange={(v) => set('host', v)}
                placeholder="example.com"
              />
            </FormField>
            <FormField label="User">
              <Input value={ws.user} onChange={(v) => set('user', v)} placeholder="root" />
            </FormField>
            <FormField label="Port">
              <Input
                value={String(ws.port ?? 22)}
                onChange={(v) => set('port', v)}
                placeholder="22"
              />
            </FormField>
            <FormField label="认证方式" className="col-span-2">
              <div className="inline-flex rounded-md border border-stone-200 p-0.5">
                {[
                  { v: 'key', label: '私钥' },
                  { v: 'password', label: '密码' },
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
                label="SSH 私钥路径（可选）"
                className="col-span-2"
                hint="填私钥路径（不是 .pub 公钥）；留空使用系统默认 ~/.ssh/id_ed25519"
              >
                <Mono
                  value={ws.ssh_key}
                  onChange={(v) => set('ssh_key', v)}
                  placeholder="~/.ssh/id_ed25519"
                />
              </FormField>
            ) : (
              <FormField
                label="SSH 密码"
                className="col-span-2"
                hint="需要系统安装 sshpass；密码以明文存于本地配置文件"
              >
                <Input
                  type="password"
                  value={ws.ssh_password}
                  onChange={(v) => set('ssh_password', v)}
                  placeholder="登录密码"
                />
              </FormField>
            )}
          </div>
        )}

        {/* Tools */}
        <FormField label="启用的工具">
          <div className="space-y-2">
            {TOOLS.map((t) => {
              const on = ws.tools?.includes(t.key);
              return (
                <div key={t.key} className="flex items-center gap-3">
                  <label className="flex items-center gap-2 text-[13px]">
                    <input
                      type="checkbox"
                      checked={!!on}
                      onChange={() => toggleTool(t.key)}
                    />
                    <span>{t.label}</span>
                  </label>
                  {on && (
                    <Mono
                      value={t.key === 'claude-code' ? ws.claude_path : ws.codex_path}
                      onChange={(v) =>
                        set(t.key === 'claude-code' ? 'claude_path' : 'codex_path', v)
                      }
                      placeholder={t.key === 'claude-code' ? '~/.claude' : '~/.codex'}
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
          {testing ? '测试中…' : '测试连接'}
        </SecondaryButton>
        <div className="flex gap-2">
          <SecondaryButton onClick={onClose} disabled={saving}>
            取消
          </SecondaryButton>
          <PrimaryButton onClick={handleSave} disabled={saving}>
            {saving ? '保存中…' : '保存'}
          </PrimaryButton>
        </div>
      </ModalFooter>
    </Modal>
  );
}
