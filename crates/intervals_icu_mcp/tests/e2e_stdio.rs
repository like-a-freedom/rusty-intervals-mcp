use rmcp::ServiceExt;
use rmcp::transport::TokioChildProcess;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn e2e_stdio_lists_tools_and_calls_profile() {
    // Start a mock Intervals API
    let mock = MockServer::start().await;

    // Mock profile endpoint
    let profile_body = serde_json::json!({ "athlete": { "id": "ath123", "name": "Test Athlete" } });
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath123/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(profile_body))
        .mount(&mock)
        .await;

    // Mock activities endpoint
    let acts_body = serde_json::json!([ { "id": "act1", "name": "Run" } ]);
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath123/activities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(acts_body))
        .mount(&mock)
        .await;

    // Spawn the server as a child process via TokioChildProcess transport.
    // Always use the prebuilt test binary to avoid expensive recompiles under coverage tools.
    // Prefer the prebuilt test binary; if missing, build once and use it to avoid re-running `cargo run` under coverage.
    let bin = std::env::var("CARGO_BIN_EXE_intervals_icu_mcp")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let manifest_dir = PathBuf::from(
                std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
            );
            let workspace_root = manifest_dir
                .parent()
                .and_then(|p| p.parent())
                .unwrap_or(&manifest_dir)
                .to_path_buf();
            let target_root = std::env::var("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| workspace_root.join("target"));

            let mut path = target_root.join("debug");
            path.push(if cfg!(windows) {
                "intervals_icu_mcp.exe"
            } else {
                "intervals_icu_mcp"
            });
            if !path.exists() {
                let status = std::process::Command::new("cargo")
                    .args([
                        "build",
                        "-p",
                        "intervals_icu_mcp",
                        "--bin",
                        "intervals_icu_mcp",
                        "--quiet",
                    ])
                    .status()
                    .expect("failed to build server binary");
                assert!(
                    status.success(),
                    "failed to build intervals_icu_mcp binary (status {status})"
                );
            }
            path
        });
    let mut cmd = Command::new(bin);
    cmd.env("INTERVALS_ICU_BASE_URL", mock.uri());
    cmd.env("INTERVALS_ICU_ATHLETE_ID", "ath123");
    cmd.env("INTERVALS_ICU_API_KEY", "tok");
    // enable debug logging from child to stderr to aid debugging
    cmd.env("RUST_LOG", "debug");

    // `TokioChildProcess::new` takes the configured `Command`.
    // spawn with piped stderr so we can capture server logs on failure
    let (child, mut stderr_opt) = TokioChildProcess::builder(cmd)
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn child");
    let service = match ().serve(child).await {
        Ok(s) => s,
        Err(e) => {
            if let Some(ref mut stderr) = stderr_opt {
                use tokio::io::AsyncReadExt;
                let mut buf = String::new();
                let _ = stderr.read_to_string(&mut buf).await;
                eprintln!("child stderr:\n{}", buf);
            }
            panic!("serve failed: {e}");
        }
    };

    // Give server a moment to initialize
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // List tools and ensure our tools are present
    let tools = match service.list_tools(Default::default()).await {
        Ok(t) => t,
        Err(e) => {
            if let Some(ref mut stderr) = stderr_opt {
                use tokio::io::AsyncReadExt;
                let mut buf = String::new();
                let _ = stderr.read_to_string(&mut buf).await;
                eprintln!("child stderr after list_tools error:\n{}", buf);
            }
            panic!("list tools failed: {e}")
        }
    };
    let names: Vec<_> = tools
        .tools
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.iter().any(|n| n == "get_athlete_profile"));
    assert!(names.iter().any(|n| n == "get_recent_activities"));

    // Call get_athlete_profile
    let res = service
        .call_tool(rmcp::model::CallToolRequestParams {
            name: "get_athlete_profile".into(),
            arguments: None,
            meta: None,
            task: None,
        })
        .await
        .expect("call_tool");
    // Expect structured JSON result matching profile
    let structured = res.structured_content;
    assert!(structured.is_some());
    let v = structured.unwrap();
    assert!(v.get("id").is_some());

    service.cancel().await.expect("cancel");
}
