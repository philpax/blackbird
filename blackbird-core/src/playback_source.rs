//! Custom rodio [`Source`] that owns its own current/next slots, pause
//! state, and seek requests.
//!
//! Replaces the use of [`rodio::Player`], which exposes a leaky abstraction
//! around an internal `to_clear` / `sound_count` pair that races against
//! itself when [`clear`](rodio::Player::clear) and
//! [`skip_one`](rodio::Player::skip_one) are called in rapid succession.
//! See <https://github.com/RustAudio/rodio/issues/636> — the rodio
//! maintainers explicitly direct rich playback use cases to compose
//! primitives instead.
//!
//! [`PlaybackSource`] is attached once to the audio mixer; the playback
//! thread drives it via [`PlaybackController`]. The source always emits
//! samples (silence when nothing is loaded or when paused) so that the
//! mixer never drops it.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::time::Duration;

use blackbird_state::TrackId;
use rodio::source::SeekError;
use rodio::{ChannelCount, SampleRate, Source};

use crate::app_state::TrackAndPosition;
use crate::playback_thread::{
    PlaybackState, PlaybackToLogicMessage, ReplayGainTrackInfo, TrackLoadMode, TrackPlayback,
};

/// Number of silence samples to emit per "span" when no source is active.
/// Aligned to the channel count by the consumer; chosen large enough to
/// keep `current_span_len` queries cheap, small enough that
/// `UniformSourceIterator` rebootstraps quickly when a track is loaded.
const SILENCE_SPAN_LEN: usize = 1024;

/// A boxed source of `f32` samples that can cross thread boundaries.
type BoxedSource = Box<dyn Source<Item = f32> + Send>;

/// A track loaded into the audio pipeline. Wraps the decoded source in a
/// [`rodio::source::TrackPosition`] so the playback thread can query
/// playback position without sampling a separate clock.
struct LoadedTrack {
    track_id: TrackId,
    inner: rodio::source::TrackPosition<BoxedSource>,
}

impl LoadedTrack {
    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn position(&self) -> Duration {
        self.inner.get_pos()
    }

    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
}

/// Shared mutable state between [`PlaybackController`] (mutated from the
/// playback thread on message handling) and [`PlaybackSource`] (read /
/// advanced from the audio thread on every sample).
struct State {
    current: Option<LoadedTrack>,
    /// Gapless next slot. Promoted to `current` when `current` exhausts.
    next: Option<LoadedTrack>,
    paused: bool,
    /// Set by [`PlaybackController::seek`] and applied on the next sample
    /// poll. Coalesces multiple seeks issued between polls.
    seek_request: Option<Duration>,
    /// Linear volume; squared from the user-facing 0..1 scale at the
    /// caller. Applied per sample.
    volume: f32,
    /// Channel count and sample rate to report when no source is loaded,
    /// so `UniformSourceIterator` has plausible metadata for its silence
    /// span. Updated whenever a real source becomes current.
    silence_channels: ChannelCount,
    silence_sample_rate: SampleRate,
    /// Logic-layer broadcast tap for `TrackStarted` / `TrackEnded` /
    /// `PlaybackStateChanged`. The audio thread sends here on transitions;
    /// the playback thread sends here on direct state changes.
    event_tx: tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
}

/// Handle for the playback thread to drive [`PlaybackSource`]. Cheap to
/// clone — wraps an `Arc`.
#[derive(Clone)]
pub struct PlaybackController {
    state: Arc<Mutex<State>>,
    replaygain: ReplayGainControl,
}

/// The rodio [`Source`] driven by [`PlaybackController`]. Add this to a
/// mixer once at startup; do not create more than one.
pub struct PlaybackSource {
    state: Arc<Mutex<State>>,
}

impl PlaybackController {
    /// Builds a controller paired with a [`PlaybackSource`]. The source
    /// is intended to be added to a mixer once; the controller can be
    /// cloned freely.
    pub fn new(
        target_channels: ChannelCount,
        target_sample_rate: SampleRate,
        volume: f32,
        apply_replaygain: bool,
        replaygain_preamp_db: f32,
        event_tx: tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
    ) -> (Self, PlaybackSource) {
        let state = Arc::new(Mutex::new(State {
            current: None,
            next: None,
            paused: false,
            seek_request: None,
            volume,
            silence_channels: target_channels,
            silence_sample_rate: target_sample_rate,
            event_tx,
        }));
        let replaygain = ReplayGainControl::new(apply_replaygain, replaygain_preamp_db);
        (
            Self {
                state: state.clone(),
                replaygain,
            },
            PlaybackSource { state },
        )
    }

