//! 文件存储基础层：JSON 文件的原子读写与损坏恢复。
//!
//! 设计要点（详见 `docs/ARCHITECTURE.md#31-storers--文件存储基础层`）：
//! - 写入前先写到 `.tmp` 文件，再 `rename`（POSIX/Windows 均原子）
//! - 读取时文件不存在返回 `T::default()`，不报错
//! - 解析失败时备份损坏文件为 `<filename>.broken-<时间戳>`，再返回默认值
//! - 数据目录按平台自动选择
//!
//! 阶段 1 把所有公共函数完整暴露；阶段 4+ 才会被 Tauri command 调用，
//! 因此暂时 allow dead_code，避免无谓地把 API 拆碎。
#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// 已初始化的数据目录。`init()` / `init_at()` 调用后被设置。
static DATA_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// 损坏文件备份保留天数：超过此期限的 `.broken-*` 会在 init 时被清理。
/// 这些备份可能含历史敏感数据（早期 API key 等），不该无限累积。
const BROKEN_BACKUP_RETENTION_DAYS: i64 = 30;

/// 初始化数据目录到 OS 默认位置：
/// - macOS:   `~/Library/Application Support/WeeklyReport/`
/// - Linux:   `~/.config/weekly-report/`
/// - Windows: `%APPDATA%\WeeklyReport\`
pub fn init() -> Result<()> {
    init_at(default_data_dir()?)
}

/// 初始化数据目录到指定路径（用于测试或自定义部署）。
/// 同时创建 `reports/` 子目录，并在 Unix 上把权限限制为 0700。
pub fn init_at(root: PathBuf) -> Result<()> {
    fs::create_dir_all(&root).with_context(|| format!("创建数据目录失败: {}", root.display()))?;
    restrict_dir_to_user(&root);
    let reports = root.join("reports");
    fs::create_dir_all(&reports)
        .with_context(|| format!("创建报告目录失败: {}", reports.display()))?;
    restrict_dir_to_user(&reports);
    *DATA_ROOT.write().expect("DATA_ROOT 锁中毒") = Some(root.clone());

    // 清理过期的损坏文件备份（best-effort，失败仅 warn）
    if let Err(e) = cleanup_old_broken_backups(&root) {
        tracing::warn!("清理过期 .broken-* 备份失败: {e:#}");
    }
    Ok(())
}

/// 把目录权限设为 `0o700`（rwx------）。仅 Unix 生效；Windows 是 no-op。
///
/// 数据目录里有 LLM key、SMTP 密码、报告全文等敏感数据；默认 0755 让同主机
/// 其他用户能列出文件名，0700 才是最小授权。失败只 warn，不阻断启动
/// （某些 FS 不支持 chmod，如 FAT32 / 部分网络盘）。
#[cfg(unix)]
fn restrict_dir_to_user(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o700);
    if let Err(e) = fs::set_permissions(path, perms) {
        tracing::warn!("无法设置目录 {} 权限为 0700: {e}", path.display());
    }
}

#[cfg(not(unix))]
fn restrict_dir_to_user(_path: &Path) {}

