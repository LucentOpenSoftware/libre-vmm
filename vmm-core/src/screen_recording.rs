//! Screen recording — capture VM console to video via virsh screenshot + ffmpeg.
//!
//! Records the VM display by periodically capturing screenshots with
//! `virsh screenshot` and then assembling them into a video with ffmpeg.

use crate::error::{VmmError, VmmResult};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

/// Video output format.
#[derive(Debug, Clone, PartialEq)]
pub enum VideoFormat {
    Mp4,
    WebM,
    Gif,
}

impl VideoFormat {
    pub fn extension(&self) -> &str {
        match self {
            Self::Mp4 => "mp4",
            Self::WebM => "webm",
            Self::Gif => "gif",
        }
    }

    pub fn ffmpeg_args(&self) -> Vec<&str> {
        match self {
            Self::Mp4 => vec!["-c:v", "libx264", "-pix_fmt", "yuv420p"],
            Self::WebM => vec!["-c:v", "libvpx-vp9", "-b:v", "2M"],
            Self::Gif => vec![],
        }
    }
}

/// Recording quality presets.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordingQuality {
    Low,
    Medium,
    High,
}

/// Recording configuration.
#[derive(Debug, Clone)]
pub struct RecordingConfig {
    pub fps: u32,
    pub format: VideoFormat,
    pub quality: RecordingQuality,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            fps: 10,
            format: VideoFormat::Mp4,
            quality: RecordingQuality::Medium,
        }
    }
}

/// Recording status.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordingStatus {
    Idle,
    Recording,
    Stopping,
    Error(String),
}

/// A screen recording session.
pub struct ScreenRecording {
    pub vm_name: String,
    pub output_path: String,
    pub status: RecordingStatus,
    pub started_at: Option<std::time::Instant>,
    pub frame_count: u64,
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    temp_dir: String,
    config: RecordingConfig,
}

impl Drop for ScreenRecording {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(jh) = self.thread.take() {
            let _ = jh.join();
        }
        // Don't clean up temp_dir here — stop_recording handles the ffmpeg step
    }
}

/// Check if ffmpeg is available.
pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// SECURITY: CWE-20 — Validate VM name to prevent command injection via virsh arguments.
fn validate_vm_name(name: &str) -> VmmResult<()> {
    if name.is_empty() || name.len() > 128 {
        return Err(VmmError::Other(
            "VM name must be 1-128 characters".to_string(),
        ));
    }
    if name.starts_with('-') {
        return Err(VmmError::Other(
            "VM name must not start with hyphen".to_string(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || "-_.".contains(c))
    {
        return Err(VmmError::Other(
            "VM name contains invalid characters".to_string(),
        ));
    }
    Ok(())
}

/// Take a single screenshot of a VM via virsh.
pub fn take_screenshot(vm_name: &str, output_path: &str) -> VmmResult<()> {
    validate_vm_name(vm_name)?;
    let output = Command::new("virsh")
        .args(["screenshot", "--", vm_name, output_path])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("virsh not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(VmmError::Other(format!("Screenshot failed: {}", stderr)));
    }

    Ok(())
}

/// Default recording output directory.
pub fn default_output_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| {
        // Fallback to XDG runtime dir or /var/tmp (safer than /tmp)
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/var/tmp".to_string())
    });
    format!("{}/Videos/libre-vmm", home)
}

/// Start recording the VM console.
///
/// Spawns a background thread that captures screenshots at the configured FPS
/// using `virsh screenshot`. Call `stop_recording()` to stop and assemble the video.
pub fn start_recording(
    vm_name: &str,
    config: &RecordingConfig,
    output_path: &str,
) -> VmmResult<ScreenRecording> {
    validate_vm_name(vm_name)?;

    if !ffmpeg_available() {
        return Err(VmmError::Other(
            "ffmpeg not found — install ffmpeg".to_string(),
        ));
    }

    // SECURITY: CWE-377 — Use /dev/shm (ramdisk) instead of /tmp (world-writable, disk-backed)
    // and set restrictive permissions to prevent other users from reading frames.
    let temp_dir = format!("/dev/shm/.libre-vmm-rec-{}", uuid::Uuid::new_v4());
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| VmmError::Other(format!("Failed to create temp dir: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp_dir, std::fs::Permissions::from_mode(0o700));
    }

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let vm = vm_name.to_string();
    let td = temp_dir.clone();
    let interval = std::time::Duration::from_millis((1000.0 / config.fps as f64) as u64);

    let thread = std::thread::Builder::new()
        .name(format!("recording-{}", vm_name))
        .spawn(move || {
            let mut frame = 0u64;
            while running_clone.load(Ordering::Relaxed) {
                let frame_path = format!("{}/frame-{:06}.ppm", td, frame);
                if let Err(e) = take_screenshot(&vm, &frame_path) {
                    warn!("Frame capture failed: {}", e);
                } else {
                    frame += 1;
                }
                std::thread::sleep(interval);
            }
            info!("Recording thread stopped after {} frames", frame);
        })
        .map_err(|e| VmmError::Other(format!("Failed to start recording thread: {}", e)))?;

    info!(
        "Recording started for VM '{}' at {} FPS",
        vm_name, config.fps
    );

    Ok(ScreenRecording {
        vm_name: vm_name.to_string(),
        output_path: output_path.to_string(),
        status: RecordingStatus::Recording,
        started_at: Some(std::time::Instant::now()),
        frame_count: 0,
        running,
        thread: Some(thread),
        temp_dir,
        config: config.clone(),
    })
}

/// Stop recording and assemble captured frames into a video file.
///
/// Returns the final output path on success.
pub fn stop_recording(recording: &mut ScreenRecording) -> VmmResult<String> {
    // Signal the thread to stop
    recording.running.store(false, Ordering::Relaxed);
    if let Some(jh) = recording.thread.take() {
        let _ = jh.join();
    }

    recording.status = RecordingStatus::Stopping;

    // Count frames
    let frame_count = std::fs::read_dir(&recording.temp_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "ppm"))
                .count()
        })
        .unwrap_or(0);

    if frame_count == 0 {
        let _ = std::fs::remove_dir_all(&recording.temp_dir);
        recording.status = RecordingStatus::Error("No frames captured".to_string());
        return Err(VmmError::Other("No frames captured".to_string()));
    }

    recording.frame_count = frame_count as u64;

    // Ensure output directory exists
    if let Some(parent) = std::path::Path::new(&recording.output_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Assemble with ffmpeg
    let mut args: Vec<String> = vec![
        "-y".to_string(),
        "-framerate".to_string(),
        recording.config.fps.to_string(),
        "-i".to_string(),
        format!("{}/frame-%06d.ppm", recording.temp_dir),
    ];

    for arg in recording.config.format.ffmpeg_args() {
        args.push(arg.to_string());
    }
    args.push(recording.output_path.clone());

    let output = Command::new("ffmpeg")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| VmmError::Other(format!("ffmpeg failed: {}", e)))?;

    // Clean up temp frames
    let _ = std::fs::remove_dir_all(&recording.temp_dir);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        recording.status = RecordingStatus::Error(format!("ffmpeg error: {}", stderr));
        return Err(VmmError::Other(format!(
            "ffmpeg encoding failed: {}",
            stderr
        )));
    }

    recording.status = RecordingStatus::Idle;
    info!(
        "Recording saved: {} ({} frames)",
        recording.output_path, frame_count
    );
    Ok(recording.output_path.clone())
}
