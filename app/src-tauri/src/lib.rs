// Tauri IPC adapters. All business logic lives in novel-core.
use novel_core::*;
use novel_core::{chapter_titles::ChapterDetails, project_manager::ProjectView};
use novel_core::{context, project_manager, project_source};
use tauri::Manager;
struct SessionProject(std::sync::Mutex<std::path::PathBuf>);

fn engine(app: &tauri::AppHandle) -> Result<Context, String> {
    let project = app
        .try_state::<SessionProject>()
        .map(|state| state.0.lock().map(|p| p.clone()).map_err(|e| e.to_string()))
        .transpose()?;
    Context::new(
        app.path().app_data_dir().map_err(|e| e.to_string())?,
        project,
    )
}

#[tauri::command]
fn context_catalog(app: tauri::AppHandle, chapter_id: String) -> Result<context::Catalog, String> {
    let context = engine(&app)?;
    novel_core::context_catalog(context, chapter_id)
}

#[tauri::command]
fn get_source_settings(app: tauri::AppHandle) -> Result<project_source::SourceSettings, String> {
    let context = engine(&app)?;
    novel_core::get_source_settings(context)
}

#[tauri::command]
fn save_source_settings(
    app: tauri::AppHandle,
    settings: project_source::SourceSettings,
    expected: Option<String>,
) -> Result<(), String> {
    let context = engine(&app)?;
    match expected {
        Some(before) => novel_core::save_source_checked(context, settings, before),
        None => novel_core::save_source_settings(context, settings),
    }
}

#[tauri::command]
fn get_prompt_settings(app: tauri::AppHandle) -> Result<PromptSettings, String> {
    let context = engine(&app)?;
    novel_core::get_prompt_settings(context)
}

#[tauri::command]
fn default_prompt_settings() -> PromptSettings {
    novel_core::default_prompt_settings()
}

#[tauri::command]
fn save_prompt_settings(
    app: tauri::AppHandle,
    settings: PromptSettings,
    expected: Option<serde_json::Value>,
) -> Result<(), String> {
    let context = engine(&app)?;
    match expected {
        Some(before) => novel_core::save_prompt_checked(context, settings, before),
        None => novel_core::save_prompt_settings(context, settings),
    }
}

#[tauri::command]
fn preview_generation_prompt(
    app: tauri::AppHandle,
    chapter_id: String,
    idea: String,
    target_words: u32,
    use_context: bool,
    pov: String,
    narrative: String,
    context_options: Option<context::Options>,
) -> Result<PromptPreview, String> {
    let context = engine(&app)?;
    novel_core::preview_generation_prompt(
        context,
        chapter_id,
        idea,
        target_words,
        use_context,
        pov,
        narrative,
        context_options,
    )
}

#[tauri::command]
fn get_pipeline_root(app: tauri::AppHandle) -> Result<String, String> {
    let context = engine(&app)?;
    novel_core::get_pipeline_root(context)
}

#[tauri::command]
fn list_chapters(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let context = engine(&app)?;
    novel_core::list_chapters(context)
}

#[tauri::command]
fn read_chapter(app: tauri::AppHandle, chapter_id: String) -> Result<String, String> {
    let context = engine(&app)?;
    novel_core::read_chapter(context, chapter_id)
}

#[tauri::command]
fn save_chapter(
    app: tauri::AppHandle,
    chapter_id: String,
    content: String,
    expected: Option<String>,
) -> Result<(), String> {
    let context = engine(&app)?;
    match expected {
        Some(before) => novel_core::save_chapter_expected(context, chapter_id, content, before),
        None => novel_core::save_chapter(context, chapter_id, content),
    }
}

#[tauri::command]
fn read_report(app: tauri::AppHandle, chapter_id: String, kind: String) -> Result<String, String> {
    let context = engine(&app)?;
    novel_core::read_report(context, chapter_id, kind)
}

#[tauri::command]
fn read_memory_file(app: tauri::AppHandle, rel: String) -> Result<String, String> {
    let context = engine(&app)?;
    novel_core::read_memory_file(context, rel)
}

#[tauri::command]
fn save_memory_file(
    app: tauri::AppHandle,
    rel: String,
    content: String,
    expected: Option<String>,
) -> Result<(), String> {
    let context = engine(&app)?;
    match expected {
        Some(before) => novel_core::save_memory_expected(context, rel, content, before),
        None => novel_core::save_memory_file(context, rel, content),
    }
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    let context = engine(&app)?;
    novel_core::get_settings(context)
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    settings: Settings,
    expected: Option<Settings>,
) -> Result<(), String> {
    let context = engine(&app)?;
    match expected {
        Some(before) => novel_core::save_settings_checked(context, settings, before).map(|_| ()),
        None => novel_core::save_settings(context, settings),
    }
}