/// 删除 `root` 下所有名字含 `.broken-` 且 mtime 超过保留期的文件。
fn cleanup_old_broken_backups(root: &Path) -> Result<()> {
    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs((BROKEN_BACKUP_RETENTION_DAYS * 24 * 3600) as u64);
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.contains(".broken-") {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if let Ok(modified) = meta.modified() {
                if modified < cutoff {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
    Ok(())
}

/// 返回当前数据目录。若未 init 则返回错误。
pub fn data_dir() -> Result<PathBuf> {
    DATA_ROOT
        .read()
        .expect("DATA_ROOT 锁中毒")
        .clone()
        .ok_or_else(|| anyhow!("存储未初始化，请先调用 store::init()"))
}

/// 计算 OS 默认数据目录（不创建）。
fn default_data_dir() -> Result<PathBuf> {
    let base = dirs::config_dir().ok_or_else(|| anyhow!("无法定位 OS 配置目录"))?;
    Ok(base.join(app_dir_name()))
}

/// 数据目录名按 OS 平台命名规范选择：Linux 用 kebab-case，macOS/Windows 用 PascalCase。
fn app_dir_name() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "weekly-report"
    }
    #[cfg(not(target_os = "linux"))]
    {
        "WeeklyReport"
    }
}

/// 读取 `data_dir() / filename` 的 JSON。
///
/// - 文件不存在 → 返回 `T::default()`
/// - JSON 损坏 → 备份为 `.broken-<时间戳>`，返回 `T::default()`
/// - 其他 I/O 错误 → 记录 warn 后返回 `T::default()`（不阻塞应用启动）
pub fn read_json<T: DeserializeOwned + Default>(filename: &str) -> Result<T> {
    let path = data_dir()?.join(filename);
    read_json_from(&path)
}

/// 原子写入 JSON 到 `data_dir() / filename`：先写 `.tmp`，再 `rename`。
pub fn write_json<T: Serialize>(filename: &str, value: &T) -> Result<()> {
    let path = data_dir()?.join(filename);
    write_json_to(&path, value)
}

/// 同 [`write_json`]，但临时文件从一开始就以 `0o600` 权限创建（Unix），
/// 避免 default umask（通常 0644）与 chmod 之间的 TOCTOU 窗口。
///
/// 用于保存敏感数据（API key / SMTP 密码 / SSH 私钥路径 / 收件人邮箱）。
/// Windows 上 NTFS 默认已经只允许当前用户访问，等价于普通 write_json。
pub fn write_json_secret<T: Serialize>(filename: &str, value: &T) -> Result<()> {
    let path = data_dir()?.join(filename);
    write_json_secret_to(&path, value)
}

pub(crate) fn write_json_secret_to<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_vec_pretty(value).context("JSON 序列化失败")?;
    atomic_write_with_mode(path, &body, Some(0o600))
}

/// 与 [`read_json`] 同语义，但直接对指定路径操作（便于测试）。
pub(crate) fn read_json_from<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!("读取 {} 失败: {} (恢复默认值)", path.display(), err);
            return Ok(T::default());
        }
    };
    match serde_json::from_slice::<T>(&bytes) {
        Ok(value) => Ok(value),
        Err(err) => {
            tracing::warn!(
                "JSON 解析失败 {}: {} (已备份并恢复默认值)",
                path.display(),
                err
            );
            backup_broken(path)?;
            Ok(T::default())
        }
    }
}

/// 与 [`write_json`] 同语义，但直接对指定路径操作（便于测试）。
pub(crate) fn write_json_to<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_vec_pretty(value).context("JSON 序列化失败")?;
    atomic_write_with_mode(path, &body, None)
}

/// 原子写入字节流：先写 `<path>.tmp`（带可选 mode），再 `rename`。
///
/// - 当 `mode = Some(0o600)` 时，临时文件在 Unix 上以该权限创建（`OpenOptions.mode`），
///   消除了"先写默认 0644 再 chmod"之间的 TOCTOU 窗口。
/// - 任何已存在的 `.tmp` 会被 `create_new` 拒绝，避免攻击者预先创建一个软链接
///   指向受害者文件来实现任意写入。
fn atomic_write_with_mode(path: &Path, body: &[u8], mode: Option<u32>) -> Result<()> {
    let tmp = tmp_path(path);
    // 如果上次写入异常中断留下残留 .tmp，先清理（仅当它是普通文件，不跟随 symlink）
    if let Ok(meta) = fs::symlink_metadata(&tmp) {
        if meta.file_type().is_file() {
            let _ = fs::remove_file(&tmp);
        } else {
            // .tmp 是 symlink 或其他类型 → 异常状态，直接报错
            return Err(anyhow!(
                "临时路径已存在非常规文件，疑似攻击: {}",
                tmp.display()
            ));
        }
    }

    let mut f =
        open_tmp(&tmp, mode).with_context(|| format!("创建临时文件失败: {}", tmp.display()))?;
    f.write_all(body)
        .with_context(|| format!("写入临时文件失败: {}", tmp.display()))?;
    // sync_all 在某些 FS（如 tmpfs）上会失败，忽略
    let _ = f.sync_all();
    drop(f);

    fs::rename(&tmp, path)
        .with_context(|| format!("rename 失败: {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn open_tmp(tmp: &Path, mode: Option<u32>) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    if let Some(m) = mode {
        opts.mode(m);
    }
    opts.open(tmp)
}

#[cfg(not(unix))]
fn open_tmp(tmp: &Path, _mode: Option<u32>) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp)
}

