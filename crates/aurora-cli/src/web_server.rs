//! Embedded 3D Spatial Audio, Spotify-Style Music Hub & Sonos Multi-Room Web Server.
//!
//! Provides an all-in-one local server delivering:
//! - 11.1.4 3D spatial audio wireframe & Atmos object telemetry.
//! - Spotify-like music library (Artists, Albums, Tracks, Search).
//! - Real-time synchronized lyrics (LRC) scrolling for live mobile display.
//! - Sonos-style multi-room zone grouping and volume controls.
//! - Media link downloader & auto-album organizer.

use anyhow::Result;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Instant;

use crate::media_downloader::ingest_media_link;
use crate::multiroom::MultiRoomCoordinator;
use crate::music_library::MusicLibrary;

/// Global application state shared across HTTP worker threads.
#[derive(Debug)]
pub struct AppState {
    pub library: MusicLibrary,
    pub multiroom: MultiRoomCoordinator,
    pub current_track_id: String,
    pub is_playing: bool,
    pub play_start: Instant,
    pub master_volume: u8,
}

impl Default for AppState {
    fn default() -> Self {
        let library = MusicLibrary::new_with_defaults();
        let current_track_id = "track-the-weeknd-blinding-lights".to_string();
        Self {
            library,
            multiroom: MultiRoomCoordinator::new_with_default_home_zones(),
            current_track_id,
            is_playing: true,
            play_start: Instant::now(),
            master_volume: 70,
        }
    }
}

/// Starts the embedded Web dashboard server.
pub fn start_web_server(port: u16, bind_ip: &str) -> Result<()> {
    let addr = format!("{}:{}", bind_ip, port);
    let listener = TcpListener::bind(&addr)?;
    let state = Arc::new(RwLock::new(AppState::default()));

    println!("\n========================================================");
    println!("  Aurora Cinema & Spotify/Sonos Multi-Room Hub Started!");
    println!("  Open in your browser / phone: http://localhost:{}", port);
    if bind_ip == "0.0.0.0" {
        println!("  Mobile access on home Wi-Fi: http://<your-ip>:{}", port);
    }
    println!("========================================================\n");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state_clone = Arc::clone(&state);
                thread::spawn(move || {
                    let _ = handle_http_client(stream, state_clone);
                });
            }
            Err(e) => {
                eprintln!("Connection error: {e}");
            }
        }
    }

    Ok(())
}

