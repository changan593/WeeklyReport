// 防止 Windows release 模式弹控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tracing_subscriber::EnvFilter;

mod email;
mod i18n;
mod llm;
mod logs;
mod projectdocs;
mod report;
mod scheduler;
mod ssh;
mod state;
mod store;
mod workspace;

use chrono::Local;
use email::{EmailRequest, SmtpConfig};
use llm::LlmProvider;
use report::{ReportRecord, Template};
use scheduler::{Schedule, SchedulerState};
use serde::{Deserialize, Serialize};
use state::Settings;
use tauri::Manager;
use workspace::Workspace;

/// 应用入口。
///
/// 启动顺序：
/// 1. 初始化 tracing 日志
/// 2. 初始化存储目录（创建 OS 配置目录 + `reports/` 子目录）
/// 3. 首次启动时创建默认本机工作区
/// 4. 启动 Tauri 主循环（command 注册见 `docs/ARCHITECTURE.md#310-mainrs--tauri-入口`）
fn main() {
    init_tracing();

    if let Err(err) = store::init() {
        tracing::error!("初始化存储失败: {:#}", err);
    } else if let Err(err) = state::ensure_default_workspace() {
        tracing::error!("初始化默认工作区失败: {:#}", err);
    }

    // 把 Settings.language 同步给后端 i18n（控制 anyhow! 错误消息与邮件模板的语言）。
    // 读失败时静默，保持默认 zh-CN。
    if let Ok(s) = state::get_settings() {
        i18n::set_language(&s.language);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // 初始化 scheduler 并加载所有 enabled 任务。
            // 用 block_on 确保后续 command 能立刻 state::<SchedulerState>。
            let scheduler = tauri::async_runtime::block_on(async {
                let s = SchedulerState::new().await?;
                if let Err(e) = s.reload_all().await {
                    tracing::warn!("reload_all 失败（继续启动）: {:#}", e);
                }
                anyhow::Ok(s)
            })
            .map_err(|e| {
                Box::<dyn std::error::Error>::from(format!("初始化 scheduler 失败: {e:#}"))
            })?;
            app.manage(scheduler);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            // Workspaces
            list_workspaces,
            save_workspace,
            delete_workspace,
            test_workspace_connection,
            // LLM Providers
            list_providers,
            save_provider,
            delete_provider,
            test_provider,
            llm_presets,
            // Templates
            list_templates,
            save_template,
            delete_template,
            // Reports
            list_reports,
            get_report,
            get_report_html,
            delete_report,
            generate_report,
            collect_logs,
            render_report,
            save_draft_summary,
            load_draft_summary,
            clear_draft_summary,
            // Settings
            get_settings,
            save_settings,
            set_app_language,
            // SMTP
            get_smtp_config,
            save_smtp_config,
            test_smtp_config,
            send_test_email,
            // Schedules
            list_schedules,
            save_schedule,
            delete_schedule,
            run_schedule_now,
            // Misc
            data_dir_path,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}

/// 初始化 tracing 日志。默认 INFO 级别，可通过 `RUST_LOG` 覆盖。
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

// ============================================================
// 通用工具：anyhow 错误 → String（给前端友好显示）
// ============================================================

fn err_to_string(e: anyhow::Error) -> String {
    format!("{e:#}")
}

// ============================================================
// 健康检查
// ============================================================

#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

#[tauri::command]
fn data_dir_path() -> Result<String, String> {
    store::data_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(err_to_string)
}

// ============================================================
// Workspaces
// ============================================================

#[tauri::command]
async fn list_workspaces() -> Result<Vec<Workspace>, String> {
    state::list_workspaces().map_err(err_to_string)
}

#[tauri::command]
async fn save_workspace(workspace: Workspace) -> Result<Workspace, String> {
    state::save_workspace(workspace).map_err(err_to_string)
}

#[tauri::command]
async fn delete_workspace(id: String) -> Result<(), String> {
    state::delete_workspace(&id).map_err(err_to_string)
}

#[tauri::command]
async fn test_workspace_connection(workspace: Workspace) -> Result<String, String> {
    workspace::test_connection(&workspace)
        .await
        .map_err(err_to_string)
}

// ============================================================
// LLM Providers
// ============================================================

#[tauri::command]
async fn list_providers() -> Result<Vec<LlmProvider>, String> {
    state::list_providers().map_err(err_to_string)
}

#[tauri::command]
async fn save_provider(provider: LlmProvider) -> Result<LlmProvider, String> {
    state::save_provider(provider).map_err(err_to_string)
}

#[tauri::command]
async fn delete_provider(id: String) -> Result<(), String> {
    state::delete_provider(&id).map_err(err_to_string)
}

#[tauri::command]
async fn test_provider(provider: LlmProvider) -> Result<String, String> {
    llm::test_connection(&provider).await.map_err(err_to_string)
}

#[tauri::command]
fn llm_presets() -> Vec<LlmProvider> {
    llm::presets().into_iter().map(|(_, p)| p).collect()
}

// ============================================================
// Templates
// ============================================================

#[tauri::command]
async fn list_templates() -> Result<Vec<Template>, String> {
    state::list_templates().map_err(err_to_string)
}

#[tauri::command]
async fn save_template(template: Template) -> Result<Template, String> {
    state::save_template(template).map_err(err_to_string)
}

#[tauri::command]
async fn delete_template(id: String) -> Result<(), String> {
    state::delete_template(&id).map_err(err_to_string)
}

// ============================================================
// Reports & Generation
// ============================================================

#[derive(Debug, Serialize)]
struct ReportPayload {
    record: ReportRecord,
    content: String,
}

#[tauri::command]
async fn list_reports() -> Result<Vec<ReportRecord>, String> {
    state::list_reports().map_err(err_to_string)
}

#[tauri::command]
async fn get_report(id: String) -> Result<ReportPayload, String> {
    let (record, content) = state::get_report(&id).map_err(err_to_string)?;
    Ok(ReportPayload { record, content })
}

/// 取报告的 HTML 版本（用于 Reports 详情的 HTML 预览 / 复制 HTML）。
/// 优先返回保存的 .html，没有就从 .md 现场渲染。
#[tauri::command]
async fn get_report_html(id: String) -> Result<String, String> {
    report::get_report_html(&id).map_err(err_to_string)
}

#[tauri::command]
async fn delete_report(id: String) -> Result<(), String> {
    state::delete_report(&id).map_err(err_to_string)
}

#[derive(Debug, Clone, Deserialize)]
struct GenerateRequest {
    workspace_ids: Vec<String>,
    template_id: String,
    days: u32,
    #[serde(default)]
    provider_id: Option<String>,
}

#[tauri::command]
async fn generate_report(req: GenerateRequest) -> Result<report::GenerationOutput, String> {
    report::run_generation(
        &req.workspace_ids,
        &req.template_id,
        req.days,
        req.provider_id.as_deref(),
    )
    .await
    .map_err(err_to_string)
}

// 两步生成新流程（详见 docs/SPEC.md "两步生成"）：
//   1. collect_logs：扫日志 + 聚合 → 返回带 timestamp 的 Summary 供前端编辑
//   2. render_report：用编辑后的 Summary 调 LLM + 存档
// 草稿 3 个命令支持"上次未完成的编辑下次继续"。

#[derive(Debug, Clone, Deserialize)]
struct CollectLogsRequest {
    workspace_ids: Vec<String>,
    days: u32,
}

#[tauri::command]
async fn collect_logs(req: CollectLogsRequest) -> Result<report::CollectionOutput, String> {
    report::collect_summary(&req.workspace_ids, req.days)
        .await
        .map_err(err_to_string)
}

#[derive(Debug, Clone, Deserialize)]
struct RenderReportRequest {
    summary: logs::Summary,
    template_id: String,
    days: u32,
    #[serde(default)]
    provider_id: Option<String>,
}

#[tauri::command]
async fn render_report(req: RenderReportRequest) -> Result<report::GenerationOutput, String> {
    report::render_from_summary(
        &req.summary,
        &req.template_id,
        req.days,
        req.provider_id.as_deref(),
    )
    .await
    .map_err(err_to_string)
}

#[tauri::command]
fn save_draft_summary(draft: report::CollectionOutput) -> Result<(), String> {
    store::save_draft_summary(&draft).map_err(err_to_string)
}

#[tauri::command]
fn load_draft_summary() -> Result<Option<report::CollectionOutput>, String> {
    store::load_draft_summary::<report::CollectionOutput>().map_err(err_to_string)
}

#[tauri::command]
fn clear_draft_summary() -> Result<(), String> {
    store::clear_draft_summary().map_err(err_to_string)
}

// ============================================================
// Settings
// ============================================================

#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    state::get_settings().map_err(err_to_string)
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    // 同步 language 给后端 i18n（前端会先调 set_app_language 命令，这里是兜底）
    i18n::set_language(&settings.language);
    state::save_settings(&settings).map_err(err_to_string)
}

