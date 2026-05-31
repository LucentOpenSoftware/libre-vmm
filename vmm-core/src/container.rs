//! Container management — systemd-nspawn / Podman / Docker containers
//! managed alongside VMs in the same library UI.
//!
//! This is the right answer to VMware's failed `vctl` bet: containers and VMs
//! solve different problems but live in the same workflow. Lima and Multipass
//! prove there's demand for unified management. The Libre VMM library window
//! lists VMs and containers side by side so the user picks the right tool for
//! the job without leaving the app.
//!
//! ARCHITECTURE:
//! - [`Backend`] enum names the engine (Nspawn, Podman, Docker).
//! - [`ContainerConfig`] is the user-facing model (saved as JSON alongside
//!   `VmConfig`).
//! - [`Container`] is the runtime view (queried from the backend on demand).
//! - [`ContainerBackend`] trait abstracts the engine; per-engine implementations
//!   live in future submodules (`nspawn.rs`, `podman.rs`, `docker.rs`).
//!
//! Wave 12.9 lands the data model + dispatcher skeleton. Backend
//! implementations are TODO for the next wave.

use crate::error::{VmmError, VmmResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Backend enum
// ---------------------------------------------------------------------------

/// Container engine selector. Each variant maps to one [`ContainerBackend`]
/// implementation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Backend {
    /// systemd-nspawn — built into systemd, full machine container with its
    /// own init. Best for VM-like workloads where you want a "lightweight VM"
    /// without QEMU overhead.
    Nspawn,
    /// Podman — OCI-compatible, rootless, daemonless. Best for everyday
    /// container workloads where you don't want a privileged daemon.
    Podman,
    /// Docker — OCI-compatible, daemon-based. Included for compatibility with
    /// existing Dockerfiles and docker-compose stacks.
    Docker,
}

impl Default for Backend {
    fn default() -> Self {
        // Podman is the safest default: rootless, no daemon, OCI standard.
        Backend::Podman
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Backend::Nspawn => write!(f, "systemd-nspawn"),
            Backend::Podman => write!(f, "Podman"),
            Backend::Docker => write!(f, "Docker"),
        }
    }
}

impl Backend {
    /// Short machine-readable identifier (used in CLI flags, config files).
    pub fn as_str(&self) -> &'static str {
        match self {
            Backend::Nspawn => "nspawn",
            Backend::Podman => "podman",
            Backend::Docker => "docker",
        }
    }

    /// The CLI binary we expect on `$PATH` for this backend.
    pub fn binary(&self) -> &'static str {
        match self {
            Backend::Nspawn => "systemd-nspawn",
            Backend::Podman => "podman",
            Backend::Docker => "docker",
        }
    }
}

// ---------------------------------------------------------------------------
// ContainerConfig (persisted user model)
// ---------------------------------------------------------------------------

/// User-facing container configuration. Persisted as JSON in
/// `~/.local/share/libre-vmm/containers/<uuid>.json`, mirroring the layout
/// used by [`crate::config::VmConfig`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    /// Unique container record ID. Distinct from the engine-assigned ID
    /// (Docker hash / nspawn machine name) so renames don't break our index.
    pub id: Uuid,

    /// User-visible container name. Must pass [`validate_container_name`].
    pub name: String,

    /// Which engine runs this container.
    pub backend: Backend,

    /// OCI image reference (for Podman / Docker, e.g. `docker.io/library/alpine:3.20`)
    /// or rootfs directory path (for nspawn, e.g. `/var/lib/machines/debian`).
    pub image: String,

    /// Optional command override. Empty = use the image's default `CMD`.
    #[serde(default)]
    pub command: Vec<String>,

    /// Environment variables in `KEY=VALUE` form.
    #[serde(default)]
    pub env: Vec<String>,

    /// Volume mounts in `host:container[:ro]` form. Validated at create time.
    #[serde(default)]
    pub volumes: Vec<String>,

    /// Port forwards in `host:container[/proto]` form (proto = tcp|udp).
    #[serde(default)]
    pub ports: Vec<String>,

    /// Network mode: `host`, `bridge`, `none`, or a custom network name.
    /// Empty string = backend default.
    #[serde(default)]
    pub network_mode: String,

    /// User description / notes.
    #[serde(default)]
    pub description: String,

    /// Tags for organization in the library view.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Auto-start on libre-vmm launch?
    #[serde(default)]
    pub autostart: bool,

    /// Memory limit in MiB (0 = unlimited / backend default).
    #[serde(default)]
    pub memory_mib: u64,

    /// CPU limit as fractional cores (0.0 = unlimited, 1.0 = one core,
    /// 2.5 = two-and-a-half cores).
    #[serde(default)]
    pub cpus: f32,
}

