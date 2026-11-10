use novel_core::{self as core, Context};
use std::{fs, path::PathBuf};
fn setup() -> (tempfile::TempDir, Context) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let root = dir.path().join("小说 空格");
    let context = Context::new(data, None).unwrap();
    core::project_manager::open_project(
        context.clone(),
        root.display().to_string(),
        true,
        None,
        None,
    )
    .unwrap();
    let context = Context::new(context.data_dir, None).unwrap();
    (dir, context)
}
#[test]
fn frozen_project_and_optimistic_edit_prevent_cross_session_overwrites() {
    let (dir, ctx) = setup();
    core::save_chapter_checked(ctx.clone(), "ch001".into(), "原稿".into(), None).unwrap();
    let other = dir.path().join("另一部书");
    core::project_manager::open_project(ctx.clone(), other.display().to_string(), true, None, None)
        .unwrap();
    core::save_chapter_checked(
        ctx.clone(),
        "ch001".into(),
        "CLI 新稿".into(),
        Some("原稿".into()),
    )
    .unwrap();
    assert!(core::save_chapter_expected(
        ctx.clone(),
        "ch001".into(),
        "GUI 旧稿".into(),
        "原稿".into()
    )
    .unwrap_err()
    .contains("内容冲突"));
    assert_eq!(
        core::read_chapter(ctx.clone(), "ch001".into()).unwrap(),
        "CLI 新稿"
    );
    assert!(!other.join("chapters/ch001/chapter.md").exists());
    let _operation = ctx.operation_access().unwrap();
    assert_eq!(
        core::read_chapter(ctx.clone(), "ch001".into()).unwrap(),
        "CLI 新稿"
    );
    assert!(core::save_chapter(ctx, "ch001".into(), "冲突".into())
        .unwrap_err()
        .starts_with("资源忙"));
}
#[test]
fn independent_settings_fields_merge_but_same_field_conflicts() {
    let (_dir, ctx) = setup();
    let before = ctx.settings.clone();
    let mut changed = before.clone();
    changed.temperature = Some(0.5);
    core::save_settings_checked(ctx.clone(), changed, before.clone()).unwrap();
    let mut gui = before.clone();
    gui.llm_model = "新模型".into();
    let merged = core::save_settings_checked(ctx.clone(), gui, before.clone()).unwrap();
    assert_eq!(merged.temperature, Some(0.5));
    assert_eq!(merged.llm_model, "新模型");
    let mut stale = before.clone();
    stale.temperature = Some(0.9);
    assert!(core::save_settings_checked(ctx, stale, before)
        .err()
        .unwrap()
        .contains("内容冲突"));
}
#[test]
fn templates_preserve_existing_author_files_and_ids_stay_inside_project() {
    let (dir, ctx) = setup();
    let path = ctx.project_dir.join("memory/characters.yaml");
    fs::write(&path, "作者人物档案").unwrap();
    fs::remove_file(ctx.project_dir.join(".initialized")).unwrap();
    core::project_manager::open_project(
        ctx.clone(),
        ctx.project_dir.display().to_string(),
        false,
        None,
        None,
    )
    .unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "作者人物档案");
    assert!(core::read_report(ctx.clone(), "../../evil".into(), "generation".into()).is_err());
    assert_eq!(
        core::canonical_chapter_id("第十二章 过河").unwrap(),
        "ch012"
    );
    let missing = Context::new(ctx.data_dir, Some(dir.path().join("missing"))).unwrap();
    assert!(core::list_chapters(missing).is_err());
    assert!(!dir.path().join("missing").exists());
}
#[test]
fn shared_external_source_and_commit_locks_protect_readers() {
    let (dir, one) = setup();
    let source = dir.path().join("共享正文");
    fs::create_dir(&source).unwrap();
    core::save_source_settings(
        one.clone(),
        core::project_source::SourceSettings {
            directory: source.display().to_string(),
        },
    )
    .unwrap();
    let second = dir.path().join("第二书");
    core::project_manager::open_project(
        one.clone(),
        second.display().to_string(),
        true,
        None,
        None,
    )
    .unwrap();
    let two = Context::new(one.data_dir.clone(), Some(PathBuf::from(second))).unwrap();
    core::save_source_settings(
        two.clone(),
        core::project_source::SourceSettings {
            directory: source.display().to_string(),
        },
    )
    .unwrap();
    let _lease = one.operation_access().unwrap();
    assert!(two.operation_access().is_err());
    let commit = one.commit_access().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let _read = two.read_access().unwrap();
        sender.send(()).unwrap();
    });
    assert!(receiver
        .recv_timeout(std::time::Duration::from_millis(50))
        .is_err());
    drop(commit);
    receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    reader.join().unwrap();
}

#[test]
fn startup_initializes_legacy_projects_without_recreating_missing_ones() {
    let (_dir, ctx) = setup();
    fs::remove_file(ctx.project_dir.join(".initialized")).unwrap();
    fs::remove_file(ctx.project_dir.join("memory/style_bank.jsonl")).unwrap();
    core::project_manager::remember_startup(&ctx).unwrap();
    assert!(ctx.project_dir.join("memory/style_bank.jsonl").is_file());
    let source = core::get_source_settings(ctx.clone()).unwrap();
    core::save_source_checked(
        ctx.clone(),
        core::project_source::SourceSettings {
            directory: "正文".into(),
        },
        source.directory.clone(),
    )
    .unwrap();
    assert!(
        core::save_source_checked(ctx.clone(), source.clone(), source.directory)
            .unwrap_err()
            .contains("内容冲突")
    );
    fs::remove_dir_all(&ctx.project_dir).unwrap();
    core::project_manager::remember_startup(&ctx).unwrap();
    assert!(!ctx.project_dir.exists());
}

#[test]
fn legacy_generation_presets_save_through_checked_api_and_reject_stale_edits() {
    let (_dir, ctx) = setup();
    fs::write(ctx.project_dir.join(".prompt-settings.json"),
        r#"{"system":"作者系统","user_template":"原有模板：{{事件骨架}}"}"#).unwrap();
    let before = core::get_prompt_settings(ctx.clone()).unwrap();
    let mut next = before.clone();
    next.generation_preset = core::GenerationPreset::Compact;
    next.compact_generation.user_template = "独立精简模板：{{事件骨架}}".into();
    core::save_prompt_checked(ctx.clone(), next.clone(), serde_json::to_value(&before).unwrap()).unwrap();
    let stored = core::get_prompt_settings(ctx.clone()).unwrap();
    assert_eq!(stored, next);
    assert_eq!(stored.system, before.system);
    assert_eq!(stored.user_template, before.user_template);
    assert_eq!(stored.consistency, before.consistency);
    assert_eq!(stored.memory, before.memory);
    assert!(core::save_prompt_checked(ctx, before.clone(), serde_json::to_value(before).unwrap())
        .unwrap_err().contains("内容冲突"));
}
