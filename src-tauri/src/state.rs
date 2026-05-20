//! 高层 CRUD：在 [`crate::store`] 之上提供业务逻辑。
//!
//! 主要职责（详见 `docs/ARCHITECTURE.md#32-statesrs`）：
//! - 各实体的 `list / save / delete`
//! - 内置模板始终注入到列表
//! - 默认 LlmProvider 切换逻辑（新增/保存/删除时维护唯一默认）
//! - 首次启动时创建本机工作区
//!
//! 阶段 1 把所有 CRUD 完整暴露；阶段 4+ 才会被 Tauri command 调用，
//! 因此暂时 allow dead_code。
#![allow(dead_code)]

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::email::SmtpConfig;
use crate::llm::LlmProvider;
use crate::report::{ReportRecord, Template};
use crate::scheduler::Schedule;
use crate::store;
use crate::workspace::{Workspace, WorkspaceKind};

// ---------- 通用文件名常量 ----------
const F_WORKSPACES: &str = "workspaces.json";
const F_TEMPLATES: &str = "templates.json";
const F_PROVIDERS: &str = "llm_providers.json";
const F_SCHEDULES: &str = "schedules.json";
const F_SMTP: &str = "smtp.json";
const F_SETTINGS: &str = "settings.json";
const F_REPORT_INDEX: &str = "reports/index.json";

// ============================================================
// Settings（通用设置，单例）
// ============================================================

/// 应用通用设置。当前主要包含 token 压缩参数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// AI 回复首尾保留字符数（详见 SPEC#3 核心算法）。默认 200。
    pub prompt_clip_chars: u32,
    /// 生成时注入到 prompt 的历史报告数量。默认 2。
    pub past_reports_context: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            prompt_clip_chars: 200,
            past_reports_context: 2,
        }
    }
}

pub fn get_settings() -> Result<Settings> {
    store::read_json(F_SETTINGS)
}

pub fn save_settings(s: &Settings) -> Result<()> {
    store::write_json(F_SETTINGS, s)
}

// ============================================================
// Workspace
// ============================================================

pub fn list_workspaces() -> Result<Vec<Workspace>> {
    store::read_json(F_WORKSPACES)
}

/// 保存 Workspace（新增或更新）。id 为空时自动生成 UUID。
pub fn save_workspace(mut ws: Workspace) -> Result<Workspace> {
    if ws.id.is_empty() {
        ws.id = Uuid::new_v4().to_string();
    }
    if ws.name.trim().is_empty() {
        bail!("工作区名称不能为空");
    }
    let mut list = list_workspaces()?;
    match list.iter().position(|w| w.id == ws.id) {
        Some(pos) => list[pos] = ws.clone(),
        None => list.push(ws.clone()),
    }
    store::write_json(F_WORKSPACES, &list)?;
    Ok(ws)
}

pub fn delete_workspace(id: &str) -> Result<()> {
    let mut list = list_workspaces()?;
    let before = list.len();
    list.retain(|w| w.id != id);
    if list.len() == before {
        // 没找到也算成功，幂等删除
        return Ok(());
    }
    store::write_json(F_WORKSPACES, &list)
}

/// 首次启动时若没有任何 Workspace，则创建一个指向 `~/.claude` / `~/.codex` 的本机工作区。
///
/// 已有 workspace 时不做任何事。
pub fn ensure_default_workspace() -> Result<()> {
    let existing = list_workspaces()?;
    if !existing.is_empty() {
        return Ok(());
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow!("无法定位用户家目录"))?;
    let ws = Workspace {
        id: Uuid::new_v4().to_string(),
        name: "本机".to_string(),
        kind: WorkspaceKind::Local,
        host: None,
        user: None,
        port: None,
        ssh_key: None,
        claude_path: Some(home.join(".claude").to_string_lossy().into_owned()),
        codex_path: Some(home.join(".codex").to_string_lossy().into_owned()),
        tools: vec!["claude-code".to_string(), "codex".to_string()],
    };
    save_workspace(ws)?;
    Ok(())
}