impl ContainerConfig {
    /// Construct a minimal config from a name + image + backend choice.
    /// All optional fields are left empty / default.
    pub fn new(name: &str, image: &str, backend: Backend) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            backend,
            image: image.to_string(),
            command: Vec::new(),
            env: Vec::new(),
            volumes: Vec::new(),
            ports: Vec::new(),
            network_mode: String::new(),
            description: String::new(),
            tags: Vec::new(),
            autostart: false,
            memory_mib: 0,
            cpus: 0.0,
        }
    }

    /// Directory where container configs live.
    pub fn config_dir() -> String {
        let home = dirs::home_dir().unwrap_or_default();
        format!("{}/.local/share/libre-vmm/containers", home.display())
    }

    /// Maximum container config file size (CWE-400). Configs are small JSON;
    /// anything over 1 MiB is suspicious.
    const MAX_CONFIG_FILE_SIZE: u64 = 1024 * 1024;
    /// Maximum number of container configs we will load (CWE-400).
    const MAX_CONFIG_COUNT: usize = 1000;

    /// Validate this config. Returns the first error message found, or `Ok(())`.
    /// Called from [`Self::save`].
    pub fn validate(&self) -> VmmResult<()> {
        if let Some(msg) = validate_container_name(&self.name) {
            return Err(VmmError::InvalidConfig(msg.to_string()));
        }
        if self.image.trim().is_empty() {
            return Err(VmmError::InvalidConfig(
                "Container image cannot be empty".to_string(),
            ));
        }
        if self.image.len() > 1024 {
            return Err(VmmError::InvalidConfig(
                "Container image reference too long (max 1024 chars)".to_string(),
            ));
        }
        // Image reference safety: reject NUL and control characters.
        if self
            .image
            .chars()
            .any(|c| c == '\0' || (c.is_control() && c != '\t'))
        {
            return Err(VmmError::InvalidConfig(
                "Container image contains invalid control characters".to_string(),
            ));
        }
        // CPU bounds. Negative / NaN / wildly out-of-range values would either
        // crash the backend CLI or be silently ignored — fail fast here.
        if !self.cpus.is_finite() {
            return Err(VmmError::InvalidConfig(
                "Container CPU limit must be finite".to_string(),
            ));
        }
        if self.cpus < 0.0 {
            return Err(VmmError::InvalidConfig(
                "Container CPU limit cannot be negative".to_string(),
            ));
        }
        if self.cpus > 1024.0 {
            return Err(VmmError::InvalidConfig(
                "Container CPU limit exceeds maximum (1024 cores)".to_string(),
            ));
        }
        // Memory bound: 1 TiB upper limit matches VmConfig.
        if self.memory_mib > 1_048_576 {
            return Err(VmmError::InvalidConfig(
                "Container memory limit exceeds maximum (1 TiB)".to_string(),
            ));
        }
        Ok(())
    }

    /// Save this config to disk.
    /// SECURITY (CWE-732): Sets restrictive file permissions (0o600) so config
    /// data (which may include sensitive env vars) stays private.
    pub fn save(&self) -> VmmResult<()> {
        self.validate()?;
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        let path = format!("{}/{}.json", dir, self.id);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// Load a config from disk by UUID.
    /// Validates the deserialized struct so a tampered file doesn't poison
    /// the rest of the app.
    pub fn load(id: &Uuid) -> VmmResult<Self> {
        let path = format!("{}/{}.json", Self::config_dir(), id);
        let json = std::fs::read_to_string(path)?;
        let cfg: Self = serde_json::from_str(&json)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// List all saved container configs.
    ///
    /// SECURITY (CWE-59): Uses `symlink_metadata` to skip symlinks that could
    /// point outside the config directory.
    /// SECURITY (CWE-400): Enforces per-file size limit and total count limit.
    pub fn list_all() -> VmmResult<Vec<Self>> {
        let dir = Self::config_dir();
        if !std::path::Path::new(&dir).exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if out.len() >= Self::MAX_CONFIG_COUNT {
                tracing::warn!(
                    "Container config count limit reached ({}), skipping remaining",
                    Self::MAX_CONFIG_COUNT
                );
                break;
            }
            if entry.path().extension().is_some_and(|e| e == "json") {
                let lmeta = match std::fs::symlink_metadata(entry.path()) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if lmeta.file_type().is_symlink() {
                    tracing::warn!(
                        "Skipping symlinked container config: {}",
                        entry.path().display()
                    );
                    continue;
                }
                if !lmeta.is_file() {
                    continue;
                }
                if lmeta.len() > Self::MAX_CONFIG_FILE_SIZE {
                    tracing::warn!(
                        "Skipping oversized container config '{}' ({} bytes)",
                        entry.path().display(),
                        lmeta.len()
                    );
                    continue;
                }
                let json = match std::fs::read_to_string(entry.path()) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                if let Ok(cfg) = serde_json::from_str::<ContainerConfig>(&json) {
                    out.push(cfg);
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Delete this config from disk. Does not touch the underlying container —
    /// that's the backend's job.
    pub fn delete(&self) -> VmmResult<()> {
        let path = format!("{}/{}.json", Self::config_dir(), self.id);
        if std::path::Path::new(&path).exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Container state + runtime view
// ---------------------------------------------------------------------------

/// Runtime state of a container, as reported by the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerState {
    Running,
    Stopped,
    Paused,
    /// Process has exited with the given status code.
    Exited(i32),
    /// Backend returned a state we don't model yet (forward-compatible).
    Unknown,
}

impl std::fmt::Display for ContainerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerState::Running => write!(f, "Running"),
            ContainerState::Stopped => write!(f, "Stopped"),
            ContainerState::Paused => write!(f, "Paused"),
            ContainerState::Exited(code) => write!(f, "Exited({})", code),
            ContainerState::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Runtime container info — config + live state. Backends construct these from
/// their `list` operation.
#[derive(Debug, Clone)]
pub struct Container {
    pub config: ContainerConfig,
    pub state: ContainerState,
    /// PID of the container's primary process, when known.
    pub pid: Option<u32>,
    /// Seconds since the container started, when known.
    pub uptime_secs: Option<u64>,
}

// ---------------------------------------------------------------------------
// Backend trait
// ---------------------------------------------------------------------------

/// Engine abstraction. Each backend (nspawn, podman, docker) implements this
/// trait in its own submodule. The GUI / CLI layer calls these methods without
/// caring which engine is underneath.
///
/// All operations take container names rather than UUIDs — backends key on
/// the engine-side identifier, and the [`ContainerConfig::name`] doubles as
/// that key.
pub trait ContainerBackend {
    /// List all containers known to this backend (running or stopped).
    fn list(&self) -> VmmResult<Vec<Container>>;

    /// Start a stopped container.
    fn start(&self, name: &str) -> VmmResult<()>;

    /// Stop a running container.
    fn stop(&self, name: &str) -> VmmResult<()>;

    /// Restart a container (stop, then start).
    fn restart(&self, name: &str) -> VmmResult<()>;

    /// Remove a container. If `force` is true, kill it first.
    fn remove(&self, name: &str, force: bool) -> VmmResult<()>;

    /// Create a new container from a config but do not start it.
    fn create(&self, config: &ContainerConfig) -> VmmResult<()>;

    /// Fetch the last `lines` lines of stdout/stderr.
    fn logs(&self, name: &str, lines: usize) -> VmmResult<String>;

    /// Execute a command inside a running container. Returns stdout.
    fn exec(&self, name: &str, cmd: &[&str]) -> VmmResult<String>;
}

// ---------------------------------------------------------------------------
// Backend stubs — Wave 12.9 placeholders
// ---------------------------------------------------------------------------

/// Returns a not-yet-implemented error tagged with the backend name. Used by
/// all three stub backends below — when Wave 12.10+ lands the real
/// implementations, these stubs go away.
fn not_yet_implemented(backend: Backend, op: &str) -> VmmError {
    VmmError::Other(format!(
        "{} backend: '{}' not yet implemented (Wave 12.9 ships the data \
         model; backend lands in a future wave)",
        backend, op
    ))
}

/// systemd-nspawn backend stub. TODO(wave-12.10): drive `machinectl` and
/// `systemd-nspawn` directly; honor `network_mode`, `volumes`, and resource
/// limits via the appropriate `--bind`, `--network-*`, and slice-property
/// flags.
pub struct NspawnBackend;

impl ContainerBackend for NspawnBackend {
    fn list(&self) -> VmmResult<Vec<Container>> {
        // TODO(wave-12.10): parse `machinectl list --output=json` or query D-Bus.
        Ok(Vec::new())
    }
    fn start(&self, _name: &str) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Nspawn, "start"))
    }
    fn stop(&self, _name: &str) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Nspawn, "stop"))
    }
    fn restart(&self, _name: &str) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Nspawn, "restart"))
    }
    fn remove(&self, _name: &str, _force: bool) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Nspawn, "remove"))
    }
    fn create(&self, _config: &ContainerConfig) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Nspawn, "create"))
    }
    fn logs(&self, _name: &str, _lines: usize) -> VmmResult<String> {
        Err(not_yet_implemented(Backend::Nspawn, "logs"))
    }
    fn exec(&self, _name: &str, _cmd: &[&str]) -> VmmResult<String> {
        Err(not_yet_implemented(Backend::Nspawn, "exec"))
    }
}