#[tauri::command]
async fn generate_chapter(
    app: tauri::AppHandle,
    chapter_id: String,
    idea: String,
    target_words: u32,
    use_context: bool,
    overwrite: bool,
    pov: Option<String>,
    narrative: Option<String>,
    prompt_override: Option<PromptOverride>,
    context_options: Option<context::Options>,
) -> Result<CommandResult, String> {
    let context = engine(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        novel_core::generate_chapter(
            context,
            chapter_id,
            idea,
            target_words,
            use_context,
            overwrite,
            pov,
            narrative,
            prompt_override,
            context_options,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn generate_chapter_manual(
    app: tauri::AppHandle,
    chapter_id: String,
    prompts: PromptPair,
    overwrite: bool,
    input_budget: u32,
) -> Result<CommandResult, String> {
    let context = engine(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        novel_core::generate_manual_chapter(context, chapter_id, prompts, overwrite, input_budget as usize)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn check_consistency(
    app: tauri::AppHandle,
    chapter_id: String,
) -> Result<CommandResult, String> {
    let context = engine(&app)?;
    tauri::async_runtime::spawn_blocking(move || novel_core::check_consistency(context, chapter_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn update_memory(app: tauri::AppHandle, chapter_id: String) -> Result<CommandResult, String> {
    let context = engine(&app)?;
    tauri::async_runtime::spawn_blocking(move || novel_core::update_memory(context, chapter_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn reset_chapter(
    app: tauri::AppHandle,
    chapter_id: String,
    include_memory: bool,
) -> Result<CommandResult, String> {
    let context = engine(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        novel_core::reset_chapter(context, chapter_id, include_memory)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn list_projects(app: tauri::AppHandle) -> Result<Vec<ProjectView>, String> {
    let context = engine(&app)?;
    novel_core::project_manager::list_projects(context)
}

#[tauri::command]
fn open_project(
    app: tauri::AppHandle,
    path: String,
    create: bool,
    name: Option<String>,
    previous_path: Option<String>,
) -> Result<String, String> {
    let context = engine(&app)?;
    let root =
        novel_core::project_manager::open_project(context, path, create, name, previous_path)?;
    if let Some(state) = app.try_state::<SessionProject>() {
        *state.0.lock().map_err(|e| e.to_string())? = root.clone().into();
    }
    Ok(root)
}

#[tauri::command]
fn update_project_entry(
    app: tauri::AppHandle,
    path: String,
    pinned: Option<bool>,
    remove: bool,
) -> Result<(), String> {
    let context = engine(&app)?;
    novel_core::project_manager::update_project_entry(context, path, pinned, remove)
}

#[tauri::command]
fn list_chapter_details(app: tauri::AppHandle) -> Result<Vec<ChapterDetails>, String> {
    let context = engine(&app)?;
    novel_core::chapter_titles::list_chapter_details(context)
}

#[tauri::command]
async fn generate_chapter_titles(
    app: tauri::AppHandle,
    project_root: String,
    chapter_id: String,
    content: String,
    previous_candidates: Vec<String>,
) -> Result<Vec<String>, String> {
    let context = engine(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        novel_core::chapter_titles::generate_chapter_titles(
            context,
            project_root,
            chapter_id,
            content,
            previous_candidates,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn save_chapter_title(
    app: tauri::AppHandle,
    project_root: String,
    chapter_id: String,
    title: String,
    expected_path: String,
) -> Result<ChapterDetails, String> {
    let context = engine(&app)?;
    novel_core::chapter_titles::save_chapter_title(
        context,
        project_root,
        chapter_id,
        title,
        expected_path,
    )
}

#[tauri::command]
fn open_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Keep the project picker available even when a previous directory
            // or the registry is unavailable; its UI reports the concrete error.
            if let Err(error) =
                engine(app.handle()).and_then(|ctx| project_manager::remember_startup(&ctx))
            {
                eprintln!("Project startup: {error}");
            }
            if let Ok(context) = engine(app.handle()) {
                app.manage(SessionProject(std::sync::Mutex::new(context.project_dir)));
            }
            // 运行时显式设置窗口/任务栏图标，确保 dev 与正式版都显示新 logo
            // include_image! 在编译期嵌入；图标文件变化会强制重新编译。
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_icon(tauri::include_image!("icons/icon.png"));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            open_project,
            update_project_entry,
            list_chapter_details,
            generate_chapter_titles,
            save_chapter_title,
            get_pipeline_root,
            list_chapters,
            read_chapter,
            save_chapter,
            read_report,
            read_memory_file,
            save_memory_file,
            get_settings,
            save_settings,
            context_catalog,
            get_source_settings,
            save_source_settings,
            get_prompt_settings,
            default_prompt_settings,
            save_prompt_settings,
            preview_generation_prompt,
            generate_chapter,
            generate_chapter_manual,
            check_consistency,
            update_memory,
            reset_chapter,
            open_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
