// Shared Rust writing engine used by Tauri, the terminal UI and background workers.
// Project files and writing rules retain the existing desktop contracts.

mod llm;
pub mod context;
pub mod project_source;
pub mod project_manager;
pub mod chapter_titles;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
mod runtime;
pub use runtime::{Context, FileLock, atomic_write};

macro_rules! attempt {
    ($value:expr) => { match $value { Ok(v) => v, Err(log) => return CommandResult { success: false, log } } };
}

// ============================================================
// 数据结构
// ============================================================

#[derive(Serialize, Deserialize, Clone)]
pub struct CommandResult {
    pub success: bool,
    pub log: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub project_dir: String,
    pub llm_base_url: String,
    pub llm_api_key: String,
    pub llm_model: String,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_output_tokens: Option<u32>,
    // 关闭模型思考（Qwen3 等思考模型）。None/true = 关闭思考（给 prompt 追加 /no_think）
    pub disable_thinking: Option<bool>,
    // LLM 路线："api"(默认/OpenAI 兼容) | "claude_code"(Claude Code 订阅) | "codex"(Codex/ChatGPT 订阅)
    // None 视为 "api"，兼容老版本设置文件
    pub llm_provider: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            project_dir: String::new(),
            llm_base_url: "http://localhost:18083/v1".to_string(),
            llm_api_key: "local".to_string(),
            llm_model: String::new(),
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            disable_thinking: None,
            llm_provider: None,
        }
    }
}

/// 是否走订阅 CLI 路线（Claude Code / Codex）而非 OpenAI 兼容 HTTP API。
fn is_cli_provider(settings: &Settings) -> bool {
    matches!(
        settings.llm_provider.as_deref(),
        Some("claude_code") | Some("codex")
    )
}

/// 是否关闭思考：默认开启关闭（None 视为 true），除非用户显式设为 false。
/// /no_think 只对本地 OpenAI 兼容模型（如 Qwen3）有意义；订阅 CLI 路线绝不追加。
fn thinking_disabled(settings: &Settings) -> bool {
    if is_cli_provider(settings) {
        return false;
    }
    settings.disable_thinking != Some(false)
}

/// 需要时给 user prompt 追加 Qwen3 的 /no_think 软开关，避免思考吃掉输出额度。
fn apply_no_think(prompt: String, settings: &Settings) -> String {
    if thinking_disabled(settings) {
        format!("{}\n\n/no_think", prompt)
    } else {
        prompt
    }
}

/// 内部运行时配置（从 Settings + config.yaml 合并而来）
struct RunConfig {
    project_dir: PathBuf,
    llm_base_url: String,
    llm_api_key: String,
    model_name: String,
    max_output_tokens: Option<u32>,
    // LLM 路线："api" | "claude_code" | "codex"
    provider: String,
}

// ============================================================
// 项目目录 + 设置
// ============================================================

// 设置统一存放在固定的全局位置（应用数据目录），与项目目录解耦，
// 避免「读 / 写位置不一致」导致自定义项目目录时配置丢失。
fn settings_path(app: &Context) -> PathBuf {
    let app_data = app.data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    app_data.join(".ui-settings.json")
}

fn load_settings(app: &Context) -> Settings { app.settings.clone() }
fn resolve_project_dir(app: &Context) -> PathBuf { app.project_dir.clone() }

fn ensure_project_initialized(project_dir: &Path, _app: &Context) -> Result<(), String> {
    if !project_dir.is_dir() {
        return Err("项目目录不存在，请在最近项目中重新定位或新建项目".into());
    }
    static INITIALIZATION: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _initialization = INITIALIZATION.lock().map_err(|e| e.to_string())?;
    let marker = project_dir.join(".initialized");
    if marker.exists() {
        return Ok(());
    }

    // Embed templates so dev and packaged builds initialize identically.
    // create_new preserves all author files, including imported projects without a marker.
    use std::io::Write;
    let templates = [
        ("config.yaml", include_str!("../../../app/src-tauri/templates/config.yaml")),
        ("memory/characters.yaml", include_str!("../../../app/src-tauri/templates/memory/characters.yaml")),
        ("memory/story_bible.yaml", include_str!("../../../app/src-tauri/templates/memory/story_bible.yaml")),
        ("memory/foreshadowing.yaml", include_str!("../../../app/src-tauri/templates/memory/foreshadowing.yaml")),
        ("memory/relationships.json", include_str!("../../../app/src-tauri/templates/memory/relationships.json")),
        ("memory/chapter_summaries.jsonl", include_str!("../../../app/src-tauri/templates/memory/chapter_summaries.jsonl")),
        ("memory/events.jsonl", include_str!("../../../app/src-tauri/templates/memory/events.jsonl")),
        ("memory/timeline.jsonl", include_str!("../../../app/src-tauri/templates/memory/timeline.jsonl")),
        ("memory/style_bank.jsonl", include_str!("../../../app/src-tauri/templates/memory/style_bank.jsonl")),
        ("outlines/book_outline.yaml", include_str!("../../../app/src-tauri/templates/outlines/book_outline.yaml")),
    ];
    for (rel, contents) in templates {
        let path = project_dir.join(rel);
        if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(contents.as_bytes()) {
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(e.to_string());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && path.is_file() => {},
            Err(e) => return Err(format!("初始化 {} 失败：{}", path.display(), e)),
        }
    }

    fs::create_dir_all(project_dir.join("chapters")).map_err(|e| e.to_string())?;
    fs::create_dir_all(project_dir.join("outputs/reports")).map_err(|e| e.to_string())?;
    fs::write(marker, "initialized").map_err(|e| e.to_string())
}

// ============================================================
// config.yaml 辅助
// ============================================================

fn load_config_yaml(project_dir: &Path) -> serde_yaml::Value {
    let path = project_dir.join("config.yaml");
    match fs::read_to_string(&path) {
        Ok(text) => serde_yaml::from_str(&text).unwrap_or(serde_yaml::Value::Null),
        Err(_) => serde_yaml::Value::Null,
    }
}

fn config_string(config: &serde_yaml::Value, key: &str, default: &str) -> String {
    config
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

/// 构建运行时配置
fn build_run_config(app: &Context) -> Result<RunConfig, String> {
    let project_dir = resolve_project_dir(app);
    let settings = load_settings(app);
    let config = load_config_yaml(&project_dir);

    let provider = settings
        .llm_provider
        .clone()
        .unwrap_or_else(|| "api".to_string());

    // API 路线：留空回退到 config.yaml 的 model_name。
    // 订阅 CLI 路线：留空表示用订阅默认模型（不能回退到 config.yaml，否则会把
    // "default-model" 当成 --model 传给 claude/codex 导致报错）。
    let model_name = if provider == "api" {
        if settings.llm_model.trim().is_empty() {
            config_string(&config, "model_name", "default-model")
        } else {
            settings.llm_model.trim().to_string()
        }
    } else {
        settings.llm_model.trim().to_string()
    };

    // 输出上限：默认不限（None）——只受模型上下文窗口约束。
    // 仅当用户在设置里显式填了正数时才作为上限；填 0 同样视为不限。
    let max_output_tokens = match settings.max_output_tokens {
        Some(n) if n > 0 => Some(n),
        _ => None,
    };

    Ok(RunConfig {
        project_dir,
        llm_base_url: settings.llm_base_url,
        llm_api_key: settings.llm_api_key,
        model_name,
        max_output_tokens,
        provider,
    })
}

// ============================================================
// 文件 I/O 工具函数
// ============================================================

pub const EDITABLE_FILES: &[&str] = &[
    "author-notes.md",
    "memory/story_bible.yaml",
    "memory/characters.yaml",
    "memory/foreshadowing.yaml",
    "memory/relationships.json",
    "memory/timeline.jsonl",
    "memory/events.jsonl",
    "memory/chapter_summaries.jsonl",
    "memory/style_bank.jsonl",
    "outlines/book_outline.yaml",
    "config.yaml",
];

fn normalize_rel(rel: &str) -> String {
    rel.replace('\\', "/")
}

fn load_yaml(path: &Path) -> serde_yaml::Value {
    match fs::read_to_string(path) {
        Ok(text) => serde_yaml::from_str(&text).unwrap_or(serde_yaml::Value::Mapping(serde_yaml::Mapping::new())),
        Err(_) => serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
    }
}

fn save_yaml(path: &Path, data: &serde_yaml::Value) -> Result<(), String> {
    atomic_write(path, serde_yaml::to_string(data).map_err(|e| e.to_string())?.as_bytes())
}

fn load_json(path: &Path) -> serde_json::Value {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
        Err(_) => serde_json::Value::Object(serde_json::Map::new()),
    }
}

fn save_json(path: &Path, data: &serde_json::Value) -> Result<(), String> {
    atomic_write(path, serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?.as_slice())
}

fn read_jsonl(path: &Path) -> Vec<serde_json::Value> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed == "{}" {
                return None;
            }
            serde_json::from_str(trimmed).ok()
        })
        .collect()
}

fn append_jsonl(path: &Path, obj: &serde_json::Value) -> Result<(), String> {
    let mut content = match fs::read_to_string(path) {
        Ok(s) => s, Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    if !content.is_empty() && !content.ends_with('\n') { content.push('\n'); }
    content.push_str(&serde_json::to_string(obj).map_err(|e| e.to_string())?);
    content.push('\n');
    atomic_write(path, content.as_bytes())
}
fn rewrite_jsonl(path: &Path, records: &[serde_json::Value]) -> Result<(), String> {
    let mut content = String::new();
    for record in records {
        content.push_str(&serde_json::to_string(record).map_err(|e| e.to_string())?);
        content.push('\n');
    }
    atomic_write(path, content.as_bytes())
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn write_text(path: &Path, text: &str) -> Result<(), String> { atomic_write(path, text.as_bytes()) }

// ============================================================
// LLM 消息构建辅助
// ============================================================

fn sys_user_messages(system_prompt: &str, user_prompt: &str) -> Vec<llm::ChatMessage> {
    vec![
        llm::ChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        },
        llm::ChatMessage {
            role: "user".to_string(),
            content: user_prompt.to_string(),
        },
    ]
}

/// 把消息列表拆成 (system, user) 两段文本，供订阅 CLI 路线使用。
/// system 角色合并为一段；其余角色按出现顺序拼成 user。
fn split_system_user(messages: &[llm::ChatMessage]) -> (String, String) {
    let mut system = String::new();
    let mut user_parts: Vec<String> = Vec::new();
    for m in messages {
        if m.role == "system" {
            if !system.is_empty() {
                system.push('\n');
            }
            system.push_str(&m.content);
        } else {
            user_parts.push(m.content.clone());
        }
    }
    (system, user_parts.join("\n\n"))
}

/// 按当前 provider 分发到对应 LLM 路线。
/// - api：OpenAI 兼容 HTTP（沿用 temperature/top_p/max_tokens）
/// - claude_code / codex：调用本机已登录订阅的官方 CLI（采样参数由 CLI 自管）
fn run_completion(
    config: &RunConfig,
    settings: &Settings,
    messages: Vec<llm::ChatMessage>,
    temperature: f64,
    top_p: f64,
    max_output_tokens: Option<u32>,
) -> Result<String, String> {
    match settings.llm_provider.as_deref() {
        Some("claude_code") => {
            let (system, user) = split_system_user(&messages);
            llm::call_claude_code(&config.model_name, &system, &user)
        }
        Some("codex") => {
            let (system, user) = split_system_user(&messages);
            llm::call_codex(&config.model_name, &system, &user)
        }
        _ => llm::call_llm(
            &config.llm_base_url,
            &config.llm_api_key,
            &config.model_name,
            messages,
            temperature,
            top_p,
            max_output_tokens,
        ),
    }
}

// ============================================================
// 4.1 generate_chapter（移植 generate_chapter_local.py）
// ============================================================

type MemorySections = context::Memory;

#[cfg(test)]
fn build_memory_sections(project_dir: &Path, use_context: bool) -> MemorySections {
    context::build(project_dir, "ch999999", "", use_context, &context::Options::default()).unwrap()
}

const DEFAULT_SYSTEM_PROMPT: &str = "你是一名中文长篇小说写作助手，只输出小说正文。";
const DEFAULT_USER_TEMPLATE: &str = "【角色档案】
{{角色档案}}

【章节摘要】
{{章节摘要}}

【历史事件】
{{历史事件}}

【时间线】
{{时间线}}

【故事圣经】
{{故事圣经}}

【全书大纲】
{{全书大纲}}

【伏笔档案】
{{伏笔档案}}

【文风参考】
{{文风参考}}

【事件骨架】
{{事件骨架}}

【写作要求】
写成完整连续的小说场景。
人物与前情为章初资料，本章可依事件骨架取得新物品、获知信息、展示新能力或改变状态。
未在本章骨架安排的未来规划不得提前展开。
目标约 {{目标字数}} 中文字。
{{叙事视角}}
{{叙事要求}}
遵循作者事件骨架的顺序，不改变动作执行者。
不重复前文已发生的事件，不提前进入下一章主要剧情。
只输出本章小说正文，不附标题、分析或说明。";

// Keep legacy generation fields as the general preset so existing projects retain their templates.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationPreset {
    #[default]
    General,
    Compact,
    Benchmark,
}

