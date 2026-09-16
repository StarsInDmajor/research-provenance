//! rp-server: persistent RP graph backend with live reload.
//!
//! Wraps rp-core as a library, serves JSON APIs + the interactive graph HTML,
//! polls the project directory for changes, and pushes updates over SSE.
//! The Python projection engine (`graph_projection.py`) is spawned as a
//! subprocess to compute layout from canonical records; everything else is
//! native Rust with only existing workspace dependencies.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use rp_core::{
    ExecutionBudget, ProjectIndex, ProjectLimits, SnapshotDto, validate_project_with_budget,
};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(
    name = "rp-server",
    about = "Research Provenance live graph server",
    version
)]
struct Cli {
    /// Path to the RP project root (contains .research/).
    #[arg(long)]
    project: PathBuf,

    /// Listen address, e.g. 127.0.0.1:8400
    #[arg(long, default_value = "127.0.0.1:8400")]
    bind: String,

    /// Poll interval for project changes (milliseconds).
    #[arg(long, default_value_t = 1000)]
    poll_ms: u64,

    /// Path to the pilot-reader directory (contains graph_projection.py etc.).
    #[arg(long)]
    reader_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    project: PathBuf,
    reader_dir: PathBuf,
    index: Option<ProjectIndex>,
    graph_html: Option<String>,
    records: Option<String>,
    canonical: Option<serde_json::Value>,
    wire: Option<String>,
    objects: Option<String>,
    loaded_at: Option<String>,
    error: Option<String>,
    generation: u64,
}

impl State {
    fn reload(&mut self) {
        self.generation += 1;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.reload_inner()));
        if result.is_err() {
            self.error = Some("reload panicked".into());
        }
    }

    fn reload_inner(&mut self) {
        let budget = ExecutionBudget::default();
        match validate_project_with_budget(&self.project, ProjectLimits::default(), &budget) {
            Ok(report) if report.is_valid() => {
                let index = match report.index {
                    Some(i) => i,
                    None => {
                        self.error = Some("valid project produced no index".into());
                        return;
                    }
                };
                let now = jiff::Timestamp::now().to_string();
                match build_snapshot(&index, &now) {
                    Ok((canonical, snapshot_json)) => {
                        match run_projection(&self.reader_dir, &snapshot_json) {
                            Ok(graph) => {
                                match render_html(&self.reader_dir, &graph, &canonical, &now) {
                                    Ok(html) => {
                                        self.index = Some(index);
                                        self.graph_html = Some(html.clone());
                                        self.canonical = Some(canonical.clone());
                                        self.records = Some(snapshot_json);
                                        // Extract wire + records for live API
                                        match std::panic::catch_unwind(
                                            std::panic::AssertUnwindSafe(|| {
                                                extract_wire_and_records(&html)
                                            }),
                                        ) {
                                            Ok(Some((gd, fb))) => {
                                                self.wire = Some(gd);
                                                self.objects = Some(fb);
                                            }
                                            Ok(None) => {
                                                self.error =
                                                    Some("wire extract: tags not found".into());
                                            }
                                            Err(_) => {
                                                self.error = Some("wire extract: panic".into());
                                            }
                                        }
                                        self.loaded_at = Some(now);
                                        self.error = None;
                                    }
                                    Err(e) => self.error = Some(format!("render: {e}")),
                                }
                            }
                            Err(e) => self.error = Some(format!("projection: {e}")),
                        }
                    }
                    Err(e) => self.error = Some(format!("snapshot: {e}")),
                }
            }
            Ok(report) => {
                let msgs: Vec<String> = report
                    .findings
                    .iter()
                    .take(5)
                    .map(|f| f.message.clone())
                    .collect();
                self.error = Some(format!("validation failed: {}", msgs.join("; ")));
            }
            Err(e) => self.error = Some(format!("load error: {e:?}")),
        }
    }
}

fn build_snapshot(
    index: &ProjectIndex,
    as_of: &str,
) -> Result<(serde_json::Value, String), String> {
    let snap: SnapshotDto = index.snapshot(as_of).map_err(|e| format!("{e:?}"))?;
    let mut canonical = serde_json::to_value(&snap).map_err(|e| e.to_string())?;
    canonical
        .as_object_mut()
        .unwrap()
        .insert("schema".into(), "rp/cli-data/snapshot/v1".into());
    let s = serde_json::to_string(&canonical).map_err(|e| e.to_string())?;
    Ok((canonical, s))
}

// ---------------------------------------------------------------------------
// Python projection engine (subprocess, same as rp-view but piped)
// ---------------------------------------------------------------------------

