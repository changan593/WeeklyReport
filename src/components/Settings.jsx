// 设置页（详见 docs/UI.md#46-设置页-settingsjsx）。
//
// Section 1 SMTP：预设按钮 + 表单 + 测试连接 / 发测试邮件 / 保存
// Section 2 数据存储：显示数据目录路径，提醒用户可备份 / 迁移

import { useEffect, useState } from 'react';
import {
  dataDirPath,
  getSmtpConfig,
  sendTestEmail,
  saveSmtpConfig,
  testSmtpConfig,
  formatError,
} from '../api.js';
import {
  FormField,
  Icon,
  Input,
  LoadingState,
  PrimaryButton,
  SecondaryButton,
  StatusBanner,
} from './ui.jsx';

const SMTP_PRESETS = [
  {
    label: 'Gmail',
    host: 'smtp.gmail.com',
    port: 465,
    use_ssl: true,
    hint: 'Gmail 需要开启两步验证并使用「应用专用密码」（App Password），不能用账户密码。',
  },
  {
    label: 'Outlook 365',
    host: 'smtp.office365.com',
    port: 587,
    use_ssl: false,
    hint: 'Microsoft 账号需在管理后台开启 SMTP AUTH，部分企业账号默认禁用。',
  },
  {
    label: 'QQ 邮箱',
    host: 'smtp.qq.com',
    port: 465,
    use_ssl: true,
    hint: '密码字段填「授权码」（在 QQ 邮箱设置 → 账户里生成），不是登录密码。',
  },
  {
    label: '163 邮箱',
    host: 'smtp.163.com',
    port: 465,
    use_ssl: true,
    hint: '密码字段填「客户端授权密码」（在网易邮箱设置里生成），不是登录密码。',
  },
  {
    label: '企业微信邮箱',
    host: 'smtp.exmail.qq.com',
    port: 465,
    use_ssl: true,
    hint: '使用企业邮箱的登录密码或专用授权码（部分企业要求开启 IMAP/SMTP 服务）。',
  },
];

const EMPTY = {
  host: '',
  port: 465,
  username: '',
  password: '',
  from_name: '',
  use_ssl: true,
};