fn default_material_guidance() -> bool { true }
fn default_benchmark_material_guidance() -> bool { false }
fn default_compact_generation() -> TaskPrompt {
    TaskPrompt {
        system: DEFAULT_SYSTEM_PROMPT.into(),
        user_template: "【角色档案】
{{角色档案}}

【章节摘要】
{{章节摘要}}

【历史事件】
{{历史事件}}

【时间线】
{{时间线}}

【事件骨架】
{{事件骨架}}

【写作要求】
写成完整连续的小说场景。
按事件骨架顺序展开，保持动作执行者一致。
人物状态与前情描述章初情况，本章变化以事件骨架为准。
不提前展开骨架以外的未来剧情。
目标约 {{目标字数}} 中文字。
{{叙事视角}}
{{叙事要求}}
{{故事圣经}}

【全书大纲】
{{全书大纲}}

【伏笔档案】
{{伏笔档案}}
{{文风参考}}".into(),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PromptSettings {
    #[serde(default)]
    pub generation_preset: GenerationPreset,
    #[serde(default = "default_compact_generation")]
    pub compact_generation: TaskPrompt,
    #[serde(default = "default_material_guidance")]
    pub general_material_guidance: bool,
    #[serde(default)]
    pub compact_material_guidance: bool,
    #[serde(default = "default_benchmark_material_guidance")]
    pub benchmark_material_guidance: bool,
    #[serde(default)]
    pub benchmark_generation: TaskPrompt,
    pub system: String,
    pub user_template: String,
    #[serde(default = "default_consistency_prompt")]
    pub consistency: TaskPrompt,
    #[serde(default = "default_memory_prompt")]
    pub memory: TaskPrompt,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct TaskPrompt {
    pub system: String,
    pub user_template: String,
}

fn task_template(template: &str) -> String {
    let mut text = template.to_string();
    for (old, new) in [
        ("chapter_id", "章节ID"), ("chapter_text", "章节正文"), ("chapter_idea", "事件骨架"),
        ("characters_text", "角色档案"), ("world_rules_text", "故事圣经"),
        ("foreshadowing_text", "伏笔档案"), ("events_text", "历史事件"), ("timeline_text", "时间线"),
    ] {
        text = text.replace(&format!("{{{}}}", old), &format!("{{{{{}}}}}", new));
    }
    text
}

fn default_consistency_prompt() -> TaskPrompt {
    TaskPrompt {
        system: "你是一位专业的小说编辑，专注于连续性审查。请用中文输出报告。".into(),
        user_template: format!("{}\n\n## 章节摘要\n{{{{章节摘要}}}}", task_template(CONSISTENCY_CHECK_PROMPT)),
    }
}

fn default_memory_prompt() -> TaskPrompt {
    TaskPrompt {
        system: "你只输出严格合法的JSON，不输出任何其他文本。".into(),
        user_template: task_template(EXTRACTION_PROMPT_TEMPLATE),
    }
}

impl Default for PromptSettings {
    fn default() -> Self {
        Self { generation_preset: GenerationPreset::General,
            compact_generation: default_compact_generation(), general_material_guidance: true, compact_material_guidance: false,
            benchmark_material_guidance: false, benchmark_generation: TaskPrompt::default(),
            system: DEFAULT_SYSTEM_PROMPT.into(), user_template: DEFAULT_USER_TEMPLATE.into(),
            consistency: default_consistency_prompt(), memory: default_memory_prompt() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PromptPair {
    pub system: String,
    pub user: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PromptPreview {
    pub prompts: PromptPair,
    pub source: String,
    pub stats: context::Stats,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PromptOverride {
    pub prompts: PromptPair,
    pub source: String,
}

/// Complete, author-supplied messages. No template, memory, or thinking suffix is applied.
pub fn preview_manual_prompt(prompts: PromptPair, input_budget: usize) -> Result<PromptPreview, String> {
    if prompts.user.trim().is_empty() { return Err("请填写完整章节提示词".into()); }
    if !(500..=100000).contains(&input_budget) { return Err("输入预算应为 500—100000 tokens".into()); }
    let estimated_tokens = context::tokens(&prompts.system) + context::tokens(&prompts.user);
    if estimated_tokens > input_budget { return Err("完整提示词超过输入预算，请精简内容或提高预算".into()); }
    let source = serde_json::to_string(&("manual", &prompts, input_budget)).map_err(|e| e.to_string())?;
    let stats = context::Stats { characters: prompts.system.chars().count() + prompts.user.chars().count(),
        estimated_tokens, budget: input_budget, ..Default::default() };
    Ok(PromptPreview { prompts, source, stats })
}

fn load_prompt_settings(project_dir: &Path) -> Result<PromptSettings, String> {
    match fs::read_to_string(project_dir.join(".prompt-settings.json")) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("提示词设置读取失败: {}", e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PromptSettings::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub const GENERATION_VARIABLES: &[&str] = &[
    "角色档案", "故事圣经", "全书大纲", "伏笔档案", "章节摘要", "历史事件", "时间线", "文风参考",
    "事件骨架", "目标字数", "叙事视角", "叙事要求", "章节ID",
];
const PROMPT_VARIABLES: &[&str] = &[
    "角色档案", "故事圣经", "全书大纲", "伏笔档案", "章节摘要", "历史事件", "时间线", "文风参考",
    "事件骨架", "目标字数", "叙事视角", "叙事要求", "章节ID", "章节正文",
];
fn reference_chapter(name: &str) -> Option<&str> {
    let id = name.strip_prefix("参考正文:")?;
    let digits = id.strip_prefix("ch")?;
    if digits.len() >= 3 && digits.bytes().all(|b| b.is_ascii_digit()) { Some(id) } else { None }
}

// 单次扫描模板；资料里的 {{...}} 不会被再次替换或当作模板执行。
fn render_prompt_template(template: &str, values: &[(String, String)]) -> Result<String, String> {
    let mut rest = template;
    let mut output = String::new();
    while let Some(start) = rest.find("{{") {
        let line = template[..template.len() - rest.len() + start].bytes().filter(|b| *b == b'\n').count() + 1;
        output.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let end = tail.find("}}").ok_or_else(|| format!("第 {line} 行：模板变量缺少 }}}}"))?;
        let name = tail[..end].trim();
        if !PROMPT_VARIABLES.contains(&name) && reference_chapter(name).is_none() {
            let hint = match name {
                "人物状态" => "；旧变量已移除，请使用 {{角色档案}}",
                "前情" => "；旧变量已移除，请分别使用 {{章节摘要}}、{{历史事件}}、{{时间线}}",
                "相关设定与伏笔" => "；旧变量已移除，请分别使用 {{故事圣经}}、{{全书大纲}}、{{伏笔档案}}",
                "世界观规则" => "；旧变量已移除，请使用 {{故事圣经}}",
                "近期事件" => "；旧变量已移除，请使用 {{历史事件}}",
                "伏笔" => "；旧变量已移除，请使用 {{伏笔档案}}",
                _ => "",
            };
            return Err(format!("第 {line} 行：未知提示词变量: {{{{{name}}}}}{hint}"));
        }
        output.push_str(values.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()).unwrap_or(""));
        rest = &tail[end + 2..];
    }
    output.push_str(rest);
    Ok(output)
}

fn validate_task_prompt(system: &str, template: &str, allowed: &[&str], required: &str) -> Result<(), String> {
    if system.trim().is_empty() || template.trim().is_empty() {
        return Err("系统提示词和任务提示词模板不能为空".into());
    }
    validate_prompt_variables(template, allowed, Some(required))
}

fn validate_prompt_variables(template: &str, allowed: &[&str], required: Option<&str>) -> Result<(), String> {
    render_prompt_template(template, &[])?;
    let re = regex::Regex::new(r"\{\{([^{}]+)\}\}").unwrap();
    let mut has_required = false;
    for caps in re.captures_iter(template) {
        let name = caps[1].trim();
        if !allowed.contains(&name) && !(allowed == GENERATION_VARIABLES && reference_chapter(name).is_some()) {
            let line = template[..caps.get(0).unwrap().start()].bytes().filter(|b| *b == b'\n').count() + 1;
            return Err(format!("第 {line} 行：当前分类不支持变量: {name}"));
        }
        has_required |= Some(name) == required;
    }
    if let Some(required) = required {
        if !has_required { return Err(format!("模板必须包含 {{{{{}}}}}", required)); }
    }
    Ok(())
}

fn validate_prompt_settings(settings: &PromptSettings) -> Result<(), String> {
    for task in prompt_tasks(settings) {
        task.validate(task.label != "对标方案")?;
    }
    Ok(())
}

const CONSISTENCY_VARIABLES: &[&str] = &["章节ID", "章节正文", "事件骨架", "角色档案", "故事圣经", "伏笔档案", "历史事件", "时间线", "章节摘要"];
const MEMORY_VARIABLES: &[&str] = &["章节ID", "章节正文", "角色档案", "伏笔档案"];

struct PromptTaskSpec<'a> {
    label: &'static str,
    system: &'a str,
    template: &'a str,
    allowed: &'static [&'static str],
    required: &'static str,
}
impl PromptTaskSpec<'_> {
    fn validate(&self, complete: bool) -> Result<(), String> {
        if complete && self.system.trim().is_empty() {
            return Err(format!("{} → 系统提示词：不能为空", self.label));
        }
        let result = if complete {
            validate_task_prompt(self.system, self.template, self.allowed, self.required)
        } else { validate_prompt_variables(self.template, self.allowed, None) };
        result.map_err(|error| format!("{} → 任务模板：{error}", self.label))
    }
}
fn prompt_tasks(settings: &PromptSettings) -> [PromptTaskSpec<'_>; 5] {
    [
        PromptTaskSpec { label: "通用方案", system: &settings.system, template: &settings.user_template, allowed: GENERATION_VARIABLES, required: "事件骨架" },
        PromptTaskSpec { label: "精简四段式", system: &settings.compact_generation.system, template: &settings.compact_generation.user_template, allowed: GENERATION_VARIABLES, required: "事件骨架" },
        PromptTaskSpec { label: "对标方案", system: &settings.benchmark_generation.system, template: &settings.benchmark_generation.user_template, allowed: GENERATION_VARIABLES, required: "事件骨架" },
        PromptTaskSpec { label: "一致性检查", system: &settings.consistency.system, template: &settings.consistency.user_template, allowed: CONSISTENCY_VARIABLES, required: "章节正文" },
        PromptTaskSpec { label: "更新记忆", system: &settings.memory.system, template: &settings.memory.user_template, allowed: MEMORY_VARIABLES, required: "章节正文" },
    ]
}

fn assemble_task_prompt(task: &TaskPrompt, values: &[(String, String)], settings: &Settings) -> Result<PromptPair, String> {
    Ok(PromptPair { system: task.system.clone(), user: apply_no_think(render_prompt_template(&task.user_template, values)?, settings) })
}

fn render_generation_template(template: &str, values: &[(String, String)]) -> Result<String, String> {
    let variable = regex::Regex::new(r"\{\{([^{}]+)\}\}").unwrap();
    let mut blocks: Vec<String> = vec![String::new()];
    for line in template.lines() {
        if line.trim().starts_with('【') && line.trim().contains('】') { blocks.push(String::new()); }
        blocks.last_mut().unwrap().push_str(line);
        blocks.last_mut().unwrap().push('\n');
    }
    let mut kept = String::new();
    for block in blocks {
        let names: Vec<_> = variable.captures_iter(&block).map(|c| c[1].trim().to_string()).collect();
        let mut lines = block.lines();
        let first = lines.next().unwrap_or("").trim();
        let body = if first.starts_with('【') && first.find('】') == Some(first.len().saturating_sub('】'.len_utf8())) {
            lines.collect::<Vec<_>>().join("\n")
        } else { block.clone() };
        let only_variables = variable.replace_all(&body, "").trim().is_empty();
        if only_variables && !names.is_empty() && names.iter().all(|n| values.iter().find(|(k, _)| k == n).map_or(true, |(_, v)| v.trim().is_empty())) { continue; }
        kept.push_str(&block);
    }
    render_prompt_template(kept.trim(), values)
}

fn normalize_idea(idea: &str) -> String {
    let mut lines: Vec<_> = idea.lines().collect();
    while lines.first().map_or(false, |line| {
        let title = line.trim().trim_start_matches('#').trim();
        title.is_empty() || title == "【事件骨架】" || title == "事件骨架"
    }) { lines.remove(0); }
    lines.join("\n").trim().to_string()
}

fn assemble_generation_prompt(
    project_dir: &Path, settings: &Settings, chapter_id: &str, idea: &str,
    target_words: u32, mem: &MemorySections, form_block: &str, preset: Option<GenerationPreset>,
) -> Result<PromptPreview, String> {
    let template = load_prompt_settings(project_dir)?;
    let preset = preset.unwrap_or(template.generation_preset);
    let index = match preset { GenerationPreset::General => 0, GenerationPreset::Compact => 1, GenerationPreset::Benchmark => 2 };
    prompt_tasks(&template)[index].validate(true)
        .map_err(|error| if preset == GenerationPreset::Benchmark { format!("对标方案尚未配置完整：{error}") } else { error })?;
    let (system, user_template, material_guidance) = match preset {
        GenerationPreset::General => (&template.system, &template.user_template, template.general_material_guidance),
        GenerationPreset::Compact => (&template.compact_generation.system, &template.compact_generation.user_template, template.compact_material_guidance),
        GenerationPreset::Benchmark => (&template.benchmark_generation.system, &template.benchmark_generation.user_template, template.benchmark_material_guidance),
    };
    let get = |k: &str| mem.sections.get(k).cloned().unwrap_or_default();
    let mut values = vec![
        ("角色档案".into(), get("Characters")),
        ("章节摘要".into(), get("Recent Chapter Summaries")),
        ("历史事件".into(), get("Recent Events")),
        ("时间线".into(), get("Timeline")),
        ("故事圣经".into(), get("Story Bible")),
        ("全书大纲".into(), get("Book Outline")),
        ("伏笔档案".into(), get("Active Foreshadowing")),
        ("文风参考".into(), get("Style References")),
        ("事件骨架".into(), normalize_idea(idea)),
        ("目标字数".into(), target_words.to_string()),
        ("叙事视角".into(), get("Narrative POV")),
        ("叙事要求".into(), form_block.into()),
        ("章节ID".into(), chapter_id.into()),
    ];
    let mut reference_sources = Vec::new();
    for caps in regex::Regex::new(r"\{\{([^{}]+)\}\}").unwrap().captures_iter(user_template) {
        let name = caps[1].trim();
        if let Some(id) = reference_chapter(name) {
            let number = project_source::number(id).ok_or("参考章号无效")?;
            if number >= project_source::number(chapter_id).ok_or("目标章号无效")? {
                return Err(format!("参考正文 {id} 必须早于目标章"));
            }
            let files = project_source::chapters(project_dir)?;
            let file = files.get(&number).ok_or_else(|| format!("参考正文 {id} 不存在"))?;
            let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
            reference_sources.push(context::Source { kind: "chapter".into(), id: id.into(), file: file.display().to_string(),
                reason: "模板明确引用该章正文".into(), included: true, characters: text.chars().count() });
            values.push((name.into(), text));
        }
    }
    let user = render_generation_template(user_template, &values)?;
    let variables: std::collections::HashSet<String> = regex::Regex::new(r"\{\{([^{}]+)\}\}").unwrap()
        .captures_iter(user_template).map(|c| c[1].trim().to_string()).collect();
    let uses_state = values.iter().any(|(key, value)| ["角色档案", "章节摘要", "历史事件", "时间线", "故事圣经", "全书大纲", "伏笔档案"].contains(&key.as_str()) && variables.contains(key) && !value.trim().is_empty());
    let user = if uses_state && material_guidance {
        format!("【资料使用原则】\n以下人物状态与前情描述本章开始前的情况；本章可按事件骨架取得物品、获知信息或改变状态。章初的‘尚未’不等于本章禁止发生；骨架未安排的未来事项不得自行展开。\n\n{}", user)
    } else { user };
    let prompts = PromptPair { system: system.clone(), user: apply_no_think(user, settings) };
    // 包含项目和调用输入，防止跨项目、跨章或修改设置后误用旧预览。
    let source = serde_json::to_string(&(project_dir, chapter_id, idea, target_words,
        settings.llm_provider.as_deref(), settings.disable_thinking, preset, material_guidance, &prompts)).map_err(|e| e.to_string())?;
    let mut stats = mem.stats.clone();
    stats.sources.extend(reference_sources);
    for source in &mut stats.sources {
        let key = match source.kind.as_str() {
            "character" => "角色档案".to_string(), "summary" => "章节摘要".into(), "event" => "历史事件".into(),
            "timeline" => "时间线".into(), "chapter" => format!("参考正文:{}", source.id),
            "rules" => "故事圣经".into(), "outline" => "全书大纲".into(), "foreshadowing" => "伏笔档案".into(),
            "style" => "文风参考".into(), _ => String::new(),
        };
        if source.included && !variables.contains(&key) {
            source.included = false;
            source.reason = "模板未引用此类资料，未发送".into();
        }
    }
    stats.characters = prompts.system.chars().count() + prompts.user.chars().count();
    stats.estimated_tokens = context::tokens(&prompts.system) + context::tokens(&prompts.user);
    stats.over_budget = stats.estimated_tokens > stats.budget;
    if stats.over_budget { stats.warnings.push("输入超过预算，请减少自动资料、参考章或在预览中精简后确认；软件不会静默丢弃手动选择。".into()); }
    if !mem.state_at.is_empty() { stats.warnings.push(mem.state_at.clone()); }
    Ok(PromptPreview { prompts, source, stats })
}

fn prepare_generation(
    root: &Path, settings: &Settings, id: &str, idea: &str, target_words: u32,
    use_context: bool, pov: &str, narrative: &str, options: &context::Options,
) -> Result<(PromptPreview, MemorySections), String> {
    let mut effective_options = options.clone();
    effective_options.generation_preset = Some(options.generation_preset.unwrap_or(load_prompt_settings(root)?.generation_preset));
    let mut mem = context::build(root, id, idea, use_context, &effective_options)?;
    let (default_pov, default_directives) = if !use_context || mem.stats.warnings.iter().any(|w| w.contains("快照")) {
        (String::new(), Vec::new())
    } else { read_narrative_defaults(root) };
    let effective_pov = if pov.trim().is_empty() { default_pov } else { pov.trim().to_string() };
    let directives = if narrative.trim().is_empty() { default_directives } else {
        narrative.split(|c| c == ',' || c == '，' || c == '\n').map(str::to_string).collect()
    };
    mem.sections.insert("Narrative POV".into(), build_form_block(&effective_pov, &[]));
    let form = build_form_block("", &directives);
    let mut preview = assemble_generation_prompt(root, settings, id, idea, target_words, &mem, &form, effective_options.generation_preset)?;
    preview.source = serde_json::to_string(&(&preview.source, options, project_source::chapter_path(root, id)?)).map_err(|e| e.to_string())?;
    Ok((preview, mem))
}

pub fn choose_generation_prompt(preview: PromptPreview, custom: Option<PromptOverride>) -> Result<PromptPair, String> {
    let limit = preview.stats.budget;
    let prompts = match custom {
        Some(custom) => {
            if custom.source != preview.source { return Err("提示词来源已变化，请重新打开提示词预览并确认。".into()); }
            custom.prompts
        }
        None => preview.prompts,
    };
    if prompts.system.trim().is_empty() || prompts.user.trim().is_empty() { return Err("系统提示词和章节提示词不能为空".into()); }
    if context::tokens(&prompts.system) + context::tokens(&prompts.user) > limit {
        return Err("实际输入超过上下文预算，请精简预览或提高输入预算后重新确认。".into());
    }
    Ok(prompts)
}

pub fn context_catalog(app: Context, chapter_id: String) -> Result<context::Catalog, String> {
    let chapter_id = canonical_chapter_id(&chapter_id)?;
    let _access = app.read_access()?;
    context::catalog(&resolve_project_dir(&app), &chapter_id)
}

pub fn get_source_settings(app: Context) -> Result<project_source::SourceSettings, String> {
    let _access = app.read_access()?;
    project_source::load(&resolve_project_dir(&app))
}

pub fn save_source_settings(app: Context, settings: project_source::SourceSettings) -> Result<(), String> {
    let _access = app.write_access()?;
    if settings.directory.trim().is_empty() { return Err("正文目录不能为空".into()); }
    let root = resolve_project_dir(&app);
    let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    atomic_write(&root.join(".chapter-source.json"), text.as_bytes())
}

pub fn get_prompt_settings(app: Context) -> Result<PromptSettings, String> {
    let _access = app.read_access()?;
    load_prompt_settings(&resolve_project_dir(&app))
}

pub fn default_prompt_settings() -> PromptSettings { PromptSettings::default() }

pub fn save_prompt_settings(app: Context, settings: PromptSettings) -> Result<(), String> {
    let _access = app.write_access()?;
    validate_prompt_settings(&settings)?;
    let path = resolve_project_dir(&app).join(".prompt-settings.json");
    project_manager::atomic_json(&path, &settings)
}

pub fn preview_generation_prompt(app: Context, chapter_id: String, idea: String,
    target_words: u32, use_context: bool, pov: String, narrative: String, context_options: Option<context::Options>,
) -> Result<PromptPreview, String> {
    let chapter_id = canonical_chapter_id(&chapter_id)?;
    let _access = app.read_access()?;
    let project_dir = resolve_project_dir(&app);
    let settings = load_settings(&app);
    prepare_generation(&project_dir, &settings, &chapter_id, &idea, target_words, use_context,
        &pov, &narrative, &context_options.unwrap_or_default()).map(|(preview, _)| preview)
}

/// 把 POV 预设展开成英文指令；并组装「形式 / 叙事控制」英文区块。
/// 经验:形式(POV/规则)用英文表述更能贴合模型 CoT，内容(idea/记忆)保持中文。
fn build_form_block(pov: &str, directives: &[String]) -> String {
    let mut lines = Vec::new();
    let pov = pov.trim();
    if pov == "objective" { lines.push("客观旁观视角：只写可见动作和可听言语，不直接进入人物内心。".to_string()); }
    else if let Some(who) = pov.strip_prefix("limited") { lines.push(format!("第三人称限制视角：{}；其他角色的心理通过其言行呈现。", who.trim_start_matches(':'))); }
    else if let Some(who) = pov.strip_prefix("first") { lines.push(format!("第一人称视角：{}。", who.trim_start_matches(':'))); }
    else if pov == "omniscient" { lines.push("全知视角，场景内避免无过渡切换人物视角。".into()); }
    else if !pov.is_empty() { lines.push(pov.into()); }
    for directive in directives { if !directive.trim().is_empty() { lines.push(directive.trim().into()); } }
    lines.join("\n")
}

/// 读取 story_bible.yaml 的书级叙事默认：narrative_defaults: { pov, directives: [...] }
fn read_narrative_defaults(project_dir: &Path) -> (String, Vec<String>) {
    let sb = load_yaml(&project_dir.join("memory").join("story_bible.yaml"));
    let nd = match sb.get("narrative_defaults") {
        Some(v) => v,
        None => return (String::new(), Vec::new()),
    };
    let pov = nd
        .get("pov")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let directives = nd
        .get("directives")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    (pov, directives)
}

/// 从 idea 文本中提取关键词（移植 Python 的 idea_terms）
fn idea_terms(idea: &str) -> std::collections::HashSet<String> {
    let mut terms = std::collections::HashSet::new();
    let re = regex::Regex::new(r"[一-鿿A-Za-z0-9_]{2,}").unwrap();
    for mat in re.find_iter(idea) {
        let chunk = mat.as_str();
        if chunk.chars().count() <= 6 {
            terms.insert(chunk.to_string());
        }
        let chars: Vec<char> = chunk.chars().collect();
        for i in 0..chars.len().saturating_sub(1) {
            let bigram: String = chars[i..=i + 1].iter().collect();
            terms.insert(bigram);
        }
    }
    terms
}

/// 轻量字面冲突提示（移植 Python 的 detect_potential_conflicts）
fn detect_potential_conflicts(idea: &str, sources: &[(String, String)]) -> Vec<String> {
    let terms = idea_terms(idea);
    if terms.is_empty() {
        return Vec::new();
    }

    let markers = [
        "年龄", "岁", "天气", "地点", "位置", "金额", "价格", "物品", "身份",
        "状态", "死亡", "失踪", "禁止", "不能", "不要", "秘密", "已", "已经",
        "任务", "奖励", "发现",
    ];

    let mut warnings = Vec::new();
    for (label, text) in sources {
        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.chars().count() < 6 || !markers.iter().any(|m| line.contains(m)) {
                continue;
            }
            let overlap: Vec<&String> = terms
                .iter()
                .filter(|t| t.chars().count() >= 2 && line.contains(t.as_str()))
                .collect();
            if overlap.len() >= 2 {
                let truncated: String = line.chars().take(180).collect();
                warnings.push(format!("- [{}] {}", label, truncated));
                if warnings.len() >= 8 {
                    return warnings;
                }
            }
        }
    }
    warnings
}

/// 清理模型输出（移植 Python 的 clean_output）
fn clean_output(raw: &str) -> String {
    let mut text = raw.trim().to_string();
    let prefixes = [
        "以下是小说正文：",
        "以下是正文：",
        "下面是小说正文：",
        "下面是正文：",
        "好的，",
        "好的。",
    ];
    for prefix in &prefixes {
        if text.starts_with(prefix) {
            text = text[prefix.len()..].trim().to_string();
            break;
        }
    }
    if text.starts_with("```") {
        if let Some(first_nl) = text.find('\n') {
            text = text[first_nl + 1..].to_string();
        }
        if text.trim_end().ends_with("```") {
            let trimmed = text.trim_end();
            text = trimmed[..trimmed.len() - 3].trim_end().to_string();
        }
    }
    text
}

fn write_generation_report(
    project_dir: &Path,
    chapter_id: &str,
    idea: &str,
    target_words: u32,
    used_context: bool,
    context_tokens: usize,
    output_text: &str,
    raw_response: &str,
    _user_prompt: &str,
    conflict_warnings: &[String],
    config: &RunConfig,
) -> Result<(), String> {
    let warning_text = if conflict_warnings.is_empty() {
        "未发现明显字面冲突；仍建议作者人工审阅长期记忆与本章 idea。".to_string()
    } else {
        conflict_warnings.join("\n")
    };

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");

    // 模型信息按路线显示：api 显示 base_url；订阅 CLI 显示 provider，model 留空记为「订阅默认」
    let model_disp = if config.model_name.trim().is_empty() {
        "(订阅默认)".to_string()
    } else {
        config.model_name.clone()
    };
    let model_info = if config.provider == "api" {
        format!("- model: {}\n- base_url: {}", config.model_name, config.llm_base_url)
    } else {
        format!("- provider: {}\n- model: {}", config.provider, model_disp)
    };

    let report = format!(
        r#"# Chapter Generation Report: {chapter_id}

## Chapter ID

{chapter_id}

## Idea

{idea}

## Target Words

{target_words}

## Used Context

{used_context}

## Context Token Estimate

{context_tokens}

## Potential Context Conflicts

{warning_text}

## Output Length

{output_len} characters

## Model Info

{model_info}

## Generation Time

{now}

## Raw Response

```text
{raw_response}
```"#,
        chapter_id = chapter_id,
        idea = idea,
        target_words = if target_words == 0 { "由手动完整提示词决定".to_string() } else { target_words.to_string() },
        used_context = used_context,
        context_tokens = context_tokens,
        warning_text = warning_text,
        output_len = output_text.chars().count(),
        model_info = model_info,
        now = now,
        raw_response = raw_response,
    );

    let report_path = project_dir
        .join("outputs")
        .join("reports")
        .join(format!("{}_chapter_generation_report.md", chapter_id));
    write_text(&report_path, &report)?;
    Ok(())
}

fn do_generate_chapter(
    app: &Context,
    chapter_id: &str,
    idea: &str,
    target_words: u32,
    use_context: bool,
    overwrite: bool,
    pov: &str,
    narrative: &str,
    prompt_override: Option<PromptOverride>,
    context_options: context::Options,
    manual_prompts: Option<PromptPair>,
) -> CommandResult {
    let config = match build_run_config(app) {
        Ok(c) => c,
        Err(e) => return CommandResult { success: false, log: e },
    };
    let settings = load_settings(app);

    let chapter_path = match project_source::chapter_path(&config.project_dir, chapter_id) {
        Ok(p) => p,
        Err(e) => return CommandResult { success: false, log: e },
    };

    if chapter_path.exists() && !overwrite {
        return CommandResult {
            success: false,
            log: format!("{} 已存在。请勾选「覆盖」以重新生成。", chapter_path.display()),
        };
    }

    let (prompts, conflict_warnings) = if let Some(prompts) = manual_prompts {
        match preview_manual_prompt(prompts, context_options.input_budget) {
            Ok(preview) => (preview.prompts, Vec::new()),
            Err(e) => return CommandResult { success: false, log: e },
        }
    } else {
    let (preview, mem) = match prepare_generation(&config.project_dir, &settings, chapter_id, idea,
        target_words, use_context, pov, narrative, &context_options) {
        Ok(p) => p, Err(e) => return CommandResult { success: false, log: e },
    };
    let prompts = match choose_generation_prompt(preview, prompt_override) {
        Ok(p) => p,
        Err(e) => return CommandResult { success: false, log: e },
    };
    let conflict_warnings = if use_context {
        detect_potential_conflicts(idea, &mem.conflict_sources)
    } else {
        Vec::new()
    };
    (prompts, conflict_warnings)
    };
    let user_prompt = prompts.user;
    let context_tokens = context::tokens(&prompts.system) + context::tokens(&user_prompt);

    let mut log = String::new();
    log.push_str(&format!("[信息] 章节: {}\n", chapter_id));
    if target_words == 0 { log.push_str("[信息] 目标长度: 由手动完整提示词决定\n"); }
    else { log.push_str(&format!("[信息] 目标长度: 约 {} 中文字\n", target_words)); }
    log.push_str(&format!("[信息] 使用长期记忆: {}\n", use_context));
    log.push_str(&format!("[信息] prompt token 估算: ~{}\n", context_tokens));

    let messages = sys_user_messages(&prompts.system, &user_prompt);

    let temperature = settings.temperature.unwrap_or(0.8);
    let top_p = settings.top_p.unwrap_or(0.9);

    log.push_str("[信息] 正在调用模型生成完整章节...\n");

    attempt!(app.stage("调用模型"));
    let raw_response = match run_completion(
        &config,
        &settings,
        messages,
        temperature,
        top_p,
        config.max_output_tokens,
    ) {
        Ok(r) => r,
        Err(e) => {
            return CommandResult {
                success: false,
                log: format!("{}[错误] 模型调用失败: {}", log, e),
            };
        }
    };

    attempt!(app.stage("解析生成结果"));
    let output_text = clean_output(&raw_response);

    attempt!(app.stage("保存结果"));
    let _commit = attempt!(app.commit_access());
    if let Err(e) = project_source::write(&config.project_dir, &chapter_path, &output_text) {
        return CommandResult { success: false, log: format!("正文保存失败：{}", e) };
    }
    log.push_str(&format!("[完成] 章节正文已保存: {}\n", chapter_path.display()));

    if let Err(error) = write_generation_report(
        &config.project_dir,
        chapter_id,
        idea,
        target_words,
        use_context,
        context_tokens,
        &output_text,
        &raw_response,
        &user_prompt,
        &conflict_warnings,
        &config,
    ) {
        return CommandResult { success: false, log: format!("{log}[错误] 正文已保存，但生成报告保存失败：{error}") };
    }
    log.push_str("[完成] 生成报告已保存\n");

    CommandResult {
        success: true,
        log: log.trim().to_string(),
    }
}

// ============================================================
// 4.2 check_consistency（移植 check_consistency.py）
// ============================================================

const CONSISTENCY_CHECK_PROMPT: &str = r#"你是一位经验丰富的小说编辑，专门负责长篇小说的连续性审查。

请仔细阅读以下完整章节正文和相关记忆档案，找出所有一致性问题。

## 检查清单

1. **人物语气一致性**: 角色的说话方式是否与角色档案中描述的一致？
2. **信息超前**: 角色是否知道了他们不应该知道的信息？（参考 knows 和 secrets 字段）
3. **世界观冲突**: 正文中是否违反了 Story Bible 中的世界观规则？
4. **时间线矛盾**: 事件的时间顺序是否有矛盾？
5. **伏笔问题**: 伏笔是否遗漏、误回收、过早揭示或互相冲突？
6. **重复事件**: 是否重复前文已发生的同一事件？
7. **重复任务/奖励/发现**: 是否重复触发同一任务，重复发放同一奖励，或重复发现同一信息？
8. **关键事实漂移**: 年龄、天气、地点、金额、物品、身份、任务状态等是否前后改变？
9. **AI 腔检查**: 是否有明显的 AI 生成痕迹？（总结腔、解释腔、列举句式等）
10. **Chapter Idea 执行**: 是否违背或偏离作者给出的 chapter idea？

## 章节正文

{chapter_text}

## 作者 Chapter Idea

{chapter_idea}

## 角色档案

{characters_text}

## 世界观规则

{world_rules_text}

## 活跃伏笔

{foreshadowing_text}

## 最近事件

{events_text}

## 最近时间线

{timeline_text}

## 输出要求

请输出一份 Markdown 格式的审查报告。格式如下：

# 一致性审查报告: {chapter_id}

## 总体评价
（一段话概括质量）

## 发现的问题

### 严重问题
- **问题**: （描述）
  - **位置**: （大约在哪一段）
  - **原因**: （为什么是问题）
  - **建议**: （如何修复）

### 中等问题
- ...

### 轻微问题/建议
- ...

## 亮点
（如果有的话，指出写得好的地方）

如果没有任何问题，请如实说明。"#;

/// 从生成报告中解析 chapter idea
fn load_chapter_idea(project_dir: &Path, chapter_id: &str) -> String {
    let report_path = project_dir
        .join("outputs")
        .join("reports")
        .join(format!("{}_chapter_generation_report.md", chapter_id));
    let text = read_text(&report_path);
    if text.is_empty() {
        return "（未找到生成报告中的 chapter idea；如需检查此项，请参考作者原始输入。）".to_string();
    }
    let re = regex::Regex::new(r"(?s)## Idea\s+(.*?)\s+## Target Words").unwrap();
    match re.captures(&text) {
        Some(caps) => caps.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_else(|| {
            "（未能从生成报告解析 chapter idea；如需检查此项，请参考作者原始输入。）".to_string()
        }),
        None => "（未能从生成报告解析 chapter idea；如需检查此项，请参考作者原始输入。）".to_string(),
    }
}

fn do_check_consistency(app: &Context, chapter_id: &str) -> CommandResult {
    let config = match build_run_config(app) {
        Ok(c) => c,
        Err(e) => return CommandResult { success: false, log: e },
    };
    let settings = load_settings(app);

    let chapter_path = match project_source::chapter_path(&config.project_dir, chapter_id) {
        Ok(p) => p,
        Err(e) => return CommandResult { success: false, log: e },
    };

    if !chapter_path.exists() {
        return CommandResult {
            success: false,
            log: format!(
                "[错误] 未找到 {}\n[提示] 请先生成该章节。",
                chapter_path.display()
            ),
        };
    }

    let chapter_text = read_text(&chapter_path);
    if chapter_text.trim().is_empty() {
        return CommandResult {
            success: false,
            log: format!("[错误] {} 是空文件", chapter_path.display()),
        };
    }

    let chapter_idea = load_chapter_idea(&config.project_dir, chapter_id);

    let selected = match context::build(&config.project_dir, chapter_id, &chapter_idea, true, &context::Options::default()) {
        Ok(m) => m, Err(e) => return CommandResult { success: false, log: e },
    };
    let section = |name: &str| selected.sections.get(name).cloned().unwrap_or_default();
    let characters_text = section("Characters");
    let world_rules_text = section("Story Bible");
    let foreshadowing_text = section("Active Foreshadowing");
    let events_text = section("Recent Events");
    let timeline_text = section("Timeline");

    let templates = match load_prompt_settings(&config.project_dir) {
        Ok(t) => t,
        Err(e) => return CommandResult { success: false, log: e },
    };
    if let Err(error) = prompt_tasks(&templates)[3].validate(true) {
        return CommandResult { success: false, log: error };
    }
    let prompts = match assemble_task_prompt(&templates.consistency, &[
        ("章节ID".into(), chapter_id.into()), ("章节正文".into(), chapter_text.clone()),
        ("事件骨架".into(), chapter_idea), ("角色档案".into(), characters_text),
        ("故事圣经".into(), world_rules_text), ("伏笔档案".into(), foreshadowing_text),
        ("历史事件".into(), events_text), ("时间线".into(), timeline_text),
        ("章节摘要".into(), section("Recent Chapter Summaries")),
    ], &settings) {
        Ok(p) => p,
        Err(e) => return CommandResult { success: false, log: e },
    };
    let messages = sys_user_messages(&prompts.system, &prompts.user);

    let top_p = settings.top_p.unwrap_or(0.9);

    let mut log = format!("[信息] 正在对 {} 进行一致性审查...\n", chapter_id);

    attempt!(app.stage("调用模型"));
    let report = match run_completion(
        &config,
        &settings,
        messages,
        0.4,
        top_p,
        config.max_output_tokens,
    ) {
        Ok(r) => r,
        Err(e) => {
            return CommandResult {
                success: false,
                log: format!("{}[错误] 模型调用失败: {}", log, e),
            };
        }
    };

    attempt!(app.stage("解析检查结果"));
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let model_label = if config.provider == "api" {
        let config_yaml = load_config_yaml(&config.project_dir);
        config_string(&config_yaml, "model_name", "未知")
    } else {
        let m = if config.model_name.trim().is_empty() {
            "(订阅默认)".to_string()
        } else {
            config.model_name.clone()
        };
        format!("{} / {}", config.provider, m)
    };
    let header = format!(
        "<!-- 一致性审查报告 -->\n<!-- 生成时间: {} -->\n<!-- 模型: {} -->\n\n",
        now, model_label
    );

    let report_path = config
        .project_dir
        .join("outputs")
        .join("reports")
        .join(format!("{}_consistency_report.md", chapter_id));
    attempt!(app.stage("保存结果"));
    let _commit = attempt!(app.commit_access());
    attempt!(write_text(&report_path, &format!("{}{}", header, report)));

    log.push_str(&format!(
        "[完成] 一致性审查报告已保存: {}\n",
        report_path.display()
    ));

    CommandResult {
        success: true,
        log: log.trim().to_string(),
    }
}

// ============================================================
// 4.3 update_memory（移植 update_memory_after_chapter.py）
// ============================================================

const EXTRACTION_PROMPT_TEMPLATE: &str = r#"你是一位专业的小说编辑助手。请阅读以下完整章节正文，然后以严格的JSON格式输出结构化的长期记忆更新信息。

## 章节正文

{chapter_text}

## 现有角色档案

{characters_text}

## 现有伏笔

{foreshadowing_text}

## 输出要求

请输出一个严格的JSON对象，不要包含任何其他文字、解释或markdown标记。JSON schema 如下：

{
  "chapter_id": "{chapter_id}",
  "chapter_summary": "用2-3句话概括本章发生了什么",
  "events": [
    {
      "event_id": "{chapter_id}_evt001",
      "chapter_id": "{chapter_id}",
      "type": "chapter_event/conflict/revelation/decision/travel/battle/task/reward/discovery",
      "characters": ["角色名"],
      "description": "事件描述",
      "location": "地点",
      "time_order": "相对时间描述",
      "importance": 5
    }
  ],
  "character_updates": [
    {
      "name": "角色名",
      "field": "current_status/knows/secrets/relationships/constraints",
      "old_value": "旧值（如果知道的话）",
      "new_value": "新值",
      "reason": "变化原因"
    }
  ],
  "foreshadowing_updates": [
    {
      "id": "伏笔ID",
      "action": "add/resolve/update",
      "description": "描述",
      "related_characters": ["角色名"],
      "planned_resolution": "计划回收方式",
      "status": "active/resolved"
    }
  ],
  "relationship_updates": [
    {
      "source": "角色A",
      "target": "角色B",
      "relation": "关系描述",
      "change": "变化说明"
    }
  ],
  "continuity_risks": [
    {
      "risk": "风险描述",
      "severity": "low/medium/high",
      "suggestion": "建议"
    }
  ]
}"#;

// 用户可以编辑抽取模板，但写入前必须确认仍符合程序依赖的输出结构。
fn validate_memory_extraction(data: &serde_json::Value, chapter_id: &str) -> Result<(), String> {
    if data.get("chapter_id").and_then(|v| v.as_str()) != Some(chapter_id) {
        return Err("记忆输出的 chapter_id 与当前章节不一致，未写入记忆".into());
    }
    if data.get("chapter_summary").and_then(|v| v.as_str()).map_or(true, |s| s.trim().is_empty()) {
        return Err("记忆输出缺少非空 chapter_summary，未写入记忆".into());
    }
    for field in ["events", "character_updates", "foreshadowing_updates", "relationship_updates", "continuity_risks"] {
        if !data.get(field).map_or(false, |v| v.is_array()) {
            return Err(format!("记忆输出的 {} 必须为数组，未写入记忆", field));
        }
    }
    for event in data["events"].as_array().unwrap() {
        if event.get("description").and_then(|v| v.as_str()).map_or(true, |s| s.trim().is_empty()) {
            return Err("记忆事件缺少 description，未写入记忆".into());
        }
        if let Some(id) = event.get("chapter_id") {
            if id.as_str() != Some(chapter_id) { return Err("事件章号不一致，未写入记忆".into()); }
        }
    }
    Ok(())
}

// 抽取偶尔会返回合法但全空的模板；反馈校验错误后最多重试一次。
// 两次结果都必须通过同一校验，调用者只会收到可以写入的结果。
fn extract_memory_with_retry<F>(
    mut messages: Vec<llm::ChatMessage>,
    chapter_id: &str,
    log: &mut String,
    mut complete: F,
) -> Result<(serde_json::Value, String), String>
where
    F: FnMut(Vec<llm::ChatMessage>) -> Result<String, String>,
{
    messages.insert(0, llm::ChatMessage {
        role: "system".into(),
        content: format!(
            "请根据本次提供的章节正文抽取真实内容，不要返回空白模板。只输出一个 JSON 对象。\
chapter_id 必须为 {chapter_id}；chapter_summary 必须是非空字符串，用2-3句话概括本章实际发生的事情。\
events、character_updates、foreshadowing_updates、relationship_updates、continuity_risks 必须为数组；\
没有相应变化的数组可以为空，但章节摘要不能留空。events 中每项必须有非空 description，\
如含 chapter_id 必须与本章一致。importance 使用1到10的整数。不要编造正文中未发生的内容。"
        ),
    });
    let mut diagnostics = String::new();
    for attempt in 1..=2 {
        // 保留原始正文和资料，每次都发送完整上下文。
        let request = messages.iter().map(|m| llm::ChatMessage {
            role: m.role.clone(), content: m.content.clone(),
        }).collect();
        let raw = complete(request).map_err(|e| {
            log.push_str(&format!("[错误] 模型调用失败: {}\n", e));
            format!("{}\n模型调用失败: {}", diagnostics, e)
        })?;
        let result = parse_extraction_json(&raw).and_then(|data| {
            validate_memory_extraction(&data, chapter_id)?;
            Ok(data)
        });
        match result {
            Ok(data) => return Ok((data, raw)),
            Err(error) => {
                diagnostics.push_str(&format!("## 第 {} 次抽取\n\n{}\n\n原始输出：\n{}\n\n", attempt, error, raw));
                if attempt == 2 {
                    log.push_str(&format!("[错误] 重试结果仍未通过校验：{}\n", error));
                    return Err(diagnostics);
                }
                log.push_str(&format!("[警告] 首次记忆抽取未通过校验：{}\n[信息] 正在自动重试记忆抽取（1/1）...\n", error));
                messages.push(llm::ChatMessage { role: "assistant".into(), content: raw });
                messages.push(llm::ChatMessage {
                    role: "user".into(),
                    content: format!("上次输出校验失败：{}。请重新阅读上方完整章节正文，重新输出完整 JSON 对象，补全缺失字段并修正类型。尤其 chapter_summary 必须根据正文填写实际摘要，不能是空字符串。不要只返回修正的字段。", error),
                });
            }
        }
    }
    unreachable!()
}

/// 从模型输出中提取 JSON（移植 Python 的 parse_extraction_json）
fn parse_extraction_json(raw_output: &str) -> Result<serde_json::Value, String> {
    let mut text = raw_output.trim().to_string();

    // 尝试从 ```json ... ``` 中提取
    let re_codeblock = regex::Regex::new(r"(?s)```(?:json)?\s*\n?(.*?)\n?```").unwrap();
    if let Some(caps) = re_codeblock.captures(&text) {
        if let Some(m) = caps.get(1) {
            text = m.as_str().trim().to_string();
        }
    }

    // 如果不是以 { 或 [ 开头，截取第一个 { 到最后一个 }
    if !text.is_empty() && !text.starts_with('{') && !text.starts_with('[') {
        if let Some(start) = text.find('{') {
            text = text[start..].to_string();
        }
    }
    if let Some(last) = text.rfind('}') {
        text = text[..=last].to_string();
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
        return Ok(value);
    }

    // 容错：模型输出被 token 上限截断时，抢救已完整的部分（截到最后一个完整元素并补全括号）
    if let Some(value) = repair_truncated_json(&text) {
        return Ok(value);
    }

    let preview: String = raw_output.chars().take(500).collect();
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(value) => Ok(value),
        Err(e) => Err(format!("JSON解析失败: {}\n原始输出前500字:\n{}", e, preview)),
    }
}

/// 尝试修复被截断的 JSON：截到最后一个完整元素（最后一个 } / ] 或逗号之前），
/// 再根据未闭合的括号补全。仅按字节扫描 ASCII 结构字符，UTF-8 续字节不会被误判。
fn repair_truncated_json(s: &str) -> Option<serde_json::Value> {
    let bytes = s.as_bytes();
    let mut in_str = false;
    let mut esc = false;
    let mut best: usize = 0; // 安全截断点（独占）
    for (i, &b) in bytes.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'}' | b']' => best = i + 1, // 完整容器闭合后
            b',' => best = i,            // 逗号前（丢弃逗号及其后的残缺元素）
            _ => {}
        }
    }
    if best == 0 {
        return None;
    }

    let mut prefix = s[..best].to_string();

    // 重新计算 prefix 的未闭合括号栈
    let mut stack: Vec<char> = Vec::new();
    let mut in_s = false;
    let mut e = false;
    for &b in prefix.as_bytes() {
        if in_s {
            if e {
                e = false;
            } else if b == b'\\' {
                e = true;
            } else if b == b'"' {
                in_s = false;
            }
            continue;
        }
        match b {
            b'"' => in_s = true,
            b'{' => stack.push('}'),
            b'[' => stack.push(']'),
            b'}' | b']' => {
                stack.pop();
            }
            _ => {}
        }
    }
    while let Some(close) = stack.pop() {
        prefix.push(close);
    }

    serde_json::from_str(&prefix).ok()
}

/// 应用记忆更新（移植 Python 的 apply_memory_updates）
fn apply_memory_updates(data: &serde_json::Value, chapter_id: &str, project_dir: &Path) -> Result<(), String> {
    // 1. 追加章节摘要
    let summary = data
        .get("chapter_summary")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mut summary_obj = serde_json::Map::new();
    summary_obj.insert(
        "chapter_id".to_string(),
        serde_json::Value::String(chapter_id.to_string()),
    );
    summary_obj.insert(
        "chapter_summary".to_string(),
        serde_json::Value::String(summary.to_string()),
    );
    append_jsonl(
        &project_dir.join("memory").join("chapter_summaries.jsonl"),
        &serde_json::Value::Object(summary_obj),
    )?;

    // 2. 追加事件
    let events = data.get("events").and_then(|v| v.as_array());
    if let Some(events) = events {
        for evt in events {
            let mut evt = evt.clone();
            if let Some(obj) = evt.as_object_mut() {
                obj.entry("chapter_id".to_string()).or_insert_with(|| {
                    serde_json::Value::String(chapter_id.to_string())
                });
            }
            append_jsonl(
                &project_dir.join("memory").join("events.jsonl"),
                &evt,
            )?;
        }
    }

    // 3. 追加 timeline
    if let Some(events) = events {
        for evt in events {
            let event_id = evt
                .get("event_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let time_order = evt
                .get("time_order")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let description = evt
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let characters = evt
                .get("characters")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![]));
            let importance = evt
                .get("importance")
                .and_then(|v| v.as_u64())
                .unwrap_or(5);

            let mut tl_obj = serde_json::Map::new();
            tl_obj.insert(
                "event_id".to_string(),
                serde_json::Value::String(event_id.to_string()),
            );
            tl_obj.insert(
                "chapter_id".to_string(),
                serde_json::Value::String(chapter_id.to_string()),
            );
            tl_obj.insert(
                "time".to_string(),
                serde_json::Value::String(time_order.to_string()),
            );
            tl_obj.insert(
                "description".to_string(),
                serde_json::Value::String(description.to_string()),
            );
            tl_obj.insert("characters".to_string(), characters);
            tl_obj.insert(
                "importance".to_string(),
                serde_json::Value::Number(importance.into()),
            );
            append_jsonl(
                &project_dir.join("memory").join("timeline.jsonl"),
                &serde_json::Value::Object(tl_obj),
            )?;
        }
    }

    // 4. 更新角色档案
    if let Some(updates) = data.get("character_updates").and_then(|v| v.as_array()) {
        update_characters(updates, project_dir)?;
    }

    // 5. 更新伏笔
    if let Some(updates) = data.get("foreshadowing_updates").and_then(|v| v.as_array()) {
        update_foreshadowing(updates, project_dir)?;
    }

    // 6. 更新关系图
    if let Some(updates) = data.get("relationship_updates").and_then(|v| v.as_array()) {
        update_relationships(updates, project_dir)?;
    }
    Ok(())
}

