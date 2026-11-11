use super::*;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Options {
    pub generation_preset: Option<GenerationPreset>,
    pub automatic: bool,
    pub characters: Vec<String>,
    pub chapters: Vec<String>,
    pub events: Vec<String>,
    pub recent_chapters: usize,
    pub max_events: usize,
    pub input_budget: usize,
    pub include_outline: bool,
    pub outline_excerpt: String,
    pub outline_full: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            generation_preset: None,
            automatic: true,
            characters: vec![],
            chapters: vec![],
            events: vec![],
            recent_chapters: 2,
            max_events: 6,
            input_budget: 8000,
            include_outline: false,
            outline_excerpt: String::new(),
            outline_full: false,
        }
    }
}
impl Options {
    pub fn validate(&self) -> Result<(), String> {
        if self.recent_chapters > 10
            || self.max_events > 30
            || !(500..=100000).contains(&self.input_budget)
        {
            return Err("上下文参数超出范围（近期摘要0—10、事件0—30、输入预算500—100000）".into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Source {
    pub kind: String,
    pub id: String,
    pub file: String,
    pub reason: String,
    pub included: bool,
    pub characters: usize,
}
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Stats {
    pub characters: usize,
    pub estimated_tokens: usize,
    pub budget: usize,
    pub over_budget: bool,
    pub sources: Vec<Source>,
    pub warnings: Vec<String>,
}
#[derive(Serialize)]
pub struct Candidate {
    pub id: String,
    pub label: String,
    pub chapter: Option<u32>,
}
#[derive(Serialize)]
pub struct Catalog {
    pub characters: Vec<Candidate>,
    pub chapters: Vec<Candidate>,
    pub events: Vec<Candidate>,
    pub source_directory: String,
    pub chapter_file: String,
    pub notes: String,
}

pub fn yaml(path: &Path) -> Result<serde_yaml::Value, String> {
    match fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(serde_yaml::Value::Null),
        Ok(s) => {
            serde_yaml::from_str(&s).map_err(|e| format!("{} 格式错误: {}", path.display(), e))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_yaml::Value::Null),
        Err(e) => Err(format!("{} 读取失败: {}", path.display(), e)),
    }
}
pub fn jsonl(path: &Path) -> Result<Vec<Value>, String> {
    let text = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| {
            serde_json::from_str(l)
                .map_err(|e| format!("{} 第{}行格式错误: {}", path.display(), i + 1, e))
        })
        .collect()
}
fn string(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}
fn seq(v: &Value, key: &str) -> Vec<Value> {
    v.get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}
fn from_yaml(path: &Path) -> Result<Value, String> {
    serde_json::to_value(yaml(path)?).map_err(|e| e.to_string())
}
fn chapter(v: &Value) -> Option<u32> {
    project_source::number(&string(v, "chapter_id"))
}
fn compact(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !"，。！？、：；,.!?;:".contains(*c))
        .collect()
}

pub fn tokens(text: &str) -> usize {
    let mut cjk = 0;
    let mut ascii = 0;
    let mut other = 0;
    for c in text.chars() {
        if ('\u{2e80}'..='\u{9fff}').contains(&c) {
            cjk += 1;
        } else if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    cjk + (ascii + 3) / 4 + (other + 1) / 2
}

pub fn snapshot(root: &Path, id: &str) -> Result<(), String> {
    let n = project_source::number(id).ok_or("快照章号无效")?;
    let target = root.join(".memory-snapshots").join(format!("ch{:03}", n));
    fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    for name in ["characters.yaml", "story_bible.yaml", "foreshadowing.yaml"] {
        let source = root.join("memory").join(name);
        if source.exists() {
            fs::copy(source, target.join(name)).map_err(|e| e.to_string())?;
        }
    }
    project_manager::atomic_json(&root.join("memory/state_metadata.json"), &serde_json::json!({"through_chapter":n}))?;
    Ok(())
}

fn state_directory(root: &Path, target: u32, latest: u32, warnings: &mut Vec<String>) -> PathBuf {
    let metadata = fs::read_to_string(root.join("memory/state_metadata.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(Value::Null);
    let through = metadata
        .get("through_chapter")
        .and_then(Value::as_u64)
        .unwrap_or(latest as u64) as u32;
    let invalid = metadata
        .get("invalid_from")
        .and_then(Value::as_u64)
        .map(|n| n as u32);
    if latest.max(through) < target && invalid.is_none() {
        return root.join("memory");
    }
    let base = root.join(".memory-snapshots");
    let mut snapshots = BTreeMap::new();
    if let Ok(entries) = fs::read_dir(base) {
        for e in entries.flatten() {
            if let Some(n) = project_source::number(&e.file_name().to_string_lossy()) {
                if n < target && invalid.map_or(true, |i| n < i) && e.path().is_dir() {
                    snapshots.insert(n, e.path());
                }
            }
        }
    }
    if let Some((n, path)) = snapshots.last_key_value() {
        warnings.push(format!(
            "使用第{}章末的状态快照；其后到目标章之间的状态请在预览中核对。",
            n
        ));
        return path.clone();
    }
    warnings.push(
        "缺少目标章之前的人物/设定快照，已排除最新状态以免泄露后续事实；请在预览中补充当时状态。"
            .into(),
    );
    // 不存在的目录使状态文件读取为空；不回退到当前状态。
    root.join(".memory-snapshots").join("unavailable")
}

pub struct Memory {
    pub sections: HashMap<String, String>,
    pub conflict_sources: Vec<(String, String)>,
    pub stats: Stats,
    pub state_at: String,
}
fn record(
    stats: &mut Stats,
    kind: &str,
    id: &str,
    file: &Path,
    reason: &str,
    included: bool,
    text: &str,
) {
    stats.sources.push(Source {
        kind: kind.into(),
        id: id.into(),
        file: file.to_string_lossy().into(),
        reason: reason.into(),
        included,
        characters: text.chars().count(),
    });
}
fn before(v: &Value, target: u32) -> bool {
    chapter(v).map_or(false, |n| n < target)
}
fn joins(values: &[Value]) -> String {
    values
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("；")
}

pub fn build(
    root: &Path,
    id: &str,
    idea: &str,
    use_context: bool,
    opts: &Options,
) -> Result<Memory, String> {
    opts.validate()?;
    let target = project_source::number(id)
        .filter(|n| *n > 0)
        .ok_or("请选择有效的章节ID（如 ch006）")?;
    let mut stats = Stats {
        budget: opts.input_budget,
        ..Stats::default()
    };
    let mut sections = HashMap::new();
    if !use_context {
        stats
            .warnings
            .push("长期记忆已关闭；保留本章骨架和手动叙事要求。".into());
        return Ok(Memory {
            sections,
            conflict_sources: vec![],
            stats,
            state_at: String::new(),
        });
    }
    let memory = root.join("memory");
    let all_summaries = jsonl(&memory.join("chapter_summaries.jsonl"))?;
    let all_events = jsonl(&memory.join("events.jsonl"))?;
    let all_timeline = jsonl(&memory.join("timeline.jsonl"))?;
    let latest = all_summaries
        .iter()
        .chain(all_events.iter())
        .chain(all_timeline.iter())
        .filter_map(chapter)
        .max()
        .unwrap_or(0);
    let state_dir = state_directory(root, target, latest, &mut stats.warnings);
    let state_at = if state_dir == memory {
        format!("章初状态（记忆账本截至第{}章）", latest)
    } else {
        format!("章初状态来源：{}", state_dir.display())
    };
    let characters = from_yaml(&state_dir.join("characters.yaml"))?;
    let people = seq(&characters, "characters");
    let mut names: HashSet<String> = opts.characters.iter().cloned().collect();
    for p in &people {
        let name = string(p, "name");
        if opts.automatic
            && (!name.is_empty() && idea.contains(&name)
                || string(p, "role").contains("主角")
                || seq(p, "aliases")
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|a| !a.is_empty() && idea.contains(a)))
        {
            names.insert(name);
        }
    }
    let mut chosen = vec![];
    for person in &people {
        let name = string(person, "name");
        let include = names.contains(&name);
        record(
            &mut stats,
            "character",
            &name,
            &state_dir.join("characters.yaml"),
            if opts.characters.contains(&name) {
                "手动选择"
            } else if include {
                "主角或本章提及"
            } else {
                "本章未选用"
            },
            include,
            "",
        );
        if !include {
            continue;
        }
        let mut text = vec![format!("{}：", name)];
        for (key, label) in [
            ("identity", "身份"),
            ("personality", "性格"),
            ("speaking_style", "言语"),
            ("current_status", "章初状态"),
            ("knows", "掌握与知情"),
            ("secrets", "本人秘密（他人不可自动获知）"),
            ("relationships", "关系"),
            ("constraints", "连续性参考（可随本章明确事件更新）"),
        ] {
            if opts.generation_preset == Some(GenerationPreset::Compact)
                && matches!(key, "personality" | "speaking_style") { continue; }
            let value = person
                .get(key)
                .map(|v| {
                    if let Some(s) = v.as_str() {
                        s.into()
                    } else {
                        joins(v.as_array().map(Vec::as_slice).unwrap_or(&[]))
                    }
                })
                .unwrap_or_default();
            if !value.trim().is_empty() {
                text.push(format!("- {}：{}", label, value));
            }
        }
        let text = if opts.generation_preset == Some(GenerationPreset::Compact) {
            format!("{}：{}", name, text.into_iter().skip(1)
                .map(|line| line.trim_start_matches("- ").to_string()).collect::<Vec<_>>().join("；"))
        } else { text.join("\n") };
        if let Some(source) = stats.sources.last_mut() {
            source.characters = text.chars().count();
        }
        chosen.push(text);
    }
    for name in &opts.characters {
        if !people.iter().any(|p| string(p, "name") == *name) {
            stats.warnings.push(format!(
                "未在可用章初档案中找到角色 {}；未借用后续状态。",
                name
            ));
        }
    }
    if people.is_empty() {
        stats
            .warnings
            .push("当前可用角色档案为空，请在预览中补充章初状态。".into());
    }
    sections.insert("Characters".into(), chosen.join("\n\n"));

    let mut summaries: Vec<_> = all_summaries
        .iter()
        .filter(|v| before(v, target))
        .cloned()
        .collect();
    summaries.sort_by_key(|v| chapter(v).unwrap_or(0));
    let recent: HashSet<String> = summaries
        .iter()
        .rev()
        .take(opts.recent_chapters)
        .map(|v| string(v, "chapter_id"))
        .collect();
    let mut summary_chapters = HashSet::new();
    let mut summary_text = vec![];
    let mut summary_seen = HashSet::new();
    for s in &all_summaries {
        let cid = string(s, "chapter_id");
        let explicit = opts.chapters.contains(&cid);
        let include = before(s, target) && (recent.contains(&cid) || explicit);
        let reason = if !before(s, target) {
            "本章或后续记录，已排除"
        } else if explicit {
            "手动参考章"
        } else if include {
            "近期直接前情"
        } else {
            "较早摘要，未选入"
        };
        record(
            &mut stats,
            "summary",
            &cid,
            &memory.join("chapter_summaries.jsonl"),
            reason,
            include,
            &string(s, "chapter_summary"),
        );
        if include && summary_seen.insert(cid.clone()) {
            summary_chapters.insert(cid.clone());
            summary_text.push((
                chapter(s).unwrap_or(0),
                format!("- [{}] {}", cid, string(s, "chapter_summary")),
            ));
        }
    }
    // A summary variable never falls back to chapter prose. Prose requires an explicit template reference.
    for cid in &opts.chapters {
        let n = project_source::number(cid).ok_or("参考章号无效")?;
        if n >= target { return Err(format!("参考章 {cid} 必须早于目标章")); }
        if !summary_chapters.contains(cid) {
            stats.warnings.push(format!("参考章 {cid} 没有摘要；如需正文，请在模板中使用 {{{{参考正文:{cid}}}}}"));
        }
    }
    summary_text.sort_by_key(|(n, _)| *n);
    sections.insert(
        "Recent Chapter Summaries".into(),
        summary_text
            .into_iter()
            .map(|(_, t)| t)
            .collect::<Vec<_>>()
            .join("\n\n"),
    );

    // Events and timeline retain independent file provenance.
    let mut merged = BTreeMap::new();
    for e in &all_events {
        let eid = string(e, "event_id");
        if !eid.is_empty() { merged.entry(eid).or_insert_with(|| e.clone()); }
    }
    // 去掉人物名后再计算历史相关性，避免主角姓名使全书每条事件都匹配。
    let mut query = idea.to_string();
    for p in &people {
        query = query.replace(&string(p, "name"), " ");
    }
    let terms = idea_terms(&query);
    let mut ranked: Vec<_> = merged
        .iter()
        .filter(|(_, e)| before(e, target))
        .map(|(eid, e)| {
            let score = idea_terms(&string(e, "description"))
                .intersection(&terms)
                .count();
            (eid.clone(), score, chapter(e).unwrap_or(0))
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
    let auto_ids: HashSet<_> = if opts.automatic {
        ranked
            .iter()
            .filter(|(_, score, _)| *score >= 2)
            .take(opts.max_events)
            .map(|(eid, _, _)| eid.clone())
            .collect()
    } else {
        HashSet::new()
    };
    let mut event_text = vec![];
    let mut descriptions = HashSet::new();
    for (eid, e) in &merged {
        let explicit = opts.events.contains(eid);
        if explicit && !before(e, target) {
            return Err(format!("事件 {} 不早于目标章，不能作为前情", eid));
        }
        let mut include = before(e, target) && (explicit || auto_ids.contains(eid));
        let desc = string(e, "description");
        if include && !descriptions.insert((chapter(e), compact(&desc))) {
            include = false;
        }
        let reason = if !before(e, target) {
            "本章或后续记录，已排除"
        } else if explicit {
            "手动选择历史事件"
        } else if include {
            "与本章关键词相关"
        } else {
            "关联度低、重复或超出事件条数"
        };
        record(
            &mut stats,
            "event",
            eid,
            &memory.join("events.jsonl"),
            reason,
            include,
            &desc,
        );
        if include {
            let time = string(e, "time_order");
            let text = format!(
                "- [{}{}] {}",
                string(e, "chapter_id"),
                if time.is_empty() {
                    String::new()
                } else {
                    format!(" / {}", time)
                },
                desc
            );
            event_text.push(text);
        }
    }
    for eid in &opts.events {
        if !merged.contains_key(eid) {
            stats.warnings.push(format!("找不到手动选择的事件 {}", eid));
        }
    }
    sections.insert("Recent Events".into(), event_text.join("\n"));
    let mut timeline_text = Vec::new();
    for (index, entry) in all_timeline.iter().enumerate() {
        let include = before(entry, target);
        let text = serde_json::to_string(entry).map_err(|e| e.to_string())?;
        record(&mut stats, "timeline", &format!("{}:{index}", string(entry, "event_id")),
            &memory.join("timeline.jsonl"), "仅携带目标章之前的时间线记录", include, &text);
        if include { timeline_text.push((chapter(entry).unwrap_or(0), text)); }
    }
    timeline_text.sort_by_key(|(n, _)| *n);
    sections.insert("Timeline".into(), timeline_text.into_iter().map(|(_, text)| text).collect::<Vec<_>>().join("\n"));

    let bible = from_yaml(&state_dir.join("story_bible.yaml"))?;
    let mut rules = vec![];
    for (key, label) in [
        ("genre", "题材"),
        ("tone", "文风"),
        ("world_rules", "世界规则"),
        ("writing_rules", "写作规则"),
        ("forbidden_patterns", "表达与连续性限制"),
    ] {
        let text = bible
            .get(key)
            .map(|v| {
                if let Some(s) = v.as_str() {
                    s.into()
                } else {
                    joins(v.as_array().map(Vec::as_slice).unwrap_or(&[]))
                }
            })
            .unwrap_or_default();
        if !text.trim().is_empty() {
            rules.push(format!("{}：{}", label, text));
        }
    }
    record(
        &mut stats,
        "rules",
        "story_bible",
        &state_dir.join("story_bible.yaml"),
        "仅保留规则字段，不发送编辑备注或叙事配置",
        !rules.is_empty(),
        &rules.join("\n"),
    );
    sections.insert("Story Bible".into(), rules.join("\n"));
    let mut clues = vec![];
    let fs_data = from_yaml(&state_dir.join("foreshadowing.yaml"))?;
    for clue in seq(&fs_data, "active") {
        let introduced = project_source::number(&string(&clue, "introduced_in"));
        let related = seq(&clue, "related_characters");
        let include = introduced.map_or(true, |n| n < target)
            && (related.is_empty()
                || related
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|n| names.contains(n)));
        let text = string(&clue, "description");
        record(
            &mut stats,
            "foreshadowing",
            &string(&clue, "id"),
            &state_dir.join("foreshadowing.yaml"),
            if include {
                "已埋下且与本章人物相关，仅携带已知线索"
            } else {
                "未发生或与本章人物无关"
            },
            include,
            &text,
        );
        if include {
            clues.push(format!("- {}", text));
            if clue.get("dramatize").and_then(Value::as_bool) == Some(true) {
                clues.push("以上线索若本章涉及，以具体场景呈现。".into());
            }
        }
    }
    record(
        &mut stats,
        "foreshadowing",
        "resolved",
        &state_dir.join("foreshadowing.yaml"),
        "已解决伏笔不重复发送；计划回收方式不作为既成事实发送",
        false,
        "",
    );
    sections.insert("Active Foreshadowing".into(), clues.join("\n"));
    let outline = if opts.include_outline && opts.outline_full {
        fs::read_to_string(root.join("outlines/book_outline.yaml")).map_err(|e| format!("全书大纲读取失败: {e}"))?
    } else if opts.include_outline {
        opts.outline_excerpt.trim().to_string()
    } else {
        String::new()
    };
    if opts.include_outline && outline.is_empty() {
        stats
            .warnings
            .push("已勾选大纲选段，但选段为空；未自动发送全书大纲。".into());
    }
    record(
        &mut stats,
        "outline",
        "outline_excerpt",
        &root.join("outlines/book_outline.yaml"),
        if outline.is_empty() {
            "默认不携带全书大纲，只接受作者明确提供的选段"
        } else {
            if opts.outline_full { "作者明确选择完整大纲" } else { "作者手动大纲选段，仅作方向参考" }
        },
        !outline.is_empty(),
        &outline,
    );
    sections.insert("Book Outline".into(), outline);
    let styles = jsonl(&memory.join("style_bank.jsonl"))?;
    let style_text: Vec<_> = styles
        .iter()
        .filter(|v| {
            let source = v
                .get("source_chapter")
                .or_else(|| v.get("chapter_id"))
                .and_then(Value::as_str)
                .and_then(project_source::number);
            source.map_or(latest < target, |n| n < target)
        })
        .take(2)
        .filter_map(|v| v.get("text").and_then(Value::as_str).map(str::to_string))
        .collect();
    record(
        &mut stats,
        "style",
        "style",
        &memory.join("style_bank.jsonl"),
        "最多两条风格样例；旧章排除后续或来源章号不明的样例",
        !style_text.is_empty(),
        &style_text.join("\n"),
    );
    sections.insert("Style References".into(), style_text.join("\n\n"));
    let conflict_sources = sections
        .iter()
        .filter(|(k, _)| k.as_str() != "Style References")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Ok(Memory {
        sections,
        conflict_sources,
        stats,
        state_at,
    })
}

pub fn catalog(root: &Path, id: &str) -> Result<Catalog, String> {
    let people = from_yaml(&root.join("memory/characters.yaml"))?;
    let summaries = jsonl(&root.join("memory/chapter_summaries.jsonl"))?;
    let events = jsonl(&root.join("memory/events.jsonl"))?;
    let mut refs: BTreeMap<u32, Candidate> = project_source::chapters(root)?
        .into_iter()
        .map(|(n, p)| {
            (
                n,
                Candidate {
                    id: format!("ch{:03}", n),
                    label: p.file_name().unwrap_or_default().to_string_lossy().into(),
                    chapter: Some(n),
                },
            )
        })
        .collect();
    for s in summaries {
        if let Some(n) = chapter(&s) {
            refs.insert(
                n,
                Candidate {
                    id: string(&s, "chapter_id"),
                    label: string(&s, "chapter_summary"),
                    chapter: Some(n),
                },
            );
        }
    }
    Ok(Catalog {
        characters: seq(&people, "characters")
            .iter()
            .map(|c| Candidate {
                id: string(c, "name"),
                label: string(c, "identity"),
                chapter: None,
            })
            .collect(),
        chapters: refs.into_values().collect(),
        events: events
            .iter()
            .map(|e| Candidate {
                id: string(e, "event_id"),
                label: string(e, "description"),
                chapter: chapter(e),
            })
            .collect(),
        source_directory: project_source::source_dir(root)?.to_string_lossy().into(),
        chapter_file: project_source::chapter_path(root, id)?
            .to_string_lossy()
            .into(),
        notes: read_text(&root.join("author-notes.md")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        let root = tempfile::tempdir().unwrap().keep();
        fs::create_dir_all(root.join("memory")).unwrap();
        root
    }
    fn rows(root: &Path, file: &str, values: Vec<Value>) {
        fs::write(
            root.join("memory").join(file),
            values
                .into_iter()
                .map(|v| format!("{}\n", v))
                .collect::<String>(),
        )
        .unwrap();
    }
    #[test]
    fn events_and_timeline_keep_separate_sources_and_exclude_current_future() {
        let root = root();
        rows(
            &root,
            "events.jsonl",
            vec![
                serde_json::json!({"chapter_id":"ch009","event_id":"future","description":"铜钥匙开启天门"}),
                serde_json::json!({"chapter_id":"ch002","event_id":"key","description":"铜钥匙来自旧宅"}),
                serde_json::json!({"chapter_id":"ch005","event_id":"current","description":"铜钥匙本章结局"}),
            ],
        );
        rows(
            &root,
            "timeline.jsonl",
            vec![
                serde_json::json!({"chapter_id":"ch002","event_id":"key","description":"铜钥匙来自旧宅","time":"三日前"}),
            ],
        );
        let opts = Options {
            recent_chapters: 0,
            ..Options::default()
        };
        let mem = build(&root, "ch005", "铜钥匙", true, &opts).unwrap();
        let text = &mem.sections["Recent Events"];
        assert_eq!(text.matches("铜钥匙来自旧宅").count(), 1);
        assert!(!text.contains("三日前"));
        assert!(mem.sections["Timeline"].contains("三日前"));
        assert!(!text.contains("开启天门"));
        assert!(!text.contains("本章结局"));
        assert!(!mem.sections["Timeline"].is_empty());
        let invalid = Options {
            events: vec!["future".into()],
            ..opts
        };
        assert!(build(&root, "ch005", "铜钥匙", true, &invalid).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn summaries_are_chronological_and_do_not_suppress_event_sources() {
        let root = root();
        rows(
            &root,
            "chapter_summaries.jsonl",
            vec![
                serde_json::json!({"chapter_id":"ch003","chapter_summary":"归还铜钥匙"}),
                serde_json::json!({"chapter_id":"ch001","chapter_summary":"找到铜钥匙"}),
                serde_json::json!({"chapter_id":"ch002","chapter_summary":"修复铜钥匙"}),
            ],
        );
        rows(
            &root,
            "events.jsonl",
            vec![
                serde_json::json!({"chapter_id":"ch003","event_id":"key","description":"归还铜钥匙"}),
            ],
        );
        let mem = build(&root, "ch004", "铜钥匙", true, &Options::default()).unwrap();
        let text = &mem.sections["Recent Chapter Summaries"];
        assert!(!text.contains("找到铜钥匙"));
        assert!(text.find("修复").unwrap() < text.find("归还").unwrap());
        assert!(mem.sections["Recent Events"].contains("归还铜钥匙"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn manual_characters_and_outline_excerpt_never_import_entire_outline_or_notes() {
        let root = root();
        fs::write(root.join("memory/characters.yaml"), r#"{"characters":[{"name":"阿甲","role":"主角","current_status":"章初疲惫"},{"name":"阿乙","identity":"船夫"}]}"#).unwrap();
        fs::write(root.join("author-notes.md"), "未来阿乙成为皇帝").unwrap();
        fs::create_dir_all(root.join("outlines")).unwrap();
        fs::write(
            root.join("outlines/book_outline.yaml"),
            "ending: 全书结局不可泄露",
        )
        .unwrap();
        let opts = Options {
            automatic: false,
            characters: vec!["阿乙".into()],
            include_outline: true,
            outline_excerpt: "本章以渡河为方向".into(),
            ..Options::default()
        };
        let mem = build(&root, "ch002", "渡河", true, &opts).unwrap();
        assert!(mem.sections["Characters"].contains("阿乙"));
        assert!(!mem.sections["Characters"].contains("阿甲"));
        let all = mem
            .sections
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("本章以渡河"));
        assert!(!all.contains("皇帝"));
        assert!(!all.contains("全书结局"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn historical_snapshot_is_used_and_latest_state_is_never_fallback() {
        let root = root();
        fs::write(
            root.join("memory/characters.yaml"),
            "characters:\n- name: 主角\n  role: 主角\n  current_status: 尚未取得宝剑\n",
        )
        .unwrap();
        snapshot(&root, "ch002").unwrap();
        fs::write(
            root.join("memory/characters.yaml"),
            "characters:\n- name: 主角\n  role: 主角\n  current_status: 已经登基称帝\n",
        )
        .unwrap();
        rows(
            &root,
            "chapter_summaries.jsonl",
            vec![serde_json::json!({"chapter_id":"ch006","chapter_summary":"已登基"})],
        );
        rows(
            &root,
            "style_bank.jsonl",
            vec![serde_json::json!({"id":"later_style", "text":"未来登基的场景不可泄露"})],
        );
        let old = build(&root, "ch003", "主角买剑", true, &Options::default()).unwrap();
        assert!(old.sections["Style References"].is_empty());
        assert!(old.sections["Characters"].contains("尚未取得宝剑"));
        assert!(!old.sections["Characters"].contains("称帝"));
        let missing = build(&root, "ch002", "主角", true, &Options::default()).unwrap();
        assert!(missing.sections["Characters"].is_empty());
        assert!(missing
            .stats
            .warnings
            .iter()
            .any(|w| w.contains("缺少目标章")));
        fs::remove_dir_all(root).unwrap();
    }
}
