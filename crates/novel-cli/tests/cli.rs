use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Output, Stdio},
    time::Duration,
};
fn run(data: &Path, args: &[&str]) -> Output {
    // Files let us wait for the CLI alone. On Windows, a detached worker may
    // inherit an extra pipe handle and keep Command::output waiting for EOF.
    let mut stdout = tempfile::tempfile().unwrap();
    let mut stderr = tempfile::tempfile().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_novel"))
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().unwrap())
        .stderr(stderr.try_clone().unwrap())
        .status()
        .unwrap();
    stdout.seek(SeekFrom::Start(0)).unwrap();
    stderr.seek(SeekFrom::Start(0)).unwrap();
    let mut output = Output { status, stdout: vec![], stderr: vec![] };
    stdout.read_to_end(&mut output.stdout).unwrap();
    stderr.read_to_end(&mut output.stderr).unwrap();
    output
}
fn ok(data: &Path, args: &[&str]) -> Value {
    let out = run(data, args);
    assert!(
        out.status.success(),
        "{:?}\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let root = temp.path().join("中文 小说");
    ok(
        &data,
        &["--json", "project", "create", root.to_str().unwrap()],
    );
    (temp, data, root)
}
fn model(
    data: &Path,
    responses: Vec<String>,
    delay: Duration,
) -> std::thread::JoinHandle<Vec<Value>> {
    controlled_model(data, responses, delay, None)
}
fn controlled_model(
    data: &Path,
    responses: Vec<String>,
    delay: Duration,
    release: Option<std::path::PathBuf>,
) -> std::thread::JoinHandle<Vec<Value>> {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", server.local_addr().unwrap());
    ok(data, &["settings", "set", "llm_base_url", &url]);
    ok(data, &["settings", "set", "llm_model", "mock-model"]);
    let accepted = data.join("model-accepted");
    std::thread::spawn(move || {
        responses.into_iter().map(|content|{
            let (mut stream,_)=server.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut bytes=Vec::new();let mut buf=[0;4096];
            let split=loop{let n=stream.read(&mut buf).unwrap();assert!(n>0);bytes.extend_from_slice(&buf[..n]);if let Some(i)=bytes.windows(4).position(|b|b==b"\r\n\r\n"){break i+4;}};
            let headers=String::from_utf8_lossy(&bytes[..split]);let length:usize=headers.lines().find_map(|l|l.to_ascii_lowercase().strip_prefix("content-length:").map(|n|n.trim().parse().unwrap())).unwrap();
            while bytes.len()<split+length{let n=stream.read(&mut buf).unwrap();assert!(n>0);bytes.extend_from_slice(&buf[..n]);}
            let request=serde_json::from_slice(&bytes[split..split+length]).unwrap();
            fs::write(&accepted,b"accepted").unwrap();
            if let Some(release) = &release {
                let deadline = std::time::Instant::now() + Duration::from_secs(20);
                while !release.exists() {
                    assert!(std::time::Instant::now() < deadline, "test did not release model response");
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            std::thread::sleep(delay);
            let body=json!({"choices":[{"message":{"content":content}}]}).to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();request
        }).collect()
    })
}
#[test]
fn full_pipeline_uses_shared_data_and_preserves_generation_contract() {
    let (_temp, data, root) = setup();
    let extraction=json!({"chapter_id":"ch001","chapter_summary":"主角取到信。","events":[],"character_updates":[],"foreshadowing_updates":[],"relationship_updates":[],"continuity_risks":[]}).to_string();
    let server = model(
        &data,
        vec![
            "他拿起桌上的信。".into(),
            "# 一致性报告\n没有冲突。".into(),
            extraction,
            json!({"titles":["旧信","归乡","门后"]}).to_string(),
        ],
        Duration::ZERO,
    );
    let generated = ok(
        &data,
        &[
            "--json",
            "chapter",
            "generate",
            "ch001",
            "--idea",
            "主角取信",
            "--no-context",
        ],
    );
    assert_eq!(generated["success"], true);
    assert_eq!(
        fs::read_to_string(root.join("chapters/ch001/chapter.md")).unwrap(),
        "他拿起桌上的信。"
    );
    ok(&data, &["--json", "chapter", "check", "ch001"]);
    ok(&data, &["--json", "memory", "update", "ch001"]);
    let titles = ok(&data, &["--json", "chapter", "title", "generate", "ch001"]);
    assert_eq!(titles.as_array().unwrap().len(), 3);
    let requests = server.join().unwrap();
    assert_eq!(requests[0]["model"], "mock-model");
    assert_eq!(requests[0]["temperature"], 0.8);
    assert!(requests[0]["messages"][1]["content"]
        .as_str()
        .unwrap()
        .contains("主角取信"));
    assert!(requests[0].get("max_tokens").is_none());
    assert_eq!(
        fs::read_to_string(root.join("memory/chapter_summaries.jsonl"))
            .unwrap()
            .lines()
            .filter(|s| serde_json::from_str::<Value>(s).unwrap()["chapter_id"] == "ch001")
            .count(),
        1
    );
    let jobs = ok(&data, &["job", "list"]);
    assert_eq!(jobs.as_array().unwrap().len(), 4);
    let completed = jobs[0]["id"].as_str().unwrap();
    assert!(!run(&data, &["__worker", completed]).status.success());
    assert_eq!(ok(&data, &["job", "show", completed])["state"], "succeeded");
    for entry in fs::read_dir(data.join("jobs")).unwrap() {
        let request = fs::read_to_string(entry.unwrap().path().join("request.json")).unwrap();
        assert!(request.contains("\"llm_api_key\": \"\""));
    }
}
#[test]
fn detached_worker_survives_client_exit_and_blocks_concurrent_writes() {
    let (_temp, data, root) = setup();
    let release = data.join("release-model");
    let server = controlled_model(&data, vec!["后台完成。".into()], Duration::ZERO, Some(release.clone()));
    let status = ok(
        &data,
        &[
            "--json",
            "chapter",
            "generate",
            "ch001",
            "--idea",
            "取信",
            "--no-context",
            "--detach",
        ],
    );
    let id = status["id"].as_str().unwrap();
    assert_eq!(ok(&data, &["job", "show", id])["state"], "running");
    let busy = run(&data, &["chapter", "new", "ch002"]);
    assert_eq!(busy.status.code(), Some(3));
    assert!(run(&data, &["chapter", "list"]).status.success());
    fs::write(release, b"release").unwrap();
    ok(&data, &["job", "wait", id]);
    assert!(!run(&data, &["__worker", id]).status.success());
    server.join().unwrap();
    assert_eq!(
        fs::read_to_string(root.join("chapters/ch001/chapter.md")).unwrap(),
        "后台完成。"
    );
    assert!(run(&data, &["chapter", "new", "ch002"]).status.success());
}
#[test]
fn killing_waiting_client_does_not_kill_worker() {
    let (_temp, data, root) = setup();
    let server = model(&data, vec!["断连后完成。".into()], Duration::from_secs(2));
    let mut client = Command::new(env!("CARGO_BIN_EXE_novel"))
        .arg("--data-dir")
        .arg(&data)
        .args([
            "chapter",
            "generate",
            "ch001",
            "--idea",
            "取信",
            "--no-context",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut id = None;
    for _ in 0..100 {
        let jobs = ok(&data, &["job", "list"]);
        if let Some(job) = jobs.as_array().unwrap().first() {
            if job["state"] == "running" {
                id = Some(job["id"].as_str().unwrap().to_string());
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let id = id.expect("worker did not start");
    client.kill().unwrap();
    client.wait().unwrap();
    ok(&data, &["job", "wait", &id]);
    server.join().unwrap();
    assert_eq!(
        fs::read_to_string(root.join("chapters/ch001/chapter.md")).unwrap(),
        "断连后完成。"
    );
}
#[test]
fn cancellation_keeps_existing_chapter_and_noninteractive_contracts_hold() {
    let (_temp, data, root) = setup();
    ok(&data, &["chapter", "new", "ch001"]);
    let release = data.join("release-model");
    let server = controlled_model(&data, vec!["应当放弃的新稿".into()], Duration::ZERO, Some(release.clone()));
    let status = ok(
        &data,
        &[
            "--json",
            "chapter",
            "generate",
            "ch001",
            "--idea",
            "取信",
            "--overwrite",
            "--no-context",
            "--detach",
        ],
    );
    let id = status["id"].as_str().unwrap();
    for _ in 0..100 {
        if data.join("model-accepted").exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    ok(&data, &["job", "cancel", id]);
    fs::write(release, b"release").unwrap();
    let wait = run(&data, &["job", "wait", id]);
    assert_eq!(wait.status.code(), Some(130));
    server.join().unwrap();
    assert_eq!(
        fs::read_to_string(root.join("chapters/ch001/chapter.md")).unwrap(),
        ""
    );
    assert_eq!(
        run(&data, &["chapter", "reset", "ch001"]).status.code(),
        Some(1)
    );
    assert_eq!(
        run(
            &data,
            &[
                "chapter",
                "generate",
                "ch001",
                "--idea",
                "a",
                "--idea-stdin"
            ]
        )
        .status
        .code(),
        Some(2)
    );
    assert!(run(&data, &[]).status.success());
    let settings = String::from_utf8(run(&data, &["settings", "show"]).stdout).unwrap();
    assert!(settings.contains("[hidden]"));
}

#[test]
fn partial_save_failure_is_reported_without_claiming_generation_succeeded() {
    let (_temp, data, root) = setup();
    // A directory at the report destination forces an atomic replacement failure,
    // while the chapter destination remains writable.
    fs::create_dir(root.join("outputs/reports/ch001_chapter_generation_report.md")).unwrap();
    let server = model(&data, vec!["正文已写入。".into()], Duration::ZERO);
    let output = run(
        &data,
        &[
            "--json",
            "chapter",
            "generate",
            "ch001",
            "--idea",
            "取信",
            "--no-context",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(error["error"].as_str().unwrap().contains("正文已保存"));
    assert_eq!(
        fs::read_to_string(root.join("chapters/ch001/chapter.md")).unwrap(),
        "正文已写入。"
    );
    server.join().unwrap();
    let jobs = ok(&data, &["job", "list"]);
    assert_eq!(jobs[0]["state"], "failed");
}

#[test]
fn benchmark_draft_persists_and_is_validated_only_when_used() {
    let (temp, data, _root) = setup();
    let mut settings = ok(&data, &["prompt", "show"]);
    assert_eq!(settings["benchmark_generation"], json!({"system":"", "user_template":""}));
    assert_eq!(settings["benchmark_material_guidance"], false);
    ok(&data, &["prompt", "preset", "benchmark"]);
    let preview = ["--json", "prompt", "preview", "ch001", "--idea", "进入山谷", "--no-context"];
    let failure = run(&data, &preview);
    assert!(!failure.status.success());
    let error: Value = serde_json::from_slice(&failure.stdout).unwrap();
    assert!(error["error"].as_str().unwrap().contains("对标方案尚未配置完整"));
    let mut general = preview.to_vec();
    general.extend(["--preset", "general"]);
    ok(&data, &general);
    settings["generation_preset"] = json!("benchmark");
    let file = temp.path().join("prompt.json");
    for draft in [json!({"system":"对标系统", "user_template":""}),
                  json!({"system":"对标系统", "user_template":"草稿"})] {
        settings["benchmark_generation"] = draft;
        fs::write(&file, serde_json::to_vec(&settings).unwrap()).unwrap();
        ok(&data, &["prompt", "save", "--file", file.to_str().unwrap()]);
        assert_eq!(ok(&data, &["prompt", "show"]), settings);
        assert!(!run(&data, &preview).status.success());
    }
    settings["benchmark_generation"]["user_template"] = json!("对标：{{事件骨架}}");
    fs::write(&file, serde_json::to_vec(&settings).unwrap()).unwrap();
    ok(&data, &["prompt", "save", "--file", file.to_str().unwrap()]);
    let result = ok(&data, &preview);
    assert!(result["prompts"]["system"].as_str().unwrap().contains("对标系统"));
    let user = result["prompts"]["user"].as_str().unwrap();
    assert!(user.contains("对标：进入山谷"));
    assert!(!user.contains("【资料使用原则】"));
    assert_eq!(ok(&data, &["prompt", "preset"])["generation_preset"], "benchmark");
}

#[test]
fn project_default_preset_persists_and_controls_cli_preview() {
    let (_temp, data, _root) = setup();
    assert_eq!(ok(&data, &["prompt", "preset"])["generation_preset"], "general");
    let before = ok(&data, &["prompt", "show"]);
    assert_eq!(ok(&data, &["prompt", "preset", "compact"])["generation_preset"], "compact");
    let after = ok(&data, &["prompt", "show"]);
    assert_eq!(before["user_template"], after["user_template"]);
    assert_eq!(before["compact_generation"], after["compact_generation"]);
    assert_eq!(before["consistency"], after["consistency"]);
    assert_eq!(before["memory"], after["memory"]);
    let preview = ok(&data, &["--json", "prompt", "preview", "ch001", "--idea", "进入山谷", "--no-context"]);
    let user = preview["prompts"]["user"].as_str().unwrap();
    assert!(user.contains("【事件骨架】"));
    assert!(!user.contains("遵循作者事件骨架的顺序，不改变动作执行者"));
    let temporary = ok(&data, &["--json", "prompt", "preview", "ch001", "--idea", "进入山谷", "--no-context", "--preset", "general"]);
    assert!(temporary["prompts"]["user"].as_str().unwrap().contains("遵循作者事件骨架的顺序，不改变动作执行者"));
    assert_eq!(ok(&data, &["prompt", "preset"])["generation_preset"], "compact");
    assert!(!run(&data, &["prompt", "preset", "unknown"]).status.success());
    assert_eq!(ok(&data, &["prompt", "preset", "general"])["generation_preset"], "general");
}

#[test]
fn cli_preview_shows_imported_prompt_and_rejects_stale_sources() {
    let (temp, data, root) = setup();
    let args = ["--json", "prompt", "preview", "ch001", "--idea", "主角取信", "--no-context"];
    let mut preview = ok(&data, &args);
    preview["prompts"]["system"] = json!("手动系统要求");
    preview["prompts"]["user"] = json!("只写拿起信的场景。");
    let file = temp.path().join("preview.json");
    fs::write(&file, serde_json::to_vec(&preview).unwrap()).unwrap();
    let mut custom_args = args.to_vec();
    custom_args.extend(["--prompt-override", file.to_str().unwrap()]);
    let shown = ok(&data, &custom_args);
    assert_eq!(shown["prompts"], preview["prompts"]);
    assert_eq!(shown["stats"]["characters"], "手动系统要求只写拿起信的场景。".chars().count());
    assert!(!root.join("chapters/ch001/chapter.md").exists());
    assert!(ok(&data, &["job", "list"]).as_array().unwrap().is_empty());
    preview["source"] = json!("过期来源");
    fs::write(&file, serde_json::to_vec(&preview).unwrap()).unwrap();
    assert!(!run(&data, &custom_args).status.success());
}
