use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::state::SongId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMode {
    Sequential,
    Shuffle,
    RepeatOne,
}

#[derive(Debug, Clone)]
struct PlayingSong {
    song_id: SongId,
    position: Duration,
}

#[derive(Debug, Clone)]
pub struct Queue {
    /// All available songs in their original order
    songs: Vec<SongId>,
    /// Current playback mode
    mode: PlaybackMode,
    /// Current position in the queue
    current_index: usize,
    /// Shuffled order (when in shuffle mode)
    shuffled_indices: Vec<usize>,
    /// LCG state for deterministic shuffle
    lcg_state: u64,
    /// Cached track data for smooth playback
    track_cache: HashMap<SongId, Vec<u8>>,
    /// Cache window size (tracks before/after current)
    cache_size: usize,
    /// Tracks currently being downloaded for cache
    pending_requests: HashSet<SongId>,
    /// Currently playing song state
    playing_song: Option<PlayingSong>,
    /// Song currently being loaded for immediate playback
    loading_song: Option<SongId>,
}

impl Queue {
    /// LCG parameters (from Numerical Recipes)
    const LCG_A: u64 = 1664525;
    const LCG_C: u64 = 1013904223;
    const LCG_M: u64 = (1u64 << 32);

    pub fn new(songs: Vec<SongId>, mode: PlaybackMode, cache_size: usize) -> Self {
        let mut queue = Queue {
            songs,
            mode,
            current_index: 0,
            shuffled_indices: Vec::new(),
            lcg_state: 0,
            track_cache: HashMap::new(),
            cache_size,
            pending_requests: HashSet::new(),
            playing_song: None,
            loading_song: None,
        };

        if mode == PlaybackMode::Shuffle {
            queue.generate_shuffle_from_index(0);
        }

        queue
    }

    pub fn set_mode(&mut self, mode: PlaybackMode) {
        if self.mode != mode {
            self.mode = mode;
            if mode == PlaybackMode::Shuffle {
                self.generate_shuffle_from_current();
            }
            // Clear cache when mode changes as the order has changed
            self.track_cache.clear();
            self.pending_requests.clear();
        }
    }

    pub fn get_mode(&self) -> PlaybackMode {
        self.mode
    }

    pub fn jump_to_song(&mut self, song_id: &SongId) -> Option<usize> {
        // Find the song in the original list
        let original_index = self.songs.iter().position(|s| s == song_id)?;

        match self.mode {
            PlaybackMode::Sequential | PlaybackMode::RepeatOne => {
                self.current_index = original_index;
            }
            PlaybackMode::Shuffle => {
                self.generate_shuffle_from_index(original_index);
            }
        }

        // Clear cache when jumping as the order has changed
        self.track_cache.clear();
        self.pending_requests.clear();

        Some(self.current_index)
    }

    pub fn current_song(&self) -> Option<&SongId> {
        match self.mode {
            PlaybackMode::Sequential | PlaybackMode::RepeatOne => {
                self.songs.get(self.current_index)
            }
            PlaybackMode::Shuffle => {
                let original_index = self.shuffled_indices.get(self.current_index)?;
                self.songs.get(*original_index)
            }
        }
    }

    pub fn advance_to_next(&mut self) -> Option<&SongId> {
        if self.songs.is_empty() {
            return None;
        }

        match self.mode {
            PlaybackMode::RepeatOne => {
                // Don't advance, stay on current song
            }
            PlaybackMode::Sequential | PlaybackMode::Shuffle => {
                self.current_index = (self.current_index + 1) % self.queue_len();
            }
        }
        self.current_song()
    }

    pub fn advance_to_previous(&mut self) -> Option<&SongId> {
        if self.songs.is_empty() {
            return None;
        }

        match self.mode {
            PlaybackMode::RepeatOne => {
                // Don't advance, stay on current song
            }
            PlaybackMode::Sequential | PlaybackMode::Shuffle => {
                if self.current_index == 0 {
                    self.current_index = self.queue_len() - 1;
                } else {
                    self.current_index -= 1;
                }
            }
        }
        self.current_song()
    }

    /// Get cached audio data for the current song
    pub fn get_current_cached_audio(&self) -> Option<Vec<u8>> {
        let current_song = self.current_song()?;
        self.track_cache.get(current_song).cloned()
    }

    /// Add audio data to cache for a specific song
    pub fn cache_track(&mut self, song_id: SongId, audio_data: Vec<u8>) {
        self.track_cache.insert(song_id.clone(), audio_data);
        self.pending_requests.remove(&song_id);
        self.cleanup_cache();
    }

