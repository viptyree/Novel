mod jobs;
mod tui;
use clap::{Args, CommandFactory, Parser, Subcommand};
use novel_core::{self as core, Context};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{self, IsTerminal, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub static STOP_WAIT: AtomicBool = AtomicBool::new(false);
type Result<T> = std::result::Result<T, String>;

#[derive(Parser)]
#[command(name="novel", version, color=clap::ColorChoice::Never, about="Sodarie Novel — 共享 Rust 引擎的小说终端工作台")]
pub struct Cli {
    #[arg(long, global = true)]
    /// 仅为本次命令选择项目，不更改已保存的当前项目。
    pub project: Option<PathBuf>,
    /// Override the shared application data directory (useful for isolated testing).
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    /// stdout 输出 JSON；进度与诊断写入 stderr。
    pub json: bool,
    #[arg(long, global = true)]
    /// 禁用颜色。
    pub no_color: bool,
    /// Use a numbered, line-oriented menu.
    #[arg(long)]
    pub plain: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Subcommand)]
pub enum Command {
    /// 项目列表、新建、打开、置顶及重新定位
    Project {
        #[command(subcommand)]
        command: Project,
    },
    /// 章节读写、生成、检查和标题
    Chapter {
        #[command(subcommand)]
        command: Chapter,
    },
    /// 人物、世界观、大纲与长期记忆
    Memory {
        #[command(subcommand)]
        command: Memory,
    },
    /// 查看生成、一致性或记忆报告
    Report {
        #[command(subcommand)]
        command: Report,
    },
    /// 查看或按字段修改共享模型设置
    Settings {
        #[command(subcommand)]
        command: Settings,
    },
    /// 查看或修改项目正文目录
    Source {
        #[command(subcommand)]
        command: Source,
    },
    /// 编辑、预览和恢复项目提示词
    Prompt {
        #[command(subcommand)]
        command: Prompt,
    },
    /// 查看可选上下文材料
    Context {
        #[command(subcommand)]
        command: ContextCommand,
    },
    /// 查询、等待和安全取消后台任务
    Job {
        #[command(subcommand)]
        command: Job,
    },
    /// 检查环境；只有 --test-model 才调用模型
    Doctor {
        #[arg(long)]
        test_model: bool,
    },
    /// 打开交互式小说工作台
    Tui,
    #[command(name = "__worker", hide = true)]
    Worker { id: String },
}
#[derive(Subcommand)]
pub enum Project {
    List,
    Current,
    Create {
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    Open {
        path: PathBuf,
    },
    Pin {
        path: PathBuf,
    },
    Unpin {
        path: PathBuf,
    },
    Forget {
        path: PathBuf,
        #[arg(long)]
        yes: bool,
    },
    Relocate {
        previous_path: PathBuf,
        path: PathBuf,
    },
}
#[derive(Subcommand)]
pub enum Chapter {
    List,
    Show {
        id: String,
    },
    New {
        id: String,
    },
    Save {
        id: String,
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        overwrite: bool,
    },
    Edit {
        id: String,
        #[arg(long)]
        external: bool,
    },
    Generate(Generate),
    Check {
        id: String,
        #[arg(long)]
        detach: bool,
    },
    Reset {
        id: String,
        #[arg(long)]
        include_memory: bool,
        #[arg(long)]
        yes: bool,
    },
    Title {
        #[command(subcommand)]
        command: Title,
    },
}
#[derive(Subcommand)]
pub enum Title {
    Generate {
        id: String,
        #[arg(long)]
        previous_candidates: Option<PathBuf>,
        #[arg(long)]
        detach: bool,
    },
    Set {
        id: String,
        title: String,
    },
}
#[derive(Args, Clone, Serialize, Deserialize)]
#[command(group(clap::ArgGroup::new("chapter_idea").required(true).args(["idea", "idea_file", "idea_stdin"])))]
pub struct Generate {
    pub id: String,
    #[arg(long, conflicts_with_all=["idea_file","idea_stdin"])]
    /// 本章 idea 或事件骨架。
    pub idea: Option<String>,
    #[arg(long, conflicts_with_all=["idea","idea_stdin"])]
    /// 从 UTF-8 文件读取 idea。
    pub idea_file: Option<PathBuf>,
    #[arg(long, conflicts_with_all=["idea","idea_file"])]
    /// 从 stdin 读取 UTF-8 idea。
    pub idea_stdin: bool,
    #[arg(long, default_value_t=4000, value_parser=clap::value_parser!(u32).range(1..))]
    /// 目标中文字数。
    pub target_words: u32,
    #[arg(long)]
    /// 允许覆盖已有正文，并保留备份。
    pub overwrite: bool,
    #[arg(long)]
    /// 不读取长期记忆上下文。
    pub no_context: bool,
    #[arg(long)]
    /// 视角，例如 limited:主角、first:主角、objective。
    pub pov: Option<String>,
    #[arg(long)]
    /// 本章叙事要求。
    pub narrative: Option<String>,
    #[arg(long)]
    /// 上下文选项 JSON 文件。
    pub context_options: Option<PathBuf>,
    #[arg(long, value_parser = ["general", "compact", "benchmark"])]
    /// 仅本次生成/预览覆盖项目默认方案。
    pub preset: Option<String>,
    #[arg(long)]
    /// 已确认提示词 JSON，含 prompts 和 source。
    pub prompt_override: Option<PathBuf>,
    #[arg(long)]
    /// 后台提交后立即返回任务 ID。
    pub detach: bool,
}
#[derive(Args)]
#[group(required = true, multiple = false)]
pub struct Input {
    #[arg(long)]
    pub file: Option<PathBuf>,
    #[arg(long)]
    pub stdin: bool,
}
impl Input {
    fn read(&self) -> Result<String> {
        if let Some(file) = &self.file {
            return read(file);
        }
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        Ok(text)
    }
}
#[derive(Subcommand)]
pub enum Memory {
    List,
    Show {
        file: String,
    },
    Save {
        file: String,
        #[command(flatten)]
        input: Input,
    },
    Edit {
        file: String,
        #[arg(long)]
        external: bool,
    },
    Update {
        id: String,
        #[arg(long)]
        detach: bool,
    },
}
#[derive(Subcommand)]
pub enum Report {
    Show {
        id: String,
        #[arg(long, default_value="generation", value_parser=["generation","consistency","memory"])]
        kind: String,
    },
}
#[derive(Subcommand)]
pub enum Settings {
    Show,
    /// Set one field. Values are JSON scalars or plain strings; API keys require --stdin.
    Set {
        key: String,
        value: Option<String>,
        #[arg(long, conflicts_with = "value")]
        stdin: bool,
    },
}
#[derive(Subcommand)]
pub enum Source {
    Show,
    Set { directory: String },
}
#[derive(Subcommand)]
pub enum Prompt {
    Show,
    /// 查看或设置当前项目的默认生成方案。
    Preset {
        #[arg(value_parser = ["general", "compact", "benchmark"])]
        preset: Option<String>,
    },
    Save {
        #[command(flatten)]
        input: Input,
    },
    Edit {
        #[arg(long)]
        external: bool,
    },
    Reset {
        #[arg(long)]
        yes: bool,
    },
    Preview(Generate),
}
#[derive(Subcommand)]
pub enum ContextCommand {
    Catalog { id: String },
}
#[derive(Subcommand)]
pub enum Job {
    List,
    Show {
        id: String,
    },
    Logs {
        id: String,
        #[arg(long)]
        follow: bool,
    },
    Wait {
        id: String,
    },
    Cancel {
        id: String,
    },
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path)
        .map(|text| text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
        .map_err(|e| format!("{}: {e}", path.display()))
}
fn decode<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}
fn value<T: Serialize>(v: T) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}
fn yes(confirmed: bool) -> Result<()> {
    if confirmed {
        Ok(())
    } else {
        Err("此操作需要显式 --yes".into())
    }
}
fn absolute(path: &Path) -> Result<PathBuf> {
    Ok(if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    })
}
fn text_result(text: String) -> Result<Value> {
    Ok(Value::String(text))
}

