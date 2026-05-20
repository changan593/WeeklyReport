//! 输入校验：所有从 IPC 进入后端、最终参与到文件路径 / shell 命令 /
//! 邮件头的字段都应在边界处通过本模块校验。
//!
//! 设计原则：
//! - 仅白名单：禁止"试图猜哪些字符危险然后 escape"
//! - 在边界处校验一次，传到内部就是干净数据
//! - 失败时返回中文错误消息，便于前端直接展示

use anyhow::{anyhow, Result};

/// 校验持久化对象的 ID。
///
/// 允许：ASCII 字母数字、`-`、`_`，长度 1..=64。
/// 拒绝：任何路径分隔符、`.`、`..`、URL 编码、控制字符。
///
/// 用于 `delete_report` / `get_report` 等接受 ID 的 IPC handler，
/// 也用于 `save_report_file` 拼接 `<id>.md` 前的兜底校验。
pub fn id(s: &str) -> Result<()> {
    if s.is_empty() {
        return Err(anyhow!("ID 不能为空"));
    }
    if s.len() > 64 {
        return Err(anyhow!("ID 过长（最多 64 字符）"));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow!("ID 含非法字符（只允许字母数字、-、_）"));
    }
    Ok(())
}

/// 校验 SSH host：DNS 主机名或 IPv4/IPv6 字面量。
///
/// 关键防御：
/// - 不允许以 `-` 开头（OpenSSH 历史漏洞：被当成命令行选项注入）
/// - 不允许空格、`@`、`/`、shell 元字符
///
/// 允许的形态：
/// - `example.com`
/// - `192.168.1.1`
/// - `2001:db8::1`（带方括号也接受 `[2001:db8::1]`）
/// - `gpu01.internal`、`server-1`
pub fn ssh_host(s: &str) -> Result<()> {
    if s.is_empty() {
        return Err(anyhow!("Host 不能为空"));
    }
    if s.len() > 253 {
        return Err(anyhow!("Host 过长"));
    }
    if s.starts_with('-') {
        return Err(anyhow!("Host 不能以 '-' 开头（可能被解释为命令行选项）"));
    }
    // 允许：字母数字、`-`、`.`、`:`（IPv6）、`[]`（IPv6 包裹）
    if !s.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == ':' || c == '[' || c == ']'
    }) {
        return Err(anyhow!(
            "Host 含非法字符（只允许字母数字、-、.、IPv6 的 : 和 []）"
        ));
    }
    Ok(())
}

/// 校验 SSH 用户名：POSIX 用户名 + `-` + `_` + `.`，长度 1..=32。
///
/// 同样禁止以 `-` 开头，防止被 ssh / rsync 当作命令行选项。
pub fn ssh_user(s: &str) -> Result<()> {
    if s.is_empty() {
        return Err(anyhow!("User 不能为空"));
    }
    if s.len() > 32 {
        return Err(anyhow!("User 过长（最多 32 字符）"));
    }
    if s.starts_with('-') {
        return Err(anyhow!("User 不能以 '-' 开头"));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(anyhow!("User 含非法字符（只允许字母数字、-、_、.）"));
    }
    Ok(())
}

/// 校验邮件地址（保守正则）。失败返回中文错误。
///
/// 实际 SMTP 发送时由 `lettre` 再校验一次；这里只做边界拦截，
/// 防止前端把 `\r\n` 等头注入字符攒进 recipients。
pub fn email(s: &str) -> Result<()> {
    if s.is_empty() {
        return Err(anyhow!("邮箱不能为空"));
    }
    if s.len() > 254 {
        return Err(anyhow!("邮箱过长"));
    }
    if s.chars().any(|c| c.is_control()) {
        return Err(anyhow!("邮箱含控制字符（防 CRLF 注入）"));
    }
    // 至少含一个 @，且 @ 两侧非空
    let at = s.find('@').ok_or_else(|| anyhow!("邮箱缺少 @"))?;
    let (local, domain) = (&s[..at], &s[at + 1..]);
    if local.is_empty() || domain.is_empty() {
        return Err(anyhow!("邮箱 @ 两侧不能为空"));
    }
    if !domain.contains('.') {
        return Err(anyhow!("邮箱域名格式不正确"));
    }
    Ok(())
}

