//! In-memory fake-core wires for the `app_loop` integration tests: a
//! plain-mode `events.subscribe` conn and framed `terminal.attach`
//! conns over duplex pairs. No sockets, no sessions.
#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use hc_proto::client::Attach;
use hc_proto::subscribe::Subscription;
use hc_proto::terminal::AttachParams;
use hc_proto::watch::{OpenSub, SubAttempt};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[path = "../../../hc-proto/tests/common/fakecore.rs"]
pub mod fakecore;

/// Methods the app-side conns advertise.
pub const METHODS: &[&str] = &[
    "hello",
    "terminal.list",
    "terminal.attach",
    "machine.list",
    "doc.get",
    "events.subscribe",
];

/// The feature set a negotiated attach path expects.
pub const FEATURES: &[&str] = &[
    hc_proto::hello::feature::AUTOMATIC_INPUT,
    hc_proto::hello::feature::RESIZE_NOTIFICATIONS,
];

/// A live terminal record for `events.subscribe` snapshots and
/// `terminal.attach` results, labelled into `work`/`tab`.
pub fn terminal_json(address: &str, tab: &str, created_ms: u64) -> Value {
    json!({
        "address": address,
        "labels": {"herdr.space": "work", "herdr.tab": tab},
        "argv": ["sh"],
        "cwd": "/home/user",
        "state": "live",
        "created_ms": created_ms,
        "screen": "exact",
        "cols": 80,
        "rows": 24,
    })
}

/// `OpenSub` over fresh duplex pairs: every attempt pushes its fake
/// end to `accepted`, then negotiates `Subscription::open` like a real
/// conn — the driver serves the handshake after receiving it.
pub fn sub_opens(accepted: mpsc::Sender<fakecore::DuplexCore>) -> OpenSub {
    Arc::new(move |params| {
        let accepted = accepted.clone();
        Box::pin(async move {
            let (core, read, write) = fakecore::pair(64 * 1024);
            if accepted.send(core).await.is_err() {
                return SubAttempt::Failed(hc_proto::subscribe::SubscribeError::Open(
                    hc_proto::client::Error::Timeout("fake accept"),
                ));
            }
            match Subscription::open(read, write, None, &params).await {
                Ok(subscription) => SubAttempt::Subscribed(Box::new(subscription)),
                Err(error) => SubAttempt::Failed(error),
            }
        })
    })
}

/// `OpenAttach` over fresh duplex pairs: each attempt hands its fake
/// end plus the requested params to `accepted`, then negotiates
/// `Attach::open` — the driver serves each conn's handshake.
pub fn attach_opens(
    accepted: mpsc::Sender<(fakecore::DuplexCore, AttachParams)>,
) -> hc_tui::app::OpenAttach {
    Arc::new(move |params| {
        let accepted = accepted.clone();
        Box::pin(async move {
            let (core, read, write) = fakecore::pair(64 * 1024);
            if accepted.send((core, params.clone())).await.is_err() {
                return Err(hc_proto::client::AttachError::Closed);
            }
            Attach::open(read, write, None, &params).await
        })
    })
}

/// The fake end of one served attach conn: `send_stream` writes use
/// the channel negotiated at handshake time.
pub struct ServedWire {
    core: fakecore::DuplexCore,
    terminal: String,
    observe: bool,
    channel: u32,
}

/// A scripted core session: the accepted subscription conn plus every
/// attach conn the app opened after the snapshot publish.
pub struct Session {
    sub: fakecore::DuplexCore,
    wires: Vec<ServedWire>,
}

impl Session {
    /// Serve the newest subscription conn's hello + `events.subscribe`,
    /// publish `snapshot`, then accept and serve `attach_count` attach
    /// conns. Panics when fewer attach conns arrive.
    pub async fn connect(
        subs: &mut mpsc::Receiver<fakecore::DuplexCore>,
        attaches: &mut mpsc::Receiver<(fakecore::DuplexCore, AttachParams)>,
        attach_count: u32,
        snapshot: Value,
    ) -> Self {
        let mut sub = subs.recv().await.expect("no subscription conn");
        sub.serve_plain_hello(METHODS).await;
        let request = sub.read_message().await;
        assert_eq!(request["method"], "events.subscribe");
        sub.reply_line(&request["id"], snapshot).await;
        let mut wires = Vec::new();
        for channel in 1..=attach_count {
            let Some((mut core, params)) = attaches.recv().await else {
                panic!("expected attach #{channel}");
            };
            let terminal = terminal_json(&params.terminal, "one", 1);
            core.serve_attach(
                METHODS,
                FEATURES,
                fakecore::attach_result(channel, terminal, true, true),
            )
            .await;
            wires.push(ServedWire {
                core,
                terminal: params.terminal.clone(),
                observe: params.observe,
                channel,
            });
        }
        Session { sub, wires }
    }

    /// Index of the served conn for `terminal` with `observe` matching.
    pub fn wire(&self, terminal: &str, observe: bool) -> usize {
        self.wires
            .iter()
            .position(|wire| wire.terminal == terminal && wire.observe == observe)
            .expect("no such attach conn arrived")
    }

    /// One DATA chunk on a served conn.
    pub async fn send_data(&mut self, wire: usize, bytes: &[u8]) {
        let wire = &mut self.wires[wire];
        wire.core.send_stream(wire.channel, 0, bytes).await;
    }

    /// A `labels` inventory event moving `terminal` onto another tab.
    pub async fn move_to_tab(&mut self, terminal: &str, tab: &str, seq: u64) {
        self.sub
            .notify(
                "event",
                json!({
                    "seq": seq,
                    "type": "labels",
                    "terminal": terminal,
                    "data": {"labels": {"herdr.space": "work", "herdr.tab": tab}},
                }),
            )
            .await;
    }
}

/// Wait until `draws` strictly exceeds `more_than`, then return it.
pub async fn draws_beyond(probe: &hc_tui::app::Probe, more_than: u64) -> u64 {
    tokio::time::timeout(Duration::from_secs(5), async {
        while probe.draws.load(Ordering::Relaxed) <= more_than {
            tokio::task::yield_now().await;
        }
        probe.draws.load(Ordering::Relaxed)
    })
    .await
    .expect("an expected draw never landed")
}

/// Poll `draws` until it stays flat for a quiet window, then return it.
pub async fn settled_draws(probe: &hc_tui::app::Probe) -> u64 {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut last = probe.draws.load(Ordering::Relaxed);
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let now = probe.draws.load(Ordering::Relaxed);
            if now == last {
                return now;
            }
            last = now;
        }
    })
    .await
    .expect("draws never settled")
}
