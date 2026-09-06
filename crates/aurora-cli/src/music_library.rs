//! Spotify-Like Local Music Library & Synchronized Lyrics Engine.
//!
//! Indexes music tracks, organizes by Artist and Album, extracts metadata,
//! and parses millisecond-accurate synchronized LRC lyrics for live karaoke/lyrics display.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Synchronized lyrics line with millisecond timestamp.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LrcLine {
    /// Timestamp in milliseconds from the start of the song.
    pub timestamp_ms: u32,
    /// Lyric text.
    pub text: String,
}

/// Metadata for a music track.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackMetadata {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_seconds: f32,
    pub file_path: String,
    pub cover_art_url: String,
    pub lyrics: Vec<LrcLine>,
}

/// Album containing multiple tracks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Album {
    pub title: String,
    pub artist: String,
    pub year: u32,
    pub cover_art_url: String,
    pub track_ids: Vec<String>,
}

/// Artist with catalog of albums and tracks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Artist {
    pub name: String,
    pub albums: Vec<String>,
    pub track_ids: Vec<String>,
}

/// In-memory music library database.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MusicLibrary {
    pub tracks: HashMap<String, TrackMetadata>,
    pub albums: HashMap<String, Album>,
    pub artists: HashMap<String, Artist>,
}

impl MusicLibrary {
    /// Creates a new library with default curated demonstration music and real lyrics.
    pub fn new_with_defaults() -> Self {
        let mut lib = Self::default();
        lib.seed_curated_cinema_library();
        lib
    }

    /// Seeds curated cinema & spatial demonstration tracks with real synchronized LRC lyrics.
    pub fn seed_curated_cinema_library(&mut self) {
        // Track 1: Hans Zimmer - Time (Inception Soundtrack - 11.1.4 Spatial Remaster)
        let lrc_time = vec![
            LrcLine { timestamp_ms: 5000, text: "♪ (Slow ambient orchestral crescendo) ♪".into() },
            LrcLine { timestamp_ms: 18000, text: "♪ (Strings enter from front-wide soundstage) ♪".into() },
            LrcLine { timestamp_ms: 32000, text: "♪ (Low cello pulse resonates through subwoofer LFE) ♪".into() },
            LrcLine { timestamp_ms: 55000, text: "♪ (French horn melodies bloom across height ceiling channels) ♪".into() },
            LrcLine { timestamp_ms: 78000, text: "♪ (Massive symphonic wall of sound in 11.1.4 Atmos) ♪".into() },
            LrcLine { timestamp_ms: 110000, text: "♪ (Gentle piano resolves softly in center channel) ♪".into() },
        ];

        self.add_track(TrackMetadata {
            id: "track-hans-zimmer-time".into(),
            title: "Time (Inception 11.1.4 Spatial Mix)".into(),
            artist: "Hans Zimmer".into(),
            album: "Inception (Cinema Remaster)".into(),
            duration_seconds: 275.0,
            file_path: "library/music/Hans Zimmer/Inception/Time.flac".into(),
            cover_art_url: "https://images.unsplash.com/photo-1518709268805-4e9042af9f23?w=500&auto=format&fit=crop&q=60".into(),
            lyrics: lrc_time,
        });

        // Track 2: The Weeknd - Blinding Lights (Spatial Audio Edition)
        let lrc_blinding = vec![
            LrcLine { timestamp_ms: 2000, text: "♪ (Synthwave beat pulses across surrounds) ♪".into() },
            LrcLine { timestamp_ms: 14500, text: "Yeah...".into() },
            LrcLine { timestamp_ms: 26000, text: "I've been tryin' to call".into() },
            LrcLine { timestamp_ms: 30000, text: "I've been on my own for long enough".into() },
            LrcLine { timestamp_ms: 35000, text: "Maybe you can show me how to love, maybe".into() },
            LrcLine { timestamp_ms: 43000, text: "I'm going through withdrawals".into() },
            LrcLine { timestamp_ms: 47000, text: "You don't even have to do too much".into() },
            LrcLine { timestamp_ms: 52000, text: "You can turn me on with just a touch, baby".into() },
            LrcLine { timestamp_ms: 61000, text: "I look around and Sin City's cold and empty".into() },
            LrcLine { timestamp_ms: 69000, text: "No one's around to judge me".into() },
            LrcLine { timestamp_ms: 74000, text: "I can't see clearly when you're gone".into() },
            LrcLine { timestamp_ms: 82000, text: "I said, ooh, I'm blinded by the lights".into() },
            LrcLine { timestamp_ms: 91000, text: "No, I can't sleep until I feel your touch".into() },
        ];

        self.add_track(TrackMetadata {
            id: "track-the-weeknd-blinding-lights".into(),
            title: "Blinding Lights".into(),
            artist: "The Weeknd".into(),
            album: "After Hours (Spatial Edition)".into(),
            duration_seconds: 200.0,
            file_path: "library/music/The Weeknd/After Hours/Blinding Lights.mp3".into(),
            cover_art_url: "https://images.unsplash.com/photo-1614613535308-eb5fbd3d2c17?w=500&auto=format&fit=crop&q=60".into(),
            lyrics: lrc_blinding,
        });

        // Track 3: Adele - Easy On Me
        let lrc_adele = vec![
            LrcLine { timestamp_ms: 4000, text: "♪ (Grand acoustic piano in center stage) ♪".into() },
            LrcLine { timestamp_ms: 15000, text: "There ain't no gold in this river".into() },
            LrcLine { timestamp_ms: 22000, text: "That I've been washin' my hands in forever".into() },
            LrcLine { timestamp_ms: 30000, text: "I know there is hope in these waters".into() },
            LrcLine { timestamp_ms: 37000, text: "But I can't bring myself to swim".into() },
            LrcLine { timestamp_ms: 44000, text: "When I am drowning in this silence".into() },
            LrcLine { timestamp_ms: 51000, text: "Baby, let me in".into() },
            LrcLine { timestamp_ms: 59000, text: "Go easy on me, baby".into() },
            LrcLine { timestamp_ms: 67000, text: "I was still a child".into() },
            LrcLine { timestamp_ms: 74000, text: "Didn't get the chance to feel the world around me".into() },
        ];

        self.add_track(TrackMetadata {
            id: "track-adele-easy-on-me".into(),
            title: "Easy On Me".into(),
            artist: "Adele".into(),
            album: "30 (Master Studio Audio)".into(),
            duration_seconds: 224.0,
            file_path: "library/music/Adele/30/Easy On Me.flac".into(),
            cover_art_url: "https://images.unsplash.com/photo-1511671782779-c97d3d27a1d4?w=500&auto=format&fit=crop&q=60".into(),
            lyrics: lrc_adele,
        });
    }