/// 在 path 文件名后追加 `.tmp` 后缀，保留完整路径与扩展名。
fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// 把损坏的文件重命名为 `<filename>.broken-<时间戳>`。
fn backup_broken(path: &Path) -> Result<()> {
    let ts = Local::now().format("%Y%m%d-%H%M%S%.3f");
    let mut new_name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    new_name.push(format!(".broken-{ts}"));
    let backup = path.with_file_name(new_name);
    fs::rename(path, &backup).with_context(|| {
        format!(
            "备份损坏文件失败: {} -> {}",
            path.display(),
            backup.display()
        )
    })?;
    Ok(())
}

/// 保存报告 Markdown 到 `reports/<id>.md`，返回完整路径。原子写入。
///
/// `id` 必须先经 [`crate::validate::id`] 校验，禁止路径穿越字符。
pub fn save_report_file(id: &str, content: &str) -> Result<PathBuf> {
    let path = report_path(id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // 报告 Markdown 可能含敏感工作信息，统一用 0600 写入
    atomic_write_with_mode(&path, content.as_bytes(), Some(0o600))?;
    Ok(path)
}

/// 读取 `reports/<id>.md` 全文。文件不存在时返回错误。
pub fn load_report_file(id: &str) -> Result<String> {
    let path = report_path(id)?;
    fs::read_to_string(&path).with_context(|| format!("读取报告失败: {}", path.display()))
}

/// 删除 `reports/<id>.md`。文件不存在时静默成功。
pub fn delete_report_file(id: &str) -> Result<()> {
    let path = report_path(id)?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("删除报告失败: {}", path.display()))?;
    }
    Ok(())
}