fn run_projection(reader_dir: &Path, snapshot_json: &str) -> Result<serde_json::Value, String> {
    let script = format!(
        r#"import sys, json
sys.path.insert(0, {reader_dir:?})
import graph_projection as gp
snap = json.load(sys.stdin)
records = list(snap['objects'].values())
graph = gp.project(records, snap['threads'][0]['id'])
print(json.dumps(graph))
"#,
        reader_dir = reader_dir.display().to_string()
    );
    let py = find_python()?;
    let child = Command::new(py)
        .args(["-I", "-c", &script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn python: {e}"))?;
    child
        .stdin
        .as_ref()
        .unwrap()
        .write_all(snapshot_json.as_bytes())
        .map_err(|e| format!("write stdin: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("wait: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "projection exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("parse projection: {e}"))
}

fn find_python() -> Result<String, String> {
    for cand in ["python3", "python"] {
        if Command::new(cand)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Ok(cand.into());
        }
    }
    Err("python3 not found".into())
}

// ---------------------------------------------------------------------------
// HTML rendering (reuse reader template with computed CSP)
// ---------------------------------------------------------------------------

fn render_html(
    reader_dir: &Path,
    graph: &serde_json::Value,
    canonical: &serde_json::Value,
    as_of: &str,
) -> Result<String, String> {
    let template = std::fs::read_to_string(reader_dir.join("template.html"))
        .map_err(|e| format!("read template: {e}"))?;
    let css = std::fs::read_to_string(reader_dir.join("reader.css"))
        .map_err(|e| format!("read css: {e}"))?;
    let routing_js = std::fs::read_to_string(reader_dir.join("routing.js"))
        .map_err(|e| format!("read routing.js: {e}"))?;
    let graph_js = std::fs::read_to_string(reader_dir.join("graph.js"))
        .map_err(|e| format!("read graph.js: {e}"))?;
    let svg_js = std::fs::read_to_string(reader_dir.join("svg.js"))
        .map_err(|e| format!("read svg.js: {e}"))?;
    let bootstrap = std::fs::read_to_string(reader_dir.join("bootstrap.js"))
        .map_err(|e| format!("read bootstrap.js: {e}"))?;
    let js = [routing_js, graph_js, svg_js].join("\n");

    // Build wire data (same shape as build.py wire_data).
    let mut wire = serde_json::json!({
        "wireVersion": 1,
        "projectId": canonical["project"]["id"],
        "graph": graph,
        "sources": {},
        "notes": {},
    });
    // Strip 'raw' from nodes/edges for wire transport.
    if let Some(obj) = wire.get_mut("graph").and_then(|g| g.as_object_mut()) {
        if let Some(nodes) = obj.get_mut("nodes").and_then(|n| n.as_array_mut()) {
            for n in nodes {
                if let Some(o) = n.as_object_mut() {
                    o.remove("raw");
                    o.remove("title");
                    o.remove("kind");
                }
            }
        }
        if let Some(edges) = obj.get_mut("edges").and_then(|e| e.as_array_mut()) {
            for e in edges {
                if let Some(o) = e.as_object_mut() {
                    o.remove("raw");
                }
            }
        }
    }
    let data_json = serde_json::to_string(&wire).map_err(|e| e.to_string())?;
    let fallback_json = serde_json::to_string(&canonical["objects"]).map_err(|e| e.to_string())?;

    let hashes = [sha256_b64(&js), sha256_b64(&bootstrap)];
    let csp = format!(
        "default-src 'none'; script-src {}; style-src 'unsafe-inline'; img-src 'none'; connect-src 'self'; base-uri 'none'; form-action 'none'; object-src 'none'",
        hashes
            .iter()
            .map(|h| format!("'sha256-{h}'"))
            .collect::<Vec<_>>()
            .join(" ")
    );

    let fallback = format!(
        "<pre id=\"canonical-records\">{}</pre>",
        escape_pre(&fallback_json)
    );

    let mut html = template;
    replace_once(&mut html, "@@CSP@@", &escape_attr(&csp));
    replace_once(&mut html, "@@CSS@@", &css);
    replace_once(&mut html, "@@JS@@", &js);
    replace_once(&mut html, "@@BOOTSTRAP@@", &bootstrap);
    let data_tag = format!(
        "<script id=\"graph-data\" type=\"application/json\">{}</script>",
        escape_script(&data_json)
    );
    replace_once(&mut html, "@@DATA_TAG@@", &data_tag);
    replace_once(&mut html, "@@FALLBACK@@", &fallback);
    replace_once(&mut html, "@@AS_OF@@", &escape_text(as_of));
    replace_once(&mut html, "@@GENERATED_AT@@", &escape_text(as_of));
    replace_once(&mut html, "@@CASE_META@@", "RP live server · 实时探索");
    replace_once(
        &mut html,
        "@@BACKGROUND@@",
        "本地实时服务；展示已记录关系，数据来自 rp-core 快照。",
    );
    replace_once(
        &mut html,
        "@@VALIDATION_MODE@@",
        "经 rp 完整校验通过；服务实时重载。",
    );
    replace_once(&mut html, "@@COUNTS@@", "");
    replace_once(
        &mut html,
        "@@SVG@@",
        "<svg id=\"graph\" role=\"group\" aria-label=\"研究节点图；脚本启动后生成\"></svg>",
    );
    replace_once(&mut html, "@@GUIDE@@", "");
    replace_once(&mut html, "@@SOURCES@@", "");
    replace_once(&mut html, "@@EVIDENCE@@", "{}");
    Ok(html)
}

fn extract_wire_and_records(html: &str) -> Option<(String, String)> {
    // Extract the graph-data script content and canonical-records pre content.
    // Locate the closing delimiter of the opening tag itself so template tag
    // changes cannot shift silent offsets.
    let gd_open = html.find(r#"<script id="graph-data" type="application/json">"#)?;
    let gd_body = gd_open + r#"<script id="graph-data" type="application/json">"#.len();
    let gd_end = html[gd_body..].find("</script>")?;
    let wire = html[gd_body..gd_body + gd_end].to_string();
    let fb_open = html.find(r#"<pre id="canonical-records">"#)?;
    let fb_body = fb_open + r#"<pre id="canonical-records">"#.len();
    let fb_end = html[fb_body..].find("</pre>")?;
    let records = html[fb_body..fb_body + fb_end].to_string();
    Some((wire, records))
}

fn sha256_b64(s: &str) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(s.as_bytes());
    base64_encode(&h.finalize())
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(CHARS[(b[0] >> 2) as usize] as char);
        out.push(CHARS[(((b[0] & 3) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            CHARS[(((b[1] & 15) << 2) | (b[2] >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            CHARS[(b[2] & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn escape_pre(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;")
}
fn escape_script(s: &str) -> String {
    // Match build.py script_json: escape &, <, > and U+2028/9 so embedded
    // canonical text can never close the script tag or break JS string
    // contexts, while remaining valid JSON (\uXXXX forms).
    s.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}
fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;")
}
fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn replace_once(haystack: &mut String, needle: &str, replacement: &str) {
    if let Some(pos) = haystack.find(needle) {
        haystack.replace_range(pos..pos + needle.len(), replacement);
    }
}

// ---------------------------------------------------------------------------
// HTTP server (minimal, no external deps)
// ---------------------------------------------------------------------------

struct SseClient {
    stream: std::net::TcpStream,
}

fn handle_conn(
    mut stream: std::net::TcpStream,
    state: &Arc<Mutex<State>>,
    sse_clients: &Arc<Mutex<Vec<SseClient>>>,
) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }
    let method = parts[0];
    let path = parts[1];

    // Consume headers
    let mut headers = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
        headers.push(line.clone());
    }

    match (method, path) {
        ("GET", "/") | ("GET", "/graph.html") => {
            let st = state.lock().unwrap();
            match &st.graph_html {
                Some(html) => send_response(
                    &mut stream,
                    200,
                    "text/html; charset=utf-8",
                    html.as_bytes(),
                ),
                None => {
                    let msg = st.error.clone().unwrap_or_else(|| "not loaded".into());
                    send_response(&mut stream, 503, "text/plain", msg.as_bytes())
                }
            }
        }
        ("GET", "/api/snapshot") => {
            let st = state.lock().unwrap();
            match &st.canonical {
                Some(c) => {
                    let body = serde_json::to_string(c).unwrap_or_default();
                    send_response(&mut stream, 200, "application/json", body.as_bytes());
                }
                None => send_response(&mut stream, 503, "application/json", b"{}"),
            }
        }
        ("GET", "/api/graph") => {
            let st = state.lock().unwrap();
            match &st.graph_html {
                Some(_) => {
                    let wire = serde_json::json!({
                        "status": "ok",
                        "generation": st.generation,
                        "loaded_at": st.loaded_at,
                        "error": st.error,
                    });
                    let body = serde_json::to_string(&wire).unwrap_or_default();
                    send_response(&mut stream, 200, "application/json", body.as_bytes());
                }
                None => send_response(&mut stream, 503, "application/json", b"{}"),
            }
        }
        ("GET", "/api/version") => {
            let st = state.lock().unwrap();
            let body = serde_json::to_string(&serde_json::json!({
                "generation": st.generation,
                "loaded_at": st.loaded_at,
                "error": st.error,
                "project": st.project.display().to_string(),
            }))
            .unwrap_or_default();
            send_response(&mut stream, 200, "application/json", body.as_bytes());
        }
        ("GET", "/api/wire") => {
            let st = state.lock().unwrap();
            match &st.wire {
                Some(w) => send_response(&mut stream, 200, "application/json", w.as_bytes()),
                None => send_response(&mut stream, 503, "application/json", b"{}"),
            }
        }
        ("GET", "/api/records") => {
            let st = state.lock().unwrap();
            match &st.objects {
                Some(o) => send_response(&mut stream, 200, "application/json", o.as_bytes()),
                None => send_response(&mut stream, 503, "application/json", b"{}"),
            }
        }
        ("GET", "/api/events") => {
            let mut stream = stream;
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n",
            );
            let st = state.lock().unwrap();
            let generation = st.generation;
            let _ =
                stream.write_all(format!("data: {{\"generation\":{generation}}}\n\n").as_bytes());
            drop(st);
            sse_clients.lock().unwrap().push(SseClient { stream });
        }
        _ => send_response(&mut stream, 404, "text/plain", b"not found"),
    }
}

fn send_response(stream: &mut std::net::TcpStream, code: u16, ctype: &str, body: &[u8]) {
    let status = match code {
        200 => "200 OK",
        404 => "404 Not Found",
        503 => "503 Service Unavailable",
        _ => "500 Internal Server Error",
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();
    let reader_dir = cli.reader_dir.clone().unwrap_or_else(|| {
        // Default: ../tools/pilot-reader relative to the executable
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .map(|p| p.join("share/research-provenance/reader"))
            .unwrap_or_else(|| PathBuf::from("tools/pilot-reader"))
    });

    let state = Arc::new(Mutex::new(State {
        project: cli.project.clone(),
        reader_dir,
        index: None,
        graph_html: None,
        records: None,
        canonical: None,
        wire: None,
        objects: None,
        loaded_at: None,
        error: Some("initial load pending".into()),
        generation: 0,
    }));

    // Initial load
    state.lock().unwrap().reload();

    // Poll for changes
    let sse_clients: Arc<Mutex<Vec<SseClient>>> = Arc::new(Mutex::new(Vec::new()));
    let poll_state = Arc::clone(&state);
    let sse_clients_for_poll = Arc::clone(&sse_clients);
    let poll_interval = Duration::from_millis(cli.poll_ms);
    std::thread::spawn(move || {
        let mut last_mtime = dir_mtime(&poll_state.lock().unwrap().project);
        loop {
            std::thread::sleep(poll_interval);
            let current = dir_mtime(&poll_state.lock().unwrap().project);
            if current != last_mtime {
                last_mtime = current;
                let mut st = poll_state.lock().unwrap();
                st.reload();
                let generation = st.generation;
                drop(st);
                // Broadcast to all SSE clients
                let msg = format!("data: {{\"generation\":{generation}}}\n\n");
                let mut clients = sse_clients_for_poll.lock().unwrap();
                clients.retain_mut(|c| c.stream.write_all(msg.as_bytes()).is_ok());
                eprintln!("reload: generation {generation}");
            }
        }
    });

    // HTTP server
    let listener = TcpListener::bind(&cli.bind).expect("bind failed");
    eprintln!("rp-server listening on http://{}", cli.bind);
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let st = Arc::clone(&state);
                let clients = Arc::clone(&sse_clients);
                std::thread::spawn(move || handle_conn(stream, &st, &clients));
            }
            Err(_) => continue,
        }
    }
}

fn dir_mtime(root: &Path) -> Option<u128> {
    // Records nest (e.g. .research/records/questions/*.yaml), so a top-level
    // scan misses the most common edit. Walk bounded depth; DirEntry::metadata
    // is lstat, so symlinked entries never recurse.
    fn walk(dir: &Path, depth: u8, latest: &mut Option<u128>) {
        if depth > 6 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                walk(&entry.path(), depth + 1, latest);
            }
            if let Ok(mtime) = meta.modified() {
                let ts = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                *latest = Some(latest.map_or(ts, |l: u128| l.max(ts)));
            }
        }
    }
    let mut latest = None;
    for base in [root.join(".research"), root.join("sources")] {
        if base.exists() {
            walk(&base, 0, &mut latest);
        }
    }
    latest
}
