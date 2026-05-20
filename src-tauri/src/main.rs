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
mod workspace;

use chrono::Local;
use email::{EmailRequest, SmtpConfig};
use llm::LlmProvider;
use report::{ReportRecord, Template};
use serde::{Deserialize, Serialize};
use state::Settings;
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
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
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

#[derive(Debug, Serialize)]
struct GenerateResponse {
    record: ReportRecord,
    content: String,
    duration_ms: u64,
}

#[tauri::command]
async fn generate_report(req: GenerateRequest) -> Result<GenerateResponse, String> {
    generate_impl(req).await.map_err(err_to_string)
}

async fn generate_impl(req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
    use anyhow::{anyhow, bail};

    // 1. 模板
    let template = state::list_templates()?
        .into_iter()
        .find(|t| t.id == req.template_id)
        .ok_or_else(|| anyhow!("模板不存在: {}", req.template_id))?;

    // 2. 解析 provider（按 LLM.md §6 优先级）
    let provider = resolve_provider(req.provider_id.as_deref(), template.provider_id.as_deref())?;

    // 3. 工作区
    let all_ws = state::list_workspaces()?;
    let workspaces: Vec<Workspace> = all_ws
        .into_iter()
        .filter(|w| req.workspace_ids.iter().any(|id| id == &w.id))
        .collect();
    if workspaces.is_empty() {
        bail!("未选中任何工作区");
    }

    // 4. 设置
    let settings = state::get_settings()?;
    let clip = settings.prompt_clip_chars as usize;

    // 5. 收集 messages（多 workspace 串行，错误不阻塞）
    let mut messages = Vec::new();
    for ws in &workspaces {
        match logs::collect_messages(ws, req.days, clip).await {
            Ok(part) => messages.extend(part),
            Err(e) => tracing::warn!("workspace {} 收集日志失败: {:#}", ws.name, e),
        }
    }

    // 6. 聚合
    let summary = logs::aggregate(messages);

    // 7. 历史报告作为风格参考
    let past = load_past_reports(settings.past_reports_context as usize)?;

    // 8. 生成
    let (markdown, tokens, duration_ms) =
        report::generate(&summary, &template, &past, &provider).await?;

    // 9. 存档
    let record = ReportRecord {
        id: String::new(),
        week: format!("最近 {} 天", req.days),
        template_id: template.id.clone(),
        template_name: template.name.clone(),
        provider_id: Some(provider.id.clone()),
        provider_name: Some(provider.name.clone()),
        tokens_used: tokens,
        project_count: summary.stats.project_count,
        generated_at: Local::now().to_rfc3339(),
    };
    let saved = state::save_report(record, &markdown)?;

    Ok(GenerateResponse {
        record: saved,
        content: markdown,
        duration_ms,
    })
}

/// 按 docs/LLM.md §6 的优先级解析 provider：
/// 显式 > 模板 > 默认 > 第一个 > 报错。
fn resolve_provider(
    explicit: Option<&str>,
    template_pid: Option<&str>,
) -> anyhow::Result<LlmProvider> {
    let providers = state::list_providers()?;

    if let Some(id) = explicit {
        if !id.is_empty() {
            if let Some(p) = providers.iter().find(|p| p.id == id) {
                return Ok(p.clone());
            }
            return Err(anyhow::anyhow!("指定的 LLM 源不存在: {id}"));
        }
    }
    if let Some(id) = template_pid {
        if !id.is_empty() {
            if let Some(p) = providers.iter().find(|p| p.id == id) {
                return Ok(p.clone());
            }
            // 模板指定的 provider 已被删除 → 回退到默认（不报错）
        }
    }
    state::get_default_provider()
}

/// 取最近 `n` 份历史报告的 Markdown 正文。失败的单条 warn! 后跳过。
fn load_past_reports(n: usize) -> anyhow::Result<Vec<String>> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut records = state::list_reports()?;
    records.sort_by(|a, b| b.generated_at.cmp(&a.generated_at));
    let mut out = Vec::new();
    for r in records.into_iter().take(n) {
        match store::load_report_file(&r.id) {
            Ok(s) => out.push(s),
            Err(e) => tracing::warn!("加载历史报告 {} 失败: {:#}", r.id, e),
        }
    }
    Ok(out)
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