/// 校验"邮件头单行文本"（subject / from_name 等）：禁止 CR/LF。
///
/// CRLF 注入可能让攻击者改写邮件头（如插入 BCC）。lettre 默认会拒绝带
/// CRLF 的字段，但我们在边界处再拦一道更稳妥。
pub fn mail_header_text(s: &str) -> Result<()> {
    if s.contains('\r') || s.contains('\n') {
        return Err(anyhow!("文本不能含换行（防止邮件头注入）"));
    }
    if s.len() > 998 {
        // RFC 5322 line length
        return Err(anyhow!("文本过长"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------- id --------

    #[test]
    fn id_accepts_uuid() {
        assert!(id("550e8400-e29b-41d4-a716-446655440000").is_ok());
    }

    #[test]
    fn id_accepts_underscore_and_digits() {
        assert!(id("builtin-tech").is_ok());
        assert!(id("custom_123").is_ok());
        assert!(id("a").is_ok());
    }

    #[test]
    fn id_rejects_path_traversal() {
        assert!(id("../etc/passwd").is_err());
        assert!(id("..").is_err());
        assert!(id("foo/bar").is_err());
        assert!(id("foo\\bar").is_err());
        assert!(id("foo.bar").is_err(), "不允许 '.'，避免 .md.md 等");
    }

    #[test]
    fn id_rejects_empty_and_too_long() {
        assert!(id("").is_err());
        let long = "a".repeat(65);
        assert!(id(&long).is_err());
    }

    #[test]
    fn id_rejects_url_encoded_and_unicode() {
        assert!(id("%2e%2e").is_err());
        assert!(id("中文ID").is_err());
        assert!(id("foo bar").is_err());
    }

    // -------- ssh_host --------

    #[test]
    fn ssh_host_accepts_typical() {
        assert!(ssh_host("example.com").is_ok());
        assert!(ssh_host("gpu01.internal").is_ok());
        assert!(ssh_host("192.168.1.1").is_ok());
        assert!(ssh_host("2001:db8::1").is_ok());
        assert!(ssh_host("[2001:db8::1]").is_ok());
    }

    #[test]
    fn ssh_host_rejects_dash_prefix() {
        // 历史经典攻击：被 OpenSSH / rsync 当成命令行选项
        assert!(ssh_host("-oProxyCommand=evil").is_err());
        assert!(ssh_host("-J jump").is_err());
    }

    #[test]
    fn ssh_host_rejects_shell_metachars() {
        assert!(ssh_host("a;b").is_err());
        assert!(ssh_host("a|b").is_err());
        assert!(ssh_host("a b").is_err());
        assert!(ssh_host("a$b").is_err());
        assert!(ssh_host("a@b").is_err()); // user 应该在 user 字段
    }

    // -------- ssh_user --------

    #[test]
    fn ssh_user_accepts_normal() {
        assert!(ssh_user("alice").is_ok());
        assert!(ssh_user("ubuntu").is_ok());
        assert!(ssh_user("ci.bot").is_ok());
        assert!(ssh_user("user_1").is_ok());
    }

    #[test]
    fn ssh_user_rejects_dash_prefix_and_meta() {
        assert!(ssh_user("-oProxy").is_err());
        assert!(ssh_user("a;b").is_err());
        assert!(ssh_user("a b").is_err());
    }

    // -------- email --------

    #[test]
    fn email_basic() {
        assert!(email("user@example.com").is_ok());
        assert!(email("a.b+c@sub.domain.com").is_ok());
    }

    #[test]
    fn email_rejects_crlf() {
        assert!(email("a@b.com\r\nBcc: evil@x.com").is_err());
        assert!(email("a@b.com\nattack").is_err());
    }

    #[test]
    fn email_rejects_no_at_or_no_domain_dot() {
        assert!(email("no-at-here").is_err());
        assert!(email("@b.com").is_err());
        assert!(email("a@").is_err());
        assert!(email("a@b").is_err()); // 域名要求至少一个 .
    }

    // -------- mail_header_text --------

    #[test]
    fn mail_header_accepts_plain() {
        assert!(mail_header_text("周报 2026-05-20").is_ok());
        assert!(mail_header_text("WeeklyReport 周报助手").is_ok());
    }

    #[test]
    fn mail_header_rejects_crlf() {
        assert!(mail_header_text("Subject\r\nBcc: evil@x.com").is_err());
        assert!(mail_header_text("multi\nline").is_err());
    }
}
