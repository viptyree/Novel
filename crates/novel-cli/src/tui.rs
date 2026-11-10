use crate::{dispatch, read, Cli, Result};
use clap::Parser;
use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use novel_core::{self as core, Context};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::Path,
    process,
};
use unicode_width::UnicodeWidthChar;

static PLAIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

type Screen = Terminal<CrosstermBackend<io::Stdout>>;
struct Session {
    terminal: Screen,
}
fn restore() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
}
impl Session {
    fn new() -> Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err("交互界面需要 TTY；SSH 请分配终端，或使用标准命令".into());
        }
        terminal::enable_raw_mode().map_err(|e| e.to_string())?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste) {
            restore();
            return Err(e.to_string());
        }
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout())).map_err(|e| {
            restore();
            e.to_string()
        })?;
        Ok(Self { terminal })
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        restore();
    }
}
fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let old = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            old(info);
        }));
    });
}
fn next_event() -> Result<Event> {
    event::read().map_err(|e| e.to_string())
}

// Navigation is explicit. Cancelling a dialog never unwinds an entire module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nav {
    Back,
    Home,
}
#[derive(Debug, PartialEq, Eq)]
enum Choice<T> {
    Selected(T),
    Back,
    Home,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum ChapterPurpose {
    Workbench,
    Reports,
    Preview,
    Context,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PromptCategory { General, Compact, Consistency, Memory }
impl PromptCategory {
    fn label(self) -> &'static str { match self {
        Self::General => "通用方案", Self::Compact => "精简四段式",
        Self::Consistency => "一致性检查", Self::Memory => "更新记忆",
    } }
    fn task(self, settings: &core::PromptSettings) -> core::TaskPrompt { match self {
        Self::General => core::TaskPrompt { system: settings.system.clone(), user_template: settings.user_template.clone() },
        Self::Compact => settings.compact_generation.clone(), Self::Consistency => settings.consistency.clone(), Self::Memory => settings.memory.clone(),
    } }
    fn replace(self, settings: &mut core::PromptSettings, task: core::TaskPrompt) { match self {
        Self::General => { settings.system = task.system; settings.user_template = task.user_template; },
        Self::Compact => settings.compact_generation = task, Self::Consistency => settings.consistency = task, Self::Memory => settings.memory = task,
    } }
    fn guidance(self, settings: &core::PromptSettings) -> Option<bool> { match self {
        Self::General => Some(settings.general_material_guidance), Self::Compact => Some(settings.compact_material_guidance), _ => None,
    } }
    fn set_guidance(self, settings: &mut core::PromptSettings, enabled: bool) { match self {
        Self::General => settings.general_material_guidance = enabled, Self::Compact => settings.compact_material_guidance = enabled, _ => {},
    } }
}
fn save_template_field(ctx: &Context, category: PromptCategory, system: bool, expected: &str, text: &str) -> Result<()> {
    let mut latest = core::get_prompt_settings(ctx.clone())?;
    let before = serde_json::to_value(&latest).map_err(|e| e.to_string())?;
    let mut task = category.task(&latest);
    let field = if system { &mut task.system } else { &mut task.user_template };
    if field != expected && field != text { return Err("该提示词已被其他入口修改，请重新打开；当前编辑尚未保存".into()); }
    *field = text.into();
    category.replace(&mut latest, task);
    core::save_prompt_checked(ctx.clone(), latest, before)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Page {
    Home,
    Projects,
    RecentProjects,
    Chapters(ChapterPurpose),
    Chapter(String),
    Memories,
    Memory(String),
    Prompts,
    PromptCategories,
    PromptTemplate(PromptCategory),
    PromptPreview(String),
    Settings,
    Report(String),
    Jobs,
    Job(String),
    Generate { id: String, preview: bool },
}
impl Page {
    fn label(&self) -> String {
        match self {
            Self::Home => "主页".into(),
            Self::Projects => "项目管理".into(),
            Self::RecentProjects => "最近项目".into(),
            Self::Chapters(p) => match p {
                ChapterPurpose::Workbench => "章节工作台",
                ChapterPurpose::Reports => "报告章节",
                ChapterPurpose::Preview => "选择预览章节",
                ChapterPurpose::Context => "选择上下文章节",
            }
            .into(),
            Self::Chapter(id) => format!("章节 {id}"),
            Self::Memories => "记忆与大纲".into(),
            Self::Memory(file) => file.clone(),
            Self::Prompts => "提示词与上下文".into(),
            Self::PromptCategories => "分类编辑提示词模板".into(),
            Self::PromptTemplate(category) => format!("{}模板（项目永久保存）", category.label()),
            Self::PromptPreview(id) => format!("{id} 预览与编辑"),
            Self::Settings => "模型与设置".into(),
            Self::Report(id) => format!("报告 {id}"),
            Self::Jobs => "任务中心".into(),
            Self::Job(id) => format!("任务 {}", &id[..id.len().min(8)]),
            Self::Generate { id, preview } => format!(
                "{id} {}",
                if *preview {
                    "提示词预览"
                } else {
                    "生成设置"
                }
            ),
        }
    }
}
struct Frame {
    page: Page,
    list: ListState,
}
struct Navigation {
    frames: Vec<Frame>,
}
impl Navigation {
    fn new() -> Self {
        Self {
            frames: vec![Frame {
                page: Page::Home,
                list: ListState::default().with_selected(Some(0)),
            }],
        }
    }
    fn push(&mut self, page: Page) {
        self.frames.push(Frame {
            page,
            list: ListState::default().with_selected(Some(0)),
        });
    }
    fn go(&mut self, nav: Nav) {
        match nav {
            Nav::Back if self.frames.len() > 1 => {
                self.frames.pop();
            }
            Nav::Home => self.frames.truncate(1),
            _ => {}
        }
    }
    fn page(&self) -> &Page {
        &self.frames.last().unwrap().page
    }
    fn path(&self) -> String {
        self.frames
            .iter()
            .map(|f| f.page.label())
            .collect::<Vec<_>>()
            .join(" › ")
    }
}
fn short_path(path: &str, width: u16) -> String {
    use unicode_width::UnicodeWidthStr;
    let (route, status) = path
        .split_once('\n')
        .map(|(route, status)| (route, Some(status)))
        .unwrap_or((path, None));
    let mut shown = route.to_string();
    if route.width() > width as usize {
        let parts: Vec<_> = route.split(" › ").collect();
        shown = parts.last().unwrap_or(&route).to_string();
        for part in parts[..parts.len().saturating_sub(1)].iter().rev() {
            let candidate = format!("{part} › {shown}");
            if candidate.width() + 4 > width as usize {
                break;
            }
            shown = candidate;
        }
        shown = format!("… › {shown}");
    }
    if let Some(status) = status {
        shown.push('\n');
        shown.push_str(status);
    }
    shown
}
fn regions(area: Rect) -> [Rect; 3] {
    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(area);
    [chunks[0], chunks[1], chunks[2]]
}
fn menu_choice(
    path: &str,
    items: &[String],
    plain: bool,
    no_color: bool,
    state: &mut ListState,
    root: bool,
) -> Result<Choice<usize>> {
    let mut labels = items.to_vec();
    if !root {
        labels.extend(["返回上一级".into(), "返回主页".into()]);
    }
    let finish = |index: usize| {
        if index < items.len() {
            Choice::Selected(index)
        } else if index == items.len() {
            Choice::Back
        } else {
            Choice::Home
        }
    };
    if plain {
        loop {
            println!("\n{path}");
            for (i, label) in labels.iter().enumerate() {
                println!("{}. {label}", i + 1);
            }
            let text = plain_line("编号；0 返回上一级，H 主页", "")?;
            if text.eq_ignore_ascii_case("h") {
                return Ok(Choice::Home);
            }
            if text == "0" {
                return Ok(Choice::Back);
            }
            if let Ok(n) = text.parse::<usize>() {
                if n > 0 && n <= labels.len() {
                    state.select(Some(n - 1));
                    return Ok(finish(n - 1));
                }
            }
        }
    }
    let mut session = Session::new()?;
    let mut digits = String::new();
    state.select(Some(
        state
            .selected()
            .unwrap_or(0)
            .min(labels.len().saturating_sub(1)),
    ));
    loop {
        session
            .terminal
            .draw(|f| {
                let [header, body, footer] = regions(f.area());
                f.render_widget(
                    Paragraph::new(short_path(path, header.width)).wrap(Wrap { trim: false }),
                    header,
                );
                let highlight = if no_color {
                    Style::default().add_modifier(ratatui::style::Modifier::REVERSED)
                } else {
                    Style::default().fg(Color::Cyan)
                };
                f.render_stateful_widget(
                    List::new(
                        labels
                            .iter()
                            .enumerate()
                            .map(|(i, s)| ListItem::new(format!("{}. {s}", i + 1))),
                    )
                    .block(Block::default().borders(Borders::ALL))
                    .highlight_style(highlight)
                    .highlight_symbol("› "),
                    body,
                    state,
                );
                f.render_widget(
                    Paragraph::new(if root {
                        "↑↓ / 数字 Enter 选择\nEsc 已在主页\nHome / H 主页"
                    } else {
                        "↑↓ / 数字 Enter 选择\n[Esc 返回上一级]\n[Home / H 返回主页]"
                    }),
                    footer,
                );
            })
            .map_err(|e| e.to_string())?;
        if let Event::Key(k) = next_event()? {
            if k.kind == KeyEventKind::Release {
                continue;
            }
            let current = state.selected().unwrap_or(0);
            match k.code {
                KeyCode::Esc => return Ok(Choice::Back),
                KeyCode::Home | KeyCode::Char('h' | 'H') => return Ok(Choice::Home),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(Choice::Back)
                }
                KeyCode::Up => {
                    state.select(Some(current.saturating_sub(1)));
                    digits.clear();
                }
                KeyCode::Down => {
                    state.select(Some((current + 1).min(labels.len() - 1)));
                    digits.clear();
                }
                KeyCode::PageUp => {
                    state.select(Some(current.saturating_sub(10)));
                    digits.clear();
                }
                KeyCode::PageDown => {
                    state.select(Some((current + 10).min(labels.len() - 1)));
                    digits.clear();
                }
                KeyCode::Enter => return Ok(finish(current)),
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    digits.push(c);
                    let mut n = digits.parse::<usize>().unwrap_or(0);
                    if n == 0 || n > labels.len() {
                        digits = c.to_string();
                        n = digits.parse().unwrap_or(0);
                    }
                    if n > 0 && n <= labels.len() {
                        state.select(Some(n - 1));
                    }
                }
                _ => {}
            }
        }
    }
}
fn plain_line(label: &str, default: &str) -> Result<String> {
    print!(
        "{label}{}：",
        if default.is_empty() {
            String::new()
        } else {
            format!(" [{default}]")
        }
    );
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut text = String::new();
    if io::stdin()
        .read_line(&mut text)
        .map_err(|e| e.to_string())?
        == 0
    {
        return Err("输入已结束".into());
    }
    let text = text.trim_end_matches(['\r', '\n']);
    Ok(if text.is_empty() {
        default.into()
    } else {
        text.into()
    })
}
fn confirm(path: &str, label: &str, plain: bool, no_color: bool) -> Result<Choice<bool>> {
    match menu_choice(
        &format!("{path} › {label}"),
        &["否 / 取消".into(), "是".into()],
        plain,
        no_color,
        &mut ListState::default().with_selected(Some(0)),
        false,
    )? {
        Choice::Selected(i) => Ok(Choice::Selected(i == 1)),
        Choice::Back => Ok(Choice::Back),
        Choice::Home => Ok(Choice::Home),
    }
}
fn viewer(path: &str, text: &str, plain: bool) -> Result<Nav> {
    if plain {
        println!("\n{path}\n{text}");
        return Ok(
            if plain_line("Enter / 0 返回上一级，H 主页", "")?.eq_ignore_ascii_case("h") {
                Nav::Home
            } else {
                Nav::Back
            },
        );
    }
    let mut session = Session::new()?;
    let mut offset = 0u16;
    loop {
        session
            .terminal
            .draw(|f| {
                let [header, body, footer] = regions(f.area());
                f.render_widget(Paragraph::new(short_path(path, header.width)), header);
                f.render_widget(
                    Paragraph::new(text)
                        .wrap(Wrap { trim: false })
                        .scroll((offset, 0))
                        .block(Block::default().borders(Borders::ALL)),
                    body,
                );
                f.render_widget(
                    Paragraph::new("↑↓ / PgUp PgDn 滚动\n[Esc 返回上一级]\n[Home / H 返回主页]"),
                    footer,
                );
            })
            .map_err(|e| e.to_string())?;
        if let Event::Key(k) = next_event()? {
            if k.kind == KeyEventKind::Release {
                continue;
            }
            match k.code {
                KeyCode::Esc | KeyCode::Enter => return Ok(Nav::Back),
                KeyCode::Home | KeyCode::Char('h' | 'H') => return Ok(Nav::Home),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(Nav::Back)
                }
                KeyCode::Up => offset = offset.saturating_sub(1),
                KeyCode::Down => offset = offset.saturating_add(1),
                KeyCode::PageUp => offset = offset.saturating_sub(10),
                KeyCode::PageDown => offset = offset.saturating_add(10),
                KeyCode::Char('a') if k.modifiers.contains(KeyModifiers::CONTROL) => offset = 0,
                _ => {}
            }
        }
    }
}
fn format_prompt_preview(output: serde_json::Value) -> Result<String> {
    let preview: core::PromptPreview = serde_json::from_value(output).map_err(|e| e.to_string())?;
    let warnings = if preview.stats.warnings.is_empty() { String::new() } else {
        format!("\n\n注意：\n{}", preview.stats.warnings.join("\n"))
    };
    Ok(format!("输入长度：{} 字符，约 {} tokens；预算 {} tokens\n\n系统提示词\n\n{}\n\n章节提示词\n\n{}{}\n\n返回预览菜单可直接编辑并采用；选择提交后台生成才会调用模型。",
        preview.stats.characters, preview.stats.estimated_tokens, preview.stats.budget,
        preview.prompts.system, preview.prompts.user, warnings))
}

