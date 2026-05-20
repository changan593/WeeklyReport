//! SMTP 单例配置：get / save（敏感字段，写入时 chmod 0600）。

use anyhow::{bail, Result};

use crate::email::SmtpConfig;
use crate::store;
use crate::validate;

use super::F_SMTP;

pub fn get_smtp_config() -> Result<SmtpConfig> {
    store::read_json(F_SMTP)
}

/// 保存 SMTP。空 host 视为"暂存未配置"——允许写入空白配置，
/// 但任何非空字段都要通过严格校验。
pub fn save_smtp_config(cfg: &SmtpConfig) -> Result<()> {
    // 完全空白的"未配置"状态允许保存（用户可能想清空）
    let all_empty = cfg.host.is_empty()
        && cfg.username.is_empty()
        && cfg.password.is_empty()
        && cfg.from_name.is_empty();
    if !all_empty {
        if cfg.host.chars().any(|c| c.is_control()) {
            bail!("SMTP host 含控制字符");
        }
        if cfg.host.len() > 253 {
            bail!("SMTP host 过长");
        }
        if !cfg.username.is_empty() {
            validate::email(&cfg.username)?;
        }
        // 密码：禁止 CRLF（避免被注入到 SMTP AUTH 行）
        if cfg.password.contains('\r') || cfg.password.contains('\n') {
            bail!("SMTP 密码不能含换行");
        }
        if !cfg.from_name.is_empty() {
            validate::mail_header_text(&cfg.from_name)?;
        }
    }
    store::write_json_secret(F_SMTP, cfg)
}
