//! Command/token fence for speech start/stop.
//!
//! Mirrors `SpeechOwner` in `apple_translation.swift`: one monotonically
//! ordered sequence shared by queued start and stop. A timeout cancels only
//! its own token; a stale start or stop is a no-op. Tests exercise queue
//! interleavings through this type instead of claiming native AVSpeech proof.

#[derive(Clone, Debug, Default)]
pub struct SpeechFence {
    seq: u64,
    latest: u64,
    latest_canceled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeechCommand {
    Start,
    Stop,
}

impl SpeechFence {
    pub fn issue(&mut self, _command: SpeechCommand) -> u64 {
        self.seq = self.seq.saturating_add(1);
        self.latest = self.seq;
        self.latest_canceled = false;
        self.seq
    }

    pub fn cancel(&mut self, token: u64) {
        if token == self.latest {
            self.latest_canceled = true;
        }
    }

    pub fn is_live(&self, token: u64) -> bool {
        token == self.latest && !self.latest_canceled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_cancels_only_its_request() {
        let mut fence = SpeechFence::default();
        let first = fence.issue(SpeechCommand::Start);
        fence.cancel(first);
        assert!(!fence.is_live(first));
        let second = fence.issue(SpeechCommand::Start);
        fence.cancel(first);
        assert!(fence.is_live(second));
        assert!(!fence.is_live(first));
    }

    #[test]
    fn stale_stop_does_not_kill_a_newer_start() {
        let mut fence = SpeechFence::default();
        let start = fence.issue(SpeechCommand::Start);
        let stop = fence.issue(SpeechCommand::Stop);
        let next = fence.issue(SpeechCommand::Start);
        assert!(!fence.is_live(start));
        assert!(!fence.is_live(stop));
        assert!(fence.is_live(next));
    }

    #[test]
    fn late_start_after_timeout_stop_and_new_start() {
        let mut fence = SpeechFence::default();
        let late = fence.issue(SpeechCommand::Start);
        fence.cancel(late);
        let stop = fence.issue(SpeechCommand::Stop);
        let fresh = fence.issue(SpeechCommand::Start);
        assert!(!fence.is_live(late));
        assert!(!fence.is_live(stop));
        assert!(fence.is_live(fresh));
    }
}
