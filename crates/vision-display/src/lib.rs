//! The window of an app on an X display, driven as its user would: keys, text and clicks sent
//! through XTEST, which the X server delivers as a keyboard's and a mouse's, text pasted from
//! the display's clipboard, and a hash of the window's pixels, which changes with anything it
//! shows. The vision core reaches herdr-app so, on the headless display of its app sandbox.
//!
//! The connection stays open for the display's life: Xwayland resets its XTEST device when its
//! client leaves, which would lose the pointer's place and the keys held.

mod clipboard;

use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use eyre::{Context, ContextCompat, Result, ensure};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, ImageFormat,
    KEY_PRESS_EVENT, KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT, MapState, Window,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::{DefaultStream, RustConnection};
use x11rb::wrapper::ConnectionExt as _;

use clipboard::Clipboard;

/// How often [`Display::connect`] looks for the window: the display sends no event to a client
/// that did not ask for one before the window came.
const WINDOW_POLL: Duration = Duration::from_millis(50);

/// How long Xwayland takes to set its XTEST pointer up after the first fake motion: a click
/// sooner can be lost, so [`Display::connect`] makes that motion.
const POINTER_SETUP: Duration = Duration::from_millis(150);

/// How many moves a drag takes the pointer through, and how long it waits before each one and
/// before the release: an app sees a hand's drag, not a jump.
const DRAG_STEPS: i32 = 12;
const DRAG_PAUSE: Duration = Duration::from_millis(16);

/// How long [`Display::paste`] waits for the window to read the clipboard: a guard that fires
/// only when the key pasted nothing.
const PASTE_GUARD: Duration = Duration::from_secs(10);

const SHIFT: u32 = 0xffe1;
const CONTROL: u32 = 0xffe3;
const ALT: u32 = 0xffe9;
const SUPER: u32 = 0xffeb;

/// A window of an X display, and the connection that sees and drives it.
pub struct Display {
    connection: RustConnection,
    root: Window,
    window: Window,
    keymap: Keymap,
    clipboard: Clipboard,
    /// Whether a held drag left the left button down, which the next drag goes on with and
    /// the next click releases.
    held: Cell<bool>,
}

/// The display's keyboard: the keysyms of each keycode, from the lowest.
struct Keymap {
    lowest: u8,
    per_keycode: u8,
    keysyms: Vec<u32>,
}

/// A key to press, with the modifiers held around it, as keysyms.
#[derive(Debug, Eq, PartialEq)]
struct Chord {
    modifiers: Vec<u32>,
    key: u32,
}

impl Display {
    /// Connect to the X display whose socket is `socket`, and return once it shows a window
    /// titled `title` the size of the display, as the sandbox's kiosk shell makes it, for at
    /// most `timeout`.
    pub fn connect(socket: &Path, title: &str, timeout: Duration) -> Result<Self> {
        let connection = open(socket)?;
        let screen = &connection.setup().roots[0];
        let (root, width, height) = (screen.root, screen.width_in_pixels, screen.height_in_pixels);
        let keymap = Keymap::read(&connection)?;
        let deadline = Instant::now() + timeout;
        let mut seen = None;
        let window = loop {
            // The window maps at the app's own size before the shell makes it the display's.
            if let Some(window) = find_window(&connection, root, title)? {
                // A window gone since is no window yet.
                if let Ok(geometry) = connection.get_geometry(window)?.reply() {
                    if (geometry.width, geometry.height) == (width, height) {
                        break window;
                    }
                    seen = Some((geometry.width, geometry.height));
                }
            }
            ensure!(
                Instant::now() < deadline,
                "the X display shows no {width}x{height} window titled {title:?} within \
                 {timeout:?}{}",
                seen.map_or_else(String::new, |(w, h)| format!(": one shows at {w}x{h}"))
            );
            std::thread::sleep(WINDOW_POLL);
        };
        let display = Self {
            clipboard: Clipboard::start(open(socket)?)?,
            connection,
            root,
            window,
            keymap,
            held: Cell::new(false),
        };
        // Keys go to the window under the pointer, which rests at the display's centre: the
        // window covers it, even while its shell centres it again. The display's focus stays
        // PointerRoot: after SetInputFocus, Weston keeps the focus and keys go nowhere.
        display.move_pointer(i16::try_from(width / 2)?, i16::try_from(height / 2)?)?;
        std::thread::sleep(POINTER_SETUP);
        Ok(display)
    }

    /// The window's width and height, in pixels: the frame of a click's coordinates.
    pub fn size(&self) -> Result<(u16, u16)> {
        let geometry = self.connection.get_geometry(self.window)?.reply()?;
        Ok((geometry.width, geometry.height))
    }

    /// Whether a held drag keeps the left button down.
    pub fn held(&self) -> bool {
        self.held.get()
    }