/// Podman backend stub. TODO(wave-12.10): drive the `podman` CLI in rootless
/// mode; prefer JSON output (`--format=json`) for parsing. Long-running ops
/// should integrate with [`crate::task::TaskManager`] for progress.
pub struct PodmanBackend;

impl ContainerBackend for PodmanBackend {
    fn list(&self) -> VmmResult<Vec<Container>> {
        // TODO(wave-12.10): `podman ps -a --format=json`.
        Ok(Vec::new())
    }
    fn start(&self, _name: &str) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Podman, "start"))
    }
    fn stop(&self, _name: &str) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Podman, "stop"))
    }
    fn restart(&self, _name: &str) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Podman, "restart"))
    }
    fn remove(&self, _name: &str, _force: bool) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Podman, "remove"))
    }
    fn create(&self, _config: &ContainerConfig) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Podman, "create"))
    }
    fn logs(&self, _name: &str, _lines: usize) -> VmmResult<String> {
        Err(not_yet_implemented(Backend::Podman, "logs"))
    }
    fn exec(&self, _name: &str, _cmd: &[&str]) -> VmmResult<String> {
        Err(not_yet_implemented(Backend::Podman, "exec"))
    }
}

/// Docker backend stub. TODO(wave-12.10): drive the `docker` CLI; share most
/// of its parsing logic with Podman since both speak the same OCI JSON.
/// Consider talking to the Docker daemon over its UNIX socket instead of
/// shelling out, if/when we want async I/O.
pub struct DockerBackend;

