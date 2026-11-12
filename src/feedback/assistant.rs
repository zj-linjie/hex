//! The voice assistant's tones: its start, its epoch-checked stop, and the
//! source that stops producing samples the moment another capture starts.

use super::*;

/// A warm microphone plays a retractable press; a cold one only prepares
/// the output until the microphone confirms an intentional recording.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn assistant_start(release_while_idle: bool) {
    if release_while_idle {
        prepare();
    } else {
        play_provisional_start();
    }
}

/// Assistant release uses the same sound without changing a dictation job's
/// stop timestamp, which governs its independently arriving completion tone.
pub fn assistant_stop_current(epoch: u64) -> bool {
    CAPTURE_EPOCH.load(Ordering::Acquire) == epoch
}

pub(super) fn assistant_stop_tone(epoch: u64) -> Option<Tone> {
    assistant_stop_current(epoch).then_some(Tone::DictationStop)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn play_assistant_stop(epoch: u64, start_owed: bool) {
    if assistant_stop_current(epoch) {
        assistant_stop_to(DICTATION_PLAYER.get(), sounds_enabled(), epoch, start_owed);
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn assistant_stop_to(
    player: Option<&SyncSender<Request>>,
    enabled: bool,
    epoch: u64,
    start_owed: bool,
) {
    if enabled {
        enqueue(
            player,
            if start_owed {
                Request::AssistantStartStop(epoch)
            } else {
                Request::AssistantStop(epoch)
            },
        );
    }
}

/// A delayed assistant cue stops producing samples as soon as another capture
/// starts, including after the mixer has accepted it.
pub(super) struct EpochSource<S> {
    pub(super) inner: S,
    pub(super) epoch: u64,
}

impl<S: Source> Iterator for EpochSource<S> {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if assistant_stop_current(self.epoch) {
            self.inner.next()
        } else {
            None
        }
    }
}

impl<S: Source> Source for EpochSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

pub(super) fn cue_allowed_after_open(enabled: bool, still_current: impl FnOnce() -> bool) -> bool {
    enabled && still_current()
}
