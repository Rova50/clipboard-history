//! Watches the X11 CLIPBOARD selection and reads new text copies.
//!
//! Runs on its own thread with a pure-Rust X11 connection. On GNOME/KDE
//! Wayland sessions, XWayland mirrors the Wayland clipboard to X11, so copies
//! made in native Wayland apps are seen too.

use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, GetPropertyReply, Property,
    Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

use crate::history::MAX_ENTRY_BYTES;

/// How long an owner may take to answer each step of a transfer.
const TIMEOUT: Duration = Duration::from_secs(2);

/// Transfers use rotating properties: an owner still writing an abandoned
/// transfer can never mix its data into the next one.
const PROPERTIES: u32 = 16;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        CLIPBOARD,
        TARGETS,
        UTF8_STRING,
        INCR,
        // Password managers (KeePassXC, Bitwarden, 1Password…) mark secrets
        // with this target so that clipboard managers can skip them.
        PASSWORD_HINT: b"x-kde-passwordManagerHint",
    }
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Starts the watcher thread on `display` (None: `$DISPLAY`); every new text
/// copy is sent on `copies`.
pub fn spawn(display: Option<String>, copies: async_channel::Sender<String>) {
    thread::spawn(move || {
        let result = Watcher::connect(display.as_deref()).and_then(|mut w| w.run(&copies));
        if let Err(err) = result {
            eprintln!("clipboard-history: surveillance du presse-papiers impossible : {err}");
        }
    });
}

struct Watcher {
    conn: RustConnection,
    window: Window,
    atoms: Atoms,
    /// Set whenever the clipboard gets a new owner, until it has been read.
    owner_changed: bool,
    transfers: u32,
}

