#![cfg(windows)]
#[allow(dead_code)]
#[path = "../src/json.rs"]
mod json;
use json::Json;
use ra3ctl::{Command, Op, PACKET_SIZE};
use std::ffi::c_void;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::io::FromRawHandle;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::ptr::{null, null_mut};
use std::sync::{atomic::{AtomicU32, Ordering}, mpsc};
use std::thread;
use std::time::{Duration, Instant};

fn obj(items: impl IntoIterator<Item = (impl Into<String>, Json)>) -> Json { Json::object(items) }
fn empty() -> Json { obj(Vec::<(String, Json)>::new()) }
fn modern(id: u32, method: &str, mut params: Json) -> Json {
    if let Json::Object(fields) = &mut params {
        fields.insert("_meta".into(), obj([
            ("io.modelcontextprotocol/protocolVersion", "2026-07-28".into()),
            ("io.modelcontextprotocol/clientCapabilities", empty()),
        ]));
    }
    obj([("jsonrpc", "2.0".into()), ("id", id.into()), ("method", method.into()), ("params", params)])
}
struct Client { child: Child, replies: mpsc::Receiver<Json> }
impl Client {
    fn new() -> Self {
        let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_ra3ctl")).args(["mcp", "--transport", "stdio"])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, replies) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let value = json::parse(&line.expect("stdout must be valid UTF-8")).expect("stdout must contain JSON-RPC only");
                if sender.send(value).is_err() { break; }
            }
        });
        Self { child, replies }
    }
    fn send(&mut self, message: Json) { self.raw(&(message.encode() + "\n")); }
    fn raw(&mut self, message: &str) {
        self.child.stdin.as_mut().unwrap().write_all(message.as_bytes()).unwrap();
        self.child.stdin.as_mut().unwrap().flush().unwrap();
    }
    fn reply(&self, id: u32) -> Json {
        let value = self.replies.recv_timeout(Duration::from_secs(10)).expect("MCP response deadline");
        assert_eq!(value.get("id").and_then(Json::integer), Some(id as i64), "{value:?}");
        value
    }
    fn call(&mut self, id: u32, name: &str, args: Json) -> Json {
        self.send(modern(id, "tools/call", obj([("name", name.into()), ("arguments", args)])));
        self.reply(id)
    }
    fn shutdown(&mut self) {
        drop(self.child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() { assert!(status.success(), "{status}"); break; }
            assert!(Instant::now() < deadline, "server did not exit promptly after EOF");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}
fn body(reply: &Json) -> &Json { reply.get("result").unwrap().get("structuredContent").unwrap() }

#[link(name = "kernel32")]
extern "system" {
    fn CreateNamedPipeW(name: *const u16, mode: u32, pipe_mode: u32, instances: u32,
        out_size: u32, in_size: u32, timeout: u32, security: *const c_void) -> *mut c_void;
    fn ConnectNamedPipe(pipe: *mut c_void, overlapped: *mut c_void) -> i32;
}
static NEXT_PIPE: AtomicU32 = AtomicU32::new(0xf0000000);
struct Fixture { pid: u32, packets: mpsc::Receiver<[u8; PACKET_SIZE]>, finished: mpsc::Receiver<Vec<[u8; PACKET_SIZE]>> }
fn fixture(stall: bool) -> Fixture {
    let pid = NEXT_PIPE.fetch_add(1, Ordering::Relaxed);
    let name: Vec<u16> = format!(r"\\.\pipe\RA3MCP.{pid}").encode_utf16().chain(Some(0)).collect();
    let handle = unsafe { CreateNamedPipeW(name.as_ptr(), 3 | 0x00080000, 8, 1, 4096, 4096, 0, null()) };
    assert_ne!(handle as isize, -1, "{}", std::io::Error::last_os_error());
    let handle = handle as usize;
    let (packet_sender, packets) = mpsc::channel();
    let (finished_sender, finished) = mpsc::channel();
    thread::spawn(move || {
        let raw = handle as *mut c_void;
        let connected = unsafe { ConnectNamedPipe(raw, null_mut()) } != 0
            || std::io::Error::last_os_error().raw_os_error() == Some(535);
        let mut pipe = unsafe { File::from_raw_handle(raw) };
        assert!(connected);
        let mut seen = Vec::new();
        let mut held = 0;
        let mut pump = 100;
        loop {
            let mut packet = [0; PACKET_SIZE];
            if pipe.read_exact(&mut packet).is_err() { break; }
            seen.push(packet);
            packet_sender.send(packet).unwrap();
            if stall { continue; } // Wait for EOF instead of replying; exercises pending ReadFile cancellation.
            let seq = u32::from_le_bytes(packet[8..12].try_into().unwrap());
            pump += 1;
            match packet[6] { 2 => held |= 1 << packet[7], 3 => held &= !(1 << packet[7]), 5 => held = 0, _ => {} }
            let mut ack = Command::at(Op::Ping, held, pump, 0).encode(seq);
            ack[7] = held;
            // Deliberately split the reply to exercise byte-stream / overlapped reads.
            if pipe.write_all(&ack[..7]).is_err() { break; }
            thread::sleep(Duration::from_millis(1));
            if pipe.write_all(&ack[7..]).is_err() { break; }
        }
        let _ = finished_sender.send(seen);
    });
    Fixture { pid, packets, finished }
}
fn args(pid: u32) -> Json { obj([("pid", pid.into())]) }

#[test]
fn real_stdio_supports_both_eras_and_recovers_from_parse_errors() {
    let mut client = Client::new();
    client.send(modern(1, "server/discover", empty()));
    let reply = client.reply(1);
    assert_eq!(reply.get("result").unwrap().get("resultType").and_then(Json::text), Some("complete"));
    client.raw("{malformed\n");
    assert_eq!(client.replies.recv_timeout(Duration::from_secs(2)).unwrap().get("error").unwrap().get("code").and_then(Json::integer), Some(-32700));
    client.send(obj([("jsonrpc", "2.0".into()), ("id", 2u32.into()), ("method", "initialize".into()), ("params", obj([
        ("protocolVersion", "2025-06-18".into()), ("capabilities", empty()),
        ("clientInfo", obj([("name", "Toolhub 测试".into()), ("version", "1".into())])),
    ]))]));
    assert_eq!(client.reply(2).get("result").unwrap().get("protocolVersion").and_then(Json::text), Some("2025-06-18"));
    client.send(obj([("jsonrpc", "2.0".into()), ("method", "notifications/initialized".into())]));
    client.send(obj([("jsonrpc", "2.0".into()), ("id", 3u32.into()), ("method", "tools/list".into())]));
    let reply = client.reply(3);
    let Json::Array(tools) = reply.get("result").unwrap().get("tools").unwrap() else { panic!() };
    assert_eq!(tools.len(), 8);
    assert!(reply.get("result").unwrap().get("resultType").is_none());
    let bad = client.call(4, "ra3_click", obj([("pid", 1u32.into()), ("x", (-1i64).into()), ("y", 0u32.into())]));
    assert_eq!(bad.get("result").unwrap().get("isError"), Some(&Json::Bool(true)));
    assert_eq!(body(&bad).get("error").unwrap().get("outcome").and_then(Json::text), Some("not_sent"));
    client.send(modern(5, "ping", empty()));
    client.reply(5);
    client.shutdown();
}

#[test]
fn persistent_pipe_actions_and_disconnect_use_the_real_binary() {
    let fixture = fixture(false);
    let mut client = Client::new();
    let discovery = client.call(0, "ra3_list_instances", empty());
    let Json::Array(instances) = body(&discovery).get("instances").unwrap() else { panic!() };
    assert!(instances.iter().any(|v| v.get("pid").and_then(Json::integer) == Some(fixture.pid as i64)));
    let ping = client.call(1, "ra3_ping", args(fixture.pid));
    assert_eq!(body(&ping).get("sequence").and_then(Json::integer), Some(1));
    let click = client.call(2, "ra3_click", obj([("pid", fixture.pid.into()), ("x", 800u32.into()), ("y", 450u32.into()), ("hold_ms", 1u32.into())]));
    assert_eq!(body(&click).get("sequence").and_then(Json::integer), Some(4));
    assert_eq!(body(&click).get("held").and_then(Json::integer), Some(0));
    let drag = client.call(3, "ra3_drag", obj([("pid", fixture.pid.into()), ("x0", 900u32.into()), ("y0", 600u32.into()),
        ("x1", 300u32.into()), ("y1", 200u32.into()), ("duration_ms", 1u32.into()), ("steps", 3u32.into())]));
    assert_eq!(body(&drag).get("sequence").and_then(Json::integer), Some(10));
    let wheel = client.call(4, "ra3_wheel", obj([("pid", fixture.pid.into()), ("x", 10u32.into()), ("y", 20u32.into()), ("ticks", (-2i64).into())]));
    assert_eq!(body(&wheel).get("sequence").and_then(Json::integer), Some(11));
    client.call(5, "ra3_release", args(fixture.pid));
    client.call(6, "ra3_disconnect", args(fixture.pid));
    let packets = fixture.finished.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(packets.iter().map(|p| p[6]).collect::<Vec<_>>(), [0, 1, 2, 3, 1, 2, 1, 1, 1, 3, 4, 5]);
    assert_eq!(i32::from_le_bytes(packets[8][12..16].try_into().unwrap()), 300);
    assert_eq!(i32::from_le_bytes(packets[8][16..20].try_into().unwrap()), 200);
    client.shutdown();
}

#[test]
fn cancellation_during_hold_closes_pipe_without_sending_more_input() {
    let fixture = fixture(false);
    let mut client = Client::new();
    client.send(modern(1, "tools/call", obj([("name", "ra3_click".into()), ("arguments", obj([
        ("pid", fixture.pid.into()), ("x", 20u32.into()), ("y", 30u32.into()), ("hold_ms", 60000u32.into()),
    ]))])));
    assert_eq!(fixture.packets.recv_timeout(Duration::from_secs(3)).unwrap()[6], 1);
    assert_eq!(fixture.packets.recv_timeout(Duration::from_secs(3)).unwrap()[6], 2);
    client.send(obj([("jsonrpc", "2.0".into()), ("method", "notifications/cancelled".into()),
        ("params", obj([("requestId", 1u32.into())]))]));
    client.send(modern(2, "ping", empty()));
    client.reply(2); // No response to the cancelled ID.
    assert_eq!(fixture.finished.recv_timeout(Duration::from_secs(2)).unwrap().len(), 2);
    client.shutdown();
}

#[test]
fn pending_io_is_cancellable_and_eof_exits_promptly() {
    let fixture = fixture(true);
    let mut client = Client::new();
    client.send(modern(1, "tools/call", obj([("name", "ra3_ping".into()), ("arguments", args(fixture.pid))])));
    fixture.packets.recv_timeout(Duration::from_secs(3)).unwrap();
    client.shutdown(); // EOF cancels the pending overlapped read rather than hanging on it.
    assert_eq!(fixture.finished.recv_timeout(Duration::from_secs(2)).unwrap().len(), 1);
}

#[test]
fn stalled_bridge_times_out_without_replaying_and_server_stays_alive() {
    let fixture = fixture(true);
    let mut client = Client::new();
    let reply = client.call(1, "ra3_ping", args(fixture.pid));
    assert_eq!(reply.get("result").unwrap().get("isError"), Some(&Json::Bool(true)));
    assert_eq!(body(&reply).get("error").unwrap().get("outcome").and_then(Json::text), Some("unknown"));
    assert_eq!(fixture.finished.recv_timeout(Duration::from_secs(2)).unwrap().len(), 1);
    client.send(modern(2, "ping", empty()));
    client.reply(2);
    client.shutdown();
}

#[test]
fn existing_cli_click_and_drag_keep_their_wire_sequences() {
    for action in [
        vec!["click", "left", "800", "450", "1"],
        vec!["drag", "left", "900", "600", "300", "200", "1", "3"],
    ] {
        let fixture = fixture(false);
        let output = ProcessCommand::new(env!("CARGO_BIN_EXE_ra3ctl")).args(["--pid", &fixture.pid.to_string()])
            .args(&action).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("held=0"));
        let packets = fixture.finished.recv_timeout(Duration::from_secs(2)).unwrap();
        let expected = if action[0] == "click" { vec![1, 2, 3] } else { vec![1, 2, 1, 1, 1, 3] };
        assert_eq!(packets.iter().map(|p| p[6]).collect::<Vec<_>>(), expected);
    }
}
