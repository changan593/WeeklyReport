// 防止 Windows release 模式弹控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tracing_subscriber::EnvFilter;

mod email;
mod llm;
mod logs;
mod report;
mod scheduler;
mod ssh;
mod state;
mod store;
mod validate;
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

    tauri::Builder::default()
        // 插件初始化保留以便将来扩展（如 SSH 私钥文件选择器），但前端目前未调用
        // 任何插件 API；`capabilities/default.json` 中也未授予 shell:/dialog:/fs:/
        // clipboard-manager: 权限，因此即便前端被 XSS 也无法触发这些能力。
        // 复制 Markdown 到剪贴板用浏览器原生 `navigator.clipboard.writeText`。
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
            delete_report,
            generate_report,
            // Settings
            get_settings,
            save_settings,
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
    validate::id(&id).map_err(err_to_string)?;
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
    validate::id(&id).map_err(err_to_string)?;
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
    validate::id(&id).map_err(err_to_string)?;
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
    validate::id(&id).map_err(err_to_string)?;
    let (record, content) = state::get_report(&id).map_err(err_to_string)?;
    Ok(ReportPayload { record, content })
}

#[tauri::command]
async fn delete_report(id: String) -> Result<(), String> {
    validate::id(&id).map_err(err_to_string)?;
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

// ============================================================
// Settings
// ============================================================

#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    state::get_settings().map_err(err_to_string)
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    state::save_settings(&settings).map_err(err_to_string)
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
        return Err("收件人为空".into());
    }
    // 收件人在送 lettre 之前先做白名单校验
    validate::email(&recipient).map_err(err_to_string)?;
    let body = EmailRequest {
        to: vec![recipient.clone()],
        cc: vec![],
        subject: "WeeklyReport 测试邮件".into(),
        body_markdown: format!(
            "# WeeklyReport 测试邮件\n\n如果你看到这封邮件，说明 SMTP 已配置成功。\n\n- 发件人：`{}`\n- 收件人：`{}`\n- 时间：{}\n",
            req.config.username,
            recipient,
            Local::now().format("%Y-%m-%d %H:%M:%S")
        ),
    };
    email::send(&req.config, &body)
        .await
        .map_err(err_to_string)?;
    Ok(format!("✓ 测试邮件已发送到 {recipient}"))
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
    validate::id(&id).map_err(err_to_string)?;
    scheduler.remove_job(&id).await.map_err(err_to_string)?;
    state::delete_schedule(&id).map_err(err_to_string)?;
    Ok(())
}

#[tauri::command]
async fn run_schedule_now(id: String) -> Result<String, String> {
    validate::id(&id).map_err(err_to_string)?;
    let sch = state::list_schedules()
        .map_err(err_to_string)?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("任务不存在: {id}"))?;
    scheduler::execute_schedule(&sch)
        .await
        .map_err(err_to_string)?;
    Ok(format!("✓ 任务「{}」立即执行完成", sch.name))
}
