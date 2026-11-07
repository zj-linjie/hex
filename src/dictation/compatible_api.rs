//! OpenAI-compatible, non-streaming text generation for dictation.
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use serde::Deserialize;

static ENABLED: AtomicBool = AtomicBool::new(false);
pub(crate) fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Release);
}
pub(super) fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Read a private JSON credential without ever persisting its contents in settings.
pub(super) fn credential(path: &str, base: &str) -> Result<Option<String>> {
    if path.trim().is_empty() {
        return Ok(None);
    }
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| eyre!("Cannot open credential file"))?;
        let metadata = file
            .metadata()
            .map_err(|_| eyre!("Cannot inspect credential file"))?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(eyre!("Credential file must be owned by you and private"));
        }
        file
    };
    #[cfg(not(unix))]
    let file = std::fs::File::open(path).map_err(|_| eyre!("Cannot open credential file"))?;
    let mut data = Vec::new();
    file.take(65537)
        .read_to_end(&mut data)
        .map_err(|_| eyre!("Cannot read credential file"))?;
    if data.len() > 65536 {
        return Err(eyre!("Credential file is too large"));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&data).map_err(|_| eyre!("Credential file must be JSON"))?;
    if let Some(expected) = json.get("url").and_then(|value| value.as_str()) {
        let expected = url::Url::parse(expected)
            .map_err(|_| eyre!("Credential file has an invalid service URL"))?;
        let actual = url::Url::parse(base).map_err(|_| eyre!("Invalid model service URL"))?;
        let deepseek_console = expected.scheme() == "https"
            && expected.host_str() == Some("platform.deepseek.com")
            && expected.port_or_known_default() == Some(443)
            && actual.scheme() == "https"
            && actual.host_str() == Some("api.deepseek.com")
            && actual.port_or_known_default() == Some(443);
        if expected.origin() != actual.origin() && !deepseek_console {
            return Err(eyre!("Credential file belongs to a different service"));
        }
    }
    let key = json
        .get("api_key")
        .or_else(|| json.get("key"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| eyre!("Credential file has no api_key"))?;
    Ok(Some(key.to_owned()))
}

#[derive(Deserialize)]
struct Reply {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    finish_reason: Option<String>,
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: Option<String>,
}

/// Only HTTPS or an explicitly local HTTP endpoint may receive transcripts.
fn endpoint(base: &str) -> Result<String> {
    let mut url = url::Url::parse(base).map_err(|_| eyre!("Invalid model service URL"))?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && local))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(eyre!(
            "Model service requires HTTPS or a local HTTP address"
        ));
    }
    let path = format!("{}/chat/completions", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url.into())
}

fn agent(url: &str, deadline: Duration) -> ureq::Agent {
    let local = url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]"));
    let configure =
        |config: crate::net::Config| config.timeout_global(Some(deadline)).max_redirects(0);
    if local {
        crate::net::loopback_agent(configure)
    } else {
        crate::net::agent(url, configure)
    }
}

pub(crate) fn models(base: &str, key_path: &str) -> Result<Vec<String>> {
    let url = endpoint(base)?
        .trim_end_matches("/chat/completions")
        .to_owned()
        + "/models";
    let key = credential(key_path, base)?;
    let agent = agent(&url, Duration::from_secs(10));
    let mut request = agent.get(&url);
    if let Some(key) = key {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    let mut response = request.call().map_err(|error| {
        eyre!(
            "Model service request failed: {}",
            crate::net::reason(&error)
        )
    })?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| eyre!("Could not read model list"))?;
    if bytes.len() > 1_048_576 {
        return Err(eyre!("Model list is too large"));
    }
    #[derive(Deserialize)]
    struct Catalog {
        data: Vec<Model>,
    }
    #[derive(Deserialize)]
    struct Model {
        id: String,
    }
    let catalog: Catalog = serde_json::from_slice(&bytes)
        .map_err(|_| eyre!("Model service did not provide a model list"))?;
    Ok(catalog
        .data
        .into_iter()
        .take(100)
        .map(|model| model.id)
        .filter(|id| id.len() <= 200)
        .collect())
}

