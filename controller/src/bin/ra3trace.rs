//! Windows debug-event launch tracer. No DLL injection or target memory writes.
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

const HELP: &str = "ra3trace --exe <RA3.exe or runme.exe> [--seconds 90] [--out trace.jsonl] -- <game arguments>\n\
Starts the executable with DEBUG_PROCESS and records its debugged descendants,\n\
image/DLL paths, debug strings, exceptions and exits. Ctrl+C detaches.\n\
Default log: logs/ra3trace-<time>-<pid>.jsonl beside ra3trace.exe.\n\
This is an observing debugger, not an injector. Debugging can change startup behavior.\n";

struct Options {
    executable: PathBuf,
    arguments: Vec<OsString>,
    seconds: u64,
    output: PathBuf,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn parse(arguments: Vec<OsString>) -> io::Result<Option<Options>> {
    if arguments.is_empty() || arguments.first().is_some_and(|v| v == "--help" || v == "-h") {
        return Ok(None);
    }
    let mut executable = None;
    let mut output = None;
    let mut seconds = 90;
    let mut game_arguments = Vec::new();
    let mut iter = arguments.into_iter();
    while let Some(flag) = iter.next() {
        if flag == "--" {
            game_arguments.extend(iter);
            break;
        }
        let value = iter.next().ok_or_else(|| invalid("missing option value"))?;
        match flag.to_str() {
            Some("--exe") => executable = Some(PathBuf::from(value)),
            Some("--out") => output = Some(PathBuf::from(value)),
            Some("--seconds") => {
                seconds = value.to_str().and_then(|s| s.parse::<u64>().ok())
                    .filter(|n| (1..=600).contains(n))
                    .ok_or_else(|| invalid("--seconds must be 1..600"))?;
            }
            _ => return Err(invalid("unknown option; put game arguments after --")),
        }
    }
    let executable = executable.ok_or_else(|| invalid("--exe is required"))?.canonicalize()?;
    if !executable.is_file() { return Err(invalid("--exe must be a file")); }
    let output = match output {
        Some(path) => path,
        None => {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?.as_millis();
            std::env::current_exe()?.parent().ok_or_else(|| invalid("no executable directory"))?
                .join("logs").join(format!("ra3trace-{stamp}-{}.jsonl", std::process::id()))
        }
    };
    Ok(Some(Options { executable, arguments: game_arguments, seconds, output }))
}

// Quote only when needed. Older launchers may inspect the raw command line
// instead of using CRT argv rules, so harmless extra quotes can change behavior.
fn quote_argument(value: &[u16]) -> Vec<u16> {
    if !value.is_empty() && !value.iter().any(|&unit| matches!(unit, 0x20 | 0x09 | 0x22)) {
        return value.to_vec();
    }
    let mut result = vec![b'"' as u16];
    let mut slashes = 0;
    for &unit in value {
        if unit == b'\\' as u16 { slashes += 1; continue; }
        let count = if unit == b'"' as u16 { slashes * 2 + 1 } else { slashes };
        result.extend(std::iter::repeat_n(b'\\' as u16, count));
        result.push(unit);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    result.push(b'"' as u16);
    result
}

fn command_line(executable: &[u16], arguments: &[Vec<u16>]) -> Vec<u16> {
    let mut command = quote_argument(executable);
    for argument in arguments {
        command.push(b' ' as u16);
        command.extend(quote_argument(argument));
    }
    command
}

// canonicalize() produces verbatim Windows paths. Launch using conventional
// drive/UNC spelling so legacy launchers receive the same paths as a manual run.
fn launch_path(value: &[u16]) -> Vec<u16> {
    const VERBATIM: &[u16] = &[92, 92, 63, 92];
    const UNC: &[u16] = &[92, 92, 63, 92, 85, 78, 67, 92];
    if value.starts_with(UNC) {
        let mut ordinary = vec![92, 92];
        ordinary.extend_from_slice(&value[UNC.len()..]);
        ordinary
    } else if value.starts_with(VERBATIM) && value.len() >= 7
        && matches!(value[4], 65..=90 | 97..=122) && value[5] == 58 && value[6] == 92 {
        value[VERBATIM.len()..].to_vec()
    } else {
        value.to_vec()
    }
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::{c_void, OsStr};
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    type Handle = *mut c_void;
    #[repr(C)]
    struct StartupInfo {
        cb: u32, reserved: *mut u16, desktop: *mut u16, title: *mut u16,
        x: u32, y: u32, x_size: u32, y_size: u32, x_chars: u32, y_chars: u32,
        fill: u32, flags: u32, show: u16, reserved_size: u16, reserved_bytes: *mut u8,
        stdin: Handle, stdout: Handle, stderr: Handle,
    }
    #[repr(C)]
    struct ProcessInformation { process: Handle, thread: Handle, pid: u32, tid: u32 }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ExceptionRecord {
        code: u32, flags: u32, nested: *mut c_void, address: *mut c_void,
        count: u32, information: [usize; 15],
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ExceptionInfo { record: ExceptionRecord, first_chance: u32 }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CreateProcessInfo {
        file: Handle, process: Handle, thread: Handle, base: *mut c_void,
        debug_offset: u32, debug_size: u32, tls: *mut c_void, start: *mut c_void,
        image_name: *mut c_void, unicode: u16,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct LoadDllInfo {
        file: Handle, base: *mut c_void, debug_offset: u32, debug_size: u32,
        image_name: *mut c_void, unicode: u16,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct DebugStringInfo { address: *mut c_void, unicode: u16, length: u16 }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RipInfo { error: u32, kind: u32 }
    #[repr(C)]
    union DebugData {
        exception: ExceptionInfo, create_process: CreateProcessInfo, load_dll: LoadDllInfo,
        debug_string: DebugStringInfo, rip: RipInfo, exit_code: u32, base: *mut c_void,
    }
    #[repr(C)]
    struct DebugEvent { code: u32, pid: u32, tid: u32, data: DebugData }
    #[repr(C)]
    struct ProcessEntry {
        size: u32, usage: u32, pid: u32, heap: usize, module: u32, threads: u32,
        parent: u32, priority: i32, flags: u32, name: [u16; 260],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateProcessW(exe: *const u16, command: *mut u16, process_attributes: *const c_void,
            thread_attributes: *const c_void, inherit: i32, flags: u32, environment: *const c_void,
            directory: *const u16, startup: *mut StartupInfo, process: *mut ProcessInformation) -> i32;
        fn WaitForDebugEventEx(event: *mut DebugEvent, milliseconds: u32) -> i32;
        fn ContinueDebugEvent(pid: u32, tid: u32, status: u32) -> i32;
        fn DebugSetProcessKillOnExit(kill: i32) -> i32;
        fn DebugActiveProcessStop(pid: u32) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetFinalPathNameByHandleW(file: Handle, buffer: *mut u16, length: u32, flags: u32) -> u32;
        fn QueryFullProcessImageNameW(process: Handle, flags: u32, buffer: *mut u16, length: *mut u32) -> i32;
        fn K32GetMappedFileNameW(process: Handle, address: *const c_void, buffer: *mut u16, length: u32) -> u32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
        fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
        fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
        fn ReadProcessMemory(process: Handle, address: *const c_void, buffer: *mut c_void,
            size: usize, read: *mut usize) -> i32;
        fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
        #[cfg(test)] fn OutputDebugStringW(message: *const u16);
        #[cfg(test)] fn GetCommandLineW() -> *const u16;
    }
    struct OwnedHandle(Handle);
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if !self.0.is_null() && self.0 as isize != -1 { unsafe { CloseHandle(self.0); } }
        }
    }
    static STOP: AtomicBool = AtomicBool::new(false);
    unsafe extern "system" fn control(event: u32) -> i32 {
        if event <= 1 { STOP.store(true, Ordering::Relaxed); 1 } else { 0 }
    }
    struct ControlGuard(bool);
    impl Drop for ControlGuard {
        fn drop(&mut self) { if self.0 { unsafe { SetConsoleCtrlHandler(Some(control), 0); } } }
    }
    struct ProcessState { handle: Handle, breakpoints: BTreeSet<u32> }
    struct DebugSession { active: BTreeMap<u32, ProcessState> }
    impl Drop for DebugSession {
        fn drop(&mut self) {
            for pid in self.active.keys() {
                if unsafe { DebugActiveProcessStop(*pid) } == 0 {
                    eprintln!("ra3trace: detach PID {pid}: {}", io::Error::last_os_error());
                }
            }
        }
    }
    struct Log { file: File, started: Instant, quiet: bool }
    impl Log {
        fn event(&mut self, pid: u32, tid: u32, name: &str, fields: &[(&str, String)], important: bool) -> io::Result<()> {
            let elapsed = self.started.elapsed().as_millis();
            let mut line = format!("{{\"elapsed_ms\":{elapsed},\"pid\":{pid},\"tid\":{tid},\"event\":{}", json_string(name));
            for (key, value) in fields { line += &format!(",{}:{}", json_string(key), json_string(value)); }
            line.push('}');
            writeln!(self.file, "{line}")?;
            self.file.flush()?;
            if important && !self.quiet {
                println!("[{elapsed}ms pid={pid}] {name} {}", fields.iter()
                    .map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join(" "));
            }
            Ok(())
        }
    }
    fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
        let mut value: Vec<u16> = value.encode_wide().collect();
        if value.contains(&0) { return Err(invalid("embedded NUL in a path or argument")); }
        value.push(0);
        Ok(value)
    }
    fn parent_pid(pid: u32) -> Option<u32> {
        unsafe {
            let snapshot = OwnedHandle(CreateToolhelp32Snapshot(2, 0));
            if snapshot.0 as isize == -1 { return None; }
            let mut entry: ProcessEntry = zeroed();
            entry.size = size_of::<ProcessEntry>() as u32;
            let mut ok = Process32FirstW(snapshot.0, &mut entry);
            while ok != 0 {
                if entry.pid == pid { return Some(entry.parent); }
                ok = Process32NextW(snapshot.0, &mut entry);
            }
            None
        }
    }
    fn image_path(file: Handle, process: Handle, base: *mut c_void, executable: bool) -> String {
        let mut buffer = vec![0u16; 32768];
        unsafe {
            if !file.is_null() && file as isize != -1 {
                let n = GetFinalPathNameByHandleW(file, buffer.as_mut_ptr(), buffer.len() as u32, 0);
                if n > 0 && (n as usize) < buffer.len() { return String::from_utf16_lossy(&buffer[..n as usize]); }
            }
            if !process.is_null() {
                let mut n = buffer.len() as u32;
                if executable && QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut n) != 0 {
                    return String::from_utf16_lossy(&buffer[..n as usize]);
                }
                let n = K32GetMappedFileNameW(process, base, buffer.as_mut_ptr(), buffer.len() as u32);
                if n > 0 && (n as usize) < buffer.len() { return String::from_utf16_lossy(&buffer[..n as usize]); }
            }
        }
        "<path unavailable>".into()
    }
    fn debug_string(process: Handle, info: DebugStringInfo) -> String {
        let bytes = (info.length as usize * if info.unicode != 0 { 2 } else { 1 }).min(32768);
        let mut buffer = vec![0u8; bytes];
        let mut read = 0;
        if process.is_null() || unsafe { ReadProcessMemory(process, info.address, buffer.as_mut_ptr().cast(), bytes, &mut read) } == 0 {
            return format!("<debug string unavailable: {}>", io::Error::last_os_error());
        }
        buffer.truncate(read);
        let text = if info.unicode != 0 {
            String::from_utf16_lossy(&buffer.chunks_exact(2).map(|v| u16::from_le_bytes([v[0], v[1]])).collect::<Vec<_>>())
        } else { String::from_utf8_lossy(&buffer).into_owned() };
        text.trim_end_matches('\0').to_owned()
    }
    fn interesting(path: &str) -> bool {
        let path = path.to_lowercase();
        ["d3d9.dll", "winmm.dll", "ra3inputbridge.asi", "steam_api.dll"].iter()
            .any(|name| path.ends_with(name))
    }
    #[derive(Default)]
    pub(super) struct Summary { pub processes: usize, pub dlls: usize, pub debug_strings: usize, pub nonzero_exits: usize }

    pub(super) fn trace(options: Options, quiet: bool) -> io::Result<Summary> {
        let executable = PathBuf::from(OsString::from_wide(&launch_path(
            &options.executable.as_os_str().encode_wide().collect::<Vec<_>>())));
        let exe = wide(executable.as_os_str())?;
        let directory = wide(executable.parent().ok_or_else(|| invalid("no working directory"))?.as_os_str())?;
        let arguments = options.arguments.iter().map(|argument| {
            let mut value = wide(argument)?;
            value.pop(); // command_line adds the final NUL once, after all arguments.
            Ok(value)
        }).collect::<io::Result<Vec<_>>>()?;
        let mut command = command_line(&exe[..exe.len() - 1], &arguments);
        let display_command = String::from_utf16_lossy(&command);
        command.push(0);
        if let Some(parent) = options.output.parent().filter(|p| !p.as_os_str().is_empty()) { std::fs::create_dir_all(parent)?; }
        let mut log = Log { file: OpenOptions::new().write(true).create_new(true).open(&options.output)?, started: Instant::now(), quiet };
        log.event(0, 0, "trace_start", &[("command", display_command), ("cwd", executable.parent().unwrap().display().to_string()),
            ("seconds", options.seconds.to_string()), ("mode", "DEBUG_PROCESS; observing debugger; no injection".into())], true)?;
        if !quiet { println!("Log: {}", options.output.display()); }
        STOP.store(false, Ordering::Relaxed);
        let control_registered = unsafe { SetConsoleCtrlHandler(Some(control), 1) } != 0;
        let _control = ControlGuard(control_registered);
        if !control_registered {
            log.event(0, 0, "console_control_unavailable", &[("error", io::Error::last_os_error().to_string())], true)?;
        }
        let mut startup: StartupInfo = unsafe { zeroed() };
        startup.cb = size_of::<StartupInfo>() as u32;
        let mut information: ProcessInformation = unsafe { zeroed() };
        // DEBUG_PROCESS follows descendants. Do not use DEBUG_ONLY_THIS_PROCESS.
        if unsafe { CreateProcessW(exe.as_ptr(), command.as_mut_ptr(), null(), null(), 0,
            0x00000001 | 0x00000200, null(), directory.as_ptr(), &mut startup, &mut information) } == 0 {
            let error = io::Error::last_os_error();
            log.event(0, 0, "launch_error", &[("error", error.to_string())], true)?;
            return Err(error);
        }
        let mut process_owner = OwnedHandle(information.process);
        let mut thread_owner = OwnedHandle(information.thread);
        let mut session = DebugSession { active: BTreeMap::new() };
        session.active.insert(information.pid, ProcessState { handle: information.process, breakpoints: BTreeSet::new() });
        // On tracer failure or exit, keep the debugged application alive.
        if unsafe { DebugSetProcessKillOnExit(0) } == 0 { return Err(io::Error::last_os_error()); }
        let mut summary = Summary::default();
        while !session.active.is_empty() && !STOP.load(Ordering::Relaxed) && log.started.elapsed() < Duration::from_secs(options.seconds) {
            let mut event: DebugEvent = unsafe { zeroed() };
            if unsafe { WaitForDebugEventEx(&mut event, 100) } == 0 {
                let error = io::Error::last_os_error();
                if matches!(error.raw_os_error(), Some(121) | Some(258)) { continue; }
                return Err(error);
            }
            let mut status = 0x00010002; // DBG_CONTINUE for non-exception notifications.
            if event.code == 1 {
                let info = unsafe { event.data.exception };
                let initial = info.first_chance != 0 && matches!(info.record.code, 0x80000003 | 0x4000001f)
                    && session.active.get_mut(&event.pid).is_some_and(|p| p.breakpoints.insert(info.record.code));
                // Consume only initial native/WOW64 loader breakpoints; pass real exceptions to the application.
                if !initial { status = 0x80010001; }
            }
            let result = (|| -> io::Result<()> {
                match event.code {
                    3 => {
                        let info = unsafe { event.data.create_process };
                        let _file = OwnedHandle(info.file);
                        // Debug-event process/thread handles are owned by Windows until exit/detach.
                        // Avoid closing twice if a returned launch handle aliases one of them.
                        if info.process == process_owner.0 { process_owner.0 = null_mut(); }
                        if info.thread == thread_owner.0 { thread_owner.0 = null_mut(); }
                        session.active.insert(event.pid, ProcessState { handle: info.process, breakpoints: BTreeSet::new() });
                        summary.processes += 1;
                        log.event(event.pid, event.tid, "process_create", &[("parent_pid", parent_pid(event.pid).map_or_else(|| "unknown".into(), |p| p.to_string())),
                            ("path", image_path(info.file, info.process, info.base, true)), ("base", format!("{:p}", info.base))], true)
                    }
                    6 => {
                        let info = unsafe { event.data.load_dll };
                        let _file = OwnedHandle(info.file);
                        let process = session.active.get(&event.pid).map_or(null_mut(), |p| p.handle);
                        let path = image_path(info.file, process, info.base, false);
                        let important = interesting(&path);
                        summary.dlls += 1;
                        log.event(event.pid, event.tid, "dll_load", &[("path", path), ("base", format!("{:p}", info.base))], important)
                    }
                    7 => log.event(event.pid, event.tid, "dll_unload", &[("base", format!("{:p}", unsafe { event.data.base }))], false),
                    5 => {
                        let code = unsafe { event.data.exit_code };
                        if code != 0 { summary.nonzero_exits += 1; }
                        log.event(event.pid, event.tid, "process_exit", &[("code", code.to_string())], true)
                    }
                    1 => {
                        let info = unsafe { event.data.exception };
                        log.event(event.pid, event.tid, "exception", &[("code", format!("0x{:08X}", info.record.code)),
                            ("first_chance", info.first_chance.to_string()), ("address", format!("{:p}", info.record.address)),
                            ("continue", format!("0x{status:08X}"))], info.first_chance == 0)
                    }
                    8 => {
                        let info = unsafe { event.data.debug_string };
                        let process = session.active.get(&event.pid).map_or(null_mut(), |p| p.handle);
                        let message = debug_string(process, info);
                        let important = message.contains("RA3InputBridge") || message.contains("Refused:") || message.contains("Host executable:");
                        summary.debug_strings += 1;
                        log.event(event.pid, event.tid, "debug_string", &[("message", message)], important)
                    }
                    9 => {
                        let info = unsafe { event.data.rip };
                        log.event(event.pid, event.tid, "debug_error", &[("error", info.error.to_string()), ("type", info.kind.to_string())], true)
                    }
                    _ => log.event(event.pid, event.tid, "debug_event", &[("code", event.code.to_string())], false),
                }
            })();
            // Always resume the event before propagating I/O errors or detaching.
            let continued = unsafe { ContinueDebugEvent(event.pid, event.tid, status) };
            let continue_error = if continued == 0 { Some(io::Error::last_os_error()) } else { None };
            if continued != 0 && event.code == 5 { session.active.remove(&event.pid); }
            result?;
            if let Some(error) = continue_error { return Err(error); }
        }
        let reason = if session.active.is_empty() { "all_debugged_processes_exited" }
            else if STOP.load(Ordering::Relaxed) { "ctrl_c" } else { "time_limit" };
        log.event(0, 0, "trace_stop", &[("reason", reason.into()), ("processes", summary.processes.to_string()),
            ("dll_loads", summary.dlls.to_string()), ("debug_strings", summary.debug_strings.to_string()),
            ("nonzero_exits", summary.nonzero_exits.to_string()),
            ("detaching_pids", session.active.keys().map(u32::to_string).collect::<Vec<_>>().join(","))], true)?;
        drop(session);
        Ok(summary)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn windows_debug_abi_sizes() {
            assert_eq!(size_of::<DebugEvent>(), if size_of::<usize>() == 8 { 176 } else { 96 });
            assert_eq!(size_of::<StartupInfo>(), if size_of::<usize>() == 8 { 104 } else { 68 });
            assert_eq!(size_of::<ProcessEntry>(), if size_of::<usize>() == 8 { 568 } else { 556 });
        }
        #[test]
        #[ignore = "subprocess fixture, invoked only by debug_event_smoke"]
        fn trace_leaf_fixture() {
            let text: Vec<u16> = "ra3trace Unicode fixture: 路径\0".encode_utf16().collect();
            unsafe { OutputDebugStringW(text.as_ptr()); }
        }
        #[test]
        #[ignore = "subprocess fixture, invoked only by debug_event_smoke"]
        fn trace_child_fixture() {
            // Check the actual raw command delivered by CreateProcessW, since
            // CRT argv alone would accept both the broken and corrected forms.
            let raw = unsafe {
                let pointer = GetCommandLineW();
                let mut length = 0;
                while *pointer.add(length) != 0 { length += 1; }
                String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length))
            };
            assert!(raw.ends_with(" --ignored --exact windows::tests::trace_child_fixture --nocapture"), "{raw}");
            assert!(!raw.contains(r"\\?\"), "verbatim prefix leaked into child command: {raw}");
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "windows::tests::trace_leaf_fixture", "--nocapture"])
                .status().unwrap();
            assert!(status.success());
        }
        #[test]
        fn debug_event_smoke() {
            let path = std::env::temp_dir().join(format!("ra3trace-test-{}-{}.jsonl", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            let options = Options { executable: std::env::current_exe().unwrap(), seconds: 15, output: path.clone(),
                arguments: ["--ignored", "--exact", "windows::tests::trace_child_fixture", "--nocapture"].map(OsString::from).to_vec() };
            let result = trace(options, true).unwrap();
            let records = std::fs::read_to_string(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            assert_eq!(result.processes, 2, "must capture fixture and its child");
            assert_eq!(result.nonzero_exits, 0, "both fixture test processes must succeed");
            assert!(result.dlls > 0);
            assert!(records.contains("ra3trace Unicode fixture: 路径"));
            assert!(records.contains("all_debugged_processes_exited"), "fixtures must exit without a timeout");
            // When the project's Win32 native test executable is available, also verify WOW64 events.
            let native = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
                .join("build/win32/Release/ra3_core_tests.exe");
            if size_of::<usize>() == 8 && native.is_file() {
                let wow_log = std::env::temp_dir().join(format!("ra3trace-wow64-{}-{}.jsonl", std::process::id(),
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
                let wow = trace(Options { executable: native, seconds: 15, output: wow_log.clone(), arguments: vec![] }, true).unwrap();
                let records = std::fs::read_to_string(&wow_log).unwrap();
                std::fs::remove_file(wow_log).unwrap();
                assert_eq!(wow.processes, 1);
                assert_eq!(wow.nonzero_exits, 0);
                assert!(records.contains("all_debugged_processes_exited"));
                assert!(records.contains("\"event\":\"process_exit\",\"code\":\"0\""));
                println!("ra3trace WOW64 native fixture passed");
            }
        }
    }
}

fn run() -> io::Result<()> {
    let Some(options) = parse(std::env::args_os().skip(1).collect())? else { print!("{HELP}"); return Ok(()); };
    #[cfg(windows)]
    { windows::trace(options, false)?; Ok(()) }
    #[cfg(not(windows))]
    { let _ = options; Err(io::Error::new(io::ErrorKind::Unsupported, "ra3trace requires Windows")) }
}
fn main() {
    if let Err(error) = run() { eprintln!("ra3trace: {error}"); std::process::exit(1); }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn quoted(value: &str) -> String { String::from_utf16(&quote_argument(&value.encode_utf16().collect::<Vec<_>>())).unwrap() }
    #[test]
    fn command_line_preserves_spaces_quotes_and_trailing_slashes() {
        assert_eq!(quoted(""), "\"\"");
        assert_eq!(quoted("C:\\Program Files\\RA3.exe"), "\"C:\\Program Files\\RA3.exe\"");
        assert_eq!(quoted("a\"b"), "\"a\\\"b\"");
        assert_eq!(quoted("C:\\game\\"), "C:\\game\\");
        assert_eq!(quoted("C:\\game dir\\"), "\"C:\\game dir\\\\\"");
        assert_eq!(quoted("-runver"), "-runver");
        assert_eq!(quoted("1.12"), "1.12");
        assert_eq!(quoted("a\tb"), "\"a\tb\"");
    }
    #[test]
    fn ra3_command_matches_working_manual_launch() {
        let exe = r"\\?\C:\Program Files (x86)\Steam\steamapps\common\Command and Conquer Red Alert 3\RA3.exe";
        let arguments = ["-win", "-xres", "1920", "-yres", "1080", "-runver", "1.12"]
            .map(|value| value.encode_utf16().collect()).to_vec();
        let command = command_line(&launch_path(&exe.encode_utf16().collect::<Vec<_>>()), &arguments);
        assert_eq!(String::from_utf16(&command).unwrap(),
            r#""C:\Program Files (x86)\Steam\steamapps\common\Command and Conquer Red Alert 3\RA3.exe" -win -xres 1920 -yres 1080 -runver 1.12"#);
    }
    #[test]
    fn launch_paths_keep_drive_unc_and_utf16_spelling() {
        for (input, expected) in [
            (r"\\?\C:\游戏\RA3.exe", r"C:\游戏\RA3.exe"),
            (r"\\?\UNC\server\share\RA3.exe", r"\\server\share\RA3.exe"),
            (r"C:\game\RA3.exe", r"C:\game\RA3.exe"),
            (r"\\?\Volume{123}\game", r"\\?\Volume{123}\game"),
        ] {
            assert_eq!(launch_path(&input.encode_utf16().collect::<Vec<_>>()), expected.encode_utf16().collect::<Vec<_>>());
        }
        let units = [92, 92, 63, 92, 67, 58, 92, 0xd800];
        assert_eq!(launch_path(&units), units[4..]);
    }
    #[test]
    fn trace_json_escapes_paths_and_control_characters() {
        assert_eq!(json_string("C:\\路径\n\"\0"), "\"C:\\\\路径\\n\\\"\\u0000\"");
    }
}
