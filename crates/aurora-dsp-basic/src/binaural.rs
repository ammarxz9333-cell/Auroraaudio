//! Headphone 3D Spatial Audio & Dynamic Head-Tracking Engine for Aurora.
//!
//! Transforms multi-channel spatial cinema mixes (up to 11.1.4 / 16 channels) into an out-of-head
//! binaural headphone listening experience with true height perception and pinna spectral cues.
//!
//! Incorporates dynamic 6-DoF/3-DoF head-tracking (yaw, pitch, roll) from mobile sensors
//! (such as smartphone gyroscope or IMU) to lock the center dialogue channel to the visual screen.

use std::f32::consts::PI;
use aurora_core::ChannelRole;

/// 3D Vector representing virtual speaker or listener positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3D {
    pub x: f32, // Right (+X), Left (-X)
    pub y: f32, // Forward (+Y), Backward (-Y)
    pub z: f32, // Up (+Z), Down (-Z)
}

impl Vec3D {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Creates a unit vector from azimuth (degrees, positive clockwise from front)
    /// and elevation (degrees, positive upwards).
    pub fn from_spherical(azimuth_deg: f32, elevation_deg: f32, distance: f32) -> Self {
        let az_rad = azimuth_deg.to_radians();
        let el_rad = elevation_deg.to_radians();

        let x = distance * el_rad.cos() * az_rad.sin();
        let y = distance * el_rad.cos() * az_rad.cos();
        let z = distance * el_rad.sin();

        Self { x, y, z }
    }

    /// Rotates vector by yaw, pitch, and roll (in radians).
    pub fn rotate(&self, yaw: f32, pitch: f32, roll: f32) -> Self {
        // Yaw (around Z axis)
        let (sy, cy) = yaw.sin_cos();
        let x1 = self.x * cy - self.y * sy;
        let y1 = self.x * sy + self.y * cy;
        let z1 = self.z;

        // Pitch (around X axis)
        let (sp, cp) = pitch.sin_cos();
        let x2 = x1;
        let y2 = y1 * cp - z1 * sp;
        let z2 = y1 * sp + z1 * cp;

        // Roll (around Y axis)
        let (sr, cr) = roll.sin_cos();
        let x3 = x2 * cr + z2 * sr;
        let y3 = y2;
        let z3 = -x2 * sr + z2 * cr;

        Self { x: x3, y: y3, z: z3 }
    }

    /// Computes azimuth in degrees from front (-180 to 180).
    pub fn azimuth_deg(&self) -> f32 {
        self.x.atan2(self.y).to_degrees()
    }

    /// Computes elevation in degrees (-90 to 90).
    pub fn elevation_deg(&self) -> f32 {
        let horizontal_dist = (self.x * self.x + self.y * self.y).sqrt();
        self.z.atan2(horizontal_dist).to_degrees()
    }

    /// Distance from origin (0, 0, 0).
    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt().max(1e-4)
    }
}

/// Head orientation angles received from mobile sensor / IMU (in degrees).
#[derive(Debug, Clone, Copy, Default)]
pub struct HeadOrientation {
    /// Yaw angle (left/right turning), in degrees.
    pub yaw_deg: f32,
    /// Pitch angle (up/down nodding), in degrees.
    pub pitch_deg: f32,
    /// Roll angle (tilt left/right), in degrees.
    pub roll_deg: f32,
}

/// Virtual speaker layout coordinates in standard cinema 11.1.4 room.
#[derive(Debug, Clone)]
pub struct VirtualSpeaker {
    pub role: ChannelRole,
    pub base_position: Vec3D,
}

impl VirtualSpeaker {
    pub fn new(role: ChannelRole, azimuth_deg: f32, elevation_deg: f32, distance_meters: f32) -> Self {
        Self {
            role,
            base_position: Vec3D::from_spherical(azimuth_deg, elevation_deg, distance_meters),
        }
    }
}

/// Biquad filter state for pinna spectral shaping and head shadowing.
#[derive(Debug, Clone, Default)]
struct BiquadFilter {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadFilter {
    fn process_sample(&mut self, input: f32) -> f32 {
        let out = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = out;
        out
    }

