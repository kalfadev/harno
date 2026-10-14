//! The page and its routes: the session list, the two logs of one session,
//! and the texts beside their lines. The logs are written by hand, since
//! analytics reads the files of `.harno/log/` and nothing else.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

const SESSION: &str = "0196aaaa-0000-7000-8000-000000000001";

/// A project whose log holds one session: the calls of the gate, and beside
/// them the words of a Claude Code session that made them.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let write = |rel: String, text: String| {
        let p = dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    let calls = [
        r#"{"n":1,"ts":"2026-01-01T10:00:02.000Z","ms":12,"cwd":".","verb":"run","args":["say","--","hi"],"cmd":["echo hi"],"exit":[0],"output":392,"filtered":3,"lines":100,"kept":1}"#,
        r#"{"n":2,"ts":"2026-01-01T10:00:06.000Z","ms":2,"cwd":".","verb":"run","args":["curl","https://x"],"refused":"run.allow does not list curl"}"#,
        "",
    ]
    .join("\n");
    let events = [
        r#"{"n":1,"ts":"2026-01-01T10:00:00.000Z","type":"session","event":"start","source":"test"}"#,
        r#"{"n":2,"ts":"2026-01-01T10:00:01.000Z","type":"prompt","chars":5}"#,
        r#"{"n":3,"ts":"2026-01-01T10:00:03.000Z","type":"tool","tool":"Bash","summary":"harno run say -- hi","turn":"m1","harno":[1]}"#,
        r#"{"n":4,"ts":"2026-01-01T10:00:05.000Z","type":"session","event":"end"}"#,
        "",
    ]
    .join("\n");
    write(format!(".harno/log/harno/{SESSION}.jsonl"), calls);
    write(format!(".harno/log/harno/{SESSION}/0001.out"), "hi\n".to_string());
    write(
        format!(".harno/log/harno/{SESSION}/0001.raw"),
        (1..=100).map(|n| n.to_string()).collect::<Vec<_>>().join("\n") + "\n",
    );
    write(format!(".harno/log/claude-code/{SESSION}.jsonl"), events);
    write(format!(".harno/log/claude-code/{SESSION}/0002.out"), "first".to_string());
    write(
        format!(".harno/log/claude-code/{SESSION}/0003.in"),
        "{\n  \"command\": \"harno run say -- hi\"\n}".to_string(),
    );
    write(format!(".harno/log/claude-code/{SESSION}/0003.out"), String::new());
    dir
}

fn get(port: u16, path: &str) -> (String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).unwrap();
    let (head, body) = buf.split_once("\r\n\r\n").unwrap();
    (head.to_string(), body.to_string())
}

/// analytics in this process, its banner sent back as it is printed.
fn serve_in(dir: &Path, port: u16) -> std::sync::mpsc::Receiver<String> {
    let root = dir.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        struct Tee(std::sync::mpsc::Sender<String>);
        impl Write for Tee {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                let _ = self.0.send(String::from_utf8_lossy(b).to_string());
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let args = vec!["--port".to_string(), port.to_string(), "--no-open".to_string()];
        analytics::main(args, root, &mut Tee(tx));
    });
    rx
}

