//! The page: one read-only page for the human, bound to 127.0.0.1 and opened
//! in the default browser.
//!
//! A call serves in its own process and ends with it, so closing the terminal
//! closes the page. `--background` starts a server of its own instead, prints
//! its address and returns, so a call through `harno run` ends at once;
//! `--stop` ends that one, and a second `--background` finds the first.
//!
//! The page is one HTML file with the session list embedded. The log stays on
//! disk and the page fetches it by piece, each piece once: the events of a
//! session, then the texts of one event when it is selected. A long session
//! does not blow the page up, and stepping through it costs one small request
//! per event.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::log::{file, harness_of, lines, sessions, text, valid_session, GATE};
use serde_json::{json, Value};

use crate::out::{truncate, Refusal, R};

const PAGE: &str = include_str!("../assets/page.html");

/// Where the background server is: `{pid, url}`, beside the log, out of git.
const STATE: &str = ".harno/log/analytics.json";

/// An answer: status, content type, body.
type Reply = (&'static str, &'static str, Vec<u8>);

pub fn serve(rest: &[String], project: &Path, out: &mut dyn Write) -> R<()> {
    let mut port = 0u16;
    let (mut open, mut stop, mut background) = (true, false, false);
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--port" => {
                let v = rest.get(i + 1).ok_or_else(|| Refusal::new("--port needs a value"))?;
                port = v.parse().map_err(|_| Refusal::new(format!("--port takes a number; got {v}")))?;
                i += 1;
            }
            "--no-open" => open = false,
            "--stop" => stop = true,
            "--background" => background = true,
            x => return Err(Refusal::new(format!("unknown option {x}")).instead("harno run analytics help")),
        }
        i += 1;
    }
    let say = |out: &mut dyn Write, text: String| {
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    };
    if stop {
        say(out, stop_server(project));
        return Ok(());
    }
    if !background {
        let listener = bind(port)?;
        let url = format!("http://{}", listener.local_addr()?);
        say(out, format!("serving {url}\n"));
        if open {
            open_browser(&url);
        }
        let project = project.to_path_buf();
        serve_routes(listener, move |path, query| handle(&project, path, query));
        return Ok(());
    }
    if let Some((pid, url)) = running(project) {
        say(out, format!("serving {url} (pid {pid}), already running; stop: harno run analytics --stop\n"));
        if open {
            open_browser(&url);
        }
        return Ok(());
    }
    // The port is taken here and let go for the server to take again, so a
    // busy port is refused before anything starts.
    let port = bind(port)?.local_addr()?.port();
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.args(["--no-open", "--port", &port.to_string()])
        .current_dir(project)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Its own process group: a Ctrl-C meant for the caller does not reach it.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn()?;
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        if child.try_wait()?.is_some() || started.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            return Err(Refusal::new(format!("the server did not start on 127.0.0.1:{port}")).instead("harno run analytics"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let url = format!("http://127.0.0.1:{port}");
    let state = project.join(STATE);
    if let Some(dir) = state.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&state, json!({"pid": child.id(), "url": url}).to_string())?;
    say(out, format!("serving {url} (pid {}); stop: harno run analytics --stop\n", child.id()));
    if open {
        open_browser(&url);
    }
    Ok(())
}

fn bind(port: u16) -> R<TcpListener> {
    TcpListener::bind(("127.0.0.1", port)).map_err(|e| Refusal::new(format!("cannot listen on 127.0.0.1:{port}: {e}")))
}

/// The background server of the state file, when it still answers.
fn running(project: &Path) -> Option<(u64, String)> {
    let state: Value = serde_json::from_str(&std::fs::read_to_string(project.join(STATE)).ok()?).ok()?;
    let pid = state.get("pid")?.as_u64()?;
    let url = state.get("url")?.as_str()?.to_string();
    TcpStream::connect(url.strip_prefix("http://")?).ok()?;
    Some((pid, url))
}

fn stop_server(project: &Path) -> String {
    let text = match running(project) {
        Some((pid, url)) => {
            let pid = pid.to_string();
            let killed = if cfg!(windows) {
                Command::new("taskkill").args(["/PID", &pid, "/F"]).stdout(Stdio::null()).stderr(Stdio::null()).status()
            } else {
                Command::new("kill").arg(&pid).stdout(Stdio::null()).stderr(Stdio::null()).status()
            };
            match killed {
                Ok(s) if s.success() => format!("stopped {url} (pid {pid})\n"),
                _ => format!("could not stop pid {pid}; {url} may still answer\n"),
            }
        }
        None => "no server running\n".to_string(),
    };
    let _ = std::fs::remove_file(project.join(STATE));
    text
}

