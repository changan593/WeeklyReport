//! 高层 CRUD：在 [`crate::store`] 之上提供业务逻辑。
//!
//! 主要职责（详见 `docs/ARCHITECTURE.md#32-statesrs`）：
//! - 各实体的 `list / save / delete`
//! - 内置模板始终注入到列表
//! - 默认 LlmProvider 切换逻辑（新增/保存/删除时维护唯一默认）
//! - 首次启动时创建本机工作区
//!
//! 按实体拆分到 7 个子模块，每个文件 < 200 行；本文件作为入口
//! 统一 `pub use` 让 `crate::state::list_workspaces` 等老路径继续可用。
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// ---------- 通用文件名常量（子模块共享） ----------
pub(crate) const F_WORKSPACES: &str = "workspaces.json";
pub(crate) const F_TEMPLATES: &str = "templates.json";
pub(crate) const F_PROVIDERS: &str = "llm_providers.json";
pub(crate) const F_SCHEDULES: &str = "schedules.json";
pub(crate) const F_SMTP: &str = "smtp.json";
pub(crate) const F_SETTINGS: &str = "settings.json";
pub(crate) const F_REPORT_INDEX: &str = "reports/index.json";

// ============================================================
// 子模块（按实体切分）
// ============================================================

pub mod providers;
pub mod reports;
pub mod schedules;
pub mod smtp;
pub mod templates;
pub mod workspaces;

// 把子模块函数 re-export 到 state:: 顶层，调用方写法不变。
pub use providers::{delete_provider, get_default_provider, list_providers, save_provider};
pub use reports::{delete_report, get_report, list_reports, save_report};
pub use schedules::{delete_schedule, list_schedules, save_schedule, update_schedule_run_status};
pub use smtp::{get_smtp_config, save_smtp_config};
pub use templates::{delete_template, list_templates, save_template};
pub use workspaces::{delete_workspace, ensure_default_workspace, list_workspaces, save_workspace};

// ============================================================
// Settings（通用设置，单例）—— 简单到不值得单独拆文件
// ============================================================

/// 周报生成模式（单轮 vs 两轮 LLM）。
///
/// - `OneRound`：经典模式 —— 一次 LLM 调用，直接从 work_logs 生成 Markdown。
/// - `TwoRoundSilent`：两轮模式 —— 第一轮从 work_logs 提取「完成事项」JSON 数组
///   （项目/标题/细节/证据/日期），第二轮用提取出的事项 + 用户模板格式生成 Markdown。
///   质量更高，特别擅长去除"过程性指令"的噪音；代价是 token 用量翻倍、延迟更长。
///   第一轮 JSON 解析失败时自动 fallback 到 `OneRound`。UI 不需新增步骤（静默模式）。
///
/// 未来扩展：`TwoRoundWithEdit` 会在两轮之间插入用户编辑步骤（需前端 UI 支持）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GenerationMode {
    #[default]
    OneRound,
    TwoRoundSilent,
}

/// 应用通用设置。当前主要包含 token 压缩参数和 prompt 行为开关。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// AI 回复首尾保留字符数（详见 SPEC#3 核心算法）。默认 200。
    pub prompt_clip_chars: u32,
    /// 生成时注入到 prompt 的历史报告数量上限。仅在 [`Self::inject_past_reports`]
    /// 为 true 时生效；为 false 时此字段被忽略。默认 2。
    pub past_reports_context: u32,
    /// 是否把历史报告注入 prompt 作为风格参考。默认 `false`。
    ///
    /// 背景：早期版本默认注入最近 N 份历史报告，但发现历史报告会反向「污染」
    /// LLM 的格式判断 —— 即便用户切换到了另一个模板，LLM 也会照搬历史报告
    /// 的章节结构，导致用户的 `Template.extra_prompt` 形同虚设。
    /// v0.1.2 起改为默认关闭；启用后仍可通过 [`Self::past_reports_same_template_only`]
    /// 限定只取同模板生成的历史报告，进一步降低污染。
    #[serde(default)]
    pub inject_past_reports: bool,
    /// 注入历史报告时是否仅取**同模板 ID** 生成的报告。默认 `true`。
    /// 仅在 [`Self::inject_past_reports`] 为 true 时生效。
    #[serde(default = "default_true")]
    pub past_reports_same_template_only: bool,
    /// 周报生成模式。详见 [`GenerationMode`]。默认 `OneRound`（省成本）。
    #[serde(default)]
    pub generation_mode: GenerationMode,
    /// 应用界面语言（IETF BCP 47 简化）。当前支持 `"zh-CN"` / `"en"`。默认 `"zh-CN"`。
    /// 前端 i18n 资源在 `src/i18n/locales/`；本字段同步到 LocalStorage 给前端首屏使用。
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_language() -> String {
    "zh-CN".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            prompt_clip_chars: 200,
            past_reports_context: 2,
            inject_past_reports: false,
            past_reports_same_template_only: true,
            generation_mode: GenerationMode::default(),
            language: default_language(),
        }
    }
}

