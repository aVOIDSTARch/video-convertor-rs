//! FFmpeg filter graph string builder.
//!
//! Constructs `-vf` / `-af` filter strings from high-level parameters.
//! This module is pure string manipulation — no FFmpeg dependency required.

/// Build a video filter chain string for FFmpeg `-vf`.
pub struct VideoFilterChain {
    filters: Vec<String>,
}

impl VideoFilterChain {
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// Scale to target width/height. Use -1 for auto-scale maintaining aspect ratio.
    pub fn scale(mut self, width: i32, height: i32) -> Self {
        self.filters.push(format!("scale={width}:{height}"));
        self
    }

    /// Scale with only width specified (height auto-calculated, divisible by 2).
    pub fn scale_width(self, width: u32) -> Self {
        self.scale(width as i32, -2)
    }

    /// Scale with only height specified (width auto-calculated, divisible by 2).
    pub fn scale_height(self, height: u32) -> Self {
        self.scale(-2, height as i32)
    }

    /// Set frame rate.
    pub fn fps(mut self, fps: f64) -> Self {
        self.filters.push(format!("fps={fps}"));
        self
    }

    /// Crop to width x height at position (x, y).
    pub fn crop(mut self, w: u32, h: u32, x: u32, y: u32) -> Self {
        self.filters.push(format!("crop={w}:{h}:{x}:{y}"));
        self
    }

    /// Pad to width x height (centered).
    pub fn pad(mut self, w: u32, h: u32) -> Self {
        self.filters
            .push(format!("pad={w}:{h}:(ow-iw)/2:(oh-ih)/2"));
        self
    }

    /// Set pixel format (e.g. "yuv420p").
    pub fn pixel_format(mut self, fmt: &str) -> Self {
        self.filters.push(format!("format={fmt}"));
        self
    }

    /// Select a single frame at the given timestamp (seconds).
    pub fn select_frame(mut self, time_secs: f64) -> Self {
        self.filters
            .push(format!("select='gte(t\\,{time_secs})'"));
        self.filters.push("trim=frames=1".to_string());
        self
    }

    /// GIF palette generation pass (split -> palettegen).
    pub fn gif_palettegen(mut self) -> Self {
        self.filters
            .push("split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse".to_string());
        self
    }

    /// Add a raw filter string.
    pub fn raw(mut self, filter: impl Into<String>) -> Self {
        self.filters.push(filter.into());
        self
    }

    /// Build the filter string (comma-separated).
    pub fn build(&self) -> Option<String> {
        if self.filters.is_empty() {
            None
        } else {
            Some(self.filters.join(","))
        }
    }
}

impl Default for VideoFilterChain {
    fn default() -> Self {
        Self::new()
    }
}

/// Build an audio filter chain string for FFmpeg `-af`.
pub struct AudioFilterChain {
    filters: Vec<String>,
}

impl AudioFilterChain {
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// Resample to target sample rate.
    pub fn resample(mut self, sample_rate: u32) -> Self {
        self.filters
            .push(format!("aresample={sample_rate}"));
        self
    }

    /// Set channel layout (e.g. "mono", "stereo").
    pub fn channel_layout(mut self, layout: &str) -> Self {
        self.filters
            .push(format!("aformat=channel_layouts={layout}"));
        self
    }

    /// Loudness normalization (EBU R128).
    pub fn loudnorm(mut self) -> Self {
        self.filters.push("loudnorm".to_string());
        self
    }

    /// Loudness normalization with custom target.
    pub fn loudnorm_target(mut self, target_lufs: f64, target_tp: f64, target_lra: f64) -> Self {
        self.filters.push(format!(
            "loudnorm=I={target_lufs}:TP={target_tp}:LRA={target_lra}"
        ));
        self
    }

    /// Volume adjustment (e.g. "1.5" for 150%, "0.5" for 50%, "3dB").
    pub fn volume(mut self, vol: &str) -> Self {
        self.filters.push(format!("volume={vol}"));
        self
    }

    /// Fade in over the given duration in seconds.
    pub fn fade_in(mut self, duration_secs: f64) -> Self {
        self.filters
            .push(format!("afade=t=in:d={duration_secs}"));
        self
    }

