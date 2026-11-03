use super::{MaybeTlsStream, Message, Socket};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub(super) struct Liveness {
    last_ping: Instant,
    awaiting: Option<(Vec<u8>, Instant)>,
    sequence: u64,
}
impl Liveness {
    pub(super) fn new() -> Self {
        Self {
            last_ping: Instant::now(),
            awaiting: None,
            sequence: 0,
        }
    }
    pub(super) fn tick(&mut self, ws: &mut Socket) -> Result<(), String> {
        let now = Instant::now();
        if self
            .awaiting
            .as_ref()
            .is_some_and(|(_, sent)| now.duration_since(*sent) >= Duration::from_secs(10))
        {
            return Err("Assistant connection timed out; the operation was not retried".into());
        }
        if self.awaiting.is_none() && now.duration_since(self.last_ping) >= Duration::from_secs(10)
        {
            self.sequence = self.sequence.wrapping_add(1);
            let payload = self.sequence.to_be_bytes().to_vec();
            ws.send(Message::Ping(payload.clone().into()))
                .map_err(|_| "Assistant connection failed; the operation was not retried")?;
            self.awaiting = Some((payload, now));
            self.last_ping = now;
        }
        Ok(())
    }
    pub(super) fn pong(&mut self, bytes: &[u8]) {
        if self
            .awaiting
            .as_ref()
            .is_some_and(|(expected, _)| expected == bytes)
        {
            self.awaiting = None;
        }
    }
}
pub(super) fn set_read_timeout(ws: &mut Socket, duration: Duration) -> Result<(), String> {
    let tcp = match ws.get_mut() {
        MaybeTlsStream::Plain(s) => s,
        MaybeTlsStream::NativeTls(s) => s.get_mut(),
        _ => return Err("Unsupported connection".into()),
    };
    tcp.set_read_timeout(Some(duration))
        .map_err(|_| "Could not configure connection".into())
}
pub(super) fn service_upload(
    ws: &mut Socket,
    live: &mut Liveness,
    pending: &mut VecDeque<Message>,
) -> Result<(), String> {
    live.tick(ws)?;
    match ws.read() {
        Ok(Message::Pong(bytes)) => live.pong(&bytes),
        Ok(Message::Ping(_)) => ws.flush().map_err(|_| "Assistant connection failed")?,
        Ok(Message::Close(_)) => {
            return Err("Assistant disconnected; the operation was not retried".into());
        }
        Ok(message) => {
            if pending.len() >= 16 {
                return Err("Assistant sent too much data during upload".into());
            }
            pending.push_back(message);
        }
        Err(tungstenite::Error::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) => {}
        Err(_) => return Err("Assistant disconnected; the operation was not retried".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching_pong_clears_only_the_outstanding_ping() {
        let mut live = Liveness::new();
        live.awaiting = Some((vec![1, 2], Instant::now()));
        live.pong(&[9]);
        assert!(live.awaiting.is_some());
        live.pong(&[1, 2]);
        assert!(live.awaiting.is_none());
    }
}
