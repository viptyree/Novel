use crate::{checked, read, value, Result, STOP_WAIT};
use novel_core::{self as core, Context, FileLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Generation {
    pub id: String,
    pub idea: String,
    pub target_words: u32,
    pub use_context: bool,
    pub overwrite: bool,
    pub pov: Option<String>,
    pub narrative: Option<String>,
    pub options: core::context::Options,
    pub prompt_override: Option<core::PromptOverride>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Generate(Generation),
    GenerateManual {
        id: String,
        prompts: core::PromptPair,
        overwrite: bool,
        input_budget: usize,
    },
    Check {
        id: String,
    },
    Memory {
        id: String,
    },
    Titles {
        id: String,
        previous_candidates: Vec<String>,
    },
}
#[derive(Serialize, Deserialize)]
struct Request {
    project: PathBuf,
    settings: core::Settings,
    action: Action,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct Status {
    pub id: String,
    pub project: PathBuf,
    pub state: String,
    pub stage: String,
    pub created_at: String,
    pub updated_at: String,
    pub result: Option<Value>,
    pub error: Option<String>,
}
impl Status {
    pub(crate) fn active(&self) -> bool {
        matches!(self.state.as_str(), "starting" | "running")
    }
}
pub(crate) fn action(data: &Path, id: &str) -> Result<Action> {
    let request: Request = serde_json::from_str(&read(&directory(data, id)?.join("request.json"))?)
        .map_err(|e| e.to_string())?;
    Ok(request.action)
}
fn directory(data: &Path, id: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(id).map_err(|_| "无效任务 ID".to_string())?;
    Ok(data.join("jobs").join(id))
}
fn store(dir: &Path, status: &Status) -> Result<()> {
    core::project_manager::atomic_json(&dir.join("status.json"), status)
}
fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn load(data: &Path, id: &str) -> Result<Status> {
    let dir = directory(data, id)?;
    let mut status: Status =
        serde_json::from_str(&read(&dir.join("status.json"))?).map_err(|e| e.to_string())?;
    if status.active() {
        let age = chrono::DateTime::parse_from_rfc3339(&status.created_at)
            .map(|t| chrono::Utc::now().signed_duration_since(t).num_seconds())
            .unwrap_or(31);
        if status.state == "running" || age > 30 {
            if let Ok(_lease) = FileLock::acquire(&dir.join("live.lock"), true) {
                // Re-read after taking the lease: the worker may have completed since the first read.
                status = serde_json::from_str(&read(&dir.join("status.json"))?)
                    .map_err(|e| e.to_string())?;
                if !status.active() {
                    return Ok(status);
                }
                // Lock protects reconciliation from a still-live worker, not just a PID (which may be reused).
                status.state = "interrupted".into();
                status.error =
                    Some("worker 已退出或主机重启；不会自动重跑，请先检查已保存内容".into());
                status.updated_at = timestamp();
                store(&dir, &status)?;
            }
        }
    }
    Ok(status)
}
pub fn list(data: &Path) -> Result<Value> {
    let mut entries = Vec::new();
    let root = data.join("jobs");
    if root.is_dir() {
        for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.path().is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if uuid::Uuid::parse_str(&id).is_err() {
                continue;
            }
            match load(data, &id) {
                Ok(status) => entries.push(status),
                Err(error) => return Err(format!("任务 {id}: {error}")),
            }
        }
    }
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    value(entries)
}
pub fn submit(ctx: Context, action: Action, detach: bool) -> Result<Value> {
    // Fast preflight; worker acquires the actual operation lock before announcing running.
    {
        let _check = ctx.operation_access()?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let dir = directory(&ctx.data_dir, &id)?;
    let staging = ctx.data_dir.join("jobs").join(format!(".submit-{id}"));
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let mut settings = ctx.settings.clone();
    settings.llm_api_key.clear();
    core::project_manager::atomic_json(
        &staging.join("request.json"),
        &Request {
            project: ctx.project_dir.clone(),
            settings,
            action,
        },
    )?;
    let now = timestamp();
    let mut status = Status {
        id: id.clone(),
        project: ctx.project_dir.clone(),
        state: "starting".into(),
        stage: "启动 worker".into(),
        created_at: now.clone(),
        updated_at: now,
        result: None,
        error: None,
    };
    store(&staging, &status)?;
    fs::rename(&staging, &dir).map_err(|e| e.to_string())?;
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("worker.log"))
        .map_err(|e| e.to_string())?;
    command
        .arg("--data-dir")
        .arg(&ctx.data_dir)
        .arg("__worker")
        .arg(&id)
        .current_dir(&ctx.data_dir)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().map_err(|e| e.to_string())?)
        .stderr(stdout);
    detach_process(&mut command);
    if let Err(e) = command.spawn() {
        status.state = "failed".into();
        status.error = Some(format!("无法启动独立 worker（未降级为会话子进程）：{e}"));
        store(&dir, &status)?;
        return Err(status.error.unwrap());
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        status = load(&ctx.data_dir, &id)?;
        if status.state != "starting" {
            break;
        }
        if Instant::now() > deadline {
            return Err(format!(
                "worker 尚未确认启动，任务 {id}；请用 novel job show {id} 检查，勿重复提交"
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if status.state == "failed" || status.state == "interrupted" {
        return Err(status.error.unwrap_or("任务启动失败".into()));
    }
    if detach {
        value(status)
    } else {
        eprintln!("任务 {id}；Ctrl+C 仅停止等待，任务继续运行。");
        wait(&ctx.data_dir, &id)
    }
}
#[cfg(windows)]
fn detach_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    // DETACHED_PROCESS + CREATE_BREAKAWAY_FROM_JOB. All SSH handles above are replaced.
    command.creation_flags(0x00000008 | 0x01000000);
}
#[cfg(unix)]
fn detach_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: only async-signal-safe libc calls run between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            Ok(())
        });
    }
}

