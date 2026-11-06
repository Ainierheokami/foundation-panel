//! Upload of the pending crash reports: `POST <endpoint>/v1/crash`, multipart "meta" JSON + optional "dump".
//!
//! A report is deleted after a 2xx and after a 4xx the server will never accept (not 408/429); a network failure or 5xx leaves it for the
//! next pass. Never blocks the panel: it runs on its own thread with its own small runtime.

use super::meta;
use std::path::Path;
use std::time::Duration;

/// A report the server will not take later either.
pub(crate) fn settled(status: u16) -> bool {
    (200..300).contains(&status) || ((400..500).contains(&status) && status != 408 && status != 429)
}

/// The body of a multipart/form-data request with the "meta" part and, when there is one, the "dump" part.
pub(crate) fn multipart_body(boundary: &str, meta_json: &[u8], dump: Option<&[u8]>) -> Vec<u8> {
    let mut body = Vec::with_capacity(meta_json.len() + dump.map_or(0, <[u8]>::len) + 512);
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"meta\"\r\nContent-Type: application/json\r\n\r\n").as_bytes());
    body.extend_from_slice(meta_json);
    body.extend_from_slice(b"\r\n");
    if let Some(dump) = dump {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"dump\"; filename=\"crash.dmp\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(dump);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

/// `install_id` goes into the meta when the writer left it out.
pub(crate) fn meta_with_identity(text: &[u8], install_id: &str) -> Option<Vec<u8>> {
    let mut value: serde_json::Value = serde_json::from_slice(text).ok()?;
    let object = value.as_object_mut()?;
    if !object.contains_key("install_id") && !install_id.is_empty() {
        object.insert("install_id".into(), install_id.into());
    }
    object.entry("v").or_insert(1.into());
    serde_json::to_vec(&value).ok()
}

/// One pass over the pending folder; returns how many reports were delivered or rejected for good.
pub(crate) async fn upload_pending_once(dir: &Path, endpoint: &str, install_id: &str) -> usize {
    meta::trim_pending(dir, meta::MAX_PENDING, 30);
    let Ok(client) = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("FoundationSunshinePanel-crash/1")
        .build()
    else {
        return 0;
    };
    let url = format!("{}/v1/crash", endpoint.trim_end_matches('/'));
    let mut done = 0;
    let own_tag = format!("-p{:08x}.", std::process::id());
    for report in meta::list_pending(dir) {
        // one this process is still writing
        let young = report.modified.elapsed().map_or(true, |age| age < Duration::from_secs(10));
        if young && report.meta.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains(&own_tag)) {
            continue;
        }
        let Ok(text) = std::fs::read(&report.meta) else { continue };
        let Some(body_meta) = meta_with_identity(&text, install_id) else {
            let _ = std::fs::remove_file(&report.meta);
            if let Some(d) = &report.dump {
                let _ = std::fs::remove_file(d);
            }
            continue;
        };
        let dump = report
            .dump
            .as_ref()
            .filter(|d| std::fs::metadata(d).map_or(false, |m| m.len() <= meta::MAX_DUMP_BYTES))
            .and_then(|d| std::fs::read(d).ok())
            .map(|mut bytes| {
                // the dump type keeps heap and data out; this blanks the account and computer names that a path or a stack word still carries
                super::redact::redact_private_names(&mut bytes, &super::redact::private_names());
                bytes
            });
        let boundary = format!("alk-crash-{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
        let body = multipart_body(&boundary, &body_meta, dump.as_deref());
        let status = match client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
            .body(body)
            .send()
            .await
        {
            Ok(response) => response.status().as_u16(),
            Err(_) => 0,
        };
        // 429 (a per-install limit) and 503 (storage failed, nothing counted): keep everything and stop this pass
        if status == 429 || status == 503 {
            break;
        }
        if !settled(status) {
            continue;
        }
        let _ = std::fs::remove_file(&report.meta);
        if let Some(d) = &report.dump {
            let _ = std::fs::remove_file(d);
        }
        done += 1;
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn what_the_server_cannot_take_yet_stays() {
        for status in [0u16, 408, 429, 500, 502, 503] {
            assert!(!settled(status), "{status}");
        }
        for status in [200u16, 204, 400, 404, 413, 422] {
            assert!(settled(status), "{status}");
        }
    }

    #[test]
    fn multipart_has_the_named_parts() {
        let body = multipart_body("B", b"{\"v\":1}", Some(b"MDMPxx"));
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--B\r\n"));
        assert!(text.contains("name=\"meta\""));
        assert!(text.contains("name=\"dump\""));
        assert!(text.contains("MDMPxx"));
        assert!(text.ends_with("--B--\r\n"));
        assert!(!String::from_utf8_lossy(&multipart_body("B", b"{}", None)).contains("name=\"dump\""));
    }

    #[test]
    fn identity_is_added_once() {
        let out = meta_with_identity(b"{\"product\":\"panel\"}", "id-1").unwrap();
        let j: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(j["install_id"], "id-1");
        assert_eq!(j["v"], 1);
        let kept = meta_with_identity(b"{\"install_id\":\"mine\"}", "id-1").unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&kept).unwrap()["install_id"], "mine");
        assert!(meta_with_identity(b"not json", "id-1").is_none());
    }

    /// A one-shot HTTP server that answers every request with  and keeps the bodies.
    fn mock(status: u16) -> (String, Arc<Mutex<Vec<Vec<u8>>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut data = Vec::new();
                let mut chunk = [0u8; 8192];
                let mut header_end = None;
                let mut length = 0usize;
                loop {
                    let Ok(n) = stream.read(&mut chunk) else { break };
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&chunk[..n]);
                    if header_end.is_none() {
                        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                            header_end = Some(at + 4);
                            let head = String::from_utf8_lossy(&data[..at]).to_ascii_lowercase();
                            length = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                        }
                    }
                    if header_end.is_some_and(|h| data.len() >= h + length) {
                        break;
                    }
                }
                if let Some(h) = header_end {
                    kept.lock().unwrap().push(data[h..].to_vec());
                }
                let _ = stream.write_all(format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes());
            }
        });
        (format!("http://127.0.0.1:{port}"), seen)
    }

    fn pending_report(dir: &Path, name: &str, dump: bool) {
        let frames = [meta::Frame::new("sunshine-gui.exe", 0x1234)];
        let json = meta::render_meta(&meta::MetaInput { product: "panel", version: "1", os_version: "w", install_id: "", kind: "panic", code: "panic", time: 1_700_000_000, uptime_s: 1, frames: &frames }).unwrap();
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(format!("{name}.json")), json.as_bytes()).unwrap();
        if dump {
            std::fs::write(dir.join(format!("{name}.dmp")), b"MDMP-body").unwrap();
        }
        let f = std::fs::File::options().write(true).open(dir.join(format!("{name}.json"))).unwrap();
        f.set_modified(std::time::SystemTime::now() - Duration::from_secs(300)).unwrap();
    }

    fn run<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    #[test]
    fn delivers_and_deletes_after_2xx() {
        let (url, seen) = mock(200);
        let dir = tempfile::tempdir().unwrap();
        pending_report(dir.path(), "a", true);
        pending_report(dir.path(), "b", false);
        assert_eq!(run(upload_pending_once(dir.path(), &url, "11111111-2222-4333-8444-555555555555")), 2);
        assert!(meta::list_pending(dir.path()).is_empty());
        let bodies = seen.lock().unwrap();
        assert_eq!(bodies.len(), 2);
        let first = String::from_utf8_lossy(&bodies[0]);
        assert!(first.contains("name=\"meta\"") && first.contains("name=\"dump\"") && first.contains("MDMP-body"));
        assert!(first.contains("\"install_id\":\"11111111-2222-4333-8444-555555555555\""));
        assert!(!String::from_utf8_lossy(&bodies[1]).contains("name=\"dump\""));
    }

    #[test]
    fn a_server_error_keeps_the_report_and_a_rejection_drops_it() {
        let dir = tempfile::tempdir().unwrap();
        pending_report(dir.path(), "a", true);
        let (url, _) = mock(503);
        assert_eq!(run(upload_pending_once(dir.path(), &url, "x")), 0);
        assert_eq!(meta::list_pending(dir.path()).len(), 1);
        let (url, _) = mock(400);
        assert_eq!(run(upload_pending_once(dir.path(), &url, "x")), 1);
        assert!(meta::list_pending(dir.path()).is_empty());
    }

    #[test]
    fn no_server_keeps_the_report() {
        let dir = tempfile::tempdir().unwrap();
        pending_report(dir.path(), "a", true);
        assert_eq!(run(upload_pending_once(dir.path(), "http://127.0.0.1:1", "x")), 0);
        assert_eq!(meta::list_pending(dir.path()).len(), 1);
    }
}
