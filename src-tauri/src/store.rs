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

/// 初始化数据目录到 OS 默认位置：
/// - macOS:   `~/Library/Application Support/WeeklyReport/`
/// - Linux:   `~/.config/weekly-report/`
/// - Windows: `%APPDATA%\WeeklyReport\`
pub fn init() -> Result<()> {
    init_at(default_data_dir()?)
}

/// 初始化数据目录到指定路径（用于测试或自定义部署）。
/// 同时创建 `reports/` 子目录。
pub fn init_at(root: PathBuf) -> Result<()> {
    fs::create_dir_all(&root).with_context(|| format!("创建数据目录失败: {}", root.display()))?;
    let reports = root.join("reports");
    fs::create_dir_all(&reports)
        .with_context(|| format!("创建报告目录失败: {}", reports.display()))?;
    *DATA_ROOT.write().expect("DATA_ROOT 锁中毒") = Some(root);
    Ok(())
}

/// 返回当前数据目录。若未 init 则返回错误。
pub fn data_dir() -> Result<PathBuf> {
    DATA_ROOT
        .read()
        .expect("DATA_ROOT 锁中毒")
        .clone()
        .ok_or_else(|| anyhow!(crate::i18n::t("err.store.not_init")))
}

/// 计算 OS 默认数据目录（不创建）。
fn default_data_dir() -> Result<PathBuf> {
    let base =
        dirs::config_dir().ok_or_else(|| anyhow!(crate::i18n::t("err.store.no_config_dir")))?;
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

/// 同 [`write_json`]，但写完后把文件权限限制为仅当前用户可读写（POSIX 0600）。
///
/// 用于保存敏感数据（API key / SMTP 密码）。Windows 上 NTFS 默认已经只允许当前用户访问，
/// 此函数为 no-op。
pub fn write_json_secret<T: Serialize>(filename: &str, value: &T) -> Result<()> {
    let path = data_dir()?.join(filename);
    write_json_secret_to(&path, value)
}

pub(crate) fn write_json_secret_to<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_json_to(path, value)?;
    restrict_to_user(path)?;
    Ok(())
}

/// 把文件权限设为 `0o600`（rw-------）。仅 Unix 生效；Windows 是 no-op。
///
/// 失败只 `warn!`，不阻断写入（权限设置失败一般是 FS 不支持 chmod，如 FAT32 / 网络盘）。
#[cfg(unix)]
fn restrict_to_user(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = match fs::metadata(path) {
        Ok(m) => m.permissions(),
        Err(e) => {
            tracing::warn!("无法读取 {} 权限: {e}", path.display());
            return Ok(());
        }
    };
    perms.set_mode(0o600);
    if let Err(e) = fs::set_permissions(path, perms) {
        tracing::warn!("无法设置 {} 权限为 0600: {e}", path.display());
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_to_user(_path: &Path) -> Result<()> {
    Ok(())
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
    atomic_write(path, &body)
}

/// 原子写入字节流：先写 `<path>.tmp`，再 `rename`。
fn atomic_write(path: &Path, body: &[u8]) -> Result<()> {
    let tmp = tmp_path(path);
    {
        let mut f = fs::File::create(&tmp)
            .with_context(|| format!("创建临时文件失败: {}", tmp.display()))?;
        f.write_all(body)
            .with_context(|| format!("写入临时文件失败: {}", tmp.display()))?;
        // sync_all 在某些 FS（如 tmpfs）上会失败，忽略
        let _ = f.sync_all();
    }
    fs::rename(&tmp, path)
        .with_context(|| format!("rename 失败: {} -> {}", tmp.display(), path.display()))?;
    Ok(())
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
pub fn save_report_file(id: &str, content: &str) -> Result<PathBuf> {
    let path = data_dir()?.join("reports").join(format!("{id}.md"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(&path, content.as_bytes())?;
    Ok(path)
}

/// 读取 `reports/<id>.md` 全文。文件不存在时返回错误。
pub fn load_report_file(id: &str) -> Result<String> {
    let path = data_dir()?.join("reports").join(format!("{id}.md"));
    fs::read_to_string(&path).with_context(|| format!("读取报告失败: {}", path.display()))
}

/// 删除 `reports/<id>.md`。文件不存在时静默成功。
pub fn delete_report_file(id: &str) -> Result<()> {
    let path = data_dir()?.join("reports").join(format!("{id}.md"));
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("删除报告失败: {}", path.display()))?;
    }
    Ok(())
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
