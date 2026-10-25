use super::cdp::Cdp;
use crate::models::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    fs::File,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::sync::oneshot;
use webm::mux::{Segment, SegmentBuilder, VideoCodecId, VideoTrack, Writer};

pub struct Capture {
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
    pub latest: Arc<Mutex<Value>>,
    pub paused: Arc<AtomicBool>,
}
impl Capture {
    pub async fn start(cdp: Cdp, session: String, dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut events = cdp.events.subscribe();
        cdp.call(
            &session,
            "Page.startScreencast",
            json!({"format":"jpeg","quality":70,"maxWidth":1280,"maxHeight":960,"everyNthFrame":1}),
        )
        .await?;
        let latest = Arc::new(Mutex::new(Value::Null));
        let frame = latest.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let pause = paused.clone();
        let (stop, mut stopped) = oneshot::channel();
        let capture_id = crate::models::id();
        let manifest = dir.join(format!("capture-{capture_id}.json"));
        let started_at = crate::models::now();
        let _ = std::fs::write(
            &manifest,
            json!({"started_at":started_at,"session":session,"state":"recording"}).to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel::<(u64, String)>(8);
        let end_time = Arc::new(AtomicU64::new(0));
        let encoder_end = end_time.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let result = encode_stream(rx, &dir, &capture_id, encoder_end);
            if let Err(error) = result {
                let _ = std::fs::write(dir.join("recording-error.txt"), error);
            }
        });
        let task = tokio::spawn(async move {
            let started = Instant::now();
            let mut last = None;
            let mut dropped = 0u64;
            let mut frames = 0u64;
            loop {
                tokio::select! {
                    _ = &mut stopped => break,
                    event = events.recv() => {
                        let v = match event { Ok(v)=>v, Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>continue, Err(_)=>break };
                        if v["method"] != "Page.screencastFrame" || v["sessionId"] != session { continue; }
                        // ACK even when paused or overloaded. The encoder never owns the CDP read loop.
                        let _ = cdp.call(&session,"Page.screencastFrameAck",json!({"sessionId":v["params"]["sessionId"]})).await;
                        let ms = started.elapsed().as_millis() as u64;
                        if pause.load(Ordering::SeqCst) { *frame.lock().unwrap()=Value::Null; continue; }
                        if last.is_some_and(|last|ms.saturating_sub(last) < 100) { continue; }
                        last=Some(ms);
                        let data=v["params"]["data"].as_str().unwrap_or("").to_owned();
                        *frame.lock().unwrap()=json!({"image":data,"timestamp_ms":ms});
                        if tx.try_send((ms,data)).is_err(){dropped+=1;}else{frames+=1;}
                    }
                }
            }
            let _ = cdp.call(&session, "Page.stopScreencast", json!({})).await;
            end_time.store(started.elapsed().as_millis() as u64, Ordering::SeqCst);
            drop(tx);
            let _ = worker.await;
            let _=std::fs::write(manifest,json!({"started_at":started_at,"ended_at":crate::models::now(),"session":session,"duration_ms":started.elapsed().as_millis(),"frames":frames,"dropped_frames":dropped,"state":"finished"}).to_string());
        });
        Ok(Self {
            stop: Some(stop),
            task,
            latest,
            paused,
        })
    }
    pub async fn finish(mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        let _ = self.task.await;
    }
}
struct Video {
    encoder: vpx_encode::Encoder,
    segment: Segment<File>,
    track: VideoTrack,
    start: u64,
    width: u32,
    height: u32,
    temp: PathBuf,
    final_path: PathBuf,
}
impl Video {
    fn new(dir: &std::path::Path, capture: &str, ms: u64, width: u32, height: u32) -> Result<Self> {
        let final_path = dir.join(format!("video-{capture}-{ms}.webm"));
        let temp = final_path.with_extension("partial");
        let writer = Writer::new(File::create(&temp).map_err(|e| e.to_string())?);
        let builder = SegmentBuilder::new(writer).map_err(|e| format!("{e:?}"))?;
        let (builder, track) = builder
            .add_video_track(width, height, VideoCodecId::VP8, None)
            .map_err(|e| format!("{e:?}"))?;
        let encoder = vpx_encode::Encoder::new(vpx_encode::Config {
            width,
            height,
            timebase: [1, 1000],
            bitrate: 1200,
            codec: vpx_encode::VideoCodecId::VP8,
        })
        .map_err(|e| e.to_string())?;
        Ok(Self {
            encoder,
            segment: builder.build(),
            track,
            start: ms,
            width,
            height,
            temp,
            final_path,
        })
    }
    fn frame(&mut self, ms: u64, image: image::RgbImage) -> Result<()> {
        let pixels = yuv420(&image);
        for packet in self
            .encoder
            .encode((ms - self.start) as i64, &pixels)
            .map_err(|e| e.to_string())?
        {
            self.segment
                .add_frame(
                    self.track,
                    packet.data,
                    packet.pts as u64 * 1_000_000,
                    packet.key,
                )
                .map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    }
    fn finish(mut self, end_ms: u64) -> Result<()> {
        let mut tail = self.encoder.finish().map_err(|e| e.to_string())?;
        while let Some(p) = tail.next().map_err(|e| e.to_string())? {
            self.segment
                .add_frame(self.track, p.data, p.pts as u64 * 1_000_000, p.key)
                .map_err(|e| format!("{e:?}"))?;
        }
        self.segment
            .finalize(Some(end_ms.saturating_sub(self.start).max(100)))
            .map_err(|_| "Could not finalize video")?;
        std::fs::rename(self.temp, self.final_path).map_err(|e| e.to_string())
    }
}
fn encode_stream(
    rx: std::sync::mpsc::Receiver<(u64, String)>,
    dir: &std::path::Path,
    capture: &str,
    end_time: Arc<AtomicU64>,
) -> Result<()> {
    let mut video: Option<Video> = None;
    for (ms, data) in rx {
        let bytes = STANDARD.decode(data).map_err(|e| e.to_string())?;
        let decoded = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
        let width = (decoded.width().min(1280) / 2 * 2).max(2);
        let height = ((decoded.height() as u64 * width as u64 / decoded.width() as u64) as u32 / 2
            * 2)
        .max(2);
        let rgb = decoded
            .resize_exact(width, height, image::imageops::FilterType::Triangle)
            .to_rgb8();
        if video
            .as_ref()
            .is_some_and(|v| ms - v.start >= 15_000 || v.width != width || v.height != height)
        {
            video.take().unwrap().finish(ms)?;
        }
        if video.is_none() {
            video = Some(Video::new(dir, capture, ms, width, height)?);
        }
        video.as_mut().unwrap().frame(ms, rgb)?;
    }
    if let Some(v) = video {
        v.finish(end_time.load(Ordering::SeqCst))?;
    }
    Ok(())
}
fn yuv420(rgb: &image::RgbImage) -> Vec<u8> {
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);
    let mut out = vec![0; w * h * 3 / 2];
    for y in 0..h {
        for x in 0..w {
            let p = rgb.get_pixel(x as u32, y as u32);
            let (r, g, b) = (p[0] as f32, p[1] as f32, p[2] as f32);
            out[y * w + x] = (16.0 + 0.257 * r + 0.504 * g + 0.098 * b).clamp(0., 255.) as u8;
            if y % 2 == 0 && x % 2 == 0 {
                let i = y / 2 * (w / 2) + x / 2;
                out[w * h + i] = (128.0 - 0.148 * r - 0.291 * g + 0.439 * b).clamp(0., 255.) as u8;
                out[w * h + w * h / 4 + i] =
                    (128.0 + 0.439 * r - 0.368 * g - 0.071 * b).clamp(0., 255.) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn webm_duration_is_in_timecode_units_not_nanoseconds() {
        let dir = std::env::temp_dir().join(format!("opendots-video-{}", crate::models::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut video = Video::new(&dir, "duration", 0, 16, 16).unwrap();
        video.frame(0, image::RgbImage::new(16, 16)).unwrap();
        video.finish(2000).unwrap();
        let bytes = std::fs::read(dir.join("video-duration-0.webm")).unwrap();
        // Matroska Duration (0x4489) uses the default 1 ms timecode scale.
        let offset = bytes
            .windows(2)
            .position(|b| b == [0x44, 0x89])
            .expect("WebM Duration element");
        let duration = match bytes[offset + 2] {
            0x84 => f32::from_be_bytes(bytes[offset + 3..offset + 7].try_into().unwrap()) as f64,
            0x88 => f64::from_be_bytes(bytes[offset + 3..offset + 11].try_into().unwrap()),
            size => panic!("Unexpected Duration size {size}"),
        };
        assert_eq!(duration, 2000.0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
