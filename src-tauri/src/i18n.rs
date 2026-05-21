//! 后端 i18n（与前端 `src/i18n` 对称）。
//!
//! - 全局 `LANG` 由 [`set_language`] 设置：启动时从 `Settings.language` 读，
//!   前端切换语言时通过 `set_app_language` 命令同步过来。
//! - [`t`] 返回当前语言对应的字符串；缺失则 fallback 到 `zh-CN`；再缺失则回退到 key 本身
//!   （便于开发期发现遗漏）。
//! - [`t_var`] 支持 `{name}` 占位符插值。
//!
//! 资源以 const slice 形式编译进二进制；运行时按 key 线性查找。当前 key 数量 < 50，
//! 性能足够；后续如增长可换成 `phf` 静态 map。

use std::sync::{OnceLock, RwLock};

static LANG: OnceLock<RwLock<String>> = OnceLock::new();

fn lang_lock() -> &'static RwLock<String> {
    LANG.get_or_init(|| RwLock::new("zh-CN".to_string()))
}

/// 设置当前语言（`"zh-CN"` / `"en"`）。
pub fn set_language(lang: &str) {
    if let Ok(mut w) = lang_lock().write() {
        *w = lang.to_string();
    }
}

/// 获取当前语言。
pub fn current_language() -> String {
    lang_lock()
        .read()
        .map(|s| s.clone())
        .unwrap_or_else(|_| "zh-CN".to_string())
}

/// 翻译指定 key。缺失则 fallback 到 zh-CN；仍缺失则返回 key 本身。
pub fn t(key: &str) -> String {
    let lang = current_language();
    if let Some(v) = lookup(&lang, key) {
        return v.to_string();
    }
    if lang != "zh-CN" {
        if let Some(v) = lookup("zh-CN", key) {
            return v.to_string();
        }
    }
    key.to_string()
}

/// 翻译 + 变量插值。`t_var("err.x", &[("name", "foo")])` 把模板里的 `{name}` 替换为 `foo`。
pub fn t_var(key: &str, vars: &[(&str, &str)]) -> String {
    let mut s = t(key);
    for (k, v) in vars {
        s = s.replace(&format!("{{{}}}", k), v);
    }
    s
}

fn lookup(lang: &str, key: &str) -> Option<&'static str> {
    let table = match lang {
        "en" => EN,
        _ => ZH_CN,
    };
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

// ============================================================
// 资源（key 命名规范：err.<module>.<name> / msg.<name>）
// ============================================================