fn update_characters(updates: &[serde_json::Value], project_dir: &Path) -> Result<(), String> {
    let chars_path = project_dir.join("memory").join("characters.yaml");
    let mut chars_data = load_yaml(&chars_path);

    // 构建 name -> index 映射
    let char_map: std::collections::HashMap<String, usize> = {
        let empty = vec![];
        let characters = chars_data
            .get("characters")
            .and_then(|v| v.as_sequence())
            .unwrap_or(&empty);
        characters
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                c.get("name")
                    .and_then(|v| v.as_str())
                    .map(|n| (n.to_string(), i))
            })
            .collect()
    };

    let characters = chars_data
        .get_mut("characters")
        .and_then(|v| v.as_sequence_mut());

    let characters = match characters {
        Some(c) => c,
        None => return Ok(()),
    };

    for update in updates {
        let name = update
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let field = update
            .get("field")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let new_value = update
            .get("new_value")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let idx = match char_map.get(name) {
            Some(i) => *i,
            None => {
                eprintln!("[警告] 角色 '{}' 不存在，跳过", name);
                continue;
            }
        };

        let char = &mut characters[idx];

        if field == "current_status" {
            if let serde_yaml::Value::Mapping(ref mut map) = char {
                map.insert(
                    serde_yaml::Value::String("current_status".to_string()),
                    serde_yaml::Value::String(new_value.to_string()),
                );
            }
        } else if field == "knows"
            || field == "secrets"
            || field == "constraints"
            || field == "relationships"
        {
            // 获取当前列表
            if let Some(list) = char.get(field).and_then(|v| v.as_sequence()) {
                // 检查是否已存在
                let exists = list
                    .iter()
                    .any(|v| v.as_str() == Some(new_value));
                if !exists {
                    if let Some(list) = char.get_mut(field).and_then(|v| v.as_sequence_mut()) {
                        list.push(serde_yaml::Value::String(new_value.to_string()));
                    }
                }
            } else {
                // 字段不存在或是 None，创建新列表
                if let Some(char_mapping) = char.as_mapping_mut() {
                    let mut new_list = serde_yaml::Sequence::new();
                    new_list.push(serde_yaml::Value::String(new_value.to_string()));
                    char_mapping.insert(
                        serde_yaml::Value::String(field.to_string()),
                        serde_yaml::Value::Sequence(new_list),
                    );
                }
            }
        }
    }

    save_yaml(&chars_path, &chars_data)?;
    Ok(())
}

