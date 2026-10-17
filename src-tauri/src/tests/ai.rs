use super::*;
use crate::test_support::*;
use crate::naming::build_directory_blueprint;
use std::fs;

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

async fn mock_ollama(
    responses: Vec<(u16, String)>,
) -> (String, tokio::task::JoinHandle<Vec<serde_json::Value>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, model_response) in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let header_end = loop {
                let mut chunk = [0u8; 4096];
                let read = stream.read(&mut chunk).await.unwrap();
                assert!(read > 0, "client closed before sending the request");
                request.extend_from_slice(&chunk[..read]);
                if let Some(position) = find_subslice(&request, b"\r\n\r\n") {
                    let end = position + 4;
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .expect("request should have a content length");
                    while request.len() < end + content_length {
                        let mut body_chunk = vec![0u8; end + content_length - request.len()];
                        let read = stream.read(&mut body_chunk).await.unwrap();
                        assert!(read > 0, "client closed before sending the request body");
                        request.extend_from_slice(&body_chunk[..read]);
                    }
                    break end;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            requests.push(
                serde_json::from_slice(&request[header_end..header_end + content_length])
                    .unwrap(),
            );
            let body = serde_json::json!({ "response": model_response }).to_string();
            let reason = if status == 200 { "OK" } else { "Error" };
            let response = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status,
                reason,
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
        requests
    });
    (format!("http://{}/api/generate", address), task)
}

fn model_json(name: &str) -> String {
    serde_json::json!({
        "new_name": name,
        "category": "Research",
        "reason": "Matched the organization and document topic."
    })
    .to_string()
}

#[tokio::test]
async fn sends_configured_request_to_local_ollama_and_normalizes_response() {
    let folder = unique_temp_dir("ollama-request");
    let target = folder.join("download.pdf");
    fs::write(&target, "fixture").unwrap();
    let (endpoint, task) = mock_ollama(vec![(200, model_json("Example_Lab_Report.pdf"))]).await;
    let mut config = PromptConfig::default();
    config.system_prompt = "test system prompt".into();
    config.prompt_template = "CTX={folder_context}; EXT={extension}; TEXT={text_snippet}".into();
    config.temperature = 2.5;

    let result = suggest_filename_with_ollama_using_config(
        "Report\nOrganization: Example Lab".into(),
        "pdf".into(),
        Some("selected-model".into()),
        Some("folder overview".into()),
        target.to_string_lossy().into_owned(),
        config,
        &endpoint,
    )
    .await
    .unwrap();
    let requests = task.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["model"], "selected-model");
    assert_eq!(requests[0]["stream"], false);
    assert_eq!(requests[0]["options"]["temperature"], 2.0);
    assert_eq!(requests[0]["format"]["required"], serde_json::json!(["new_name", "category", "reason"]));
    assert!(requests[0]["system"].as_str().unwrap().contains("test system prompt"));
    assert!(requests[0]["prompt"].as_str().unwrap().contains("CTX=folder overview; EXT=pdf; TEXT=Report"));
    assert!(!requests[0]["prompt"].as_str().unwrap().contains("Confirmed document date"));
    assert_eq!(result.new_name, "Example_Lab_Report.pdf");
    assert_eq!(result.category, "Research");
    remove_temp_dir(&folder);
}

#[tokio::test]
async fn retries_one_rejected_model_name_and_returns_last_error_after_second_rejection() {
    let folder = unique_temp_dir("ollama-retry");
    let target = folder.join("download.txt");
    fs::write(&target, "fixture").unwrap();
    let (endpoint, task) = mock_ollama(vec![
        (200, model_json("document.txt")),
        (200, model_json("Example_Lab_Report.txt")),
    ]).await;
    let result = suggest_filename_with_ollama_using_config(
        "Report\nOrganization: Example Lab".into(),
        "txt".into(),
        None,
        None,
        target.to_string_lossy().into_owned(),
        PromptConfig::default(),
        &endpoint,
    )
    .await
    .unwrap();
    let requests = task.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[1]["prompt"].as_str().unwrap().contains("previous filename was rejected"));
    assert_eq!(result.new_name, "Example_Lab_Report.txt");

    let (endpoint, task) = mock_ollama(vec![
        (200, model_json("download.txt")),
        (200, model_json("file.txt")),
    ]).await;
    let error = suggest_filename_with_ollama_using_config(
        "Report\nOrganization: Example Lab".into(),
        "txt".into(),
        None,
        None,
        target.to_string_lossy().into_owned(),
        PromptConfig::default(),
        &endpoint,
    )
    .await
    .unwrap_err();
    assert!(error.contains("could not produce a usable filename"));
    assert_eq!(task.await.unwrap().len(), 2);
    remove_temp_dir(&folder);
}

