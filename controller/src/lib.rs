mod ai_index;
mod json;
mod pipe;
pub mod mcp;
pub mod probe;
use std::io;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

pub const MAGIC: u32 = 0x504d3352;
pub const VERSION: u16 = 1;
pub const PACKET_SIZE: usize = 24;

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum Op { Ping = 0, Move = 1, Down = 2, Up = 3, Wheel = 4, ReleaseAll = 5 }

#[derive(Clone, Copy, Debug)]
pub struct Command {
    pub op: Op,
    pub button: u8,
    pub x: i32,
    pub y: i32,
    pub value: i32,
}
impl Command {
    pub fn at(op: Op, button: u8, x: i32, y: i32) -> Self {
        Self { op, button, x, y, value: 0 }
    }
    pub fn encode(self, sequence: u32) -> [u8; PACKET_SIZE] {
        let mut p = [0; PACKET_SIZE];
        p[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        p[4..6].copy_from_slice(&VERSION.to_le_bytes());
        p[6] = self.op as u8;
        p[7] = self.button;
        p[8..12].copy_from_slice(&sequence.to_le_bytes());
        p[12..16].copy_from_slice(&self.x.to_le_bytes());
        p[16..20].copy_from_slice(&self.y.to_le_bytes());
        p[20..24].copy_from_slice(&self.value.to_le_bytes());
        p
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ack {
    pub status: u8,
    pub held: u8,
    pub sequence: u32,
    pub pump: u32,
    pub detail: u32,
}
fn u32_at(p: &[u8], start: usize) -> u32 {
    u32::from_le_bytes(p[start..start + 4].try_into().expect("fixed packet field"))
}
fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
impl Ack {
    pub fn decode(p: &[u8; PACKET_SIZE], sequence: u32) -> io::Result<Self> {
        if u32_at(p, 0) != MAGIC || u16::from_le_bytes([p[4], p[5]]) != VERSION {
            return Err(invalid("bridge protocol magic/version mismatch"));
        }
        if p[6] > 4 || p[7] & !7 != 0 || u32_at(p, 20) != 0 {
            return Err(invalid("malformed bridge acknowledgement"));
        }
        if u32_at(p, 8) != sequence { return Err(invalid("bridge sequence mismatch")); }
        Ok(Self { status: p[6], held: p[7], sequence, pump: u32_at(p, 12), detail: u32_at(p, 16) })
    }
    fn check(self) -> io::Result<Self> {
        if self.status == 0 { return Ok(self); }
        let reason = match self.status {
            1 => "invalid command, coordinates, or button state",
            2 => "game input/window is not ready",
            3 => "bridge queue is full",
            4 => "game pump timed out; outcome may be unknown; do not automatically retry",
            _ => "unknown bridge error",
        };
        Err(io::Error::new(io::ErrorKind::Other,
            format!("{reason} (status={}, detail={}, pump={}, held={})", self.status, self.detail, self.pump, self.held)))
    }
}

pub struct Session { pipe: pipe::Pipe, next_sequence: u32, poisoned: bool }
impl Session {
    pub fn connect(pid: u32) -> io::Result<Self> {
        Self::connect_cancellable(pid, &AtomicBool::new(false))
    }
    pub(crate) fn connect_cancellable(pid: u32, cancel: &AtomicBool) -> io::Result<Self> {
        Ok(Self { pipe: pipe::Pipe::connect(pid, cancel)?, next_sequence: 1, poisoned: false })
    }
    pub fn send(&mut self, command: Command) -> io::Result<Ack> {
        self.send_cancellable(command, &AtomicBool::new(false))
    }
    pub(crate) fn send_cancellable(&mut self, command: Command, cancel: &AtomicBool) -> io::Result<Ack> {
        if self.poisoned { return Err(io::Error::new(io::ErrorKind::BrokenPipe, "session failed; close it before reconnecting")); }
        pipe::check_cancel(cancel)?;
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        let result = (|| {
            self.pipe.write(&command.encode(sequence), cancel)?;
            let mut reply = [0; PACKET_SIZE];
            self.pipe.read(&mut reply, cancel)?;
            Ack::decode(&reply, sequence)?.check()
        })();
        if result.is_err() { self.poisoned = true; }
        result
    }
    pub fn release_all(&mut self) -> io::Result<Ack> {
        self.send(Command::at(Op::ReleaseAll, 0, 0, 0))
    }
    pub fn click(&mut self, button: u8, x: i32, y: i32, hold: Duration) -> io::Result<Ack> {
        self.click_cancellable(button, x, y, hold, &AtomicBool::new(false))
    }
    pub(crate) fn click_cancellable(&mut self, button: u8, x: i32, y: i32, hold: Duration,
                                    cancel: &AtomicBool) -> io::Result<Ack> {
        let result = (|| {
            self.send_cancellable(Command::at(Op::Move, 0, x, y), cancel)?;
            self.send_cancellable(Command::at(Op::Down, button, x, y), cancel)?;
            pipe::wait_until(Instant::now() + hold, cancel)?;
            self.send_cancellable(Command::at(Op::Up, button, x, y), cancel)
        })();
        if result.is_err() { self.poisoned = true; } // Owner closes; native bridge requests release on disconnect.
        result
    }
    pub fn drag(&mut self, button: u8, start: (i32, i32), end: (i32, i32),
                duration: Duration, steps: u32) -> io::Result<Ack> {
        self.drag_cancellable(button, start, end, duration, steps, &AtomicBool::new(false))
    }
    pub(crate) fn drag_cancellable(&mut self, button: u8, start: (i32, i32), end: (i32, i32),
                duration: Duration, steps: u32, cancel: &AtomicBool) -> io::Result<Ack> {
        if steps == 0 || steps > 1000 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "drag steps must be 1..1000"));
        }
        let result = (|| {
            self.send_cancellable(Command::at(Op::Move, 0, start.0, start.1), cancel)?;
            self.send_cancellable(Command::at(Op::Down, button, start.0, start.1), cancel)?;
            let began = Instant::now();
            for step in 1..=steps {
                // Each exchange waits for a game pump, so down/move/up cannot collapse
                // into one frame. Frame rate can extend the requested duration.
                let due = began + duration.mul_f64(f64::from(step) / f64::from(steps));
                pipe::wait_until(due, cancel)?;
                let point = interpolate(start, end, step, steps);
                self.send_cancellable(Command::at(Op::Move, 0, point.0, point.1), cancel)?;
            }
            self.send_cancellable(Command::at(Op::Up, button, end.0, end.1), cancel)
        })();
        if result.is_err() { self.poisoned = true; }
        result
    }
}