#[derive(Clone)]
struct GenerationDraft {
    idea: String,
    target: String,
    use_context: bool,
    overwrite: bool,
    pov: String,
    narrative: String,
    options: String,
    prompt: String,
    confirmed: Option<core::PromptPreview>,
    confirmed_input: String,
    preview: Option<core::PromptPreview>,
}
impl Default for GenerationDraft {
    fn default() -> Self {
        Self {
            idea: String::new(),
            target: "4000".into(),
            use_context: true,
            overwrite: false,
            pov: String::new(),
            narrative: String::new(),
            options: serde_json::to_string_pretty(&core::context::Options::default()).unwrap(),
            prompt: String::new(),
            confirmed: None,
            confirmed_input: String::new(),
            preview: None,
        }
    }
}
impl GenerationDraft {
    fn input_key(&self) -> String {
        serde_json::to_string(&(&self.idea, &self.target, self.use_context, &self.pov,
            &self.narrative, &self.options, &self.prompt)).unwrap()
    }
    fn generation(&self, id: &str, use_confirmed: bool) -> Result<crate::jobs::Generation> {
        if self.idea.trim().is_empty() { return Err("请先填写本章事件骨架".into()); }
        let target_words = self.target.parse::<u32>().map_err(|_| "目标中文字数必须为正整数")?;
        if target_words == 0 { return Err("目标中文字数必须为正整数".into()); }
        let options: core::context::Options = serde_json::from_str(&self.options).map_err(|e| e.to_string())?;
        options.validate()?;
        let prompt_override = if use_confirmed && self.confirmed.is_some() {
            if self.confirmed_input != self.input_key() {
                return Err("本章手动预览已过期，请重新打开预览并采用，或恢复自动组装".into());
            }
            self.confirmed.as_ref().map(|p| core::PromptOverride { prompts: p.prompts.clone(), source: p.source.clone() })
        } else if !self.prompt.trim().is_empty() {
            Some(serde_json::from_str(&read(Path::new(&self.prompt))?).map_err(|e| e.to_string())?)
        } else { None };
        Ok(crate::jobs::Generation { id: core::canonical_chapter_id(id)?, idea: self.idea.trim().into(),
            target_words, use_context: self.use_context, overwrite: self.overwrite,
            pov: Some(self.pov.clone()), narrative: Some(self.narrative.clone()), options, prompt_override })
    }
    fn automatic(&mut self) {
        self.confirmed = None;
        self.confirmed_input.clear();
        self.preview = None;
        self.prompt.clear();
    }
    fn adopt(&mut self, preview: core::PromptPreview) {
        self.confirmed_input = self.input_key();
        self.confirmed = Some(preview.clone());
        self.preview = Some(preview);
    }
}
fn update_preview_stats(preview: &mut core::PromptPreview) {
    preview.stats.characters = preview.prompts.system.chars().count() + preview.prompts.user.chars().count();
    preview.stats.estimated_tokens = core::context::tokens(&preview.prompts.system) + core::context::tokens(&preview.prompts.user);
    preview.stats.over_budget = preview.stats.estimated_tokens > preview.stats.budget;
    if !preview.stats.over_budget { preview.stats.warnings.retain(|w| !w.starts_with("输入超过预算")); }
}
fn validate_preview(ctx: &Context, id: &str, draft: &GenerationDraft, edited: &core::PromptPreview) -> Result<()> {
    let fresh = crate::preview_generation(ctx.clone(), draft.generation(id, false)?)?;
    core::choose_generation_prompt(fresh, Some(core::PromptOverride {
        prompts: edited.prompts.clone(), source: edited.source.clone(),
    })).map(|_| ())
}
fn preset_label(preset: core::GenerationPreset) -> &'static str {
    if preset == core::GenerationPreset::Compact { "精简四段式" } else { "通用方案" }
}