    /// Configure as high-shelf filter (for head shadowing on contralateral ear).
    fn set_high_shelf(&mut self, sample_rate: f32, cutoff_hz: f32, gain_db: f32) {
        let gain_lin = 10.0_f32.powf(gain_db / 40.0);
        let omega = 2.0 * PI * cutoff_hz / sample_rate;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let beta = (gain_lin + gain_lin).sqrt();

        let a0 = (gain_lin + 1.0) - (gain_lin - 1.0) * cos_omega + beta * sin_omega;
        self.b0 = (gain_lin * ((gain_lin + 1.0) + (gain_lin - 1.0) * cos_omega + beta * sin_omega)) / a0;
        self.b1 = (-2.0 * gain_lin * ((gain_lin - 1.0) + (gain_lin + 1.0) * cos_omega)) / a0;
        self.b2 = (gain_lin * ((gain_lin + 1.0) + (gain_lin - 1.0) * cos_omega - beta * sin_omega)) / a0;
        self.a1 = (2.0 * ((gain_lin - 1.0) - (gain_lin + 1.0) * cos_omega)) / a0;
        self.a2 = ((gain_lin + 1.0) - (gain_lin - 1.0) * cos_omega - beta * sin_omega) / a0;
    }

    /// Configure as peaking notch / boost filter (for pinna elevation notch).
    fn set_peaking(&mut self, sample_rate: f32, f0: f32, q: f32, gain_db: f32) {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let omega = 2.0 * PI * f0 / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let a0 = 1.0 + alpha / a;

        self.b0 = (1.0 + alpha * a) / a0;
        self.b1 = (-2.0 * omega.cos()) / a0;
        self.b2 = (1.0 - alpha * a) / a0;
        self.a1 = (-2.0 * omega.cos()) / a0;
        self.a2 = (1.0 - alpha / a) / a0;
    }
}

/// Fractional delay line for interaural time difference (ITD).
#[derive(Debug, Clone)]
struct FractionalDelayLine {
    buffer: Vec<f32>,
    write_pos: usize,
}

impl FractionalDelayLine {
    fn new(max_delay_samples: usize) -> Self {
        Self {
            buffer: vec![0.0; max_delay_samples + 4],
            write_pos: 0,
        }
    }

    fn write_sample(&mut self, sample: f32) {
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % self.buffer.len();
    }

    fn read_fractional(&self, delay_samples: f32) -> f32 {
        let buf_len = self.buffer.len();
        let total_pos = self.write_pos as f32 + buf_len as f32 - 1.0 - delay_samples;
        let idx0 = (total_pos.floor() as usize) % buf_len;
        let idx1 = (idx0 + 1) % buf_len;
        let frac = total_pos.fract();

        self.buffer[idx0] * (1.0 - frac) + self.buffer[idx1] * frac
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
    }
}

/// State for a single virtual channel ear rendering (Left or Right ear).
#[derive(Debug, Clone)]
struct EarChannelProcessor {
    delay_line: FractionalDelayLine,
    shadow_filter: BiquadFilter,
    pinna_notch: BiquadFilter,
    current_gain: f32,
    target_gain: f32,
    current_delay: f32,
    target_delay: f32,
}

impl EarChannelProcessor {
    fn new(sample_rate: u32) -> Self {
        let max_delay = (0.003 * sample_rate as f32) as usize + 8; // up to 3ms delay
        let mut shadow = BiquadFilter::default();
        shadow.set_high_shelf(sample_rate as f32, 2500.0, 0.0);
        let mut pinna = BiquadFilter::default();
        pinna.set_peaking(sample_rate as f32, 7500.0, 2.0, 0.0);

        Self {
            delay_line: FractionalDelayLine::new(max_delay),
            shadow_filter: shadow,
            pinna_notch: pinna,
            current_gain: 1.0,
            target_gain: 1.0,
            current_delay: 0.0,
            target_delay: 0.0,
        }
    }

