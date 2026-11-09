use super::*;
use std::sync::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

// Readers hold this for the entire operation, including background generation.
// Switching uses try_write so it fails immediately while a task is running.
static PROJECT_ACCESS: RwLock<()> = RwLock::new(());
static REGISTRY_ACCESS: Mutex<()> = Mutex::new(());

pub fn read_access() -> Result<RwLockReadGuard<'static, ()>, String> {
    PROJECT_ACCESS.read().map_err(|_| "项目状态不可用，请重启应用".into())
}

pub fn switch_access() -> Result<RwLockWriteGuard<'static, ()>, String> {
    PROJECT_ACCESS.try_write().map_err(|_| "项目正在执行操作，请完成后再切换".into())
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ProjectEntry {
    pub name: String,
    pub path: String,
    pub first_opened_at: String,
    pub last_opened_at: String,
    pub pinned: bool,
}

#[derive(Serialize)]
pub struct ProjectView {
    #[serde(flatten)]
    pub entry: ProjectEntry,
    pub exists: bool,
    pub current: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Registry {
    projects: Vec<ProjectEntry>,
}

fn registry_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("projects.json"))
}

fn load(app: &tauri::AppHandle) -> Result<Registry, String> {
    match fs::read_to_string(registry_path(app)?) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("项目列表损坏，请保留文件后修复：{e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, serde_json::to_string_pretty(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(&temp, path).map_err(|e| e.to_string())
}

fn normalized(path: &Path) -> PathBuf {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // Keep comparisons stable after a Windows directory goes missing: canonicalize
    // adds a verbatim prefix while the legacy settings path may not have one.
    #[cfg(windows)]
    {
        let text = path.to_string_lossy().replace('/', "\\");
        if let Some(tail) = text.strip_prefix("\\\\?\\UNC\\") {
            return PathBuf::from(format!("\\\\{tail}"));
        }
        return PathBuf::from(text.strip_prefix("\\\\?\\").unwrap_or(&text));
    }
    #[cfg(not(windows))]
    path
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    let a = normalized(a);
    let b = normalized(b);
    if cfg!(windows) {
        a.to_string_lossy().trim_end_matches('\\').to_lowercase()
            == b.to_string_lossy().trim_end_matches('\\').to_lowercase()
    } else {
        a == b
    }
}

fn register(registry: &mut Registry, path: &Path, name: Option<&str>, opened: bool) {
    let now = chrono::Utc::now().to_rfc3339();
    if let Some(entry) = registry.projects.iter_mut().find(|e| same_path(Path::new(&e.path), path)) {
        if opened { entry.last_opened_at = now; }
        if let Some(name) = name.filter(|s| !s.trim().is_empty()) { entry.name = name.trim().into(); }
        return;
    }
    registry.projects.push(ProjectEntry {
        name: name.filter(|s| !s.trim().is_empty()).map(|s| s.trim().to_owned())
            .unwrap_or_else(|| path.file_name().unwrap_or_default().to_string_lossy().into_owned()),
        path: path.to_string_lossy().into_owned(),
        first_opened_at: now.clone(), last_opened_at: now, pinned: false,
    });
}

fn absolute(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() { return Err("请填写项目的完整绝对路径".into()); }
    Ok(path)
}

fn recognizable(path: &Path) -> bool {
    ["memory", "outlines", "chapters", "正文"].iter().any(|p| path.join(p).is_dir())
        || ["config.yaml", ".initialized", ".chapter-source.json"].iter().any(|p| path.join(p).is_file())
}

pub fn remember_startup(app: &tauri::AppHandle) -> Result<(), String> {
    let _access = switch_access()?;
    let _registry = REGISTRY_ACCESS.lock().map_err(|e| e.to_string())?;
    let mut registry = load(app)?;
    let mut settings = load_settings(app);
    let current = resolve_project_dir(app);
    // Initialize the default once, then persist its path so a deleted default
    // is treated as missing rather than silently recreated on later launches.
    if settings.project_dir.trim().is_empty() {
        let known = registry.projects.iter().any(|e| same_path(Path::new(&e.path), &current));
        if current.is_dir() || !known {
            fs::create_dir_all(&current).map_err(|e| e.to_string())?;
            ensure_project_initialized(&current, app)?;
        }
        settings.project_dir = normalized(&current).to_string_lossy().into_owned();
        atomic_json(&settings_path(app), &settings)?;
    }
    register(&mut registry, &normalized(&current), None, current.is_dir());
    atomic_json(&registry_path(app)?, &registry)
}

#[tauri::command]
pub fn list_projects(app: tauri::AppHandle) -> Result<Vec<ProjectView>, String> {
    let _access = read_access()?;
    let _registry = REGISTRY_ACCESS.lock().map_err(|e| e.to_string())?;
    let mut registry = load(&app)?;
    let current = resolve_project_dir(&app);
    // Migrate the old single path even when its directory has gone missing.
    // Reading the list never creates or initializes a project directory.
    register(&mut registry, &normalized(&current), None, false);
    atomic_json(&registry_path(&app)?, &registry)?;
    registry.projects.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.last_opened_at.cmp(&a.last_opened_at)));
    Ok(registry.projects.into_iter().map(|entry| ProjectView {
        exists: Path::new(&entry.path).is_dir(), current: same_path(Path::new(&entry.path), &current), entry,
    }).collect())
}