struct App {
    ctx: Context,
    plain: bool,
    no_color: bool,
    nav: Navigation,
    keys: Vec<String>,
    drafts: std::collections::HashMap<(std::path::PathBuf, String), GenerationDraft>,
}
enum Effect {
    Stay,
    Push(Page),
    Home,
    Exit,
}
macro_rules! answer {
    ($value:expr) => {
        match $value? {
            Choice::Selected(v) => v,
            Choice::Back => return Ok(Effect::Stay),
            Choice::Home => return Ok(Effect::Home),
        }
    };
}
fn words(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| s.to_string()).collect()
}
impl App {
    fn path(&self, label: &str) -> String {
        format!("{} › {label}", self.nav.path())
    }
    fn input(&self, label: &str, default: &str) -> Result<Choice<String>> {
        input_dialog(&self.path(label), default, self.plain, false)
    }
    fn confirm(&self, label: &str) -> Result<Choice<bool>> {
        confirm(&self.nav.path(), label, self.plain, self.no_color)
    }
    fn view(&self, label: &str, text: &str) -> Result<Effect> {
        Ok(
            if viewer(&self.path(label), text, self.plain)? == Nav::Home {
                Effect::Home
            } else {
                Effect::Stay
            },
        )
    }
    fn execute(&mut self, args: Vec<String>) -> Result<Effect> {
        let switch = args.first().map(String::as_str) == Some("project")
            && matches!(
                args.get(1).map(String::as_str),
                Some("open" | "create" | "relocate")
            );
        let is_preview = args.first().map(String::as_str) == Some("prompt")
            && args.get(1).map(String::as_str) == Some("preview");
        let cli = Cli::try_parse_from(std::iter::once("novel".to_string()).chain(args))
            .map_err(|e| e.to_string())?;
        let output = dispatch(self.ctx.clone(), cli.command.ok_or("缺少操作")?)?;
        self.ctx = Context::new(
            self.ctx.data_dir.clone(),
            if switch {
                None
            } else {
                Some(self.ctx.project_dir.clone())
            },
        )?;
        if is_preview {
            return self.view("提示词预览（未调用模型）", &format_prompt_preview(output)?);
        }
        let text = output
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| serde_json::to_string_pretty(&output).unwrap());
        self.view("操作结果", &text)
    }
    fn items(&mut self) -> Result<Vec<String>> {
        self.keys.clear();
        let labels: Vec<String> = match self.nav.page().clone() {
            Page::Home => words(&[
                "项目管理",
                "章节工作台",
                "记忆与大纲",
                "提示词与上下文",
                "模型与设置",
                "报告",
                "任务中心",
                "环境检查",
                "退出",
            ]),
            Page::Projects => words(&[
                "最近项目 / 切换",
                "新建项目",
                "按路径打开",
                "置顶",
                "取消置顶",
                "移除记录",
                "重新定位",
            ]),
            Page::RecentProjects => {
                let entries = core::project_manager::list_projects(self.ctx.clone())?;
                self.keys = entries.iter().map(|p| p.entry.path.clone()).collect();
                entries
                    .iter()
                    .map(|p| {
                        format!(
                            "{}{} {}",
                            if p.exists { "" } else { "[缺失] " },
                            p.entry.name,
                            p.entry.path
                        )
                    })
                    .collect()
            }
            Page::Chapters(_) => {
                let entries = core::chapter_titles::list_chapter_details(self.ctx.clone())?;
                self.keys = entries.iter().map(|c| c.id.clone()).collect();
                let mut labels = entries
                    .iter()
                    .map(|c| format!("{} {}", c.id, c.title))
                    .collect::<Vec<_>>();
                labels.push("输入章号 / 新章节".into());
                labels
            }
            Page::Chapter(_) => words(&[
                "阅读正文",
                "编辑正文",
                "外部编辑器",
                "新建空章",
                "生成 / 重新生成",
                "一致性检查",
                "更新长期记忆",
                "生成三个章名",
                "保存章名",
                "重置章节",
            ]),
            Page::Memories => core::EDITABLE_FILES.iter().map(|s| s.to_string()).collect(),
            Page::Memory(_) => words(&["查看", "内置编辑器", "外部编辑器", "从文件导入"]),
            Page::Prompts => {
                let preset = core::get_prompt_settings(self.ctx.clone())?.generation_preset;
                let mut labels = words(&[
                    "查看提示词",
                    "编辑提示词模板（分类）",
                    "外部编辑完整 JSON（高级）",
                    "从 JSON 导入提示词",
                    "恢复默认提示词",
                    "预览章节提示词",
                    "上下文目录",
                    "查看正文来源",
                    "设置正文来源",
                ]);
                labels.push(format!("项目默认生成方案：{}", if preset == core::GenerationPreset::Compact { "精简四段式" } else { "通用方案" }));
                labels
            },
            Page::PromptCategories => words(&["通用方案", "精简四段式", "一致性检查", "更新记忆"]),
            Page::PromptTemplate(category) => {
                let settings = core::get_prompt_settings(self.ctx.clone())?;
                let mut labels = words(&["编辑系统提示词", "编辑任务提示词模板", "查看模板与变量说明", "恢复当前分类默认模板"]);
                if let Some(enabled) = category.guidance(&settings) {
                    labels.push(format!("追加资料使用原则：{}", if enabled { "开启" } else { "关闭" }));
                }
                labels
            }
            Page::Settings => words(&[
                "查看设置",
                "llm_provider",
                "llm_base_url",
                "llm_model",
                "llm_api_key",
                "temperature",
                "top_p",
                "max_output_tokens",
                "disable_thinking",
            ]),
            Page::Report(_) => words(&["生成报告", "一致性报告", "记忆更新报告"]),
            Page::Jobs => {
                let jobs = crate::jobs::list(&self.ctx.data_dir)?;
                let jobs = jobs.as_array().ok_or("任务列表格式错误")?;
                self.keys = jobs
                    .iter()
                    .filter_map(|j| j["id"].as_str().map(str::to_owned))
                    .collect();
                jobs.iter()
                    .map(|j| {
                        format!(
                            "{} {} {}",
                            j["id"].as_str().unwrap_or(""),
                            j["state"].as_str().unwrap_or(""),
                            j["stage"].as_str().unwrap_or("")
                        )
                    })
                    .collect()
            }
            Page::Job(_) => words(&["状态 / 结果", "阶段日志", "查看实时进度", "请求安全取消"]),
            Page::PromptPreview(_) => words(&["查看完整提示词", "编辑系统提示词（保存即采用）",
                "编辑章节提示词（保存即采用）", "采用当前预览用于本章", "恢复自动组装"]),
            Page::Generate { id, preview } => {
                let default_preset = core::get_prompt_settings(self.ctx.clone())?.generation_preset;
                let d = self
                    .drafts
                    .entry((self.ctx.project_dir.clone(), id))
                    .or_default();
                let mut labels = vec![
                    format!("Idea / 事件骨架（{} 字）", d.idea.chars().count()),
                    format!("目标中文字数：{}", d.target),
                    format!("长期记忆：{}", if d.use_context { "是" } else { "否" }),
                    format!("覆盖已有正文：{}", if d.overwrite { "是" } else { "否" }),
                    format!("视角：{}", d.pov),
                    "叙事要求".into(),
                    "上下文选项 JSON".into(),
                    "已确认提示词 JSON 文件".into(),
                    "提示词预览（不调用模型）".into(),
                ];
                if !preview { labels.push("提交后台生成".into()); }
                let options: core::context::Options = serde_json::from_str(&d.options).map_err(|e| e.to_string())?;
                labels.push(format!("本章生成方案：{}", options.generation_preset.map(preset_label)
                    .map(str::to_string).unwrap_or_else(|| format!("跟随项目默认（{}）", preset_label(default_preset)))));
                labels.push(format!("恢复自动组装（{}）", if d.confirmed.is_some() {
                    if d.confirmed_input == d.input_key() { "已采用本章预览" } else { "本章预览已过期" }
                } else if !d.prompt.is_empty() { "使用导入文件" } else { "当前自动" }));
                labels
            }
        };
        Ok(labels)
    }
}
impl App {
    fn act(&mut self, index: usize) -> Result<Effect> {
        let page = self.nav.page().clone();
        let mut args: Vec<String>;
        match page {
            Page::Home => {
                return Ok(match index {
                    0 => Effect::Push(Page::Projects),
                    1 => Effect::Push(Page::Chapters(ChapterPurpose::Workbench)),
                    2 => Effect::Push(Page::Memories),
                    3 => Effect::Push(Page::Prompts),
                    4 => Effect::Push(Page::Settings),
                    5 => Effect::Push(Page::Chapters(ChapterPurpose::Reports)),
                    6 => Effect::Push(Page::Jobs),
                    7 => {
                        let mut args = words(&["doctor"]);
                        if answer!(self.confirm("发送最小模型测试请求？")) {
                            args.push("--test-model".into());
                        }
                        return self.execute(args);
                    }
                    _ => Effect::Exit,
                })
            }
            Page::Projects => {
                if index == 0 {
                    return Ok(Effect::Push(Page::RecentProjects));
                }
                if index == 6 {
                    let old = answer!(self.input("原路径", ""));
                    let new = answer!(self.input("新路径", ""));
                    args = words(&["project", "relocate", &old, &new]);
                } else {
                    let path = answer!(self.input("项目完整路径", ""));
                    args = words(&[
                        "project",
                        ["", "create", "open", "pin", "unpin", "forget"][index],
                        &path,
                    ]);
                    if index == 1 {
                        let name = answer!(self.input("项目名称（可留空）", ""));
                        if !name.is_empty() {
                            args.extend(words(&["--name", &name]));
                        }
                    }
                    if index == 5 {
                        if !answer!(self.confirm("仅移除项目记录？")) {
                            return Ok(Effect::Stay);
                        }
                        args.push("--yes".into());
                    }
                }
            }
            Page::RecentProjects => {
                let path = self
                    .keys
                    .get(index)
                    .ok_or("项目列表已变更，请刷新")?
                    .clone();
                args = words(&["project", "open", &path]);
            }
            Page::Chapters(purpose) => {
                let id = if let Some(id) = self.keys.get(index) {
                    id.clone()
                } else {
                    core::canonical_chapter_id(&answer!(self.input("章号", "ch001")))?
                };
                return match purpose {
                    ChapterPurpose::Workbench => Ok(Effect::Push(Page::Chapter(id))),
                    ChapterPurpose::Reports => Ok(Effect::Push(Page::Report(id))),
                    ChapterPurpose::Preview => {
                        Ok(Effect::Push(Page::Generate { id, preview: true }))
                    }
                    ChapterPurpose::Context => self.execute(words(&["context", "catalog", &id])),
                };
            }
            Page::Chapter(id) => {
                args = match index {
                    0 => words(&["chapter", "show", &id]),
                    1 | 2 => return self.edit_chapter(&id, index == 2),
                    3 => words(&["chapter", "new", &id]),
                    4 => return Ok(Effect::Push(Page::Generate { id, preview: false })),
                    5 => words(&["chapter", "check", &id, "--detach"]),
                    6 => words(&["memory", "update", &id, "--detach"]),
                    7 => {
                        let previous = answer!(self.input("上一批候选 JSON 文件（可留空）", ""));
                        let mut args = words(&["chapter", "title", "generate", &id, "--detach"]);
                        if !previous.is_empty() {
                            args.extend(words(&["--previous-candidates", &previous]));
                        }
                        args
                    }
                    8 => {
                        let title = answer!(self.input("章名", ""));
                        words(&["chapter", "title", "set", &id, &title])
                    }
                    _ => {
                        if !answer!(self.confirm("备份并重置本章正文和报告？")) {
                            return Ok(Effect::Stay);
                        }
                        let mut args = words(&["chapter", "reset", &id, "--yes"]);
                        if answer!(self.confirm("同时清除本章记忆记录？")) {
                            args.push("--include-memory".into());
                        }
                        args
                    }
                };
            }
            Page::PromptPreview(id) => return self.preview_action(&id, index),
            Page::PromptCategories => {
                let category = *[PromptCategory::General, PromptCategory::Compact, PromptCategory::Consistency, PromptCategory::Memory]
                    .get(index).ok_or("无效的提示词分类")?;
                return Ok(Effect::Push(Page::PromptTemplate(category)));
            }
            Page::PromptTemplate(category) => return self.template_action(category, index),
            Page::Memories => {
                return Ok(Effect::Push(Page::Memory(
                    core::EDITABLE_FILES
                        .get(index)
                        .ok_or("文件列表已变更")?
                        .to_string(),
                )))
            }
            Page::Memory(file) => {
                args = match index {
                    0 => words(&["memory", "show", &file]),
                    1 | 2 => return self.edit_memory(&file, index == 2),
                    _ => {
                        let path = answer!(self.input("UTF-8 文件路径", ""));
                        words(&["memory", "save", &file, "--file", &path])
                    }
                }
            }
            Page::Prompts => {
                args = match index {
                    0 => words(&["prompt", "show"]),
                    1 => return Ok(Effect::Push(Page::PromptCategories)),
                    2 => return self.edit_prompt(true),
                    3 => {
                        let path = answer!(self.input("提示词 JSON 文件路径", ""));
                        words(&["prompt", "save", "--file", &path])
                    }
                    4 => {
                        if !answer!(self.confirm("恢复默认提示词？")) {
                            return Ok(Effect::Stay);
                        }
                        words(&["prompt", "reset", "--yes"])
                    }
                    5 => return Ok(Effect::Push(Page::Chapters(ChapterPurpose::Preview))),
                    6 => return Ok(Effect::Push(Page::Chapters(ChapterPurpose::Context))),
                    7 => words(&["source", "show"]),
                    9 => {
                        let preset = core::get_prompt_settings(self.ctx.clone())?.generation_preset;
                        let index = answer!(menu_choice(
                            &self.path("选择项目默认生成方案（保存后持续生效）"),
                            &words(&["通用方案", "精简四段式"]), self.plain, self.no_color,
                            &mut ListState::default().with_selected(Some(if preset == core::GenerationPreset::Compact { 1 } else { 0 })), false,
                        ));
                        words(&["prompt", "preset", if index == 1 { "compact" } else { "general" }])
                    }
                    _ => {
                        let current = core::get_source_settings(self.ctx.clone())?.directory;
                        let directory =
                            answer!(self.input("正文目录（相对项目或绝对路径）", &current));
                        words(&["source", "set", &directory])
                    }
                }
            }
            Page::Settings => {
                if index == 0 {
                    args = words(&["settings", "show"]);
                } else {
                    let fields = [
                        "",
                        "llm_provider",
                        "llm_base_url",
                        "llm_model",
                        "llm_api_key",
                        "temperature",
                        "top_p",
                        "max_output_tokens",
                        "disable_thinking",
                    ];
                    let field = fields[index];
                    if field == "llm_api_key" {
                        let secret = answer!(input_dialog(
                            &self.path("密钥输入（不回显）"),
                            "",
                            self.plain,
                            true
                        ));
                        {
                            let _global = self.ctx.global_access()?;
                            let mut settings = Context::read_settings(&self.ctx.data_dir)?;
                            settings.llm_api_key = secret;
                            core::project_manager::atomic_json(
                                &self.ctx.data_dir.join(".ui-settings.json"),
                                &settings,
                            )?;
                        }
                        self.ctx = Context::new(
                            self.ctx.data_dir.clone(),
                            Some(self.ctx.project_dir.clone()),
                        )?;
                        return self.view("操作结果", "密钥已保存。");
                    }
                    let settings =
                        serde_json::to_value(&self.ctx.settings).map_err(|e| e.to_string())?;
                    let current = settings
                        .get(field)
                        .and_then(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .or_else(|| Some(v.to_string()))
                        })
                        .unwrap_or_default();
                    let next = answer!(self.input(field, &current));
                    args = words(&["settings", "set", field, &next]);
                }
            }
            Page::Report(id) => {
                args = words(&[
                    "report",
                    "show",
                    &id,
                    "--kind",
                    ["generation", "consistency", "memory"][index],
                ])
            }
            Page::Jobs => {
                return Ok(Effect::Push(Page::Job(
                    self.keys.get(index).ok_or("任务列表已变更")?.clone(),
                )))
            }
            Page::Job(id) => {
                if index == 2 {
                    return Ok(
                        if job_monitor(&self.path("实时进度"), &self.ctx, &id, self.plain)?
                            == Nav::Home
                        {
                            Effect::Home
                        } else {
                            Effect::Stay
                        },
                    );
                }
                if index == 3 && !answer!(self.confirm("请求安全取消任务？")) {
                    return Ok(Effect::Stay);
                }
                args = words(&["job", ["show", "logs", "", "cancel"][index], &id]);
            }
            Page::Generate { id, preview } => return self.generate_action(&id, preview, index),
        }
        self.execute(args)
    }
    fn edit_chapter(&self, id: &str, external: bool) -> Result<Effect> {
        let before = crate::chapter_before(&self.ctx, id)?;
        let nav = edit_with_save(
            &self.ctx.data_dir,
            &self.path("编辑正文"),
            before.as_deref().unwrap_or(""),
            external,
            self.plain,
            |text| {
                core::save_chapter_checked(
                    self.ctx.clone(),
                    id.to_string(),
                    text.to_string(),
                    before.clone(),
                )
            },
        )?;
        Ok(if nav == Nav::Home {
            Effect::Home
        } else {
            Effect::Stay
        })
    }
    fn edit_memory(&self, file: &str, external: bool) -> Result<Effect> {
        let text = core::read_memory_file(self.ctx.clone(), file.to_string())?;
        let before = if self.ctx.project_dir.join(file).is_file() {
            Some(text.clone())
        } else {
            None
        };
        let nav = edit_with_save(
            &self.ctx.data_dir,
            &self.path("编辑文件"),
            &text,
            external,
            self.plain,
            |text| {
                core::save_memory_checked(
                    self.ctx.clone(),
                    file.to_string(),
                    text.to_string(),
                    before.clone(),
                )
            },
        )?;
        Ok(if nav == Nav::Home {
            Effect::Home
        } else {
            Effect::Stay
        })
    }
    fn template_action(&self, category: PromptCategory, index: usize) -> Result<Effect> {
        let mut settings = core::get_prompt_settings(self.ctx.clone())?;
        let before = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
        let task = category.task(&settings);
        if index < 2 {
            let system = index == 0;
            let initial = if system { task.system } else { task.user_template };
            let mut expected = initial.clone();
            let title = self.path(if system { "系统提示词（保存到当前项目）" } else { "任务模板（直接换行，保留 {{变量}}；保存到当前项目）" });
            let nav = edit_with_save(&self.ctx.data_dir, &title, &initial, false, self.plain, |text| {
                save_template_field(&self.ctx, category, system, &expected, text)?;
                expected = text.into();
                Ok(())
            })?;
            return Ok(if nav == Nav::Home { Effect::Home } else { Effect::Stay });
        }
        if index == 2 {
            let variables = match category {
                PromptCategory::General | PromptCategory::Compact => "{{人物状态}}、{{前情}}、{{相关设定与伏笔}}、{{文风参考}}、{{事件骨架}}（必需）、{{目标字数}}、{{叙事要求}}、{{章节ID}}",
                PromptCategory::Consistency => "{{章节ID}}、{{章节正文}}（必需）、{{事件骨架}}、{{人物状态}}、{{世界观规则}}、{{伏笔}}、{{近期事件}}、{{时间线}}",
                PromptCategory::Memory => "{{章节ID}}、{{章节正文}}（必需）、{{人物状态}}、{{伏笔}}。请保留默认 JSON 输出结构及字段名，程序依赖这些字段更新记忆",
            };
            return self.view("模板与变量说明", &format!("系统提示词\n\n{}\n\n任务模板\n\n{}\n\n可用变量：{}\n\n直接编辑文字和换行即可；保存后持续生效，不改变项目默认方案。未引用的资料不会发送。", task.system, task.user_template, variables));
        }
        if index == 3 {
            if !answer!(self.confirm("仅恢复当前分类模板及其资料原则开关？")) { return Ok(Effect::Stay); }
            let defaults = core::default_prompt_settings();
            category.replace(&mut settings, category.task(&defaults));
            if let Some(enabled) = category.guidance(&defaults) { category.set_guidance(&mut settings, enabled); }
        } else if index == 4 {
            let enabled = category.guidance(&settings).ok_or("当前分类没有资料原则开关")?;
            category.set_guidance(&mut settings, !enabled);
        } else { return Err("无效的模板操作".into()); }
        core::save_prompt_checked(self.ctx.clone(), settings, before)?;
        Ok(Effect::Stay)
    }
    fn edit_prompt(&self, external: bool) -> Result<Effect> {
        let before = serde_json::to_value(core::get_prompt_settings(self.ctx.clone())?)
            .map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(&before).unwrap();
        let nav = edit_with_save(
            &self.ctx.data_dir,
            &self.path("编辑提示词 JSON"),
            &text,
            external,
            self.plain,
            |text| {
                let parsed = serde_json::from_str(text).map_err(|e| e.to_string())?;
                core::save_prompt_checked(self.ctx.clone(), parsed, before.clone())
            },
        )?;
        Ok(if nav == Nav::Home {
            Effect::Home
        } else {
            Effect::Stay
        })
    }
    fn generate_action(&mut self, id: &str, preview: bool, index: usize) -> Result<Effect> {
        let key = (self.ctx.project_dir.clone(), id.to_string());
        let draft = self.drafts.entry(key.clone()).or_default().clone();
        if index == 2 || index == 3 {
            let d = self.drafts.get_mut(&key).unwrap();
            if index == 2 {
                d.use_context = !d.use_context;
            } else {
                d.overwrite = !d.overwrite;
            }
            return Ok(Effect::Stay);
        }
        if index < 8 {
            let (label, initial) = match index {
                0 => ("Idea / 事件骨架", draft.idea),
                1 => ("目标中文字数", draft.target),
                4 => ("视角", draft.pov),
                5 => ("叙事要求", draft.narrative),
                6 => ("上下文选项 JSON", draft.options),
                _ => ("已确认提示词 JSON 文件", draft.prompt),
            };
            let path = self.path(label);
            let d = self.drafts.get_mut(&key).unwrap();
            if matches!(index, 0 | 5 | 6) {
                let nav = edit_with_save(
                    &self.ctx.data_dir,
                    &path,
                    &initial,
                    false,
                    self.plain,
                    |text| {
                        if index == 6 {
                            let options: core::context::Options =
                                serde_json::from_str(text).map_err(|e| e.to_string())?;
                            options.validate()?;
                        }
                        match index {
                            0 => d.idea = text.into(),
                            5 => d.narrative = text.into(),
                            _ => d.options = text.into(),
                        };
                        Ok(())
                    },
                )?;
                return Ok(if nav == Nav::Home {
                    Effect::Home
                } else {
                    Effect::Stay
                });
            }
            match input_dialog(&path, &initial, self.plain, false)? {
                Choice::Selected(text) => match index {
                    1 => d.target = text,
                    4 => d.pov = text,
                    _ => d.prompt = text,
                },
                Choice::Back => {}
                Choice::Home => return Ok(Effect::Home),
            };
            return Ok(Effect::Stay);
        }
        let preset_index = if preview { 9 } else { 10 };
        if index == preset_index {
            let options: core::context::Options = serde_json::from_str(&draft.options).map_err(|e| e.to_string())?;
            let default_preset = core::get_prompt_settings(self.ctx.clone())?.generation_preset;
            let selected = answer!(menu_choice(&self.path("选择本章生成方案（不修改项目默认）"),
                &[format!("跟随项目默认（{}）", preset_label(default_preset)), "通用方案".into(), "精简四段式".into()],
                self.plain, self.no_color, &mut ListState::default().with_selected(Some(match options.generation_preset {
                    None => 0, Some(core::GenerationPreset::General) => 1, Some(core::GenerationPreset::Compact) => 2,
                })), false));
            let mut options = options;
            options.generation_preset = match selected { 1 => Some(core::GenerationPreset::General),
                2 => Some(core::GenerationPreset::Compact), _ => None };
            self.drafts.get_mut(&key).unwrap().options = serde_json::to_string_pretty(&options).unwrap();
            return Ok(Effect::Stay);
        }
        if index == preset_index + 1 {
            self.drafts.get_mut(&key).unwrap().automatic();
            return Ok(Effect::Stay);
        }
        if index == 8 { return self.open_preview(id); }
        if preview || index != 9 { return Err("无效的生成操作".into()); }
        let mut generation = draft.generation(id, true)?;
        // Validate before submitting a worker, and freeze the exact prompt checked here.
        let checked = crate::preview_generation(self.ctx.clone(), generation.clone())?;
        core::choose_generation_prompt(checked.clone(), None)?;
        generation.prompt_override = Some(core::PromptOverride { prompts: checked.prompts, source: checked.source });
        if core::project_source::chapter_path(&self.ctx.project_dir, id)?.is_file() && !draft.overwrite {
            if !answer!(self.confirm("覆盖已有正文（自动备份）？")) { return Ok(Effect::Stay); }
            generation.overwrite = true;
            self.drafts.get_mut(&key).unwrap().overwrite = true;
        }
        let output = crate::jobs::submit(self.ctx.clone(), crate::jobs::Action::Generate(generation), true)?;
        self.view("操作结果", &serde_json::to_string_pretty(&output).unwrap())
    }
    fn open_preview(&mut self, id: &str) -> Result<Effect> {
        let key = (self.ctx.project_dir.clone(), id.to_string());
        let draft = self.drafts.get(&key).ok_or("本章生成草稿不存在")?;
        let mut fresh = crate::preview_generation(self.ctx.clone(), draft.generation(id, false)?)?;
        if let Some(confirmed) = &draft.confirmed {
            if draft.confirmed_input == draft.input_key() && confirmed.source == fresh.source {
                fresh.prompts = confirmed.prompts.clone();
                update_preview_stats(&mut fresh);
            }
        }
        self.drafts.get_mut(&key).unwrap().preview = Some(fresh);
        Ok(Effect::Push(Page::PromptPreview(id.to_string())))
    }
    fn preview_action(&mut self, id: &str, index: usize) -> Result<Effect> {
        let key = (self.ctx.project_dir.clone(), id.to_string());
        let candidate = self.drafts.get(&key).and_then(|d| d.preview.clone()).ok_or("请重新打开提示词预览")?;
        if index == 0 { return self.view("完整提示词（未调用模型）",
            &format_prompt_preview(serde_json::to_value(candidate).map_err(|e| e.to_string())?)?); }
        if index == 4 {
            self.drafts.get_mut(&key).unwrap().automatic();
            let draft = &self.drafts[&key];
            let fresh = crate::preview_generation(self.ctx.clone(), draft.generation(id, false)?)?;
            self.drafts.get_mut(&key).unwrap().preview = Some(fresh);
            return Ok(Effect::Stay);
        }
        if index == 3 {
            let draft = self.drafts.get_mut(&key).unwrap();
            validate_preview(&self.ctx, id, draft, &candidate)?;
            draft.adopt(candidate);
            return self.view("本章预览已采用", "本章生成将使用当前提示词。尚未调用模型；返回生成设置后提交。更改参数或资料后需要重新预览。");
        }
        if index != 1 && index != 2 { return Err("无效的预览操作".into()); }
        let initial = if index == 1 { &candidate.prompts.system } else { &candidate.prompts.user };
        let path = self.path(if index == 1 { "系统提示词（保存即用于本章，不修改项目模板）" }
            else { "章节提示词（保存即用于本章，不修改项目模板）" });
        let ctx = self.ctx.clone();
        let draft = self.drafts.get_mut(&key).unwrap();
        let mut candidate = candidate.clone();
        let nav = edit_with_save(&ctx.data_dir, &path, initial, false, self.plain, |text| {
            let mut edited = candidate.clone();
            if index == 1 { edited.prompts.system = text.into(); } else { edited.prompts.user = text.into(); }
            update_preview_stats(&mut edited);
            validate_preview(&ctx, id, draft, &edited)?;
            draft.adopt(edited.clone());
            candidate = edited;
            Ok(())
        })?;
        Ok(if nav == Nav::Home { Effect::Home } else { Effect::Stay })
    }
}
fn job_monitor(path: &str, ctx: &Context, id: &str, plain: bool) -> Result<Nav> {
    if plain {
        return viewer(
            path,
            &serde_json::to_string_pretty(&crate::jobs::load(&ctx.data_dir, id)?).unwrap(),
            true,
        );
    }
    let mut session = Session::new()?;
    loop {
        let status = crate::jobs::load(&ctx.data_dir, id)?;
        let text = serde_json::to_string_pretty(&status).unwrap();
        session
            .terminal
            .draw(|f| {
                let [header, body, footer] = regions(f.area());
                f.render_widget(Paragraph::new(short_path(path, header.width)), header);
                f.render_widget(
                    Paragraph::new(text.as_str())
                        .wrap(Wrap { trim: false })
                        .block(Block::default().borders(Borders::ALL)),
                    body,
                );
                f.render_widget(
                    Paragraph::new(
                        "自动刷新；返回不取消任务\n[Esc 返回上一级]\n[Home / H 返回主页]",
                    ),
                    footer,
                );
            })
            .map_err(|e| e.to_string())?;
        if event::poll(std::time::Duration::from_millis(250)).map_err(|e| e.to_string())? {
            if let Event::Key(k) = next_event()? {
                if k.kind == KeyEventKind::Release {
                    continue;
                }
                match k.code {
                    KeyCode::Esc | KeyCode::Enter => return Ok(Nav::Back),
                    KeyCode::Home | KeyCode::Char('h' | 'H') => return Ok(Nav::Home),
                    _ => {}
                }
            }
        }
    }
}
pub fn run(mut ctx: Context, plain: bool, no_color: bool) -> Result<()> {
    install_panic_hook();
    let plain = plain || std::env::var("TERM").as_deref() == Ok("dumb");
    PLAIN.store(plain, std::sync::atomic::Ordering::Relaxed);
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("菜单需要交互终端".into());
    }
    let saved = Context::new(ctx.data_dir.clone(), None)?;
    if core::project_manager::same_path(&saved.project_dir, &ctx.project_dir) {
        core::project_manager::remember_startup(&ctx)?;
        ctx = Context::new(ctx.data_dir.clone(), None)?;
    }
    let mut app = App {
        ctx,
        plain,
        no_color,
        nav: Navigation::new(),
        drafts: std::collections::HashMap::new(),
        keys: vec![],
    };
    loop {
        crate::STOP_WAIT.store(false, std::sync::atomic::Ordering::Relaxed);
        let items = match app.items() {
            Ok(items) => items,
            Err(error) => {
                let nav = viewer(&app.path("加载失败"), &error, plain)?;
                app.nav.go(nav);
                continue;
            }
        };
        let project = app
            .ctx
            .project_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let path = if *app.nav.page() == Page::Home {
            let chapters =
                core::chapter_titles::list_chapter_details(app.ctx.clone()).unwrap_or_default();
            let recent = chapters.last().map(|c| c.id.as_str()).unwrap_or("无章节");
            let jobs = crate::jobs::list(&app.ctx.data_dir)
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            let active = jobs
                .iter()
                .filter(|j| j["state"] == "running" || j["state"] == "starting")
                .count();
            format!(
                "{}\n{project} · 最近 {recent} · 任务 {active}",
                app.nav.path()
            )
        } else {
            format!("{}\n项目：{project}", app.nav.path())
        };
        let root = *app.nav.page() == Page::Home;
        let choice = menu_choice(
            &path,
            &items,
            plain,
            no_color,
            &mut app.nav.frames.last_mut().unwrap().list,
            root,
        )?;
        let effect = match choice {
            Choice::Back => {
                app.nav.go(Nav::Back);
                continue;
            }
            Choice::Home => {
                app.nav.go(Nav::Home);
                continue;
            }
            Choice::Selected(index) => match app.act(index) {
                Ok(effect) => effect,
                Err(error) => {
                    if viewer(&app.path("操作未完成"), &error, plain)? == Nav::Home {
                        Effect::Home
                    } else {
                        Effect::Stay
                    }
                }
            },
        };
        match effect {
            Effect::Stay => {}
            Effect::Push(page) => app.nav.push(page),
            Effect::Home => app.nav.go(Nav::Home),
            Effect::Exit => return Ok(()),
        }
    }
}
/// Standalone CLI editing keeps its public interface; the TUI supplies a real save callback.
pub fn edit(data: &Path, title: &str, initial: &str, external: bool) -> Result<Option<String>> {
    let mut saved = None;
    edit_with_save(
        data,
        title,
        initial,
        external,
        PLAIN.load(std::sync::atomic::Ordering::Relaxed),
        |text| {
            saved = Some(text.to_string());
            Ok(())
        },
    )?;
    Ok(saved)
}
fn edit_with_save<F: FnMut(&str) -> Result<()>>(
    data: &Path,
    title: &str,
    initial: &str,
    external: bool,
    plain: bool,
    mut save: F,
) -> Result<Nav> {
    install_panic_hook();
    if external {
        if let Some(text) = external_edit(data, initial)? {
            if let Err(error) = save(&text) {
                // Continue with the edited text intact; do not close the editor after a failed save.
                return editor(
                    title,
                    initial,
                    &text,
                    false,
                    false,
                    plain,
                    Some(error),
                    save,
                );
            }
        }
        return Ok(Nav::Back);
    }
    editor(title, initial, initial, false, false, plain, None, save)
}
fn input_dialog(title: &str, default: &str, plain: bool, secret: bool) -> Result<Choice<String>> {
    if secret {
        return secret_input(title);
    }
    let mut accepted = None;
    let nav = editor(title, default, default, true, secret, plain, None, |text| {
        accepted = Some(text.to_string());
        Ok(())
    })?;
    Ok(if nav == Nav::Home {
        Choice::Home
    } else if let Some(text) = accepted {
        Choice::Selected(text)
    } else {
        Choice::Back
    })
}
fn leave_request(
    nav: Nav,
    dirty: bool,
    single_line: bool,
    pending: &mut Option<Nav>,
) -> Option<Nav> {
    if dirty && !single_line {
        *pending = Some(nav);
        None
    } else {
        Some(nav)
    }
}
#[allow(clippy::too_many_arguments)]
fn editor<F: FnMut(&str) -> Result<()>>(
    title: &str,
    original: &str,
    initial: &str,
    single_line: bool,
    secret: bool,
    plain: bool,
    error: Option<String>,
    mut save: F,
) -> Result<Nav> {
    if plain || std::env::var("TERM").as_deref() == Ok("dumb") {
        return plain_editor(title, original, initial, single_line, secret, error, save);
    }
    let mut session = Session::new()?;
    let mut chars: Vec<_> = initial.chars().collect();
    let mut cursor = chars.len();
    let mut offset = 0;
    let mut search: Option<String> = None;
    let mut pending: Option<Nav> = None;
    let mut confirm_index = 2;
    let mut message = error;
    loop {
        let text: String = chars.iter().collect();
        let size = session.terminal.size().map_err(|e| e.to_string())?;
        let [_, body, _] = regions(Rect::new(0, 0, size.width, size.height));
        let width = body.width.saturating_sub(2).max(1) as usize;
        let height = body.height.saturating_sub(2).max(1) as usize;
        let (lines, positions) = layout(&chars, width, secret);
        let (row, col) = positions[cursor];
        if row < offset {
            offset = row;
        }
        if row >= offset + height {
            offset = row - height + 1;
        }
        session
            .terminal
            .draw(|f| {
                let [header, body, footer] = regions(f.area());
                f.render_widget(Paragraph::new(short_path(title, header.width)), header);
                f.render_widget(
                    Paragraph::new(lines.join("\n"))
                        .scroll((offset.min(u16::MAX as usize) as u16, 0))
                        .block(Block::default().borders(Borders::ALL)),
                    body,
                );
                let footer_text = if let Some(message) = &message {
                    format!("保存失败：{message}")
                } else if let Some(query) = &search {
                    format!("搜索：{query}\nEnter 查找 · Esc 关闭搜索\nHome 返回主页")
                } else if single_line {
                    "Enter 确认 · Esc 取消\nHome 返回主页\nCtrl+A 行首 · Ctrl+E 行尾".into()
                } else {
                    "Ctrl+S / F2 保存\nEsc 上一级 · Home 主页\nCtrl+F 搜索 · Ctrl+A/E 行首尾".into()
                };
                f.render_widget(
                    Paragraph::new(footer_text).wrap(Wrap { trim: false }),
                    footer,
                );
                if let Some(target) = pending {
                    let w = f.area().width.min(44);
                    let h = f.area().height.min(8);
                    let popup =
                        Rect::new((f.area().width - w) / 2, (f.area().height - h) / 2, w, h);
                    f.render_widget(Clear, popup);
                    let mut state = ListState::default().with_selected(Some(confirm_index));
                    f.render_stateful_widget(
                        List::new(["S 保存后离开", "D 放弃修改", "C 继续编辑"])
                            .block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .title(if target == Nav::Home {
                                        "回主页前保存？"
                                    } else {
                                        "返回前保存？"
                                    })
                                    .title_bottom("↑↓ Enter · Esc 继续"),
                            )
                            .highlight_symbol("› ")
                            .highlight_style(
                                Style::default().add_modifier(ratatui::style::Modifier::REVERSED),
                            ),
                        popup,
                        &mut state,
                    );
                } else if body.width > 2 && body.height > 2 {
                    f.set_cursor_position((
                        body.x + 1 + (col as u16).min(body.width - 3),
                        body.y + 1 + (row.saturating_sub(offset) as u16).min(body.height - 3),
                    ));
                }
            })
            .map_err(|e| e.to_string())?;
        match next_event()? {
            Event::Paste(paste) if pending.is_none() && search.is_none() => {
                let text = paste.replace("\r\n", "\n").replace('\r', "\n");
                let text = if single_line {
                    text.replace('\n', " ")
                } else {
                    text
                };
                let add: Vec<_> = text.chars().collect();
                let len = add.len();
                chars.splice(cursor..cursor, add);
                cursor += len;
                message = None;
            }
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                if let Some(target) = pending {
                    let decision = match k.code {
                        KeyCode::Char('s' | 'S') => Some(0),
                        KeyCode::Char('d' | 'D' | 'y' | 'Y') => Some(1),
                        KeyCode::Char('c' | 'C') | KeyCode::Esc => Some(2),
                        KeyCode::Up => {
                            confirm_index = confirm_index.saturating_sub(1);
                            None
                        }
                        KeyCode::Down => {
                            confirm_index = (confirm_index + 1).min(2);
                            None
                        }
                        KeyCode::Enter => Some(confirm_index),
                        KeyCode::Home => {
                            pending = Some(Nav::Home);
                            None
                        }
                        _ => None,
                    };
                    match decision {
                        Some(0) => match save(&text) {
                            Ok(()) => return Ok(target),
                            Err(error) => {
                                message = Some(error);
                                pending = None;
                            }
                        },
                        Some(1) => return Ok(target),
                        Some(2) => pending = None,
                        _ => {}
                    }
                    continue;
                }
                if k.code == KeyCode::Home {
                    if let Some(nav) =
                        leave_request(Nav::Home, text != original, single_line, &mut pending)
                    {
                        return Ok(nav);
                    }
                    search = None;
                    confirm_index = 2;
                    continue;
                }
                if let Some(query) = &mut search {
                    match k.code {
                        KeyCode::Esc => search = None,
                        KeyCode::Backspace => {
                            query.pop();
                        }
                        KeyCode::Enter => {
                            if !query.is_empty() {
                                let tail: String = chars[cursor..].iter().collect();
                                let full: String = chars.iter().collect();
                                if let Some(pos) = tail.find(query.as_str()) {
                                    cursor += tail[..pos].chars().count();
                                } else if let Some(pos) = full.find(query.as_str()) {
                                    cursor = full[..pos].chars().count();
                                }
                            }
                            search = None;
                        }
                        KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                            query.push(c)
                        }
                        _ => {}
                    }
                    continue;
                }
                match k.code {
                    KeyCode::F(2) => match save(&text) {
                        Ok(()) => return Ok(Nav::Back),
                        Err(error) => message = Some(error),
                    },
                    KeyCode::Char('s') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        match save(&text) {
                            Ok(()) => return Ok(Nav::Back),
                            Err(error) => message = Some(error),
                        }
                    }
                    KeyCode::Enter if single_line => match save(&text) {
                        Ok(()) => return Ok(Nav::Back),
                        Err(error) => message = Some(error),
                    },
                    KeyCode::Char('f')
                        if k.modifiers.contains(KeyModifiers::CONTROL) && !secret =>
                    {
                        search = Some(String::new());
                        message = None;
                    }
                    KeyCode::Esc => {
                        if let Some(nav) =
                            leave_request(Nav::Back, text != original, single_line, &mut pending)
                        {
                            return Ok(nav);
                        }
                        confirm_index = 2;
                    }
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Some(nav) =
                            leave_request(Nav::Back, text != original, single_line, &mut pending)
                        {
                            return Ok(nav);
                        }
                        confirm_index = 2;
                    }
                    KeyCode::Char('a') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        cursor = chars[..cursor]
                            .iter()
                            .rposition(|c| *c == '\n')
                            .map(|p| p + 1)
                            .unwrap_or(0)
                    }
                    KeyCode::End => {
                        cursor += chars[cursor..]
                            .iter()
                            .position(|c| *c == '\n')
                            .unwrap_or(chars.len() - cursor)
                    }
                    KeyCode::Char('e') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        cursor += chars[cursor..]
                            .iter()
                            .position(|c| *c == '\n')
                            .unwrap_or(chars.len() - cursor)
                    }
                    KeyCode::Left => cursor = cursor.saturating_sub(1),
                    KeyCode::Right => cursor = (cursor + 1).min(chars.len()),
                    KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                        let distance = if matches!(k.code, KeyCode::PageUp | KeyCode::PageDown) {
                            height
                        } else {
                            1
                        };
                        let target = if matches!(k.code, KeyCode::Up | KeyCode::PageUp) {
                            row.saturating_sub(distance)
                        } else {
                            (row + distance).min(lines.len() - 1)
                        };
                        if let Some((index, _)) = positions
                            .iter()
                            .enumerate()
                            .filter(|(_, p)| p.0 == target)
                            .min_by_key(|(_, p)| p.1.abs_diff(col))
                        {
                            cursor = index;
                        }
                    }
                    KeyCode::Backspace if cursor > 0 => {
                        cursor -= 1;
                        chars.remove(cursor);
                        message = None;
                    }
                    KeyCode::Delete if cursor < chars.len() => {
                        chars.remove(cursor);
                        message = None;
                    }
                    KeyCode::Enter => {
                        chars.insert(cursor, '\n');
                        cursor += 1;
                        message = None;
                    }
                    KeyCode::Tab => {
                        chars.splice(cursor..cursor, [' ', ' ', ' ', ' ']);
                        cursor += 4;
                        message = None;
                    }
                    KeyCode::Char(c)
                        if !k
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        chars.insert(cursor, c);
                        cursor += 1;
                        message = None;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