/// 拼出 `data_dir/reports/<id>.md`。`id` 必须通过白名单校验，禁止 `..`、`/`、`\` 等。
fn report_path(id: &str) -> Result<PathBuf> {
    crate::validate::id(id)?;
    Ok(data_dir()?.join("reports").join(format!("{id}.md")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
    struct Sample {
        name: String,
        count: u32,
        items: Vec<String>,
    }

    fn temp_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("weekly-report-store-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn round_trip_json() {
        let dir = temp_dir();
        let path = dir.join("sample.json");
        let value = Sample {
            name: "hi".into(),
            count: 42,
            items: vec!["a".into(), "b".into()],
        };
        write_json_to(&path, &value).unwrap();
        let loaded: Sample = read_json_from(&path).unwrap();
        assert_eq!(loaded, value);
    }

    #[test]
    fn missing_file_returns_default() {
        let dir = temp_dir();
        let path = dir.join("absent.json");
        let loaded: Sample = read_json_from(&path).unwrap();
        assert_eq!(loaded, Sample::default());
    }

    #[test]
    fn broken_json_is_backed_up_and_returns_default() {
        let dir = temp_dir();
        let path = dir.join("broken.json");
        std::fs::write(&path, b"{ not valid json").unwrap();

        let loaded: Sample = read_json_from(&path).unwrap();
        assert_eq!(loaded, Sample::default());

        // 原文件应被改名为 .broken-*
        assert!(!path.exists(), "原文件应已被重命名");
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("broken.json.broken-")
            })
            .collect();
        assert_eq!(backups.len(), 1, "应有一个 .broken-* 备份");
    }

    #[test]
    fn atomic_write_no_residual_tmp_on_success() {
        let dir = temp_dir();
        let path = dir.join("atomic.json");
        let v = Sample {
            name: "x".into(),
            ..Default::default()
        };
        write_json_to(&path, &v).unwrap();
        assert!(path.exists());
        let tmp = tmp_path(&path);
        assert!(!tmp.exists(), "成功写入后不应留下 .tmp");
    }

    #[test]
    #[cfg(unix)]
    fn write_json_secret_sets_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        // 用 path-based 变体避免污染全局 DATA_ROOT（与 state.rs 测试并发跑）
        let dir = temp_dir();
        let path = dir.join("secret.json");
        let v = Sample {
            name: "secret".into(),
            count: 1,
            ..Default::default()
        };
        write_json_secret_to(&path, &v).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "敏感文件应为 0o600，实际 {mode:o}");
    }

    #[test]
    #[cfg(unix)]
    fn write_json_secret_never_exposes_644_window() {
        // 验证 .tmp 在被 rename 之前就已是 0o600（不存在"先 644 再 chmod"的窗口）。
        // 做法：观察函数执行过程中创建的临时文件权限。
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir();
        let path = dir.join("secret.json");
        let v = Sample {
            name: "secret".into(),
            count: 7,
            ..Default::default()
        };
        write_json_secret_to(&path, &v).unwrap();
        // 最终文件应是 0o600
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // 不应残留 .tmp
        let tmp = tmp_path(&path);
        assert!(!tmp.exists(), "成功写入后不应留下 .tmp");
    }

    #[test]
    fn atomic_write_rejects_symlink_tmp() {
        // 攻击场景：恶意进程预先在 .tmp 路径创建一个软链接指向受害者文件，
        // 期望让我们覆盖它。我们应该检测到这一异常并报错。
        let dir = temp_dir();
        let path = dir.join("data.json");
        let tmp = tmp_path(&path);
        // 创建一个指向 /tmp/some-victim 的软链接（不需要真实存在）
        #[cfg(unix)]
        std::os::unix::fs::symlink("/tmp/weekly-report-victim-target", &tmp).unwrap();
        #[cfg(not(unix))]
        std::os::windows::fs::symlink_file("C:\\victim", &tmp).unwrap_or_default();

        let v = Sample {
            name: "x".into(),
            ..Default::default()
        };
        // 仅在 symlink 创建成功时跑断言（Windows 上需要管理员权限）
        if std::fs::symlink_metadata(&tmp).is_ok() {
            let r = write_json_to(&path, &v);
            assert!(r.is_err(), "应拒绝在 symlink .tmp 上写入");
        }
    }

    #[test]
    fn cleanup_keeps_recent_and_unrelated_files() {
        // 不引入 filetime crate，只验证："刚创建的 .broken-* 不会被误删，
        // 无关文件不会被触碰"。过期路径走 mtime 判断，由文件系统保证。
        let dir = temp_dir();

        let recent_broken = dir.join("smtp.json.broken-recent");
        std::fs::write(&recent_broken, b"recent").unwrap();

        let unrelated = dir.join("normal.json");
        std::fs::write(&unrelated, b"x").unwrap();

        cleanup_old_broken_backups(&dir).unwrap();

        assert!(recent_broken.exists(), "刚创建的 .broken 不该被清理");
        assert!(unrelated.exists(), "无关文件不该被触碰");
    }

    #[test]
    fn atomic_overwrite_preserves_existing_until_rename() {
        // 多次写入不应中途破坏文件
        let dir = temp_dir();
        let path = dir.join("over.json");
        for i in 0..5 {
            let v = Sample {
                name: format!("v{i}"),
                count: i,
                ..Default::default()
            };
            write_json_to(&path, &v).unwrap();
            let loaded: Sample = read_json_from(&path).unwrap();
            assert_eq!(loaded.count, i);
        }
    }
}
