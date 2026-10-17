use ra3ctl::{Command, Op, Session};
use std::env;
use std::io;
use std::time::Duration;

const HELP: &str = "ra3ctl --pid <game PID> <command>
  ping
  move <x> <y>
  click <left|right|middle> <x> <y> [hold_ms=80]
  drag <left|right|middle> <x0> <y0> <x1> <y1> [duration_ms=400] [steps=20]
  wheel <x> <y> <ticks>
  release
Coordinates are game client pixels. Wheel ticks: -100..-1 or 1..100.
The bridge must be loaded in the verified ra3_1.12.game process.
";
fn bad(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, message) }
fn number(value: &str, min: i32, max: i32) -> io::Result<i32> {
    let n = value.parse::<i32>().map_err(|_| bad("expected an integer"))?;
    if !(min..=max).contains(&n) { return Err(bad(&format!("integer must be {min}..{max}"))); }
    Ok(n)
}
fn xy(args: &[String], index: usize) -> io::Result<(i32, i32)> {
    Ok((number(&args[index], 0, 65535)?, number(&args[index + 1], 0, 65535)?))
}
fn button(value: &str) -> io::Result<u8> {
    match value { "left" => Ok(0), "right" => Ok(1), "middle" => Ok(2), _ => Err(bad("button must be left, right, or middle")) }
}
fn millis(args: &[String], index: usize, default: i32) -> io::Result<Duration> {
    let ms = match args.get(index) { Some(s) => number(s, 1, 60000)?, None => default };
    Ok(Duration::from_millis(ms as u64))
}
fn run() -> io::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || matches!(args[0].as_str(), "--help" | "-h" | "help") { print!("{HELP}"); return Ok(()); }
    if args.len() < 3 || args[0] != "--pid" { return Err(bad(HELP)); }
    let pid = args[1].parse::<u32>().map_err(|_| bad("invalid PID"))?;
    // Validate all arguments before opening a pipe or pressing a button.
    enum Action { Send(Command), Click(u8, (i32, i32), Duration), Drag(u8, (i32, i32), (i32, i32), Duration, u32) }
    let rest = &args[3..];
    let action = match (args[2].as_str(), rest.len()) {
        ("ping", 0) => Action::Send(Command::at(Op::Ping, 0, 0, 0)),
        ("release", 0) => Action::Send(Command::at(Op::ReleaseAll, 0, 0, 0)),
        ("move", 2) => { let p = xy(rest, 0)?; Action::Send(Command::at(Op::Move, 0, p.0, p.1)) }
        ("click", 3..=4) => Action::Click(button(&rest[0])?, xy(rest, 1)?, millis(rest, 3, 80)?),
        ("drag", 5..=7) => Action::Drag(button(&rest[0])?, xy(rest, 1)?, xy(rest, 3)?, millis(rest, 5, 400)?,
            match rest.get(6) { Some(s) => number(s, 1, 1000)? as u32, None => 20 }),
        ("wheel", 3) => {
            let p = xy(rest, 0)?;
            let ticks = number(&rest[2], -100, 100)?;
            if ticks == 0 { return Err(bad("wheel ticks cannot be zero")); }
            Action::Send(Command { op: Op::Wheel, button: 0, x: p.0, y: p.1, value: ticks })
        }
        _ => return Err(bad(HELP)),
    };
    let mut session = Session::connect(pid)?;
    let ack = match action {
        Action::Send(c) => session.send(c)?,
        Action::Click(b, p, duration) => session.click(b, p.0, p.1, duration)?,
        Action::Drag(b, start, end, duration, steps) => session.drag(b, start, end, duration, steps)?,
    };
    println!("pumped sequence={} pump={} held={}", ack.sequence, ack.pump, ack.held);
    Ok(())
}
fn main() {
    if let Err(error) = run() { eprintln!("ra3ctl: {error}"); std::process::exit(1); }
}
