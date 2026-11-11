//! Shared terminal typography and adaptive, terminal-background-safe layouts.
use super::workspace::{Focus, Item, Model, ViewState, MODULES};
use super::*;
use ratatui::{
    style::Modifier,
    text::{Line, Span},
    widgets::{BorderType, Padding},
};

pub(super) fn accent(no_color: bool) -> Style {
    if no_color {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
}
pub(super) fn selected(no_color: bool) -> Style {
    if no_color {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        accent(false)
    }
}
pub(super) fn block(title: impl Into<String>, focused: bool, no_color: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            accent(no_color)
        } else {
            Style::default()
        })
        .title(Line::from(Span::styled(
            format!(" {} ", title.into()),
            if focused {
                accent(no_color)
            } else {
                Style::default()
            },
        )))
}
pub(super) fn chrome(f: &mut ratatui::Frame, path: &str, no_color: bool) -> [Rect; 3] {
    f.render_widget(Clear, f.area());
    let parts = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(f.area());
    let mut lines = vec![Line::from(Span::styled(
        " SODARIE  /  小说终端工作台",
        accent(no_color),
    ))];
    lines.extend(
        short_path(path, parts[0].width.saturating_sub(2))
            .lines()
            .map(|line| Line::from(line.to_owned())),
    );
    f.render_widget(Paragraph::new(lines), parts[0]);
    [parts[0], parts[1], parts[2]]
}
fn menu(
    f: &mut ratatui::Frame,
    area: Rect,
    items: &[Item],
    state: &mut ListState,
    focus: bool,
    query: &str,
    searching: bool,
    no_color: bool,
) {
    let no_matches = !query.is_empty()
        && items.iter().all(|item| {
            matches!(
                item.action,
                workspace::Action::NewChapter(_) | workspace::Action::ToggleJobs
            )
        });
    let title = if no_matches {
        format!("没有匹配项 · {query}")
    } else if searching {
        format!("搜索：{query}  (Enter 完成)")
    } else if !query.is_empty() {
        format!("搜索：{query}")
    } else {
        "操作".into()
    };
    let list = List::new(items.iter().enumerate().map(|(i, item)| {
        ListItem::new(Line::from(vec![
            Span::raw(format!("{:>2}. ", i + 1)),
            Span::styled(
                format!("[{}] ", item.group),
                Style::default().add_modifier(Modifier::DIM),
            ),
            Span::raw(item.label.clone()),
        ]))
    }))
    .block(block(title, focus, no_color))
    .highlight_style(selected(no_color))
    .highlight_symbol(if no_color { "> " } else { "› " });
    if items.is_empty() {
        f.render_widget(
            Paragraph::new("没有匹配项。\nEsc 清除筛选，或返回。")
                .block(block("搜索结果", focus, no_color))
                .wrap(Wrap { trim: false }),
            area,
        );
    } else {
        f.render_stateful_widget(list, area, state);
    }
}
fn prose(f: &mut ratatui::Frame, area: Rect, model: &Model, view: &mut ViewState, no_color: bool) {
    let inner = block(&model.title, view.focus == Focus::Body, no_color).inner(area);
    let (lines, _) = layout(
        &model.text.chars().collect::<Vec<_>>(),
        inner.width.max(1) as usize,
        false,
    );
    let max = lines
        .len()
        .saturating_sub(inner.height as usize)
        .min(u16::MAX as usize) as u16;
    view.offset = view.offset.min(max);
    f.render_widget(
        Paragraph::new(lines.join("\n"))
            .scroll((view.offset, 0))
            .block(block(&model.title, view.focus == Focus::Body, no_color)),
        area,
    );
}
pub(super) fn body_width(width: u16, chapter_list: bool) -> u16 {
    width.saturating_sub(if width >= 120 {
        18 + if chapter_list { 32 } else { 24 } + 2
    } else if width >= 90 {
        20
    } else {
        2
    })
}
pub(super) fn render_workspace(
    f: &mut ratatui::Frame,
    model: &Model,
    items: &[Item],
    state: &mut ListState,
    view: &mut ViewState,
    global: usize,
    searching: bool,
    no_color: bool,
) {
    let [_, body, footer] = chrome(
        f,
        &format!("{}\n项目：{} · {}", model.path, model.project, model.note),
        no_color,
    );
    let area = if f.area().width >= 90 {
        let cols = Layout::horizontal([Constraint::Length(18), Constraint::Min(1)]).split(body);
        let mut nav = ListState::default().with_selected(Some(if view.focus == Focus::Global {
            global
        } else {
            MODULES.iter().position(|m| *m == model.module).unwrap_or(0)
        }));
        f.render_stateful_widget(
            List::new(MODULES.iter().map(|m| ListItem::new(Line::from(m.label()))))
                .block(
                    block("导航", view.focus == Focus::Global, no_color)
                        .padding(Padding::vertical(1)),
                )
                .highlight_style(selected(no_color))
                .highlight_symbol("› "),
            cols[0],
            &mut nav,
        );
        cols[1]
    } else {
        body
    };
    if !model.chapter_list && !model.chapters.is_empty() && f.area().width >= 120 {
        let cols = Layout::horizontal([Constraint::Length(24), Constraint::Min(1)]).split(area);
        if view.directory.selected().is_none() {
            view.directory.select(Some(
                model
                    .chapters
                    .iter()
                    .position(|c| Some(&c.id) == model.selected_chapter.as_ref())
                    .unwrap_or(0),
            ));
        }
        f.render_stateful_widget(
            List::new(model.chapters.iter().map(|c| {
                ListItem::new(vec![
                    Line::from(c.id.clone()),
                    Line::from(if c.title.is_empty() {
                        "未命名章节".into()
                    } else {
                        c.title.clone()
                    }),
                    Line::from(""),
                ])
            }))
            .block(block("章节目录", view.focus == Focus::Directory, no_color))
            .highlight_style(selected(no_color))
            .highlight_symbol("› "),
            cols[0],
            &mut view.directory,
        );
        chapter_workspace(f, cols[1], model, items, state, view, searching, no_color);
    } else if model.chapter_list && f.area().width >= 120 {
        let cols = Layout::horizontal([Constraint::Length(32), Constraint::Min(1)]).split(area);
        menu(
            f,
            cols[0],
            items,
            state,
            view.focus == Focus::Actions,
            &view.query,
            searching,
            no_color,
        );
        prose(f, cols[1], model, view, no_color);
    } else if !model.text.is_empty() && (model.selected_chapter.is_some() && !model.chapter_list) {
        chapter_workspace(f, area, model, items, state, view, searching, no_color);
    } else if !model.text.is_empty() && f.area().width >= 90 {
        let cols = Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(area);
        menu(
            f,
            cols[0],
            items,
            state,
            view.focus == Focus::Actions,
            &view.query,
            searching,
            no_color,
        );
        prose(f, cols[1], model, view, no_color);
    } else {
        menu(
            f,
            area,
            items,
            state,
            view.focus == Focus::Actions,
            &view.query,
            searching,
            no_color,
        );
    }
    let help = if searching {
        "输入搜索 · Backspace 删除 · Enter / Esc 完成"
    } else if view.focus == Focus::Body && model.selected_chapter.is_some() && !model.chapter_list {
        "Esc 返回 · Home/H 主页 · F1 导航\n↑↓ 翻阅 · Enter/E 编辑 · G 生成\nTab 切换区域 · 数字 Enter 操作"
    } else if view.focus == Focus::Body {
        "Esc 返回 · Home/H 主页 · F1 导航\n↑↓ / PgUp PgDn 阅读\nTab 切回操作 · 数字 Enter 选择"
    } else {
        "Esc 返回 · Home/H 主页 · F1 导航\n↑↓ / 数字 Enter 选择 · Tab 切换\n/ 搜索（列表页） · 草稿保留至退出"
    };
    f.render_widget(Paragraph::new(help).wrap(Wrap { trim: false }), footer);
    if view.focus == Focus::Global && f.area().width < 90 {
        let width = f.area().width.min(36);
        let height = f.area().height.min(14);
        let popup = Rect::new(
            (f.area().width - width) / 2,
            (f.area().height - height) / 2,
            width,
            height,
        );
        f.render_widget(Clear, popup);
        let mut nav = ListState::default().with_selected(Some(global));
        f.render_stateful_widget(
            List::new(MODULES.iter().map(|m| ListItem::new(m.label())))
                .block(block("模块导航 · Esc 关闭", true, no_color))
                .highlight_style(selected(no_color))
                .highlight_symbol("› "),
            popup,
            &mut nav,
        );
    }
}
#[allow(clippy::too_many_arguments)]
fn chapter_workspace(
    f: &mut ratatui::Frame,
    area: Rect,
    model: &Model,
    items: &[Item],
    state: &mut ListState,
    view: &mut ViewState,
    searching: bool,
    no_color: bool,
) {
    let action_height = (area.height / 3 + 2).clamp(4, 10);
    let rows =
        Layout::vertical([Constraint::Min(1), Constraint::Length(action_height)]).split(area);
    prose(f, rows[0], model, view, no_color);
    menu(
        f,
        rows[1],
        items,
        state,
        view.focus == Focus::Actions,
        &view.query,
        searching,
        no_color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layouts_keep_navigation_and_chinese_content_at_all_terminal_sizes() {
        for (width, height) in [
            (8, 6),
            (24, 16),
            (40, 24),
            (89, 24),
            (90, 24),
            (119, 30),
            (120, 30),
            (160, 40),
        ] {
            for no_color in [true, false] {
                let mut terminal =
                    Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
                let model = Model {
                    path: "主页 › 章节工作台 › ch001".into(),
                    project: "中文项目与较长路径".repeat(8),
                    note: "已保存".into(),
                    module: workspace::Module::Chapters,
                    items: vec![Item {
                        label: "编辑正文".into(),
                        group: "写作",
                        action: workspace::Action::Chapter(workspace::ChapterOp::Edit),
                    }],
                    chapters: vec![],
                    selected_chapter: Some("ch001".into()),
                    text: "春雨落在旧院子里。\n".repeat(100),
                    title: "ch001 · 归途".into(),
                    searchable: false,
                    chapter_list: false,
                };
                let mut state = ListState::default().with_selected(Some(0));
                let mut view = ViewState {
                    focus: Focus::Body,
                    ..ViewState::default()
                };
                terminal
                    .draw(|f| {
                        render_workspace(
                            f,
                            &model,
                            &model.items,
                            &mut state,
                            &mut view,
                            1,
                            false,
                            no_color,
                        )
                    })
                    .unwrap();
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect();
                if width >= 40 && height >= 24 {
                    assert!(text.contains("ch001"));
                    assert!(text.contains("Esc"));
                    assert!(text.replace(' ', "").contains("编辑正文"));
                }
                if no_color {
                    assert!(terminal
                        .backend()
                        .buffer()
                        .content
                        .iter()
                        .all(|c| c.fg == Color::Reset && c.bg == Color::Reset));
                }
            }
        }
    }
}
