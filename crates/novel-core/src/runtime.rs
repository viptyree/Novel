use crate::{project_manager, project_source, Settings};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const APP_ID: &str = "com.yuxin.novel-pipeline";
type Progress = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// A submitted operation always retains its project and settings, even if another UI switches projects.
#[derive(Clone)]
pub struct Context {
    pub data_dir: PathBuf,
    pub project_dir: PathBuf,
    pub settings: Settings,
    progress: Option<Progress>,
}
impl Context {
    pub fn default_data_dir() -> Result<PathBuf, String> {
        dirs::data_dir()
            .map(|p| p.join(APP_ID))
            .ok_or("无法定位用户数据目录".into())
    }
    pub fn read_settings(data_dir: &Path) -> Result<Settings, String> {
        match fs::read_to_string(data_dir.join(".ui-settings.json")) {
            Ok(s) => serde_json::from_str(&s).map_err(|e| format!("设置文件损坏：{e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn new(data_dir: PathBuf, project: Option<PathBuf>) -> Result<Self, String> {
        let mut settings = Self::read_settings(&data_dir)?;
        let project_dir = project.unwrap_or_else(|| {
            if settings.project_dir.trim().is_empty() {
                data_dir.join("novel-project")
            } else {
                PathBuf::from(&settings.project_dir)
            }
        });
        let project_dir = if project_dir.is_absolute() {
            project_dir
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(project_dir)
        };
        let project_dir = project_manager::normalized(&project_dir);
        settings.project_dir = project_dir.to_string_lossy().into_owned();
        Ok(Self {
            data_dir,
            project_dir,
            settings,
            progress: None,
        })
    }
    pub fn data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.data_dir.clone())
    }
    pub fn with_progress(mut self, progress: Progress) -> Self {
        self.progress = Some(progress);
        self
    }
    pub fn stage(&self, stage: &str) -> Result<(), String> {
        if let Some(callback) = &self.progress {
            callback(stage)?;
        }
        Ok(())
    }
    pub fn global_access(&self) -> Result<FileLock, String> {
        fs::create_dir_all(&self.data_dir).map_err(|e| e.to_string())?;
        FileLock::acquire(&self.data_dir.join(".novel-settings.lock"), true)
    }
    fn resources(&self) -> Result<Vec<PathBuf>, String> {
        if !self.project_dir.is_dir() {
            return Err("项目目录不存在，请重新定位或新建项目".into());
        }
        let mut paths = vec![self.project_dir.clone()];
        let source = project_source::source_dir(&self.project_dir)?;
        if source.is_dir() {
            paths.push(project_manager::normalized(&source));
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
    fn lock_resources(
        &self,
        name: &str,
        exclusive: bool,
        wait: bool,
    ) -> Result<Vec<FileLock>, String> {
        loop {
            let paths = self.resources()?;
            let locks: Result<Vec<_>, _> = paths
                .iter()
                .map(|p| {
                    if wait {
                        FileLock::acquire_wait(&p.join(name), exclusive)
                    } else {
                        FileLock::acquire(&p.join(name), exclusive)
                    }
                })
                .collect();
            let locks = locks?;
            // The source setting may have changed before the root lock was acquired.
            // Re-resolve while holding it; retry in global path order if necessary.
            if paths == self.resources()? {
                return Ok(locks);
            }
        }
    }
    /// Held during model work: prevents writers, but does not block readers of saved files.
    pub fn operation_access(&self) -> Result<Vec<FileLock>, String> {
        self.lock_resources(".novel-operation.lock", true, false)
    }
    pub fn read_access(&self) -> Result<Vec<FileLock>, String> {
        self.lock_resources(".novel-commit.lock", false, true)
    }
    pub fn commit_access(&self) -> Result<Vec<FileLock>, String> {
        self.lock_resources(".novel-commit.lock", true, true)
    }
    pub fn write_access(&self) -> Result<(Vec<FileLock>, Vec<FileLock>), String> {
        Ok((self.operation_access()?, self.commit_access()?))
    }
}

/// OS advisory locks are released automatically on process death; lock files are never deleted.
pub struct FileLock(File);
impl FileLock {
    pub fn acquire_wait(path: &Path, exclusive: bool) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if exclusive {
            FileExt::lock_exclusive(&file)
        } else {
            FileExt::lock_shared(&file)
        }
        .map_err(|e| e.to_string())?;
        Ok(Self(file))
    }
    pub fn acquire(path: &Path, exclusive: bool) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let result = if exclusive {
            FileExt::try_lock_exclusive(&file)
        } else {
            FileExt::try_lock_shared(&file)
        };
        result.map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.raw_os_error() == fs2::lock_contended_error().raw_os_error()
            {
                format!("资源忙：{}，请等待当前操作结束", path.display())
            } else {
                format!("无法锁定 {}：{e}", path.display())
            }
        })?;
        Ok(Self(file))
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

/// tempfile::persist replaces the destination on Windows as well as Unix.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("文件缺少父目录")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temp.write_all(bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    temp.persist(path)
        .map_err(|e| format!("保存 {} 失败：{e}", path.display()))?;
    Ok(())
}