impl ContainerBackend for DockerBackend {
    fn list(&self) -> VmmResult<Vec<Container>> {
        // TODO(wave-12.10): `docker ps -a --format=json`.
        Ok(Vec::new())
    }
    fn start(&self, _name: &str) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Docker, "start"))
    }
    fn stop(&self, _name: &str) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Docker, "stop"))
    }
    fn restart(&self, _name: &str) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Docker, "restart"))
    }
    fn remove(&self, _name: &str, _force: bool) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Docker, "remove"))
    }
    fn create(&self, _config: &ContainerConfig) -> VmmResult<()> {
        Err(not_yet_implemented(Backend::Docker, "create"))
    }
    fn logs(&self, _name: &str, _lines: usize) -> VmmResult<String> {
        Err(not_yet_implemented(Backend::Docker, "logs"))
    }
    fn exec(&self, _name: &str, _cmd: &[&str]) -> VmmResult<String> {
        Err(not_yet_implemented(Backend::Docker, "exec"))
    }
}

/// Dispatcher — returns a boxed backend for the requested engine. This is the
/// entry point GUI / CLI code should use; it hides the concrete types so the
/// caller can write `dispatch(cfg.backend).start(&cfg.name)`.
pub fn dispatch(backend: Backend) -> Box<dyn ContainerBackend> {
    match backend {
        Backend::Nspawn => Box::new(NspawnBackend),
        Backend::Podman => Box::new(PodmanBackend),
        Backend::Docker => Box::new(DockerBackend),
    }
}