    /// A hash of the window's pixels: anything that changes on screen changes it.
    pub fn fingerprint(&self) -> Result<u64> {
        let (width, height) = self.size()?;
        let image = self
            .connection
            .get_image(
                ImageFormat::Z_PIXMAP,
                self.window,
                0,
                0,
                width,
                height,
                u32::MAX,
            )?
            .reply()
            .wrap_err("cannot capture the window")?;
        let mut hasher = std::hash::DefaultHasher::new();
        image.data.hash(&mut hasher);
        Ok(hasher.finish())
    }

    /// Press `keys` in order, each a key with its modifiers joined by `+`: `ctrl+shift+t`,
    /// `enter`, `a`.
    pub fn keys<S: AsRef<str>>(&self, keys: &[S]) -> Result<()> {
        let chords = keys
            .iter()
            .map(|key| Chord::parse(key.as_ref()))
            .collect::<Result<Vec<_>>>()?;
        self.press_all(&chords)
    }

    /// Type `text`, each character as the key that types it, with Shift when the keyboard
    /// needs it; a newline as Enter.
    pub fn type_text(&self, text: &str) -> Result<()> {
        let chords = text
            .chars()
            .map(|character| Chord {
                modifiers: Vec::new(),
                key: if character == '\n' {
                    named_key("enter").unwrap_or_default()
                } else {
                    character_keysym(character)
                },
            })
            .collect::<Vec<_>>();
        self.press_all(&chords)
    }

    /// Paste `text`: put it on the display's clipboard, press `key`, the window's paste key,
    /// and return once the window read it. A later paste never reaches a window still reading
    /// this one.
    pub fn paste(&self, text: &str, key: &str) -> Result<()> {
        let chord = Chord::parse(key)?;
        let read = self.clipboard.offer(text)?;
        self.press_all(&[chord])?;
        read.recv_timeout(PASTE_GUARD).map_err(|_| {
            eyre::eyre!("the window did not read the clipboard within {PASTE_GUARD:?} of {key}")
        })
    }

    /// Click the window's pixel at `x`, `y` with the left button, `modifiers` held: `ctrl`,
    /// `shift`, `alt`, `super`. A button a held drag left down is released first, where the
    /// drag ended, which drops there.
    pub fn click<S: AsRef<str>>(&self, x: u16, y: u16, modifiers: &[S]) -> Result<()> {
        let modifier_keys = self.modifiers(modifiers)?;
        let (x, y) = self.on_root(x, y)?;
        if self.held.replace(false) {
            self.fake(BUTTON_RELEASE_EVENT, 1)?;
        }
        self.move_pointer(x, y)?;
        self.hold(&modifier_keys, KEY_PRESS_EVENT)?;
        self.fake(BUTTON_PRESS_EVENT, 1)?;
        self.fake(BUTTON_RELEASE_EVENT, 1)?;
        self.hold(&modifier_keys, KEY_RELEASE_EVENT)?;
        self.connection.sync()?;
        Ok(())
    }

    /// Drag with the left button from the window's pixel `from` to `to`, `modifiers` held, as
    /// a hand does: a press, the pointer's way there in [`DRAG_STEPS`] moves, [`DRAG_PAUSE`]
    /// apart, then the release, unless `hold` keeps the button down, for a look at the drag
    /// under way; the modifiers are released all the same. A drag after a held one goes on
    /// with it from `from`, with no press.
    pub fn drag<S: AsRef<str>>(
        &self,
        from: (u16, u16),
        to: (u16, u16),
        modifiers: &[S],
        hold: bool,
    ) -> Result<()> {
        let modifier_keys = self.modifiers(modifiers)?;
        let (from_x, from_y) = self.on_root(from.0, from.1)?;
        let (to_x, to_y) = self.on_root(to.0, to.1)?;
        self.move_pointer(from_x, from_y)?;
        self.hold(&modifier_keys, KEY_PRESS_EVENT)?;
        if !self.held.get() {
            self.fake(BUTTON_PRESS_EVENT, 1)?;
        }
        self.connection.sync()?;
        for step in 1..=DRAG_STEPS {
            std::thread::sleep(DRAG_PAUSE);
            let along = |start: i16, end: i16| {
                let way = (i32::from(end) - i32::from(start)) * step / DRAG_STEPS;
                i16::try_from(i32::from(start) + way).wrap_err("off the display")
            };
            self.move_pointer(along(from_x, to_x)?, along(from_y, to_y)?)?;
        }
        self.held.set(hold);
        if !hold {
            std::thread::sleep(DRAG_PAUSE);
            self.fake(BUTTON_RELEASE_EVENT, 1)?;
        }
        self.hold(&modifier_keys, KEY_RELEASE_EVENT)?;
        self.connection.sync()?;
        Ok(())
    }