fn update_foreshadowing(updates: &[serde_json::Value], project_dir: &Path) -> Result<(), String> {
    let fs_path = project_dir.join("memory").join("foreshadowing.yaml");
    let mut fs_data = load_yaml(&fs_path);

    // 确保 active 和 resolved 存在
    if fs_data.get("active").is_none() {
        if let Some(map) = fs_data.as_mapping_mut() {
            map.insert(
                serde_yaml::Value::String("active".to_string()),
                serde_yaml::Value::Sequence(serde_yaml::Sequence::new()),
            );
        }
    }
    if fs_data.get("resolved").is_none() {
        if let Some(map) = fs_data.as_mapping_mut() {
            map.insert(
                serde_yaml::Value::String("resolved".to_string()),
                serde_yaml::Value::Sequence(serde_yaml::Sequence::new()),
            );
        }
    }

    // 构建 active id -> index 映射
    let active_map: std::collections::HashMap<String, usize> = {
        let empty_seq = serde_yaml::Sequence::new();
        let active = fs_data
            .get("active")
            .and_then(|v| v.as_sequence())
            .unwrap_or(&empty_seq);
        active
            .iter()
            .enumerate()
            .filter_map(|(i, fs)| {
                fs.get("id")
                    .and_then(|v| v.as_str())
                    .map(|id| (id.to_string(), i))
            })
            .collect()
    };

    for update in updates {
        let action = update
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let fs_id = update
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if action == "add" {
            let mut new_entry = serde_yaml::Mapping::new();
            new_entry.insert(
                serde_yaml::Value::String("id".to_string()),
                serde_yaml::Value::String(fs_id.to_string()),
            );
            let introduced = update
                .get("introduced_in")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            new_entry.insert(
                serde_yaml::Value::String("introduced_in".to_string()),
                serde_yaml::Value::String(introduced.to_string()),
            );
            let desc = update
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            new_entry.insert(
                serde_yaml::Value::String("description".to_string()),
                serde_yaml::Value::String(desc.to_string()),
            );
            let related: serde_yaml::Value = serde_json::from_str(
                &serde_json::to_string(
                    &update
                        .get("related_characters")
                        .cloned()
                        .unwrap_or(serde_json::Value::Array(vec![])),
                )
                .unwrap_or_default(),
            )
            .unwrap_or(serde_yaml::Value::Sequence(serde_yaml::Sequence::new()));
            new_entry.insert(
                serde_yaml::Value::String("related_characters".to_string()),
                related,
            );
            let planned = update
                .get("planned_resolution")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            new_entry.insert(
                serde_yaml::Value::String("planned_resolution".to_string()),
                serde_yaml::Value::String(planned.to_string()),
            );
            new_entry.insert(
                serde_yaml::Value::String("status".to_string()),
                serde_yaml::Value::String("active".to_string()),
            );
            if let Some(active) = fs_data
                .get_mut("active")
                .and_then(|v| v.as_sequence_mut())
            {
                active.push(serde_yaml::Value::Mapping(new_entry));
            }
        } else if action == "resolve" {
            if let Some(idx) = active_map.get(fs_id) {
                let active = fs_data
                    .get_mut("active")
                    .and_then(|v| v.as_sequence_mut());
                if let Some(active) = active {
                    if *idx < active.len() {
                        let mut entry = active.remove(*idx);
                        // 设置 status = resolved
                        if let Some(map) = entry.as_mapping_mut() {
                            map.insert(
                                serde_yaml::Value::String("status".to_string()),
                                serde_yaml::Value::String("resolved".to_string()),
                            );
                        }
                        if let Some(resolved) = fs_data
                            .get_mut("resolved")
                            .and_then(|v| v.as_sequence_mut())
                        {
                            resolved.push(entry);
                        }
                    }
                }
            }
        } else if action == "update" {
            if let Some(idx) = active_map.get(fs_id) {
                let active = fs_data
                    .get_mut("active")
                    .and_then(|v| v.as_sequence_mut());
                if let Some(active) = active {
                    if *idx < active.len() {
                        let entry = &mut active[*idx];
                        if let Some(desc) = update.get("description").and_then(|v| v.as_str()) {
                            if let Some(map) = entry.as_mapping_mut() {
                                map.insert(
                                    serde_yaml::Value::String("description".to_string()),
                                    serde_yaml::Value::String(desc.to_string()),
                                );
                            }
                        }
                        if let Some(planned) =
                            update.get("planned_resolution").and_then(|v| v.as_str())
                        {
                            if let Some(map) = entry.as_mapping_mut() {
                                map.insert(
                                    serde_yaml::Value::String("planned_resolution".to_string()),
                                    serde_yaml::Value::String(planned.to_string()),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    save_yaml(&fs_path, &fs_data)?;
    Ok(())
}

fn update_relationships(updates: &[serde_json::Value], project_dir: &Path) -> Result<(), String> {
    let rel_path = project_dir.join("memory").join("relationships.json");
    let mut rel_data = load_json(&rel_path);

    // 确保 nodes 和 edges 存在
    if rel_data.get("nodes").is_none() {
        if let Some(obj) = rel_data.as_object_mut() {
            obj.insert("nodes".to_string(), serde_json::Value::Array(vec![]));
        }
    }
    if rel_data.get("edges").is_none() {
        if let Some(obj) = rel_data.as_object_mut() {
            obj.insert("edges".to_string(), serde_json::Value::Array(vec![]));
        }
    }

    // 构建 node_ids set
    let mut node_ids: std::collections::HashSet<String> = {
        let empty = vec![];
        let nodes = rel_data
            .get("nodes")
            .and_then(|v| v.as_array())
            .unwrap_or(&empty);
        nodes
            .iter()
            .filter_map(|n| n.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect()
    };

    // 构建 edge_keys set
    let mut edge_keys: std::collections::HashSet<(String, String)> = {
        let empty = vec![];
        let edges = rel_data
            .get("edges")
            .and_then(|v| v.as_array())
            .unwrap_or(&empty);
        edges
            .iter()
            .filter_map(|e| {
                let source = e.get("source").and_then(|v| v.as_str())?;
                let target = e.get("target").and_then(|v| v.as_str())?;
                Some((source.to_string(), target.to_string()))
            })
            .collect()
    };

    for update in updates {
        let source = update
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let target = update
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let relation = update
            .get("relation")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // 缺的节点补建
        if !node_ids.contains(source) {
            let mut node = serde_json::Map::new();
            node.insert("id".to_string(), serde_json::Value::String(source.to_string()));
            node.insert("type".to_string(), serde_json::Value::String("character".to_string()));
            node.insert("role".to_string(), serde_json::Value::String("".to_string()));
            if let Some(nodes) = rel_data
                .get_mut("nodes")
                .and_then(|v| v.as_array_mut())
            {
                nodes.push(serde_json::Value::Object(node));
            }
            node_ids.insert(source.to_string());
        }
        if !node_ids.contains(target) {
            let mut node = serde_json::Map::new();
            node.insert("id".to_string(), serde_json::Value::String(target.to_string()));
            node.insert("type".to_string(), serde_json::Value::String("character".to_string()));
            node.insert("role".to_string(), serde_json::Value::String("".to_string()));
            if let Some(nodes) = rel_data
                .get_mut("nodes")
                .and_then(|v| v.as_array_mut())
            {
                nodes.push(serde_json::Value::Object(node));
            }
            node_ids.insert(target.to_string());
        }

        let edge_key = (source.to_string(), target.to_string());
        if edge_keys.contains(&edge_key) {
            // 更新已有边
            if let Some(edges) = rel_data
                .get_mut("edges")
                .and_then(|v| v.as_array_mut())
            {
                for e in edges.iter_mut() {
                    let s = e.get("source").and_then(|v| v.as_str()).unwrap_or("");
                    let t = e.get("target").and_then(|v| v.as_str()).unwrap_or("");
                    if s == source && t == target {
                        if let Some(obj) = e.as_object_mut() {
                            obj.insert(
                                "relation".to_string(),
                                serde_json::Value::String(relation.to_string()),
                            );
                            if let Some(change) =
                                update.get("change").and_then(|v| v.as_str())
                            {
                                obj.insert(
                                    "change".to_string(),
                                    serde_json::Value::String(change.to_string()),
                                );
                            }
                        }
                        break;
                    }
                }
            }
        } else {
            // 新增边
            let mut edge = serde_json::Map::new();
            edge.insert(
                "source".to_string(),
                serde_json::Value::String(source.to_string()),
            );
            edge.insert(
                "target".to_string(),
                serde_json::Value::String(target.to_string()),
            );
            edge.insert(
                "relation".to_string(),
                serde_json::Value::String(relation.to_string()),
            );
            let change = update
                .get("change")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            edge.insert(
                "change".to_string(),
                serde_json::Value::String(change.to_string()),
            );
            if let Some(edges) = rel_data
                .get_mut("edges")
                .and_then(|v| v.as_array_mut())
            {
                edges.push(serde_json::Value::Object(edge));
            }
            edge_keys.insert(edge_key);
        }
    }

    save_json(&rel_path, &rel_data)?;
    Ok(())
}

fn generate_memory_report(
    data: &serde_json::Value,
    chapter_id: &str,
    raw_output: &str,
    project_dir: &Path,
) -> Result<(), String> {
    let summary = data
        .get("chapter_summary")
        .and_then(|v| v.as_str())
        .unwrap_or("无");
    let events = data
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let char_updates = data
        .get("character_updates")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let fs_updates = data
        .get("foreshadowing_updates")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let rel_updates = data
        .get("relationship_updates")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let risks = data
        .get("continuity_risks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut lines = Vec::new();

    lines.push(format!("# 记忆更新报告: {}\n", chapter_id));
    lines.push(format!("## 章节摘要\n\n{}\n", summary));
    lines.push(format!("## 新增事件 ({})\n", events.len()));

    for evt in &events {
        let eid = evt.get("event_id").and_then(|v| v.as_str()).unwrap_or("?");
        let desc = evt
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        lines.push(format!("- **[{}]** {}", eid, desc));
        let etype = evt.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let chars = evt
            .get("characters")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let loc = evt
            .get("location")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        lines.push(format!(
            "  - 类型: {} | 角色: {} | 地点: {}\n",
            etype, chars, loc
        ));
    }

    lines.push(format!("## 角色更新 ({})\n", char_updates.len()));
    for cu in &char_updates {
        let name = cu.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let field = cu.get("field").and_then(|v| v.as_str()).unwrap_or("?");
        let new_val = cu
            .get("new_value")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        lines.push(format!("- **{}.{}**: {}", name, field, new_val));
        let reason = cu.get("reason").and_then(|v| v.as_str()).unwrap_or("");
        lines.push(format!("  - 原因: {}\n", reason));
    }

    lines.push(format!("## 伏笔更新 ({})\n", fs_updates.len()));
    for fu in &fs_updates {
        let fid = fu.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let fact = fu.get("action").and_then(|v| v.as_str()).unwrap_or("");
        let desc = fu
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        lines.push(format!("- **[{}]** {}: {}\n", fid, fact, desc));
    }

    lines.push(format!("## 关系更新 ({})\n", rel_updates.len()));
    for ru in &rel_updates {
        let src = ru.get("source").and_then(|v| v.as_str()).unwrap_or("?");
        let rel = ru.get("relation").and_then(|v| v.as_str()).unwrap_or("?");
        let tgt = ru.get("target").and_then(|v| v.as_str()).unwrap_or("?");
        lines.push(format!("- {} --[{}]--> {}\n", src, rel, tgt));
    }

    lines.push(format!("## 连续性风险 ({})\n", risks.len()));
    for risk in &risks {
        let severity = risk.get("severity").and_then(|v| v.as_str()).unwrap_or("low");
        let severity_cn = match severity {
            "high" => "高",
            "medium" => "中",
            _ => "低",
        };
        let risk_desc = risk.get("risk").and_then(|v| v.as_str()).unwrap_or("");
        lines.push(format!("- [{}] {}", severity_cn, risk_desc));
        let suggestion = risk
            .get("suggestion")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        lines.push(format!("  - 建议: {}\n", suggestion));
    }

    lines.push("## Raw LLM Output (前2000字)\n\n```".to_string());
    let preview: String = raw_output.chars().take(2000).collect();
    lines.push(preview);
    lines.push("```".to_string());

    let report_path = project_dir
        .join("outputs")
        .join("reports")
        .join(format!("{}_memory_update_report.md", chapter_id));
    write_text(&report_path, &lines.join("\n"))?;
    Ok(())
}

fn do_update_memory(app: &Context, chapter_id: &str) -> CommandResult {
    let config = match build_run_config(app) {
        Ok(c) => c,
        Err(e) => return CommandResult { success: false, log: e },
    };
    let settings = load_settings(app);
    let latest = context::jsonl(&config.project_dir.join("memory/chapter_summaries.jsonl")).unwrap_or_default()
        .iter().filter_map(|v| v.get("chapter_id").and_then(|x| x.as_str()).and_then(project_source::number)).max().unwrap_or(0);
    if project_source::number(chapter_id).map_or(true, |n| n < latest) {
        return CommandResult { success: false, log: "旧章更新会覆盖后续人物状态，已停止。请先处理后续记忆或从当前最新章更新。".into() };
    }


    let chapter_path = match project_source::chapter_path(&config.project_dir, chapter_id) {
        Ok(p) => p,
        Err(e) => return CommandResult { success: false, log: e },
    };

    if !chapter_path.exists() {
        return CommandResult {
            success: false,
            log: format!(
                "[错误] 未找到 {}\n[提示] 请先生成该章节。",
                chapter_path.display()
            ),
        };
    }

    let chapter_text = read_text(&chapter_path);
    if chapter_text.trim().is_empty() {
        return CommandResult {
            success: false,
            log: format!("[错误] {} 是空文件", chapter_path.display()),
        };
    }

    let mut log = String::new();
    log.push_str(&format!(
        "[信息] 正在读取 {}...\n",
        chapter_path.display()
    ));
    log.push_str(&format!(
        "[信息] 章节文本长度: {} 字符\n",
        chapter_text.chars().count()
    ));

    // 读取现有角色和伏笔
    let characters =
        load_yaml(&config.project_dir.join("memory").join("characters.yaml"));
    let foreshadowing =
        load_yaml(&config.project_dir.join("memory").join("foreshadowing.yaml"));

    // YAML Value 可直接序列化成 JSON；不能把 YAML 文本当成 JSON 再解析。
    let characters_text = serde_json::to_string_pretty(&characters).unwrap_or_default();
    let foreshadowing_text = serde_json::to_string_pretty(&foreshadowing).unwrap_or_default();
    let templates = match load_prompt_settings(&config.project_dir) {
        Ok(t) => t,
        Err(e) => return CommandResult { success: false, log: e },
    };
    if let Err(error) = prompt_tasks(&templates)[4].validate(true) {
        return CommandResult { success: false, log: error };
    }
    let prompts = match assemble_task_prompt(&templates.memory, &[
        ("章节ID".into(), chapter_id.into()), ("章节正文".into(), chapter_text),
        ("角色档案".into(), characters_text), ("伏笔档案".into(), foreshadowing_text),
    ], &settings) {
        Ok(p) => p,
        Err(e) => return CommandResult { success: false, log: e },
    };
    let messages = sys_user_messages(&prompts.system, &prompts.user);

    let top_p = settings.top_p.unwrap_or(0.85);

    log.push_str("[信息] 正在调用模型进行记忆抽取...\n");

    // 不额外设置输出上限；即使 JSON 可解析，也必须通过结构校验才能写入。
    attempt!(app.stage("调用模型"));
    let (data, raw_output) = match extract_memory_with_retry(messages, chapter_id, &mut log, |request| {
        app.stage("调用模型抽取记忆")?;
        let response = run_completion(&config, &settings, request, 0.3, top_p, None);
        app.stage("解析记忆结果")?;
        response
    }) {
        Ok(result) => result,
        Err(e) => {
            let fail_path = config
                .project_dir
                .join("outputs")
                .join("reports")
                .join(format!("{}_memory_extraction_failed.md", chapter_id));
            attempt!(app.stage("保存结果"));
            let _commit = attempt!(app.commit_access());
            let saved = atomic_write(&fail_path, format!("# 记忆抽取失败\n\n{}", e).as_bytes());
            let report_log = match saved {
                Ok(()) => format!("[信息] 失败详情及原始输出已保存到: {}", fail_path.display()),
                Err(err) => format!("[警告] 无法保存失败报告 {}: {}\n{}", fail_path.display(), err, e),
            };
            return CommandResult {
                success: false,
                log: format!(
                    "{}[错误] 记忆抽取失败，未写入记忆。\n{}",
                    log,
                    report_log
                ),
            };
        }
    };

    log.push_str("[成功] JSON 解析及结构校验成功\n");
    log.push_str("[信息] 正在应用记忆更新...\n");

    attempt!(app.stage("保存结果"));
    let _commit = attempt!(app.commit_access());
    if latest > 0 {
        if let Err(e) = context::snapshot(&config.project_dir, &format!("ch{:03}", latest)) {
            return CommandResult { success: false, log: format!("更新前状态快照保存失败：{}", e) };
        }
    }
    if let Err(error) = apply_memory_updates(&data, chapter_id, &config.project_dir) {
        return CommandResult { success: false, log: format!("{log}[错误] 记忆更新可能部分完成：{error}。请检查记忆和快照，勿直接重复更新。") };
    }
    if let Err(e) = context::snapshot(&config.project_dir, chapter_id) {
        log.push_str(&format!("[警告] 记忆已更新，但状态快照保存失败：{}\n", e));
    }
    if let Err(error) = generate_memory_report(&data, chapter_id, &raw_output, &config.project_dir) {
        return CommandResult { success: false, log: format!("{log}[错误] 记忆已写入，但报告保存失败：{error}。请勿重复更新记忆。") };
    }

    log.push_str(&format!(
        "\n[完成] {} 的记忆更新全部完成！",
        chapter_id
    ));

    CommandResult {
        success: true,
        log: log.trim().to_string(),
    }
}

// ============================================================
// 4.4 reset_chapter（移植 reset_chapter.py）
// ============================================================

fn belongs_to_chapter(record: &serde_json::Value, chapter_id: &str) -> bool {
    if let Some(cid) = record.get("chapter_id").and_then(|v| v.as_str()) {
        if cid == chapter_id {
            return true;
        }
    }
    if let Some(eid) = record.get("event_id").and_then(|v| v.as_str()) {
        if eid.starts_with(&format!("{}_", chapter_id)) {
            return true;
        }
    }
    false
}

fn filter_memory_records(chapter_id: &str, project_dir: &Path) -> Result<String, String> {
    let targets = [
        project_dir.join("memory").join("chapter_summaries.jsonl"),
        project_dir.join("memory").join("events.jsonl"),
        project_dir.join("memory").join("timeline.jsonl"),
    ];
    let mut log = String::new();
    for path in &targets {
        let records = read_jsonl(path);
        let original_count = records.len();
        let kept: Vec<serde_json::Value> = records
            .into_iter()
            .filter(|r| !belongs_to_chapter(r, chapter_id))
            .collect();
        let removed = original_count - kept.len();
        rewrite_jsonl(path, &kept)?;
        log.push_str(&format!(
            "[记忆] {}: 删除 {} 条记录\n",
            path.file_name().unwrap_or_default().to_string_lossy(),
            removed
        ));
    }
    Ok(log)
}

fn do_reset_chapter(
    app: &Context,
    chapter_id: &str,
    include_memory: bool,
) -> CommandResult {
    let config = match build_run_config(app) {
        Ok(c) => c,
        Err(e) => return CommandResult { success: false, log: e },
    };

    let mut log = String::new();

    // 删除章节目录
    let chapter_path = match project_source::chapter_path(&config.project_dir, chapter_id) {
        Ok(p) => p, Err(e) => return CommandResult { success: false, log: e },
    };
    if chapter_path.exists() {
        if let Err(e) = project_source::backup(&config.project_dir, &chapter_path).and_then(|_| fs::remove_file(&chapter_path).map_err(|e| e.to_string())) {
            return CommandResult { success: false, log: e };
        }
        log.push_str(&format!("[删除并备份] {}\n", chapter_path.display()));
    }

    // 删除报告文件
    let reports_dir = config.project_dir.join("outputs").join("reports");
    let mut deleted_reports = 0u32;
    match fs::read_dir(&reports_dir) {
        Ok(entries) => for entry in entries {
            let entry = attempt!(entry.map_err(|e| format!("正文已处理，但报告目录读取失败：{e}")));
            let path = entry.path();
            if path.is_file() && path.file_name().and_then(|n| n.to_str()).map_or(false, |name| name.starts_with(&format!("{}_", chapter_id))) {
                attempt!(fs::remove_file(&path).map_err(|e| format!("重置可能部分完成，报告 {} 删除失败：{e}", path.display())));
                deleted_reports += 1;
                log.push_str(&format!("[删除] {}\n", path.display()));
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(e) => return CommandResult { success: false, log: format!("正文已处理，但报告目录读取失败：{e}") },
    }
    log.push_str(&format!(
        "[完成] 已删除 {} 个报告文件\n",
        deleted_reports
    ));

    // 处理记忆过滤
    if include_memory {
        let metadata_path = config.project_dir.join("memory/state_metadata.json");
        let mut metadata: serde_json::Value = fs::read_to_string(&metadata_path).ok()
            .and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| serde_json::json!({}));
        let n = project_source::number(chapter_id).unwrap_or(0);
        let prior = metadata.get("invalid_from").and_then(|v| v.as_u64()).unwrap_or(n as u64);
        metadata["invalid_from"] = serde_json::json!(prior.min(n as u64));
        if let Err(e) = project_manager::atomic_json(&metadata_path, &metadata) {
            return CommandResult { success: false, log: format!("状态失效标记保存失败：{}", e) };
        }
        log.push_str(&attempt!(filter_memory_records(chapter_id, &config.project_dir)));
    } else {
        log.push_str("[信息] 未删除长期记忆。需要时可勾选「同时清除记忆」。\n");
    }

    CommandResult {
        success: true,
        log: log.trim().to_string(),
    }
}

// ============================================================
// 简单 Tauri 命令（文件读写 / 设置 / 列表）
// ============================================================

pub fn get_pipeline_root(app: Context) -> Result<String, String> {
    let config = build_run_config(&app)?;
    Ok(config.project_dir.to_string_lossy().to_string())
}

pub fn list_chapters(app: Context) -> Result<Vec<String>, String> {
    let _access = app.read_access()?;
    let config = build_run_config(&app)?;
    Ok(project_source::chapters(&config.project_dir)?.keys().map(|n| format!("ch{:03}", n)).collect())
}

pub fn read_chapter(app: Context, chapter_id: String) -> Result<String, String> {
    let chapter_id = canonical_chapter_id(&chapter_id)?;
    let _access = app.read_access()?;
    let config = build_run_config(&app)?;
    let path = project_source::chapter_path(&config.project_dir, &chapter_id)?;
    Ok(read_text(&path))
}

pub fn save_chapter(app: Context, chapter_id: String, content: String) -> Result<(), String> {
    let chapter_id = canonical_chapter_id(&chapter_id)?;
    let _access = app.write_access()?;
    let config = build_run_config(&app)?;
    let path = project_source::chapter_path(&config.project_dir, &chapter_id)?;
    project_source::write(&config.project_dir, &path, &content)
}

pub fn read_report(
    app: Context,
    chapter_id: String,
    kind: String,
) -> Result<String, String> {
    let chapter_id = canonical_chapter_id(&chapter_id)?;
    let _access = app.read_access()?;
    let config = build_run_config(&app)?;
    let filename = match kind.as_str() {
        "generation" => format!("{}_chapter_generation_report.md", chapter_id),
        "consistency" => format!("{}_consistency_report.md", chapter_id),
        "memory" => format!("{}_memory_update_report.md", chapter_id),
        _ => return Err(format!("未知报告类型: {}", kind)),
    };
    let path = config
        .project_dir
        .join("outputs")
        .join("reports")
        .join(filename);
    Ok(read_text(&path))
}

pub fn read_memory_file(app: Context, rel: String) -> Result<String, String> {
    let _access = app.read_access()?;
    let rel = normalize_rel(&rel);
    if !EDITABLE_FILES.contains(&rel.as_str()) {
        return Err(format!("不允许读取该文件: {}", rel));
    }
    let config = build_run_config(&app)?;
    Ok(read_text(&config.project_dir.join(&rel)))
}

pub fn save_memory_file(
    app: Context,
    rel: String,
    content: String,
) -> Result<(), String> {
    let _access = app.write_access()?;
    let rel = normalize_rel(&rel);
    if !EDITABLE_FILES.contains(&rel.as_str()) {
        return Err(format!("不允许写入该文件: {}", rel));
    }
    let config = build_run_config(&app)?;
    let path = config.project_dir.join(&rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(&path, content.as_bytes())
}

pub fn get_settings(app: Context) -> Result<Settings, String> {
    let config = build_run_config(&app)?;
    let mut settings = load_settings(&app);
    // 确保 project_dir 回填（展示当前实际使用的目录）
    if settings.project_dir.trim().is_empty() {
        settings.project_dir = config.project_dir.to_string_lossy().to_string();
    }
    Ok(settings)
}

pub fn save_settings(app: Context, settings: Settings) -> Result<(), String> {
    let _global = app.global_access()?;
    let current = Context::new(app.data_dir.clone(), None)?;
    let active = resolve_project_dir(&current);
    if !project_manager::same_path(Path::new(&settings.project_dir), &active) {
        return Err("项目目录已变更，请通过最近项目入口切换项目并重新加载设置".into());
    }
    project_manager::atomic_json(&settings_path(&app), &settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;

    fn temp_project(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut dir = std::env::temp_dir();
        dir.push(format!("novelpipe_test_{}_{}_{}", tag, std::process::id(), nanos));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("memory")).unwrap();
        dir
    }

    #[test]
    fn empty_sections_and_unused_form_rules_are_omitted() {
        assert!(build_form_block("", &[]).is_empty());
        assert!(!build_form_block("limited:陆青霜", &[]).contains("王二"));
        assert_eq!(normalize_idea("## 【事件骨架】\n\n1. 取书。"), "1. 取书。");
        let output = render_generation_template("【空区块】\n{{角色档案}}\n\n【事件骨架】\n{{事件骨架}}", &[
            ("角色档案".into(), String::new()), ("事件骨架".into(), "1. 取书。".into()),
        ]).unwrap();
        assert!(!output.contains("空区块")); assert!(output.contains("1. 取书。"));
    }

    #[test]
    fn over_budget_requires_shortening_and_shared_assembly_preserves_confirmed_prompt() {
        let dir = temp_project("budget");
        let settings = Settings::default();
        let opts = context::Options { input_budget: 500, ..context::Options::default() };
        let (preview, _) = prepare_generation(&dir, &settings, "ch001", &"长".repeat(800), 4000, false, "", "", &opts).unwrap();
        assert!(preview.stats.over_budget);
        assert!(choose_generation_prompt(preview.clone(), None).is_err());
        let shorter = PromptPair { system: "中文小说助手".into(), user: "只写取书场景".into() };
        let (fresh, _) = prepare_generation(&dir, &settings, "ch001", &"长".repeat(800), 4000, false, "", "", &opts).unwrap();
        assert_eq!(fresh.source, preview.source);
        assert_eq!(choose_generation_prompt(fresh, Some(PromptOverride { prompts: shorter.clone(), source: preview.source })).unwrap(), shorter);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_prompt_settings_keep_generation_and_default_other_tasks() {
        let old = r#"{"system":"原有系统提示词","user_template":"{{事件骨架}}"}"#;
        let settings: PromptSettings = serde_json::from_str(old).unwrap();
        assert_eq!(settings.system, "原有系统提示词");
        assert_eq!(settings.consistency, default_consistency_prompt());
        assert_eq!(settings.memory, default_memory_prompt());
        assert!(validate_prompt_settings(&settings).is_ok());
        let roundtrip: PromptSettings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(roundtrip, settings);
    }

    #[test]
    fn task_templates_render_json_and_literal_chapter_without_recursive_replacement() {
        let settings = Settings { disable_thinking: Some(false), ..Settings::default() };
        let chapter = "陆青霜发现写着 {{章节ID}} 的纸条。";
        let values = vec![("章节ID".into(), "ch005".into()), ("章节正文".into(), chapter.into()),
            ("角色档案".into(), "人物资料".into()), ("伏笔档案".into(), "伏笔资料".into())];
        let memory = assemble_task_prompt(&default_memory_prompt(), &values, &settings).unwrap();
        assert!(memory.user.contains(chapter));
        assert!(memory.user.contains("\"chapter_id\": \"ch005\""));
        assert!(memory.user.contains("人物资料"));
        assert!(!memory.user.contains("{chapter_text}"));
        let mut check = default_consistency_prompt();
        check.system = "自定义检查系统".into();
        check.user_template = "只检查正文：{{章节正文}}".into();
        let rendered = assemble_task_prompt(&check, &values, &settings).unwrap();
        assert_eq!(rendered.system, "自定义检查系统");
        assert_eq!(rendered.user, format!("只检查正文：{}", chapter));
        let yaml: serde_yaml::Value = serde_yaml::from_str("characters:\n  - name: 陆青霜\n").unwrap();
        let json = serde_json::to_value(&yaml).unwrap();
        assert_eq!(json["characters"][0]["name"], "陆青霜");
    }

    #[test]
    fn memory_contract_rejects_wrong_chapter_and_missing_arrays() {
        let mut data = json!({"chapter_id":"ch005", "chapter_summary":"获得伪装能力", "events":[],
            "character_updates":[], "foreshadowing_updates":[], "relationship_updates":[], "continuity_risks":[]});
        assert!(validate_memory_extraction(&data, "ch005").is_ok());
        assert!(validate_memory_extraction(&data, "ch006").is_err());
        data["events"] = json!("错误类型");
        assert!(validate_memory_extraction(&data, "ch005").is_err());
    }

    #[test]
    fn memory_extraction_retries_invalid_outputs_with_original_context() {
        let valid = json!({"chapter_id":"ch005", "chapter_summary":"主角查明真相。", "events":[],
            "character_updates":[], "foreshadowing_updates":[], "relationship_updates":[], "continuity_risks":[]});
        let mut empty = valid.clone();
        empty["chapter_summary"] = json!("");
        let mut wrong_chapter = valid.clone();
        wrong_chapter["chapter_id"] = json!("ch004");
        let mut wrong_type = valid.clone();
        wrong_type["events"] = json!({});
        for invalid in [empty.to_string(), wrong_chapter.to_string(), wrong_type.to_string(), "不是 JSON".into()] {
            let mut calls = 0;
            let mut log = String::new();
            let result = extract_memory_with_retry(sys_user_messages("抽取记忆", "完整章节正文与人物资料"), "ch005", &mut log, |request| {
                calls += 1;
                assert!(request.iter().any(|m| m.content == "完整章节正文与人物资料"));
                assert!(request[0].content.contains("chapter_summary 必须是非空字符串"));
                if calls == 1 { return Ok(invalid.clone()); }
                assert_eq!(request[request.len() - 2].content, invalid);
                assert!(request.last().unwrap().content.contains("上次输出校验失败"));
                Ok(valid.to_string())
            }).unwrap();
            assert_eq!(calls, 2);
            assert_eq!(result.0, valid);
            assert!(log.contains("自动重试"));
        }
    }

    #[test]
    fn memory_extraction_accepts_valid_first_response_and_bounds_failures() {
        let raw = json!({"chapter_id":"ch005", "chapter_summary":"主角查明真相。", "events":[],
            "character_updates":[], "foreshadowing_updates":[], "relationship_updates":[], "continuity_risks":[]}).to_string();
        let mut calls = 0;
        let mut log = String::new();
        extract_memory_with_retry(vec![], "ch005", &mut log, |_| {
            calls += 1;
            Ok(raw.clone())
        }).unwrap();
        assert_eq!(calls, 1);
        assert!(log.is_empty());

        calls = 0;
        let failure = extract_memory_with_retry(vec![], "ch005", &mut log, |_| {
            calls += 1;
            Ok("{\"chapter_id\":\"ch005\",\"chapter_summary\":\"\"}".into())
        }).unwrap_err();
        assert_eq!(calls, 2);
        assert!(failure.contains("第 1 次抽取"));
        assert!(failure.contains("第 2 次抽取"));
        assert!(failure.contains("chapter_summary"));

        calls = 0;
        let failure = extract_memory_with_retry(vec![], "ch005", &mut log, |_| {
            calls += 1;
            Err("连接失败".into())
        }).unwrap_err();
        assert_eq!(calls, 1);
        assert!(failure.contains("连接失败"));
    }

    #[test]
    fn template_validation_and_literal_author_input() {
        assert!(validate_prompt_settings(&PromptSettings::default()).is_ok());
        let mut template = PromptSettings::default();
        template.user_template = "{{不支持的变量}} {{事件骨架}}".into();
        assert!(validate_prompt_settings(&template).is_err());
        template.user_template = "{{事件骨架".into();
        assert!(validate_prompt_settings(&template).is_err());
        template.user_template = "不含事件骨架".into();
        assert!(validate_prompt_settings(&template).is_err());
        let rendered = render_prompt_template("{{事件骨架}} / {{目标字数}}", &[
            ("事件骨架".into(), "主角看到 {{目标字数}} 字样".into()),
            ("目标字数".into(), "4000".into()),
        ]).unwrap();
        assert_eq!(rendered, "主角看到 {{目标字数}} 字样 / 4000");
    }

    #[test]
    fn literal_titles_and_fixed_text_survive_empty_materials() {
        let values = vec![("角色档案".into(), String::new()), ("事件骨架".into(), "进入山谷".into())];
        for template in [
            "【人物状态】\n固定角色说明\n{{角色档案}}\n【本章】\n{{事件骨架}}",
            "前置固定说明\n{{角色档案}}\n【本章】\n{{事件骨架}}",
            "【人物状态】固定角色说明\n{{角色档案}}\n【本章】\n{{事件骨架}}",
        ] {
            let rendered = render_generation_template(template, &values).unwrap();
            assert!(rendered.contains("固定"), "{rendered}");
            assert!(rendered.contains("进入山谷"));
        }
        let literal = "【人物状态】\n【本章】\n{{事件骨架}}";
        assert!(render_generation_template(literal, &values).unwrap().contains("【人物状态】"));
        let empty = "【人物状态】\n{{角色档案}}\n【本章】\n{{事件骨架}}";
        assert!(!render_generation_template(empty, &values).unwrap().contains("【人物状态】"));
        let error = validate_prompt_variables("【人物状态】\n普通标题\n{{人物状态}}\n{{事件骨架}}", GENERATION_VARIABLES, None).unwrap_err();
        assert!(error.contains("第 3 行"));
        assert!(error.contains("{{角色档案}}"));
    }

    #[test]
    fn manual_generation_sends_exact_messages_without_templates_or_memory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("book");
        fs::create_dir_all(&root).unwrap();
        let mut ctx = Context::new(dir.path().join("data"), Some(root.clone())).unwrap();
        fs::write(root.join(".prompt-settings.json"), "invalid template settings").unwrap();
        fs::create_dir_all(root.join("memory")).unwrap();
        fs::write(root.join("memory/characters.yaml"), "invalid: [").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        ctx.settings.llm_base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        ctx.settings.llm_model = "manual-test".into();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0u8; 4096];
            let (split, length) = loop {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..index]).to_lowercase();
                    let length = header.lines().find_map(|line| line.strip_prefix("content-length:")).unwrap().trim().parse::<usize>().unwrap();
                    break (index + 4, length);
                }
            };
            while bytes.len() < split + length {
                let n = stream.read(&mut buf).unwrap(); assert!(n > 0); bytes.extend_from_slice(&buf[..n]);
            }
            let request: serde_json::Value = serde_json::from_slice(&bytes[split..split + length]).unwrap();
            let body = json!({"choices":[{"message":{"content":"手动生成的正文。"}}]}).to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            request
        });
        let prompts = PromptPair { system: "自定系统\n".into(), user: "  【人物状态】\n{{人物状态}} {{未知变量}}\n请直接写正文。\n".into() };
        let result = generate_manual_chapter(ctx.clone(), "ch001".into(), prompts.clone(), false, 100000).unwrap();
        assert!(result.success, "{}", result.log);
        let request = server.join().unwrap();
        assert_eq!(request["messages"][0]["content"], prompts.system);
        assert_eq!(request["messages"][1]["content"], prompts.user);
        let chapter = project_source::chapter_path(&root, "ch001").unwrap();
        assert_eq!(fs::read_to_string(&chapter).unwrap(), "手动生成的正文。");
        let blocked = generate_manual_chapter(ctx, "ch001".into(), prompts, false, 100000).unwrap();
        assert!(!blocked.success);
        assert_eq!(fs::read_to_string(chapter).unwrap(), "手动生成的正文。");
        assert!(preview_manual_prompt(PromptPair { system: String::new(), user: "正文要求".into() }, 500).is_ok());
        assert!(preview_manual_prompt(PromptPair { system: "系统".into(), user: "  ".into() }, 500).is_err());
        assert!(preview_manual_prompt(PromptPair { system: String::new(), user: "长".repeat(501) }, 500).is_err());
    }

    #[test]
    fn removed_variable_names_are_rejected_without_migration() {
        for name in ["人物状态", "前情", "相关设定与伏笔", "世界观规则", "近期事件", "伏笔"] {
            let mut template = PromptSettings::default();
            template.user_template = format!("{{{{{name}}}}} {{{{事件骨架}}}}");
            let error = validate_prompt_settings(&template).unwrap_err();
            assert!(error.contains(name));
        }
    }

    #[test]
    fn each_generation_variable_reads_only_its_own_file() {
        let dir = temp_project("one_file_per_variable");
        fs::create_dir_all(dir.join("outlines")).unwrap();
        fs::write(dir.join("memory/characters.yaml"), "characters:\n- name: CHAR_MARK\n  role: 主角\n").unwrap();
        fs::write(dir.join("memory/story_bible.yaml"), "world_rules: [BIBLE_MARK]\n").unwrap();
        fs::write(dir.join("memory/foreshadowing.yaml"), "active:\n- id: clue\n  description: CLUE_MARK\n  introduced_in: ch001\n").unwrap();
        fs::write(dir.join("memory/chapter_summaries.jsonl"), r#"{"chapter_id":"ch001","chapter_summary":"SUMMARY_MARK"}"#).unwrap();
        fs::write(dir.join("memory/events.jsonl"), r#"{"chapter_id":"ch001","event_id":"event","description":"EVENT_MARK"}"#).unwrap();
        fs::write(dir.join("memory/timeline.jsonl"), r#"{"chapter_id":"ch001","event_id":"event","description":"TIME_MARK","time":"yesterday"}"#).unwrap();
        fs::write(dir.join("memory/style_bank.jsonl"), r#"{"source_chapter":"ch001","text":"STYLE_MARK"}"#).unwrap();
        fs::write(dir.join("outlines/book_outline.yaml"), "plan: OUTLINE_MARK\n").unwrap();
        let cases = [("角色档案", "CHAR_MARK", "character"), ("故事圣经", "BIBLE_MARK", "rules"),
            ("伏笔档案", "CLUE_MARK", "foreshadowing"), ("章节摘要", "SUMMARY_MARK", "summary"),
            ("历史事件", "EVENT_MARK", "event"), ("时间线", "TIME_MARK", "timeline"),
            ("文风参考", "STYLE_MARK", "style"), ("全书大纲", "OUTLINE_MARK", "outline")];
        let opts = context::Options { events: vec!["event".into()], include_outline: true, outline_full: true, ..Default::default() };
        for (name, marker, kind) in cases {
            let mut template = PromptSettings::default();
            template.user_template = format!("{{{{{name}}}}}\n{{{{事件骨架}}}}\n{{{{叙事视角}}}}\n{{{{叙事要求}}}}");
            fs::write(dir.join(".prompt-settings.json"), serde_json::to_vec(&template).unwrap()).unwrap();
            let preview = prepare_generation(&dir, &Settings::default(), "ch003", "IDEA_MARK", 1000, true, "objective", "DIRECTIVE_MARK", &opts).unwrap().0;
            assert!(preview.prompts.user.contains(marker), "{name}: {}", preview.prompts.user);
            for (_, other, _) in cases { if other != marker { assert!(!preview.prompts.user.contains(other), "{name} leaked {other}"); } }
            assert!(preview.prompts.user.contains("客观旁观视角"));
            assert!(preview.prompts.user.contains("DIRECTIVE_MARK"));
            for source in preview.stats.sources.iter().filter(|s| s.included) { assert_eq!(source.kind, kind); }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preview_and_override_preserve_exact_messages() {
        let dir = temp_project("prompt_exact");
        let settings = Settings::default();
        let mem = build_memory_sections(&dir, false);
        let preview = assemble_generation_prompt(&dir, &settings, "ch001", "1. 进入山谷。", 4000, &mem, "", None).unwrap();
        assert!(preview.prompts.user.contains("【事件骨架】\n1. 进入山谷。"));
        assert!(preview.prompts.user.ends_with("/no_think"));
        let automatic = choose_generation_prompt(preview.clone(), None).unwrap();
        assert_eq!(automatic, preview.prompts);
        // 用户可删除 /no_think；生成路径不可重新追加或重新组装。
        let edited = PromptPair { system: "自定义系统要求".into(), user: "手动确认的章节提示词".into() };
        let chosen = choose_generation_prompt(preview.clone(), Some(PromptOverride {
            prompts: edited.clone(), source: preview.source.clone(),
        })).unwrap();
        let messages = sys_user_messages(&chosen.system, &chosen.user);
        assert_eq!(messages[0].content, edited.system);
        assert_eq!(messages[1].content, edited.user);
        let changed = assemble_generation_prompt(&dir, &settings, "ch002", "1. 进入山谷。", 4000, &mem, "", None).unwrap();
        assert!(choose_generation_prompt(changed, Some(PromptOverride { prompts: edited, source: preview.source })).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prompt_sources_detect_memory_and_template_changes() {
        let dir = temp_project("prompt_sources");
        let settings = Settings::default();
        let preview = |dir: &Path| {
            let mem = build_memory_sections(dir, true);
            assemble_generation_prompt(dir, &settings, "ch001", "进入山谷", 4000, &mem, "", None).unwrap()
        };
        let before = preview(&dir);
        fs::write(dir.join("memory/characters.yaml"), "characters:\n  - name: 主角\n    role: 主角\n    current_status: 疲惫但警惕\n").unwrap();
        let with_memory = preview(&dir);
        assert_ne!(before.source, with_memory.source);
        assert!(with_memory.prompts.user.contains("疲惫但警惕"));
        let custom = PromptSettings { system: "项目自己的系统提示词".into(), user_template: "【事件骨架】\n{{事件骨架}}".into(), ..PromptSettings::default() };
        fs::write(dir.join(".prompt-settings.json"), serde_json::to_string(&custom).unwrap()).unwrap();
        assert_eq!(load_prompt_settings(&dir).unwrap(), custom);
        let with_template = preview(&dir);
        assert_ne!(with_memory.source, with_template.source);
        assert_eq!(with_template.prompts.system, custom.system);
        assert_eq!(with_template.prompts.user, "【事件骨架】\n进入山谷\n\n/no_think");
        fs::write(dir.join(".prompt-settings.json"), "broken json").unwrap();
        assert!(load_prompt_settings(&dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn generation_presets_migrate_and_keep_independent_templates() {
        let old: PromptSettings = serde_json::from_str(r#"{"system":"原有系统","user_template":"{{事件骨架}}"}"#).unwrap();
        assert_eq!(old.generation_preset, GenerationPreset::General);
        assert!(old.general_material_guidance);
        assert!(!old.compact_material_guidance);
        assert_eq!(old.system, "原有系统");
        assert_eq!(old.compact_generation, default_compact_generation());
        assert_eq!(old.benchmark_generation, TaskPrompt::default());
        assert!(!old.benchmark_material_guidance);
        assert!(validate_prompt_settings(&old).is_ok());
        let mut next = old.clone();
        next.generation_preset = GenerationPreset::Compact;
        next.compact_generation.system = "独立的精简系统".into();
        next.compact_generation.user_template = "精简：{{事件骨架}}".into();
        let restored: PromptSettings = serde_json::from_str(&serde_json::to_string(&next).unwrap()).unwrap();
        assert_eq!(next, restored);
        assert_eq!(restored.system, old.system);
        assert_eq!(restored.consistency, old.consistency);
        assert_eq!(restored.memory, old.memory);
        next.compact_generation.user_template = "缺少骨架".into();
        assert!(validate_prompt_settings(&next).is_err());
    }

    #[test]
    fn compact_preset_uses_independent_sources_and_project_default_can_be_overridden() {
        let dir = temp_project("compact_generation");
        fs::write(dir.join("memory/characters.yaml"), "characters:\n  - name: 林川\n    role: 主角\n    identity: 学徒\n    current_status: 疲惫但警惕\n    personality: 性格字段样例\n    speaking_style: 言语字段样例\n    knows: [导师在山谷]\n    secrets: [藏着钥匙]\n    relationships: [导师是师父]\n    constraints: [尚未掌握御剑]\n").unwrap();
        fs::write(dir.join("memory/story_bible.yaml"), "world_rules: [飞行消耗灵力]\n").unwrap();
        fs::write(dir.join("memory/style_bank.jsonl"), r#"{"id":"style","source_chapter":"ch001","text":"山风掠过衣角。"}"#).unwrap();
        fs::write(dir.join("memory/foreshadowing.yaml"), "active:\n  - id: clue\n    introduced_in: ch001\n    description: 钥匙上刻着残纹\n    planned_resolution: 未来天门真相\n").unwrap();
        fs::write(dir.join("memory/chapter_summaries.jsonl"), r#"{"chapter_id":"ch002","chapter_summary":"林川跟随导师赶路"}"#).unwrap();
        fs::write(dir.join("memory/events.jsonl"), r#"{"chapter_id":"ch001","event_id":"key","description":"林川取得铜钥匙"}"#).unwrap();
        let mut template = PromptSettings::default();
        template.generation_preset = GenerationPreset::Compact;
        fs::write(dir.join(".prompt-settings.json"), serde_json::to_string(&template).unwrap()).unwrap();
        let settings = Settings { disable_thinking: Some(false), ..Settings::default() };
        let opts = context::Options { events: vec!["key".into()], include_outline: true,
            outline_excerpt: "仅展开山谷训练".into(), ..context::Options::default() };
        let preview = |opts: &context::Options| prepare_generation(&dir, &settings, "ch003", "1. 林川进入山谷。", 1000, true, "", "", opts).unwrap().0;
        let compact = preview(&opts);
        let headings: Vec<_> = compact.prompts.user.lines().filter(|line| line.starts_with('【') && line.ends_with('】')).collect();
        assert_eq!(headings, ["【角色档案】", "【章节摘要】", "【历史事件】", "【事件骨架】", "【写作要求】", "【全书大纲】", "【伏笔档案】"]);
        for fact in ["学徒", "疲惫但警惕", "导师在山谷", "藏着钥匙", "导师是师父", "尚未掌握御剑", "飞行消耗灵力", "山风掠过衣角。", "钥匙上刻着残纹", "仅展开山谷训练"] {
            assert!(compact.prompts.user.contains(fact), "missing {fact}");
        }
        assert!(!compact.prompts.user.contains("未来天门真相"));
        let requirements = compact.prompts.user.split("【写作要求】").nth(1).unwrap();
        for fact in ["飞行消耗灵力", "山风掠过衣角。", "钥匙上刻着残纹", "仅展开山谷训练"] {
            assert!(requirements.contains(fact));
        }
        assert!(!compact.prompts.user.contains("性格字段样例"));
        assert!(!compact.prompts.user.contains("言语字段样例"));
        assert!(!compact.prompts.user.contains("直接前情与参考章"));
        assert!(compact.prompts.user.split("【章节摘要】").nth(1).unwrap().split("【历史事件】").next().unwrap().contains("林川跟随导师赶路"));
        assert!(compact.prompts.user.split("【历史事件】").nth(1).unwrap().split("【事件骨架】").next().unwrap().contains("林川取得铜钥匙"));
        let general_opts = context::Options { generation_preset: Some(GenerationPreset::General), ..opts.clone() };
        let general = preview(&general_opts);
        assert!(general.prompts.user.contains("性格字段样例"));
        assert!(general.prompts.user.contains("言语字段样例"));
        assert!(general.prompts.user.starts_with("【资料使用原则】"));
        assert!(general.prompts.user.contains("【故事圣经】"));
        assert_ne!(compact.source, general.source);
        assert!(choose_generation_prompt(general, Some(PromptOverride { prompts: compact.prompts.clone(), source: compact.source.clone() })).is_err());
        assert_eq!(choose_generation_prompt(compact.clone(), None).unwrap(), compact.prompts);
        assert_eq!(load_prompt_settings(&dir).unwrap().generation_preset, GenerationPreset::Compact);
        // The additional guidance belongs to each preset; it is no longer unconditional.
        template.compact_material_guidance = true;
        template.general_material_guidance = false;
        fs::write(dir.join(".prompt-settings.json"), serde_json::to_string(&template).unwrap()).unwrap();
        assert!(preview(&opts).prompts.user.starts_with("【资料使用原则】"));
        assert!(!preview(&general_opts).prompts.user.contains("【资料使用原则】"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn explicit_reference_preserves_prose_and_does_not_replace_summaries() {
        let dir = temp_project("compact_history");
        fs::create_dir_all(dir.join("正文")).unwrap();
        let prose = "第一段。\n\n同样的话。\n同样的话。\n- [ch999] 正文内的列表。";
        fs::write(dir.join("正文/第1章.md"), prose).unwrap();
        fs::write(dir.join("memory/chapter_summaries.jsonl"), r#"{"chapter_id":"ch002","chapter_summary":"林川走入山谷"}"#).unwrap();
        fs::write(dir.join("memory/events.jsonl"), r#"{"chapter_id":"ch002","event_id":"same","description":"林川走入山谷"}
{"chapter_id":"ch003","event_id":"later","description":"林川走入山谷"}"#).unwrap();
        let mut template = PromptSettings::default();
        template.compact_generation.user_template = "【参考正文】\n{{参考正文:ch001}}\n【章节摘要】\n{{章节摘要}}\n【历史事件】\n{{历史事件}}\n【事件骨架】\n{{事件骨架}}".into();
        fs::write(dir.join(".prompt-settings.json"), serde_json::to_string(&template).unwrap()).unwrap();
        let options = context::Options { generation_preset: Some(GenerationPreset::Compact),
            chapters: vec!["ch001".into()], events: vec!["same".into(), "later".into()], ..context::Options::default() };
        let (preview, _) = prepare_generation(&dir, &Settings::default(), "ch004", "林川离开", 1000, true, "", "", &options).unwrap();
        assert!(preview.prompts.user.contains(prose));
        assert_eq!(preview.prompts.user.matches("林川走入山谷").count(), 3);
        assert!(preview.prompts.user.find(prose).unwrap() < preview.prompts.user.find("林川走入山谷").unwrap());
        assert!(preview.stats.sources.iter().any(|s| s.kind == "chapter" && s.included && s.id == "ch001"));
        // Selecting a chapter with no summary must never substitute its prose.
        template.compact_generation.user_template = "{{章节摘要}}\n{{事件骨架}}".into();
        fs::write(dir.join(".prompt-settings.json"), serde_json::to_vec(&template).unwrap()).unwrap();
        let without_prose = prepare_generation(&dir, &Settings::default(), "ch004", "离开", 1000, true, "", "", &options).unwrap().0;
        assert!(!without_prose.prompts.user.contains(prose));
        for id in ["ch004", "ch005", "ch009"] {
            template.compact_generation.user_template = format!("{{{{参考正文:{id}}}}}\n{{{{事件骨架}}}}");
            fs::write(dir.join(".prompt-settings.json"), serde_json::to_vec(&template).unwrap()).unwrap();
            assert!(prepare_generation(&dir, &Settings::default(), "ch004", "离开", 1000, true, "", "", &options).is_err());
        }
        template.compact_generation.user_template = "{{参考正文:ch002}}\n{{事件骨架}}".into();
        fs::write(dir.join(".prompt-settings.json"), serde_json::to_vec(&template).unwrap()).unwrap();
        assert!(prepare_generation(&dir, &Settings::default(), "ch004", "离开", 1000, true, "", "", &options).err().unwrap().contains("不存在"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clean_output_strips_prefix_and_fences() {
        assert_eq!(clean_output("以下是正文：\n正文开始"), "正文开始");
        assert_eq!(clean_output("```text\n正文内容\n```"), "正文内容");
        assert_eq!(clean_output("纯正文"), "纯正文");
    }

    #[test]
    fn parse_extraction_json_handles_messy_output() {
        // 带 ```json 围栏
        let v = parse_extraction_json("```json\n{\"a\":1}\n```").unwrap();
        assert_eq!(v["a"], 1);
        // 前缀文字 + 尾部多余内容
        let v = parse_extraction_json("好的，结果如下：{\"b\":2} 以上。").unwrap();
        assert_eq!(v["b"], 2);
        // 非法 JSON 返回错误
        assert!(parse_extraction_json("这不是 JSON").is_err());
    }

    #[test]
    fn generation_report_idea_roundtrip() {
        // 写生成报告后，一致性检查必须能从中解析回 idea（跨文件依赖）
        let dir = temp_project("report");
        let cfg = RunConfig {
            project_dir: dir.clone(),
            llm_base_url: "x".into(),
            llm_api_key: "x".into(),
            model_name: "test-model".into(),
            max_output_tokens: Some(100),
            provider: "api".into(),
        };
        let idea = "主角回到故乡，发现父亲留下的一封信，决定调查旧事。";
        write_generation_report(
            &dir, "ch001", idea, 4000, true, 123, "正文内容", "raw resp", "user prompt", &[], &cfg,
        ).unwrap();
        assert_eq!(load_chapter_idea(&dir, "ch001"), idea);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_memory_updates_roundtrip_formats() {
        let dir = temp_project("mem");
        write_text(
            &dir.join("memory").join("characters.yaml"),
            "characters:\n  - name: 林川\n    current_status: 在故乡\n    knows: []\n",
        ).unwrap();

        let data = json!({
            "chapter_id": "ch001",
            "chapter_summary": "主角回到故乡。",
            "events": [{
                "event_id": "ch001_evt001",
                "type": "discovery",
                "characters": ["林川"],
                "description": "发现父亲的信",
                "location": "老宅",
                "time_order": "傍晚",
                "importance": 8
            }],
            "character_updates": [
                {"name": "林川", "field": "current_status", "new_value": "决定调查旧事"},
                {"name": "林川", "field": "knows", "new_value": "父亲留下了一封信"}
            ],
            "foreshadowing_updates": [
                {"id": "fs001", "action": "add", "description": "信里提到的名字",
                 "planned_resolution": "后续揭示", "related_characters": ["林川"]}
            ],
            "relationship_updates": [
                {"source": "林川", "target": "父亲", "relation": "父子", "change": "通过信件重新连接"}
            ]
        });
        apply_memory_updates(&data, "ch001", &dir).unwrap();

        // 章节摘要
        let summaries = read_jsonl(&dir.join("memory").join("chapter_summaries.jsonl"));
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0]["chapter_id"], "ch001");
        assert_eq!(summaries[0]["chapter_summary"], "主角回到故乡。");

        // 事件：补上了 chapter_id
        let events = read_jsonl(&dir.join("memory").join("events.jsonl"));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["chapter_id"], "ch001");
        assert_eq!(events[0]["description"], "发现父亲的信");

        // 时间线：time 取自 time_order
        let timeline = read_jsonl(&dir.join("memory").join("timeline.jsonl"));
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0]["event_id"], "ch001_evt001");
        assert_eq!(timeline[0]["time"], "傍晚");

        // 角色：current_status 覆盖；knows 追加
        let chars = load_yaml(&dir.join("memory").join("characters.yaml"));
        let c = &chars.get("characters").unwrap().as_sequence().unwrap()[0];
        assert_eq!(c.get("current_status").unwrap().as_str(), Some("决定调查旧事"));
        let knows = c.get("knows").unwrap().as_sequence().unwrap();
        assert!(knows.iter().any(|v| v.as_str() == Some("父亲留下了一封信")));

        // 伏笔：新增到 active
        let fsd = load_yaml(&dir.join("memory").join("foreshadowing.yaml"));
        let active = fsd.get("active").unwrap().as_sequence().unwrap();
        assert!(active
            .iter()
            .any(|f| f.get("id").and_then(|v| v.as_str()) == Some("fs001")));

        // 关系图：新增边 林川 -> 父亲
        let rel = load_json(&dir.join("memory").join("relationships.json"));
        let edges = rel.get("edges").unwrap().as_array().unwrap();
        assert!(edges
            .iter()
            .any(|e| e["source"] == "林川" && e["target"] == "父亲"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn repair_recovers_truncated_extraction_json() {
        // 模拟模型在 events 第二个元素中途被 token 上限截断
        let truncated = "{\n  \"chapter_summary\": \"测试摘要\",\n  \"events\": [\n    {\"event_id\": \"ch001_evt001\", \"description\": \"完整事件\"},\n    {\"event_id\": \"ch001_evt002\",\n     \"chapte";
        // 严格解析必然失败
        assert!(serde_json::from_str::<serde_json::Value>(truncated).is_err());
        // 抢救后应能解析，且保住摘要与已完整的首个事件
        let v = parse_extraction_json(truncated).expect("应能抢救截断的 JSON");
        assert_eq!(v["chapter_summary"], "测试摘要");
        let events = v["events"].as_array().unwrap();
        assert!(!events.is_empty());
        assert_eq!(events[0]["event_id"], "ch001_evt001");
        assert_eq!(events[0]["description"], "完整事件");
    }

    #[test]
    fn belongs_to_chapter_matches_id_and_event_prefix() {
        assert!(belongs_to_chapter(&json!({"chapter_id": "ch002"}), "ch002"));
        assert!(belongs_to_chapter(&json!({"event_id": "ch002_evt003"}), "ch002"));
        assert!(!belongs_to_chapter(&json!({"chapter_id": "ch001"}), "ch002"));
    }

    #[test]
    fn llm_call_against_mock_server() {
        // 进程内 mock OpenAI 兼容服务，验证 HTTP 请求与响应解析全链路
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let body = r#"{"choices":[{"message":{"content":"测试输出"}}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.as_bytes().len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });

        let base = format!("http://127.0.0.1:{}/v1", port);
        let msgs = vec![llm::ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let out = llm::call_llm(&base, "k", "m", msgs, 0.7, 0.9, Some(50)).unwrap();
        assert_eq!(out, "测试输出");
        let _ = handle.join();
    }

    #[test]
    fn llm_call_normalizes_missing_scheme() {
        // 用户漏写 http:// 时应自动补全，而不是报 builder error
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let body = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.as_bytes().len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        // 注意：没有 http:// 前缀
        let base = format!("127.0.0.1:{}", port);
        let msgs = vec![llm::ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let out = llm::call_llm(&base, "k", "m", msgs, 0.7, 0.9, Some(50)).unwrap();
        assert_eq!(out, "ok");
        let _ = handle.join();
    }
}


pub fn generate_chapter(app: Context, chapter_id: String, idea: String, target_words: u32,
    use_context: bool, overwrite: bool, pov: Option<String>, narrative: Option<String>,
    prompt_override: Option<PromptOverride>, context_options: Option<context::Options>) -> Result<CommandResult, String> {
    let chapter_id = canonical_chapter_id(&chapter_id)?;
    let _access = app.operation_access()?;
    app.stage("准备上下文")?;
    Ok(do_generate_chapter(&app, &chapter_id, &idea, target_words, use_context, overwrite,
        &pov.unwrap_or_default(), &narrative.unwrap_or_default(), prompt_override, context_options.unwrap_or_default(), None))
}
pub fn generate_manual_chapter(app: Context, chapter_id: String, prompts: PromptPair,
    overwrite: bool, input_budget: usize) -> Result<CommandResult, String> {
    let chapter_id = canonical_chapter_id(&chapter_id)?;
    preview_manual_prompt(prompts.clone(), input_budget)?;
    let _access = app.operation_access()?;
    app.stage("准备手动完整提示词")?;
    Ok(do_generate_chapter(&app, &chapter_id, "手动完整提示词", 0, false, overwrite,
        "", "", None, context::Options { input_budget, ..Default::default() }, Some(prompts)))
}
pub fn check_consistency(app: Context, chapter_id: String) -> Result<CommandResult, String> {
    let chapter_id = canonical_chapter_id(&chapter_id)?;
    let _access = app.operation_access()?;
    app.stage("准备一致性检查")?;
    Ok(do_check_consistency(&app, &chapter_id))
}
pub fn update_memory(app: Context, chapter_id: String) -> Result<CommandResult, String> {
    let chapter_id = canonical_chapter_id(&chapter_id)?;
    let _access = app.operation_access()?;
    app.stage("准备记忆抽取")?;
    Ok(do_update_memory(&app, &chapter_id))
}
pub fn reset_chapter(app: Context, chapter_id: String, include_memory: bool) -> Result<CommandResult, String> {
    let chapter_id = canonical_chapter_id(&chapter_id)?;
    let _access = app.write_access()?;
    Ok(do_reset_chapter(&app, &chapter_id, include_memory))
}

/// Canonical IDs also keep all report names inside outputs/reports.
pub fn canonical_chapter_id(id: &str) -> Result<String, String> {
    project_source::number(id).filter(|n| *n > 0).map(|n| format!("ch{n:03}"))
        .ok_or("章号格式应为 ch001、ch002……".into())
}

/// Optimistic edit: never replace a chapter that changed while the editor was open.
pub fn save_chapter_checked(app: Context, chapter_id: String, content: String, expected: Option<String>) -> Result<(), String> {
    let _access = app.write_access()?;
    let path = project_source::chapter_path(&app.project_dir, &canonical_chapter_id(&chapter_id)?)?;
    let current = if path.exists() { Some(fs::read_to_string(&path).map_err(|e| e.to_string())?) } else { None };
    if current != expected { return Err("内容冲突：正文已变更，请重新加载；编辑稿已保留".into()); }
    project_source::write(&app.project_dir, &path, &content)
}
pub fn save_memory_checked(app: Context, rel: String, content: String, expected: Option<String>) -> Result<(), String> {
    let _access = app.write_access()?;
    let rel = normalize_rel(&rel);
    if !EDITABLE_FILES.contains(&rel.as_str()) { return Err("不允许编辑该文件".into()); }
    let path = app.project_dir.join(rel);
    let current = if path.exists() { Some(fs::read_to_string(&path).map_err(|e| e.to_string())?) } else { None };
    if current != expected { return Err("内容冲突：文件已变更，请重新加载；编辑稿已保留".into()); }
    atomic_write(&path, content.as_bytes())
}

pub fn save_prompt_checked(app: Context, settings: PromptSettings, expected: serde_json::Value) -> Result<(), String> {
    let _access = app.write_access()?;
    let current = load_prompt_settings(&app.project_dir)?;
    if serde_json::to_value(&current).map_err(|e| e.to_string())? != expected {
        return Err("内容冲突：提示词已变更，请重新加载".into());
    }
    // Check only edited template fields. Untouched legacy drafts must not prevent
    // repairing another field, switching presets or changing guidance options.
    for (before, after) in prompt_tasks(&current).iter().zip(prompt_tasks(&settings)) {
        if before.template != after.template { after.validate(false)?; }
    }
    project_manager::atomic_json(&app.project_dir.join(".prompt-settings.json"), &settings)
}

pub fn test_model(app: &Context) -> Result<String, String> {
    let config = build_run_config(app)?;
    run_completion(&config, &app.settings, sys_user_messages("You are a helpful assistant.", "Reply with OK."), 0.0, 1.0, Some(16))
}

/// Apply only fields edited by the GUI, preserving concurrent changes to other settings.
pub fn save_settings_checked(app: Context, settings: Settings, expected: Settings) -> Result<Settings, String> {
    let _global = app.global_access()?;
    if !project_manager::same_path(Path::new(&settings.project_dir), &app.project_dir) {
        return Err("项目已变更，请重新加载设置".into());
    }
    let mut current = serde_json::to_value(Context::read_settings(&app.data_dir)?).map_err(|e| e.to_string())?;
    let next = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    let before = serde_json::to_value(expected).map_err(|e| e.to_string())?;
    for (key, val) in next.as_object().ok_or("设置格式错误")? {
        if key == "project_dir" || Some(val) == before.get(key) { continue; }
        if current.get(key) != before.get(key) && current.get(key) != Some(val) {
            return Err(format!("内容冲突：设置 {key} 已被其他入口修改，请重新加载"));
        }
        current[key] = val.clone();
    }
    let stored: Settings = serde_json::from_value(current).map_err(|e| e.to_string())?;
    project_manager::atomic_json(&settings_path(&app), &stored)?;
    let mut shown = stored;
    shown.project_dir = app.project_dir.to_string_lossy().into_owned();
    Ok(shown)
}

/// IPC compatibility: the existing read commands return empty text for an absent file.
/// This version treats absent and empty alike, while still detecting concurrent content changes.
pub fn save_chapter_expected(app: Context, id: String, content: String, expected: String) -> Result<(), String> {
    let _access = app.write_access()?;
    let path = project_source::chapter_path(&app.project_dir, &canonical_chapter_id(&id)?)?;
    let current = if path.exists() { fs::read_to_string(&path).map_err(|e| e.to_string())? } else { String::new() };
    if current != expected { return Err("内容冲突：正文已被其他入口修改，请重新加载；当前编辑内容尚未保存".into()); }
    project_source::write(&app.project_dir, &path, &content)
}
pub fn save_memory_expected(app: Context, rel: String, content: String, expected: String) -> Result<(), String> {
    let _access = app.write_access()?;
    let rel = normalize_rel(&rel);
    if !EDITABLE_FILES.contains(&rel.as_str()) { return Err("不允许编辑该文件".into()); }
    let path = app.project_dir.join(rel);
    let current = if path.exists() { fs::read_to_string(&path).map_err(|e| e.to_string())? } else { String::new() };
    if current != expected { return Err("内容冲突：文件已被其他入口修改，请重新加载；当前编辑内容尚未保存".into()); }
    atomic_write(&path, content.as_bytes())
}


pub fn model_diagnostics(app: &Context) -> serde_json::Value {
    let provider = app.settings.llm_provider.as_deref().unwrap_or("api");
    match provider {
        "claude_code" | "codex" => {
            let executable = if provider == "claude_code" { "claude" } else { "codex" };
            let path = llm::resolve_executable(executable).map(|(path, _)| path);
            serde_json::json!({"provider":provider,"executable":path,"available":path.is_some(),"authentication":"使用该用户已登录的本机 CLI；未自动调用模型"})
        }
        "api" => {
            let base = app.settings.llm_base_url.trim();
            let normalized = if base.starts_with("http://") || base.starts_with("https://") { base.to_owned() } else {format!("http://{base}")};
            let valid = reqwest::Url::parse(&normalized).map(|u| u.host_str().is_some()).unwrap_or(false);
            serde_json::json!({"provider":"api","url_valid":valid,"network_tested":false,"model":app.settings.llm_model})
        }
        _ => serde_json::json!({"provider":provider,"available":false,"error":"未知模型路线"}),
    }
}

pub fn save_source_checked(app: Context, settings: project_source::SourceSettings, expected: String) -> Result<(), String> {
    let _access = app.write_access()?;
    if project_source::load(&app.project_dir)?.directory != expected {
        return Err("内容冲突：正文来源已被其他入口修改，请重新加载设置".into());
    }
    if settings.directory.trim().is_empty() { return Err("正文目录不能为空".into()); }
    project_manager::atomic_json(&app.project_dir.join(".chapter-source.json"), &settings)
}
