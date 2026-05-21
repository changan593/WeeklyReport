// 设置页（详见 docs/UI.md#46-设置页-settingsjsx）。
//
// Section 1 语言：切换 UI 中英
// Section 2 SMTP：预设按钮 + 表单 + 测试连接 / 发测试邮件 / 保存
// Section 3 数据存储：显示数据目录路径，提醒用户可备份 / 迁移

import { useEffect, useState } from 'react';
import {
  dataDirPath,
  getSettings,
  getSmtpConfig,
  saveSettings,
  saveSmtpConfig,
  sendTestEmail,
  setAppLanguage,
  testSmtpConfig,
  formatError,
} from '../api.js';
import { SUPPORTED_LANGS, useTranslation } from '../i18n/index.jsx';
import {
  FormField,
  Icon,
  Input,
  LoadingState,
  PrimaryButton,
  SecondaryButton,
  StatusBanner,
} from './ui.jsx';

const LANG_OPTIONS = [
  { value: 'zh-CN', i18nKey: 'settings.language.zh_cn' },
  { value: 'en', i18nKey: 'settings.language.en' },
];

// SMTP 预设：label 是品牌名（不翻译），hint 走 i18n key 运行时取
const SMTP_PRESETS = [
  { label: 'Gmail',          host: 'smtp.gmail.com',     port: 465, use_ssl: true,  hintKey: 'settings.smtp.preset.gmail.hint' },
  { label: 'Outlook 365',    host: 'smtp.office365.com', port: 587, use_ssl: false, hintKey: 'settings.smtp.preset.outlook.hint' },
  { label: 'QQ 邮箱',         host: 'smtp.qq.com',        port: 465, use_ssl: true,  hintKey: 'settings.smtp.preset.qq.hint' },
  { label: '163 邮箱',        host: 'smtp.163.com',       port: 465, use_ssl: true,  hintKey: 'settings.smtp.preset.n163.hint' },
  { label: '企业微信邮箱',     host: 'smtp.exmail.qq.com', port: 465, use_ssl: true,  hintKey: 'settings.smtp.preset.wecom.hint' },
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
  const { t, lang, setLang } = useTranslation();
  const [cfg, setCfg] = useState(EMPTY);
  const [loaded, setLoaded] = useState(false);
  const [presetHint, setPresetHint] = useState(null);
  const [status, setStatus] = useState(null);
  const [busy, setBusy] = useState(null); // null | 'test' | 'send' | 'save'
  const [testEmail, setTestEmail] = useState('');
  const [dataDir, setDataDir] = useState('');
  // 存当前后端 Settings 全量值，切换语言时回写要保留其余字段
  const [settings, setSettings] = useState(null);

  useEffect(() => {
    getSmtpConfig()
      .then((c) => {
        if (c) setCfg({ ...EMPTY, ...c });
      })
      .catch(() => {})
      .finally(() => setLoaded(true));
    dataDirPath().then(setDataDir).catch(() => setDataDir(''));
    getSettings().then(setSettings).catch(() => setSettings(null));
  }, []);

  async function handleChangeLang(next) {
    if (next === lang) return;
    setLang(next); // 立即生效 UI
    // 立刻通知后端 i18n（让 anyhow! 错误和邮件模板马上跟上），失败不阻塞 UI
    try {
      await setAppLanguage(next);
    } catch {
      /* 后端没响应不影响 UI，重启后会被启动同步矫正 */
    }
    // 持久化到后端 Settings.language
    try {
      const merged = { ...(settings ?? {}), language: next };
      await saveSettings(merged);
      setSettings(merged);
    } catch (e) {
      setStatus({ type: 'error', msg: formatError(e) });
    }
  }

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
    setPresetHint(t(preset.hintKey));
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
    setStatus({ type: 'info', msg: t('settings.smtp.status.test_running') });
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
      setStatus({ type: 'error', msg: t('settings.smtp.status.recipient_required') });
      return;
    }
    setBusy('send');
    setStatus({ type: 'info', msg: t('settings.smtp.status.sending') });
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
      setStatus({ type: 'success', msg: t('settings.smtp.status.saved') });
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
        <h1 className="text-2xl font-medium text-stone-900">{t('settings.title')}</h1>
        <p className="mt-1 text-[13px] text-stone-500">{t('settings.subtitle')}</p>
      </header>

      <Section title={t('settings.sections.language')}>
        <div className="inline-flex rounded-md border border-stone-200 p-0.5">
          {LANG_OPTIONS.filter((o) => SUPPORTED_LANGS.includes(o.value)).map((opt) => (
            <button
              key={opt.value}
              type="button"
              onClick={() => handleChangeLang(opt.value)}
              className={`rounded px-3 py-1 text-[12.5px] ${
                lang === opt.value
                  ? 'bg-stone-900 text-white'
                  : 'text-stone-600 hover:text-stone-900'
              }`}
            >
              {t(opt.i18nKey)}
            </button>
          ))}
        </div>
        <p className="mt-2 text-[12px] text-stone-500">{t('settings.language.hint')}</p>
      </Section>

      <Section title={t('settings.sections.smtp')}>
        {/* 预设按钮 */}
        <div className="mb-3">
          <div className="mb-1.5 text-[11.5px] font-medium text-stone-600">
            {t('settings.smtp.preset_label')}
          </div>
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
          <FormField label={t('settings.smtp.host')}>
            <Input value={cfg.host} onChange={(v) => set('host', v)} placeholder="smtp.example.com" />
          </FormField>
          <FormField label={t('settings.smtp.port')}>
            <Input
              value={String(cfg.port)}
              onChange={(v) => set('port', v)}
              placeholder={cfg.use_ssl ? '465' : '587'}
            />
          </FormField>
        </div>

        <FormField label={t('settings.smtp.encryption')} className="mt-3">
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
          <FormField label={t('settings.smtp.username')}>
            <Input
              value={cfg.username}
              onChange={(v) => set('username', v)}
              placeholder="you@example.com"
            />
          </FormField>
          <FormField label={t('settings.smtp.password')}>
            <Input
              type="password"
              value={cfg.password}
              onChange={(v) => set('password', v)}
            />
          </FormField>
        </div>

        <FormField label={t('settings.smtp.from_name')} className="mt-3">
          <Input
            value={cfg.from_name}
            onChange={(v) => set('from_name', v)}
            placeholder={t('settings.smtp.from_name_placeholder')}
          />
        </FormField>

        <div className="mt-3">
          <StatusBanner status={status} />
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Input
            value={testEmail}
            onChange={setTestEmail}
            placeholder={t('settings.smtp.test_email_placeholder')}
            className="flex-1 min-w-[200px]"
          />
          <SecondaryButton onClick={handleSendTest} disabled={busy !== null}>
            <Icon name="mail" size={14} />
            {busy === 'send' ? t('settings.smtp.sending') : t('settings.smtp.send_test')}
          </SecondaryButton>
          <SecondaryButton onClick={handleTest} disabled={busy !== null}>
            <Icon name="refresh" size={14} />
            {busy === 'test' ? t('settings.smtp.testing') : t('settings.smtp.test_conn')}
          </SecondaryButton>
          <PrimaryButton onClick={handleSave} disabled={busy !== null}>
            {busy === 'save' ? t('common.saving') : t('common.save')}
          </PrimaryButton>
        </div>
      </Section>

      <Section title={t('settings.sections.data')}>
        <p className="text-[12.5px] text-stone-600">{t('settings.data.desc')}</p>
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