pub(super) fn generate(
    base: &str,
    model: &str,
    key: Option<&str>,
    prompt: &str,
    deadline: Duration,
) -> Result<String> {
    if model.trim().is_empty() {
        return Err(eyre!("Choose a text model"));
    }
    let url = endpoint(base)?;
    let agent = agent(&url, deadline);
    let mut request = agent.post(&url).header("Content-Type", "application/json");
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    let mut body = serde_json::json!({"model": model, "messages": [{"role":"user", "content":prompt}], "stream":false});
    if url::Url::parse(&url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .as_deref()
        == Some("api.deepseek.com")
    {
        body["thinking"] = serde_json::json!({"type":"disabled"});
    }
    let mut response = request.send(body.to_string().as_bytes()).map_err(|error| {
        eyre!(
            "Model service request failed: {}",
            crate::net::reason(&error)
        )
    })?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| eyre!("Could not read model response"))?;
    if bytes.len() > 1_048_576 {
        return Err(eyre!("Model response is too large"));
    }
    let reply: Reply = serde_json::from_slice(&bytes)
        .map_err(|_| eyre!("Model service returned an invalid response"))?;
    let choice = reply
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| eyre!("Model service returned no choices"))?;
    if choice
        .finish_reason
        .as_deref()
        .is_some_and(|reason| reason != "stop")
    {
        return Err(eyre!("Model response was incomplete"));
    }
    let text = choice.message.content.unwrap_or_default();
    if text.trim().is_empty() {
        return Err(eyre!("Model returned empty text"));
    }
    Ok(text.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_endpoint_and_appends_route() {
        assert_eq!(
            endpoint("https://api.deepseek.com/v1/").unwrap(),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert!(endpoint("http://example.com/v1").is_err());
        assert!(endpoint("https://user:secret@example.com").is_err());
        assert!(endpoint("http://127.0.0.1:11434/v1").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn private_credentials_are_bound_to_their_service() {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let path = std::env::temp_dir().join(format!("hex-compatible-key-{}", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        write!(
            file,
            "{{\"url\":\"https://example.test/v1\",\"key\":\"example-key\"}}"
        )
        .unwrap();
        drop(file);
        let path_str = path.to_str().unwrap();
        assert_eq!(
            credential(path_str, "https://example.test/v1")
                .unwrap()
                .as_deref(),
            Some("example-key")
        );
        assert!(credential(path_str, "https://other.test/v1").is_err());
        std::fs::write(
            &path,
            b"{\"url\":\"https://platform.deepseek.com/api_keys\",\"key\":\"example-key\"}",
        )
        .unwrap();
        assert!(credential(path_str, "https://api.deepseek.com/v1").is_ok());
        assert!(credential(path_str, "https://other.test/v1").is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(credential(path_str, "https://example.test/v1").is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn sends_chat_completion_and_reads_text() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&request);
                if text.contains("test words") {
                    break;
                }
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("POST /v1/chat/completions "));
            assert!(request.contains("test-model"));
            let body = r#"{"choices":[{"finish_reason":"stop","message":{"content":"  polished text  "}}]}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let text = generate(
            &format!("http://{addr}/v1"),
            "test-model",
            None,
            "test words",
            Duration::from_secs(3),
        )
        .unwrap();
        assert_eq!(text, "polished text");
        server.join().unwrap();
    }

    #[test]
    #[ignore = "requires an explicitly supplied credential and a real model service"]
    fn live_compatible_service() {
        let path = std::env::var("HEX_COMPATIBLE_TEST_KEY_PATH").unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let override_url = std::env::var("HEX_COMPATIBLE_TEST_BASE_URL").ok();
        let base = override_url
            .as_deref()
            .unwrap_or_else(|| json["url"].as_str().unwrap());
        let model =
            std::env::var("HEX_COMPATIBLE_TEST_MODEL").unwrap_or_else(|_| "glm-5.3-flash".into());
        let key = credential(&path, base).unwrap();
        let answer = generate(
            base,
            &model,
            key.as_deref(),
            "Return only the word OK.",
            Duration::from_secs(30),
        )
        .unwrap();
        assert!(!answer.is_empty());
    }
}
