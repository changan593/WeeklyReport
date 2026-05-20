//! SMTP 单例配置：get / save（敏感字段，写入时 chmod 0600）。

use anyhow::Result;

use crate::email::SmtpConfig;
use crate::store;

use super::F_SMTP;

pub fn get_smtp_config() -> Result<SmtpConfig> {
    store::read_json(F_SMTP)
}

pub fn save_smtp_config(cfg: &SmtpConfig) -> Result<()> {
    store::write_json_secret(F_SMTP, cfg)
}