    fn process_sample(&mut self, input: f32) -> f32 {
        self.delay_line.write_sample(input);
        // Smooth parameter interpolation
        self.current_gain += (self.target_gain - self.current_gain) * 0.05;
        self.current_delay += (self.target_delay - self.current_delay) * 0.05;

        let delayed = self.delay_line.read_fractional(self.current_delay);
        let shadowed = self.shadow_filter.process_sample(delayed);
        let pinnaed = self.pinna_notch.process_sample(shadowed);

        pinnaed * self.current_gain
    }

    fn reset(&mut self) {
        self.delay_line.reset();
    }
}

/// Headphone 3D Spatial Audio & Head-Tracking Processor.
pub struct BinauralSpatialEngine {
    sample_rate: u32,
    speakers: Vec<VirtualSpeaker>,
    left_ear_procs: Vec<EarChannelProcessor>,
    right_ear_procs: Vec<EarChannelProcessor>,
    head_orientation: HeadOrientation,
    orientation_zero_offset: HeadOrientation,
    head_tracking_enabled: bool,
    spatial_enabled: bool,
    room_reflection_amount: f32,
}

impl BinauralSpatialEngine {
    /// Creates a 11.1.4 Binaural Cinema spatial engine.
    pub fn new_11_1_4(sample_rate: u32) -> Self {
        // Standard Dolby Atmos / Cinema 11.1.4 virtual speaker coordinate layout
        let speakers = vec![
            VirtualSpeaker::new(ChannelRole::FrontLeft, -30.0, 0.0, 2.2),
            VirtualSpeaker::new(ChannelRole::FrontRight, 30.0, 0.0, 2.2),
            VirtualSpeaker::new(ChannelRole::FrontCenter, 0.0, 0.0, 2.0),
            VirtualSpeaker::new(ChannelRole::LowFrequencyEffects, 0.0, -15.0, 1.8),
            VirtualSpeaker::new(ChannelRole::SurroundLeft, -90.0, 0.0, 2.0),
            VirtualSpeaker::new(ChannelRole::SurroundRight, 90.0, 0.0, 2.0),
            VirtualSpeaker::new(ChannelRole::SurroundBackLeft, -145.0, 0.0, 2.2),
            VirtualSpeaker::new(ChannelRole::SurroundBackRight, 145.0, 0.0, 2.2),
            VirtualSpeaker::new(ChannelRole::WideLeft, -60.0, 0.0, 2.1), // Wide Left
            VirtualSpeaker::new(ChannelRole::WideRight, 60.0, 0.0, 2.1), // Wide Right
            VirtualSpeaker::new(ChannelRole::TopFrontLeft, -45.0, 45.0, 2.4),
            VirtualSpeaker::new(ChannelRole::TopFrontRight, 45.0, 45.0, 2.4),
            VirtualSpeaker::new(ChannelRole::TopRearLeft, -135.0, 45.0, 2.4),
            VirtualSpeaker::new(ChannelRole::TopRearRight, 135.0, 45.0, 2.4),
            VirtualSpeaker::new(ChannelRole::TopSideLeft, -90.0, 55.0, 2.2),
            VirtualSpeaker::new(ChannelRole::TopSideRight, 90.0, 55.0, 2.2),
        ];

        let count = speakers.len();
        let mut left_procs = Vec::with_capacity(count);
        let mut right_procs = Vec::with_capacity(count);

        for _ in 0..count {
            left_procs.push(EarChannelProcessor::new(sample_rate));
            right_procs.push(EarChannelProcessor::new(sample_rate));
        }

        let mut engine = Self {
            sample_rate,
            speakers,
            left_ear_procs: left_procs,
            right_ear_procs: right_procs,
            head_orientation: HeadOrientation::default(),
            orientation_zero_offset: HeadOrientation::default(),
            head_tracking_enabled: true,
            spatial_enabled: true,
            room_reflection_amount: 0.12,
        };

        engine.recalculate_filters();
        engine
    }

    /// Updates listener head orientation (in degrees).
    pub fn update_head_orientation(&mut self, yaw: f32, pitch: f32, roll: f32) {
        self.head_orientation = HeadOrientation {
            yaw_deg: yaw,
            pitch_deg: pitch,
            roll_deg: roll,
        };
        self.recalculate_filters();
    }

