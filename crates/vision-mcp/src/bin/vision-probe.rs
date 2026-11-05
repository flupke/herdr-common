//! The probe the vision core's integration tests drive: a full-screen terminal program that
//! speaks vision-signal and shows what it read. Run it in a raw terminal (`stty raw -echo`).
//!
//! It shows the keys typed, the last text pasted, the last click or drag (`dragging` while the
//! button is down), where the last drag dropped, and work started by Enter, which shows
//! `working on <typed>`, then `done: <typed>` half a second later. It paints its reaction to an
//! input a moment after reading it, as a busy app does: a core that reads the screen before the
//! frame that acknowledges its input sees the screen from before.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use vision_signal::{FrameMarker, parse_acknowledgement_request};

/// How long the work Enter starts takes.
const WORK: Duration = Duration::from_millis(500);

/// How long the app takes to paint its reaction to an input.
const REACTION: Duration = Duration::from_millis(200);

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

#[derive(Default)]
struct State {
    typed: String,
    pasted: String,
    mouse: String,
    dropped: String,
    work: String,
    /// Where the left button went down, and whether it moved since.
    pressed: Option<((u32, u32), bool)>,
    marker: FrameMarker,
}

fn main() {
    let (rows, columns) = size();
    let state = Arc::new(Mutex::new(State {
        marker: FrameMarker {
            columns,
            rows,
            ..FrameMarker::default()
        },
        ..State::default()
    }));
    // The alternate screen, SGR mouse reports with motion while a button is down, bracketed
    // paste, no cursor.
    print_raw("\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?2004h\x1b[?25l");
    paint(&state);
    let mut input = Vec::new();
    let mut buffer = [0; 4096];
    let mut stdin = std::io::stdin().lock();
    loop {
        let read = stdin.read(&mut buffer).unwrap_or(0);
        if read == 0 {
            print_raw(FrameMarker::RESET);
            return;
        }
        input.extend_from_slice(&buffer[..read]);
        let consumed = handle(&state, &input);
        input.drain(..consumed);
        std::thread::sleep(REACTION);
        paint(&state);
    }
}

/// Act on the complete inputs at the start of `input`, and say how many bytes they took.
fn handle(state: &Arc<Mutex<State>>, input: &[u8]) -> usize {
    let mut at = 0;
    while at < input.len() {
        let rest = &input[at..];
        let taken = if rest.starts_with(PASTE_START) {
            let Some(end) = find(rest, PASTE_END) else {
                return at;
            };
            let text = String::from_utf8_lossy(&rest[PASTE_START.len()..end]).into_owned();
            let mut state = lock(state);
            match parse_acknowledgement_request(&text) {
                Some(request) => state.marker.acknowledged = request,
                None => state.pasted = text,
            }
            end + PASTE_END.len()
        } else if rest.starts_with(b"\x1b[<") {
            let Some(end) = rest.iter().position(|byte| *byte == b'M' || *byte == b'm') else {
                return at;
            };
            mouse(&mut lock(state), &rest[3..end], rest[end] == b'm');
            end + 1
        } else if rest[0] == b'\x1b' {
            // Another escape sequence: up to its final byte.
            match rest
                .iter()
                .skip(2)
                .position(|byte| (0x40..0x7f).contains(byte))
            {
                Some(end) => end + 3,
                None if rest.len() < 2 => return at,
                None => rest.len(),
            }
        } else {
            key(state, rest[0]);
            1
        };
        at += taken;
    }
    at
}

fn key(state: &Arc<Mutex<State>>, byte: u8) {
    let mut locked = lock(state);
    match byte {
        b'\r' => {
            let typed = locked.typed.clone();
            locked.work = format!("working on {typed}");
            let state = Arc::clone(state);
            std::thread::spawn(move || {
                std::thread::sleep(WORK);
                lock(&state).work = format!("done: {typed}");
                paint(&state);
            });
        }
        0x7f => {
            locked.typed.pop();
        }
        byte if byte >= 0x20 => locked.typed.push(char::from(byte)),
        _ => {}
    }
}

/// An SGR mouse report: `button;column;row`, one-based, released when `released`.
fn mouse(state: &mut State, report: &[u8], released: bool) {
    let fields: Vec<u32> = String::from_utf8_lossy(report)
        .split(';')
        .filter_map(|field| field.parse().ok())
        .collect();
    let [button, column, row] = fields[..] else {
        return;
    };
    let cell = (column - 1, row - 1);
    if released {
        if let Some((from, moved)) = state.pressed.take() {
            state.mouse = if moved {
                let way = format!("{},{} -> {},{}", from.0, from.1, cell.0, cell.1);
                state.dropped.clone_from(&way);
                format!("drag {way}")
            } else {
                format!("click {},{}", cell.0, cell.1)
            };
        }
    } else if button & 32 != 0 {
        if let Some((from, moved)) = &mut state.pressed {
            *moved = true;
            state.mouse = format!("dragging {},{} -> {},{}", from.0, from.1, cell.0, cell.1);
        }
    } else if button == 0 {
        state.pressed = Some((cell, false));
    }
}

/// Paint the whole screen, then the frame's marker.
fn paint(state: &Mutex<State>) {
    let mut state = lock(state);
    state.marker.frame += 1;
    let screen = format!(
        "\x1b[H\x1b[2Jvision probe\r\ntyped: {}\r\npasted: {}\r\nmouse: {}\r\ndropped: {}\r\n\
         work: {}{}",
        state.typed,
        state.pasted,
        state.mouse,
        state.dropped,
        state.work,
        state.marker.escape_sequence()
    );
    print_raw(&screen);
}

fn print_raw(text: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

/// The terminal's rows and columns, as `stty size` gives them.
fn size() -> (u16, u16) {
    let output = std::process::Command::new("stty")
        .arg("size")
        .stdin(std::process::Stdio::inherit())
        .output();
    let printed = output
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let mut numbers = printed
        .split_whitespace()
        .filter_map(|number| number.parse().ok());
    (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}