// ============================================================
// Template（带内置模板注入）
// ============================================================

/// 列出所有模板：内置 3 个 + 用户自定义。
///
/// 内置始终在前，顺序固定（tech / exec / simple）。
pub fn list_templates() -> Result<Vec<Template>> {
    let mut customs: Vec<Template> = store::read_json(F_TEMPLATES)?;
    // 防御性：即使持久化文件被人手改塞入了 builtin 标记，也过滤掉
    customs.retain(|t| !t.builtin && !t.id.starts_with("builtin-"));

    let mut result = builtin_templates();
    result.extend(customs);
    Ok(result)
}

/// 保存自定义模板。拒绝写入内置模板。
pub fn save_template(mut t: Template) -> Result<Template> {
    if t.builtin || t.id.starts_with("builtin-") {
        bail!("不可修改内置模板");
    }
    if t.name.trim().is_empty() {
        bail!("模板名称不能为空");
    }
    if t.sections.is_empty() {
        bail!("模板至少需要一个章节");
    }
    if t.id.is_empty() {
        t.id = Uuid::new_v4().to_string();
    }
    let mut customs: Vec<Template> = store::read_json(F_TEMPLATES)?;
    customs.retain(|x| !x.builtin && !x.id.starts_with("builtin-"));
    match customs.iter().position(|x| x.id == t.id) {
        Some(pos) => customs[pos] = t.clone(),
        None => customs.push(t.clone()),
    }
    store::write_json(F_TEMPLATES, &customs)?;
    Ok(t)
}

/// 删除自定义模板。拒绝删除内置；若被定时任务引用则拒绝。
pub fn delete_template(id: &str) -> Result<()> {
    if id.starts_with("builtin-") {
        bail!("不可删除内置模板");
    }
    let schedules = list_schedules()?;
    let users: Vec<String> = schedules
        .iter()
        .filter(|s| s.template_id == id)
        .map(|s| s.name.clone())
        .collect();
    if !users.is_empty() {
        bail!("无法删除：以下定时任务正在使用此模板: {}", users.join(", "));
    }
    let mut customs: Vec<Template> = store::read_json(F_TEMPLATES)?;
    customs.retain(|t| t.id != id);
    store::write_json(F_TEMPLATES, &customs)
}

/// 3 个内置模板。
fn builtin_templates() -> Vec<Template> {
    vec![
        Template {
            id: "builtin-tech".into(),
            name: "技术周报".into(),
            style: "tech".into(),
            sections: vec![
                "本周 TL;DR".into(),
                "各项目进展".into(),
                "技术亮点".into(),
                "下周计划".into(),
            ],
            provider_id: None,
            extra_prompt: String::new(),
            builtin: true,
        },
        Template {
            id: "builtin-exec".into(),
            name: "管理层汇报".into(),
            style: "exec".into(),
            sections: vec![
                "执行摘要".into(),
                "关键进展".into(),
                "风险阻塞".into(),
                "下周重点".into(),
            ],
            provider_id: None,
            extra_prompt: String::new(),
            builtin: true,
        },
        Template {
            id: "builtin-simple".into(),
            name: "简洁日报".into(),
            style: "simple".into(),
            sections: vec!["做了啥".into(), "问题".into(), "下周".into()],
            provider_id: None,
            extra_prompt: String::new(),
            builtin: true,
        },
    ]
}

// ============================================================
// LlmProvider（带默认源管理）
// ============================================================

pub fn list_providers() -> Result<Vec<LlmProvider>> {
    store::read_json(F_PROVIDERS)
}

