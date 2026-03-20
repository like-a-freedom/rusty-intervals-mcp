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
    // keep child stderr quieter to avoid stdio backpressure during long e2e runs
    cmd.env("RUST_LOG", "info");

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

    let peer_info = service
        .peer_info()
        .expect("client should capture initialize result from server");
    assert!(
        peer_info.capabilities.tools.is_some(),
        "server initialize must advertise tool capability or MCP hosts may discover zero tools"
    );
    assert!(
        peer_info.capabilities.resources.is_some(),
        "server initialize must advertise resource capability"
    );

    // List tools and ensure intent tools are present (8 intents only)
    let tools = match tokio::time::timeout(
        std::time::Duration::from_secs(20),
        service.list_tools(Default::default()),
    )
    .await
    {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => {
            if let Some(ref mut stderr) = stderr_opt {
                use tokio::io::AsyncReadExt;
                let mut buf = String::new();
                let _ = stderr.read_to_string(&mut buf).await;
                eprintln!("child stderr after list_tools error:\n{}", buf);
            }
            panic!("list tools failed: {e}")
        }
        Err(_) => {
            if let Some(ref mut stderr) = stderr_opt {
                use tokio::io::AsyncReadExt;
                let mut buf = String::new();
                let _ = stderr.read_to_string(&mut buf).await;
                eprintln!("child stderr after list_tools timeout:\n{}", buf);
            }
            panic!("list tools timed out")
        }
    };
    let expected_tool_names = [
        "plan_training",
        "analyze_training",
        "modify_training",
        "compare_periods",
        "assess_recovery",
        "manage_profile",
        "manage_gear",
        "analyze_race",
    ];

    for tool in &tools.tools {
        assert!(
            expected_tool_names.contains(&tool.name.as_ref()),
            "unexpected MCP tool exposed: {}",
            tool.name
        );
        assert!(
            !tool.input_schema.is_empty(),
            "{} should expose a non-empty input schema",
            tool.name
        );
        assert_eq!(
            tool.input_schema
                .get("type")
                .and_then(serde_json::Value::as_str),
            Some("object"),
            "{} input schema should be a JSON object schema",
            tool.name
        );
        assert!(
            tool.input_schema.contains_key("properties"),
            "{} input schema should include properties",
            tool.name
        );

        let output_schema = tool.output_schema.as_ref().unwrap_or_else(|| {
            panic!(
                "{} should expose an output schema to MCP clients",
                tool.name
            )
        });
        assert_eq!(
            output_schema
                .get("type")
                .and_then(serde_json::Value::as_str),
            Some("object"),
            "{} output schema should be a JSON object schema",
            tool.name
        );

        let output_properties = output_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("{} output schema should include properties", tool.name));
        assert!(
            output_properties.contains_key("content"),
            "{} output schema should expose content blocks",
            tool.name
        );
        assert!(
            output_properties.contains_key("suggestions"),
            "{} output schema should expose suggestions",
            tool.name
        );
        assert!(
            output_properties.contains_key("next_actions"),
            "{} output schema should expose next_actions",
            tool.name
        );
    }

    let names: Vec<_> = tools
        .tools
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();

    // Verify only 8 intent tools are exposed (no dynamic OpenAPI tools)
    assert_eq!(names.len(), 8, "Should have exactly 8 intent tools");
    for expected_name in expected_tool_names {
        assert!(
            names.iter().any(|name| name == expected_name),
            "Missing {expected_name}"
        );
    }

    // Note: Full intent execution testing requires mock API endpoints for all internal calls
    // This test verifies the MCP layer correctly exposes only intent tools
    // Integration tests in e2e_http.rs cover full intent execution with mocked APIs

    tokio::time::timeout(std::time::Duration::from_secs(10), service.cancel())
        .await
        .expect("cancel timeout")
        .expect("cancel");
}