    /// Mark a track as pending download
    pub fn mark_track_pending(&mut self, song_id: SongId) {
        self.pending_requests.insert(song_id);
    }

    /// Get songs that need to be cached but aren't yet
    pub fn get_songs_needing_cache(&self) -> Vec<SongId> {
        let songs_in_window = self.get_songs_around_current(self.cache_size);
        songs_in_window
            .into_iter()
            .filter(|song_id| {
                !self.track_cache.contains_key(song_id) && !self.pending_requests.contains(song_id)
            })
            .collect()
    }

    /// Check if we have too many concurrent requests
    pub fn can_start_more_requests(&self) -> bool {
        self.pending_requests.len() < 3 // Limit concurrent requests
    }

    /// Remove a pending request (e.g., if it failed)
    pub fn remove_pending_request(&mut self, song_id: &SongId) {
        self.pending_requests.remove(song_id);
    }

    fn get_songs_around_current(&self, window_size: usize) -> Vec<SongId> {
        let mut result = Vec::new();
        let queue_len = self.queue_len();

        if queue_len == 0 {
            return result;
        }

        // Get songs in window around current position
        for offset in -(window_size as i32)..=(window_size as i32) {
            let index = (self.current_index as i32 + offset).rem_euclid(queue_len as i32) as usize;

            let song_id = match self.mode {
                PlaybackMode::Sequential | PlaybackMode::RepeatOne => self.songs.get(index),
                PlaybackMode::Shuffle => {
                    if let Some(original_index) = self.shuffled_indices.get(index) {
                        self.songs.get(*original_index)
                    } else {
                        continue;
                    }
                }
            };

            if let Some(song) = song_id {
                result.push(song.clone());
            }
        }

        result
    }

    fn cleanup_cache(&mut self) {
        let songs_to_keep: HashSet<SongId> = self
            .get_songs_around_current(self.cache_size)
            .into_iter()
            .collect();
        self.track_cache
            .retain(|song_id, _| songs_to_keep.contains(song_id));
    }

