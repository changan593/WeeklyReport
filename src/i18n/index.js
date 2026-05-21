// 应用内 i18n：最小自实现（不引第三方依赖，符合 CLAUDE.md 的"轻量"约束）。
//
// 用法：
//   import { I18nProvider, useTranslation } from './i18n';
//   <I18nProvider initial="zh-CN" persist={true}>...</I18nProvider>
//   const { t, lang, setLang } = useTranslation();
//   t('nav.workspaces')  // 按点号路径查 zh-CN.js 或 en.js
//
// 切换语言：setLang('en') 立即触发所有使用 t() 的组件 re-render。
//
// 持久化：
//   - LocalStorage 'wr_lang'：启动时同步可用，避免 first-paint flicker
//   - 后端 Settings.language：跟 LocalStorage 一致；setLang 同时写两边

import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import zhCN from './locales/zh-CN.js';
import en from './locales/en.js';

const RESOURCES = { 'zh-CN': zhCN, en };
const LS_KEY = 'wr_lang';
const DEFAULT_LANG = 'zh-CN';

const I18nContext = createContext({
  lang: DEFAULT_LANG,
  setLang: () => {},
  t: (key) => key,
});

/** 按点号路径在资源对象里取值；缺 key 时返回 key 本身（便于发现漏翻）。 */
function lookup(resource, key) {
  const parts = key.split('.');
  let cur = resource;
  for (const p of parts) {
    if (cur == null || typeof cur !== 'object') return undefined;
    cur = cur[p];
  }
  return typeof cur === 'string' ? cur : undefined;
}

export function I18nProvider({ children, initial }) {
  // 同步初始：localStorage 优先，否则 initial prop，否则默认
  const [lang, setLangState] = useState(() => {
    try {
      const ls = localStorage.getItem(LS_KEY);
      if (ls && RESOURCES[ls]) return ls;
    } catch {
      /* localStorage 不可用就走 initial */
    }
    return RESOURCES[initial] ? initial : DEFAULT_LANG;
  });

  const setLang = useCallback((next) => {
    if (!RESOURCES[next]) return;
    setLangState(next);
    try {
      localStorage.setItem(LS_KEY, next);
    } catch {
      /* localStorage 写失败不致命，下次启动用 initial */
    }
  }, []);

  const t = useCallback(
    (key, vars) => {
      let v = lookup(RESOURCES[lang], key);
      // fallback 到中文，避免英文资源漏 key 时显示生 key
      if (v == null && lang !== DEFAULT_LANG) {
        v = lookup(RESOURCES[DEFAULT_LANG], key);
      }
      if (v == null) return key;
      // 变量插值：t('xxx', { name: 'foo' }) → '... {name} ...'.replace('{name}', 'foo')
      if (vars && typeof v === 'string') {
        return v.replace(/\{(\w+)\}/g, (_, k) =>
          vars[k] != null ? String(vars[k]) : `{${k}}`
        );
      }
      return v;
    },
    [lang]
  );

  const value = useMemo(() => ({ lang, setLang, t }), [lang, setLang, t]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

/** 组件用：const { t, lang, setLang } = useTranslation(); */
export function useTranslation() {
  return useContext(I18nContext);
}

/** 启动时从后端 Settings.language 同步到 LocalStorage / state；与后端保持一致。 */
export function useSyncLangFromBackend(getSettings) {
  const { lang, setLang } = useTranslation();
  useEffect(() => {
    getSettings()
      .then((s) => {
        const backendLang = s?.language;
        if (backendLang && RESOURCES[backendLang] && backendLang !== lang) {
          setLang(backendLang);
        }
      })
      .catch(() => {
        /* 启动时拿不到 settings 不阻塞渲染，沿用 LocalStorage 的语言 */
      });
    // 仅启动跑一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}

export const SUPPORTED_LANGS = Object.keys(RESOURCES);
