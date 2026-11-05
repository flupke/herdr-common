//! The display's clipboard, which this process owns while it offers a text: a window of its own
//! holds the `CLIPBOARD` selection, and a thread hands the text to each window that asks for
//! it, as a person's clipboard manager would.

use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::JoinHandle;

use eyre::{Result, WrapErr, ensure};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode,
    SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

/// The atoms the clipboard speaks.
#[derive(Clone, Copy)]
struct Atoms {
    clipboard: Atom,
    targets: Atom,
    utf8: Atom,
    /// The text targets it answers, besides `utf8`.
    texts: [Atom; 3],
}

/// What the clipboard offers, and who waits for a window to read it.
#[derive(Default)]
struct Offer {
    text: Vec<u8>,
    read: Option<mpsc::Sender<()>>,
}

pub(crate) struct Clipboard {
    connection: Arc<RustConnection>,
    window: Window,
    atoms: Atoms,
    offer: Arc<Mutex<Offer>>,
    thread: Option<JoinHandle<()>>,
}

impl Clipboard {
    /// Create the clipboard's window on `connection`, a connection of its own, and the thread
    /// that answers the windows that ask for the text.
    pub(crate) fn start(connection: RustConnection) -> Result<Self> {
        let connection = Arc::new(connection);
        let root = connection.setup().roots[0].root;
        let window = connection.generate_id()?;
        connection.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            x11rb::COPY_FROM_PARENT,
            // Its destruction ends the thread.
            &CreateWindowAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
        )?;
        let atom = |name: &[u8]| -> Result<Atom> {
            Ok(connection.intern_atom(false, name)?.reply()?.atom)
        };
        let atoms = Atoms {
            clipboard: atom(b"CLIPBOARD")?,
            targets: atom(b"TARGETS")?,
            utf8: atom(b"UTF8_STRING")?,
            texts: [
                AtomEnum::STRING.into(),
                atom(b"TEXT")?,
                atom(b"text/plain;charset=utf-8")?,
            ],
        };
        connection.flush()?;
        let offer = Arc::new(Mutex::new(Offer::default()));
        let thread = {
            let connection = Arc::clone(&connection);
            let offer = Arc::clone(&offer);
            std::thread::spawn(move || serve(&connection, window, atoms, &offer))
        };
        Ok(Self {
            connection,
            window,
            atoms,
            offer,
            thread: Some(thread),
        })
    }

    /// Offer `text`, owning the clipboard, and return a channel that hears once a window read
    /// it.
    pub(crate) fn offer(&self, text: &str) -> Result<mpsc::Receiver<()>> {
        // One property of the requesting window holds the whole text: no incremental transfer.
        let limit = self.connection.maximum_request_bytes().saturating_sub(1024);
        ensure!(
            text.len() <= limit,
            "a paste holds at most {limit} bytes; this one has {}",
            text.len()
        );
        let (read, heard) = mpsc::channel();
        *self.offer.lock().unwrap_or_else(PoisonError::into_inner) = Offer {
            text: text.as_bytes().to_vec(),
            read: Some(read),
        };
        self.connection
            .set_selection_owner(self.window, self.atoms.clipboard, CURRENT_TIME)?;
        let owner = self
            .connection
            .get_selection_owner(self.atoms.clipboard)?
            .reply()
            .wrap_err("cannot read the clipboard's owner")?
            .owner;
        ensure!(owner == self.window, "the display refused the clipboard");
        Ok(heard)
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        let _ = self.connection.destroy_window(self.window);
        let _ = self.connection.flush();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Answer each request for the clipboard of `window`, until it is destroyed or the display
/// goes.
fn serve(connection: &RustConnection, window: Window, atoms: Atoms, offer: &Mutex<Offer>) {
    loop {
        match connection.wait_for_event() {
            Ok(Event::SelectionRequest(request)) => {
                if answer(connection, atoms, offer, &request).is_err() {
                    return;
                }
            }
            Ok(Event::DestroyNotify(destroyed)) if destroyed.window == window => return,
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

/// Give the requestor of `request` the text, or the targets it can have, and tell it.
fn answer(
    connection: &RustConnection,
    atoms: Atoms,
    offer: &Mutex<Offer>,
    request: &SelectionRequestEvent,
) -> Result<()> {
    // A client from before ICCCM 2 names no property: the target is then the property.
    let property = if request.property == NONE {
        request.target
    } else {
        request.property
    };
    let given = if request.target == atoms.targets {
        let mut targets = vec![atoms.targets, atoms.utf8];
        targets.extend(atoms.texts);
        connection.change_property32(
            PropMode::REPLACE,
            request.requestor,
            property,
            AtomEnum::ATOM,
            &targets,
        )?;
        property
    } else if request.target == atoms.utf8 || atoms.texts.contains(&request.target) {
        let mut offer = offer.lock().unwrap_or_else(PoisonError::into_inner);
        connection.change_property8(
            PropMode::REPLACE,
            request.requestor,
            property,
            atoms.utf8,
            &offer.text,
        )?;
        if let Some(read) = offer.read.take() {
            let _ = read.send(());
        }
        property
    } else {
        NONE
    };
    let notify = SelectionNotifyEvent {
        response_type: SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: request.time,
        requestor: request.requestor,
        selection: request.selection,
        target: request.target,
        property: given,
    };
    connection.send_event(false, request.requestor, EventMask::NO_EVENT, notify)?;
    connection.flush()?;
    Ok(())
}
