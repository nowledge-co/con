use super::*;

pub(super) fn spawn_session_save_worker() -> crossbeam_channel::Sender<SessionSaveRequest> {
    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::Builder::new()
        .name("con-session-save".into())
        .spawn(move || {
            loop {
                let request = match rx.recv() {
                    Ok(request) => request,
                    Err(_) => break,
                };

                let (mut latest_session, mut latest_history, mut flush_waiters) = match request {
                    SessionSaveRequest::Save(session, history) => {
                        (Some(session), Some(history), Vec::new())
                    }
                    SessionSaveRequest::Flush(session, history, waiter) => {
                        (Some(session), Some(history), vec![waiter])
                    }
                };

                while let Ok(request) = rx.try_recv() {
                    match request {
                        SessionSaveRequest::Save(session, history) => {
                            latest_session = Some(session);
                            latest_history = Some(history);
                        }
                        SessionSaveRequest::Flush(session, history, waiter) => {
                            latest_session = Some(session);
                            latest_history = Some(history);
                            flush_waiters.push(waiter);
                        }
                    }
                }

                if let Some(session) = latest_session
                    && let Err(err) = session.save()
                {
                    log::warn!("Failed to save session: {}", err);
                }
                if let Some(history) = latest_history
                    && let Err(err) = history.save()
                {
                    log::warn!("Failed to save command history: {}", err);
                }

                for waiter in flush_waiters {
                    let _ = waiter.send(());
                }
            }
        })
        .expect("failed to spawn session save worker");
    tx
}

// ── Theme conversion ──────────────────────────────────────────