#[tokio::test]
async fn ollama_connection_failure_returns_a_local_service_error() {
    let folder = unique_temp_dir("ollama-connection-error");
    let target = folder.join("download.txt");
    fs::write(&target, "fixture").unwrap();
    let error = suggest_filename_with_ollama_using_config(
        "Report\nOrganization: Example Lab".into(),
        "txt".into(),
        None,
        None,
        target.to_string_lossy().into_owned(),
        PromptConfig::default(),
        "http://127.0.0.1:0/api/generate",
    )
    .await
    .unwrap_err();
    assert!(error.contains("Failed to connect to Ollama"));
    remove_temp_dir(&folder);
}

#[tokio::test]
async fn ollama_http_error_is_reported_without_retry() {
    let folder = unique_temp_dir("ollama-http-error");
    let target = folder.join("download.txt");
    fs::write(&target, "fixture").unwrap();
    let (endpoint, task) = mock_ollama(vec![(503, "unavailable".into())]).await;
    let error = suggest_filename_with_ollama_using_config(
        "Report\nOrganization: Example Lab".into(),
        "txt".into(),
        None,
        None,
        target.to_string_lossy().into_owned(),
        PromptConfig::default(),
        &endpoint,
    )
    .await
    .unwrap_err();
    assert!(error.contains("503"));
    assert_eq!(task.await.unwrap().len(), 1);
    remove_temp_dir(&folder);
}

#[tokio::test]
#[ignore = "requires a local Ollama server with qwen2.5-coder:14b"]
async fn local_ollama_mixed_invoice_smoke() {
    let folder = unique_temp_dir("ollama-smoke");
    for name in [
        "download (20).pdf",
        "2025-11-12_Nordic_Engineering_Association_Lasku.pdf",
        "2024-02-13_Nordic_Engineering_Association_Lasku.pdf",
        "2025-07-14_Nordic_Engineering_Association_Invoice.pdf",
        "2026-03-12_Nordic_Engineering_Association_Report.pdf",
    ] {
        std::fs::write(folder.join(name), []).unwrap();
    }
    let result = suggest_filename_with_ollama(
        "INVOICE\nInvoice date: 2026-09-14\nSupplier: Nordic Engineering Association\nInvoice number: 10024665".to_string(),
        "pdf".to_string(),
        Some("qwen2.5-coder:14b".to_string()),
        Some(build_directory_blueprint(folder.to_string_lossy().into_owned()).unwrap()),
        folder.join("download (20).pdf").to_string_lossy().into_owned(),
        None,
    ).await;
    std::fs::remove_dir_all(&folder).unwrap();
    let result = result.expect("local model should suggest a usable filename");
    assert!(result.new_name.starts_with("2026-09-14_"), "{}", result.new_name);
    assert!(result.new_name.contains("Nordic") || result.new_name.contains("10024665"), "{}", result.new_name);
    assert!(!result.new_name.to_lowercase().contains("download"), "{}", result.new_name);
    assert!(result.new_name.to_lowercase().contains("lasku"), "{}", result.new_name);
}

#[tokio::test]
async fn cloud_ai_provider_missing_key_errors_cleanly() {
    let client = reqwest::Client::new();
    let schema = serde_json::json!({});

    // OpenAI without key
    let mut config = PromptConfig {
        provider: "openai".into(),
        openai_api_key: String::new(),
        ..Default::default()
    };
    // Do not alter the process environment while other tests are running.
    if ["OPENAI_API_KEY", "GEMINI_API_KEY", "ANTHROPIC_API_KEY"]
        .iter().any(|name| std::env::var_os(name).is_some()) { return; }
    let err = query_ai_provider(&client, &config, "gpt-4o-mini", "sys", "prompt", 0.2, &schema, None)
        .await
        .unwrap_err();
    assert!(err.contains("No openai API key configured"), "{}", err);

    // Gemini without key
    config.provider = "gemini".into();
    let err = query_ai_provider(&client, &config, "gemini-1.5-flash", "sys", "prompt", 0.2, &schema, None)
        .await
        .unwrap_err();
    assert!(err.contains("No gemini API key configured"), "{}", err);

    // Anthropic without key
    config.provider = "anthropic".into();
    let err = query_ai_provider(&client, &config, "claude-3-5-haiku", "sys", "prompt", 0.2, &schema, None)
        .await
        .unwrap_err();
    assert!(err.contains("No anthropic API key configured"), "{}", err);
}