pub fn worker(data: &Path, id: &str) -> Result<()> {
    let dir = directory(data, id)?;
    // Only the lease owner may publish initialization failures.
    let _lease = FileLock::acquire(&dir.join("live.lock"), true)?;
    let result = worker_inner(data, id);
    if let Err(error) = &result {
        if let Ok(dir) = directory(data, id) {
            if let Ok(text) = read(&dir.join("status.json")) {
                if let Ok(mut status) = serde_json::from_str::<Status>(&text) {
                    if status.active() {
                        status.state = "failed".into();
                        status.error = Some(error.clone());
                        status.updated_at = timestamp();
                        let _ = store(&dir, &status);
                    }
                }
            }
        }
    }
    result
}
fn worker_inner(data: &Path, id: &str) -> Result<()> {
    let dir = directory(data, id)?;
    let request: Request =
        serde_json::from_str(&read(&dir.join("request.json"))?).map_err(|e| e.to_string())?;
    let mut ctx = Context::new(data.to_owned(), Some(request.project))?;
    let secret = ctx.settings.llm_api_key.clone();
    ctx.settings = request.settings;
    ctx.settings.llm_api_key = secret.clone();
    let mut initial: Status =
        serde_json::from_str(&read(&dir.join("status.json"))?).map_err(|e| e.to_string())?;
    if initial.state != "starting" {
        return Err("任务已经启动或结束，不会重复执行".into());
    }
    let committing = Arc::new(AtomicBool::new(false));
    let callback_dir = dir.clone();
    let callback_committing = committing.clone();
    ctx = ctx.with_progress(Arc::new(move |stage| {
        if !callback_committing.load(Ordering::Relaxed) && callback_dir.join("cancel").exists() {
            return Err("任务已取消".into());
        }
        if stage == "保存结果" {
            callback_committing.store(true, Ordering::Relaxed);
        }
        let mut status: Status = serde_json::from_str(&read(&callback_dir.join("status.json"))?)
            .map_err(|e| e.to_string())?;
        status.state = "running".into();
        status.stage = stage.into();
        status.updated_at = timestamp();
        store(&callback_dir, &status)?;
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(callback_dir.join("progress.log"))
            .map_err(|e| e.to_string())?;
        writeln!(log, "{} {stage}", timestamp()).map_err(|e| e.to_string())
    }));
    // A panic becomes a persisted failure. Interrupted workers are reconciled by liveness locks.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match request.action {
        Action::Generate(g) => checked(core::generate_chapter(
            ctx,
            g.id,
            g.idea,
            g.target_words,
            g.use_context,
            g.overwrite,
            g.pov,
            g.narrative,
            g.prompt_override,
            Some(g.options),
        )?),
        Action::GenerateManual { id, prompts, overwrite, input_budget } =>
            checked(core::generate_manual_chapter(ctx, id, prompts, overwrite, input_budget)?),
        Action::Check { id } => checked(core::check_consistency(ctx, id)?),
        Action::Memory { id } => checked(core::update_memory(ctx, id)?),
        Action::Titles {
            id,
            previous_candidates,
        } => {
            let content = core::read_chapter(ctx.clone(), id.clone())?;
            let root = ctx.project_dir.display().to_string();
            value(core::chapter_titles::generate_chapter_titles(
                ctx,
                root,
                id,
                content,
                previous_candidates,
            )?)
        }
    }))
    .unwrap_or_else(|_| Err("worker 内部异常；请检查已保存结果".into()));
    initial.updated_at = timestamp();
    match result {
        Ok(result) => {
            initial.state = "succeeded".into();
            initial.stage = "完成".into();
            initial.result = Some(result);
        }
        Err(error) => {
            initial.state = if error.contains("任务已取消") {
                "cancelled"
            } else {
                "failed"
            }
            .into();
            initial.stage = "结束".into();
            initial.error = Some(if secret.is_empty() {
                error
            } else {
                error.replace(&secret, "[hidden]")
            });
        }
    }
    store(&dir, &initial)
}
pub fn wait(data: &Path, id: &str) -> Result<Value> {
    let started = Instant::now();
    let mut last = String::new();
    loop {
        if STOP_WAIT.load(Ordering::Relaxed) {
            return Err(format!("等待已中断；任务 {id} 继续运行"));
        }
        let status = load(data, id)?;
        if status.stage != last {
            eprintln!("[{}s] {}", started.elapsed().as_secs(), status.stage);
            last = status.stage.clone();
        }
        if !status.active() {
            return if status.state == "succeeded" {
                Ok(status.result.unwrap_or(json!({"success":true})))
            } else {
                Err(status
                    .error
                    .unwrap_or_else(|| format!("任务 {}", status.state)))
            };
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
pub fn cancel(data: &Path, id: &str) -> Result<()> {
    let status = load(data, id)?;
    if !status.active() {
        return Err("任务已经结束".into());
    }
    core::atomic_write(&directory(data, id)?.join("cancel"), b"cancel")
}
pub fn logs(data: &Path, id: &str, follow: bool) -> Result<Value> {
    let dir = directory(data, id)?;
    if follow {
        let mut offset = 0;
        loop {
            let text = fs::read_to_string(dir.join("progress.log")).unwrap_or_default();
            if text.len() > offset {
                eprint!("{}", &text[offset..]);
                offset = text.len();
            }
            let status = load(data, id)?;
            if STOP_WAIT.load(Ordering::Relaxed) {
                return Err("等待已中断；已停止跟随日志，任务继续运行".into());
            }
            if !status.active() {
                return value(status);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    let _ = load(data, id)?;
    Ok(Value::String(
        fs::read_to_string(dir.join("progress.log")).unwrap_or_default(),
    ))
}
