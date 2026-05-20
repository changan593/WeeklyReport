// Tauri command 包装层 + 通用异步加载 hook。
//
// 所有 Tauri command 在此处统一封装，便于前端只 import 这一个文件，
// 也便于将来切换 IPC 机制（如 mock）时只动一处。

import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';

// ============================================================
// Workspaces
// ============================================================

export function listWorkspaces() {
  return invoke('list_workspaces');
}

export function saveWorkspace(workspace) {
  return invoke('save_workspace', { workspace });
}

export function deleteWorkspace(id) {
  return invoke('delete_workspace', { id });
}

export function testWorkspaceConnection(workspace) {
  return invoke('test_workspace_connection', { workspace });
}

// ============================================================
// LLM Providers
// ============================================================

export function listProviders() {
  return invoke('list_providers');
}

export function saveProvider(provider) {
  return invoke('save_provider', { provider });
}

export function deleteProvider(id) {
  return invoke('delete_provider', { id });
}

export function testProvider(provider) {
  return invoke('test_provider', { provider });
}

export function llmPresets() {
  return invoke('llm_presets');
}

// ============================================================
// Misc
// ============================================================

export function dataDirPath() {
  return invoke('data_dir_path');
}

// ============================================================
// useAsyncState hook
//
// 统一处理 loading / error / reload 模式：
//
//   const [data, loading, reload, error] = useAsyncState(listWorkspaces, []);
//
// `loader` 必须返回 Promise。`deps` 变化时自动重新加载。
// ============================================================

export function useAsyncState(loader, deps = []) {
  const [data, setData] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const aliveRef = useRef(true);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const v = await loader();
      if (aliveRef.current) setData(v);
    } catch (e) {
      if (aliveRef.current) setError(formatError(e));
    } finally {
      if (aliveRef.current) setLoading(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  useEffect(() => {
    aliveRef.current = true;
    reload();
    return () => {
      aliveRef.current = false;
    };
  }, [reload]);

  return [data, loading, reload, error];
}

/// 把 Tauri command 错误（Error 实例或字符串）统一转为可读字符串。
export function formatError(e) {
  if (!e) return '未知错误';
  if (typeof e === 'string') return e;
  if (e.message) return e.message;
  try {
    return JSON.stringify(e);
  } catch (_) {
    return String(e);
  }
}
