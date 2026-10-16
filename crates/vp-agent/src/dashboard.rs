// SPDX-License-Identifier: MIT OR Apache-2.0
//! Explicit, bounded localhost sessions. No service is installed.
use axum::{
    extract::{
        ws::{Message, WebSocketUpgrade},
        DefaultBodyLimit, Query, Request, State,
    },
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{watch, Mutex, Semaphore};
use vp_analyze::{Provider, MAX_TRACE};

type ApiResult = Result<Json<Value>, (StatusCode, String)>;
struct Capture {
    records: Vec<Value>,
    active: bool,
    previous: Option<(u64, Instant)>,
    identity: Option<u64>,
    started: Instant,
}
struct App {
    token: String,
    origin: String,
    pid: Option<i32>,
    tree: bool,
    capture: Mutex<Capture>,
    shutdown: watch::Sender<bool>,
    clients: Arc<Semaphore>,
    analysis: Semaphore,
    deadline: Instant,
}
fn failure(message: impl ToString) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, message.to_string())
}
fn equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

async fn guard(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let h = request.headers();
    let host = app.origin.strip_prefix("http://").unwrap();
    if h.get("host").and_then(|v| v.to_str().ok()) != Some(host) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(origin) = h.get("origin") {
        if origin.to_str().ok() != Some(&app.origin) {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    if request.uri().path().starts_with("/api/") {
        let bearer = h
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "));
        let socket = h
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').map(str::trim).find(|s| equal(s, &app.token)));
        if !bearer.or(socket).is_some_and(|s| equal(s, &app.token)) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert("cache-control", "no-store".parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("referrer-policy", "no-referrer".parse().unwrap());
    headers.insert("content-security-policy","default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'; form-action 'none'".parse().unwrap());
    response
}
fn asset(mime: &str, body: &'static str) -> Response {
    ([("content-type", mime)], body).into_response()
}
async fn status(app: &App) -> Value {
    let capture = app.capture.lock().await;
    json!({"active":capture.active,"pid":app.pid,"remaining":app.deadline.saturating_duration_since(Instant::now()).as_secs(),"records":capture.records.len(),"source":capture.records.first().map(|v|v["source"].clone()),"metrics":capture.records.iter().rev().find(|v|v["type"]=="metrics"),"capabilities":{"native":"process CPU, RSS and PSS via /proc at 1Hz","frames":false,"nativeStacks":false}})
}
fn proc_stat(pid: i32) -> anyhow::Result<(u64, u64)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let fields: Vec<_> = text
        .rsplit_once(") ")
        .ok_or_else(|| anyhow::anyhow!("Invalid process stat"))?
        .1
        .split_whitespace()
        .collect();
    if fields.first() == Some(&"Z") {
        anyhow::bail!("Target exited");
    }
    Ok((
        fields
            .get(11)
            .ok_or_else(|| anyhow::anyhow!("Missing user ticks"))?
            .parse::<u64>()?
            + fields
                .get(12)
                .ok_or_else(|| anyhow::anyhow!("Missing kernel ticks"))?
                .parse::<u64>()?,
        fields
            .get(19)
            .ok_or_else(|| anyhow::anyhow!("Missing start time"))?
            .parse()?,
    ))
}
fn memory(pid: i32) -> (Option<u64>, Option<u64>) {
    let text = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).unwrap_or_default();
    let value = |name: &str| {
        text.lines()
            .find_map(|line| {
                line.strip_prefix(name)?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
            .map(|n| n * 1024)
    };
    let rss = value("Rss:").or_else(|| {
        std::fs::read_to_string(format!("/proc/{pid}/status"))
            .ok()?
            .lines()
            .find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
            .map(|n| n * 1024)
    });
    (rss, value("Pss:"))
}
/// The root PID plus all of its descendants, from the parent links in `/proc`.
fn process_tree(root: i32) -> Vec<i32> {
    let mut children: std::collections::HashMap<i32, Vec<i32>> = std::collections::HashMap::new();
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<i32>().ok())
            else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                continue;
            };
            let Some(ppid) = text
                .rsplit_once(") ")
                .and_then(|(_, rest)| rest.split_whitespace().nth(1))
                .and_then(|v| v.parse::<i32>().ok())
            else {
                continue;
            };
            children.entry(ppid).or_default().push(pid);
        }
    }
    let mut all = vec![root];
    let mut next = 0;
    while next < all.len() {
        if let Some(kids) = children.get(&all[next]) {
            all.extend(kids.iter().copied());
        }
        next += 1;
    }
    all
}
/// CPU ticks and start-time identity of the root, with CPU ticks summed over the tree when asked.
/// Only the root's failure is an error: children come and go.
fn total_stat(pid: i32, tree: bool) -> anyhow::Result<(u64, u64)> {
    let (mut ticks, identity) = proc_stat(pid)?;
    if tree {
        for child in process_tree(pid).into_iter().filter(|p| *p != pid) {
            if let Ok((t, _)) = proc_stat(child) {
                ticks += t;
            }
        }
    }
    Ok((ticks, identity))
}
fn total_memory(pid: i32, tree: bool) -> (Option<u64>, Option<u64>) {
    let (mut rss, mut pss) = memory(pid);
    if tree {
        for child in process_tree(pid).into_iter().filter(|p| *p != pid) {
            let (r, p) = memory(child);
            rss = rss.map(|a| a + r.unwrap_or(0));
            pss = pss.map(|a| a + p.unwrap_or(0));
        }
    }
    (rss, pss)
}
async fn begin_capture(app: &App) -> Result<(), (StatusCode, String)> {
    let pid=app.pid.ok_or_else(||failure("Launch dashboard with --pid to enable native collection. Browser dumps can be opened without a PID."))?;
    let mut c = app.capture.lock().await;
    if c.active {
        return Err(failure("Already capturing"));
    }
    let (ticks, identity) = total_stat(pid, app.tree).map_err(failure)?;
    c.records = vec![
        json!({"type":"metadata","format":"vpt","version":1,"source":"native","adapter":"generic-proc","profile":"lite","startedAt":chrono::Utc::now().to_rfc3339(),"capabilities":{"cpu":true,"rss":true,"pss":"permission-dependent","fps":false,"stacks":false}}),
    ];
    c.active = true;
    c.started = Instant::now();
    c.previous = Some((ticks, Instant::now()));
    c.identity = Some(identity);
    Ok(())
}
async fn start(State(app): State<Arc<App>>) -> ApiResult {
    begin_capture(&app).await?;
    Ok(Json(json!({"ok":true})))
}
async fn import(State(app): State<Arc<App>>, body: axum::body::Bytes) -> ApiResult {
    let _permit = app
        .analysis
        .try_acquire()
        .map_err(|_| failure("Another import or analysis is running"))?;
    if app.capture.lock().await.active {
        return Err(failure(
            "Open dumps in a viewer session without active capture",
        ));
    }
    let text = std::str::from_utf8(&body).map_err(failure)?;
    let records = vp_analyze::parse(text).map_err(failure)?;
    let mut capture = app.capture.lock().await;
    if capture.active {
        return Err(failure(
            "A capture started while the dump was being validated",
        ));
    }
    capture.records = records;
    Ok(Json(json!({"ok":true})))
}

