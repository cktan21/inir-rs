//! The backend and standalone fallback share one evdev implementation.
use crate::events::Event;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub async fn stream(events: &mpsc::Sender<Event>, stop: &CancellationToken) -> Result<(), String> {
    let (sender, mut receiver) = mpsc::channel(16);
    let worker = inir_utils::input_locks::stream(&sender, stop);
    tokio::pin!(worker);
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            result = &mut worker => return result,
            Some(state) = receiver.recv() => {
                tokio::select! {
                    _ = stop.cancelled() => return Ok(()),
                    result = events.send(Event::Keyboard(state)) => if result.is_err() { return Ok(()); },
                }
            }
        }
    }
}