    /// Loads `track` and either starts it immediately or sits paused at a
    /// saved position. Drops any prior gapless next slot. Broadcasts
    /// `TrackStarted` and `PlaybackStateChanged` so the logic layer
    /// updates its UI.
    pub fn load_track(&self, track: TrackPlayback, mode: TrackLoadMode) -> Result<(), DecodeError> {
        let loaded = decode_track(track, &self.replaygain)?;
        let (track_id, position, broadcast) = {
            let mut state = self.state.lock().unwrap();
            state.silence_channels = loaded.channels();
            state.silence_sample_rate = loaded.sample_rate();
            let track_id = loaded.track_id.clone();
            state.current = Some(loaded);
            state.next = None;
            let (paused, seek) = match mode {
                TrackLoadMode::Play => (false, None),
                TrackLoadMode::Paused(pos) => (true, Some(pos)),
            };
            state.paused = paused;
            state.seek_request = seek;
            let position = seek.unwrap_or_default();
            (track_id, position, state.event_tx.clone())
        };
        let _ = broadcast.send(PlaybackToLogicMessage::TrackStarted(TrackAndPosition {
            track_id,
            position,
        }));
        let new_state = match mode {
            TrackLoadMode::Play => PlaybackState::Playing,
            TrackLoadMode::Paused(_) => PlaybackState::Paused,
        };
        let _ = broadcast.send(PlaybackToLogicMessage::PlaybackStateChanged(new_state));
        Ok(())
    }

    /// Stages `track` as the gapless next track. Replaces any previously
    /// staged next. Has no effect on the currently playing track.
    pub fn append_next(&self, track: TrackPlayback) -> Result<(), DecodeError> {
        let loaded = decode_track(track, &self.replaygain)?;
        let mut state = self.state.lock().unwrap();
        state.next = Some(loaded);
        Ok(())
    }

    /// Drops the staged gapless next track, if any. Used when the playback
    /// mode changes and the next-track selection is no longer valid.
    pub fn clear_next(&self) {
        let mut state = self.state.lock().unwrap();
        state.next = None;
    }

    /// Begins or resumes playback. Broadcasts `PlaybackStateChanged` if
    /// the state actually changed.
    pub fn play(&self) {
        self.set_paused(false);
    }

    /// Pauses playback. Broadcasts `PlaybackStateChanged` if the state
    /// actually changed.
    pub fn pause(&self) {
        self.set_paused(true);
    }

    /// Toggles between playing and paused. Broadcasts
    /// `PlaybackStateChanged` if the state actually changed.
    pub fn toggle(&self) {
        let target = {
            let state = self.state.lock().unwrap();
            !state.paused
        };
        self.set_paused(target);
    }

    fn set_paused(&self, paused: bool) {
        let (changed, new_state, broadcast) = {
            let mut state = self.state.lock().unwrap();
            let changed = state.paused != paused;
            state.paused = paused;
            let new_state = derive_state(state.current.is_some(), paused);
            (changed, new_state, state.event_tx.clone())
        };
        if changed {
            let _ = broadcast.send(PlaybackToLogicMessage::PlaybackStateChanged(new_state));
        }
    }

    /// Stops playback and clears both the current and next slots. The
    /// position is reported as zero in the broadcast for parity with the
    /// previous behavior.
    pub fn stop(&self) {
        let (track_id, broadcast) = {
            let mut state = self.state.lock().unwrap();
            let track_id = state.current.as_ref().map(|t| t.track_id.clone());
            state.current = None;
            state.next = None;
            state.paused = true;
            state.seek_request = None;
            (track_id, state.event_tx.clone())
        };
        let _ = broadcast.send(PlaybackToLogicMessage::PlaybackStateChanged(
            PlaybackState::Stopped,
        ));
        if let Some(track_id) = track_id {
            let _ = broadcast.send(PlaybackToLogicMessage::PositionChanged(TrackAndPosition {
                track_id,
                position: Duration::ZERO,
            }));
        }
    }