pub fn main() {
    let mut cli = Cli::parse();
    cli.no_color |= std::env::var_os("NO_COLOR").is_some();
    tui::set_no_color(cli.no_color);
    let _ = ctrlc::set_handler(|| {
        STOP_WAIT.store(true, Ordering::Relaxed);
    });
    let json = cli.json;
    let result = run(cli);
    match result {
        Ok(output) => {
            if !output.is_null() {
                print_output(&output, json);
            }
        }
        Err(error) => {
            let code = if error.starts_with("资源忙") || error.starts_with("内容冲突") {
                3
            } else if error.starts_with("等待已中断") || error.starts_with("任务已取消") {
                130
            } else {
                1
            };
            if json {
                println!("{}", json!({"success":false,"error":error}));
            } else {
                eprintln!("{error}");
            }
            std::process::exit(code);
        }
    }
}
pub fn print_output(output: &Value, as_json: bool) {
    if !as_json {
        if let Value::String(s) = output {
            println!("{s}");
            return;
        }
    }
    println!("{}", serde_json::to_string_pretty(output).unwrap());
}
fn run(cli: Cli) -> Result<Value> {
    if cli.command.is_none() && !io::stdin().is_terminal() {
        Cli::command().print_help().map_err(|e| e.to_string())?;
        println!();
        return Ok(Value::Null);
    }
    let data = absolute(&cli.data_dir.unwrap_or(Context::default_data_dir()?))?;
    if let Some(Command::Worker { id }) = &cli.command {
        return jobs::worker(&data, id).map(|_| Value::Null);
    }
    let context = Context::new(data, cli.project.map(|p| absolute(&p)).transpose()?)?;
    match cli.command {
        None | Some(Command::Tui) => {
            tui::run(context, cli.plain, cli.no_color)?;
            Ok(Value::Null)
        }
        Some(command) => dispatch(context, command),
    }
}
pub fn generation(context: &Context, args: &Generate) -> Result<jobs::Generation> {
    let idea = if let Some(s) = &args.idea {
        s.clone()
    } else if let Some(p) = &args.idea_file {
        read(p)?
    } else if args.idea_stdin {
        Input {
            file: None,
            stdin: true,
        }
        .read()?
    } else {
        return Err("请提供 --idea、--idea-file 或 --idea-stdin".into());
    };
    if idea.trim().is_empty() || args.target_words == 0 {
        return Err("idea 和目标字数不能为空或为零".into());
    }
    let mut options: core::context::Options = args
        .context_options
        .as_ref()
        .map(|p| read(p).and_then(|s| decode(&s)))
        .transpose()?
        .unwrap_or_default();
    if let Some(preset) = &args.preset {
        options.generation_preset = Some(match preset.as_str() {
            "compact" => core::GenerationPreset::Compact,
            "benchmark" => core::GenerationPreset::Benchmark,
            _ => core::GenerationPreset::General,
        });
    }
    let prompt_override = args
        .prompt_override
        .as_ref()
        .map(|p| read(p).and_then(|s| decode(&s)))
        .transpose()?;
    let _ = context;
    Ok(jobs::Generation {
        id: core::canonical_chapter_id(&args.id)?,
        idea,
        target_words: args.target_words,
        use_context: !args.no_context,
        overwrite: args.overwrite,
        pov: args.pov.clone(),
        narrative: args.narrative.clone(),
        options,
        prompt_override,
    })
}
pub fn preview_generation(ctx: Context, g: jobs::Generation) -> Result<core::PromptPreview> {
    let mut preview = core::preview_generation_prompt(ctx, g.id, g.idea, g.target_words,
        g.use_context, g.pov.unwrap_or_default(), g.narrative.unwrap_or_default(), Some(g.options))?;
    if let Some(custom) = g.prompt_override {
        preview.prompts = core::choose_generation_prompt(preview.clone(), Some(custom))?;
        preview.stats.characters = preview.prompts.system.chars().count() + preview.prompts.user.chars().count();
        preview.stats.estimated_tokens = core::context::tokens(&preview.prompts.system) + core::context::tokens(&preview.prompts.user);
        preview.stats.over_budget = preview.stats.estimated_tokens > preview.stats.budget;
        if !preview.stats.over_budget { preview.stats.warnings.retain(|w| !w.starts_with("输入超过预算")); }
    }
    Ok(preview)
}

