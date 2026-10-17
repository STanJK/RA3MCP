//! Resident stdio MCP server. Legacy handshakes and 2026 per-request metadata.
use crate::json::{self, Json};
use crate::{Ack, Command, Op, Session};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

const MODERN: &str = "2026-07-28";
const LEGACY: &[&str] = &["2025-11-25", "2025-06-18"];
const MAX_FRAME: usize = 1_048_576;
const MAX_PENDING: usize = 64;
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
const CAPS_KEY: &str = "io.modelcontextprotocol/clientCapabilities";
const INSTRUCTIONS: &str = "Control the verified RA3 1.12 input bridge. Discover candidate pipe PIDs, then pass an explicit PID on every game call. Coordinates are client pixels. Successful pumped acknowledgements do not prove a gameplay outcome. Mouse actions are serialized without a focus delay. Do not automatically retry input after errors, cancellation or timeouts. Each connected pipe is exclusive; disconnect before using another controller. Closing stdin closes cached pipes and exits. This server never starts, installs or injects into the game.";
type Tokens = Arc<Mutex<BTreeMap<String, Arc<AtomicBool>>>>;
type Output = Arc<Mutex<Box<dyn Write + Send>>>;

fn object(items: impl IntoIterator<Item = (impl Into<String>, Json)>) -> Json { Json::object(items) }
fn empty() -> Json { object(Vec::<(String, Json)>::new()) }
fn info() -> Json { object([("name", "ra3ctl".into()), ("version", env!("CARGO_PKG_VERSION").into())]) }
fn capabilities() -> Json { object([("tools", object([("listChanged", false.into())]))]) }
fn versions() -> Json { Json::Array(std::iter::once(MODERN).chain(LEGACY.iter().copied()).map(Json::from).collect()) }
fn error(id: Json, code: i64, message: &str, data: Option<Json>) -> Json {
    let mut body = object([("code", code.into()), ("message", message.into())]);
    if let (Some(data), Json::Object(fields)) = (data, &mut body) { fields.insert("data".into(), data); }
    object([("jsonrpc", "2.0".into()), ("id", id), ("error", body)])
}
fn result(id: Json, mut value: Json, modern: bool) -> Json {
    if modern {
        if let Json::Object(fields) = &mut value {
            fields.insert("resultType".into(), "complete".into());
            fields.insert("_meta".into(), object([("io.modelcontextprotocol/serverInfo", info())]));
        }
    }
    object([("jsonrpc", "2.0".into()), ("id", id), ("result", value)])
}
fn write(output: &Output, value: &Json) -> io::Result<()> {
    let mut output = output.lock().map_err(|_| io::Error::other("stdout lock poisoned"))?;
    writeln!(output, "{}", value.encode())?;
    output.flush()
}
fn id_key(value: &Json) -> Option<String> {
    match value {
        Json::String(_) => Some(value.encode()),
        Json::Number(n) => value.integer().map(|v| format!("number:{v}"))
            .or_else(|| (!n.contains(['.', 'e', 'E'])).then(|| format!("number:{n}"))),
        _ => None,
    }
}

