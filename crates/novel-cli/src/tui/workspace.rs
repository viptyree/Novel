//! Presentation actions are independent of menu positions. The adapters below
//! reuse the existing checked transactions and worker submission paths.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Advanced {
    Chapter(String),
    Generation { id: String, preview: bool },
    Prompts,
    Settings,
}
impl Advanced {
    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Chapter(_) => "危险操作",
            Self::Generation { .. } => "高级生成选项",
            Self::Prompts => "提示词与资料 · 高级",
            Self::Settings => "采样与输出参数",
        }
    }
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) enum Focus {
    Global,
    Directory,
    Body,
    #[default]
    Actions,
}
#[derive(Default)]
pub(super) struct ViewState {
    pub(super) focus: Focus,
    pub(super) query: String,
    pub(super) offset: u16,
    pub(super) directory: ListState,
    pub(super) visited: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Module {
    Home,
    Chapters,
    Projects,
    Memory,
    Prompts,
    Settings,
    Reports,
    Jobs,
}
pub(super) const MODULES: [Module; 8] = [
    Module::Home,
    Module::Chapters,
    Module::Projects,
    Module::Memory,
    Module::Prompts,
    Module::Settings,
    Module::Reports,
    Module::Jobs,
];
impl Module {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Home => "主页",
            Self::Chapters => "章节工作台",
            Self::Projects => "项目管理",
            Self::Memory => "记忆与大纲",
            Self::Prompts => "提示词与上下文",
            Self::Settings => "模型与设置",
            Self::Reports => "报告",
            Self::Jobs => "任务中心",
        }
    }
    pub(super) fn page(self) -> Page {
        match self {
            Self::Home => Page::Home,
            Self::Chapters => Page::Chapters(ChapterPurpose::Workbench),
            Self::Projects => Page::RecentProjects,
            Self::Memory => Page::Memories,
            Self::Prompts => Page::Prompts,
            Self::Settings => Page::Settings,
            Self::Reports => Page::Chapters(ChapterPurpose::Reports),
            Self::Jobs => Page::Jobs,
        }
    }
}
fn module(page: &Page) -> Module {
    match page {
        Page::Home => Module::Home,
        Page::Projects | Page::RecentProjects | Page::Project(_) => Module::Projects,
        Page::Memories | Page::Memory(_) => Module::Memory,
        Page::Settings | Page::Advanced(Advanced::Settings) => Module::Settings,
        Page::Prompts
        | Page::PromptCategories
        | Page::PromptTemplate(_)
        | Page::Advanced(Advanced::Prompts) => Module::Prompts,
        Page::Report(_) | Page::Chapters(ChapterPurpose::Reports) => Module::Reports,
        Page::Jobs | Page::Job(_) => Module::Jobs,
        _ => Module::Chapters,
    }
}

