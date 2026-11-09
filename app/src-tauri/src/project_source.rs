use super::*;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct SourceSettings {
    pub directory: String,
}
impl Default for SourceSettings {
    fn default() -> Self {
        Self {
            directory: "chapters".into(),
        }
    }
}

pub fn load(root: &Path) -> Result<SourceSettings, String> {
    let path = root.join(".chapter-source.json");
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SourceSettings {
            directory: if root.join("正文").is_dir() {
                "正文".into()
            } else {
                "chapters".into()
            },
        }),
        Err(e) => Err(e.to_string()),
    }
}

pub fn number(id: &str) -> Option<u32> {
    if let Some(s) = id.strip_prefix("ch") {
        return s.parse().ok();
    }
    let text = id.strip_prefix('第')?.split('章').next()?;
    if let Ok(n) = text.parse() {
        return Some(n);
    }
    let (mut sum, mut digit) = (0u32, 0u32);
    for c in text.chars() {
        match c {
            '零' | '〇' => digit = 0,
            '一' => digit = 1,
            '二' | '两' => digit = 2,
            '三' => digit = 3,
            '四' => digit = 4,
            '五' => digit = 5,
            '六' => digit = 6,
            '七' => digit = 7,
            '八' => digit = 8,
            '九' => digit = 9,
            '十' | '百' | '千' => {
                sum += digit.max(1)
                    * match c {
                        '十' => 10,
                        '百' => 100,
                        _ => 1000,
                    };
                digit = 0;
            }
            _ => return None,
        }
    }
    Some(sum + digit).filter(|n| *n > 0)
}

pub fn source_dir(root: &Path) -> Result<PathBuf, String> {
    let settings = load(root)?;
    let path = PathBuf::from(settings.directory.trim());
    if path.as_os_str().is_empty() {
        return Err("正文目录不能为空".into());
    }
    Ok(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

pub fn chapters(root: &Path) -> Result<BTreeMap<u32, PathBuf>, String> {
    let dir = source_dir(root)?;
    let mut result = BTreeMap::new();
    if !dir.exists() {
        return Ok(result);
    }
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if let Some(n) = number(&name) {
                for child in fs::read_dir(&path).map_err(|e| e.to_string())? {
                    let file = child.map_err(|e| e.to_string())?.path();
                    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    if file.is_file() && file.extension().and_then(|s| s.to_str()) == Some("md")
                        && (stem == "chapter" || number(stem) == Some(n)) {
                        if result.insert(n, file).is_some() {
                            return Err(format!("正文目录第 {} 章存在多个版本，请先明确保留哪个文件", n));
                        }
                    }
                }
            }
            continue;
        }
        let n = if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
            number(path.file_stem().and_then(|s| s.to_str()).unwrap_or(""))
        } else {
            None
        };
        if let Some(n) = n {
            if result.insert(n, path).is_some() {
                return Err(format!(
                    "正文目录第 {} 章存在多个版本，请先明确保留哪个文件",
                    n
                ));
            }
        }
    }
    Ok(result)
}

pub fn chapter_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    let n = number(id)
        .filter(|n| *n > 0)
        .ok_or("章号格式应为 ch001、ch002……")?;
    if let Some(p) = chapters(root)?.get(&n) {
        return Ok(p.clone());
    }
    let dir = source_dir(root)?;
    let configured = load(root)?.directory;
    Ok(if configured.trim() == "chapters" {
        dir.join(format!("ch{:03}", n)).join("chapter.md")
    } else {
        dir.join(format!("第{}章.md", n))
    })
}

pub fn backup(root: &Path, path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }
    let name = path.file_name().ok_or("无效正文文件名")?;
    let dir = root
        .join(".chapter-backups")
        .join(chrono::Local::now().format("%Y%m%d-%H%M%S-%f").to_string());
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::copy(path, dir.join(name)).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn write(root: &Path, path: &Path, text: &str) -> Result<(), String> {
    if path.exists() && fs::read_to_string(path).map_err(|e| e.to_string())? == text {
        return Ok(());
    }
    backup(root, path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("md.tmp");
    fs::write(&temp, text).map_err(|e| e.to_string())?;
    fs::rename(temp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_resolution_unifies_author_files_and_backs_up_overwrites() {
        let root = std::env::temp_dir().join(format!(
            "novel_source_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("正文")).unwrap();
        let original = root.join("正文/第十二章「过河」.md");
        fs::write(&original, "作者旧稿").unwrap();
        assert_eq!(number("第十二章"), Some(12));
        assert_eq!(number("第102章"), Some(102));
        assert_eq!(chapter_path(&root, "ch012").unwrap(), original);
        write(&root, &original, "作者新稿").unwrap();
        assert_eq!(fs::read_to_string(&original).unwrap(), "作者新稿");
        let backups: Vec<_> = fs::read_dir(root.join(".chapter-backups"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(
            fs::read_to_string(backups[0].path().join(original.file_name().unwrap())).unwrap(),
            "作者旧稿"
        );
        fs::write(root.join("正文/第12章.md"), "冲突版本").unwrap();
        assert!(chapters(&root).is_err());
        assert!(chapter_path(&root, "../../bad").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