#[derive(Clone, Debug)]
enum Action {
    Discover, Disconnect, Ping, Move(i32, i32), Release,
    Click(u8, i32, i32, u64),
    Drag(u8, (i32, i32), (i32, i32), u64, u32),
    Wheel(i32, i32, i32),
}
#[derive(Debug)]
struct ToolError { code: &'static str, message: String, outcome: &'static str, os_error: Option<i32> }
impl ToolError {
    fn invalid(message: impl Into<String>) -> Self { Self { code: "invalid_arguments", message: message.into(), outcome: "not_sent", os_error: None } }
    fn io(code: &'static str, error: io::Error, outcome: &'static str) -> Self {
        Self { code, message: error.to_string(), outcome, os_error: error.raw_os_error() }
    }
    fn json(self, pid: Option<u32>) -> Json {
        let mut details = object([("code", self.code.into()), ("message", self.message.into()),
            ("outcome", self.outcome.into()), ("retryable", false.into())]);
        if let (Some(n), Json::Object(fields)) = (self.os_error, &mut details) { fields.insert("os_error".into(), (n as i64).into()); }
        let mut value = object([("ok", false.into()), ("error", details)]);
        if let (Some(pid), Json::Object(fields)) = (pid, &mut value) { fields.insert("pid".into(), pid.into()); }
        value
    }
}
fn integer(args: &Json, key: &str, min: i64, max: i64, default: Option<i64>) -> Result<i64, ToolError> {
    let n = match args.get(key) {
        None => default.ok_or_else(|| ToolError::invalid(format!("{key} is required")))?,
        Some(value) => value.integer().ok_or_else(|| ToolError::invalid(format!("{key} must be an integer")))?,
    };
    if !(min..=max).contains(&n) { return Err(ToolError::invalid(format!("{key} must be {min}..{max}"))); }
    Ok(n)
}
fn parse_action(name: &str, args: &Json, definition: &Json) -> Result<(Option<u32>, Action), ToolError> {
    let fields = args.fields().ok_or_else(|| ToolError::invalid("arguments must be an object"))?;
    let allowed = definition.get("inputSchema").and_then(|v| v.get("properties")).and_then(Json::fields).expect("catalog properties");
    for key in fields.keys() {
        if !allowed.contains_key(key) { return Err(ToolError::invalid(format!("unknown argument: {key}"))); }
    }
    if name == "ra3_list_instances" { return Ok((None, Action::Discover)); }
    let pid = integer(args, "pid", 1, u32::MAX as i64, None)? as u32;
    let coordinate = |key| integer(args, key, 0, 65535, None).map(|v| v as i32);
    let button = || match args.get("button") {
        None => Ok(0),
        Some(v) => match v.text() {
            Some("left") => Ok(0), Some("right") => Ok(1), Some("middle") => Ok(2),
            _ => Err(ToolError::invalid("button must be left, right, or middle")),
        },
    };
    let action = match name {
        "ra3_disconnect" => Action::Disconnect,
        "ra3_ping" => Action::Ping,
        "ra3_release" => Action::Release,
        "ra3_move" => Action::Move(coordinate("x")?, coordinate("y")?),
        "ra3_click" => Action::Click(button()?, coordinate("x")?, coordinate("y")?,
            integer(args, "hold_ms", 1, 60000, Some(80))? as u64),
        "ra3_drag" => Action::Drag(button()?, (coordinate("x0")?, coordinate("y0")?), (coordinate("x1")?, coordinate("y1")?),
            integer(args, "duration_ms", 1, 60000, Some(400))? as u64,
            integer(args, "steps", 1, 1000, Some(20))? as u32),
        "ra3_wheel" => {
            let ticks = integer(args, "ticks", -100, 100, None)? as i32;
            if ticks == 0 { return Err(ToolError::invalid("ticks must be nonzero")); }
            Action::Wheel(coordinate("x")?, coordinate("y")?, ticks)
        }
        _ => unreachable!("catalog and action handlers must agree"),
    };
    Ok((Some(pid), action))
}

