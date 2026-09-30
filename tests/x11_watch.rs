//! End-to-end tests of the clipboard watcher against a headless X server.
//!
//! Each test starts its own Xvfb and plays the application that copies. The
//! tests are skipped when Xvfb is not installed, unless the environment
//! variable CLIPBOARD_HISTORY_REQUIRE_XVFB is set (as in CI).
//! CLIPBOARD_HISTORY_X_SERVER=Xephyr runs them in a nested window instead.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use clipboard_history::history::MAX_ENTRY_BYTES;
use clipboard_history::x11_watch;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, CreateWindowAux, EventMask,
    PropMode, Property, SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent,
    Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

const CHUNK_BYTES: usize = 64 * 1024;
const WAIT: Duration = Duration::from_secs(6);

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        CLIPBOARD,
        TARGETS,
        UTF8_STRING,
        INCR,
        PASSWORD_HINT: b"x-kde-passwordManagerHint",
        IMAGE_PNG: b"image/png",
    }
}

// ---------------------------------------------------------------- tests

#[test]
fn copied_text_is_reported() {
    let Some(mut session) = Session::start() else { return };
    session.copy(Clip::text("bonjour"));
    assert_eq!(session.next_copy().as_deref(), Some("bonjour"));
}

#[test]
fn copies_flagged_as_secret_are_skipped() {
    let Some(mut session) = Session::start() else { return };
    session.copy(Clip::text("MotDePasse123!").secret());
    session.copy_after_a_while(Clip::text("après"));
    assert_eq!(session.next_copy().as_deref(), Some("après"));
}

#[test]
fn non_text_copies_are_skipped() {
    let Some(mut session) = Session::start() else { return };
    session.copy(Clip::image());
    session.copy_after_a_while(Clip::text("après"));
    assert_eq!(session.next_copy().as_deref(), Some("après"));
}

#[test]
fn large_copies_sent_in_chunks_are_reassembled() {
    let Some(mut session) = Session::start() else { return };
    let large = "y".repeat(300_000);
    session.copy(Clip::text(&large).chunked());
    assert_eq!(session.next_copy(), Some(large));
}

#[test]
fn oversized_copies_are_skipped() {
    let Some(mut session) = Session::start() else { return };
    session.copy(Clip::text(&"x".repeat(MAX_ENTRY_BYTES + 1)));
    session.copy_after_a_while(Clip::text("après"));
    assert_eq!(session.next_copy().as_deref(), Some("après"));
}

#[test]
fn oversized_chunked_copies_are_received_to_the_end_then_skipped() {
    let Some(mut session) = Session::start() else { return };
    session.copy(Clip::text(&"x".repeat(MAX_ENTRY_BYTES + 1)).chunked());
    session.copy_after_a_while(Clip::text("après"));
    assert_eq!(session.next_copy().as_deref(), Some("après"));
    assert!(session.owner.transfers.is_empty(), "the owner must not be left waiting");
}

// ---------------------------------------------------------------- harness

/// A headless X server, the watcher under test and a fake copying app.
struct Session {
    owner: Owner,
    copies: async_channel::Receiver<String>,
    _xvfb: Xvfb,
}

impl Session {
    fn start() -> Option<Self> {
        let xvfb = Xvfb::start()?;
        let owner = Owner::connect(&xvfb.display);
        let (sender, copies) = async_channel::unbounded();
        x11_watch::spawn(Some(xvfb.display.clone()), sender);
        let mut session = Self { owner, copies, _xvfb: xvfb };
        // Let the watcher finish its startup read of the (empty) clipboard.
        session.serve_for(Duration::from_millis(300));
        Some(session)
    }

    fn copy(&mut self, clip: Clip) {
        self.owner.copy(clip);
    }

    /// Copies once the watcher had time to handle the previous copy.
    fn copy_after_a_while(&mut self, clip: Clip) {
        self.serve_for(Duration::from_millis(500));
        self.copy(clip);
    }

    /// Answers the watcher until it reports a copy, or None after [`WAIT`].
    fn next_copy(&mut self) -> Option<String> {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            self.owner.serve_pending_requests();
            if let Ok(text) = self.copies.try_recv() {
                return Some(text);
            }
            thread::sleep(Duration::from_millis(2));
        }
        None
    }

    fn serve_for(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.owner.serve_pending_requests();
            thread::sleep(Duration::from_millis(2));
        }
    }
}

struct Xvfb {
    process: Child,
    display: String,
}