/// 设置后端 i18n 语言（前端切换 UI 语言时调用，使 anyhow! 错误消息立刻跟上）。
#[tauri::command]
fn set_app_language(lang: String) {
    i18n::set_language(&lang);
}

// ============================================================
// SMTP
// ============================================================

#[tauri::command]
async fn get_smtp_config() -> Result<SmtpConfig, String> {
    state::get_smtp_config().map_err(err_to_string)
}

#[tauri::command]
async fn save_smtp_config(config: SmtpConfig) -> Result<(), String> {
    state::save_smtp_config(&config).map_err(err_to_string)
}

#[tauri::command]
async fn test_smtp_config(config: SmtpConfig) -> Result<String, String> {
    email::test_smtp(&config).await.map_err(err_to_string)
}

#[derive(Debug, Clone, Deserialize)]
struct TestEmailRequest {
    config: SmtpConfig,
    to: String,
}

#[tauri::command]
async fn send_test_email(req: TestEmailRequest) -> Result<String, String> {
    let recipient = req.to.trim().to_string();
    if recipient.is_empty() {
        return Err(i18n::t("err.email.recipient_empty"));
    }
    let time = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let body = EmailRequest {
        to: vec![recipient.clone()],
        cc: vec![],
        subject: i18n::t("email.test.subject"),
        body_markdown: i18n::t_var(
            "email.test.body",
            &[
                ("from", req.config.username.as_str()),
                ("to", recipient.as_str()),
                ("time", time.as_str()),
            ],
        ),
    };
    email::send(&req.config, &body)
        .await
        .map_err(err_to_string)?;
    Ok(i18n::t_var(
        "msg.test_email_sent",
        &[("recipient", recipient.as_str())],
    ))
}