fn call(dir: &Path, args: &[&str]) -> (String, i32) {
    let mut buf: Vec<u8> = Vec::new();
    let code = analytics::main(args.iter().map(|s| s.to_string()).collect(), dir.to_path_buf(), &mut buf);
    (String::from_utf8(buf).unwrap(), code)
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[test]
fn the_page_and_the_routes_serve_the_log_on_localhost() {
    let p = project();
    let port = free_port();
    let rx = serve_in(p.path(), port);
    let banner = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(banner, format!("serving http://127.0.0.1:{port}\n"));

    let (head, page) = get(port, "/");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(page.contains(SESSION), "the session list is embedded");
    assert!(!page.contains("cytoscape"), "the page is the log alone");
    assert_eq!(page.matches("<main>").count(), 1, "one page");
    assert_eq!(page.matches("const esc").count(), 1, "one script");

    let (_, list) = get(port, "/api/sessions");
    let list: serde_json::Value = serde_json::from_str(&list).unwrap();
    let s = list.as_array().unwrap().iter().find(|s| s["id"] == SESSION).unwrap();
    assert_eq!(s["label"], "first", "the label is the first prompt");
    assert_eq!(s["calls"], 2);
    assert_eq!(s["started"], true, "a session line opened it");
    assert_eq!(s["ended"], true, "and ended it");
    assert!(s["last"].as_str().unwrap() >= s["ts"].as_str().unwrap(), "{s}");

    let (_, body) = get(port, &format!("/api/log/{SESSION}"));
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["harness"], "claude-code");
    assert_eq!(body["events"].as_array().unwrap().len(), 4);
    assert_eq!(body["events"][1]["text"], "first", "what was written comes with the line");
    assert_eq!(body["calls"].as_array().unwrap().len(), 2);
    assert_eq!(body["calls"][0]["verb"], "run");
    assert_eq!(body["events"][2]["harno"], serde_json::json!([1]), "the tool call claims the call it made");

    // One request per line: the texts beside it, together.
    let (_, c) = get(port, &format!("/api/call/{SESSION}/1"));
    let c: serde_json::Value = serde_json::from_str(&c).unwrap();
    assert_eq!(c["out"], "hi\n");
    assert!(c["raw"].as_str().unwrap().ends_with("\n100\n"), "what the command wrote comes with it");
    let (_, e) = get(port, &format!("/api/event/{SESSION}/3"));
    let e: serde_json::Value = serde_json::from_str(&e).unwrap();
    assert!(e["in"].as_str().unwrap().contains("\"command\": \"harno run say -- hi\""), "{e}");
    assert_eq!(e["out"], "");
    let (head, _) = get(port, &format!("/api/call/{SESSION}/99"));
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");

    // The live view asks only for the lines after the last it has.
    let (_, later) = get(port, &format!("/api/log/{SESSION}?calls=1&events=3"));
    let later: serde_json::Value = serde_json::from_str(&later).unwrap();
    assert_eq!(later["calls"].as_array().unwrap().len(), 1);
    assert_eq!(later["calls"][0]["n"], 2);
    assert_eq!(later["events"].as_array().unwrap().len(), 1);
    assert_eq!(later["events"][0]["n"], 4);

    let (head, _) = get(port, "/api/call/..%2F..%2Fconfig.yml/1");
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");
    let (head, _) = get(port, "/api/log/..%2Fconfig");
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");

    // Read-only.
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "POST / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).unwrap();
    assert!(buf.starts_with("HTTP/1.1 405"), "{buf}");
}

#[test]
fn the_server_runs_in_the_background_until_stop() {
    let p = project();
    // The binary itself: the server is a process of its own.
    let analytics = |args: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_analytics")).args(args).current_dir(p.path()).output().unwrap();
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    let out = analytics(&["--background", "--no-open"]);
    assert!(out.starts_with("serving http://127.0.0.1:") && out.ends_with("; stop: harno run analytics --stop\n"), "{out}");
    let url = out["serving ".len()..].split(' ').next().unwrap().to_string();
    let port: u16 = url.rsplit(':').next().unwrap().parse().unwrap();
    let (head, _) = get(port, "/");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");

    let again = analytics(&["--background", "--no-open"]);
    assert!(again.starts_with(&format!("serving {url} (pid ")) && again.contains("already running"), "{again}");

    let stop = analytics(&["--stop"]);
    assert!(stop.starts_with(&format!("stopped {url} (pid ")), "{stop}");
    let gone = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(20));
        TcpStream::connect(("127.0.0.1", port)).is_err()
    });
    assert!(gone, "the server still answers on {port}");
    assert_eq!(analytics(&["--stop"]), "no server running\n");
}

#[test]
fn a_bad_option_and_a_directory_without_a_log_are_refused() {
    let p = project();
    assert!(call(p.path(), &["--port", "http"]).0.contains("--port takes a number; got http"));
    assert!(call(p.path(), &["--port", "70000"]).0.contains("--port takes a number; got 70000"));
    assert!(call(p.path(), &["--port"]).0.contains("--port needs a value"));
    let (out, code) = call(p.path(), &["--host"]);
    assert!(out.starts_with("refused: unknown option --host\ninstead: harno run analytics help\n"), "{out}");
    assert_eq!(code, 1);
    let empty = tempfile::tempdir().unwrap();
    let (out, code) = call(empty.path(), &[]);
    assert_eq!(out, "refused: no .harno/log here or in any parent directory\n");
    assert_eq!(code, 1);
    let (out, code) = call(empty.path(), &["help"]);
    assert!(out.starts_with("usage: harno run analytics"), "{out}");
    assert_eq!(code, 0);
}