// ---------------------------------------------------------------------------
// Name validation — shared rules with VmConfig (see config::validate_vm_name).
// ---------------------------------------------------------------------------

/// Validate a container name for safe use in engine CLI arguments and file
/// paths. Mirrors [`crate::config::validate_vm_name`] so VMs and containers
/// follow the same naming rules in the library UI.
///
/// Returns an error message if invalid, or `None` if acceptable.
pub fn validate_container_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("Container name cannot be empty");
    }
    if name.len() > 128 {
        return Some("Container name must be 128 characters or less");
    }
    // Strict allowlist: alphanumeric, spaces, hyphens, underscores, dots.
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || " -_.".contains(c))
    {
        return Some(
            "Container name can only contain letters, numbers, spaces, \
             hyphens, underscores, and dots",
        );
    }
    if name != name.trim() {
        return Some("Container name cannot start or end with whitespace");
    }
    if name.starts_with('.') || name.starts_with('-') {
        return Some("Container name cannot start with a dot or hyphen");
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> ContainerConfig {
        let mut c =
            ContainerConfig::new("test-ctr", "docker.io/library/alpine:3.20", Backend::Podman);
        c.command = vec!["sh".to_string(), "-c".to_string(), "echo hi".to_string()];
        c.env = vec!["FOO=bar".to_string()];
        c.volumes = vec!["/tmp/host:/tmp/guest:ro".to_string()];
        c.ports = vec!["8080:80/tcp".to_string()];
        c.network_mode = "bridge".to_string();
        c.description = "Test container".to_string();
        c.tags = vec!["dev".to_string(), "alpine".to_string()];
        c.memory_mib = 256;
        c.cpus = 1.5;
        c
    }

    #[test]
    fn config_roundtrips_through_json() {
        let original = sample_config();
        let json = serde_json::to_string(&original).expect("serialize");
        let decoded: ContainerConfig = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(decoded.id, original.id);
        assert_eq!(decoded.name, original.name);
        assert_eq!(decoded.backend, original.backend);
        assert_eq!(decoded.image, original.image);
        assert_eq!(decoded.command, original.command);
        assert_eq!(decoded.env, original.env);
        assert_eq!(decoded.volumes, original.volumes);
        assert_eq!(decoded.ports, original.ports);
        assert_eq!(decoded.network_mode, original.network_mode);
        assert_eq!(decoded.description, original.description);
        assert_eq!(decoded.tags, original.tags);
        assert_eq!(decoded.autostart, original.autostart);
        assert_eq!(decoded.memory_mib, original.memory_mib);
        assert!((decoded.cpus - original.cpus).abs() < f32::EPSILON);
    }

    #[test]
    fn config_disk_round_trip_via_explicit_path() {
        // Test the on-disk serialization without mutating process-wide $HOME
        // (which would corrupt parallel tests in other modules that also use
        // dirs::home_dir() — notably restricted::tests).
        //
        // save() and load() are thin wrappers around serde_json + fs::{write,
        // read_to_string}; we test the JSON round-trip in
        // config_roundtrips_through_json. Here we just exercise the file I/O
        // path with an explicit path so no shared state is touched.
        let dir = std::env::temp_dir().join(format!("libre-vmm-container-rt-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let cfg = sample_config();
        let path = dir.join(format!("{}.json", cfg.id));

        // Write via the same serde_json::to_string_pretty save() uses.
        let json = serde_json::to_string_pretty(&cfg).expect("serialize");
        std::fs::write(&path, json).expect("write");

        // Read via the same path load() uses.
        let read = std::fs::read_to_string(&path).expect("read");
        let loaded: ContainerConfig = serde_json::from_str(&read).expect("deserialize");
        loaded.validate().expect("validate");

        assert_eq!(loaded.id, cfg.id);
        assert_eq!(loaded.name, cfg.name);
        assert_eq!(loaded.image, cfg.image);
        assert_eq!(loaded.backend, cfg.backend);
        assert_eq!(loaded.memory_mib, cfg.memory_mib);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backend_display_matches_expectations() {
        assert_eq!(Backend::Nspawn.to_string(), "systemd-nspawn");
        assert_eq!(Backend::Podman.to_string(), "Podman");
        assert_eq!(Backend::Docker.to_string(), "Docker");
    }

    #[test]
    fn backend_as_str_and_binary() {
        assert_eq!(Backend::Nspawn.as_str(), "nspawn");
        assert_eq!(Backend::Podman.as_str(), "podman");
        assert_eq!(Backend::Docker.as_str(), "docker");
        assert_eq!(Backend::Nspawn.binary(), "systemd-nspawn");
        assert_eq!(Backend::Podman.binary(), "podman");
        assert_eq!(Backend::Docker.binary(), "docker");
    }

    #[test]
    fn backend_default_is_podman() {
        assert_eq!(Backend::default(), Backend::Podman);
    }

    #[test]
    fn container_state_display() {
        assert_eq!(ContainerState::Running.to_string(), "Running");
        assert_eq!(ContainerState::Stopped.to_string(), "Stopped");
        assert_eq!(ContainerState::Paused.to_string(), "Paused");
        assert_eq!(ContainerState::Exited(0).to_string(), "Exited(0)");
        assert_eq!(ContainerState::Exited(137).to_string(), "Exited(137)");
        assert_eq!(ContainerState::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn name_validation_accepts_valid_names() {
        assert!(validate_container_name("alpine").is_none());
        assert!(validate_container_name("my-container").is_none());
        assert!(validate_container_name("dev_test.01").is_none());
        assert!(validate_container_name("Container 1").is_none());
        // Exactly 128 chars: should be accepted.
        let max = "a".repeat(128);
        assert!(validate_container_name(&max).is_none());
    }

    #[test]
    fn name_validation_rejects_invalid_names() {
        assert!(validate_container_name("").is_some());
        // Slashes, colons, quotes, etc. are blocked.
        assert!(validate_container_name("bad/name").is_some());
        assert!(validate_container_name("bad:name").is_some());
        assert!(validate_container_name("bad\"name").is_some());
        assert!(validate_container_name("bad$name").is_some());
        // Leading dot / hyphen blocked.
        assert!(validate_container_name(".hidden").is_some());
        assert!(validate_container_name("-flag").is_some());
        // Leading/trailing whitespace blocked.
        assert!(validate_container_name(" leading").is_some());
        assert!(validate_container_name("trailing ").is_some());
        // Over 128 chars blocked.
        let too_long = "a".repeat(129);
        assert!(validate_container_name(&too_long).is_some());
    }

    #[test]
    fn validate_rejects_empty_image() {
        let mut cfg = sample_config();
        cfg.image = String::new();
        assert!(cfg.validate().is_err());
        cfg.image = "   ".to_string();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_bad_cpu_and_memory() {
        let mut cfg = sample_config();
        cfg.cpus = -1.0;
        assert!(cfg.validate().is_err());
        cfg.cpus = f32::NAN;
        assert!(cfg.validate().is_err());
        cfg.cpus = f32::INFINITY;
        assert!(cfg.validate().is_err());
        cfg.cpus = 9999.0;
        assert!(cfg.validate().is_err());
        cfg.cpus = 1.0;
        cfg.memory_mib = 2_000_000;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_control_chars_in_image() {
        let mut cfg = sample_config();
        cfg.image = "alpine\x00malicious".to_string();
        assert!(cfg.validate().is_err());
        cfg.image = "alpine\nmalicious".to_string();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn dispatch_returns_a_backend_for_every_variant() {
        // All three should produce a working trait object whose stub list
        // returns an empty Vec without panicking.
        let n = dispatch(Backend::Nspawn);
        assert!(n.list().unwrap().is_empty());
        let p = dispatch(Backend::Podman);
        assert!(p.list().unwrap().is_empty());
        let d = dispatch(Backend::Docker);
        assert!(d.list().unwrap().is_empty());
    }

    #[test]
    fn stub_backend_returns_not_implemented_for_writes() {
        let p = dispatch(Backend::Podman);
        assert!(p.start("anything").is_err());
        assert!(p.stop("anything").is_err());
        assert!(p.create(&sample_config()).is_err());
    }
}