    pub fn len(&self) -> usize {
        self.songs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    fn queue_len(&self) -> usize {
        match self.mode {
            PlaybackMode::Sequential | PlaybackMode::RepeatOne => self.songs.len(),
            PlaybackMode::Shuffle => self.shuffled_indices.len(),
        }
    }

    fn generate_shuffle_from_index(&mut self, start_index: usize) {
        if self.songs.is_empty() {
            self.shuffled_indices.clear();
            self.current_index = 0;
            return;
        }

        // Seed LCG with the starting song's index for determinism
        self.lcg_state = start_index as u64;

        // Create shuffled indices using Fisher-Yates with LCG
        let mut indices: Vec<usize> = (0..self.songs.len()).collect();

        for i in (1..indices.len()).rev() {
            let j = self.next_lcg() as usize % (i + 1);
            indices.swap(i, j);
        }

        // Find where the start_index ended up and move it to position 0
        if let Some(pos) = indices.iter().position(|&x| x == start_index) {
            indices.swap(0, pos);
        }

        self.shuffled_indices = indices;
        self.current_index = 0;
    }

    fn generate_shuffle_from_current(&mut self) {
        if let Some(current_song) = self.current_song().cloned() {
            if let Some(original_index) = self.songs.iter().position(|s| s == &current_song) {
                self.generate_shuffle_from_index(original_index);
            }
        }
    }

    fn next_lcg(&mut self) -> u64 {
        self.lcg_state = (Self::LCG_A
            .wrapping_mul(self.lcg_state)
            .wrapping_add(Self::LCG_C))
            % Self::LCG_M;
        self.lcg_state
    }

    /// Start playing a specific song
    pub fn start_playing(&mut self, song_id: &SongId) -> Option<Vec<u8>> {
        self.jump_to_song(song_id);
        if let Some(current_song) = self.current_song() {
            self.playing_song = Some(PlayingSong {
                song_id: current_song.clone(),
                position: Duration::from_secs(0),
            });
            self.get_current_cached_audio()
        } else {
            None
        }
    }

    /// Get the next track with audio data, handling repeat modes
    /// Returns (song_id, audio_data) if available
    /// Returns None if currently loading a song to prevent skipping during load
    pub fn get_next_track_with_audio(&mut self) -> Option<(SongId, Vec<u8>)> {
        if self.songs.is_empty() || self.loading_song.is_some() {
            return None;
        }

        match self.mode {
            PlaybackMode::Sequential => {
                // Advance to next track, wrapping around
                self.current_index = (self.current_index + 1) % self.queue_len();
            }
            PlaybackMode::Shuffle => {
                // Advance to next track, wrapping around
                self.current_index = (self.current_index + 1) % self.queue_len();
            }
            PlaybackMode::RepeatOne => {
                // Repeat current track - don't advance
                if let Some(playing_song) = &mut self.playing_song {
                    let song_id = playing_song.song_id.clone();
                    if let Some(audio) = self.track_cache.get(&song_id) {
                        // Reset position for repeat
                        playing_song.position = Duration::from_secs(0);
                        return Some((song_id, audio.clone()));
                    }
                }
            }
        }

        // Get the current song after advancement (for Sequential and Shuffle modes)
        if let Some(current_song) = self.current_song() {
            let song_id = current_song.clone();
            self.playing_song = Some(PlayingSong {
                song_id: song_id.clone(),
                position: Duration::from_secs(0),
            });

            self.track_cache
                .get(&song_id)
                .map(|audio| (song_id, audio.clone()))
        } else {
            None
        }
    }

    /// Start loading a song for immediate playback
    pub fn start_loading_song(&mut self, song_id: SongId) {
        self.loading_song = Some(song_id);
    }

    /// Finish loading a song (success or failure)
    pub fn finish_loading_song(&mut self) {
        self.loading_song = None;
    }

    /// Check if currently loading a song for immediate playback
    pub fn is_loading_song(&self) -> bool {
        self.loading_song.is_some()
    }

    /// Update the position of the currently playing song
    pub fn update_playing_position(&mut self, position: Duration) {
        if let Some(playing_song) = &mut self.playing_song {
            playing_song.position = position;
        }
    }

    /// Get the currently playing song info
    pub fn get_playing_song(&self) -> Option<(SongId, Duration)> {
        self.playing_song
            .as_ref()
            .map(|p| (p.song_id.clone(), p.position))
    }

    /// Check if there's a song currently playing
    pub fn is_playing(&self) -> bool {
        self.playing_song.is_some()
    }

    /// Stop playback
    pub fn stop_playing(&mut self) {
        self.playing_song = None;
    }
}

/// Thread-safe wrapper for Queue
pub type SharedQueue = Arc<RwLock<Queue>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_shuffle() {
        let songs = vec![
            SongId("1".to_string()),
            SongId("2".to_string()),
            SongId("3".to_string()),
            SongId("4".to_string()),
            SongId("5".to_string()),
        ];

        let mut queue1 = Queue::new(songs.clone(), PlaybackMode::Shuffle, 1);
        queue1.jump_to_song(&SongId("3".to_string()));

        let mut queue2 = Queue::new(songs, PlaybackMode::Shuffle, 1);
        queue2.jump_to_song(&SongId("3".to_string()));

        // Should produce the same shuffle sequence
        assert_eq!(queue1.shuffled_indices, queue2.shuffled_indices);
        assert_eq!(queue1.current_song(), queue2.current_song());
    }

    #[test]
    fn test_sequential_playback() {
        let songs = vec![
            SongId("1".to_string()),
            SongId("2".to_string()),
            SongId("3".to_string()),
        ];

        let mut queue = Queue::new(songs, PlaybackMode::Sequential, 1);

        assert_eq!(queue.current_song(), Some(&SongId("1".to_string())));
        assert_eq!(queue.advance_to_next(), Some(&SongId("2".to_string())));
        assert_eq!(queue.advance_to_next(), Some(&SongId("3".to_string())));
        assert_eq!(queue.advance_to_next(), Some(&SongId("1".to_string()))); // Wraps around
    }

    #[test]
    fn test_repeat_one_playback() {
        let songs = vec![
            SongId("1".to_string()),
            SongId("2".to_string()),
            SongId("3".to_string()),
        ];

        let mut queue = Queue::new(songs, PlaybackMode::RepeatOne, 1);

        // Start playing the first song
        assert_eq!(queue.current_song(), Some(&SongId("1".to_string())));

        // In RepeatOne mode, advancing should stay on the same song
        assert_eq!(queue.advance_to_next(), Some(&SongId("1".to_string())));
        assert_eq!(queue.advance_to_next(), Some(&SongId("1".to_string())));

        // Jump to a different song
        queue.jump_to_song(&SongId("2".to_string()));
        assert_eq!(queue.current_song(), Some(&SongId("2".to_string())));

        // Should still repeat the new current song
        assert_eq!(queue.advance_to_next(), Some(&SongId("2".to_string())));
    }
}