/// 保存 provider（新增或更新）。维护「同时只有一个 default」不变量。
///
/// 规则：
/// - 新增且列表原本为空 → 自动设为 default
/// - 入参 `is_default = true` → 其他全部置 false
/// - 最终列表非空但没有 default → 第一个升级为 default
pub fn save_provider(mut p: LlmProvider) -> Result<LlmProvider> {
    if p.name.trim().is_empty() {
        bail!("LLM 源名称不能为空");
    }
    let new_id = p.id.is_empty();
    if new_id {
        p.id = Uuid::new_v4().to_string();
    }

    let mut list = list_providers()?;
    let existed = list.iter().any(|x| x.id == p.id);

    // 新增且列表原本为空 → 强制默认
    if new_id && list.is_empty() {
        p.is_default = true;
    }

    // 标记为默认时，其他取消
    if p.is_default {
        for x in list.iter_mut() {
            if x.id != p.id {
                x.is_default = false;
            }
        }
    }

    if existed {
        if let Some(pos) = list.iter().position(|x| x.id == p.id) {
            list[pos] = p.clone();
        }
    } else {
        list.push(p.clone());
    }

    // 兜底：非空但无 default → 第一个升级
    if !list.is_empty() && !list.iter().any(|x| x.is_default) {
        list[0].is_default = true;
        // 若 p 就是 list[0]，同步更新返回值
        if list[0].id == p.id {
            p.is_default = true;
        }
    }

    store::write_json_secret(F_PROVIDERS, &list)?;
    Ok(p)
}

/// 删除 provider。若删除的是当前 default，则把剩余的第一个升级为 default。
pub fn delete_provider(id: &str) -> Result<()> {
    let mut list = list_providers()?;
    let pos = match list.iter().position(|x| x.id == id) {
        Some(i) => i,
        None => return Ok(()), // 幂等
    };
    let was_default = list[pos].is_default;
    list.remove(pos);
    if was_default {
        if let Some(first) = list.first_mut() {
            first.is_default = true;
        }
    }
    store::write_json_secret(F_PROVIDERS, &list)
}

/// 按优先级返回当前应使用的 provider（参考 `docs/LLM.md#6-优先级解析`）。
///
/// 此函数只覆盖 3-5 步：default → 第一个 → 报错。调用方处理 1-2 步。
pub fn get_default_provider() -> Result<LlmProvider> {
    let list = list_providers()?;
    if let Some(p) = list.iter().find(|x| x.is_default) {
        return Ok(p.clone());
    }
    if let Some(p) = list.first() {
        return Ok(p.clone());
    }
    bail!("未配置任何 LLM 源")
}

// ============================================================
// Schedule
// ============================================================

pub fn list_schedules() -> Result<Vec<Schedule>> {
    store::read_json(F_SCHEDULES)
}

pub fn save_schedule(mut s: Schedule) -> Result<Schedule> {
    if s.name.trim().is_empty() {
        bail!("任务名称不能为空");
    }
    if s.cron.trim().is_empty() {
        bail!("cron 表达式不能为空");
    }
    if s.id.is_empty() {
        s.id = Uuid::new_v4().to_string();
    }
    // 不持久化 next_run（运行时计算）
    s.next_run = None;

    let mut list = list_schedules()?;
    match list.iter().position(|x| x.id == s.id) {
        Some(pos) => list[pos] = s.clone(),
        None => list.push(s.clone()),
    }
    store::write_json(F_SCHEDULES, &list)?;
    Ok(s)
}

pub fn delete_schedule(id: &str) -> Result<()> {
    let mut list = list_schedules()?;
    list.retain(|x| x.id != id);
    store::write_json(F_SCHEDULES, &list)
}

/// 只更新 schedule 的 `last_run` / `last_status` 字段；其他字段原样保留。
///
/// 给 scheduler 在任务完成（成功或失败）后调用。
pub fn update_schedule_run_status(
    id: &str,
    last_run: Option<String>,
    last_status: Option<String>,
) -> Result<()> {
    let mut list = list_schedules()?;
    if let Some(s) = list.iter_mut().find(|s| s.id == id) {
        s.last_run = last_run;
        s.last_status = last_status;
        store::write_json(F_SCHEDULES, &list)?;
    }
    Ok(())
}