trait Backend: Send + 'static {
    fn discover(&mut self) -> Result<Json, ToolError>;
    fn execute(&mut self, pid: u32, action: Action, cancel: &AtomicBool) -> Result<Json, ToolError>;
}
#[derive(Default)]
struct GameBackend { sessions: BTreeMap<u32, Session> }
impl Backend for GameBackend {
    fn discover(&mut self) -> Result<Json, ToolError> {
        #[cfg(not(windows))]
        return Err(ToolError::io("platform", io::Error::new(io::ErrorKind::Unsupported, "Windows required"), "not_sent"));
        #[cfg(windows)]
        {
            let mut pids = Vec::new();
            for entry in std::fs::read_dir(r"\\.\pipe\").map_err(|e| ToolError::io("discovery_failed", e, "not_sent"))? {
                let entry = entry.map_err(|e| ToolError::io("discovery_failed", e, "not_sent"))?;
                let name = entry.file_name();
                if let Some(pid) = name.to_str().and_then(|s| s.strip_prefix("RA3MCP.")).and_then(|s| s.parse::<u32>().ok()).filter(|p| *p != 0) {
                    pids.push(pid);
                }
            }
            pids.sort_unstable(); pids.dedup();
            let instances = pids.into_iter().map(|pid| object([
                ("pid", pid.into()), ("pipe", format!(r"\\.\pipe\RA3MCP.{pid}").into()),
                ("connected", self.sessions.contains_key(&pid).into()),
            ])).collect();
            Ok(object([("ok", true.into()), ("instances", Json::Array(instances)),
                ("note", "Pipe presence is a candidate endpoint, not a readiness check; use ra3_ping.".into())]))
        }
    }
    fn execute(&mut self, pid: u32, action: Action, cancel: &AtomicBool) -> Result<Json, ToolError> {
        if matches!(action, Action::Disconnect) {
            let existed = self.sessions.remove(&pid).is_some();
            return Ok(object([("ok", true.into()), ("pid", pid.into()), ("disconnected", existed.into())]));
        }
        if !self.sessions.contains_key(&pid) {
            if self.sessions.len() >= 16 { return Err(ToolError::invalid("16 pipes are cached; disconnect an unused PID first")); }
            let session = Session::connect_cancellable(pid, cancel).map_err(|e| ToolError::io("connect_failed", e, "not_sent"))?;
            self.sessions.insert(pid, session);
        }
        let session = self.sessions.get_mut(&pid).expect("connected session");
        let ack = match action {
            Action::Ping => session.send_cancellable(Command::at(Op::Ping, 0, 0, 0), cancel),
            Action::Move(x, y) => session.send_cancellable(Command::at(Op::Move, 0, x, y), cancel),
            Action::Release => session.send_cancellable(Command::at(Op::ReleaseAll, 0, 0, 0), cancel),
            Action::Click(button, x, y, ms) => session.click_cancellable(button, x, y, Duration::from_millis(ms), cancel),
            Action::Drag(button, start, end, ms, steps) => session.drag_cancellable(button, start, end, Duration::from_millis(ms), steps, cancel),
            Action::Wheel(x, y, ticks) => session.send_cancellable(Command { op: Op::Wheel, button: 0, x, y, value: ticks }, cancel),
            Action::Discover | Action::Disconnect => unreachable!(),
        };
        match ack {
            Ok(ack) if !cancel.load(Ordering::Acquire) => Ok(ack_json(pid, ack)),
            other => {
                self.sessions.remove(&pid); // Close before returning an error; native bridge releases on next pump.
                Err(ToolError::io("bridge_error", other.err().unwrap_or_else(crate::pipe::cancelled), "unknown"))
            }
        }
    }
}
fn ack_json(pid: u32, ack: Ack) -> Json {
    object([("ok", true.into()), ("pid", pid.into()), ("status", "pumped".into()),
        ("sequence", ack.sequence.into()), ("pump", ack.pump.into()), ("held", (ack.held as u32).into()),
        ("detail", ack.detail.into())])
}
fn tool_result(value: Json) -> Json {
    let failed = value.get("ok") == Some(&Json::Bool(false));
    object([("content", Json::Array(vec![object([("type", "text".into()), ("text", value.encode().into())])])),
        ("structuredContent", value), ("isError", failed.into())])
}