export default function Settings() {
  const [cfg, setCfg] = useState(EMPTY);
  const [loaded, setLoaded] = useState(false);
  const [presetHint, setPresetHint] = useState(null);
  const [status, setStatus] = useState(null);
  const [busy, setBusy] = useState(null); // null | 'test' | 'send' | 'save'
  const [testEmail, setTestEmail] = useState('');
  const [dataDir, setDataDir] = useState('');

  useEffect(() => {
    getSmtpConfig()
      .then((c) => {
        if (c) setCfg({ ...EMPTY, ...c });
      })
      .catch(() => {})
      .finally(() => setLoaded(true));
    dataDirPath().then(setDataDir).catch(() => setDataDir(''));
  }, []);

  function set(field, value) {
    setCfg((prev) => ({ ...prev, [field]: value }));
  }

  function applyPreset(preset) {
    setCfg((prev) => ({
      ...prev,
      host: preset.host,
      port: preset.port,
      use_ssl: preset.use_ssl,
    }));
    setPresetHint(preset.hint);
    setStatus(null);
  }

  function normalize() {
    return {
      ...cfg,
      port: Number(cfg.port) || (cfg.use_ssl ? 465 : 587),
    };
  }

  async function handleTest() {
    setBusy('test');
    setStatus({ type: 'info', msg: '正在测试 SMTP 连接…' });
    try {
      const msg = await testSmtpConfig(normalize());
      setStatus({ type: 'success', msg });
    } catch (e) {
      setStatus({ type: 'error', msg: formatError(e) });
    } finally {
      setBusy(null);
    }
  }

  async function handleSendTest() {
    if (!testEmail.trim()) {
      setStatus({ type: 'error', msg: '请填收件人邮箱' });
      return;
    }
    setBusy('send');
    setStatus({ type: 'info', msg: '正在发送测试邮件…' });
    try {
      const msg = await sendTestEmail(normalize(), testEmail.trim());
      setStatus({ type: 'success', msg });
    } catch (e) {
      setStatus({ type: 'error', msg: formatError(e) });
    } finally {
      setBusy(null);
    }
  }

  async function handleSave() {
    setBusy('save');
    try {
      await saveSmtpConfig(normalize());
      setStatus({ type: 'success', msg: '已保存 SMTP 配置' });
      setTimeout(() => setStatus(null), 2500);
    } catch (e) {
      setStatus({ type: 'error', msg: formatError(e) });
    } finally {
      setBusy(null);
    }
  }

  if (!loaded) return <LoadingState />;

  return (
    <div>
      <header className="mb-6">
        <h1 className="text-2xl font-medium text-stone-900">设置</h1>
        <p className="mt-1 text-[13px] text-stone-500">
          配置 SMTP（用于定时任务发送邮件）；查看数据目录
        </p>
      </header>

      <Section title="SMTP 邮箱配置">
        {/* 预设按钮 */}
        <div className="mb-3">
          <div className="mb-1.5 text-[11.5px] font-medium text-stone-600">快速预设</div>
          <div className="flex flex-wrap gap-1.5">
            {SMTP_PRESETS.map((p, i) => (
              <button
                key={i}
                type="button"
                onClick={() => applyPreset(p)}
                className="rounded border border-stone-200 bg-white px-2.5 py-1 text-[11.5px] text-stone-600 hover:border-stone-300 hover:text-stone-900"
              >
                {p.label}
              </button>
            ))}
          </div>
          {presetHint && (
            <div className="mt-2 rounded border border-amber-200 bg-amber-50 px-3 py-2 text-[12px] text-amber-700">
              {presetHint}
            </div>
          )}
        </div>

        <div className="grid grid-cols-2 gap-3">
          <FormField label="SMTP Host">
            <Input value={cfg.host} onChange={(v) => set('host', v)} placeholder="smtp.example.com" />
          </FormField>
          <FormField label="Port">
            <Input
              value={String(cfg.port)}
              onChange={(v) => set('port', v)}
              placeholder={cfg.use_ssl ? '465' : '587'}
            />
          </FormField>
        </div>

        <FormField label="加密方式" className="mt-3">
          <div className="inline-flex rounded-md border border-stone-200 p-0.5">
            {[
              { v: false, label: 'STARTTLS' },
              { v: true, label: 'SSL / TLS' },
            ].map((opt) => (
              <button
                key={String(opt.v)}
                type="button"
                onClick={() => set('use_ssl', opt.v)}
                className={`rounded px-3 py-1 text-[12.5px] ${
                  cfg.use_ssl === opt.v
                    ? 'bg-stone-900 text-white'
                    : 'text-stone-600 hover:text-stone-900'
                }`}
              >
                {opt.label}
              </button>
            ))}
          </div>
        </FormField>

        <div className="mt-3 grid grid-cols-2 gap-3">
          <FormField label="用户名（邮箱）">
            <Input
              value={cfg.username}
              onChange={(v) => set('username', v)}
              placeholder="you@example.com"
            />
          </FormField>
          <FormField label="密码 / 授权码">
            <Input
              type="password"
              value={cfg.password}
              onChange={(v) => set('password', v)}
            />
          </FormField>
        </div>

        <FormField label="发件人显示名" className="mt-3">
          <Input
            value={cfg.from_name}
            onChange={(v) => set('from_name', v)}
            placeholder="（可选）WeeklyReport 周报助手"
          />
        </FormField>

        <div className="mt-3">
          <StatusBanner status={status} />
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Input
            value={testEmail}
            onChange={setTestEmail}
            placeholder="测试收件邮箱（可与用户名相同）"
            className="flex-1 min-w-[200px]"
          />
          <SecondaryButton onClick={handleSendTest} disabled={busy !== null}>
            <Icon name="mail" size={14} />
            {busy === 'send' ? '发送中…' : '发测试邮件'}
          </SecondaryButton>
          <SecondaryButton onClick={handleTest} disabled={busy !== null}>
            <Icon name="refresh" size={14} />
            {busy === 'test' ? '测试中…' : '测连接'}
          </SecondaryButton>
          <PrimaryButton onClick={handleSave} disabled={busy !== null}>
            {busy === 'save' ? '保存中…' : '保存'}
          </PrimaryButton>
        </div>
      </Section>

      <Section title="数据存储">
        <p className="text-[12.5px] text-stone-600">
          所有配置以 JSON 文件形式存放在以下目录；可直接备份或迁移。
        </p>
        <div className="mt-2 break-all rounded bg-stone-50 px-3 py-2 font-mono text-[12px] text-stone-700">
          {dataDir || '—'}
        </div>
      </Section>
    </div>
  );
}

function Section({ title, children }) {
  return (
    <section className="mb-6">
      <h2 className="mb-2 text-[11.5px] font-medium uppercase tracking-wider text-stone-500">
        {title}
      </h2>
      <div className="rounded-lg border border-stone-200 bg-white p-5">{children}</div>
    </section>
  );
}
