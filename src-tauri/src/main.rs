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

/// 应用入口。
///
/// 启动顺序：
/// 1. 初始化 tracing 日志
/// 2. 初始化存储目录（创建 OS 配置目录 + `reports/` 子目录）
/// 3. 首次启动时创建默认本机工作区
/// 4. 启动 Tauri 主循环（command 注册由后续阶段补全，详见
///    `docs/ARCHITECTURE.md#310-mainrs--tauri-入口`）
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
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}

/// 初始化 tracing 日志。默认 INFO 级别，可通过 `RUST_LOG` 覆盖。
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

/// 健康检查 command，仅为阶段 0 验证前后端通路。
#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}