struct Server<B> { backend: B, tools: Vec<Json>, legacy: Option<String>, initialized: bool }
impl<B: Backend> Server<B> {
    fn new(backend: B) -> Self {
        let Json::Array(tools) = json::parse(include_str!("../mcp-tools.json")).expect("valid bundled MCP tool catalog") else { panic!("tool catalog array"); };
        Self { backend, tools, legacy: None, initialized: false }
    }
    fn handle(&mut self, message: &Json, cancel: &AtomicBool) -> Option<Json> {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Json::text).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or_else(empty);
        if id.is_none() {
            if method == "notifications/initialized" && self.legacy.is_some() { self.initialized = true; }
            return None; // Notifications never execute game actions.
        }
        let id = id.unwrap();
        if params.fields().is_none() { return Some(error(id, -32602, "params must be an object", None)); }
        let meta = params.get("_meta");
        let requested = meta.and_then(|v| v.get(VERSION_KEY));
        let modern = requested.is_some();
        if modern {
            let Some(version) = requested.and_then(Json::text) else { return Some(error(id, -32602, "protocolVersion metadata must be a string", None)); };
            if version != MODERN {
                return Some(error(id, -32022, "Unsupported protocol version",
                    Some(object([("supported", versions()), ("requested", version.into())]))));
            }
            if meta.and_then(|v| v.get(CAPS_KEY)).and_then(Json::fields).is_none() {
                return Some(error(id, -32602, "clientCapabilities metadata is required and must be an object", None));
            }
            if let Some(client) = meta.and_then(|v| v.get("io.modelcontextprotocol/clientInfo")) {
                if client.get("name").and_then(Json::text).is_none() || client.get("version").and_then(Json::text).is_none() {
                    return Some(error(id, -32602, "clientInfo metadata requires string name and version fields", None));
                }
            }
        }
        if method == "initialize" && !modern {
            if self.legacy.is_some() { return Some(error(id, -32600, "Already initialized", None)); }
            let Some(requested) = params.get("protocolVersion").and_then(Json::text) else { return Some(error(id, -32602, "protocolVersion is required", None)); };
            let client = params.get("clientInfo");
            if params.get("capabilities").and_then(Json::fields).is_none()
                || client.and_then(|v| v.get("name")).and_then(Json::text).is_none()
                || client.and_then(|v| v.get("version")).and_then(Json::text).is_none() {
                return Some(error(id, -32602, "capabilities and clientInfo.name/version are required", None));
            }
            let selected = if LEGACY.contains(&requested) { requested } else { LEGACY[0] };
            self.legacy = Some(selected.into());
            return Some(result(id, object([("protocolVersion", selected.into()), ("capabilities", capabilities()),
                ("serverInfo", info()), ("instructions", INSTRUCTIONS.into())]), false));
        }
        if !modern && method != "ping" && (!self.initialized || self.legacy.is_none()) {
            return Some(error(id, -32602, "Use initialize + notifications/initialized, or supply 2026 per-request metadata", None));
        }
        let value = match method {
            "server/discover" if modern => object([("supportedVersions", versions()), ("capabilities", capabilities()),
                ("instructions", INSTRUCTIONS.into())]),
            "ping" => empty(),
            "tools/list" => {
                if params.get("cursor").is_some() { return Some(error(id, -32602, "This fixed tool list has no pagination cursor", None)); }
                object([("tools", Json::Array(self.tools.clone()))])
            }
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Json::text) else { return Some(error(id, -32602, "tools/call requires a tool name", None)); };
                let Some(definition) = self.tools.iter().find(|v| v.get("name").and_then(Json::text) == Some(name)) else {
                    return Some(error(id, -32602, &format!("Unknown tool: {name}"), None));
                };
                let args = params.get("arguments").cloned().unwrap_or_else(empty);
                if args.fields().is_none() { return Some(error(id, -32602, "tools/call arguments must be an object", None)); }
                let pid = if name == "ra3_list_instances" { None } else {
                    args.get("pid").and_then(Json::integer).and_then(|v| u32::try_from(v).ok()).filter(|v| *v != 0)
                };
                let outcome = match parse_action(name, &args, definition) {
                    Ok((_, Action::Discover)) => self.backend.discover(),
                    Ok((Some(pid), action)) => self.backend.execute(pid, action, cancel),
                    Ok(_) => unreachable!(),
                    Err(error) => Err(error),
                };
                let value = outcome.unwrap_or_else(|failure| failure.json(pid));
                tool_result(value)
            }
            _ => return Some(error(id, -32601, &format!("Method not found: {method}"), None)),
        };
        Some(result(id, value, modern))
    }
}