async fn append_records(State(app): State<Arc<App>>, body: axum::body::Bytes) -> ApiResult {
    let text = std::str::from_utf8(&body).map_err(failure)?;
    let mut capture = app.capture.lock().await;
    if !capture.active {
        return Err(failure("No active capture"));
    }
    let next = vp_analyze::parse_append(text, &capture.records).map_err(failure)?;
    if next.is_empty() {
        return Ok(Json(json!({"ok":true,"appended":0})));
    }
    let mut added = 0usize;
    for record in &next {
        added += serde_json::to_string(record).map_err(failure)?.len() + 1;
    }
    let existing = encode(&capture.records).map_err(failure)?.len();
    if existing + added > MAX_TRACE {
        return Err(failure("Dump cap reached"));
    }
    let appended = next.len();
    capture.records.extend(next);
    Ok(Json(json!({"ok":true,"appended":appended})))
}
async fn assets(State(app): State<Arc<App>>) -> Json<Value> {
    let c = app.capture.lock().await;
    Json(json!({"assets":vp_analyze::asset_report(&c.records)}))
}
async fn trace(State(app): State<Arc<App>>) -> Result<Response, (StatusCode, String)> {
    let c = app.capture.lock().await;
    if c.records.is_empty() {
        return Err(failure("No capture or dump loaded"));
    }
    let text = encode(&c.records).map_err(failure)?;
    Ok((
        [
            ("content-type", "application/x-ndjson"),
            ("content-disposition", "attachment; filename=trace.vpt"),
        ],
        text,
    )
        .into_response())
}