    /// The keycodes of `modifiers`, by name.
    fn modifiers<S: AsRef<str>>(&self, modifiers: &[S]) -> Result<Vec<u8>> {
        modifiers
            .iter()
            .map(|name| Ok(self.keymap.keycode(modifier(name.as_ref())?)?.0))
            .collect()
    }

    /// Press (`KEY_PRESS_EVENT`) or release `keys`, the release in reverse.
    fn hold(&self, keys: &[u8], kind: u8) -> Result<()> {
        if kind == KEY_PRESS_EVENT {
            keys.iter().try_for_each(|key| self.fake(kind, *key))
        } else {
            keys.iter().rev().try_for_each(|key| self.fake(kind, *key))
        }
    }

    /// The display's point of the window's pixel at `x`, `y`.
    fn on_root(&self, x: u16, y: u16) -> Result<(i16, i16)> {
        let (width, height) = self.size()?;
        ensure!(
            x < width && y < height,
            "({x}, {y}) is outside the window, which is {width}x{height} pixels"
        );
        let origin = self
            .connection
            .translate_coordinates(self.window, self.root, 0, 0)?
            .reply()?;
        let to_root = |position: u16, origin: i16| {
            i16::try_from(i32::from(position) + i32::from(origin)).wrap_err("off the display")
        };
        Ok((to_root(x, origin.dst_x)?, to_root(y, origin.dst_y)?))
    }

    /// Press each of `chords` in order, once the keyboard has a key for every one of them: when
    /// one is missing, it presses none.
    fn press_all(&self, chords: &[Chord]) -> Result<()> {
        let keycodes = chords
            .iter()
            .map(|chord| self.keycodes(chord))
            .collect::<Result<Vec<_>>>()?;
        for (key, modifiers) in &keycodes {
            self.press(*key, modifiers)?;
        }
        Ok(())
    }

    /// The keycode of `chord`'s key, and those of the modifiers held around it, Shift included
    /// when the key's keysym takes it.
    fn keycodes(&self, chord: &Chord) -> Result<(u8, Vec<u8>)> {
        let (key, shifted) = self.keymap.keycode(chord.key)?;
        let mut modifiers = chord
            .modifiers
            .iter()
            .map(|modifier| Ok(self.keymap.keycode(*modifier)?.0))
            .collect::<Result<Vec<_>>>()?;
        if shifted && !chord.modifiers.contains(&SHIFT) {
            modifiers.push(self.keymap.keycode(SHIFT)?.0);
        }
        Ok((key, modifiers))
    }

    fn press(&self, key: u8, modifiers: &[u8]) -> Result<()> {
        for modifier in modifiers {
            self.fake(KEY_PRESS_EVENT, *modifier)?;
        }
        self.fake(KEY_PRESS_EVENT, key)?;
        self.fake(KEY_RELEASE_EVENT, key)?;
        for modifier in modifiers.iter().rev() {
            self.fake(KEY_RELEASE_EVENT, *modifier)?;
        }
        self.connection.sync()?;
        Ok(())
    }

    fn move_pointer(&self, x: i16, y: i16) -> Result<()> {
        self.connection
            .xtest_fake_input(MOTION_NOTIFY_EVENT, 0, 0, self.root, x, y, 0)?;
        self.connection.sync()?;
        Ok(())
    }

    fn fake(&self, kind: u8, detail: u8) -> Result<()> {
        self.connection
            .xtest_fake_input(kind, detail, 0, x11rb::NONE, 0, 0, 0)?;
        Ok(())
    }
}

/// A connection to the X display whose socket is `socket`.
fn open(socket: &Path) -> Result<RustConnection> {
    let stream = UnixStream::connect(socket)
        .wrap_err_with(|| format!("cannot reach the X display at {}", socket.display()))?;
    let (stream, _) = DefaultStream::from_unix_stream(stream)?;
    RustConnection::connect_to_stream(stream, 0).wrap_err("the X display refused the connection")
}

impl Keymap {
    fn read(connection: &RustConnection) -> Result<Self> {
        let setup = connection.setup();
        let lowest = setup.min_keycode;
        let count = setup.max_keycode - lowest + 1;
        let mapping = connection
            .get_keyboard_mapping(lowest, count)?
            .reply()
            .wrap_err("cannot read the X display's keyboard")?;
        Ok(Self {
            lowest,
            per_keycode: mapping.keysyms_per_keycode,
            keysyms: mapping.keysyms,
        })
    }

    /// The keycode that types `keysym`, and whether it takes Shift: its keysym is the second of
    /// its keycode's.
    fn keycode(&self, keysym: u32) -> Result<(u8, bool)> {
        let per_keycode = usize::from(self.per_keycode).max(1);
        self.keysyms
            .chunks(per_keycode)
            .enumerate()
            .find_map(|(index, keysyms)| {
                let level = keysyms.iter().take(2).position(|each| *each == keysym)?;
                let keycode = u8::try_from(index).ok()?.checked_add(self.lowest)?;
                Some((keycode, level == 1))
            })
            .with_context(|| {
                format!("the display's keyboard has no key for the keysym {keysym:#x}")
            })
    }
}