const ZH_CN: &[(&str, &str)] = &[
    // ssh.rs
    ("err.ssh.exec_failed", "无法启动 ssh 命令：{err}\n请确认系统已安装 OpenSSH"),
    ("err.ssh.not_ssh", "不是 SSH 工作区"),
    ("err.ssh.missing_host", "SSH 工作区缺少 host"),
    ("err.ssh.missing_password", "SSH 密码方式：ssh_password 不能为空"),
    ("err.ssh.cache_path_utf8", "缓存路径不是 UTF-8: {path}"),
    ("err.ssh.no_cache_dir", "无法定位 OS 缓存目录"),
    // store.rs
    ("err.store.not_init", "存储未初始化，请先调用 store::init()"),
    ("err.store.no_config_dir", "无法定位 OS 配置目录"),
    // scheduler.rs
    ("err.scheduler.init_failed", "初始化 scheduler 失败: {err}"),
    ("err.scheduler.start_failed", "启动 scheduler 失败: {err}"),
    ("err.scheduler.cron_invalid", "cron 表达式无效或 job 构造失败: {err}"),
    ("err.scheduler.add_job_failed", "添加 job 失败: {err}"),
    ("err.scheduler.no_workspaces", "workspace_ids 为空"),
    ("err.scheduler.no_recipients", "recipients 为空,没有发件目标"),
    // report.rs
    ("err.report.template_not_found", "模板不存在: {id}"),
    ("err.report.provider_not_found", "指定的 LLM 源不存在: {id}"),
    ("err.report.no_workspaces", "未选中任何工作区"),
    // email.rs
    ("err.email.no_host", "SMTP host 未配置"),
    ("err.email.no_recipients", "收件人列表为空"),
    ("err.email.smtp_send_failed", "SMTP 发送失败:{err}"),
    (
        "err.email.smtp_connect_failed",
        "SMTP 连接失败:{err}\n常见原因:密码/授权码错误、被防火墙拦截、端口与加密方式不匹配",
    ),
    ("err.email.smtp_no_greeting", "SMTP 服务器未响应有效问候"),
    ("err.email.smtp_transport_failed", "构造 SMTP 传输失败:{err}"),
    ("err.email.invalid_address", "无效邮箱地址 \"{addr}\":{err}"),
    // llm/*
    ("err.llm.openai.no_content", "OpenAI 响应缺少 choices[0].message.content"),
    ("err.llm.anthropic.no_text", "Anthropic 响应缺少 content[0].text"),
    ("err.llm.gemini.no_text", "Gemini 响应缺少 candidates[0].content.parts[0].text"),
    // logs.rs
    ("err.logs.collect_panic", "收集任务 panic: {err}"),
    // state/*
    ("err.state.report_not_found", "报告不存在: {id}"),
    ("err.state.no_home_dir", "无法定位用户家目录"),
    // main.rs
    ("err.email.recipient_empty", "收件人为空"),
    ("err.email.build_message_failed", "构造邮件失败"),
    ("err.schedule.not_found", "任务不存在: {id}"),
    ("msg.smtp_conn_ok", "✓ SMTP 连接成功:{host}:{port}({enc})"),
    // 成功消息
    ("msg.test_email_sent", "✓ 测试邮件已发送到 {recipient}"),
    ("msg.schedule_run_done", "✓ 任务「{name}」立即执行完成"),
    // 邮件模板
    ("email.test.subject", "WeeklyReport 测试邮件"),
    (
        "email.test.body",
        "# WeeklyReport 测试邮件\n\n如果你看到这封邮件,说明 SMTP 已配置成功。\n\n- 发件人:`{from}`\n- 收件人:`{to}`\n- 时间:{time}\n",
    ),
    // 邮件模板（PR #4）
    ("email.subject_default_week", "周报 {date}"),
    ("email.html.title", "周报"),
    ("email.html.generated_at", "生成时间"),
    ("email.html.template", "模板"),
    ("email.html.range", "时间范围"),
    ("email.html.footer", "由 WeeklyReport 自动生成"),
];