async fn report(
    State(app): State<Arc<App>>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, (StatusCode, String)> {
    let c = app.capture.lock().await;
    if c.records.is_empty() {
        return Err(failure("No capture or dump loaded"));
    }
    let include_trace = query.get("trace").is_none_or(|v| v != "0");
    let value = vp_analyze::report(&c.records, vp_analyze::ReportOptions { include_trace });
    let text = serde_json::to_string(&value).map_err(failure)?;
    Ok((
        [
            ("content-type", "application/json"),
            (
                "content-disposition",
                "attachment; filename=trace.report.json",
            ),
        ],
        text,
    )
        .into_response())
}
fn encode(records: &[Value]) -> anyhow::Result<String> {
    let mut text = String::new();
    for v in records {
        let line = serde_json::to_string(v)?;
        if text.len() + line.len() + 1 > MAX_TRACE {
            anyhow::bail!("Dump cap reached");
        }
        text.push_str(&line);
        text.push('\n');
    }
    Ok(text)
}
async fn secret(action: &str, key: Option<&str>) -> anyhow::Result<String> {
    use tokio::io::AsyncWriteExt;
    let mut cmd = tokio::process::Command::new("secret-tool");
    cmd.kill_on_drop(true);
    cmd.arg(action);
    if action == "store" {
        cmd.arg("--label=Vapour Profiler Claude key");
    }
    cmd.args(["application", "vapour-profiler", "provider", "anthropic"]);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().map_err(|_| {
        anyhow::anyhow!("Install secret-tool (libsecret) and unlock your OS keyring")
    })?;
    if let Some(key) = key {
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(key.as_bytes()).await?;
        }
    }
    drop(child.stdin.take());
    let result = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output()).await??;
    if !result.status.success() {
        anyhow::bail!("Keyring action failed or no key is stored");
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}
async fn set_key(Json(input): Json<Value>) -> ApiResult {
    let key = input["key"]
        .as_str()
        .ok_or_else(|| failure("Missing key"))?;
    if key.is_empty() || key.len() > 4096 || key.contains(['\r', '\n']) {
        return Err(failure("Invalid key"));
    }
    secret("store", Some(key)).await.map_err(failure)?;
    Ok(Json(json!({"ok":true})))
}
async fn remove_key() -> ApiResult {
    secret("clear", None).await.map_err(failure)?;
    Ok(Json(
        json!({"ok":true,"note":"ANTHROPIC_API_KEY, if set in the launching shell, must be cleared there separately."}),
    ))
}
async fn analyze(State(app): State<Arc<App>>, Json(input): Json<Value>) -> ApiResult {
    let _permit = app
        .analysis
        .try_acquire()
        .map_err(|_| failure("Analysis already running"))?;
    let summary = {
        let c = app.capture.lock().await;
        if c.records.is_empty() {
            return Err(failure("Load a dump or start a capture first"));
        }
        vp_analyze::summarize(&c.records)
    };
    if input["offline"] != false {
        return Ok(Json(vp_analyze::offline(&summary, None)));
    }
    let key = match std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
    {
        Some(key) => key,
        None => secret("lookup", None).await.unwrap_or_default(),
    };
    if key.is_empty() || key.len() > 4096 {
        return Ok(Json(vp_analyze::offline(
            &summary,
            Some("No key available; analysis stayed offline"),
        )));
    }
    let model = input["model"]
        .as_str()
        .unwrap_or("claude-sonnet-4-6")
        .to_owned();
    let mut shutdown = app.shutdown.subscribe();
    let result = tokio::select! {result=vp_analyze::Claude.report(summary.clone(),key,model)=>result,_=shutdown.changed()=>return Err(failure("Session ended"))};
    Ok(Json(match result {Ok(report)=>report,Err(_)=>vp_analyze::offline(&summary,Some("Claude request failed, was rate-limited, or returned unsupported findings. Check the key/model/network; offline results are shown."))}))
}
async fn socket(State(app): State<Arc<App>>, ws: WebSocketUpgrade) -> Response {
    let Ok(permit) = app.clients.clone().try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    ws.protocols(["vapour.v1"]).max_message_size(4096).on_upgrade(move|mut ws|async move{
        let _permit=permit;let mut shutdown=app.shutdown.subscribe();let mut tick=tokio::time::interval(Duration::from_secs(1));
        loop {tokio::select!{_=shutdown.changed()=>break,_=tick.tick()=>{
            let message=status(&app).await.to_string();
            if !matches!(tokio::time::timeout(Duration::from_secs(2),ws.send(Message::Text(message.into()))).await,Ok(Ok(()))){break;}
        },message=ws.recv()=>{if message.is_none()||matches!(message,Some(Ok(Message::Close(_)))|Some(Err(_))){break;}}}}
    }).into_response()
}
fn dump_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("vapour-profiler/dumps")
}
pub async fn serve(
    pid: Option<i32>,
    tree: bool,
    autostart: bool,
    port: u16,
    ttl: u64,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Cannot bind local dashboard port {port}: {e}. No existing process was terminated."
            )
        })?;
    let port = listener.local_addr()?.port();
    let token = rand::random::<[u8; 32]>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let (shutdown, _) = watch::channel(false);
    let app = Arc::new(App {
        token,
        origin: format!("http://127.0.0.1:{port}"),
        pid,
        tree,
        capture: Mutex::new(Capture {
            records: vec![],
            active: false,
            previous: None,
            identity: None,
            started: Instant::now(),
        }),
        shutdown,
        clients: Arc::new(Semaphore::new(4)),
        analysis: Semaphore::new(1),
        deadline: Instant::now() + Duration::from_secs(ttl),
    });
    if autostart {
        begin_capture(&app)
            .await
            .map_err(|(_, e)| anyhow::anyhow!("autostart failed: {e}"))?;
    }
    let sampler = app.clone();
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let mut stopped = sampler.shutdown.subscribe();
        loop {
            tokio::select! {_=stopped.changed()=>break,_=interval.tick()=>{
                let mut c=sampler.capture.lock().await;
                if !c.active {continue;}
                let pid=sampler.pid.unwrap();
                let Ok((ticks,identity))=total_stat(pid,sampler.tree) else {let _=sampler.shutdown.send(true);break};
                if c.identity!=Some(identity){let _=sampler.shutdown.send(true);break;}
                let now=Instant::now();
                let (old,previous)=c.previous.unwrap();
                // SAFETY: sysconf reads the host's scalar clock-tick configuration.
                let hz=unsafe{libc::sysconf(libc::_SC_CLK_TCK)};
                let cpu=if hz>0 {Some(ticks.saturating_sub(old) as f64/hz as f64/now.duration_since(previous).as_secs_f64()*100.)}else{None};
                // SAFETY: query a scalar count of online machine CPUs, not cgroup parallelism.
                let cores=unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) }.max(1) as usize;
                let (rss,pss)=total_memory(pid,sampler.tree);let time=c.started.elapsed().as_secs_f64()*1000.;
                if c.records.len()>=301{let _=sampler.shutdown.send(true);break;}
                c.records.push(json!({"type":"metrics","timeMs":time,"values":{"cpuOneCorePercent":cpu,"cpuMachinePercent":cpu.map(|n|(n/cores as f64).min(100.)),"rssBytes":rss,"pssBytes":pss}}));
                c.previous=Some((ticks,now));
            }}
        }
    });
    let router = Router::new()
        .route(
            "/",
            get(|| async { asset("text/html; charset=utf-8", include_str!("../ui/index.html")) }),
        )
        .route(
            "/app.js",
            get(|| async { asset("text/javascript", include_str!("../ui/app.js")) }),
        )
        .route(
            "/style.css",
            get(|| async { asset("text/css", include_str!("../ui/style.css")) }),
        )
        .route(
            "/api/status",
            get(|State(app): State<Arc<App>>| async move { Json(status(&app).await) }),
        )
        .route("/api/live", get(socket))
        .route("/api/start", post(start))
        .route("/api/trace", get(trace).post(import))
        .route("/api/records", post(append_records))
        .route("/api/report", get(report))
        .route("/api/assets", get(assets))
        .route(
            "/api/analyze",
            post(analyze).layer(DefaultBodyLimit::max(4096)),
        )
        .route("/api/key", post(set_key).layer(DefaultBodyLimit::max(8192)))
        .route("/api/key/remove", post(remove_key))
        .route(
            "/api/off",
            post(|State(app): State<Arc<App>>| async move {
                let _ = app.shutdown.send(true);
                Json(json!({"ok":true}))
            }),
        )
        .layer(DefaultBodyLimit::max(MAX_TRACE))
        .layer(tower::limit::ConcurrencyLimitLayer::new(8))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app.clone());
    println!(
        "Open {} / session TTL {}s\n{}#token={}",
        app.origin, ttl, app.origin, app.token
    );
    let mut end = app.shutdown.subscribe();
    let stop = app.clone();
    axum::serve(listener,router).with_graceful_shutdown(async move{
        #[cfg(unix)] let mut term=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select!{_=tokio::time::sleep(Duration::from_secs(ttl))=>{},_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{},_=end.changed()=>{}}
        let _=stop.shutdown.send(true);
    }).await?;
    task.abort();
    let _ = task.await;
    let mut c = app.capture.lock().await;
    if c.active {
        let duration = c.started.elapsed().as_secs_f64() * 1000.;
        c.records
            .push(json!({"type":"end","durationMs":duration,"dropped":{}}));
        let dir = dump_dir();
        std::fs::create_dir_all(&dir)?;
        // Keep storage bounded; never overwrite or remove a user's existing dump.
        let existing = std::fs::read_dir(&dir)?
            .filter_map(Result::ok)
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum::<u64>();
        let text = encode(&c.records)?;
        let report = serde_json::to_string_pretty(&vp_analyze::report(
            &c.records,
            vp_analyze::ReportOptions::default(),
        ))?;
        if existing + text.len() as u64 + report.len() as u64 <= 200 * 1024 * 1024 {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%f").to_string();
            let path = dir.join(format!("capture-{stamp}.vpt"));
            let mut trace_file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            trace_file.write_all(text.as_bytes())?;
            let report_path = dir.join(format!("capture-{stamp}.report.json"));
            let mut report_file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&report_path)?;
            report_file.write_all(report.as_bytes())?;
            println!("Flushed {}", path.display());
            println!("Flushed {}", report_path.display());
        } else {
            eprintln!("Dump storage cap reached (200 MiB); export from the dashboard before ending a session or clear old dumps.");
        }
    }
    println!("Dashboard off: socket and collectors closed.");
    Ok(())
}

#[cfg(test)]
mod tree_tests {
    use super::*;

    #[test]
    fn process_tree_includes_descendants() {
        // sh stays alive while its backgrounded sleep (a child) runs.
        let mut parent = std::process::Command::new("sh")
            .args(["-c", "sleep 5 & wait"])
            .spawn()
            .unwrap();
        let root = parent.id() as i32;
        let mut found = false;
        for _ in 0..50 {
            if process_tree(root).len() > 1 {
                found = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = parent.kill();
        let _ = parent.wait();
        assert!(found, "child process not found in tree");
        assert_eq!(process_tree(root)[0], root);
        assert!(
            total_stat(root, true).is_err(),
            "reaped root must be an error"
        );
    }
}
