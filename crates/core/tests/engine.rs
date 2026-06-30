//! Integration tests for the subprocess engine against a real ffmpeg/ffprobe.

mod common;

use common::*;
use media_convertor_core::ffmpeg::command::{
    concat_args, extract_audio_args, filter_args, thumbnail_args, transcode_args, Trim,
};
use media_convertor_core::operation::ConvertRequest;
use media_convertor_core::progress::{NoProgress, ProgressEvent, ProgressHandler};
use media_convertor_core::{Config, Container, Engine};
use std::path::Path;

fn engine() -> Engine {
    Engine::new(&Config::default()).expect("engine init")
}

#[test]
fn discovers_capabilities() {
    if !require_ffmpeg() {
        return;
    }
    let caps = engine().capabilities().clone();
    assert!(caps.encoders.len() > 20, "expected many encoders");
    assert!(caps.has_encoder("aac"), "aac encoder should exist");
    assert!(caps.has_filter("scale"), "scale filter should exist");
    assert!(!caps.muxers.is_empty());
    assert!(!caps.version.is_empty());
}

#[test]
fn probe_reports_streams() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let mp4 = child(dir.path(), "in.mp4");
    make_mp4(&mp4, 2.0);

    let info = engine().probe(&mp4).expect("probe");
    assert!(info.has_video());
    assert!(info.has_audio());
    assert_eq!(info.container, Some(Container::Mp4));
    assert!(info.duration.unwrap().as_secs_f64() > 1.0);
}

#[test]
fn probe_missing_file_errors() {
    if !require_ffmpeg() {
        return;
    }
    let err = engine().probe(Path::new("/nonexistent/file.mp4"));
    assert!(err.is_err());
}

#[test]
fn transcode_audio_reencode() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let wav = child(dir.path(), "in.wav");
    let mp3 = child(dir.path(), "out.mp3");
    make_wav(&wav, 2.0);

    let fmt = ConvertRequest {
        preset: Some("podcast-mp3".into()),
        ..Default::default()
    }
    .build_format(None)
    .unwrap();
    let args = transcode_args(&wav, &mp3, &fmt, &Trim::default());
    engine().run(&args, None, &mut NoProgress, None).expect("transcode");

    assert!(mp3.exists());
    assert_eq!(stream_codecs(&mp3), vec!["mp3"]);
}

#[test]
fn transcode_video_reencode_hevc() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let mp4 = child(dir.path(), "in.mp4");
    let mkv = child(dir.path(), "out.mkv");
    make_mp4(&mp4, 2.0);

    let fmt = ConvertRequest {
        format: Some("mkv".into()),
        video_codec: Some("h265".into()),
        crf: Some(30),
        audio_codec: Some("opus".into()),
        ..Default::default()
    }
    .build_format(None)
    .unwrap();
    let args = transcode_args(&mp4, &mkv, &fmt, &Trim::default());
    engine().run(&args, None, &mut NoProgress, None).expect("transcode");

    let codecs = stream_codecs(&mkv);
    assert!(codecs.contains(&"hevc".to_string()), "got {codecs:?}");
    assert!(codecs.contains(&"opus".to_string()), "got {codecs:?}");
}

#[test]
fn transcode_emits_progress() {
    if !require_ffmpeg() {
        return;
    }
    struct Capture {
        count: usize,
        completed: bool,
    }
    impl ProgressHandler for Capture {
        fn on_progress(&mut self, _e: &ProgressEvent) {
            self.count += 1;
        }
        fn on_complete(&mut self) {
            self.completed = true;
        }
    }

    let dir = tempdir();
    let wav = child(dir.path(), "in.wav");
    let mp3 = child(dir.path(), "out.mp3");
    make_wav(&wav, 3.0);

    let fmt = ConvertRequest {
        preset: Some("podcast-mp3".into()),
        ..Default::default()
    }
    .build_format(None)
    .unwrap();
    let args = transcode_args(&wav, &mp3, &fmt, &Trim::default());

    let total = engine().probe(&wav).ok().and_then(|i| i.duration);
    let mut cap = Capture {
        count: 0,
        completed: false,
    };
    engine().run(&args, total, &mut cap, None).unwrap();
    assert!(cap.completed, "on_complete should fire");
}