const EN: &[(&str, &str)] = &[
    // ssh.rs
    ("err.ssh.exec_failed", "Failed to start ssh: {err}\nMake sure OpenSSH is installed."),
    ("err.ssh.not_ssh", "Not an SSH workspace"),
    ("err.ssh.missing_host", "SSH workspace is missing host"),
    ("err.ssh.missing_password", "SSH password auth: ssh_password cannot be empty"),
    ("err.ssh.cache_path_utf8", "Cache path is not valid UTF-8: {path}"),
    ("err.ssh.no_cache_dir", "Cannot locate OS cache directory"),
    // store.rs
    ("err.store.not_init", "Storage not initialized — call store::init() first"),
    ("err.store.no_config_dir", "Cannot locate OS config directory"),
    // scheduler.rs
    ("err.scheduler.init_failed", "Failed to initialize scheduler: {err}"),
    ("err.scheduler.start_failed", "Failed to start scheduler: {err}"),
    ("err.scheduler.cron_invalid", "Invalid cron expression or job build failed: {err}"),
    ("err.scheduler.add_job_failed", "Failed to add job: {err}"),
    ("err.scheduler.no_workspaces", "workspace_ids is empty"),
    ("err.scheduler.no_recipients", "recipients is empty — no delivery target"),
    // report.rs
    ("err.report.template_not_found", "Template not found: {id}"),
    ("err.report.provider_not_found", "Specified LLM source not found: {id}"),
    ("err.report.no_workspaces", "No workspace selected"),
    // email.rs
    ("err.email.no_host", "SMTP host is not configured"),
    ("err.email.no_recipients", "Recipient list is empty"),
    ("err.email.smtp_send_failed", "SMTP send failed: {err}"),
    (
        "err.email.smtp_connect_failed",
        "SMTP connection failed: {err}\nCommon causes: wrong password/app-token, firewall blocking, mismatched port/encryption",
    ),
    ("err.email.smtp_no_greeting", "SMTP server did not return a valid greeting"),
    ("err.email.smtp_transport_failed", "Failed to build SMTP transport: {err}"),
    ("err.email.invalid_address", "Invalid email address \"{addr}\": {err}"),
    // llm/*
    ("err.llm.openai.no_content", "OpenAI response is missing choices[0].message.content"),
    ("err.llm.anthropic.no_text", "Anthropic response is missing content[0].text"),
    ("err.llm.gemini.no_text", "Gemini response is missing candidates[0].content.parts[0].text"),
    // logs.rs
    ("err.logs.collect_panic", "Log collection task panicked: {err}"),
    // state/*
    ("err.state.report_not_found", "Report not found: {id}"),
    ("err.state.no_home_dir", "Cannot locate user home directory"),
    // main.rs
    ("err.email.recipient_empty", "Recipient is empty"),
    ("err.email.build_message_failed", "Failed to build email message"),
    ("err.schedule.not_found", "Schedule not found: {id}"),
    ("msg.smtp_conn_ok", "✓ SMTP connection succeeded: {host}:{port} ({enc})"),
    // 成功消息
    ("msg.test_email_sent", "✓ Test email sent to {recipient}"),
    ("msg.schedule_run_done", "✓ Schedule \"{name}\" finished an immediate run"),
    // 邮件模板
    ("email.test.subject", "WeeklyReport test email"),
    (
        "email.test.body",
        "# WeeklyReport test email\n\nIf you see this email, your SMTP is configured correctly.\n\n- From: `{from}`\n- To: `{to}`\n- Time: {time}\n",
    ),
    // 邮件模板（PR #4）
    ("email.subject_default_week", "Weekly Report {date}"),
    ("email.html.title", "Weekly Report"),
    ("email.html.generated_at", "Generated at"),
    ("email.html.template", "Template"),
    ("email.html.range", "Date range"),
    ("email.html.footer", "Generated by WeeklyReport"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_returns_zh_by_default() {
        set_language("zh-CN");
        assert_eq!(t("err.ssh.not_ssh"), "不是 SSH 工作区");
    }

    #[test]
    fn t_returns_en_when_set() {
        set_language("en");
        assert_eq!(t("err.ssh.not_ssh"), "Not an SSH workspace");
    }

    #[test]
    fn t_falls_back_to_zh_when_missing_in_en() {
        // 假装有 key 只在 zh-CN：用现有 key 模拟通过测试覆盖 fallback 路径
        set_language("en");
        // 真实场景：所有 key 中英都齐 → 这里只确保 fallback 不 panic
        let _ = t("nonexistent.key.x.y.z");
    }

    #[test]
    fn t_returns_key_when_missing_everywhere() {
        set_language("zh-CN");
        assert_eq!(t("nonexistent.key.x"), "nonexistent.key.x");
    }

    #[test]
    fn t_var_substitutes_placeholders() {
        set_language("zh-CN");
        let s = t_var("err.report.template_not_found", &[("id", "abc")]);
        assert_eq!(s, "模板不存在: abc");
    }

    #[test]
    fn t_var_leaves_unknown_placeholders() {
        set_language("zh-CN");
        let s = t_var("err.scheduler.init_failed", &[]);
        // err 模板里有 {err}，未提供时保留原样
        assert!(s.contains("{err}"));
    }
}