    /// Records a seek to be applied on the next audio-thread poll.
    pub fn seek(&self, position: Duration) {
        let mut state = self.state.lock().unwrap();
        state.seek_request = Some(position);
    }

    /// Sets the linear volume multiplier. Caller is responsible for any
    /// curve mapping (e.g. squaring the user-facing 0..1 control).
    pub fn set_volume(&self, volume: f32) {
        let mut state = self.state.lock().unwrap();
        state.volume = volume;
    }

    /// Enables or disables ReplayGain for both the currently playing
    /// source and any future ones.
    pub fn set_replaygain_enabled(&self, enabled: bool) {
        self.replaygain.set_enabled(enabled);
    }

    /// Sets the ReplayGain preamp in dB for both the currently playing
    /// source and any future ones.
    pub fn set_replaygain_preamp_db(&self, preamp_db: f32) {
        self.replaygain.set_preamp_db(preamp_db);
    }

    /// Snapshots the currently playing track and its position, if any.
    /// Returns `None` when nothing is loaded.
    pub fn current_position(&self) -> Option<TrackAndPosition> {
        let state = self.state.lock().unwrap();
        state.current.as_ref().map(|t| TrackAndPosition {
            track_id: t.track_id.clone(),
            position: t.position(),
        })
    }

    /// Returns the current high-level playback state.
    pub fn current_state(&self) -> PlaybackState {
        let state = self.state.lock().unwrap();
        derive_state(state.current.is_some(), state.paused)
    }
}

fn derive_state(has_current: bool, paused: bool) -> PlaybackState {
    match (has_current, paused) {
        (false, _) => PlaybackState::Stopped,
        (true, true) => PlaybackState::Paused,
        (true, false) => PlaybackState::Playing,
    }
}

impl Iterator for PlaybackSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let mut state = self.state.lock().unwrap();

        // Apply any pending seek before we sample, so the seek is
        // observed on the very next poll rather than after a debounce.
        if let Some(pos) = state.seek_request.take()
            && let Some(t) = state.current.as_mut()
        {
            let _ = t.inner.try_seek(pos);
        }

        if state.paused {
            return Some(0.0);
        }

        let volume = state.volume;
        loop {
            let Some(track) = state.current.as_mut() else {
                return Some(0.0);
            };
            if let Some(sample) = track.inner.next() {
                return Some(sample * volume);
            }
            // Current source exhausted; advance to the staged next slot,
            // or transition to stopped silence if nothing is queued.
            state.current = None;
            let Some(next) = state.next.take() else {
                let _ = state.event_tx.send(PlaybackToLogicMessage::TrackEnded);
                let _ = state.event_tx.send(PlaybackToLogicMessage::PlaybackStateChanged(
                    PlaybackState::Stopped,
                ));
                return Some(0.0);
            };
            let track_id = next.track_id.clone();
            let position = next.position();
            state.silence_channels = next.channels();
            state.silence_sample_rate = next.sample_rate();
            state.current = Some(next);
            let _ = state
                .event_tx
                .send(PlaybackToLogicMessage::TrackStarted(TrackAndPosition {
                    track_id,
                    position,
                }));
            // Loop to pull a sample from the new current.
        }
    }
}

impl Source for PlaybackSource {
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        let state = self.state.lock().unwrap();
        match state.current.as_ref() {
            Some(t) => t.current_span_len(),
            None => Some(silence_span(state.silence_channels)),
        }
    }

    #[inline]
    fn channels(&self) -> ChannelCount {
        let state = self.state.lock().unwrap();
        state
            .current
            .as_ref()
            .map(|t| t.channels())
            .unwrap_or(state.silence_channels)
    }

    #[inline]
    fn sample_rate(&self) -> SampleRate {
        let state = self.state.lock().unwrap();
        state
            .current
            .as_ref()
            .map(|t| t.sample_rate())
            .unwrap_or(state.silence_sample_rate)
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        // Stream is "infinite" — we keep emitting silence after every
        // track ends so the mixer doesn't drop us.
        None
    }

    #[inline]
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let mut state = self.state.lock().unwrap();
        match state.current.as_mut() {
            Some(t) => t.inner.try_seek(pos),
            None => Ok(()),
        }
    }
}