pub fn get_settings() -> anyhow::Result<Settings> {
    crate::store::read_json(F_SETTINGS)
}

pub fn save_settings(s: &Settings) -> anyhow::Result<()> {
    crate::store::write_json(F_SETTINGS, s)
}

// ============================================================
// 测试（聚合在一处，便于共享 setup 助手 + 跨实体测试）
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmProvider;
    use crate::report::{ReportRecord, Template};
    use crate::scheduler::Schedule;
    use crate::store;
    use crate::workspace::{Workspace, WorkspaceKind};
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard};
    use uuid::Uuid;

    // state 通过 store 的全局 DATA_ROOT 工作；多个 test 间需要串行化。
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

    // -------- Settings + 损坏恢复 --------

    #[test]
    fn settings_round_trip() {
        let (_dir, _guard) = setup();
        let s = Settings {
            prompt_clip_chars: 300,
            past_reports_context: 4,
            inject_past_reports: true,
            past_reports_same_template_only: false,
            generation_mode: GenerationMode::TwoRoundSilent,
            language: "zh-CN".to_string(),
        };
        save_settings(&s).unwrap();
        let loaded = get_settings().unwrap();
        assert_eq!(loaded, s);
    }

    #[test]
    fn settings_defaults_are_safe() {
        let s = Settings::default();
        assert!(
            !s.inject_past_reports,
            "默认不注入历史报告，避免污染 LLM 格式判断"
        );
        assert!(
            s.past_reports_same_template_only,
            "若用户开启注入，默认仍限定同模板"
        );
        assert_eq!(
            s.generation_mode,
            GenerationMode::OneRound,
            "默认单轮模式，避免成本翻倍"
        );
    }

    #[test]
    fn settings_backward_compat_without_new_fields() {
        // 旧版本 settings.json 没有 inject_past_reports / past_reports_same_template_only
        // / generation_mode 字段，反序列化应回到默认，不该崩
        let json = r#"{
            "prompt_clip_chars": 200,
            "past_reports_context": 2,
            "language": "zh-CN"
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert!(!s.inject_past_reports);
        assert!(s.past_reports_same_template_only);
        assert_eq!(s.generation_mode, GenerationMode::OneRound);
    }

    #[test]
    fn generation_mode_serializes_as_string() {
        let s = serde_json::to_string(&GenerationMode::OneRound).unwrap();
        assert_eq!(s, "\"OneRound\"");
        let s = serde_json::to_string(&GenerationMode::TwoRoundSilent).unwrap();
        assert_eq!(s, "\"TwoRoundSilent\"");
    }

    #[test]
    fn broken_json_recovers_to_default_and_backs_up() {
        let (dir, _guard) = setup();
        let bad = dir.join("workspaces.json");
        std::fs::write(&bad, b"not json {").unwrap();

        let list = list_workspaces().unwrap();
        assert!(list.is_empty(), "损坏文件应恢复为默认空列表");

        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("workspaces.json.broken-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
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
        assert!(!saved.id.is_empty());
        let list = list_workspaces().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "公司机");
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
        assert_eq!(list_workspaces().unwrap().len(), 1);
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
        let mut t = list_templates().unwrap().remove(0);
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
        assert_eq!(list.len(), 4);
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
        assert!(delete_template(&t.id).is_err());
    }

    // -------- LlmProvider 默认管理 --------

    #[test]
    fn first_provider_becomes_default_automatically() {
        let (_dir, _guard) = setup();
        let p = save_provider(make_provider("A", false)).unwrap();
        assert!(p.is_default);
    }

    #[test]
    fn setting_default_cancels_others() {
        let (_dir, _guard) = setup();
        let a = save_provider(make_provider("A", false)).unwrap();
        let b = save_provider(make_provider("B", true)).unwrap();
        let list = list_providers().unwrap();
        assert!(!list.iter().find(|p| p.id == a.id).unwrap().is_default);
        assert!(list.iter().find(|p| p.id == b.id).unwrap().is_default);
    }

    #[test]
    fn deleting_default_provider_re_elects_first() {
        let (_dir, _guard) = setup();
        let a = save_provider(make_provider("A", false)).unwrap();
        let b = save_provider(make_provider("B", false)).unwrap();
        let _c = save_provider(make_provider("C", false)).unwrap();
        delete_provider(&a.id).unwrap();
        let list = list_providers().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list.iter().find(|p| p.is_default).unwrap().id, b.id);
    }

    #[test]
    fn deleting_non_default_does_not_change_default() {
        let (_dir, _guard) = setup();
        let a = save_provider(make_provider("A", false)).unwrap();
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
        assert!(!raw.contains("next_run"));
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
        assert!(list_reports().unwrap().is_empty());
    }
}