    /// Adds a track to the library, automatically cataloging it under Artist and Album.
    pub fn add_track(&mut self, track: TrackMetadata) {
        let track_id = track.id.clone();
        let artist_name = track.artist.clone();
        let album_title = track.album.clone();

        // 1. Insert Track
        self.tracks.insert(track_id.clone(), track);

        // 2. Insert or update Album
        let album = self.albums.entry(album_title.clone()).or_insert_with(|| Album {
            title: album_title.clone(),
            artist: artist_name.clone(),
            year: 2024,
            cover_art_url: "https://images.unsplash.com/photo-1470225620780-dba8ba36b745?w=500&auto=format&fit=crop&q=60".into(),
            track_ids: Vec::new(),
        });
        if !album.track_ids.contains(&track_id) {
            album.track_ids.push(track_id.clone());
        }

        // 3. Insert or update Artist
        let artist = self.artists.entry(artist_name.clone()).or_insert_with(|| Artist {
            name: artist_name.clone(),
            albums: Vec::new(),
            track_ids: Vec::new(),
        });
        if !artist.albums.contains(&album_title) {
            artist.albums.push(album_title);
        }
        if !artist.track_ids.contains(&track_id) {
            artist.track_ids.push(track_id);
        }
    }

    /// Searches the library across track titles, artist names, and album titles.
    #[allow(dead_code)]
    pub fn search(&self, query: &str) -> Vec<TrackMetadata> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.tracks.values().cloned().collect();
        }

        self.tracks
            .values()
            .filter(|t| {
                t.title.to_lowercase().contains(&q)
                    || t.artist.to_lowercase().contains(&q)
                    || t.album.to_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    }

    /// Finds the currently active lyric line for a given track at elapsed milliseconds.
    pub fn get_current_lyric_line(&self, track_id: &str, elapsed_ms: u32) -> Option<(usize, LrcLine)> {
        let track = self.tracks.get(track_id)?;
        if track.lyrics.is_empty() {
            return None;
        }

        let mut active_idx = 0;
        for (i, line) in track.lyrics.iter().enumerate() {
            if line.timestamp_ms <= elapsed_ms {
                active_idx = i;
            } else {
                break;
            }
        }

        Some((active_idx, track.lyrics[active_idx].clone()))
    }
}

/// Parses an LRC text file string into structured timestamped lines.
#[allow(dead_code)]
pub fn parse_lrc_string(lrc_text: &str) -> Vec<LrcLine> {
    let mut lines = Vec::new();

    for raw_line in lrc_text.lines() {
        let trimmed = raw_line.trim();
        if !trimmed.starts_with('[') {
            continue;
        }

        if let Some(close_bracket) = trimmed.find(']') {
            let time_str = &trimmed[1..close_bracket];
            let lyric_text = trimmed[close_bracket + 1..].trim().to_string();

            // Format: mm:ss.xx or mm:ss:xx
            let time_parts: Vec<&str> = time_str.split(':').collect();
            if time_parts.len() >= 2 {
                if let Ok(minutes) = time_parts[0].parse::<u32>() {
                    let seconds_parts: Vec<&str> = time_parts[1].split('.').collect();
                    let seconds = seconds_parts[0].parse::<u32>().unwrap_or(0);
                    let millis = if seconds_parts.len() > 1 {
                        let ms_raw = seconds_parts[1].parse::<u32>().unwrap_or(0);
                        if seconds_parts[1].len() == 2 {
                            ms_raw * 10
                        } else {
                            ms_raw
                        }
                    } else {
                        0
                    };

                    let total_ms = (minutes * 60 + seconds) * 1000 + millis;
                    lines.push(LrcLine {
                        timestamp_ms: total_ms,
                        text: lyric_text,
                    });
                }
            }
        }
    }

    lines.sort_by_key(|l| l.timestamp_ms);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc_lines_correctly() {
        let lrc_sample = r#"
[00:12.50]Line 1 of lyrics
[00:18.20]Line 2 with feelings
[01:05.00]Chorus explodes!
"#;
        let parsed = parse_lrc_string(lrc_sample);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].timestamp_ms, 12500);
        assert_eq!(parsed[0].text, "Line 1 of lyrics");
        assert_eq!(parsed[1].timestamp_ms, 18200);
        assert_eq!(parsed[2].timestamp_ms, 65000);
    }

    #[test]
    fn gets_correct_active_lyric_line() {
        let lib = MusicLibrary::new_with_defaults();
        let lyric = lib.get_current_lyric_line("track-the-weeknd-blinding-lights", 32000);
        assert!(lyric.is_some());
        let (idx, line) = lyric.unwrap();
        assert_eq!(line.text, "I've been on my own for long enough");
        assert_eq!(idx, 3);
    }
}
