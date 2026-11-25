use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::TokioChildProcess;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn stdio_command(db: &std::path::Path, base_url: &str, athlete: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_intervals_icu_mcp"));
    command
        .env("MCP_STATE_DB_PATH", db)
        .env("MCP_STATE_ENCRYPTION_KEY", "11".repeat(32))
        .env("INTERVALS_ICU_BASE_URL", base_url)
        .env("INTERVALS_ICU_ATHLETE_ID", athlete)
        .env("INTERVALS_ICU_API_KEY", "test-key")
        .env("MCP_TRANSPORT", "stdio")
        .env("RUST_LOG", "error")
        .env_remove("MCP_TOKEN_REGISTRY_PATH")
        .env_remove("INTERVALS_ICU_OPENAPI_SPEC")
        .env_remove("INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED");
    command
}

fn mutation_call(token: &str) -> CallToolRequestParams {
    CallToolRequestParams::new("modify_training").with_arguments(
        serde_json::json!({
            "action": "create", "new_date": "2026-01-01", "new_name": "Run",
            "idempotency_token": token
        })
        .as_object()
        .unwrap()
        .clone(),
    )
}

async fn mount_create(upstream: &MockServer, athlete: &str) {
    Mock::given(method("POST"))
        .and(path(format!("/api/v1/athlete/{athlete}/events")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 123, "name": "Run", "category": "WORKOUT", "start_date_local": "2026-01-01"
        })))
        .expect(1)
        .mount(upstream)
        .await;
}

#[tokio::test]
async fn stdio_restarts_isolate_mutations_by_athlete_and_upstream() {
    let state = tempfile::tempdir().unwrap();
    let db = state.path().join("state.sqlite3");
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    mount_create(&first, "i111").await;
    mount_create(&first, "i222").await;
    mount_create(&second, "i111").await;

    for (upstream, athlete) in [
        (&first, "i111"),
        (&first, "i111"),
        (&first, "i222"),
        (&second, "i111"),
    ] {
        let transport =
            TokioChildProcess::new(stdio_command(&db, &upstream.uri(), athlete)).unwrap();
        let service = ().serve(transport).await.unwrap();
        let result = service
            .call_tool(mutation_call("shared-key"))
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true));
        service.cancel().await.unwrap();
    }
}

#[tokio::test]
async fn stdio_blocks_unscoped_sqlite_history_without_replaying_it() {
    use sha2::{Digest, Sha256};

    let state = tempfile::tempdir().unwrap();
    let db = state.path().join("state.sqlite3");
    let upstream = MockServer::start().await;
    let transport = TokioChildProcess::new(stdio_command(&db, &upstream.uri(), "i111")).unwrap();
    ().serve(transport).await.unwrap().cancel().await.unwrap();

    {
        let connection = rusqlite::Connection::open(&db).unwrap();
        for (token, state) in [
            ("legacy-unknown", "unknown"),
            ("legacy-completed", "completed"),
        ] {
            // Storage key used by the unscoped STDIO router before this cutover.
            let prior_key = hex::encode(Sha256::digest(format!(r#"[null,null,"{token}"]"#)));
            let scope_hash = Sha256::digest(
                [
                    b"intervals-icu-mcp:operation-scope:v1\0".as_slice(),
                    prior_key.as_bytes(),
                ]
                .concat(),
            );
            let completed = state == "completed";
            connection.execute(
                "INSERT INTO idempotency_operations (scope_hash, fingerprint_hash, state, created_at,
                    expires_at, result_nonce, result_ciphertext) VALUES (?1, zeroblob(32), ?2, 0, ?3, ?4, ?5)",
                rusqlite::params![scope_hash.as_slice(), state, completed.then_some(i64::MAX), completed.then_some([0u8;12]), completed.then_some([0u8;16])],
            ).unwrap();
        }
    }

    let transport = TokioChildProcess::new(stdio_command(&db, &upstream.uri(), "i222")).unwrap();
    let service = ().serve(transport).await.unwrap();
    for token in ["legacy-unknown", "legacy-completed"] {
        let error = service.call_tool(mutation_call(token)).await.unwrap_err();
        assert!(error.to_string().contains("do not retry"), "{error}");
    }
    service.cancel().await.unwrap();
    assert!(
        upstream
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method != wiremock::http::Method::POST)
    );
}

#[tokio::test]
async fn stdio_restart_blocks_a_mutation_interrupted_before_or_after_dispatch() {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    for (tool, boundary_method, boundary_path, arguments, expected_writes) in [
        (
            "plan_training",
            "GET",
            "/api/v1/athlete/i111/profile",
            serde_json::json!({"period_start":"2026-01-01", "period_end":"2026-01-07", "adaptive":false, "idempotency_token":"interrupted"}),
            0,
        ),
        (
            "modify_training",
            "POST",
            "/api/v1/athlete/i111/events",
            serde_json::json!({"action":"create", "new_date":"2026-01-01", "new_name":"Run", "idempotency_token":"interrupted"}),
            1,
        ),
    ] {
        let upstream = MockServer::start().await;
        Mock::given(method(boundary_method))
            .and(path(boundary_path))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(15)))
            .expect(1)
            .mount(&upstream)
            .await;

        let state = tempfile::tempdir().unwrap();
        let db = state.path().join("state.sqlite3");
        let mut child = stdio_command(&db, &upstream.uri(), "i111")
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
        input.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}\n").await.unwrap();
        let initialized = tokio::time::timeout(Duration::from_secs(10), output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            serde_json::from_str::<serde_json::Value>(&initialized)
                .unwrap()
                .get("result")
                .is_some(),
            "{initialized}"
        );
        input
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        let call = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":arguments}});
        input
            .write_all(format!("{call}\n").as_bytes())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if upstream
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .any(|request| request.url.path() == boundary_path)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the server should reach the controlled boundary");
        child.kill().await.unwrap();
        drop(input);
        drop(output);

        let transport =
            TokioChildProcess::new(stdio_command(&db, &upstream.uri(), "i111")).unwrap();
        let service = ().serve(transport).await.unwrap();
        let retry =
            CallToolRequestParams::new(tool).with_arguments(arguments.as_object().unwrap().clone());
        let error = tokio::time::timeout(Duration::from_secs(5), service.call_tool(retry))
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("unknown outcome"), "{error}");
        service.cancel().await.unwrap();
        let requests = upstream.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == wiremock::http::Method::POST)
                .count(),
            expected_writes
        );
    }
}

