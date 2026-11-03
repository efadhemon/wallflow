//! Video decoding through an `ffmpeg` child process.
//!
//! FFmpeg does the demuxing and (hardware) decoding and writes raw NV12 frames to
//! a pipe. Running it as a separate process keeps codec crashes away from the
//! renderer and needs no FFmpeg headers at build time; the same command line works
//! on Linux (VA-API / NVDEC), Windows (D3D11VA) and macOS (VideoToolbox).
//!
//! Frames go through a small bounded channel. When the renderer stops taking them
//! (paused, or the compositor has stopped asking for frames because the desktop is
//! covered), the channel fills, the pipe fills, and FFmpeg simply blocks — so a
//! hidden wallpaper costs no CPU at all.

use crate::probe::VideoInfo;
use anyhow::{Context, Result};
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};

/// Frames decoded ahead of the display. Two is enough to absorb jitter and keeps
/// memory at a few frames.
const QUEUE_DEPTH: usize = 2;

pub struct Frame {
    /// Y plane (`width * height` bytes) followed by the interleaved UV plane.
    pub data: Vec<u8>,
}

pub fn nv12_len(width: u32, height: u32) -> usize {
    let (w, h) = (width as usize, height as usize);
    w * h + 2 * w.div_ceil(2) * h.div_ceil(2)
}

pub struct Decoder {
    child: Child,
    frames: Receiver<Frame>,
    /// Spent buffers go back to the reader thread, so steady-state playback
    /// allocates nothing (multi-megabyte allocations are page faults every time).
    recycle: Sender<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
}

impl Decoder {
    pub fn spawn(ffmpeg: &str, path: &Path, info: &VideoInfo, hwaccel: &str) -> Result<Self> {
        let mut command = Command::new(ffmpeg);
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        if hwaccel != "none" {
            command.args(["-hwaccel", hwaccel]);
        }
        command
            .args(["-stream_loop", "-1", "-i"])
            .arg(path)
            .args(["-an", "-sn", "-dn", "-f", "rawvideo", "-pix_fmt", "nv12"])
            // Keep FFmpeg's own threads modest; one 1080p stream does not need twelve.
            .args(["-threads", "2", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = command
            .spawn()
            .with_context(|| format!("running `{ffmpeg}` (is FFmpeg installed?)"))?;
        let mut stdout = child.stdout.take().context("ffmpeg has no stdout")?;

        let frame_len = nv12_len(info.width, info.height);
        let (frame_tx, frames): (SyncSender<Frame>, _) = mpsc::sync_channel(QUEUE_DEPTH);
        let (recycle, spent) = mpsc::channel::<Vec<u8>>();

        let reader = thread::Builder::new()
            .name("wallflow-decode".into())
            .spawn(move || {
                loop {
                    let mut data = spent.try_recv().unwrap_or_default();
                    data.resize(frame_len, 0);
                    match stdout.read_exact(&mut data) {
                        Ok(()) => {}
                        Err(err) if err.kind() == ErrorKind::UnexpectedEof => {
                            log::info!("decoder reached the end of its output");
                            return;
                        }
                        Err(err) => {
                            log::warn!("reading decoded frames failed: {err}");
                            return;
                        }
                    }
                    if frame_tx.send(Frame { data }).is_err() {
                        return; // renderer is gone
                    }
                }
            })?;

        Ok(Self {
            child,
            frames,
            recycle,
            reader: Some(reader),
        })
    }

    /// The next decoded frame, if one is ready.
    pub fn try_next(&self) -> Result<Option<Frame>, Ended> {
        match self.frames.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(Ended),
        }
    }

    pub fn give_back(&self, frame: Frame) {
        let _ = self.recycle.send(frame.data);
    }
}

#[derive(Debug)]
pub struct Ended;

impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Not joined: the reader may be blocked handing over a frame, and only
        // unblocks once `frames` (dropped right after this) is gone. Either that or
        // the closed pipe ends it promptly.
        drop(self.reader.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nv12_size_rounds_chroma_up() {
        assert_eq!(nv12_len(1920, 1080), 1920 * 1080 * 3 / 2);
        assert_eq!(nv12_len(3, 3), 9 + 2 * 2 * 2);
    }
}
