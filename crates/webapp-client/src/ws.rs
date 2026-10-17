//! Authenticated WebSocket connection (`ws` feature) to one of the app's own
//! routes, with reconnect and keepalive.
//!
//! Each connection attempt fetches a single-use ticket from
//! `POST /api/auth/ws-ticket` (so it works with the cookie and the Discord
//! bearer token alike) and passes it as `?ticket=`, which the server's
//! `WsAuth` consumes. The URL is same-origin, so it also works through the
//! Discord activity proxy.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::Duration,
};

use leptos::{prelude::*, task::spawn_local};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};
use webapp_common::WsTicketResponse;

use crate::api::{FetchError, api_post};

/// How often an empty text message is sent, to stay under Fly.io's ~60 s
/// idle timeout. Server handlers must ignore empty messages.
const PING_INTERVAL: Duration = Duration::from_secs(30);
const MIN_BACKOFF: Duration = Duration::from_secs(1);
/// Also the wait after a 429, so reconnects stay within the server's ticket
/// rate limit.
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A socket that stayed open this long resets the backoff when it closes; one
/// that closes sooner keeps backing off, so a server that keeps dropping
/// connections isn't hammered.
const STABLE_MS: f64 = 10_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsStatus {
    /// Requesting a ticket, connecting, or waiting to reconnect.
    Connecting,
    Open,
    /// Stopped for good: the session is gone (the ticket request got a 401)
    /// or the owning component was cleaned up.
    Closed,
}

/// Handle to a connection from [`connect_ws`]. `Copy`, so it can be used
/// freely in views and event handlers.
#[derive(Clone, Copy)]
pub struct WsConnection {
    pub status: Signal<WsStatus>,
    shared: StoredValue<Rc<Shared>, LocalStorage>,
}

impl WsConnection {
    /// Sends a text message. Returns `false`, dropping the message, unless the
    /// socket is open (nothing is queued across reconnects).
    pub fn send(&self, text: &str) -> bool {
        self.shared
            .try_with_value(|shared| shared.send(text))
            .unwrap_or(false)
    }
}

/// Opens an authenticated WebSocket to the app's route `path` (e.g.
/// `"/api/ws"`) and keeps it open: it reconnects with backoff, getting a new
/// ticket each time, and sends an empty text message every 30 s as a
/// keepalive. Call it after sign-in, i.e. inside `<AuthGate>`.
///
/// `on_message` receives each incoming text message. The connection closes
/// for good when the calling component's reactive owner is cleaned up (e.g.
/// on sign-out).
pub fn connect_ws(path: &str, on_message: impl Fn(String) + 'static) -> WsConnection {
    let status = RwSignal::new(WsStatus::Connecting);
    let shared = Rc::new(Shared {
        path: path.to_string(),
        on_message: Box::new(on_message),
        status,
        socket: RefCell::new(None),
        opened_at: Cell::new(None),
        backoff: Cell::new(MIN_BACKOFF),
        stopped: Cell::new(false),
        ping: RefCell::new(None),
    });

    let weak = Rc::downgrade(&shared);
    let ping = set_interval_with_handle(
        move || {
            if let Some(shared) = weak.upgrade() {
                shared.send("");
            }
        },
        PING_INTERVAL,
    );
    *shared.ping.borrow_mut() = ping.ok();
    connect(Rc::downgrade(&shared));

    // Callbacks only hold weak references, so this is the one strong one.
    let shared = StoredValue::new_local(shared);
    on_cleanup(move || {
        shared.try_with_value(|shared| shared.stop());
    });
    WsConnection {
        status: status.into(),
        shared,
    }
}

struct Shared {
    path: String,
    on_message: Box<dyn Fn(String)>,
    status: RwSignal<WsStatus>,
    socket: RefCell<Option<Socket>>,
    /// When the current socket opened (`Date.now()`), if it did.
    opened_at: Cell<Option<f64>>,
    /// Wait before the next reconnect attempt.
    backoff: Cell<Duration>,
    stopped: Cell<bool>,
    ping: RefCell<Option<IntervalHandle>>,
}