macro_rules! operations {
    ($name:ident { $($variant:ident = $index:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum $name { $($variant),+ }
        impl $name { fn index(self) -> usize { match self { $(Self::$variant => $index),+ } } }
    };
}
operations!(ChapterOp { Read=0, Edit=1, External=2, Create=3, Generate=4, Check=5, Memory=6, Titles=7, Rename=8, Reset=9 });
operations!(GenerateOp { Idea=0, Length=1, Context=2, Overwrite=3, Pov=4, Narrative=5, Options=6, Import=7, Preview=8, Submit=9 });
operations!(ManualOp { User=0, System=1, Read=2, Overwrite=3, Budget=4, Submit=5, Clear=6 });
operations!(PromptOp { Read=0, External=2, Import=3, Reset=4, Preview=5, Context=6, Source=7, SetSource=8, Preset=9 });
operations!(Setting { Provider=1, Url=2, Model=3, Key=4, Temperature=5, TopP=6, Tokens=7, Thinking=8 });
operations!(MemoryOp { Read=0, Edit=1, External=2, Import=3 });
operations!(TemplateOp { System=0, User=1, Read=2, Reset=3, Guidance=4 });
operations!(PreviewOp { Read=0, System=1, User=2, Adopt=3, Automatic=4 });

#[derive(Clone, Debug)]
pub(super) enum Action {
    Navigate(Page),
    Module(Module),
    Back,
    Home,
    Exit,
    Doctor,
    NewChapter(ChapterPurpose),
    OpenChapter(String, ChapterPurpose),
    Project(String, ProjectOp),
    Chapter(ChapterOp),
    Generate(GenerateOp),
    Scheme,
    Automatic,
    Manual(ManualOp),
    Prompt(PromptOp),
    Setting(Setting),
    Memory(MemoryOp),
    Template(TemplateOp),
    Preview(PreviewOp),
    Report(&'static str),
    Mode(bool),
    ToggleJobs,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum ProjectOp {
    Open,
    Pin,
    Unpin,
    Forget,
    Relocate,
}
#[derive(Clone)]
pub(super) struct Item {
    pub(super) label: String,
    pub(super) group: &'static str,
    pub(super) action: Action,
}
fn item(label: impl Into<String>, group: &'static str, action: Action) -> Item {
    Item {
        label: label.into(),
        group,
        action,
    }
}
pub(super) struct Model {
    pub(super) path: String,
    pub(super) project: String,
    pub(super) note: String,
    pub(super) module: Module,
    pub(super) items: Vec<Item>,
    pub(super) chapters: Vec<core::chapter_titles::ChapterDetails>,
    pub(super) selected_chapter: Option<String>,
    pub(super) text: String,
    pub(super) title: String,
    pub(super) searchable: bool,
    pub(super) chapter_list: bool,
}
fn base_page(page: &Page) -> Page {
    match page {
        Page::Advanced(Advanced::Chapter(id)) => Page::Chapter(id.clone()),
        Page::Advanced(Advanced::Generation { id, preview }) => Page::Generate {
            id: id.clone(),
            preview: *preview,
        },
        Page::Advanced(Advanced::Prompts) => Page::Prompts,
        Page::Advanced(Advanced::Settings) => Page::Settings,
        other => other.clone(),
    }
}

pub(super) fn model(app: &mut App) -> Result<Model> {
    let page = app.nav.page().clone();
    let mut m = Model {
        path: app.nav.path(),
        project: app
            .ctx
            .project_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        note: String::new(),
        module: module(&page),
        items: vec![],
        chapters: vec![],
        selected_chapter: None,
        text: String::new(),
        title: page.label(),
        searchable: false,
        chapter_list: false,
    };
    match &page {
        Page::Home => {
            let (chapters, chapter_error) =
                match core::chapter_titles::list_chapter_details(app.ctx.clone()) {
                    Ok(chapters) => (chapters, None),
                    Err(error) => (vec![], Some(error)),
                };
            let jobs = crate::jobs::list(&app.ctx.data_dir)?;
            let active = jobs
                .as_array()
                .map(|j| {
                    j.iter()
                        .filter(|j| {
                            matches!(j["state"].as_str(), Some("starting" | "running"))
                                && j["project"]
                                    .as_str()
                                    .map(|p| {
                                        core::project_manager::same_path(
                                            Path::new(p),
                                            &app.ctx.project_dir,
                                        )
                                    })
                                    .unwrap_or(false)
                        })
                        .count()
                })
                .unwrap_or(0);
            m.note = format!("{} 章 · {} 个运行中任务", chapters.len(), active);
            if let Some(error) = chapter_error {
                m.note = format!("项目资料不可读：{error}；请切换或重新定位项目");
            }
            m.title = "让故事继续".into();
            m.text = "先选择章节，阅读已有正文，或从一个新想法开始。\n\n生成 → 阅读与修改 → 一致性检查 → 更新记忆\n\n本章生成草稿保留至退出 CLI；模型任务在后台运行，离开页面或断开 SSH 不会取消。\n\nF1 随时打开模块导航。".into();
            if let Some((project, id)) = &app.last_chapter {
                if core::project_manager::same_path(project, &app.ctx.project_dir) {
                    m.items.push(item(
                        format!("继续当前章节 · {id}"),
                        "写作",
                        Action::Navigate(Page::Chapter(id.clone())),
                    ));
                }
            }
            m.items.extend([
                item("进入章节工作台", "写作", Action::Module(Module::Chapters)),
                item(
                    "新建章节",
                    "写作",
                    Action::NewChapter(ChapterPurpose::Workbench),
                ),
                item(
                    "最近项目 / 切换项目",
                    "项目",
                    Action::Module(Module::Projects),
                ),
                item("查看后台任务", "项目", Action::Module(Module::Jobs)),
                item("环境检查", "工具", Action::Doctor),
                item("退出程序", "工具", Action::Exit),
            ]);
        }
        Page::Projects => {
            m.items = vec![
                item(
                    "新建项目",
                    "项目",
                    Action::Project(String::new(), ProjectOp::Open),
                ),
                item(
                    "按路径打开已有项目",
                    "项目",
                    Action::Project(String::new(), ProjectOp::Relocate),
                ),
            ];
        }
        Page::RecentProjects => {
            m.searchable = true;
            for p in core::project_manager::list_projects(app.ctx.clone())? {
                let current = core::project_manager::same_path(
                    Path::new(&p.entry.path),
                    &app.ctx.project_dir,
                );
                m.items.push(item(
                    format!(
                        "{}{}{} · {}",
                        if current { "[当前] " } else { "" },
                        if p.exists { "" } else { "[缺失] " },
                        p.entry.name,
                        p.entry.path
                    ),
                    "最近项目",
                    Action::Navigate(Page::Project(p.entry.path)),
                ));
            }
            m.items.push(item(
                "新建 / 按路径打开",
                "项目",
                Action::Navigate(Page::Projects),
            ));
        }
        Page::Project(path) => {
            let entry = core::project_manager::list_projects(app.ctx.clone())?
                .into_iter()
                .find(|p| p.entry.path == *path)
                .ok_or("项目记录已变更，请返回刷新")?;
            m.title = entry.entry.name;
            m.text = format!(
                "项目目录\n{path}\n\n{}\n{}",
                if entry.exists {
                    "目录可用"
                } else {
                    "目录不存在，请重新定位"
                },
                if core::project_manager::same_path(Path::new(path), &app.ctx.project_dir) {
                    "这是当前项目"
                } else {
                    "打开后将切换当前项目"
                }
            );
            if entry.exists {
                m.items.push(item(
                    "打开项目",
                    "项目",
                    Action::Project(path.clone(), ProjectOp::Open),
                ));
            }
            m.items.push(item(
                if entry.entry.pinned {
                    "取消置顶"
                } else {
                    "置顶项目"
                },
                "管理",
                Action::Project(
                    path.clone(),
                    if entry.entry.pinned {
                        ProjectOp::Unpin
                    } else {
                        ProjectOp::Pin
                    },
                ),
            ));
            m.items.push(item(
                "重新定位目录",
                "管理",
                Action::Project(path.clone(), ProjectOp::Relocate),
            ));
            m.items.push(item(
                "从列表移除（保留文件）",
                "危险操作",
                Action::Project(path.clone(), ProjectOp::Forget),
            ));
        }
        Page::Chapters(purpose) => {
            m.chapters = core::chapter_titles::list_chapter_details(app.ctx.clone())?;
            m.searchable = true;
            m.chapter_list = true;
            for c in &m.chapters {
                m.items.push(item(
                    format!(
                        "{} · {}",
                        c.id,
                        if c.title.is_empty() {
                            "未命名章节"
                        } else {
                            &c.title
                        }
                    ),
                    "章节",
                    Action::OpenChapter(c.id.clone(), purpose.clone()),
                ));
            }
            m.items.push(item(
                "输入章号 / 新章节",
                "写作",
                Action::NewChapter(purpose.clone()),
            ));
            m.note = format!("{} 章 · / 搜索章号或标题", m.chapters.len());
        }
        Page::Chapter(id) => {
            m.chapters = core::chapter_titles::list_chapter_details(app.ctx.clone())?;
            m.selected_chapter = Some(id.clone());
            let title = m
                .chapters
                .iter()
                .find(|c| c.id == *id)
                .map(|c| c.title.as_str())
                .unwrap_or("");
            m.title = format!("{id} · {}", if title.is_empty() { "正文" } else { title });
            let text = core::read_chapter(app.ctx.clone(), id.clone());
            match text {
                Ok(text) => {
                    m.note = format!(
                        "{} 字 · {}",
                        text.chars().count(),
                        if core::project_source::chapter_path(&app.ctx.project_dir, id)?.is_file() {
                            "已保存文件"
                        } else {
                            "尚无正文文件"
                        }
                    );
                    m.text = if text.trim().is_empty() {
                        "本章尚无正文。选择 AI 生成，或进入编辑正文。".into()
                    } else {
                        text
                    };
                }
                Err(error) => {
                    m.note = "读取失败".into();
                    m.text = format!("正文暂不可读：{error}");
                }
            }
            m.items = vec![
                item("编辑正文", "写作", Action::Chapter(ChapterOp::Edit)),
                item("全屏阅读正文", "写作", Action::Chapter(ChapterOp::Read)),
                item(
                    "AI 生成 / 重新生成",
                    "AI 辅助",
                    Action::Chapter(ChapterOp::Generate),
                ),
                item("编辑章名", "写作", Action::Chapter(ChapterOp::Rename)),
                item(
                    "生成三个章名",
                    "AI 辅助",
                    Action::Chapter(ChapterOp::Titles),
                ),
                item(
                    "一致性检查",
                    "检查与记忆",
                    Action::Chapter(ChapterOp::Check),
                ),
                item(
                    "更新长期记忆",
                    "检查与记忆",
                    Action::Chapter(ChapterOp::Memory),
                ),
                item(
                    "查看本章报告",
                    "检查与记忆",
                    Action::Navigate(Page::Report(id.clone())),
                ),
                item("外部编辑器", "写作", Action::Chapter(ChapterOp::External)),
                item("新建空章", "写作", Action::Chapter(ChapterOp::Create)),
                item(
                    "重置章节…",
                    "危险操作",
                    Action::Navigate(Page::Advanced(Advanced::Chapter(id.clone()))),
                ),
            ];
        }
        Page::Memories => {
            m.searchable = true;
            for file in core::EDITABLE_FILES {
                let (name, group) = memory_label(file);
                m.items.push(item(
                    format!("{name} · {file}"),
                    group,
                    Action::Navigate(Page::Memory(file.to_string())),
                ));
            }
            m.items.sort_by_key(|i| match i.group {
                "作者资料" => 0,
                "故事设定" => 1,
                _ => 2,
            });
            m.note = "作者备注不发送模型；资料仅按生成模板及上下文选择发送".into();
        }
        Page::Memory(file) => {
            m.title = memory_label(file).0.into();
            m.text = core::read_memory_file(app.ctx.clone(), file.clone())?;
            m.note = file.clone();
            m.items = vec![
                item("内置编辑器", "编辑", Action::Memory(MemoryOp::Edit)),
                item("全屏查看", "阅读", Action::Memory(MemoryOp::Read)),
                item("外部编辑器", "高级", Action::Memory(MemoryOp::External)),
                item("从文件导入", "高级", Action::Memory(MemoryOp::Import)),
            ];
        }
        Page::Prompts => {
            let p = core::get_prompt_settings(app.ctx.clone())?;
            m.note = format!(
                "项目默认：{} · 模板修改持续保存，本章预览独立",
                preset_label(p.generation_preset)
            );
            m.items = vec![
                item(
                    "生成提示词 · 选择预设",
                    "生成",
                    Action::Navigate(Page::PromptCategories),
                ),
                item(
                    "一致性检查模板",
                    "检查与记忆",
                    Action::Navigate(Page::PromptTemplate(PromptCategory::Consistency)),
                ),
                item(
                    "记忆更新模板",
                    "检查与记忆",
                    Action::Navigate(Page::PromptTemplate(PromptCategory::Memory)),
                ),
                item("项目默认生成方案", "项目", Action::Prompt(PromptOp::Preset)),
                item("预览章节提示词", "本章", Action::Prompt(PromptOp::Preview)),
                item(
                    "高级：导入、正文来源与上下文",
                    "高级",
                    Action::Navigate(Page::Advanced(Advanced::Prompts)),
                ),
            ];
        }
        Page::PromptCategories => {
            for category in [
                PromptCategory::General,
                PromptCategory::Compact,
                PromptCategory::Benchmark,
            ] {
                m.items.push(item(
                    category.label(),
                    "生成方案",
                    Action::Navigate(Page::PromptTemplate(category)),
                ));
            }
        }
        Page::PromptTemplate(category) => {
            let p = core::get_prompt_settings(app.ctx.clone())?;
            let task = category.task(&p);
            m.note = "项目永久模板 · 不修改本章临时预览".into();
            m.text = format!(
                "系统提示词\n{}\n\n任务提示词\n{}",
                task.system, task.user_template
            );
            m.items = vec![
                item(
                    "编辑系统提示词",
                    "编辑",
                    Action::Template(TemplateOp::System),
                ),
                item(
                    "编辑任务提示词模板",
                    "编辑",
                    Action::Template(TemplateOp::User),
                ),
                item("模板与变量说明", "说明", Action::Template(TemplateOp::Read)),
            ];
            if let Some(enabled) = category.guidance(&p) {
                m.items.push(item(
                    format!(
                        "追加资料使用原则：{}",
                        if enabled { "开启" } else { "关闭" }
                    ),
                    "方案",
                    Action::Template(TemplateOp::Guidance),
                ));
            }
            m.items.push(item(
                "恢复当前分类默认模板",
                "危险操作",
                Action::Template(TemplateOp::Reset),
            ));
        }
        Page::Generate { id, preview } => {
            let d = app
                .drafts
                .entry((app.ctx.project_dir.clone(), id.clone()))
                .or_default();
            m.text = d.idea.clone();
            m.title = format!("{id} · 事件骨架");
            m.note = "本章草稿 · 未提交前不会调用模型".into();
            m.items = vec![
                item("生成模式：模板（切换为手动）", "模式", Action::Mode(true)),
                item(
                    format!("编辑事件骨架（{} 字）", d.idea.chars().count()),
                    "构思",
                    Action::Generate(GenerateOp::Idea),
                ),
                item(
                    format!("目标字数：{}", d.target),
                    "基础",
                    Action::Generate(GenerateOp::Length),
                ),
                item(
                    format!("覆盖已有正文：{}", if d.overwrite { "是" } else { "否" }),
                    "基础",
                    Action::Generate(GenerateOp::Overwrite),
                ),
                item(
                    "提示词预览（不调用模型）",
                    "生成",
                    Action::Generate(GenerateOp::Preview),
                ),
            ];
            if !preview {
                m.items.push(item(
                    "提交后台生成",
                    "生成",
                    Action::Generate(GenerateOp::Submit),
                ));
            }
            m.items.push(item(
                "高级：上下文、预设与叙事",
                "高级",
                Action::Navigate(Page::Advanced(Advanced::Generation {
                    id: id.clone(),
                    preview: *preview,
                })),
            ));
            let status = if d.confirmed.is_some() {
                if d.confirmed_input == d.input_key() {
                    "已采用本章预览"
                } else {
                    "本章预览已过期"
                }
            } else if d.prompt.is_empty() {
                "当前自动"
            } else {
                "使用导入文件"
            };
            m.note.push_str(&format!(" · {status}"));
        }
        Page::ManualGenerate(id) => {
            let d = app
                .manual_drafts
                .entry((app.ctx.project_dir.clone(), id.clone()))
                .or_default();
            m.title = format!("{id} · 手动完整提示词");
            m.text = d.prompts.user.clone();
            let stats = core::preview_manual_prompt(d.prompts.clone(), d.input_budget);
            m.note = match stats {
                Ok(_) => format!(
                    "{} 字 · 预算 {} tokens · 原文发送",
                    d.prompts.user.chars().count(),
                    d.input_budget
                ),
                Err(e) => format!("尚不能提交：{e}"),
            };
            m.items = vec![
                item("生成模式：手动（切换为模板）", "模式", Action::Mode(false)),
                item(
                    "编辑完整章节提示词",
                    "提示词",
                    Action::Manual(ManualOp::User),
                ),
                item(
                    "编辑系统提示词（可留空）",
                    "提示词",
                    Action::Manual(ManualOp::System),
                ),
                item(
                    format!("输入预算：{} tokens", d.input_budget),
                    "基础",
                    Action::Manual(ManualOp::Budget),
                ),
                item(
                    format!("覆盖已有正文：{}", if d.overwrite { "是" } else { "否" }),
                    "基础",
                    Action::Manual(ManualOp::Overwrite),
                ),
                item("查看完整提示词", "预览", Action::Manual(ManualOp::Read)),
                item("提交后台生成", "生成", Action::Manual(ManualOp::Submit)),
                item(
                    "清空本章手动草稿",
                    "危险操作",
                    Action::Manual(ManualOp::Clear),
                ),
            ];
        }
        Page::PromptPreview(id) => {
            m.note = "只用于本章 · 不修改项目模板 · 不调用模型".into();
            if let Some(preview) = app
                .drafts
                .get(&(app.ctx.project_dir.clone(), id.clone()))
                .and_then(|d| d.preview.as_ref())
            {
                m.text = format_prompt_preview(
                    serde_json::to_value(preview).map_err(|e| e.to_string())?,
                )?;
            }
            m.items = vec![
                item("查看完整提示词", "预览", Action::Preview(PreviewOp::Read)),
                item(
                    "编辑系统提示词（保存即采用）",
                    "本章",
                    Action::Preview(PreviewOp::System),
                ),
                item(
                    "编辑章节提示词（保存即采用）",
                    "本章",
                    Action::Preview(PreviewOp::User),
                ),
                item(
                    "采用当前预览用于本章",
                    "本章",
                    Action::Preview(PreviewOp::Adopt),
                ),
                item(
                    "恢复自动组装",
                    "本章",
                    Action::Preview(PreviewOp::Automatic),
                ),
            ];
        }
        Page::Settings => {
            let s = Context::read_settings(&app.ctx.data_dir)?;
            let provider = s.llm_provider.as_deref().unwrap_or("api");
            m.note = "连接方式决定适用设置；密钥不会回显".into();
            m.items.push(item(
                format!("模型连接方式：{provider}"),
                "连接",
                Action::Setting(Setting::Provider),
            ));
            if provider == "api" {
                m.items.extend([
                    item(
                        format!("API 地址：{}", s.llm_base_url),
                        "连接",
                        Action::Setting(Setting::Url),
                    ),
                    item("API 密钥：隐藏", "连接", Action::Setting(Setting::Key)),
                ]);
            }
            m.items.push(item(
                format!("模型名称：{}", s.llm_model),
                "连接",
                Action::Setting(Setting::Model),
            ));
            if provider == "api" {
                m.items.push(item(
                    format!("关闭模型思考：{}", s.disable_thinking.unwrap_or(true)),
                    "生成",
                    Action::Setting(Setting::Thinking),
                ));
                m.items.push(item(
                    "采样与输出参数…",
                    "高级",
                    Action::Navigate(Page::Advanced(Advanced::Settings)),
                ));
            }
            m.text=format!("当前项目\n{}\n\nAPI：使用兼容接口。\nClaude Code / Codex：使用主机已登录的订阅 CLI。\n\nAPI 密钥仅在 API 模式下使用。",app.ctx.project_dir.display());
        }
        Page::Report(id) => {
            m.title = format!("{id} · 报告");
            m.note = "选择报告；检查和记忆更新需要单独执行".into();
            m.items = vec![
                item("生成报告", "报告", Action::Report("generation")),
                item("一致性报告", "报告", Action::Report("consistency")),
                item("记忆更新报告", "报告", Action::Report("memory")),
            ];
        }
        Page::Jobs => {
            m.searchable = true;
            let jobs = crate::jobs::list(&app.ctx.data_dir)?;
            for j in jobs.as_array().ok_or("任务列表格式错误")? {
                if !app.all_jobs
                    && !j["project"]
                        .as_str()
                        .map(|p| {
                            core::project_manager::same_path(Path::new(p), &app.ctx.project_dir)
                        })
                        .unwrap_or(false)
                {
                    continue;
                }
                if let Some(id) = j["id"].as_str() {
                    m.items.push(item(
                        format!(
                            "{} · {} · {} · {}",
                            state_label(j["state"].as_str().unwrap_or("")),
                            j["project"]
                                .as_str()
                                .and_then(|p| Path::new(p).file_name())
                                .unwrap_or_default()
                                .to_string_lossy(),
                            j["stage"].as_str().unwrap_or(""),
                            &id[..id.len().min(8)]
                        ),
                        "任务",
                        Action::Navigate(Page::Job(id.into())),
                    ));
                }
            }
            m.note = if app.all_jobs {
                "全部项目任务"
            } else {
                "当前项目任务"
            }
            .into();
            m.items.push(item(
                if app.all_jobs {
                    "仅显示当前项目"
                } else {
                    "显示全部项目"
                },
                "筛选",
                Action::ToggleJobs,
            ));
        }
        Page::Advanced(kind) => match kind {
            Advanced::Chapter(_) => {
                m.note = "重置会备份正文，执行前仍需确认".into();
                m.items.push(item(
                    "备份并重置本章正文和报告",
                    "危险操作",
                    Action::Chapter(ChapterOp::Reset),
                ));
            }
            Advanced::Prompts => {
                m.items = vec![
                    item("查看完整提示词配置", "只读", Action::Prompt(PromptOp::Read)),
                    item(
                        "外部编辑完整 JSON",
                        "导入与编辑",
                        Action::Prompt(PromptOp::External),
                    ),
                    item(
                        "从 JSON 导入提示词",
                        "导入与编辑",
                        Action::Prompt(PromptOp::Import),
                    ),
                    item("查看正文来源", "资料", Action::Prompt(PromptOp::Source)),
                    item("设置正文来源", "资料", Action::Prompt(PromptOp::SetSource)),
                    item("上下文目录", "资料", Action::Prompt(PromptOp::Context)),
                    item(
                        "恢复全部默认提示词",
                        "危险操作",
                        Action::Prompt(PromptOp::Reset),
                    ),
                ]
            }
            Advanced::Settings => {
                let s = Context::read_settings(&app.ctx.data_dir)?;
                m.items = vec![
                    item(
                        format!(
                            "温度 temperature：{}",
                            s.temperature
                                .map(|v| v.to_string())
                                .unwrap_or("默认".into())
                        ),
                        "采样",
                        Action::Setting(Setting::Temperature),
                    ),
                    item(
                        format!(
                            "核采样 top_p：{}",
                            s.top_p.map(|v| v.to_string()).unwrap_or("默认".into())
                        ),
                        "采样",
                        Action::Setting(Setting::TopP),
                    ),
                    item(
                        format!(
                            "输出上限 max_output_tokens：{}",
                            s.max_output_tokens
                                .map(|v| v.to_string())
                                .unwrap_or("默认".into())
                        ),
                        "输出",
                        Action::Setting(Setting::Tokens),
                    ),
                ];
                m.note = "输入 null 恢复配置默认；采样和输出参数保留现有校验".into();
            }
            Advanced::Generation { id, .. } => {
                let d = app
                    .drafts
                    .entry((app.ctx.project_dir.clone(), id.clone()))
                    .or_default();
                let options: core::context::Options =
                    serde_json::from_str(&d.options).map_err(|e| e.to_string())?;
                m.items = vec![
                    item(
                        format!("长期记忆：{}", if d.use_context { "是" } else { "否" }),
                        "上下文",
                        Action::Generate(GenerateOp::Context),
                    ),
                    item(
                        format!(
                            "本章生成方案：{}",
                            options
                                .generation_preset
                                .map(preset_label)
                                .unwrap_or("跟随项目默认")
                        ),
                        "方案",
                        Action::Scheme,
                    ),
                    item(
                        format!("视角：{}", d.pov),
                        "叙事",
                        Action::Generate(GenerateOp::Pov),
                    ),
                    item(
                        "编辑叙事要求",
                        "叙事",
                        Action::Generate(GenerateOp::Narrative),
                    ),
                    item(
                        "上下文选项 JSON",
                        "高级",
                        Action::Generate(GenerateOp::Options),
                    ),
                    item(
                        "导入已确认提示词 JSON 文件",
                        "高级",
                        Action::Generate(GenerateOp::Import),
                    ),
                    item("恢复自动组装", "本章", Action::Automatic),
                ];
                m.note = "只改变本章；不修改项目默认方案".into();
            }
        },
        Page::Job(_) => unreachable!(),
    }
    Ok(m)
}

fn chapter_text(ctx: &Context, id: &str) -> String {
    match core::read_chapter(ctx.clone(), id.into()) {
        Ok(t) if t.trim().is_empty() => "本章尚无正文。选择 AI 生成，或进入编辑正文。".into(),
        Ok(t) => t,
        Err(e) => format!("正文暂不可读：{e}\n\n可创建空章或直接设置 AI 生成。"),
    }
}
fn memory_label(file: &str) -> (&str, &'static str) {
    match file {
        "author-notes.md" => ("作者备注（不发送模型）", "作者资料"),
        "config.yaml" => ("流水线配置", "作者资料"),
        "memory/story_bible.yaml" => ("故事圣经", "故事设定"),
        "memory/characters.yaml" => ("角色档案", "故事设定"),
        "outlines/book_outline.yaml" => ("全书大纲", "故事设定"),
        "memory/foreshadowing.yaml" => ("伏笔档案", "故事设定"),
        "memory/style_bank.jsonl" => ("文风参考", "故事设定"),
        "memory/chapter_summaries.jsonl" => ("章节摘要", "自动记录"),
        "memory/events.jsonl" => ("历史事件", "自动记录"),
        "memory/timeline.jsonl" => ("时间线", "自动记录"),
        "memory/relationships.json" => ("关系记录", "自动记录"),
        _ => (file, "故事设定"),
    }
}
fn state_label(state: &str) -> &str {
    match state {
        "starting" => "启动中",
        "running" => "运行中",
        "succeeded" => "已完成",
        "failed" => "失败",
        "cancelled" => "已取消",
        "interrupted" => "已中断",
        other => other,
    }
}

pub(super) fn dispatch(app: &mut App, action: Action) -> Result<Effect> {
    match action {
        Action::Back => {
            app.nav.go(Nav::Back);
            return Ok(Effect::Stay);
        }
        Action::Home => return Ok(Effect::Home),
        Action::Exit => return Ok(Effect::Exit),
        Action::Module(module) => {
            app.nav.go(Nav::Home);
            return Ok(if module == Module::Home {
                Effect::Stay
            } else {
                Effect::Push(module.page())
            });
        }
        Action::Navigate(page) => {
            if let Page::Chapter(id) = &page {
                app.last_chapter = Some((app.ctx.project_dir.clone(), id.clone()));
            }
            return Ok(Effect::Push(page));
        }
        Action::OpenChapter(id, purpose) => {
            return dispatch(
                app,
                Action::Navigate(match purpose {
                    ChapterPurpose::Workbench => Page::Chapter(id),
                    ChapterPurpose::Reports => Page::Report(id),
                    ChapterPurpose::Preview => Page::Generate { id, preview: true },
                    ChapterPurpose::Context => {
                        return app.execute(words(&["context", "catalog", &id]))
                    }
                }),
            )
        }
        Action::NewChapter(purpose) => {
            let details = core::chapter_titles::list_chapter_details(app.ctx.clone())?;
            let next = details
                .iter()
                .filter_map(|c| c.id.trim_start_matches("ch").parse::<u32>().ok())
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let id = core::canonical_chapter_id(&answer!(
                app.input("章号（尚未写入正文文件）", &format!("ch{next:03}"))
            ))?;
            return dispatch(app, Action::OpenChapter(id, purpose));
        }
        Action::ToggleJobs => {
            app.all_jobs = !app.all_jobs;
            return Ok(Effect::Stay);
        }
        Action::Mode(manual) => {
            let id = match app.nav.page() {
                Page::Generate { id, .. } | Page::ManualGenerate(id) => id.clone(),
                _ => return Err("请先选择章节".into()),
            };
            return Ok(Effect::Replace(if manual {
                Page::ManualGenerate(id)
            } else {
                Page::Generate { id, preview: false }
            }));
        }
        Action::Project(path, op) => {
            if path.is_empty()
                && has_drafts(app)
                && !answer!(app.confirm("打开或新建项目？本次会话的生成草稿按项目保留。"))
            {
                return Ok(Effect::Stay);
            }
            if path.is_empty() {
                return match op {
                    ProjectOp::Open => app.act_on(Page::Projects, 1),
                    _ => app.act_on(Page::Projects, 2),
                };
            }
            let mut args = match op {
                ProjectOp::Open => {
                    if has_drafts(app)
                        && !answer!(app.confirm(
                            "切换项目？本次会话的已确认生成草稿会按项目保留，未提交模型。"
                        ))
                    {
                        return Ok(Effect::Stay);
                    }
                    words(&["project", "open", &path])
                }
                ProjectOp::Pin => words(&["project", "pin", &path]),
                ProjectOp::Unpin => words(&["project", "unpin", &path]),
                ProjectOp::Forget => {
                    if !answer!(app.confirm("仅移除项目记录，保留全部小说文件？"))
                    {
                        return Ok(Effect::Stay);
                    }
                    words(&["project", "forget", &path, "--yes"])
                }
                ProjectOp::Relocate => {
                    let next = answer!(app.input("新目录完整路径", &path));
                    words(&["project", "relocate", &path, &next])
                }
            };
            let effect = app.execute(std::mem::take(&mut args))?;
            if matches!(effect, Effect::Home) {
                return Ok(effect);
            }
            app.nav.go(Nav::Home);
            return Ok(Effect::Push(Page::RecentProjects));
        }
        _ => {}
    }
    let base = base_page(app.nav.page());
    if let Action::Setting(Setting::Provider) = action {
        let selected = answer!(menu_choice(
            &app.path("模型连接方式"),
            &words(&[
                "API（兼容接口）",
                "Claude Code（已登录订阅）",
                "Codex（已登录订阅）"
            ]),
            app.plain,
            app.no_color,
            &mut ListState::default().with_selected(Some(0)),
            false
        ));
        return app.execute(words(&[
            "settings",
            "set",
            "llm_provider",
            ["api", "claude_code", "codex"][selected],
        ]));
    }
    let index = match action {
        Action::Doctor => return app.act_on(Page::Home, 7),
        Action::Chapter(op) => op.index(),
        Action::Generate(op) => op.index(),
        Action::Manual(op) => op.index(),
        Action::Prompt(op) => op.index(),
        Action::Setting(op) => op.index(),
        Action::Memory(op) => op.index(),
        Action::Template(op) => op.index(),
        Action::Preview(op) => op.index(),
        Action::Scheme => {
            if matches!(base, Page::Generate { preview: true, .. }) {
                9
            } else {
                10
            }
        }
        Action::Automatic => {
            if matches!(base, Page::Generate { preview: true, .. }) {
                10
            } else {
                11
            }
        }
        Action::Report(kind) => {
            return match &base {
                Page::Report(id) => {
                    let text = core::read_report(app.ctx.clone(), id.clone(), kind.into())?;
                    app.view(
                        "报告",
                        if text.trim().is_empty() {
                            "尚无此报告。请先运行对应的生成、检查或记忆更新。"
                        } else {
                            &text
                        },
                    )
                }
                _ => Err("请先选择章节".into()),
            }
        }
        _ => return Err("无效的页面操作".into()),
    };
    app.act_on(base, index)
}
fn has_drafts(app: &App) -> bool {
    app.drafts.iter().any(|((p, _), d)| {
        core::project_manager::same_path(p, &app.ctx.project_dir) && !d.idea.is_empty()
    }) || app.manual_drafts.iter().any(|((p, _), d)| {
        core::project_manager::same_path(p, &app.ctx.project_dir)
            && (!d.prompts.system.is_empty() || !d.prompts.user.is_empty())
    })
}
impl App {
    fn act_on(&mut self, page: Page, index: usize) -> Result<Effect> {
        let original = std::mem::replace(&mut self.nav.frames.last_mut().unwrap().page, page);
        let result = self.act(index);
        self.nav.frames.last_mut().unwrap().page = original;
        result
    }
}

pub(super) fn visible(model: &Model, query: &str) -> Vec<Item> {
    model
        .items
        .iter()
        .filter(|i| {
            query.is_empty()
                || i.label.to_lowercase().contains(&query.to_lowercase())
                || matches!(i.action, Action::NewChapter(_) | Action::ToggleJobs)
        })
        .cloned()
        .collect()
}
pub(super) fn choose(app: &mut App) -> Result<Action> {
    let mut model = model(app)?;
    if app.plain {
        let items = model.items.clone();
        let root = matches!(app.nav.page(), Page::Home);
        return Ok(
            match menu_choice(
                &format!("{}\n项目：{} · {}", model.path, model.project, model.note),
                &items
                    .iter()
                    .map(|i| format!("[{}] {}", i.group, i.label))
                    .collect::<Vec<_>>(),
                true,
                app.no_color,
                &mut app.nav.frames.last_mut().unwrap().list,
                root,
            )? {
                Choice::Selected(i) => items[i].action.clone(),
                Choice::Back => Action::Back,
                Choice::Home => Action::Home,
            },
        );
    }
    let frame = app.nav.frames.last_mut().unwrap();
    if !frame.view.visited {
        frame.view.visited = true;
        if let Page::Chapter(id) = &frame.page {
            frame.view.focus = Focus::Body;
            frame.view.offset = *app
                .reading
                .get(&(app.ctx.project_dir.clone(), format!("{id}:工作台")))
                .unwrap_or(&0);
        }
    }
    let mut global = MODULES.iter().position(|m| *m == model.module).unwrap_or(0);
    let mut searching = false;
    let mut digits = String::new();
    let mut session = Session::new()?;
    loop {
        let frame = app.nav.frames.last_mut().unwrap();
        let filtered = visible(&model, &frame.view.query);
        let selected = frame
            .list
            .selected()
            .unwrap_or(0)
            .min(filtered.len().saturating_sub(1));
        frame.list.select(Some(selected));
        let size = session.terminal.size().map_err(|e| e.to_string())?;
        if size.width < 120 && frame.view.focus == Focus::Directory {
            frame.view.focus = Focus::Actions;
        }
        if model.chapter_list {
            if let Some(Item {
                action: Action::OpenChapter(id, _),
                ..
            }) = filtered.get(selected)
            {
                if model.selected_chapter.as_ref() != Some(id) {
                    model.text = chapter_text(&app.ctx, id);
                    model.title = format!("{id} · 正文预览");
                    model.selected_chapter = Some(id.clone());
                    frame.view.offset = 0;
                }
            } else {
                model.selected_chapter = None;
                model.text = "输入章号，开始下一章。".into();
                model.title = "新章节".into();
            }
        }
        session
            .terminal
            .draw(|f| {
                ui::render_workspace(
                    f,
                    &model,
                    &filtered,
                    &mut frame.list,
                    &mut frame.view,
                    global,
                    searching,
                    app.no_color,
                )
            })
            .map_err(|e| e.to_string())?;
        if let Page::Chapter(id) = &frame.page {
            app.reading.insert(
                (app.ctx.project_dir.clone(), format!("{id}:工作台")),
                frame.view.offset,
            );
        }
        if let Event::Key(k) = next_event()? {
            if k.kind == KeyEventKind::Release {
                continue;
            }
            if searching {
                match k.code {
                    KeyCode::Esc | KeyCode::Enter => searching = false,
                    KeyCode::Backspace => {
                        frame.view.query.pop();
                        frame.list.select(Some(0));
                    }
                    KeyCode::Char(c) => {
                        frame.view.query.push(c);
                        frame.list.select(Some(0));
                    }
                    _ => {}
                }
                continue;
            }
            match k.code {
                KeyCode::F(1) => {
                    frame.view.focus = if frame.view.focus == Focus::Global {
                        Focus::Actions
                    } else {
                        Focus::Global
                    }
                }
                KeyCode::Esc if frame.view.focus == Focus::Global => {
                    frame.view.focus = Focus::Actions
                }
                KeyCode::Esc if !frame.view.query.is_empty() => {
                    frame.view.query.clear();
                    frame.list.select(Some(0));
                }
                KeyCode::Esc => return Ok(Action::Back),
                KeyCode::Home | KeyCode::Char('h' | 'H') => return Ok(Action::Home),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(Action::Back)
                }
                KeyCode::Char('/') if model.searchable => {
                    searching = true;
                    frame.view.focus = Focus::Actions;
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    let mut focus = vec![Focus::Global];
                    if size.width >= 120 && !model.chapters.is_empty() && !model.chapter_list {
                        focus.push(Focus::Directory);
                    }
                    if !model.text.is_empty()
                        && (size.width >= 90 || matches!(frame.page, Page::Chapter(_)))
                    {
                        focus.push(Focus::Body);
                    }
                    focus.push(Focus::Actions);
                    let i = focus
                        .iter()
                        .position(|p| *p == frame.view.focus)
                        .unwrap_or(0);
                    frame.view.focus = focus[(i + if k.code == KeyCode::BackTab {
                        focus.len() - 1
                    } else {
                        1
                    }) % focus.len()];
                    digits.clear();
                }
                KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                    let down = matches!(k.code, KeyCode::Down | KeyCode::PageDown);
                    let step = if matches!(k.code, KeyCode::PageUp | KeyCode::PageDown) {
                        8
                    } else {
                        1
                    };
                    let shift = |current: usize, max: usize| {
                        if down {
                            current.saturating_add(step).min(max)
                        } else {
                            current.saturating_sub(step)
                        }
                    };
                    match frame.view.focus {
                        Focus::Global => global = shift(global, MODULES.len() - 1),
                        Focus::Directory => {
                            let current = frame.view.directory.selected().unwrap_or_else(|| {
                                model
                                    .chapters
                                    .iter()
                                    .position(|c| Some(&c.id) == model.selected_chapter.as_ref())
                                    .unwrap_or(0)
                            });
                            frame.view.directory.select(Some(shift(
                                current,
                                model.chapters.len().saturating_sub(1),
                            )));
                        }
                        Focus::Body => {
                            let (lines, _) = layout(
                                &model.text.chars().collect::<Vec<_>>(),
                                ui::body_width(size.width, model.chapter_list).max(1) as usize,
                                false,
                            );
                            frame.view.offset =
                                shift(frame.view.offset as usize, lines.len().saturating_sub(3))
                                    .min(u16::MAX as usize) as u16;
                        }
                        Focus::Actions => frame
                            .list
                            .select(Some(shift(selected, filtered.len().saturating_sub(1)))),
                    }
                    digits.clear();
                }
                KeyCode::Enter => match frame.view.focus {
                    Focus::Global => return Ok(Action::Module(MODULES[global])),
                    Focus::Directory => {
                        if let Some(c) = model
                            .chapters
                            .get(frame.view.directory.selected().unwrap_or(0))
                        {
                            return Ok(Action::Navigate(Page::Chapter(c.id.clone())));
                        }
                    }
                    Focus::Body if matches!(frame.page, Page::Chapter(_)) => {
                        return Ok(Action::Chapter(ChapterOp::Edit))
                    }
                    Focus::Body => {}
                    _ => {
                        if let Some(i) = filtered.get(selected) {
                            return Ok(i.action.clone());
                        }
                    }
                },
                KeyCode::Char('e') if matches!(frame.page, Page::Chapter(_)) => {
                    return Ok(Action::Chapter(ChapterOp::Edit))
                }
                KeyCode::Char('g') if matches!(frame.page, Page::Chapter(_)) => {
                    return Ok(Action::Chapter(ChapterOp::Generate))
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    frame.view.focus = Focus::Actions;
                    digits.push(c);
                    let mut n = digits.parse::<usize>().unwrap_or(0);
                    if n == 0 || n > filtered.len() {
                        digits = c.to_string();
                        n = digits.parse().unwrap_or(0);
                    }
                    if n > 0 && n <= filtered.len() {
                        frame.list.select(Some(n - 1));
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> (tempfile::TempDir, App) {
        let directory = tempfile::tempdir().unwrap();
        let ctx = Context::new(
            directory.path().join("data"),
            Some(directory.path().join("book")),
        )
        .unwrap();
        core::project_manager::open_project(
            ctx.clone(),
            ctx.project_dir.display().to_string(),
            true,
            None,
            None,
        )
        .unwrap();
        let app = App {
            ctx,
            plain: true,
            no_color: true,
            nav: Navigation::new(),
            keys: vec![],
            last_chapter: None,
            all_jobs: false,
            drafts: Default::default(),
            manual_drafts: Default::default(),
            reading: Default::default(),
        };
        (directory, app)
    }
    #[test]
    fn reordered_menu_actions_still_update_the_correct_chapter_draft() {
        let (_directory, mut app) = app();
        app.nav.push(Page::Generate {
            id: "ch002".into(),
            preview: false,
        });
        let mut entries = model(&mut app).unwrap().items;
        entries.reverse();
        let action = entries
            .into_iter()
            .find(|i| matches!(i.action, Action::Generate(GenerateOp::Overwrite)))
            .unwrap()
            .action;
        dispatch(&mut app, action).unwrap();
        let key = (app.ctx.project_dir.clone(), "ch002".into());
        assert!(app.drafts[&key].overwrite);
        assert!(!app.ctx.data_dir.join("jobs").exists());
        assert!(
            !core::project_source::chapter_path(&app.ctx.project_dir, "ch002")
                .unwrap()
                .exists()
        );
    }
    #[test]
    fn provider_settings_hide_api_credentials_and_show_current_values() {
        let (_directory, mut app) = app();
        app.nav.push(Page::Settings);
        let mut settings = Context::read_settings(&app.ctx.data_dir).unwrap();
        settings.llm_provider = Some("codex".into());
        settings.llm_model = "my-model".into();
        settings.llm_api_key = "must-not-appear".into();
        core::save_settings(app.ctx.clone(), settings).unwrap();
        let m = model(&mut app).unwrap();
        assert!(m.items.iter().any(|i| i.label.contains("my-model")));
        assert!(!m.items.iter().any(|i| matches!(
            i.action,
            Action::Setting(Setting::Key | Setting::Url | Setting::Thinking)
        )));
        assert!(!format!(
            "{} {:?}",
            m.text,
            m.items.iter().map(|i| &i.label).collect::<Vec<_>>()
        )
        .contains("must-not-appear"));
    }
    #[test]
    fn a_missing_chapter_does_not_claim_to_be_saved_or_count_placeholder_text() {
        let (_directory, mut app) = app();
        app.nav.push(Page::Chapter("ch003".into()));
        let m = model(&mut app).unwrap();
        assert!(
            m.note.contains("0 字") && m.note.contains("尚无正文文件"),
            "{}",
            m.note
        );
        assert!(
            !core::project_source::chapter_path(&app.ctx.project_dir, "ch003")
                .unwrap()
                .exists()
        );
    }
    #[test]
    fn module_changes_keep_local_drafts_and_scope_continue_to_the_current_project() {
        let (_directory, mut app) = app();
        let key = (app.ctx.project_dir.clone(), "ch005".into());
        app.drafts.insert(
            key.clone(),
            GenerationDraft {
                idea: "会话构思".into(),
                ..GenerationDraft::default()
            },
        );
        app.last_chapter = Some(key.clone());
        assert!(model(&mut app)
            .unwrap()
            .items
            .iter()
            .any(|i| i.label.contains("继续当前章节")));
        dispatch(&mut app, Action::Module(Module::Memory)).unwrap();
        assert_eq!(app.drafts[&key].idea, "会话构思");
        let next = app.ctx.project_dir.join("another-project");
        core::project_manager::open_project(
            app.ctx.clone(),
            next.display().to_string(),
            true,
            None,
            None,
        )
        .unwrap();
        app.ctx = Context::new(app.ctx.data_dir.clone(), Some(next)).unwrap();
        app.nav.go(Nav::Home);
        assert!(!model(&mut app)
            .unwrap()
            .items
            .iter()
            .any(|i| i.label.contains("继续当前章节")));
        assert_eq!(app.drafts[&key].idea, "会话构思");
    }
    #[test]
    fn task_lists_are_scoped_to_the_current_project_until_all_projects_is_chosen() {
        let (_directory, mut app) = app();
        let other = app.ctx.project_dir.join("other-book");
        for project in [&app.ctx.project_dir, &other] {
            let id = uuid::Uuid::new_v4().to_string();
            let folder = app.ctx.data_dir.join("jobs").join(&id);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("status.json"),serde_json::to_vec(&serde_json::json!({"id":id,"project":project,"state":"succeeded","stage":"完成","created_at":"2026-11-11T09:00:00Z","updated_at":"2026-11-11T09:00:00Z","result":null,"error":null})).unwrap()).unwrap();
        }
        app.nav.push(Page::Jobs);
        let current = model(&mut app).unwrap();
        assert_eq!(
            current
                .items
                .iter()
                .filter(|i| matches!(i.action, Action::Navigate(Page::Job(_))))
                .count(),
            1
        );
        dispatch(&mut app, Action::ToggleJobs).unwrap();
        let all = model(&mut app).unwrap();
        assert_eq!(
            all.items
                .iter()
                .filter(|i| matches!(i.action, Action::Navigate(Page::Job(_))))
                .count(),
            2
        );
    }
    #[test]
    fn filtering_preserves_the_explicit_new_chapter_action_without_mixing_chapter_targets() {
        let (_directory, mut app) = app();
        app.nav.push(Page::Chapters(ChapterPurpose::Workbench));
        let m = model(&mut app).unwrap();
        let filtered = visible(&m, "missing");
        assert_eq!(filtered.len(), 1);
        assert!(matches!(
            filtered[0].action,
            Action::NewChapter(ChapterPurpose::Workbench)
        ));
    }
}

pub(super) fn pick_module(plain: bool, no_color: bool) -> Result<Nav> {
    let mut state = ListState::default().with_selected(Some(0));
    Ok(
        match menu_choice(
            "模块导航",
            &MODULES
                .iter()
                .map(|m| m.label().to_string())
                .collect::<Vec<_>>(),
            plain,
            no_color,
            &mut state,
            false,
        )? {
            Choice::Selected(i) => Nav::Module(MODULES[i]),
            Choice::Back => Nav::Back,
            Choice::Home => Nav::Home,
        },
    )
}