/// Returns a silence span size that is a multiple of `channels` so that
/// frame alignment is preserved when the audio thread reads silence
/// before a real source is loaded.
fn silence_span(channels: ChannelCount) -> usize {
    let n = channels.get() as usize;
    let aligned = SILENCE_SPAN_LEN - (SILENCE_SPAN_LEN % n);
    aligned.max(n)
}

// ---------------------------------------------------------------------------
// Decoder + ReplayGain wiring
// ---------------------------------------------------------------------------

/// Decode error returned by [`PlaybackController::load_track`] /
/// [`append_next`]. Carries the failing `TrackId` so the caller can
/// report which track failed.
#[derive(Debug)]
pub struct DecodeError {
    pub track_id: TrackId,
    pub error: rodio::decoder::DecoderError,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "failed to decode track {}: {}",
            self.track_id.0, self.error
        )
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

fn decode_track(
    track: TrackPlayback,
    control: &ReplayGainControl,
) -> Result<LoadedTrack, DecodeError> {
    let TrackPlayback {
        track_id,
        data,
        replaygain,
    } = track;
    let decoder = rodio::decoder::DecoderBuilder::new()
        .with_byte_len(data.len() as u64)
        .with_data(std::io::Cursor::new(data))
        .build();
    let decoder = match decoder {
        Ok(d) => d,
        Err(error) => return Err(DecodeError { track_id, error }),
    };
    // Box the decoder behind the ReplayGain wrapper (when present) so
    // both branches end up with the same `Box<dyn Source>` type.
    let boxed: BoxedSource = match replaygain {
        Some(info) => Box::new(RuntimeReplayGain {
            input: decoder,
            info,
            control: control.clone(),
        }),
        None => Box::new(decoder),
    };
    let inner = boxed.track_position();
    Ok(LoadedTrack { track_id, inner })
}

/// Shared, lock-free settings read per sample by every queued
/// [`RuntimeReplayGain`] source. Owned by [`PlaybackController`] and
/// updated via its `set_replaygain_*` methods.
#[derive(Clone)]
pub struct ReplayGainControl {
    enabled: Arc<AtomicBool>,
    /// Preamp as a linear factor (i.e. `10^(preamp_db / 20)`) stored as
    /// `f32::to_bits` so the atomic load is lock-free.
    preamp_linear_bits: Arc<AtomicU32>,
}

impl ReplayGainControl {
    fn new(enabled: bool, preamp_db: f32) -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(enabled)),
            preamp_linear_bits: Arc::new(AtomicU32::new(db_to_linear(preamp_db).to_bits())),
        }
    }

    fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    fn set_preamp_db(&self, preamp_db: f32) {
        self.preamp_linear_bits
            .store(db_to_linear(preamp_db).to_bits(), Ordering::Relaxed);
    }
}

fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// A rodio [`Source`] wrapper that applies `info.factor * preamp` to each
/// sample when enabled, clamped to `info.inv_peak` to avoid clipping. The
/// enabled flag and preamp value are read per sample from a shared
/// [`ReplayGainControl`] so they can be updated live from the playback
/// thread.
struct RuntimeReplayGain<I> {
    input: I,
    info: ReplayGainTrackInfo,
    control: ReplayGainControl,
}

impl<I> Iterator for RuntimeReplayGain<I>
where
    I: Source,
{
    type Item = I::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.input.next()?;
        if self.control.enabled.load(Ordering::Relaxed) {
            let preamp = f32::from_bits(self.control.preamp_linear_bits.load(Ordering::Relaxed));
            let multiplier = (self.info.factor * preamp).min(self.info.inv_peak);
            Some(sample * multiplier)
        } else {
            Some(sample)
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.input.size_hint()
    }
}