impl Shared {
    fn send(&self, text: &str) -> bool {
        self.socket.borrow().as_ref().is_some_and(|socket| {
            socket.ws.ready_state() == WebSocket::OPEN && socket.ws.send_with_str(text).is_ok()
        })
    }

    fn stop(&self) {
        self.stopped.set(true);
        self.socket.take();
        if let Some(ping) = self.ping.take() {
            ping.clear();
        }
        self.status.try_set(WsStatus::Closed);
    }

    /// Schedules the next connection attempt after the current backoff (plus
    /// up to 25% jitter, so clients dropped together don't all return at
    /// once), and doubles the backoff.
    fn retry_later(self: &Rc<Self>) {
        self.status.try_set(WsStatus::Connecting);
        let delay = self.backoff.get();
        self.backoff.set((delay * 2).min(MAX_BACKOFF));
        let weak = Rc::downgrade(self);
        set_timeout(
            move || connect(weak),
            delay.mul_f64(1.0 + js_sys::Math::random() / 4.0),
        );
    }
}

/// A WebSocket and the JS callbacks attached to it, which live exactly as long
/// as it does.
struct Socket {
    ws: WebSocket,
    _onopen: Closure<dyn FnMut(Event)>,
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
    _onclose: Closure<dyn FnMut(CloseEvent)>,
}

impl Drop for Socket {
    /// Detaches the callbacks before closing, so a deliberate close (or
    /// replacing a dead socket) never triggers a reconnect.
    fn drop(&mut self) {
        self.ws.set_onopen(None);
        self.ws.set_onmessage(None);
        self.ws.set_onclose(None);
        let _ = self.ws.close();
    }
}

/// One connection attempt: fetch a ticket, then open the socket.
fn connect(weak: Weak<Shared>) {
    spawn_local(async move {
        let ticket = api_post::<_, WsTicketResponse>("/api/auth/ws-ticket", &serde_json::json!({}))
            .await;
        let Some(shared) = weak.upgrade() else {
            return;
        };
        if shared.stopped.get() {
            return;
        }
        match ticket {
            Ok(WsTicketResponse { ticket }) => match open(&shared, &ticket) {
                Ok(socket) => *shared.socket.borrow_mut() = Some(socket),
                Err(_) => shared.retry_later(),
            },
            Err(FetchError::Status(401)) => shared.stop(),
            Err(FetchError::Status(429)) => {
                shared.backoff.set(MAX_BACKOFF);
                shared.retry_later();
            }
            Err(_) => shared.retry_later(),
        }
    });
}

fn open(shared: &Rc<Shared>, ticket: &str) -> Result<Socket, JsValue> {
    let location = window().location();
    let scheme = if location.protocol()? == "https:" { "wss:" } else { "ws:" };
    let host = location.host()?;
    let separator = if shared.path.contains('?') { '&' } else { '?' };
    let url = format!("{scheme}//{host}{}{separator}ticket={ticket}", shared.path);
    let ws = WebSocket::new(&url)?;
    shared.opened_at.set(None);

    let weak = Rc::downgrade(shared);
    let onopen = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
        if let Some(shared) = weak.upgrade() {
            shared.opened_at.set(Some(js_sys::Date::now()));
            shared.status.try_set(WsStatus::Open);
        }
    });
    let weak = Rc::downgrade(shared);
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        if let Some(shared) = weak.upgrade()
            && let Some(text) = event.data().as_string()
        {
            (shared.on_message)(text);
        }
    });
    let weak = Rc::downgrade(shared);
    let onclose = Closure::<dyn FnMut(CloseEvent)>::new(move |_: CloseEvent| {
        let Some(shared) = weak.upgrade() else {
            return;
        };
        if shared.stopped.get() {
            return;
        }
        if shared
            .opened_at
            .take()
            .is_some_and(|opened| js_sys::Date::now() - opened >= STABLE_MS)
        {
            shared.backoff.set(MIN_BACKOFF);
        }
        shared.retry_later();
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));

    Ok(Socket {
        ws,
        _onopen: onopen,
        _onmessage: onmessage,
        _onclose: onclose,
    })
}