#[tauri::command]
pub fn open_project(app: tauri::AppHandle, path: String, create: bool, name: Option<String>, previous_path: Option<String>) -> Result<String, String> {
    let _access = switch_access()?;
    let _registry = REGISTRY_ACCESS.lock().map_err(|e| e.to_string())?;
    let mut registry = load(&app)?;
    let requested = absolute(&path)?;
    if create {
        if requested.exists() {
            if !requested.is_dir() || fs::read_dir(&requested).map_err(|e| e.to_string())?.next().is_some() {
                return Err("新项目需要不存在的目录或空目录；已有项目请使用打开项目".into());
            }
        } else {
            fs::create_dir_all(&requested).map_err(|e| e.to_string())?;
        }
    } else if !requested.is_dir() {
        return Err("项目目录不存在，请重新定位目录".into());
    } else if !recognizable(&requested) {
        return Err("此目录未识别到小说项目，请选择包含正文、memory、outlines 或 config.yaml 的目录".into());
    }
    let root = normalized(&fs::canonicalize(&requested).map_err(|e| e.to_string())?);
    if let Some(previous) = &previous_path {
        if !registry.projects.iter().any(|e| e.path == *previous) { return Err("待重新定位的项目记录不存在".into()); }
        if registry.projects.iter().any(|e| e.path != *previous && same_path(Path::new(&e.path), &root)) {
            return Err("目标目录已在项目列表中，请直接打开该项目".into());
        }
    }
    ensure_project_initialized(&root, &app)?;
    // Preserve name and pin when repairing a moved directory.
    if let Some(previous) = previous_path {
        let entry = registry.projects.iter_mut().find(|e| e.path == previous).ok_or("项目记录不存在")?;
        entry.path = root.to_string_lossy().into_owned();
    }
    register(&mut registry, &root, name.as_deref(), true);
    let mut settings = load_settings(&app);
    settings.project_dir = root.to_string_lossy().into_owned();
    // Publish the active path last, after validation and registry persistence.
    atomic_json(&registry_path(&app)?, &registry)?;
    atomic_json(&settings_path(&app), &settings)?;
    Ok(settings.project_dir)
}

#[tauri::command]
pub fn update_project_entry(app: tauri::AppHandle, path: String, pinned: Option<bool>, remove: bool) -> Result<(), String> {
    let _access = read_access()?;
    let _registry = REGISTRY_ACCESS.lock().map_err(|e| e.to_string())?;
    let mut registry = load(&app)?;
    if remove {
        if same_path(Path::new(&path), &resolve_project_dir(&app)) { return Err("请先切换到其他项目，再移除当前项目记录".into()); }
        registry.projects.retain(|e| e.path != path);
    } else {
        let entry = registry.projects.iter_mut().find(|e| e.path == path).ok_or("项目记录不存在")?;
        if let Some(pinned) = pinned { entry.pinned = pinned; }
    }
    atomic_json(&registry_path(&app)?, &registry)
}
