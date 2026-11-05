//! Bounded ordered shutdown: accepted input crosses the DATA write fence
//! before CLOSE. Every reader/writer/drain task remains owned and cancellable.
use super::*;

const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CLOSING: usize = 8;

struct Drain {
    route: u64,
    attach: Arc<Attach>,
    out: mpsc::Sender<Out>,
    pending: VecDeque<Out>,
    reader: JoinHandle<()>,
    writer: JoinHandle<Result<(), String>>,
}

impl Pane {
    pub(super) fn begin_close(&mut self, mut parked: Parked) {
        parked
            .held
            .release_into(&parked.vt, &mut parked.pending_input, &mut self.close_error);
        self.start_drain(Drain {
            route: parked.route,
            attach: parked.attach,
            out: parked.out,
            pending: parked
                .pending_reply
                .into_iter()
                .map(Out::Reply)
                .chain(parked.pending_input)
                .collect(),
            reader: parked.reader,
            writer: parked.writer,
        });
    }

    fn start_drain(&mut self, drain: Drain) {
        self.sweep_closing();
        if self.closing.len() >= MAX_CLOSING {
            let oldest = self.closing.remove(0);
            oldest.writer_abort.abort();
            oldest.done.abort();
            self.close_error = Some("Input drain limit exceeded; an older route was closed".into());
        }
        let route = drain.route;
        let writer_abort = drain.writer.abort_handle();
        let reader_abort = drain.reader.abort_handle();
        let abort_on_failure = writer_abort.clone();
        let ev = self.ev.clone();
        let done = tokio::spawn(async move {
            let result = tokio::time::timeout(CLOSE_TIMEOUT, drain.finish())
                .await
                .unwrap_or_else(|_| Err("Timed out draining terminal input before detach".into()));
            if result.is_err() {
                abort_on_failure.abort();
            }
            let _ = ev
                .send(PaneEv::RouteClosed {
                    route,
                    error: result.clone().err(),
                })
                .await;
            result
        });
        self.closing.push(Closing {
            route,
            done,
            writer_abort,
            reader_abort,
        });
    }

    /// Caller closes the UI event receiver first: writer progress must not
    /// wait for the exited UI while accepted keys and releases drain.
    pub async fn shutdown(&mut self) {
        let state = std::mem::replace(&mut self.state, State::Idle);
        match state {
            State::Opening { task, .. } => {
                task.abort();
                let _ = task.await;
                if self
                    .pending
                    .iter()
                    .any(|item| !matches!(item, Pending::Control(_)))
                {
                    self.close_error = Some(
                        "Detached before queued input could reach a fresh terminal paint".into(),
                    );
                }
            }
            State::Live(mut live) => {
                let mut pending = VecDeque::new();
                if !live.await_paint
                    && let Some(vt) = self.vt.as_ref()
                {
                    pending.extend(live.pending_reply.into_iter().map(Out::Reply));
                    for item in self.pending.drain(..) {
                        if let Pending::Control(out) = item {
                            pending.push_back(out);
                            continue;
                        }
                        held::Held::forget_parked(&mut self.parked, &item, self.generation);
                        live.held.track(&item);
                        match input::encode_pending(vt, &item) {
                            Ok(bytes) if !bytes.is_empty() => pending.push_back(Out::Input(bytes)),
                            Ok(_) => {}
                            Err(error) => self.close_error = Some(error),
                        }
                    }
                } else if self
                    .pending
                    .iter()
                    .any(|item| !matches!(item, Pending::Control(_)))
                {
                    self.close_error = Some(
                        "Detached before queued input could reach a fresh terminal paint".into(),
                    );
                }
                if !live.await_paint
                    && let Some(vt) = self.vt.as_ref()
                {
                    live.held
                        .release_into(vt, &mut pending, &mut self.close_error);
                }
                if live.spec.role == Role::Active {
                    pending.push_back(Out::Focus(false));
                }
                self.start_drain(Drain {
                    route: self.generation,
                    attach: live.attach,
                    out: live.out,
                    pending,
                    reader: live.reader,
                    writer: live.writer,
                });
            }
            _ => {}
        }
        if self
            .pending
            .iter()
            .any(|item| !matches!(item, Pending::Control(_)))
            && self.close_error.is_none()
        {
            self.close_error = Some("Detached before queued input could reach the terminal".into());
        }
        self.pending.clear();
        self.pending_bytes = 0;
        for parked in std::mem::take(&mut self.parked) {
            self.begin_close(parked);
        }
        // All drains already started, so parallel stalls share one time bound.
        for mut closing in self.closing.drain(..) {
            match (&mut closing.done).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => self.close_error = Some(error),
                Err(error) => {
                    closing.writer_abort.abort();
                    self.close_error = Some(format!("Terminal input drain failed: {error}"));
                }
            }
        }
    }

    pub(crate) fn close_failure(&self) -> Option<&str> {
        self.close_error.as_deref()
    }
    pub(crate) fn take_input_rejection(&mut self) -> Option<String> {
        self.input_rejection.take()
    }
}

impl Drain {
    async fn finish(self) -> Result<(), String> {
        let Self {
            attach,
            out,
            pending,
            reader,
            writer,
            ..
        } = self;
        reader.abort();
        let _ = reader.await;
        for command in pending {
            out.send(command)
                .await
                .map_err(|_| "Terminal input writer closed before drain".to_string())?;
        }
        drop(out);
        writer
            .await
            .map_err(|error| format!("Terminal input writer failed: {error}"))??;
        attach
            .drain_data()
            .await
            .map_err(|error| format!("Terminal input write fence failed: {error}"))?;
        let attach = Arc::try_unwrap(attach)
            .map_err(|_| "Terminal attach still owned after input drain".to_string())?;
        attach
            .detach()
            .await
            .map(|_| ())
            .map_err(|error| format!("Terminal detach failed: {error}"))
    }
}