    /// Re-zero head tracking so the current phone/head position becomes forward center.
    pub fn center_head_tracking(&mut self) {
        self.orientation_zero_offset = self.head_orientation;
        self.recalculate_filters();
    }

    /// Enable or disable head tracking.
    pub fn set_head_tracking_enabled(&mut self, enabled: bool) {
        self.head_tracking_enabled = enabled;
        self.recalculate_filters();
    }

    /// Enable or disable 3D spatial rendering (if disabled, falls back to direct stereo downmix).
    pub fn set_spatial_enabled(&mut self, enabled: bool) {
        self.spatial_enabled = enabled;
    }

    /// Recalculates ITD (Interaural Time Difference), ILD (Level Difference), and pinna filters.
    fn recalculate_filters(&mut self) {
        // Effective head orientation relative to zero calibration
        let (eff_yaw, eff_pitch, eff_roll) = if self.head_tracking_enabled {
            (
                -(self.head_orientation.yaw_deg - self.orientation_zero_offset.yaw_deg).to_radians(),
                -(self.head_orientation.pitch_deg - self.orientation_zero_offset.pitch_deg).to_radians(),
                -(self.head_orientation.roll_deg - self.orientation_zero_offset.roll_deg).to_radians(),
            )
        } else {
            (0.0, 0.0, 0.0)
        };

        let head_radius_m = 0.0875_f32; // Standard average human head radius
        let speed_of_sound = 343.0_f32; // m/s
        let fs = self.sample_rate as f32;

        for (i, spk) in self.speakers.iter().enumerate() {
            // Rotate speaker position by inverse listener head rotation
            let rel_pos = spk.base_position.rotate(eff_yaw, eff_pitch, eff_roll);
            let az_deg = rel_pos.azimuth_deg();
            let el_deg = rel_pos.elevation_deg();

            let az_rad = az_deg.to_radians();

            // ITD calculation via Woodworth spherical head model:
            // delta_t = (r/c) * (sin(theta) + theta)
            let itd_sec = (head_radius_m / speed_of_sound) * (az_rad.sin() + az_rad);
            let itd_samples = (itd_sec.abs() * fs).min(0.0015 * fs);

            let (left_delay, right_delay) = if az_deg < 0.0 {
                // Speaker is to the left: Left ear is ipsilateral (arrives first), Right ear delayed
                (0.0, itd_samples)
            } else {
                // Speaker is to the right: Right ear is ipsilateral, Left ear delayed
                (itd_samples, 0.0)
            };

            // ILD calculation: High frequency shadowing on contralateral ear
            let shadow_db = -(az_deg.abs() / 90.0).min(1.5) * 6.0; // Up to -9 dB high-frequency damping
            let (left_shadow_db, right_shadow_db) = if az_deg < 0.0 {
                (0.0, shadow_db)
            } else {
                (shadow_db, 0.0)
            };

            // Pinna elevation spectral cue:
            // Elevation causes a pinna notch around 6-10 kHz that shifts with height
            let pinna_freq = (7000.0 + el_deg * 40.0).clamp(4500.0, 12000.0);
            let pinna_notch_db = if el_deg > 10.0 { -4.5 } else { 0.0 };

            // Base distance attenuation
            let dist_attn = (1.5 / rel_pos.length()).clamp(0.5, 1.5);

            // Left ear configuration
            self.left_ear_procs[i].target_delay = left_delay;
            self.left_ear_procs[i].target_gain = dist_attn * if az_deg > 30.0 { 0.7 } else { 1.0 };
            self.left_ear_procs[i].shadow_filter.set_high_shelf(fs, 3000.0, left_shadow_db);
            self.left_ear_procs[i].pinna_notch.set_peaking(fs, pinna_freq, 2.5, pinna_notch_db);

            // Right ear configuration
            self.right_ear_procs[i].target_delay = right_delay;
            self.right_ear_procs[i].target_gain = dist_attn * if az_deg < -30.0 { 0.7 } else { 1.0 };
            self.right_ear_procs[i].shadow_filter.set_high_shelf(fs, 3000.0, right_shadow_db);
            self.right_ear_procs[i].pinna_notch.set_peaking(fs, pinna_freq, 2.5, pinna_notch_db);
        }
    }