// Integer interpolation, including decreasing axes; the final sample is exact.
pub fn interpolate(start: (i32, i32), end: (i32, i32), step: u32, steps: u32) -> (i32, i32) {
    assert!(steps > 0 && step <= steps);
    let axis = |a: i32, b: i32| -> i32 {
        (i64::from(a) + (i64::from(b) - i64::from(a)) * i64::from(step) / i64::from(steps)) as i32
    };
    (axis(start.0, end.0), axis(start.1, end.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_golden() {
        let c = Command { op: Op::Wheel, button: 0, x: 800, y: 600, value: -2 };
        assert_eq!(c.encode(0x12345678), [0x52, 0x33, 0x4d, 0x50, 1, 0, 4, 0,
            0x78, 0x56, 0x34, 0x12, 0x20, 3, 0, 0, 0x58, 2, 0, 0, 0xfe, 0xff, 0xff, 0xff]);
    }
    #[test]
    fn ack_rejects_wrong_peer_or_sequence() {
        let mut p = Command::at(Op::Ping, 0, 42, 9).encode(7);
        p[7] = 5; // reply held mask
        assert_eq!(Ack::decode(&p, 7).unwrap(), Ack { status: 0, held: 5, sequence: 7, pump: 42, detail: 9 });
        assert!(Ack::decode(&p, 8).is_err());
        p[0] ^= 1; assert!(Ack::decode(&p, 7).is_err()); p[0] ^= 1;
        p[7] = 8; assert!(Ack::decode(&p, 7).is_err()); p[7] = 0;
        p[20] = 1; assert!(Ack::decode(&p, 7).is_err());
    }
    #[test]
    fn drag_stays_within_endpoints_and_finishes_exactly() {
        for (a, b) in [((100, 600), (900, 100)), ((900, 100), (100, 600)), ((0, 0), (65535, 65535))] {
            assert_eq!(interpolate(a, b, 0, 37), a);
            assert_eq!(interpolate(a, b, 37, 37), b);
            for step in 0..=37 {
                let p = interpolate(a, b, step, 37);
                assert!((a.0.min(b.0)..=a.0.max(b.0)).contains(&p.0));
                assert!((a.1.min(b.1)..=a.1.max(b.1)).contains(&p.1));
            }
        }
    }
}
