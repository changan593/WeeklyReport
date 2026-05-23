// 纯函数工具：可被任何组件 import + 可被 vitest 单测。
//
// 关键设计：这些函数都不接触 Tauri / DOM / fetch，便于在 node 环境跑。

/// 把多收件人字符串拆成邮箱数组。
/// 分隔符：逗号、分号、空格、换行（混搭）。空段、空白段会被丢弃。
export function splitEmails(text) {
  return (text || '')
    .split(/[,;\s]+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/// 把 ISO 8601 时间字符串截到分钟，便于在表格里紧凑展示。
/// `null` / `undefined` / 空 → 返回 `"—"`。
export function formatIsoMinute(iso) {
  if (!iso) return '—';
  return iso.replace('T', ' ').slice(0, 16);
}

/// 把 Tauri command 错误（Error 实例 / 字符串 / serde 序列化后的对象）统一转为可读字符串。
/// 用于 UI 显示与 logging。
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

/// 给 Promise 加超时上限：到点没 settle 就 reject 一个带 `timeout` 标记的错误。
/// 用于内联连接测试这类「后端理论必返但万一卡住要救场」的场景。
export function withTimeout(promise, ms, label) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      const err = new Error(
        `${label || '请求'}超时（${Math.round(ms / 1000)} 秒）`,
      );
      err.timeout = true;
      reject(err);
    }, ms);
    promise
      .then((v) => {
        clearTimeout(timer);
        resolve(v);
      })
      .catch((e) => {
        clearTimeout(timer);
        reject(e);
      });
  });
}