struct Job { message: Json, token: Arc<AtomicBool>, key: Option<String> }
fn read_frame<R: BufRead>(input: &mut R) -> io::Result<Option<Result<Json, String>>> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            if bytes.is_empty() && !oversized { return Ok(None); }
            break;
        }
        let amount = available.iter().position(|v| *v == b'\n').map_or(available.len(), |n| n + 1);
        let finished = available[amount - 1] == b'\n';
        if !oversized && bytes.len() + amount <= MAX_FRAME { bytes.extend_from_slice(&available[..amount]); }
        else { oversized = true; bytes.clear(); }
        input.consume(amount);
        if finished { break; }
    }
    if oversized { return Ok(Some(Err("MCP frame exceeds 1 MiB".into()))); }
    Ok(Some(String::from_utf8(bytes).map_err(|_| "MCP messages must be UTF-8".into()).and_then(|text| json::parse(&text))))
}
fn serve<R: BufRead, W: Write + Send + 'static, B: Backend>(mut input: R, output: W, backend: B) -> io::Result<()> {
    let output: Output = Arc::new(Mutex::new(Box::new(output)));
    let tokens: Tokens = Arc::new(Mutex::new(BTreeMap::new()));
    let (sender, receiver) = mpsc::sync_channel::<Job>(MAX_PENDING);
    let worker_output = output.clone();
    let worker_tokens = tokens.clone();
    let worker = thread::spawn(move || -> io::Result<()> {
        let mut server = Server::new(backend);
        for job in receiver {
            if !job.token.load(Ordering::Acquire) {
                let reply = server.handle(&job.message, &job.token);
                let mut active = worker_tokens.lock().map_err(|_| io::Error::other("request registry poisoned"))?;
                if !job.token.load(Ordering::Acquire) {
                    if let Some(reply) = reply { write(&worker_output, &reply)?; }
                }
                if let Some(key) = &job.key { active.remove(key); }
            } else if let Some(key) = &job.key {
                worker_tokens.lock().map_err(|_| io::Error::other("request registry poisoned"))?.remove(key);
            }
        }
        Ok(()) // Dropping backend closes all cached pipes.
    });
    let read_result = (|| -> io::Result<()> {
        while let Some(frame) = read_frame(&mut input)? {
            let message = match frame {
                Ok(value) => value,
                Err(message) => { write(&output, &error(Json::Null, -32700, &message, None))?; continue; }
            };
            let id = message.get("id");
            let key = id.and_then(id_key);
            if message.fields().is_none() || message.get("jsonrpc").and_then(Json::text) != Some("2.0")
                || message.get("method").and_then(Json::text).is_none() || (id.is_some() && key.is_none()) {
                write(&output, &error(id.filter(|v| id_key(v).is_some()).cloned().unwrap_or(Json::Null), -32600, "Invalid JSON-RPC request", None))?;
                continue;
            }
            if message.get("method").and_then(Json::text) == Some("notifications/cancelled") && id.is_none() {
                if let Some(key) = message.get("params").and_then(|v| v.get("requestId")).and_then(id_key) {
                    if let Some(token) = tokens.lock().map_err(|_| io::Error::other("request registry poisoned"))?.get(&key) {
                        token.store(true, Ordering::Release);
                    }
                }
                continue;
            }
            let token = Arc::new(AtomicBool::new(false));
            if let Some(key) = &key {
                let mut active = tokens.lock().map_err(|_| io::Error::other("request registry poisoned"))?;
                if active.contains_key(key) {
                    write(&output, &error(id.cloned().unwrap(), -32600, "Duplicate in-flight request ID", None))?;
                    continue;
                }
                active.insert(key.clone(), token.clone());
            }
            let job = Job { message, token, key };
            match sender.try_send(job) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(job)) => {
                    if let Some(key) = &job.key {
                        tokens.lock().map_err(|_| io::Error::other("request registry poisoned"))?.remove(key);
                        write(&output, &error(job.message.get("id").cloned().unwrap(), -32603, "Request queue is full", None))?;
                    }
                }
                Err(mpsc::TrySendError::Disconnected(_)) => return Err(io::Error::new(io::ErrorKind::BrokenPipe, "MCP worker exited")),
            }
        }
        Ok(())
    })();
    // EOF/read failure cancels active and queued work before joining the worker.
    if let Ok(active) = tokens.lock() { for token in active.values() { token.store(true, Ordering::Release); } }
    drop(sender);
    let worker_result = worker.join().map_err(|_| io::Error::other("MCP worker panicked"))?;
    read_result.and(worker_result)
}