/// Open `url` in the default browser. A failure is quiet: the banner has
/// already printed the address.
fn open_browser(url: &str) {
    let mut cmd = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else {
        Command::new("xdg-open")
    };
    if let Ok(mut child) = cmd.arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

fn ok(kind: &'static str, body: Vec<u8>) -> Reply {
    ("200 OK", kind, body)
}

fn not_found(what: &str) -> Reply {
    ("404 Not Found", "text/plain", format!("{what}\n").into_bytes())
}

/// Serve GET requests until the process ends: `route` answers each path and
/// query string; anything but GET is refused.
fn serve_routes(listener: TcpListener, route: impl Fn(&str, &str) -> Reply + Send + Sync + 
'static) {
    let route = Arc::new(route);
    for stream in listener.incoming().flatten() {
        let route = route.clone();
        std::thread::spawn(move || {
            let _ = answer(stream, &*route);
        });
    }
}

fn answer(mut stream: TcpStream, route: &dyn Fn(&str, &str) -> Reply) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let (status, kind, body) = if method == "GET" {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        route(path, query)
    } else {
        ("405 Method Not Allowed", "text/plain", b"read-only\n".to_vec())
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()
}

fn handle(project: &Path, path: &str, query: &str) -> Reply {
    match path {
        "/" | "/index.html" => {
            let embedded = serde_json::to_string(&json!({"sessions": sessions_json(project)})).unwrap_or_default().replace("</", "<\\/");
            ok("text/html; charset=utf-8", PAGE.replace("/*HARNO_DATA*/null", &embedded).into_bytes())
        }
        "/api/sessions" => ok("application/json", serde_json::to_vec(&sessions_json(project)).unwrap_or_default()),
        p if p.starts_with("/api/log/") => {
            let session = &p["/api/log/".len()..];
            if !valid_session(session) {
                return not_found("no such session");
            }
            // `?calls=<n>&events=<n>`: only the lines after those, for the live view.
            let since = |key: &str| -> u64 {
                query.split('&').find_map(|kv| kv.strip_prefix(key)).and_then(|n| n.parse().ok()).unwrap_or(0)
            };
            let after = |rows: Vec<Value>, n: u64| -> Vec<Value> {
                rows.into_iter().filter(|e| e.get("n").and_then(|x| x.as_u64()).unwrap_or(0) > n).collect()
            };
            let harness = harness_of(project, session);
            let calls = after(lines(&file(project, GATE, session)), since("calls="));
            let mut events = match &harness {
                Some(h) => after(lines(&file(project, h, session)), since("events=")),
                None => Vec::new(),
            };
            if calls.is_empty() && events.is_empty() && harness.is_none() && !file(project, GATE, session).is_file() {
                return not_found("no such session");
            }
            // What was written stays beside the line; the timeline shows it,
            // so it comes with the line.
            for e in &mut events {
                let kind = e.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
                if kind != "prompt" && kind != "response" {
                    continue;
                }
                let n = e.get("n").and_then(|x| x.as_u64()).unwrap_or(0);
                let said = harness.as_deref().and_then(|h| text(project, h, session, n, "out")).unwrap_or_default();
                if let Some(m) = e.as_object_mut() {
                    m.insert("text".into(), said.into());
                }
            }
            let body = json!({"harness": harness, "calls": calls, "events": events});
            ok("application/json", serde_json::to_vec(&body).unwrap_or_default())
        }
        // What a call of the gate returned, and what its commands wrote
        // before a filter cut it.
        p if p.starts_with("/api/call/") => {
            let Some((session, n)) = split_n(&p["/api/call/".len()..]) else { return not_found("no such call") };
            match text(project, GATE, &session, n, "out") {
                Some(out) => {
                    let raw = text(project, GATE, &session, n, "raw");
                    ok("application/json", serde_json::to_vec(&json!({"out": out, "raw": raw})).unwrap_or_default())
                }
                None => not_found("no such call"),
            }
        }
        // The input and the output of one line of a harness log.
        p if p.starts_with("/api/event/") => {
            let Some((session, n)) = split_n(&p["/api/event/".len()..]) else { return not_found("no such event") };
            let Some(harness) = harness_of(project, &session) else { return not_found("no such event") };
            let read = |ext: &str| text(project, &harness, &session, n, ext);
            match (read("in"), read("out")) {
                (None, None) => not_found("no such event"),
                (input, output) => ok("application/json", serde_json::to_vec(&json!({"in": input, "out": output})).unwrap_or_default()),
            }
        }
        _ => not_found("not found"),
    }
}

/// `<session>/<n>` of a request path.
fn split_n(rest: &str) -> Option<(String, u64)> {
    let (session, n) = rest.split_once('/')?;
    let n: u64 = n.parse().ok()?;
    valid_session(session).then(|| (session.to_string(), n))
}

fn sessions_json(project: &Path) -> Value {
    // The label is the first prompt, whole; the list only needs its start.
    json!(sessions(project)
        .iter()
        .map(|s| json!({"id": s.id, "ts": s.ts, "label": truncate(&s.label, 160), "gate_label": truncate(&s.gate_label, 160), "calls": s.calls, "harness": s.harness, "gate": s.gate, "last": s.last, "started": s.started, "ended": s.ended}))
        .collect::<Vec<_>>())
}