async fn mock_provider_response(body: &str, status: u16) -> (String, tokio::task::JoinHandle<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let body = body.to_string();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0u8; 4096];
            let size = stream.read(&mut chunk).await.unwrap();
            assert!(size > 0);
            bytes.extend_from_slice(&chunk[..size]);
            if let Some(index) = find_subslice(&bytes, b"\r\n\r\n") { break index + 4; }
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().unwrap())
        }).unwrap();
        while bytes.len() < header_end + content_length {
            let mut chunk = vec![0u8; header_end + content_length - bytes.len()];
            let size = stream.read(&mut chunk).await.unwrap();
            assert!(size > 0);
            bytes.extend_from_slice(&chunk[..size]);
        }
        let response = format!("HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", status, body.len(), body);
        stream.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(bytes).unwrap()
    });
    (format!("http://{}/request", address), task)
}

#[tokio::test]
async fn cloud_providers_send_correct_headers_and_parse_responses() {
    let cases = [
        ("openai", "Bearer key-openai", r#"{"choices":[{"message":{"content":"{\"ok\":true}"}}]}"#),
        ("openrouter", "Bearer key-openrouter", r#"{"choices":[{"message":{"content":"{\"ok\":true}"}}]}"#),
        ("gemini", "x-goog-api-key: key-gemini", r#"{"candidates":[{"content":{"parts":[{"text":"{\"ok\":"},{"text":"true}"}]}}]}"#),
        ("anthropic", "x-api-key: key-anthropic", r#"{"content":[{"type":"text","text":"{\"ok\":true}"}]}"#),
    ];
    for (provider, expected_header, response) in cases {
        let (endpoint, task) = mock_provider_response(response, 200).await;
        let mut config = PromptConfig { provider: provider.into(), ..Default::default() };
        match provider {
            "openai" => config.openai_api_key = "key-openai".into(),
            "openrouter" => config.openrouter_api_key = "key-openrouter".into(),
            "gemini" => config.gemini_api_key = "key-gemini".into(),
            "anthropic" => config.anthropic_api_key = "key-anthropic".into(),
            _ => unreachable!(),
        }
        let output = query_ai_provider(&reqwest::Client::new(), &config, "sample-model", "system", "prompt", 0.2,
            &serde_json::json!({}), Some(&endpoint)).await.unwrap();
        assert_eq!(output, r#"{"ok":true}"#);
        let request = task.await.unwrap();
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        assert!(headers.to_lowercase().contains(&expected_header.to_lowercase()), "{}: {}", provider, headers);
        assert!(!headers.lines().next().unwrap().contains("key-"), "key leaked into URL");
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        if provider == "gemini" {
            assert_eq!(json["contents"][0]["parts"][0]["text"], "prompt");
            assert_eq!(json["generationConfig"]["responseMimeType"], "application/json");
        } else if provider == "anthropic" {
            assert_eq!(json["messages"][0]["content"], "prompt");
            assert!(headers.to_lowercase().contains("anthropic-version: 2023-06-01"));
        } else {
            assert_eq!(json["messages"][1]["content"], "prompt");
            assert_eq!(json["response_format"]["type"], "json_object");
        }
    }
}

#[test]
fn custom_endpoints_reject_insecure_remote_hosts_and_url_credentials() {
    assert!(validate_endpoint("https://api.example.test/v1/chat/completions").is_ok());
    assert!(validate_endpoint("http://localhost:11434/api/generate").is_ok());
    assert!(validate_endpoint("http://127.0.0.1:11434/api/generate").is_ok());
    assert!(validate_endpoint("http://api.example.test/v1/chat/completions").is_err());
    assert!(validate_endpoint("http://localhost.evil.test/request").is_err());
    assert!(validate_endpoint("https://user:pass@api.example.test/request").is_err());
    assert!(validate_endpoint("https://api.example.test/request?key=secret").is_err());
}

#[tokio::test]
async fn provider_errors_do_not_echo_server_content_or_credentials() {
    let (endpoint, task) = mock_provider_response("secret-key-in-error", 401).await;
    let config = PromptConfig { provider: "openai".into(), openai_api_key: "secret-key".into(), ..Default::default() };
    let error = query_ai_provider(&reqwest::Client::new(), &config, "model", "sys", "prompt", 0.2,
        &serde_json::json!({}), Some(&endpoint)).await.unwrap_err();
    assert!(error.contains("401"));
    assert!(!error.contains("secret-key"));
    task.await.unwrap();
}

#[tokio::test]
async fn connection_test_uses_synthetic_prompt_and_reports_success() {
    let (endpoint, task) = mock_provider_response(r#"{"choices":[{"message":{"content":"{\"ok\":true}"}}]}"#, 200).await;
    let config = PromptConfig { provider: "openai".into(), model: "gpt-5.4-mini".into(), openai_api_key: "test-key".into(), ..Default::default() };
    let status = test_ai_provider_with_config(&config, Some(&endpoint)).await.unwrap();
    assert!(status.contains("Connected to openai"));
    let request = task.await.unwrap();
    assert!(request.contains("Return {\\\"ok\\\":true}."));
    assert!(!request.contains("Document excerpt:"));
}

#[tokio::test]
async fn newer_models_omit_unsupported_temperature() {
    let cases = [
        ("openai", "gpt-5.4-mini", r#"{"choices":[{"message":{"content":"{}"}}]}"#),
        ("anthropic", "claude-haiku-5-5", r#"{"content":[{"type":"text","text":"{}"}]}"#),
        ("openrouter", "openrouter/auto", r#"{"choices":[{"message":{"content":"{}"}}]}"#),
    ];
    for (provider, model, response) in cases {
        let (endpoint, task) = mock_provider_response(response, 200).await;
        let mut config = PromptConfig { provider: provider.into(), ..Default::default() };
        match provider {
            "openai" => config.openai_api_key = "test-key".into(),
            "anthropic" => config.anthropic_api_key = "test-key".into(),
            "openrouter" => config.openrouter_api_key = "test-key".into(),
            _ => unreachable!(),
        }
        query_ai_provider(&reqwest::Client::new(), &config, model, "JSON", "{}", 0.2,
            &serde_json::json!({}), Some(&endpoint)).await.unwrap();
        let request = task.await.unwrap();
        let body: serde_json::Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert!(body.get("temperature").is_none(), "{} should omit temperature", model);
    }
}

#[tokio::test]
async fn stopping_a_run_aborts_its_in_flight_ai_request_without_affecting_the_next_run() {
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio::time::{timeout, Duration};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/api/generate", listener.local_addr().unwrap());
    let (accepted_sender, accepted_receiver) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.unwrap();
        let _ = accepted_sender.send(());
        tokio::time::sleep(Duration::from_secs(10)).await;
    });

    let run_id = begin_processing_run();
    let request = tokio::spawn(async move {
        let work = suggest_filename_with_ollama_using_config(
            "Report\nOrganization: Example Lab".into(), "txt".into(), None, None,
            "/tmp/cancellable-report.txt".into(), PromptConfig::default(), &endpoint,
        );
        run_for_processing_run(run_id, work).await
    });
    timeout(Duration::from_secs(2), accepted_receiver).await.unwrap().unwrap();
    assert!(cancel_processing_run(run_id));
    let error = timeout(Duration::from_secs(2), request).await.unwrap().unwrap().unwrap_err();
    assert_eq!(error, PROCESSING_STOPPED);
    server.abort();

    let next_run = begin_processing_run();
    assert_ne!(next_run, run_id);
    assert!(!cancel_processing_run(run_id));
    let result = run_for_processing_run(next_run, async { Ok::<_, String>(42) }).await.unwrap();
    assert_eq!(result, 42);
    assert!(cancel_processing_run(next_run));
}