fn handle_http_client(mut stream: TcpStream, state: Arc<RwLock<AppState>>) -> Result<()> {
    let mut buffer = [0u8; 8192];
    let bytes_read = stream.read(&mut buffer)?;
    if bytes_read == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    if parts.len() < 2 {
        return Ok(());
    }

    let method = parts[0];
    let full_path = parts[1];
    let path = full_path.split('?').next().unwrap_or("");
    let query = if full_path.contains('?') {
        full_path.split('?').nth(1).unwrap_or("")
    } else {
        ""
    };

    if method == "GET" && (path == "/" || path == "/index.html") {
        let html = include_str!("web_dashboard.html");
        send_response(&mut stream, "text/html; charset=UTF-8", html.as_bytes())?;
    } else if path == "/api/status" {
        let status_json = r#"{
            "status": "online",
            "active_codec": "Dolby Atmos TrueHD Lossless / E-AC-3 JOC",
            "layout": "11.1.4 (16 Channels)",
            "sample_rate": 48000,
            "bit_depth": 24,
            "active_objects_count": 8,
            "master_volume_db": -12.0,
            "dialogue_boost_db": 3.0,
            "true_peak_limiter": "active (-0.2 dBFS ceiling)",
            "upmixer_mode": "11.1.4 Neural Height Expansion",
            "subwoofer_fir_phase": "Aligned (80 Hz Crossover)"
        }"#;
        send_response(&mut stream, "application/json", status_json.as_bytes())?;
    } else if path == "/api/library/tracks" {
        let st = state.read().unwrap();
        let tracks: Vec<_> = st.library.tracks.values().cloned().collect();
        let json = serde_json::to_string(&tracks)?;
        send_response(&mut stream, "application/json", json.as_bytes())?;
    } else if path == "/api/library/albums" {
        let st = state.read().unwrap();
        let albums: Vec<_> = st.library.albums.values().cloned().collect();
        let json = serde_json::to_string(&albums)?;
        send_response(&mut stream, "application/json", json.as_bytes())?;
    } else if path == "/api/multiroom/zones" {
        let st = state.read().unwrap();
        let zones = st.multiroom.list_zones();
        let json = serde_json::to_string(&zones)?;
        send_response(&mut stream, "application/json", json.as_bytes())?;
    } else if path == "/api/multiroom/group" {
        let zone_id = parse_param(query, "id").unwrap_or_default();
        let mut st = state.write().unwrap();
        st.multiroom.toggle_group(&zone_id);
        let zones = st.multiroom.list_zones();
        let json = serde_json::to_string(&zones)?;
        send_response(&mut stream, "application/json", json.as_bytes())?;
    } else if path == "/api/multiroom/volume" {
        let zone_id = parse_param(query, "id").unwrap_or_default();
        let vol = parse_param(query, "vol")
            .and_then(|v| v.parse::<u8>().ok())
            .unwrap_or(50);
        let mut st = state.write().unwrap();
        st.multiroom.set_zone_volume(&zone_id, vol);
        let zones = st.multiroom.list_zones();
        let json = serde_json::to_string(&zones)?;
        send_response(&mut stream, "application/json", json.as_bytes())?;
    } else if path == "/api/player/current" {
        let st = state.read().unwrap();
        let elapsed_ms = if st.is_playing {
            st.play_start.elapsed().as_millis() as u32
        } else {
            0
        };

        let current_track = st.library.tracks.get(&st.current_track_id).cloned();
        let active_lyric = st
            .library
            .get_current_lyric_line(&st.current_track_id, elapsed_ms);

        #[derive(serde::Serialize)]
        struct PlayerStatus {
            track: Option<crate::music_library::TrackMetadata>,
            elapsed_ms: u32,
            is_playing: bool,
            active_lyric_index: Option<usize>,
            active_lyric_text: Option<String>,
            master_volume: u8,
        }

        let resp = PlayerStatus {
            track: current_track,
            elapsed_ms,
            is_playing: st.is_playing,
            active_lyric_index: active_lyric.as_ref().map(|(idx, _)| *idx),
            active_lyric_text: active_lyric.map(|(_, l)| l.text),
            master_volume: st.master_volume,
        };

        let json = serde_json::to_string(&resp)?;
        send_response(&mut stream, "application/json", json.as_bytes())?;
    } else if path == "/api/player/play" {
        let track_id = parse_param(query, "id");
        let mut st = state.write().unwrap();
        if let Some(id) = track_id {
            if st.library.tracks.contains_key(&id) {
                st.current_track_id = id;
            }
        }
        st.is_playing = true;
        st.play_start = Instant::now();
        send_response(&mut stream, "application/json", b"{\"status\":\"playing\"}")?;
    } else if path == "/api/player/toggle" {
        let mut st = state.write().unwrap();
        st.is_playing = !st.is_playing;
        if st.is_playing {
            st.play_start = Instant::now();
        }
        send_response(&mut stream, "application/json", b"{\"status\":\"ok\"}")?;
    } else if path == "/api/download" {
        let url = parse_param(query, "url").unwrap_or_else(|| {
            // Also check POST body
            if let Some(body_start) = request.find("\r\n\r\n") {
                let body = &request[body_start + 4..];
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(body) {
                    val.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string()
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            }
        });

        if !url.is_empty() {
            let mut st = state.write().unwrap();
            let lib_dir = PathBuf::from("library");
            match ingest_media_link(&url, &mut st.library, &lib_dir) {
                Ok(new_track) => {
                    st.current_track_id = new_track.id.clone();
                    st.is_playing = true;
                    st.play_start = Instant::now();
                    let json = serde_json::to_string(&new_track)?;
                    send_response(&mut stream, "application/json", json.as_bytes())?;
                }
                Err(e) => {
                    let err = format!("{{\"error\":\"{}\"}}", e);
                    send_response(&mut stream, "application/json", err.as_bytes())?;
                }
            }
        } else {
            send_response(&mut stream, "application/json", b"{\"error\":\"Missing url param\"}")?;
        }
    } else {
        let not_found = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(not_found.as_bytes())?;
    }

    Ok(())
}

fn parse_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        let mut parts = pair.split('=');
        if let Some(k) = parts.next() {
            if k == key {
                return parts.next().map(|v| v.replace("%20", " ").replace('+', " "));
            }
        }
    }
    None
}

fn send_response(stream: &mut TcpStream, content_type: &str, body: &[u8]) -> Result<()> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        content_type,
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    Ok(())
}
