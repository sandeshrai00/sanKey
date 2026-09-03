//! Shared Symphonia decoder for the loader and converter.

use std::path::Path;

/// Decode `path` via Symphonia into interleaved `f32` PCM.
/// Returns `(samples, channels, sample_rate)`.
pub fn decode_interleaved(path: &str) -> Result<(Vec<f32>, u16, u32), String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::CODEC_TYPE_NULL;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;
    use std::fs::File;

    let file = File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = Path::new(path).extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("Failed to probe format for '{}': {}", path, e))?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("No supported audio tracks found")?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &Default::default())
        .map_err(|e| format!("Failed to create decoder: {}", e))?;

    let sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2) as u16;

    let mut samples = Vec::new();
    // Pre-size from the track header when known: avoids repeated
    // realloc-doubling (up to 2× transient) while packets stream in.
    if let Some(frames) = track.codec_params.n_frames {
        samples.reserve(frames as usize * channels as usize);
    }
    let mut buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(e) => {
                // Truncated/corrupt packets used to be eaten silently, leaving
                // audio shorter than its own timing says (see engine
                // "start sample past end" spam).
                crate::always_eprint!("⚠️  symphonia: read error in '{}': {}", path, e);
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(e) => {
                crate::always_eprint!("⚠️  symphonia: decode error in '{}': {}", path, e);
                continue;
            }
        };
        if buf.is_none() {
            let spec = *decoded.spec();
            let duration = decoded.capacity() as u64;
            buf = Some(SampleBuffer::<f32>::new(duration, spec));
        }
        if let Some(b) = &mut buf {
            b.copy_interleaved_ref(decoded);
            samples.extend_from_slice(b.samples());
        }
    }

    if samples.is_empty() {
        return Err("No audio data decoded".to_string());
    }
    Ok((samples, channels, sample_rate))
}

/// Duration via Symphonia metadata (fast, no decode).
pub fn duration_ms(path: &str) -> Result<f64, Box<dyn std::error::Error>> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;
    use std::fs::File;

    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = Path::new(path).extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())?;
    let format = probed.format;
    let track = format.tracks().iter().find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL).ok_or("No track")?;
    if let Some(tb) = &track.codec_params.time_base {
        if let Some(nf) = track.codec_params.n_frames {
            return Ok(((nf as f64) * (tb.numer as f64) / (tb.denom as f64)) * 1000.0);
        }
    }
    if let Some(sr) = track.codec_params.sample_rate {
        if let Some(nf) = track.codec_params.n_frames {
            return Ok((nf as f64) / (sr as f64) * 1000.0);
        }
    }
    Ok(100.0)
}