// ============================================================
// SMTP（单例）
// ============================================================

pub fn get_smtp_config() -> Result<SmtpConfig> {
    store::read_json(F_SMTP)
}

pub fn save_smtp_config(cfg: &SmtpConfig) -> Result<()> {
    store::write_json_secret(F_SMTP, cfg)
}

// ============================================================
// Reports（元数据 + 正文文件）
// ============================================================

pub fn list_reports() -> Result<Vec<ReportRecord>> {
    store::read_json(F_REPORT_INDEX)
}

/// 保存一份新的报告：写 Markdown 文件 + 更新 index。
///
/// id 为空时自动生成 UUID。原子写。
pub fn save_report(mut record: ReportRecord, content: &str) -> Result<ReportRecord> {
    if record.id.is_empty() {
        record.id = Uuid::new_v4().to_string();
    }
    store::save_report_file(&record.id, content)?;

    let mut list = list_reports()?;
    match list.iter().position(|r| r.id == record.id) {
        Some(pos) => list[pos] = record.clone(),
        None => list.push(record.clone()),
    }
    store::write_json(F_REPORT_INDEX, &list)?;
    Ok(record)
}

/// 返回 (元数据, Markdown 正文)。
pub fn get_report(id: &str) -> Result<(ReportRecord, String)> {
    let list = list_reports()?;
    let record = list
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| anyhow!("报告不存在: {id}"))?;
    let content = store::load_report_file(id)?;
    Ok((record, content))
}

