// 防止 Windows release 模式弹控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tracing_subscriber::EnvFilter;

mod email;
mod llm;
mod logs;
mod report;
mod scheduler;
mod state;
mod store;
mod workspace;

use llm::LlmProvider;
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