pub fn dispatch(ctx: Context, command: Command) -> Result<Value> {
    use core::{chapter_titles as titles, project_manager as projects};
    match command {
        Command::Project { command } => match command {
            Project::List => value(projects::list_projects(ctx)?),
            Project::Current => text_result(ctx.project_dir.display().to_string()),
            Project::Create { path, name } => text_result(projects::open_project(
                ctx,
                absolute(&path)?.display().to_string(),
                true,
                name,
                None,
            )?),
            Project::Open { path } => text_result(projects::open_project(
                ctx,
                absolute(&path)?.display().to_string(),
                false,
                None,
                None,
            )?),
            Project::Relocate {
                previous_path,
                path,
            } => text_result(projects::open_project(
                ctx,
                absolute(&path)?.display().to_string(),
                false,
                None,
                Some(absolute(&previous_path)?.display().to_string()),
            )?),
            Project::Pin { path } => {
                projects::update_project_entry(
                    ctx,
                    absolute(&path)?.display().to_string(),
                    Some(true),
                    false,
                )?;
                Ok(json!({"success":true}))
            }
            Project::Unpin { path } => {
                projects::update_project_entry(
                    ctx,
                    absolute(&path)?.display().to_string(),
                    Some(false),
                    false,
                )?;
                Ok(json!({"success":true}))
            }
            Project::Forget { path, yes: confirm } => {
                yes(confirm)?;
                projects::update_project_entry(
                    ctx,
                    absolute(&path)?.display().to_string(),
                    None,
                    true,
                )?;
                Ok(json!({"success":true}))
            }
        },
        Command::Chapter { command } => match command {
            Chapter::List => value(titles::list_chapter_details(ctx)?),
            Chapter::Show { id } => {
                let path = core::project_source::chapter_path(&ctx.project_dir, &id)?;
                if !path.is_file() {
                    return Err("章节不存在".into());
                }
                text_result(core::read_chapter(ctx, id)?)
            }
            Chapter::New { id } => {
                core::save_chapter_checked(ctx, id, String::new(), None)?;
                Ok(json!({"success":true}))
            }
            Chapter::Save {
                id,
                input,
                overwrite,
            } => {
                let content = input.read()?;
                let previous = chapter_before(&ctx, &id)?;
                if previous.is_some() && !overwrite {
                    return Err("章节已存在，覆盖需要 --overwrite".into());
                }
                core::save_chapter_checked(ctx, id, content, previous)?;
                Ok(json!({"success":true}))
            }
            Chapter::Edit { id, external } => {
                edit_chapter(ctx, id, external)?;
                Ok(json!({"success":true}))
            }
            Chapter::Generate(args) => {
                let request = generation(&ctx, &args)?;
                jobs::submit(ctx, jobs::Action::Generate(request), args.detach)
            }
            Chapter::Check { id, detach } => jobs::submit(
                ctx,
                jobs::Action::Check {
                    id: core::canonical_chapter_id(&id)?,
                },
                detach,
            ),
            Chapter::Reset {
                id,
                include_memory,
                yes: confirm,
            } => {
                yes(confirm)?;
                checked(core::reset_chapter(ctx, id, include_memory)?)
            }
            Chapter::Title { command } => match command {
                Title::Generate {
                    id,
                    previous_candidates,
                    detach,
                } => {
                    let candidates = previous_candidates
                        .map(|p| read(&p).and_then(|s| decode(&s)))
                        .transpose()?
                        .unwrap_or_default();
                    jobs::submit(
                        ctx,
                        jobs::Action::Titles {
                            id: core::canonical_chapter_id(&id)?,
                            previous_candidates: candidates,
                        },
                        detach,
                    )
                }
                Title::Set { id, title } => {
                    let path = core::project_source::chapter_path(&ctx.project_dir, &id)?;
                    let root = ctx.project_dir.display().to_string();
                    value(titles::save_chapter_title(
                        ctx,
                        root,
                        id,
                        title,
                        path.display().to_string(),
                    )?)
                }
            },
        },
        Command::Memory { command } => match command {
            Memory::List => value(core::EDITABLE_FILES),
            Memory::Show { file } => text_result(core::read_memory_file(ctx, file)?),
            Memory::Save { file, input } => {
                core::save_memory_file(ctx, file, input.read()?)?;
                Ok(json!({"success":true}))
            }
            Memory::Edit { file, external } => {
                edit_memory(ctx, file, external)?;
                Ok(json!({"success":true}))
            }
            Memory::Update { id, detach } => jobs::submit(
                ctx,
                jobs::Action::Memory {
                    id: core::canonical_chapter_id(&id)?,
                },
                detach,
            ),
        },
        Command::Report {
            command: Report::Show { id, kind },
        } => text_result(core::read_report(ctx, id, kind)?),
        Command::Settings { command } => match command {
            Settings::Show => {
                let mut settings = ctx.settings.clone();
                settings.llm_api_key = "[hidden]".into();
                value(settings)
            }
            Settings::Set {
                key,
                value: input,
                stdin,
            } => {
                if key == "project_dir" {
                    return Err("请使用 project open 切换项目".into());
                }
                if key == "llm_api_key" && !stdin {
                    return Err("密钥请通过 --stdin 输入，避免写入 shell 历史".into());
                }
                let input = if stdin {
                    Input {
                        file: None,
                        stdin: true,
                    }
                    .read()?
                    .trim_end_matches(['\r', '\n'])
                    .to_string()
                } else {
                    input.ok_or("请提供值或 --stdin")?
                };
                let _lock = ctx.global_access()?;
                let mut settings = value(Context::read_settings(&ctx.data_dir)?)?;
                if settings.get(&key).is_none() {
                    return Err(format!("未知设置：{key}"));
                }
                settings[&key] = if matches!(
                    key.as_str(),
                    "llm_base_url" | "llm_api_key" | "llm_model" | "llm_provider"
                ) {
                    Value::String(input)
                } else {
                    serde_json::from_str(&input).map_err(|e| e.to_string())?
                };
                let parsed: core::Settings =
                    serde_json::from_value(settings).map_err(|e| e.to_string())?;
                if !matches!(
                    parsed.llm_provider.as_deref(),
                    None | Some("api" | "claude_code" | "codex")
                ) {
                    return Err("未知模型路线".into());
                }
                projects::atomic_json(&ctx.data_dir.join(".ui-settings.json"), &parsed)?;
                Ok(json!({"success":true}))
            }
        },
        Command::Source { command } => match command {
            Source::Show => value(core::get_source_settings(ctx)?),
            Source::Set { directory } => {
                core::save_source_settings(
                    ctx,
                    core::project_source::SourceSettings { directory },
                )?;
                Ok(json!({"success":true}))
            }
        },
        Command::Prompt { command } => match command {
            Prompt::Show => value(core::get_prompt_settings(ctx)?),
            Prompt::Preset { preset } => {
                let mut settings = core::get_prompt_settings(ctx.clone())?;
                if let Some(name) = preset {
                    let before = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
                    settings.generation_preset = match name.as_str() {
                        "compact" => core::GenerationPreset::Compact,
                        "benchmark" => core::GenerationPreset::Benchmark,
                        _ => core::GenerationPreset::General,
                    };
                    core::save_prompt_checked(ctx.clone(), settings.clone(), before)?;
                }
                let label = match settings.generation_preset {
                    core::GenerationPreset::General => "通用方案",
                    core::GenerationPreset::Compact => "精简四段式",
                    core::GenerationPreset::Benchmark => "对标方案",
                };
                Ok(json!({"generation_preset": settings.generation_preset,
                    "label": label,
                    "project": ctx.project_dir}))
            },
            Prompt::Save { input } => {
                core::save_prompt_settings(ctx, decode(&input.read()?)?)?;
                Ok(json!({"success":true}))
            }
            Prompt::Edit { external } => {
                edit_prompt(ctx, external)?;
                Ok(json!({"success":true}))
            }
            Prompt::Reset { yes: confirm } => {
                yes(confirm)?;
                core::save_prompt_settings(ctx, core::default_prompt_settings())?;
                Ok(json!({"success":true}))
            }
            Prompt::Preview(args) => {
                let g = generation(&ctx, &args)?;
                value(preview_generation(ctx, g)?)
            }
        },
        Command::Context {
            command: ContextCommand::Catalog { id },
        } => value(core::context_catalog(ctx, id)?),
        Command::Job { command } => match command {
            Job::List => jobs::list(&ctx.data_dir),
            Job::Show { id } => value(jobs::load(&ctx.data_dir, &id)?),
            Job::Logs { id, follow } => jobs::logs(&ctx.data_dir, &id, follow),
            Job::Wait { id } => jobs::wait(&ctx.data_dir, &id),
            Job::Cancel { id } => {
                jobs::cancel(&ctx.data_dir, &id)?;
                Ok(json!({"success":true,"message":"已请求安全取消；模型请求或文件提交完成后生效"}))
            }
        },
        Command::Doctor { test_model } => {
            let project = ctx.project_dir.is_dir();
            let source = core::project_source::source_dir(&ctx.project_dir)?;
            let mut output = json!({"data_dir":ctx.data_dir,"project":ctx.project_dir,"project_exists":project,"source":source,"provider":ctx.settings.llm_provider,"tty":io::stdin().is_terminal(),"model_configuration":core::model_diagnostics(&ctx)});
            if test_model {
                output["model_response"] = value(core::test_model(&ctx)?)?;
            }
            Ok(output)
        }
        Command::Tui | Command::Worker { .. } => Err("内部入口不可从菜单调度".into()),
    }
}
pub fn checked(result: core::CommandResult) -> Result<Value> {
    if !result.success {
        Err(result.log)
    } else {
        value(result)
    }
}
fn chapter_before(ctx: &Context, id: &str) -> Result<Option<String>> {
    let _read = ctx.read_access()?;
    let path = core::project_source::chapter_path(&ctx.project_dir, id)?;
    if path.exists() {
        read(&path).map(Some)
    } else {
        Ok(None)
    }
}
pub fn edit_chapter(ctx: Context, id: String, external: bool) -> Result<()> {
    let before = chapter_before(&ctx, &id)?;
    let content = tui::edit(
        &ctx.data_dir,
        &format!("正文 {id}"),
        before.as_deref().unwrap_or(""),
        external,
    )?;
    if let Some(content) = content {
        if let Err(e) = core::save_chapter_checked(ctx.clone(), id, content.clone(), before) {
            return Err(preserve_edit(&ctx, &content, e));
        }
    }
    Ok(())
}
pub fn edit_memory(ctx: Context, file: String, external: bool) -> Result<()> {
    let text = core::read_memory_file(ctx.clone(), file.clone())?;
    let before = if ctx.project_dir.join(&file).is_file() {
        Some(text.clone())
    } else {
        None
    };
    if let Some(content) = tui::edit(&ctx.data_dir, &file, &text, external)? {
        if let Err(e) = core::save_memory_checked(ctx.clone(), file, content.clone(), before) {
            return Err(preserve_edit(&ctx, &content, e));
        }
    }
    Ok(())
}
pub fn edit_prompt(ctx: Context, external: bool) -> Result<()> {
    let before = value(core::get_prompt_settings(ctx.clone())?)?;
    let text = serde_json::to_string_pretty(&before).unwrap();
    if let Some(content) = tui::edit(&ctx.data_dir, "提示词设置 JSON", &text, external)? {
        let parsed = decode(&content).map_err(|e| preserve_edit(&ctx, &content, e))?;
        // Compare and write under one lock through the core checked API.
        core::save_prompt_checked(ctx.clone(), parsed, before)
            .map_err(|e| preserve_edit(&ctx, &content, e))?;
    }
    Ok(())
}
fn preserve_edit(ctx: &Context, text: &str, error: String) -> String {
    let path = ctx
        .data_dir
        .join("edit-recovery")
        .join(format!("{}.txt", uuid::Uuid::new_v4()));
    match core::atomic_write(&path, text.as_bytes()) {
        Ok(()) => format!("{error}\n编辑稿保留于 {}", path.display()),
        Err(e) => format!("{error}\n恢复稿保存失败：{e}\n{text}"),
    }
}