/// 删除报告：同时删除 .md 文件和 index.json 中的条目。
pub fn delete_report(id: &str) -> Result<()> {
    let mut list = list_reports()?;
    list.retain(|r| r.id != id);
    store::write_json(F_REPORT_INDEX, &list)?;
    store::delete_report_file(id)?;
    Ok(())
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard};

    // state.rs 通过 store 的全局 DATA_ROOT 工作；多个 test 间需要串行化。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// 锁定全局状态并把数据目录指向一个独立 tempdir。
    fn setup() -> (PathBuf, MutexGuard<'static, ()>) {
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut dir = std::env::temp_dir();
        dir.push(format!("weekly-report-state-{}", Uuid::new_v4()));
        store::init_at(dir.clone()).unwrap();
        (dir, guard)
    }

    fn make_provider(name: &str, is_default: bool) -> LlmProvider {
        LlmProvider {
            id: String::new(),
            name: name.into(),
            kind: crate::llm::LlmKind::OpenAiCompatible,
            base_url: "https://api.example.com".into(),
            api_key: "sk-test".into(),
            model: "test-model".into(),
            max_tokens: 1024,
            temperature: Some(0.7),
            is_default,
            extra_headers: Default::default(),
        }
    }

    // -------- store-level：round-trip & corruption --------

    #[test]
    fn settings_round_trip() {
        let (_dir, _guard) = setup();
        let s = Settings {
            prompt_clip_chars: 300,
            past_reports_context: 4,
        };
        save_settings(&s).unwrap();
        let loaded = get_settings().unwrap();
        assert_eq!(loaded, s);
    }

    #[test]
    fn broken_json_recovers_to_default_and_backs_up() {
        let (dir, _guard) = setup();
        // 故意写入损坏的 workspaces.json
        let bad = dir.join("workspaces.json");
        std::fs::write(&bad, b"not json {").unwrap();

        let list = list_workspaces().unwrap();
        assert!(list.is_empty(), "损坏文件应恢复为默认空列表");

        // 备份应已生成
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("workspaces.json.broken-")
            })
            .collect();
        assert_eq!(backups.len(), 1, "应有一个 .broken-* 备份");
    }

    // -------- Workspace --------

    #[test]
    fn workspace_save_and_load_equal() {
        let (_dir, _guard) = setup();
        let ws = Workspace {
            id: String::new(),
            name: "公司机".into(),
            kind: WorkspaceKind::Local,
            tools: vec!["claude-code".into()],
            claude_path: Some("/home/x/.claude".into()),
            ..Default::default()
        };
        let saved = save_workspace(ws.clone()).unwrap();
        assert!(!saved.id.is_empty(), "应自动生成 UUID");

        let list = list_workspaces().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "公司机");
        assert_eq!(list[0].id, saved.id);
    }

    #[test]
    fn workspace_empty_name_rejected() {
        let (_dir, _guard) = setup();
        let ws = Workspace {
            name: "  ".into(),
            ..Default::default()
        };
        assert!(save_workspace(ws).is_err());
    }

    #[test]
    fn ensure_default_workspace_creates_only_once() {
        let (_dir, _guard) = setup();
        ensure_default_workspace().unwrap();
        assert_eq!(list_workspaces().unwrap().len(), 1);
        ensure_default_workspace().unwrap();
        assert_eq!(list_workspaces().unwrap().len(), 1, "幂等");
    }

    // -------- Template --------

    #[test]
    fn templates_always_include_builtins() {
        let (_dir, _guard) = setup();
        let list = list_templates().unwrap();
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|t| t.builtin));
        assert_eq!(list[0].id, "builtin-tech");
    }

    #[test]
    fn templates_builtin_cannot_be_modified() {
        let (_dir, _guard) = setup();
        let mut t = list_templates().unwrap().remove(0); // builtin-tech
        t.name = "Hacked".into();
        assert!(save_template(t).is_err());
    }

    #[test]
    fn templates_builtin_cannot_be_deleted() {
        let (_dir, _guard) = setup();
        assert!(delete_template("builtin-tech").is_err());
    }

    #[test]
    fn templates_custom_round_trip_excludes_builtin_in_file() {
        let (dir, _guard) = setup();
        let t = Template {
            id: String::new(),
            name: "My Template".into(),
            style: "custom".into(),
            sections: vec!["A".into(), "B".into()],
            provider_id: None,
            extra_prompt: String::new(),
            builtin: false,
        };
        save_template(t).unwrap();
        let list = list_templates().unwrap();
        assert_eq!(list.len(), 4, "3 内置 + 1 自定义");
        // 持久化文件中不应含 builtin
        let raw = std::fs::read_to_string(dir.join("templates.json")).unwrap();
        assert!(!raw.contains("builtin-tech"));
        assert!(raw.contains("My Template"));
    }

    #[test]
    fn templates_blocked_by_referencing_schedule() {
        let (_dir, _guard) = setup();
        let t = save_template(Template {
            id: String::new(),
            name: "T".into(),
            style: "custom".into(),
            sections: vec!["x".into()],
            builtin: false,
            extra_prompt: String::new(),
            provider_id: None,
        })
        .unwrap();
        save_schedule(Schedule {
            id: String::new(),
            name: "用此模板".into(),
            cron: "0 0 9 ? * MON *".into(),
            enabled: true,
            workspace_ids: vec![],
            template_id: t.id.clone(),
            provider_id: None,
            days: 7,
            recipients: vec![],
            cc: vec![],
            subject_tpl: String::new(),
            last_run: None,
            last_status: None,
            next_run: None,
        })
        .unwrap();
        assert!(
            delete_template(&t.id).is_err(),
            "被定时任务引用时应拒绝删除"
        );
    }

    // -------- LlmProvider 默认管理 --------

    #[test]
    fn first_provider_becomes_default_automatically() {
        let (_dir, _guard) = setup();
        let p = save_provider(make_provider("A", false)).unwrap();
        assert!(p.is_default, "第一个 provider 应自动设为默认");
    }

    #[test]
    fn setting_default_cancels_others() {
        let (_dir, _guard) = setup();
        let a = save_provider(make_provider("A", false)).unwrap();
        let b = save_provider(make_provider("B", true)).unwrap();
        let list = list_providers().unwrap();
        let a2 = list.iter().find(|p| p.id == a.id).unwrap();
        let b2 = list.iter().find(|p| p.id == b.id).unwrap();
        assert!(!a2.is_default, "之前的 default 应被取消");
        assert!(b2.is_default);
    }

    #[test]
    fn deleting_default_provider_re_elects_first() {
        let (_dir, _guard) = setup();
        let a = save_provider(make_provider("A", false)).unwrap(); // default
        let b = save_provider(make_provider("B", false)).unwrap();
        let c = save_provider(make_provider("C", false)).unwrap();
        assert!(a.is_default);

        delete_provider(&a.id).unwrap();
        let list = list_providers().unwrap();
        assert_eq!(list.len(), 2);
        // 剩余的第一个升级为 default
        let new_default = list.iter().find(|p| p.is_default).unwrap();
        // 顺序应保持插入顺序：B 先于 C
        assert_eq!(new_default.id, b.id);
        // C 仍非默认
        assert!(!list.iter().find(|p| p.id == c.id).unwrap().is_default);
    }

    #[test]
    fn deleting_non_default_does_not_change_default() {
        let (_dir, _guard) = setup();
        let a = save_provider(make_provider("A", false)).unwrap(); // auto default
        let b = save_provider(make_provider("B", false)).unwrap();
        delete_provider(&b.id).unwrap();
        let list = list_providers().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, a.id);
        assert!(list[0].is_default);
    }

    #[test]
    fn get_default_provider_errors_when_empty() {
        let (_dir, _guard) = setup();
        assert!(get_default_provider().is_err());
    }

    #[test]
    fn get_default_provider_falls_back_to_first_if_none_flagged() {
        let (_dir, _guard) = setup();
        // 手工写入两个都不带 default 的（绕过 save_provider）
        let list = vec![
            LlmProvider {
                id: "p1".into(),
                ..make_provider("A", false)
            },
            LlmProvider {
                id: "p2".into(),
                ..make_provider("B", false)
            },
        ];
        store::write_json(F_PROVIDERS, &list).unwrap();
        let got = get_default_provider().unwrap();
        assert_eq!(got.id, "p1");
    }

    // -------- Schedule --------

    #[test]
    fn schedule_next_run_not_persisted() {
        let (dir, _guard) = setup();
        save_schedule(Schedule {
            id: String::new(),
            name: "s".into(),
            cron: "0 0 9 * * MON *".into(),
            enabled: true,
            workspace_ids: vec![],
            template_id: "builtin-tech".into(),
            provider_id: None,
            days: 7,
            recipients: vec![],
            cc: vec![],
            subject_tpl: String::new(),
            last_run: None,
            last_status: None,
            next_run: Some("应该被清除".into()),
        })
        .unwrap();
        let raw = std::fs::read_to_string(dir.join("schedules.json")).unwrap();
        assert!(!raw.contains("next_run"), "next_run 不应持久化");
    }

    // -------- Report --------

    #[test]
    fn report_save_get_delete_round_trip() {
        let (_dir, _guard) = setup();
        let r = save_report(
            ReportRecord {
                id: String::new(),
                week: "最近 7 天".into(),
                template_id: "builtin-tech".into(),
                template_name: "技术周报".into(),
                provider_id: Some("p1".into()),
                provider_name: Some("Claude".into()),
                tokens_used: 1234,
                project_count: 2,
                generated_at: "2026-05-20T12:00:00+08:00".into(),
            },
            "# Hello\n\n内容",
        )
        .unwrap();
        assert!(!r.id.is_empty());

        let (record, body) = get_report(&r.id).unwrap();
        assert_eq!(record.tokens_used, 1234);
        assert_eq!(body, "# Hello\n\n内容");

        delete_report(&r.id).unwrap();
        assert!(get_report(&r.id).is_err());
        let list = list_reports().unwrap();
        assert!(list.is_empty());
    }
}