impl Watcher {
    fn connect(display: Option<&str>) -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(display)?;
        let window = create_requestor_window(&conn, conn.setup().roots[screen_num].root)?;
        let atoms = Atoms::new(&conn)?.reply()?;
        conn.xfixes_query_version(5, 0)?.reply()?;
        conn.xfixes_select_selection_input(
            window,
            atoms.CLIPBOARD,
            SelectionEventMask::SET_SELECTION_OWNER,
        )?;
        conn.flush()?;
        // Read whatever the clipboard holds at startup.
        Ok(Self { conn, window, atoms, owner_changed: true, transfers: 0 })
    }

    fn run(&mut self, copies: &async_channel::Sender<String>) -> Result<()> {
        loop {
            while self.owner_changed {
                self.owner_changed = false;
                if let Some(text) = self.read_text()? {
                    copies.send_blocking(text)?;
                }
            }
            let event = self.conn.wait_for_event()?;
            self.record_owner_change(&event);
        }
    }

    fn record_owner_change(&mut self, event: &Event) {
        if let Event::XfixesSelectionNotify(ev) = event {
            self.owner_changed |= ev.owner != NONE;
        }
    }

    /// The clipboard text, or None for secrets, non-text and oversized
    /// content, and for owners that do not answer.
    fn read_text(&mut self) -> Result<Option<String>> {
        if !self.offers_public_text()? {
            return Ok(None);
        }
        let bytes = self.read_target(self.atoms.UTF8_STRING)?;
        Ok(bytes.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
    }

    fn offers_public_text(&mut self) -> Result<bool> {
        let Some((_, reply)) = self.convert(self.atoms.TARGETS)? else {
            return Ok(false);
        };
        let targets: Vec<Atom> = reply.value32().map(Iterator::collect).unwrap_or_default();
        Ok(targets.contains(&self.atoms.UTF8_STRING) && !targets.contains(&self.atoms.PASSWORD_HINT))
    }

    /// The clipboard converted to `target`, whether sent at once or in chunks.
    fn read_target(&mut self, target: Atom) -> Result<Option<Vec<u8>>> {
        let Some((property, reply)) = self.convert(target)? else {
            return Ok(None);
        };
        if reply.type_ != self.atoms.INCR {
            return Ok(Some(reply.value));
        }
        // The INCR value is a lower bound of the total size.
        let announced = reply.value32().and_then(|mut v| v.next()).unwrap_or(0) as usize;
        self.read_incr(property, announced)
    }

    /// Asks the owner to convert the clipboard to `target`; returns the
    /// property used and its value. Oversized values are dropped unread.
    fn convert(&mut self, target: Atom) -> Result<Option<(Atom, GetPropertyReply)>> {
        let property = self.next_property()?;
        self.conn.delete_property(self.window, property)?;
        self.conn.convert_selection(
            self.window,
            self.atoms.CLIPBOARD,
            target,
            property,
            CURRENT_TIME,
        )?;
        self.conn.flush()?;
        if !self.wait_for_conversion(target)? {
            return Ok(None);
        }
        Ok(self.take_property(property)?.map(|reply| (property, reply)))
    }

    fn next_property(&mut self) -> Result<Atom> {
        let name = format!("CLIP_HISTORY_DATA_{}", self.transfers % PROPERTIES);
        self.transfers = self.transfers.wrapping_add(1);
        Ok(self.conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
    }

    /// Waits for the owner's answer; false if it refused or did not answer.
    fn wait_for_conversion(&mut self, target: Atom) -> Result<bool> {
        let deadline = Instant::now() + TIMEOUT;
        while let Some(event) = self.next_transfer_event(deadline)? {
            if let Event::SelectionNotify(ev) = event
                && ev.requestor == self.window
                && ev.target == target
            {
                return Ok(ev.property != NONE);
            }
        }
        Ok(false)
    }

    /// Reads then deletes `property`, unless it is larger than the limit.
    /// For INCR transfers, the deletion asks the owner for the first chunk.
    fn take_property(&mut self, property: Atom) -> Result<Option<GetPropertyReply>> {
        let size = self.property_size(property)?;
        let reply = if size > MAX_ENTRY_BYTES {
            None
        } else {
            Some(self.get_property(property, size)?)
        };
        self.conn.delete_property(self.window, property)?;
        self.conn.flush()?;
        Ok(reply)
    }

    fn property_size(&self, property: Atom) -> Result<usize> {
        let reply = self.conn.get_property(false, self.window, property, AtomEnum::ANY, 0, 0)?;
        Ok(reply.reply()?.bytes_after as usize)
    }

    fn get_property(&self, property: Atom, size: usize) -> Result<GetPropertyReply> {
        let length = size.div_ceil(4) as u32;
        let reply =
            self.conn.get_property(false, self.window, property, AtomEnum::ANY, 0, length)?;
        Ok(reply.reply()?)
    }

    /// Reads a chunked (INCR) transfer. Oversized ones are still received to
    /// the end, but discarded: an abandoned transfer would leave the owner
    /// blocked until its timeout, and its next copies would be missed.
    fn read_incr(&mut self, property: Atom, announced: usize) -> Result<Option<Vec<u8>>> {
        let mut data = Vec::new();
        let mut fits = announced <= MAX_ENTRY_BYTES;
        let complete = self.receive_chunks(property, |chunk| {
            fits = fits && data.len() + chunk.len() <= MAX_ENTRY_BYTES;
            if fits {
                data.extend_from_slice(chunk);
            } else {
                data = Vec::new();
            }
        })?;
        Ok((complete && fits).then_some(data))
    }

    /// Passes each chunk of an INCR transfer to `on_chunk`; false if the
    /// owner stopped answering before the end.
    fn receive_chunks(&mut self, property: Atom, mut on_chunk: impl FnMut(&[u8])) -> Result<bool> {
        loop {
            if !self.wait_for_chunk(property)? {
                return Ok(false);
            }
            let chunk = self
                .conn
                .get_property(true, self.window, property, AtomEnum::ANY, 0, u32::MAX / 4)?
                .reply()?;
            self.conn.flush()?;
            if chunk.value.is_empty() {
                return Ok(true);
            }
            on_chunk(&chunk.value);
        }
    }

    fn wait_for_chunk(&mut self, property: Atom) -> Result<bool> {
        let deadline = Instant::now() + TIMEOUT;
        while let Some(event) = self.next_transfer_event(deadline)? {
            if let Event::PropertyNotify(ev) = event
                && ev.window == self.window
                && ev.atom == property
                && ev.state == Property::NEW_VALUE
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The next event of the ongoing transfer, or None at `deadline`.
    /// Clipboard owner changes arriving meanwhile are recorded for later.
    fn next_transfer_event(&mut self, deadline: Instant) -> Result<Option<Event>> {
        loop {
            match self.conn.poll_for_event()? {
                Some(event @ Event::XfixesSelectionNotify(_)) => self.record_owner_change(&event),
                Some(event) => return Ok(Some(event)),
                None if Instant::now() >= deadline => return Ok(None),
                None => thread::sleep(Duration::from_millis(5)),
            }
        }
    }
}

/// An invisible window receiving the converted data.
fn create_requestor_window(conn: &RustConnection, root: Window) -> Result<Window> {
    let window = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;
    Ok(window)
}