fn layout(chars: &[char], width: usize, secret: bool) -> (Vec<String>, Vec<(usize, usize)>) {
    let mut rows = vec![String::new()];
    let mut positions = Vec::with_capacity(chars.len() + 1);
    let mut col = 0;
    let mut just_wrapped = false;
    for c in chars {
        let display = if secret && *c != '\n' {
            '*'
        } else if *c == '\t' {
            ' '
        } else {
            *c
        };
        let w = display.width().unwrap_or(0);
        if *c != '\n' && col + w > width && !rows.last().unwrap().is_empty() {
            rows.push(String::new());
            col = 0;
        }
        positions.push((rows.len() - 1, col));
        if *c == '\n' {
            if !just_wrapped {
                rows.push(String::new());
            }
            col = 0;
            just_wrapped = false;
            continue;
        }
        rows.last_mut().unwrap().push(display);
        col += w;
        just_wrapped = false;
        if col >= width {
            rows.push(String::new());
            col = 0;
            just_wrapped = true;
        }
    }
    positions.push((rows.len() - 1, col));
    (rows, positions)
}
fn external_edit(data: &Path, initial: &str) -> Result<Option<String>> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .map_err(|_| "请设置 VISUAL 或 EDITOR；也可以使用内置编辑器".to_string())?;
    let dir = data.join("edit-recovery");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.md", uuid::Uuid::new_v4()));
    core::atomic_write(&path, initial.as_bytes())?;
    // Prefer a literal executable path (including Windows spaces), otherwise parse quoted arguments without invoking a shell.
    let parts = if Path::new(&editor).is_file() {
        vec![editor]
    } else {
        shell_words::split(&editor).map_err(|e| e.to_string())?
    };
    let Some(executable) = parts.first() else {
        return Err("编辑器命令为空".into());
    };
    let result = process::Command::new(executable)
        .args(&parts[1..])
        .arg(&path)
        .status()
        .map_err(|e| format!("{e}；编辑稿位于 {}", path.display()))?;
    if !result.success() {
        return Err(format!("编辑器未成功退出；编辑稿保留于 {}", path.display()));
    }
    let edited = read(&path)?;
    let _ = fs::remove_file(path);
    Ok(if edited == initial {
        None
    } else {
        Some(edited)
    })
}