#[test]
fn extract_audio_copy() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let mp4 = child(dir.path(), "in.mp4");
    let out = child(dir.path(), "audio.m4a");
    make_mp4(&mp4, 2.0);

    let args = extract_audio_args(&mp4, &out, None);
    engine().run(&args, None, &mut NoProgress, None).expect("extract");
    assert!(out.exists());
    assert_eq!(stream_codecs(&out), vec!["aac"]);
}

#[test]
fn thumbnail_single_frame() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let mp4 = child(dir.path(), "in.mp4");
    let jpg = child(dir.path(), "thumb.jpg");
    make_mp4(&mp4, 2.0);

    let args = thumbnail_args(&mp4, &jpg, 1.0, Some(160));
    engine().run(&args, None, &mut NoProgress, None).expect("thumbnail");
    assert!(jpg.exists());
    assert!(std::fs::metadata(&jpg).unwrap().len() > 0);
}

#[test]
fn filter_scale() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let mp4 = child(dir.path(), "in.mp4");
    let out = child(dir.path(), "scaled.mp4");
    make_mp4(&mp4, 2.0);

    let args = filter_args(&mp4, &out, "scale=160:-2");
    engine().run(&args, None, &mut NoProgress, None).expect("filter");

    let info = engine().probe(&out).unwrap();
    assert_eq!(info.video_stream().unwrap().width, Some(160));
}

#[test]
fn concat_two_clips() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let a = child(dir.path(), "a.mkv");
    let out = child(dir.path(), "joined.mkv");
    make_mp4(&a, 2.0);
    // Re-mux to mkv first so concat sees identical streams.
    let mkv = child(dir.path(), "a2.mkv");
    engine()
        .run(
            &["-i".into(), a.clone().into_os_string(), mkv.clone().into_os_string()],
            None,
            &mut NoProgress,
            None,
        )
        .unwrap();

    let list = child(dir.path(), "list.txt");
    let p = mkv.canonicalize().unwrap();
    std::fs::write(
        &list,
        format!("file '{}'\nfile '{}'\n", p.display(), p.display()),
    )
    .unwrap();
    let args = concat_args(&list, &out);
    engine().run(&args, None, &mut NoProgress, None).expect("concat");

    let info = engine().probe(&out).unwrap();
    assert!(info.duration.unwrap().as_secs_f64() > 3.0);
}

#[test]
fn transcode_trim_window() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let wav = child(dir.path(), "in.wav");
    let out = child(dir.path(), "trim.mp3");
    make_wav(&wav, 5.0);

    let fmt = ConvertRequest {
        preset: Some("podcast-mp3".into()),
        ..Default::default()
    }
    .build_format(None)
    .unwrap();
    let trim = Trim {
        start: Some(1.0),
        duration: Some(2.0),
        end: None,
    };
    let args = transcode_args(&wav, &out, &fmt, &trim);
    engine().run(&args, None, &mut NoProgress, None).unwrap();

    let dur = engine().probe(&out).unwrap().duration.unwrap().as_secs_f64();
    assert!((1.5..2.6).contains(&dur), "trimmed duration was {dur}");
}

#[test]
fn bad_codec_fails_cleanly() {
    if !require_ffmpeg() {
        return;
    }
    let dir = tempdir();
    let wav = child(dir.path(), "in.wav");
    let out = child(dir.path(), "out.mp3");
    make_wav(&wav, 1.0);

    // mp3 container cannot hold an aac stream → ffmpeg errors; engine surfaces it.
    let args = vec![
        "-i".into(),
        wav.clone().into_os_string(),
        "-c:a".into(),
        "aac".into(),
        out.clone().into_os_string(),
    ];
    let result = engine().run(&args, None, &mut NoProgress, None);
    assert!(result.is_err(), "expected ffmpeg to reject aac-in-mp3");
}