/// Runs until stdin closes. stdout contains MCP JSON-RPC messages only.
pub fn run_stdio() -> io::Result<()> {
    eprintln!("ra3ctl MCP {}: stdio; no game connection until a tool call", env!("CARGO_PKG_VERSION"));
    serve(io::stdin().lock(), io::stdout(), GameBackend::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Mock { calls: u32 }
    impl Backend for Mock {
        fn discover(&mut self) -> Result<Json, ToolError> { Ok(object([("ok", true.into()), ("instances", Json::Array(vec![]))])) }
        fn execute(&mut self, pid: u32, _: Action, _: &AtomicBool) -> Result<Json, ToolError> {
            self.calls += 1;
            Ok(ack_json(pid, Ack { status: 0, sequence: self.calls, pump: self.calls + 100, held: 0, detail: 0 }))
        }
    }
    fn request(id: i64, method: &str, mut params: Json, modern: bool) -> Json {
        if modern {
            if let Json::Object(fields) = &mut params { fields.insert("_meta".into(), object([(VERSION_KEY, MODERN.into()), (CAPS_KEY, empty())])); }
        }
        object([("jsonrpc", "2.0".into()), ("id", id.into()), ("method", method.into()), ("params", params)])
    }
    #[test]
    fn both_lifecycles_and_tool_validation_before_io() {
        let mut server = Server::new(Mock::default());
        let cancel = AtomicBool::new(false);
        let discovery = server.handle(&request(1, "server/discover", empty(), true), &cancel).unwrap();
        assert_eq!(discovery.get("result").unwrap().get("resultType").and_then(Json::text), Some("complete"));
        let init = request(2, "initialize", object([("protocolVersion", "2025-11-25".into()), ("capabilities", empty()), ("clientInfo", info())]), false);
        assert_eq!(server.handle(&init, &cancel).unwrap().get("result").unwrap().get("protocolVersion").and_then(Json::text), Some("2025-11-25"));
        server.handle(&object([("jsonrpc", "2.0".into()), ("method", "notifications/initialized".into())]), &cancel);
        let list = server.handle(&request(3, "tools/list", empty(), false), &cancel).unwrap();
        assert!(list.get("result").unwrap().get("resultType").is_none());
        let bad = request(4, "tools/call", object([("name", "ra3_click".into()), ("arguments", object([("pid", 7u32.into()), ("x", (-1i64).into()), ("y", 0u32.into())]))]), true);
        assert_eq!(server.handle(&bad, &cancel).unwrap().get("result").unwrap().get("isError"), Some(&Json::Bool(true)));
        assert_eq!(server.backend.calls, 0);
        let good = request(5, "tools/call", object([("name", "ra3_click".into()), ("arguments", object([("pid", 7u32.into()), ("x", 10u32.into()), ("y", 20u32.into())]))]), true);
        assert_eq!(server.handle(&good, &cancel).unwrap().get("result").unwrap().get("isError"), Some(&Json::Bool(false)));
        assert_eq!(server.backend.calls, 1);
    }
    #[test]
    fn modern_metadata_missing_and_unsupported_versions_fail() {
        let mut server = Server::new(Mock::default());
        let cancel = AtomicBool::new(false);
        let message = request(1, "tools/list", object([("_meta", object([(VERSION_KEY, MODERN.into())]))]), false);
        assert_eq!(server.handle(&message, &cancel).unwrap().get("error").unwrap().get("code").and_then(Json::integer), Some(-32602));
        let message = request(2, "tools/list", object([("_meta", object([(VERSION_KEY, "2099-01-01".into()), (CAPS_KEY, empty())]))]), false);
        assert_eq!(server.handle(&message, &cancel).unwrap().get("error").unwrap().get("code").and_then(Json::integer), Some(-32022));
    }
    #[test]
    fn oversized_frame_resynchronizes_at_next_newline() {
        let text = format!("{}\n{{}}\n", "x".repeat(MAX_FRAME + 20));
        let mut input = io::Cursor::new(text.into_bytes());
        assert!(read_frame(&mut input).unwrap().unwrap().is_err());
        assert_eq!(read_frame(&mut input).unwrap().unwrap().unwrap(), empty());
    }
}