fn plain_editor<F: FnMut(&str) -> Result<()>>(
    title: &str,
    original: &str,
    initial: &str,
    single_line: bool,
    secret: bool,
    error: Option<String>,
    mut save: F,
) -> Result<Nav> {
    if secret {
        return match secret_input(title)? {
            Choice::Selected(text) => {
                save(&text)?;
                Ok(Nav::Back)
            }
            Choice::Back => Ok(Nav::Back),
            Choice::Home => Ok(Nav::Home),
        };
    }
    if single_line {
        loop {
            let text = plain_line(&format!("{title}（.back 返回，.home 主页）"), initial)?;
            match text.as_str() {
                ".back" => return Ok(Nav::Back),
                ".home" => return Ok(Nav::Home),
                _ => match save(&text) {
                    Ok(()) => return Ok(Nav::Back),
                    Err(error) => eprintln!("保存失败：{error}"),
                },
            }
        }
    }
    println!("\n{title}\n{initial}\n逐行输入；.save 保存，.back 返回上一级，.home 主页，首行 .append 追加。导航前可选择保存、放弃、继续。");
    if let Some(error) = error {
        eprintln!("保存失败：{error}");
    }
    let mut text = initial.to_owned();
    let mut first = true;
    loop {
        let mut input = String::new();
        if io::stdin()
            .read_line(&mut input)
            .map_err(|e| e.to_string())?
            == 0
        {
            return Ok(Nav::Back);
        }
        let command = input.trim_end_matches(['\r', '\n']);
        if command == ".save" {
            match save(&text) {
                Ok(()) => return Ok(Nav::Back),
                Err(error) => eprintln!("保存失败：{error}"),
            };
            continue;
        }
        if matches!(command, ".back" | ".cancel" | ".home") {
            let target = if command == ".home" {
                Nav::Home
            } else {
                Nav::Back
            };
            if text == original {
                return Ok(target);
            }
            match plain_line("未保存：S 保存后离开 / D 放弃 / C 继续", "C")?
                .to_ascii_lowercase()
                .as_str()
            {
                "s" => match save(&text) {
                    Ok(()) => return Ok(target),
                    Err(error) => eprintln!("保存失败：{error}"),
                },
                "d" => return Ok(target),
                _ => {}
            }
            continue;
        }
        if first && command == ".append" {
            text = initial.into();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
        } else {
            if first {
                text.clear();
            }
            text.push_str(&input.replace("\r\n", "\n"));
        }
        first = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn template_field_edits_preserve_other_tasks_and_reject_conflicts_and_invalid_variables() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Context::new(dir.path().join("data"), Some(dir.path().join("book"))).unwrap();
        core::project_manager::open_project(ctx.clone(), ctx.project_dir.display().to_string(), true, None, None).unwrap();
        let before = core::get_prompt_settings(ctx.clone()).unwrap();
        save_template_field(&ctx, PromptCategory::Compact, true, &before.compact_generation.system, "精简系统").unwrap();
        save_template_field(&ctx, PromptCategory::Consistency, true, &before.consistency.system, "检查系统").unwrap();
        let edited = core::get_prompt_settings(ctx.clone()).unwrap();
        assert_eq!(edited.compact_generation.system, "精简系统");
        assert_eq!(edited.consistency.system, "检查系统");
        assert_eq!(edited.system, before.system);
        assert_eq!(edited.memory, before.memory);
        assert_eq!(edited.generation_preset, before.generation_preset);
        assert!(save_template_field(&ctx, PromptCategory::Compact, true, &before.compact_generation.system, "旧会话修改").is_err());
        assert!(save_template_field(&ctx, PromptCategory::Compact, false, &before.compact_generation.user_template, "缺少骨架").is_err());
        assert_eq!(core::get_prompt_settings(ctx).unwrap(), edited);
    }

    #[test]
    fn adopted_messages_are_exact_and_changes_require_a_new_preview() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Context::new(dir.path().join("data"), Some(dir.path().join("book"))).unwrap();
        core::project_manager::open_project(ctx.clone(), ctx.project_dir.display().to_string(), true, None, None).unwrap();
        let mut draft = GenerationDraft { idea: "进入山谷".into(), use_context: false, ..GenerationDraft::default() };
        let mut preview = crate::preview_generation(ctx.clone(), draft.generation("ch001", false).unwrap()).unwrap();
        preview.prompts.system = "作者手动的系统要求".into();
        preview.prompts.user = "只写山谷入口。不要追加任何标记。".into();
        validate_preview(&ctx, "ch001", &draft, &preview).unwrap();
        draft.adopt(preview.clone());
        let generated = draft.generation("ch001", true).unwrap();
        let exact = crate::preview_generation(ctx.clone(), generated.clone()).unwrap();
        assert_eq!(exact.prompts, preview.prompts);
        let encoded = serde_json::to_value(crate::jobs::Action::Generate(generated)).unwrap();
        assert_eq!(encoded["prompt_override"]["prompts"]["user"], preview.prompts.user);
        draft.overwrite = true;
        assert!(draft.generation("ch001", true).is_ok());
        draft.idea.push_str("变化");
        assert!(draft.generation("ch001", true).err().unwrap().contains("预览已过期"));
        draft.idea = "进入山谷".into();
        let mut settings = core::get_prompt_settings(ctx.clone()).unwrap();
        settings.generation_preset = core::GenerationPreset::Compact;
        core::save_prompt_settings(ctx.clone(), settings).unwrap();
        assert!(crate::preview_generation(ctx.clone(), draft.generation("ch001", true).unwrap()).is_err());
        // Reset keeps the chapter input and options, but clears local/imported prompt overrides.
        draft.prompt = "missing-import.json".into();
        draft.automatic();
        let reset = draft.generation("ch001", true).unwrap();
        assert!(reset.prompt_override.is_none());
        assert_eq!(reset.idea, "进入山谷");
        assert!(crate::preview_generation(ctx, reset).is_ok());
    }

    #[test]
    fn back_preserves_parent_selection_and_home_keeps_root() {
        let mut nav = Navigation::new();
        nav.frames[0].list.select(Some(1));
        nav.push(Page::Chapters(ChapterPurpose::Workbench));
        nav.frames[1].list.select(Some(12));
        *nav.frames[1].list.offset_mut() = 6;
        nav.push(Page::Chapter("ch013".into()));
        nav.go(Nav::Back);
        assert_eq!(nav.page(), &Page::Chapters(ChapterPurpose::Workbench));
        assert_eq!(nav.frames[1].list.selected(), Some(12));
        assert_eq!(nav.frames[1].list.offset(), 6);
        nav.push(Page::Chapter("ch013".into()));
        nav.go(Nav::Home);
        assert_eq!(nav.frames.len(), 1);
        assert_eq!(nav.frames[0].list.selected(), Some(1));
        nav.go(Nav::Back);
        assert_eq!(nav.page(), &Page::Home);
    }
    #[test]
    fn dirty_editor_requires_confirmation_and_retains_destination() {
        let mut pending = None;
        assert_eq!(leave_request(Nav::Home, true, false, &mut pending), None);
        assert_eq!(pending, Some(Nav::Home));
        pending = None;
        assert_eq!(leave_request(Nav::Back, true, false, &mut pending), None);
        assert_eq!(pending, Some(Nav::Back));
        assert_eq!(
            leave_request(Nav::Home, false, false, &mut pending),
            Some(Nav::Home)
        );
    }
}

fn secret_input(title: &str) -> Result<Choice<String>> {
    struct RawGuard;
    impl Drop for RawGuard {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
            println!();
        }
    }
    print!("{title} · Enter 确认 / Esc 返回 / Home 主页：");
    io::stdout().flush().map_err(|e| e.to_string())?;
    terminal::enable_raw_mode().map_err(|e| e.to_string())?;
    let _guard = RawGuard;
    let mut text = String::new();
    loop {
        match next_event()? {
            Event::Paste(value) => text.push_str(value.trim_end_matches(['\r', '\n'])),
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Enter => return Ok(Choice::Selected(text)),
                KeyCode::Esc => return Ok(Choice::Back),
                KeyCode::Home => return Ok(Choice::Home),
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(Choice::Back)
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    text.push(c)
                }
                _ => {}
            },
            _ => {}
        }
    }
}