    /// Fade out starting at the given time, lasting the given duration.
    pub fn fade_out(mut self, start_secs: f64, duration_secs: f64) -> Self {
        self.filters
            .push(format!("afade=t=out:st={start_secs}:d={duration_secs}"));
        self
    }

    /// Trim audio to a time range.
    pub fn trim(mut self, start_secs: f64, end_secs: f64) -> Self {
        self.filters
            .push(format!("atrim=start={start_secs}:end={end_secs}"));
        self.filters.push("asetpts=PTS-STARTPTS".to_string());
        self
    }

    /// Add a raw filter string.
    pub fn raw(mut self, filter: impl Into<String>) -> Self {
        self.filters.push(filter.into());
        self
    }

    /// Build the filter string (comma-separated).
    pub fn build(&self) -> Option<String> {
        if self.filters.is_empty() {
            None
        } else {
            Some(self.filters.join(","))
        }
    }
}

impl Default for AudioFilterChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_video_chain_returns_none() {
        assert!(VideoFilterChain::new().build().is_none());
    }

    #[test]
    fn video_scale_and_fps() {
        let chain = VideoFilterChain::new()
            .scale(1280, 720)
            .fps(30.0)
            .build()
            .unwrap();
        assert_eq!(chain, "scale=1280:720,fps=30");
    }

    #[test]
    fn video_scale_width_auto_height() {
        let chain = VideoFilterChain::new().scale_width(480).build().unwrap();
        assert_eq!(chain, "scale=480:-2");
    }

    #[test]
    fn video_pixel_format() {
        let chain = VideoFilterChain::new()
            .scale(1920, 1080)
            .pixel_format("yuv420p")
            .build()
            .unwrap();
        assert_eq!(chain, "scale=1920:1080,format=yuv420p");
    }

    #[test]
    fn video_select_frame() {
        let chain = VideoFilterChain::new()
            .select_frame(5.0)
            .build()
            .unwrap();
        assert!(chain.contains("select="));
        assert!(chain.contains("trim=frames=1"));
    }

    #[test]
    fn video_gif_palette() {
        let chain = VideoFilterChain::new()
            .scale_width(480)
            .fps(15.0)
            .gif_palettegen()
            .build()
            .unwrap();
        assert!(chain.contains("palettegen"));
        assert!(chain.contains("paletteuse"));
    }

    #[test]
    fn empty_audio_chain_returns_none() {
        assert!(AudioFilterChain::new().build().is_none());
    }

    #[test]
    fn audio_resample_and_mono() {
        let chain = AudioFilterChain::new()
            .resample(44100)
            .channel_layout("mono")
            .build()
            .unwrap();
        assert_eq!(chain, "aresample=44100,aformat=channel_layouts=mono");
    }

    #[test]
    fn audio_loudnorm() {
        let chain = AudioFilterChain::new().loudnorm().build().unwrap();
        assert_eq!(chain, "loudnorm");
    }

    #[test]
    fn audio_loudnorm_custom() {
        let chain = AudioFilterChain::new()
            .loudnorm_target(-16.0, -1.5, 11.0)
            .build()
            .unwrap();
        assert_eq!(chain, "loudnorm=I=-16:TP=-1.5:LRA=11");
    }

    #[test]
    fn audio_volume_and_fade() {
        let chain = AudioFilterChain::new()
            .volume("1.5")
            .fade_in(2.0)
            .fade_out(58.0, 2.0)
            .build()
            .unwrap();
        assert_eq!(
            chain,
            "volume=1.5,afade=t=in:d=2,afade=t=out:st=58:d=2"
        );
    }

    #[test]
    fn audio_trim() {
        let chain = AudioFilterChain::new()
            .trim(10.0, 30.0)
            .build()
            .unwrap();
        assert!(chain.contains("atrim=start=10:end=30"));
        assert!(chain.contains("asetpts=PTS-STARTPTS"));
    }

    #[test]
    fn raw_filter() {
        let chain = VideoFilterChain::new()
            .raw("hflip")
            .raw("vflip")
            .build()
            .unwrap();
        assert_eq!(chain, "hflip,vflip");
    }
}
