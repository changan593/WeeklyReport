import { describe, expect, it } from 'vitest';
import { formatError, formatIsoMinute, splitEmails, withTimeout } from './utils.js';

describe('splitEmails', () => {
  it('splits on comma', () => {
    expect(splitEmails('a@x.com, b@x.com')).toEqual(['a@x.com', 'b@x.com']);
  });

  it('splits on semicolon', () => {
    expect(splitEmails('a@x.com;b@x.com')).toEqual(['a@x.com', 'b@x.com']);
  });

  it('splits on whitespace including newline', () => {
    expect(splitEmails('a@x.com\nb@x.com  c@x.com')).toEqual([
      'a@x.com',
      'b@x.com',
      'c@x.com',
    ]);
  });

  it('drops empty pieces from trailing / repeated separators', () => {
    expect(splitEmails(',,a@x.com,,,b@x.com,,')).toEqual(['a@x.com', 'b@x.com']);
  });

  it('returns empty array for null / undefined / empty', () => {
    expect(splitEmails(null)).toEqual([]);
    expect(splitEmails(undefined)).toEqual([]);
    expect(splitEmails('')).toEqual([]);
    expect(splitEmails('   ')).toEqual([]);
  });

  it('preserves order', () => {
    expect(splitEmails('z@x.com a@x.com m@x.com')).toEqual([
      'z@x.com',
      'a@x.com',
      'm@x.com',
    ]);
  });
});

describe('formatIsoMinute', () => {
  it('truncates ISO 8601 to minute', () => {
    expect(formatIsoMinute('2026-05-20T14:35:21.123Z')).toBe('2026-05-20 14:35');
  });

  it('returns em-dash for null / undefined / empty', () => {
    expect(formatIsoMinute(null)).toBe('—');
    expect(formatIsoMinute(undefined)).toBe('—');
    expect(formatIsoMinute('')).toBe('—');
  });

  it('replaces T separator with space', () => {
    const out = formatIsoMinute('2026-05-20T09:00:00+08:00');
    expect(out).not.toContain('T');
    expect(out).toBe('2026-05-20 09:00');
  });
});

describe('formatError', () => {
  it('passes through plain string', () => {
    expect(formatError('boom')).toBe('boom');
  });

  it('extracts message from Error instance', () => {
    expect(formatError(new Error('network failed'))).toBe('network failed');
  });

  it('handles null / undefined / empty', () => {
    expect(formatError(null)).toBe('未知错误');
    expect(formatError(undefined)).toBe('未知错误');
  });

  it('falls back to JSON for plain object', () => {
    const out = formatError({ code: 500, reason: 'x' });
    expect(out).toContain('500');
    expect(out).toContain('reason');
  });

  it('falls back to String() if JSON.stringify throws (circular ref)', () => {
    const o = {};
    o.self = o;
    const out = formatError(o);
    // 不抛错；具体格式不限，只要是 string
    expect(typeof out).toBe('string');
    expect(out.length).toBeGreaterThan(0);
  });
});

describe('withTimeout', () => {
  it('resolves when underlying promise settles in time', async () => {
    const v = await withTimeout(Promise.resolve(42), 1000);
    expect(v).toBe(42);
  });

  it('propagates underlying rejection without timing out first', async () => {
    await expect(
      withTimeout(Promise.reject(new Error('boom')), 1000),
    ).rejects.toThrow('boom');
  });

  it('rejects with timeout error if underlying takes too long', async () => {
    const slow = new Promise((resolve) => setTimeout(resolve, 200));
    await expect(withTimeout(slow, 20, '连接测试')).rejects.toThrow(/连接测试.*超时/);
  });

  it('marks timeout errors with err.timeout = true', async () => {
    const slow = new Promise(() => {}); // never settles
    try {
      await withTimeout(slow, 20);
    } catch (e) {
      expect(e.timeout).toBe(true);
      return;
    }
    throw new Error('should have thrown');
  });
});