// ============================================================
// Schedules
// ============================================================

#[derive(Debug, Serialize)]
struct ScheduleView {
    #[serde(flatten)]
    schedule: Schedule,
    /// 运行时计算的下次触发时间（仅展示用，不持久化）
    next_run_computed: Option<String>,
}

fn enrich_schedules(list: Vec<Schedule>) -> Vec<ScheduleView> {
    list.into_iter()
        .map(|s| {
            let next = if s.enabled {
                scheduler::next_run_time(&s.cron)
            } else {
                None
            };
            ScheduleView {
                schedule: s,
                next_run_computed: next,
            }
        })
        .collect()
}

#[tauri::command]
async fn list_schedules() -> Result<Vec<ScheduleView>, String> {
    let list = state::list_schedules().map_err(err_to_string)?;
    Ok(enrich_schedules(list))
}

#[tauri::command]
async fn save_schedule(
    scheduler: tauri::State<'_, SchedulerState>,
    schedule: Schedule,
) -> Result<Schedule, String> {
    let saved = state::save_schedule(schedule).map_err(err_to_string)?;
    scheduler.refresh_job(&saved).await.map_err(err_to_string)?;
    Ok(saved)
}

#[tauri::command]
async fn delete_schedule(
    scheduler: tauri::State<'_, SchedulerState>,
    id: String,
) -> Result<(), String> {
    scheduler.remove_job(&id).await.map_err(err_to_string)?;
    state::delete_schedule(&id).map_err(err_to_string)?;
    Ok(())
}

#[tauri::command]
async fn run_schedule_now(id: String) -> Result<String, String> {
    let sch = state::list_schedules()
        .map_err(err_to_string)?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| i18n::t_var("err.schedule.not_found", &[("id", id.as_str())]))?;
    scheduler::execute_schedule(&sch)
        .await
        .map_err(err_to_string)?;
    Ok(i18n::t_var(
        "msg.schedule_run_done",
        &[("name", sch.name.as_str())],
    ))
}