impl Chord {
    /// `ctrl+shift+t`: modifiers, then a key's name or one character.
    fn parse(text: &str) -> Result<Self> {
        // `ctrl++` holds Ctrl and presses `+`.
        let (modifiers, key) = match text.strip_suffix("++") {
            Some(modifiers) => (Some(modifiers), "+"),
            None if text == "+" => (None, "+"),
            None => match text.rsplit_once('+') {
                Some((modifiers, key)) => (Some(modifiers), key),
                None => (None, text),
            },
        };
        let modifiers = modifiers
            .map(|modifiers| modifiers.split('+').collect::<Vec<_>>())
            .unwrap_or_default()
            .iter()
            .map(|name| modifier(name).wrap_err_with(|| format!("in {text:?}")))
            .collect::<Result<Vec<_>>>()?;
        let mut characters = key.chars();
        let key = match (characters.next(), characters.next()) {
            (Some(character), None) => character_keysym(character),
            _ => named_key(&key.to_ascii_lowercase())
                .with_context(|| format!("{key:?} in {text:?} is no key"))?,
        };
        Ok(Self { modifiers, key })
    }
}

/// The keysym of a modifier's name: `ctrl`, `shift`, `alt`, `super`.
fn modifier(name: &str) -> Result<u32> {
    match name.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Ok(CONTROL),
        "shift" => Ok(SHIFT),
        "alt" => Ok(ALT),
        "super" | "cmd" | "meta" => Ok(SUPER),
        other => Err(eyre::eyre!(
            "{other:?} is no modifier: ctrl, shift, alt or super"
        )),
    }
}

/// The keys [`named_key`] knows by name, with their keysyms, besides `f1`..`f12`.
const NAMED_KEYS: [(&str, u32); 17] = [
    ("enter", 0xff0d),
    ("return", 0xff0d),
    ("esc", 0xff1b),
    ("escape", 0xff1b),
    ("tab", 0xff09),
    ("backspace", 0xff08),
    ("space", 0x20),
    ("delete", 0xffff),
    ("insert", 0xff63),
    ("home", 0xff50),
    ("end", 0xff57),
    ("pageup", 0xff55),
    ("pagedown", 0xff56),
    ("left", 0xff51),
    ("up", 0xff52),
    ("right", 0xff53),
    ("down", 0xff54),
];

/// The keysym of a key's name: `enter`, `esc`, `f5`.
fn named_key(name: &str) -> Option<u32> {
    if let Some((_, keysym)) = NAMED_KEYS.iter().find(|(named, _)| *named == name) {
        return Some(*keysym);
    }
    let number: u32 = name.strip_prefix('f')?.parse().ok()?;
    // F1 is 0xffbe, and the next eleven follow it.
    (1..=12).contains(&number).then_some(0xffbe + number - 1)
}

/// The keysym of a character: its code for Latin-1, which X's keysyms share, else the Unicode
/// keysym.
fn character_keysym(character: char) -> u32 {
    let code = u32::from(character);
    if (0x20..0x7f).contains(&code) || (0xa0..0x100).contains(&code) {
        code
    } else {
        0x0100_0000 | code
    }
}

/// The viewable window under `parent`, itself included, titled `title`: the largest, since an
/// app may keep small helper windows with its title.
fn find_window(connection: &RustConnection, parent: Window, title: &str) -> Result<Option<Window>> {
    let utf8 = connection.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
    let net_name = connection
        .intern_atom(false, b"_NET_WM_NAME")?
        .reply()?
        .atom;
    let mut found: Option<(u32, Window)> = None;
    let mut pending = vec![parent];
    while let Some(window) = pending.pop() {
        let Ok(tree) = connection.query_tree(window)?.reply() else {
            continue;
        };
        pending.extend(tree.children);
        let names = [
            (net_name, utf8),
            (AtomEnum::WM_NAME.into(), AtomEnum::ANY.into()),
        ];
        let titled = names.iter().any(|(property, kind)| {
            connection
                .get_property(false, window, *property, *kind, 0, 256)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .is_some_and(|reply| reply.value == title.as_bytes())
        });
        if !titled {
            continue;
        }
        let Ok(attributes) = connection.get_window_attributes(window)?.reply() else {
            continue;
        };
        let Ok(geometry) = connection.get_geometry(window)?.reply() else {
            continue;
        };
        let area = u32::from(geometry.width) * u32::from(geometry.height);
        if attributes.map_state == MapState::VIEWABLE && found.is_none_or(|(most, _)| area > most) {
            found = Some((area, window));
        }
    }
    Ok(found.map(|(_, window)| window))
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
