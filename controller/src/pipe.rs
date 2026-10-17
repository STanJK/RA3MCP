use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "request cancelled; any partial input cannot be rolled back")
}
pub(crate) fn check_cancel(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Acquire) { Err(cancelled()) } else { Ok(()) }
}
pub(crate) fn wait_until(due: Instant, cancel: &AtomicBool) -> io::Result<()> {
    loop {
        check_cancel(cancel)?;
        match due.checked_duration_since(Instant::now()) {
            None => return Ok(()),
            Some(delay) => thread::sleep(delay.min(Duration::from_millis(10))),
        }
    }
}

pub(crate) struct Pipe {
    #[cfg(windows)] file: std::fs::File,
    #[cfg(windows)] timeout: Duration,
}
impl Pipe {
    pub fn connect(pid: u32, cancel: &AtomicBool) -> io::Result<Self> {
        if pid == 0 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "PID must be nonzero")); }
        #[cfg(not(windows))]
        { let _ = cancel; Err(io::Error::new(io::ErrorKind::Unsupported, "the game pipe requires Windows")) }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let path = format!(r"\\.\pipe\RA3MCP.{pid}");
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                check_cancel(cancel)?;
                match std::fs::OpenOptions::new().read(true).write(true).custom_flags(0x40000000).open(&path) {
                    Ok(file) => return Ok(Self { file, timeout: Duration::from_secs(5) }),
                    Err(error) if matches!(error.raw_os_error(), Some(2) | Some(231)) && Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(25));
                    }
                    Err(error) => return Err(io::Error::new(error.kind(),
                        format!("cannot open {path}: {error}; check bridge log, PID, and exclusive pipe ownership"))),
                }
            }
        }
    }
    pub fn write(&self, bytes: &[u8], cancel: &AtomicBool) -> io::Result<()> {
        self.transfer(bytes.as_ptr() as *mut u8, bytes.len(), false, cancel)
    }
    pub fn read(&self, bytes: &mut [u8], cancel: &AtomicBool) -> io::Result<()> {
        self.transfer(bytes.as_mut_ptr(), bytes.len(), true, cancel)
    }
    fn transfer(&self, bytes: *mut u8, length: usize, read: bool, cancel: &AtomicBool) -> io::Result<()> {
        #[cfg(not(windows))]
        { let _ = (bytes, length, read, cancel); Err(io::Error::new(io::ErrorKind::Unsupported, "Windows required")) }
        #[cfg(windows)]
        { windows::transfer(&self.file, bytes, length, read, cancel, self.timeout) }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::ptr::null;
    type Handle = *mut c_void;
    #[repr(C)]
    struct Overlapped { internal: usize, internal_high: usize, offset: u32, offset_high: u32, event: Handle }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateEventW(attributes: *const c_void, manual: i32, initial: i32, name: *const u16) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn ReadFile(file: Handle, buffer: *mut c_void, size: u32, read: *mut u32, overlapped: *mut Overlapped) -> i32;
        fn WriteFile(file: Handle, buffer: *const c_void, size: u32, written: *mut u32, overlapped: *mut Overlapped) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn GetOverlappedResult(file: Handle, overlapped: *mut Overlapped, transferred: *mut u32, wait: i32) -> i32;
        fn CancelIoEx(file: Handle, overlapped: *mut Overlapped) -> i32;
    }
    struct Event(Handle);
    impl Drop for Event { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
    // Must drain cancellation before the stack OVERLAPPED or packet buffer can disappear.
    fn abort(file: Handle, overlapped: &mut Overlapped) {
        unsafe {
            CancelIoEx(file, overlapped);
            let mut ignored = 0;
            GetOverlappedResult(file, overlapped, &mut ignored, 1);
        }
    }
    pub(super) fn transfer(file: &std::fs::File, bytes: *mut u8, length: usize, read: bool,
                          cancel: &AtomicBool, timeout: Duration) -> io::Result<()> {
        let file = file.as_raw_handle();
        let deadline = Instant::now() + timeout;
        let mut offset = 0;
        while offset < length {
            check_cancel(cancel)?;
            let event = Event(unsafe { CreateEventW(null(), 1, 0, null()) });
            if event.0.is_null() { return Err(io::Error::last_os_error()); }
            let mut overlapped = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: event.0 };
            let mut transferred = 0;
            let started = unsafe {
                if read { ReadFile(file, bytes.add(offset).cast(), (length - offset) as u32, &mut transferred, &mut overlapped) }
                else { WriteFile(file, bytes.add(offset).cast(), (length - offset) as u32, &mut transferred, &mut overlapped) }
            };
            if started == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(997) { return Err(error); } // ERROR_IO_PENDING
                loop {
                    if cancel.load(Ordering::Acquire) {
                        abort(file, &mut overlapped); return Err(cancelled());
                    }
                    if Instant::now() >= deadline {
                        abort(file, &mut overlapped);
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "pipe I/O timed out; input outcome may be unknown; do not automatically retry"));
                    }
                    match unsafe { WaitForSingleObject(event.0, 10) } {
                        0 => break,
                        258 => continue,
                        _ => {
                            let error = io::Error::last_os_error();
                            abort(file, &mut overlapped); return Err(error);
                        }
                    }
                }
            }
            // Read the final count for both immediate and pending completions.
            if unsafe { GetOverlappedResult(file, &mut overlapped, &mut transferred, 0) } == 0 {
                let error = io::Error::last_os_error();
                abort(file, &mut overlapped);
                return Err(error);
            }
            if transferred == 0 {
                return Err(io::Error::new(if read { io::ErrorKind::UnexpectedEof } else { io::ErrorKind::WriteZero }, "bridge pipe closed"));
            }
            offset += transferred as usize;
        }
        Ok(())
    }
    #[cfg(test)]
    #[test]
    fn overlapped_abi_matches_windows() {
        assert_eq!(std::mem::size_of::<Overlapped>(), if std::mem::size_of::<usize>() == 8 { 32 } else { 20 });
    }
}