impl Xvfb {
    fn start() -> Option<Self> {
        let server = std::env::var("CLIPBOARD_HISTORY_X_SERVER").unwrap_or("Xvfb".into());
        let spawned = Command::new(server)
            .args(["-displayfd", "1", "-nolisten", "tcp", "-screen", "64x64"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut process = match spawned {
            Ok(process) => process,
            Err(err) if std::env::var_os("CLIPBOARD_HISTORY_REQUIRE_XVFB").is_some() => {
                panic!("Xvfb est requis : {err}")
            }
            Err(_) => {
                eprintln!("Xvfb absent (sudo apt install xvfb) : test ignoré");
                return None;
            }
        };
        // With -displayfd, Xvfb writes its display number once it is ready.
        let mut number = String::new();
        BufReader::new(process.stdout.take().expect("piped stdout"))
            .read_line(&mut number)
            .expect("display number from Xvfb");
        Some(Self { process, display: format!(":{}", number.trim()) })
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

// ---------------------------------------------------------------- fake app

#[derive(Clone, Copy)]
enum Delivery {
    AtOnce,
    Chunked,
}

enum Kind {
    Text,
    Secret,
    Image,
}

struct Clip {
    data: Vec<u8>,
    kind: Kind,
    delivery: Delivery,
}

impl Clip {
    fn text(text: &str) -> Self {
        Self { data: text.as_bytes().to_vec(), kind: Kind::Text, delivery: Delivery::AtOnce }
    }

    fn image() -> Self {
        Self { data: vec![0x89, b'P', b'N', b'G'], kind: Kind::Image, delivery: Delivery::AtOnce }
    }

    fn secret(self) -> Self {
        Self { kind: Kind::Secret, ..self }
    }

    fn chunked(self) -> Self {
        Self { delivery: Delivery::Chunked, ..self }
    }
}

/// An INCR transfer in progress: the next chunk is sent each time the
/// requestor deletes the property.
struct Transfer {
    requestor: Window,
    property: Atom,
    data: Vec<u8>,
    sent: usize,
}

/// Plays the application owning the clipboard, answering like real apps do.
struct Owner {
    conn: RustConnection,
    window: Window,
    atoms: Atoms,
    clip: Option<Clip>,
    transfers: Vec<Transfer>,
}

impl Owner {
    fn connect(display: &str) -> Self {
        let (conn, screen_num) = x11rb::connect(Some(display)).expect("connect to Xvfb");
        let root = conn.setup().roots[screen_num].root;
        let window = conn.generate_id().unwrap();
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
            &CreateWindowAux::new(),
        )
        .unwrap();
        let atoms = Atoms::new(&conn).unwrap().reply().unwrap();
        Self { conn, window, atoms, clip: None, transfers: Vec::new() }
    }

    fn copy(&mut self, clip: Clip) {
        self.clip = Some(clip);
        self.conn
            .set_selection_owner(self.window, self.atoms.CLIPBOARD, CURRENT_TIME)
            .unwrap();
        self.conn.flush().unwrap();
    }

    fn serve_pending_requests(&mut self) {
        while let Some(event) = self.conn.poll_for_event().unwrap() {
            match event {
                Event::SelectionRequest(request) => self.answer(&request),
                Event::PropertyNotify(ev) if ev.state == Property::DELETE => {
                    self.send_next_chunk(ev.window, ev.atom)
                }
                _ => {}
            }
        }
        self.conn.flush().unwrap();
    }

    fn targets(&self, clip: &Clip) -> Vec<Atom> {
        let a = &self.atoms;
        match clip.kind {
            Kind::Text => vec![a.TARGETS, a.UTF8_STRING],
            Kind::Secret => vec![a.TARGETS, a.UTF8_STRING, a.PASSWORD_HINT],
            Kind::Image => vec![a.TARGETS, a.IMAGE_PNG],
        }
    }

    fn answer(&mut self, request: &SelectionRequestEvent) {
        let property = if request.property == NONE { request.target } else { request.property };
        let answered = self.write_answer(request, property);
        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: if answered { property } else { NONE },
        };
        self.conn.send_event(false, request.requestor, EventMask::NO_EVENT, notify).unwrap();
    }

    /// Writes the requested data; false if the target is not offered.
    fn write_answer(&mut self, request: &SelectionRequestEvent, property: Atom) -> bool {
        let Some(clip) = &self.clip else { return false };
        let targets = self.targets(clip);
        if request.target == self.atoms.TARGETS {
            self.conn
                .change_property32(PropMode::REPLACE, request.requestor, property, AtomEnum::ATOM, &targets)
                .unwrap();
            return true;
        }
        if !targets.contains(&request.target) {
            return false;
        }
        let (data, delivery) = (clip.data.clone(), clip.delivery);
        match delivery {
            Delivery::AtOnce => {
                self.conn
                    .change_property8(PropMode::REPLACE, request.requestor, property, request.target, &data)
                    .unwrap();
            }
            Delivery::Chunked => self.start_transfer(request.requestor, property, data),
        }
        true
    }

    fn start_transfer(&mut self, requestor: Window, property: Atom, data: Vec<u8>) {
        // Watch the requestor's properties to know when it wants the next chunk.
        self.conn
            .change_window_attributes(
                requestor,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .unwrap();
        self.conn
            .change_property32(PropMode::REPLACE, requestor, property, self.atoms.INCR, &[data.len() as u32])
            .unwrap();
        self.transfers.push(Transfer { requestor, property, data, sent: 0 });
    }

    fn send_next_chunk(&mut self, requestor: Window, property: Atom) {
        let Some(index) = self
            .transfers
            .iter()
            .position(|t| t.requestor == requestor && t.property == property)
        else {
            return;
        };
        let transfer = &mut self.transfers[index];
        let end = (transfer.sent + CHUNK_BYTES).min(transfer.data.len());
        let chunk = transfer.data[transfer.sent..end].to_vec();
        transfer.sent = end;
        self.conn
            .change_property8(PropMode::REPLACE, requestor, property, self.atoms.UTF8_STRING, &chunk)
            .unwrap();
        // The final, empty chunk ends the transfer.
        if chunk.is_empty() {
            self.transfers.remove(index);
        }
    }
}