#[tokio::test]
async fn stdio_refuses_a_legacy_file_as_the_state_database() {
    let state = tempfile::tempdir().unwrap();
    let db = state.path().join("old-journal.json");
    let legacy = br#"[["legacy-key",{"result":{"content":[]}}]]"#;
    std::fs::write(&db, legacy).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    let result = stdio_command(&db, "http://127.0.0.1:1", "i111")
        .output()
        .await
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("durable state store"));
    assert_eq!(std::fs::read(&db).unwrap(), legacy);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(db).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}

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

    // The OpenAPI registry is private server state. It deliberately contains
    // operations that must never become client-visible MCP tools.
    let openapi = serde_json::json!({
        "openapi": "3.0.0",
        "info": { "title": "private test API", "version": "1" },
        "paths": {
            "/api/v1/raw-profile": {
                "get": {
                    "operationId": "rawGetProfile",
                    "responses": { "200": { "description": "ok" } }
                }
            },
            "/api/v1/raw-activities": {
                "get": {
                    "operationId": "rawListActivities",
                    "responses": { "200": { "description": "ok" } }
                }
            }
        }
    });
    Mock::given(method("GET"))
        .and(path("/api/v1/docs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openapi))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/raw-profile"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "sentinel": "raw-dispatch" })),
        )
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
    let state_dir = tempfile::tempdir().expect("create temporary state directory");
    let state_db_path = state_dir.path().join("mcp-state.sqlite3");
    let mut cmd = Command::new(bin);
    cmd.env("MCP_STATE_DB_PATH", state_db_path);
    cmd.env("MCP_STATE_ENCRYPTION_KEY", "11".repeat(32));
    cmd.env("INTERVALS_ICU_BASE_URL", mock.uri());
    cmd.env("INTERVALS_ICU_ATHLETE_ID", "ath123");
    cmd.env("INTERVALS_ICU_API_KEY", "tok");
    cmd.env("MCP_TRANSPORT", "stdio");
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

    // Wait for server to initialize (retry instead of fixed sleep)
    for _ in 0..20 {
        if service.peer_info().is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let peer_info = service.peer_info().expect("server should be initialized");
    assert!(
        peer_info.capabilities.tools.is_some(),
        "server initialize must advertise tool capability or MCP hosts may discover zero tools"
    );
    assert!(
        peer_info.capabilities.resources.is_some(),
        "server initialize must advertise resource capability"
    );

    // The public MCP contract is exactly the curated intent surface.
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
        "track_progress",
    ];

    let actual_names: std::collections::BTreeSet<String> = tools
        .tools
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    let expected_names = std::collections::BTreeSet::from_iter(
        expected_tool_names.iter().map(|name| (*name).to_string()),
    );
    assert_eq!(actual_names, expected_names);
    assert_eq!(tools.tools.len(), expected_tool_names.len());

    // Every public tool must carry its complete intent schema.
    for tool in &tools.tools {
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

    // Raw OpenAPI operation ids must not bypass the public intent boundary.
    let raw_call = service
        .call_tool(CallToolRequestParams::new("rawGetProfile"))
        .await;
    assert!(
        raw_call.is_err(),
        "raw OpenAPI operation must not be an MCP tool"
    );
    assert!(
        raw_call.unwrap_err().to_string().contains("Unknown intent"),
        "the server must reject the raw name at the intent boundary"
    );
    let requests = mock.received_requests().await.unwrap_or_default();
    assert!(
        !requests
            .iter()
            .any(|request| request.url.path() == "/api/v1/raw-profile"),
        "unknown MCP tools must not dispatch raw upstream operations"
    );

    // Note: Full intent execution testing requires mock API endpoints for all internal calls
    // This test verifies the MCP layer correctly exposes only intent tools
    // Integration tests in e2e_http.rs cover full intent execution with mocked APIs

    tokio::time::timeout(std::time::Duration::from_secs(10), service.cancel())
        .await
        .expect("cancel timeout")
        .expect("cancel");
}