impl<I> Source for RuntimeReplayGain<I>
where
    I: Source,
{
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    #[inline]
    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    #[inline]
    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    #[inline]
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.input.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use rodio::math::nz;

    fn ev_channel() -> tokio::sync::broadcast::Sender<PlaybackToLogicMessage> {
        let (tx, _rx) = tokio::sync::broadcast::channel(64);
        tx
    }

    fn loaded(track_id: &str, samples: Vec<f32>, sr: u32) -> LoadedTrack {
        let sr = SampleRate::new(sr).unwrap();
        let buf = SamplesBuffer::new(nz!(1), sr, samples);
        let boxed: BoxedSource = Box::new(buf);
        LoadedTrack {
            track_id: TrackId(track_id.to_string()),
            inner: boxed.track_position(),
        }
    }

    #[test]
    fn silence_when_no_source() {
        let (_ctrl, mut src) =
            PlaybackController::new(nz!(2), nz!(48000), 1.0, false, 0.0, ev_channel());
        for _ in 0..10 {
            assert_eq!(src.next(), Some(0.0));
        }
    }

    #[test]
    fn pulls_from_current_then_advances_to_next() {
        let (ctrl, mut src) =
            PlaybackController::new(nz!(1), nz!(48000), 1.0, false, 0.0, ev_channel());
        // Inject directly — bypassing decode_track since we just want to
        // exercise the slot-transition logic.
        {
            let mut state = ctrl.state.lock().unwrap();
            state.current = Some(loaded("a", vec![1.0, 2.0], 48000));
            state.next = Some(loaded("b", vec![3.0, 4.0], 48000));
        }
        assert_eq!(src.next(), Some(1.0));
        assert_eq!(src.next(), Some(2.0));
        assert_eq!(src.next(), Some(3.0));
        assert_eq!(src.next(), Some(4.0));
        // Both exhausted: silence forever.
        assert_eq!(src.next(), Some(0.0));
        assert_eq!(src.next(), Some(0.0));
    }

    #[test]
    fn pause_emits_silence_without_advancing_inner() {
        let (ctrl, mut src) =
            PlaybackController::new(nz!(1), nz!(48000), 1.0, false, 0.0, ev_channel());
        {
            let mut state = ctrl.state.lock().unwrap();
            state.current = Some(loaded("a", vec![1.0, 2.0, 3.0], 48000));
        }
        ctrl.pause();
        assert_eq!(src.next(), Some(0.0));
        assert_eq!(src.next(), Some(0.0));
        ctrl.play();
        assert_eq!(src.next(), Some(1.0));
        assert_eq!(src.next(), Some(2.0));
        assert_eq!(src.next(), Some(3.0));
    }

    #[test]
    fn metadata_reflects_new_source_after_transition() {
        let (ctrl, mut src) =
            PlaybackController::new(nz!(2), nz!(48000), 1.0, false, 0.0, ev_channel());
        {
            let mut state = ctrl.state.lock().unwrap();
            state.current = Some(loaded("a", vec![1.0], 44100));
            state.next = Some(loaded("b", vec![2.0], 96000));
        }
        assert_eq!(src.sample_rate().get(), 44100);
        // Drain the only sample from `current`, advancing into `next`.
        let _ = src.next();
        let _ = src.next();
        // Now `current` is `b`; metadata should reflect that.
        assert_eq!(src.sample_rate().get(), 96000);
    }

    #[test]
    fn clear_next_drops_staged_track() {
        let (ctrl, mut src) =
            PlaybackController::new(nz!(1), nz!(48000), 1.0, false, 0.0, ev_channel());
        {
            let mut state = ctrl.state.lock().unwrap();
            state.current = Some(loaded("a", vec![1.0], 48000));
            state.next = Some(loaded("b", vec![2.0], 48000));
        }
        ctrl.clear_next();
        assert_eq!(src.next(), Some(1.0));
        // `next` was cleared, so we get silence after `current` exhausts.
        assert_eq!(src.next(), Some(0.0));
    }

    #[test]
    fn volume_scales_samples() {
        let (ctrl, mut src) =
            PlaybackController::new(nz!(1), nz!(48000), 1.0, false, 0.0, ev_channel());
        {
            let mut state = ctrl.state.lock().unwrap();
            state.current = Some(loaded("a", vec![1.0, 2.0], 48000));
        }
        ctrl.set_volume(0.5);
        assert_eq!(src.next(), Some(0.5));
        assert_eq!(src.next(), Some(1.0));
    }
}