    /// Renders multichannel audio (up to 16 channels) to binaural headphone Left and Right.
    ///
    /// `input_channels`: Slice of channel audio buffers.
    /// `out_left`: Output buffer for headphone left ear.
    /// `out_right`: Output buffer for headphone right ear.
    pub fn render_binaural(
        &mut self,
        input_channels: &[Vec<f32>],
        out_left: &mut [f32],
        out_right: &mut [f32],
    ) {
        let frame_count = out_left.len().min(out_right.len());
        out_left[..frame_count].fill(0.0);
        out_right[..frame_count].fill(0.0);

        if !self.spatial_enabled {
            // Direct stereo fold-down fallback
            for (ch_idx, buf) in input_channels.iter().enumerate() {
                let frames = frame_count.min(buf.len());
                let (l_gain, r_gain) = if ch_idx < self.speakers.len() {
                    let az = self.speakers[ch_idx].base_position.azimuth_deg();
                    if az < -10.0 { (0.9, 0.1) } else if az > 10.0 { (0.1, 0.9) } else { (0.7, 0.7) }
                } else {
                    (0.5, 0.5)
                };
                for i in 0..frames {
                    out_left[i] += buf[i] * l_gain;
                    out_right[i] += buf[i] * r_gain;
                }
            }
            return;
        }

        // Render each virtual speaker channel into binaural left and right ears
        let num_speakers = self.speakers.len().min(input_channels.len());

        for ch_idx in 0..num_speakers {
            let in_buf = &input_channels[ch_idx];
            let frames = frame_count.min(in_buf.len());
            let l_proc = &mut self.left_ear_procs[ch_idx];
            let r_proc = &mut self.right_ear_procs[ch_idx];

            for i in 0..frames {
                let s = in_buf[i];
                let l_sample = l_proc.process_sample(s);
                let r_sample = r_proc.process_sample(s);

                out_left[i] += l_sample;
                out_right[i] += r_sample;
            }
        }

        // Subtle cross-feed / early reflection matrix to externalize sound beyond head boundaries
        let refl = self.room_reflection_amount;
        for i in 0..frame_count {
            let l = out_left[i];
            let r = out_right[i];
            out_left[i] = l + r * refl;
            out_right[i] = r + l * refl;
        }
    }

    /// Resets all delay lines and internal filter memories.
    pub fn reset(&mut self) {
        for p in self.left_ear_procs.iter_mut() {
            p.reset();
        }
        for p in self.right_ear_procs.iter_mut() {
            p.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binaural_rendering_and_head_tracking() {
        let mut engine = BinauralSpatialEngine::new_11_1_4(48000);

        // Center channel audio
        let mut channels: Vec<Vec<f32>> = vec![vec![0.0; 256]; 16];
        // Channel 2 is Center
        for s in channels[2].iter_mut() {
            *s = 0.5;
        }

        let mut out_l = vec![0.0; 256];
        let mut out_r = vec![0.0; 256];

        // 1. Initial head forward: Center channel should be symmetric in both ears
        engine.render_binaural(&channels, &mut out_l, &mut out_r);
        let l_sum: f32 = out_l.iter().map(|s| s.abs()).sum();
        let r_sum: f32 = out_r.iter().map(|s| s.abs()).sum();
        assert!((l_sum - r_sum).abs() < 0.15, "Center channel should be balanced");

        // 2. Head rotated 45 degrees to the right: Center speaker appears on the left side of head
        engine.update_head_orientation(45.0, 0.0, 0.0);
        let mut out_l_rot = vec![0.0; 256];
        let mut out_r_rot = vec![0.0; 256];
        engine.render_binaural(&channels, &mut out_l_rot, &mut out_r_rot);

        let l_sum_rot: f32 = out_l_rot.iter().map(|s| s.abs()).sum();
        let r_sum_rot: f32 = out_r_rot.iter().map(|s| s.abs()).sum();
        assert!(l_sum_rot > r_sum_rot, "Rotating head right should make center louder in left ear");
    }
}
