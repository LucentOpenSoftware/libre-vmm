//! Minimal VNC (RFB) client for embedding in egui.
//! Connects to QEMU's VNC server, receives framebuffer updates,
//! and forwards keyboard/mouse input.

use eframe::egui;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

/// Shared state between the VNC background thread and the GUI.
#[derive(Clone)]
pub struct VncFramebuffer {
    inner: Arc<Mutex<VncState>>,
}

pub struct VncState {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>, // RGBA, row-major
    pub connected: bool,
    pub error: Option<String>,
    /// Pending input events to send to the server
    pub pending_keys: Vec<KeyEvent>,
    pub pending_mouse: Option<MouseEvent>,
    pub needs_refresh: bool,
    /// Pending resolution change request (width, height) from the GUI.
    pub pending_resize: Option<(u16, u16)>,
    /// Monotonically increasing counter bumped on every pixel mutation.
    /// Used by the GUI to skip texture uploads when the framebuffer is unchanged.
    pub pixel_generation: u64,
}

#[derive(Clone)]
pub struct KeyEvent {
    pub down: bool,
    pub key: u32, // X11 keysym
}

#[derive(Clone)]
pub struct MouseEvent {
    pub x: u16,
    pub y: u16,
    pub buttons: u8,
}

impl VncFramebuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VncState {
                width: 0,
                height: 0,
                pixels: Vec::new(),
                connected: false,
                error: None,
                pending_keys: Vec::new(),
                pending_mouse: None,
                needs_refresh: true,
                pending_resize: None,
                pixel_generation: 0,
            })),
        }
    }

    /// Get the shared VNC state. Returns None if the mutex is poisoned,
    /// indicating a panic in the VNC thread left state potentially corrupted.
    /// Callers must handle the None case by treating the connection as dead.
    /// SECURITY: CWE-662 — Never recover poisoned mutex with corrupted state.
    pub fn get_state(&self) -> Option<std::sync::MutexGuard<'_, VncState>> {
        match self.inner.lock() {
            Ok(guard) => Some(guard),
            Err(_poison) => {
                tracing::error!(
                    "VNC state mutex poisoned — prior panic left state corrupted. \
                     Treating connection as dead."
                );
                None
            },
        }
    }

    /// Send a key event to the VM.
    /// SECURITY: CWE-770 — Cap queue size to prevent unbounded memory growth
    /// if the VNC thread is slow or blocked.
    pub fn send_key(&self, down: bool, keysym: u32) {
        const MAX_PENDING_KEYS: usize = 1024;
        if let Ok(mut state) = self.inner.lock() {
            if state.pending_keys.len() < MAX_PENDING_KEYS {
                state.pending_keys.push(KeyEvent { down, key: keysym });
            } else {
                tracing::warn!(
                    "VNC key event queue full ({} events), dropping event",
                    MAX_PENDING_KEYS
                );
            }
        }
    }

    /// Send a mouse/pointer event to the VM.
    pub fn send_mouse(&self, x: u16, y: u16, buttons: u8) {
        if let Ok(mut state) = self.inner.lock() {
            state.pending_mouse = Some(MouseEvent { x, y, buttons });
        }
    }

    /// Request a full framebuffer refresh.
    #[allow(dead_code)]
    pub fn request_refresh(&self) {
        if let Ok(mut state) = self.inner.lock() {
            state.needs_refresh = true;
        }
    }

    /// Request a guest display resolution change via ExtendedDesktopSize.
    /// The VNC thread will send a SetDesktopSize message on the next loop iteration.
    pub fn request_resolution(&self, width: u16, height: u16) {
        // Clamp to reasonable bounds
        let w = width.clamp(320, 8192);
        let h = height.clamp(200, 8192);
        if let Ok(mut state) = self.inner.lock() {
            // Only queue if different from current size
            if state.width != w || state.height != h {
                state.pending_resize = Some((w, h));
            }
        }
    }

    /// Start the VNC client in a background thread (no auth).
    /// SECURITY: Only allows connections to localhost to prevent SSRF (CWE-918).
    #[allow(dead_code)]
    pub fn connect(&self, host: &str, port: u16) {
        self.connect_with_password(host, port, None);
    }

    /// Start the VNC client with optional password for VNC authentication.
    pub fn connect_with_password(&self, host: &str, port: u16, password: Option<String>) {
        // SECURITY: Restrict VNC connections to localhost only (CWE-918)
        const ALLOWED_HOSTS: &[&str] = &["127.0.0.1", "::1", "localhost"];
        if !ALLOWED_HOSTS.contains(&host) {
            tracing::error!(
                "VNC connection to non-localhost host '{}' blocked (SSRF prevention)",
                host
            );
            if let Ok(mut state) = self.inner.lock() {
                state.error = Some(format!(
                    "VNC connection blocked: only localhost allowed, got '{}'",
                    host
                ));
            }
            return;
        }

        let fb = self.clone();
        let addr = format!("{}:{}", host, port);
        let pwd = password;

        if let Err(e) = std::thread::Builder::new()
            .name("vnc-client".into())
            .spawn(move || {
                if let Err(e) = vnc_thread(&fb, &addr, pwd.as_deref()) {
                    if let Ok(mut state) = fb.inner.lock() {
                        state.connected = false;
                        state.error = Some(format!("{}", e));
                    }
                }
            })
        {
            tracing::error!("Failed to spawn VNC thread: {}", e);
            if let Ok(mut state) = self.inner.lock() {
                state.error = Some(format!("Thread spawn failed: {}", e));
            }
        }
    }

    /// Disconnect.
    pub fn disconnect(&self) {
        if let Ok(mut state) = self.inner.lock() {
            state.connected = false;
        }
    }
}

// ============ VNC DES Authentication ============

/// VNC authentication uses DES with bit-reversed key bytes.
/// The 16-byte challenge is encrypted in two 8-byte ECB blocks.
fn vnc_des_encrypt(challenge: &[u8; 16], password: &str) -> [u8; 16] {
    // Build 8-byte key from password (padded with zeros, truncated to 8)
    let mut key = [0u8; 8];
    for (i, &b) in password.as_bytes().iter().take(8).enumerate() {
        // VNC reverses bits in each key byte
        key[i] = reverse_bits(b);
    }
    let mut response = [0u8; 16];
    // Encrypt each 8-byte half with DES ECB
    des_encrypt_block(&key, &challenge[0..8], &mut response[0..8]);
    des_encrypt_block(&key, &challenge[8..16], &mut response[8..16]);
    response
}

fn reverse_bits(b: u8) -> u8 {
    let mut r = 0u8;
    for i in 0..8 {
        r |= ((b >> i) & 1) << (7 - i);
    }
    r
}

/// Minimal DES ECB encryption (single 8-byte block).
/// Implements the full DES algorithm for VNC auth compatibility.
fn des_encrypt_block(key: &[u8; 8], input: &[u8], output: &mut [u8]) {
    // Initial and final permutation tables
    static IP: [u8; 64] = [
        58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14,
        6, 64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11,
        3, 61, 53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
    ];
    static FP: [u8; 64] = [
        40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62,
        30, 37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19,
        59, 27, 34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
    ];
    // Expansion permutation
    static E: [u8; 48] = [
        32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17,
        18, 19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
    ];
    // Permuted choice 1
    static PC1: [u8; 56] = [
        57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3,
        60, 52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45,
        37, 29, 21, 13, 5, 28, 20, 12, 4,
    ];
    // Permuted choice 2
    static PC2: [u8; 48] = [
        14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41,
        52, 31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
    ];
    // P permutation
    static P: [u8; 32] = [
        16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9,
        19, 13, 30, 6, 22, 11, 4, 25,
    ];
    // S-boxes
    static SBOXES: [[u8; 64]; 8] = [
        [
            14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6,
            12, 11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2,
            4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
        ],
        [
            15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 14, 12, 0,
            1, 10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1,
            3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
        ],
        [
            10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8,
            5, 14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13,
            0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
        ],
        [
            7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7,
            2, 12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6,
            10, 1, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
        ],
        [
            2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0,
            15, 10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7,
            1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
        ],
        [
            12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1,
            13, 14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12,
            9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
        ],
        [
            4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3,
            5, 12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8,
            1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
        ],
        [
            13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5,
            6, 2, 0, 14, 9, 11, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1, 13, 4, 8, 2, 14,
            13, 6, 9, 12, 7, 1, 5, 11, 3, 10, 0, 15,
        ],
    ];
    static SHIFTS: [u8; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

    fn get_bit(data: &[u8], pos: u8) -> u64 {
        let byte_idx = (pos as usize - 1) / 8;
        let bit_idx = 7 - ((pos as usize - 1) % 8);
        ((data[byte_idx] >> bit_idx) & 1) as u64
    }
    fn permute(data: &[u8], table: &[u8], _bits: usize) -> u64 {
        let mut result: u64 = 0;
        for (i, &pos) in table.iter().enumerate() {
            result |= get_bit(data, pos) << (table.len() - 1 - i);
        }
        result
    }
    fn u64_to_bytes(val: u64, nbits: usize) -> Vec<u8> {
        let nbytes = (nbits + 7) / 8;
        let mut out = vec![0u8; nbytes];
        for i in 0..nbytes {
            out[i] = ((val >> (nbits - 8 - i * 8)) & 0xFF) as u8;
        }
        out
    }

    // Key schedule
    let cd = permute(key, &PC1, 56);
    let mut c = (cd >> 28) & 0x0FFFFFFF;
    let mut d = cd & 0x0FFFFFFF;
    let mut subkeys = [0u64; 16];
    for round in 0..16 {
        let shift = SHIFTS[round] as u32;
        c = ((c << shift) | (c >> (28 - shift))) & 0x0FFFFFFF;
        d = ((d << shift) | (d >> (28 - shift))) & 0x0FFFFFFF;
        let cd_bytes = u64_to_bytes((c << 28) | d, 56);
        subkeys[round] = permute(&cd_bytes, &PC2, 48);
    }

    // Encrypt
    let ip_val = permute(input, &IP, 64);
    let mut l = (ip_val >> 32) & 0xFFFFFFFF;
    let mut r = ip_val & 0xFFFFFFFF;

    for round in 0..16 {
        let r_bytes = u64_to_bytes(r, 32);
        let expanded = permute(&r_bytes, &E, 48);
        let xored = expanded ^ subkeys[round];

        // S-box substitution
        let mut s_out: u32 = 0;
        for i in 0..8 {
            let bits = ((xored >> (42 - i * 6)) & 0x3F) as usize;
            let row = ((bits >> 5) << 1) | (bits & 1);
            let col = (bits >> 1) & 0xF;
            let val = SBOXES[i][row * 16 + col] as u32;
            s_out |= val << (28 - i * 4);
        }

        let s_bytes = u64_to_bytes(s_out as u64, 32);
        let p_val = permute(&s_bytes, &P, 32);
        let new_r = l ^ (p_val & 0xFFFFFFFF);
        l = r;
        r = new_r;
    }

    // Final permutation (note: L and R are swapped before FP)
    let pre_fp = (r << 32) | l;
    let pre_bytes = u64_to_bytes(pre_fp, 64);
    let result = permute(&pre_bytes, &FP, 64);
    let result_bytes = u64_to_bytes(result, 64);
    for i in 0..8 {
        output[i] = result_bytes[i];
    }
}

// ============ VNC/RFB Protocol Implementation ============

fn vnc_thread(
    fb: &VncFramebuffer,
    addr: &str,
    password: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(std::time::Duration::from_millis(50)))?;
    // SECURITY: CWE-400 — Set write timeout to prevent indefinite blocking
    // if the VNC server stops reading (e.g., resource exhaustion attack).
    stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;

    // --- RFB Handshake ---
    // Read server version
    let mut ver_buf = [0u8; 12];
    stream.read_exact(&mut ver_buf)?;
    // Send our version (3.8)
    stream.write_all(b"RFB 003.008\n")?;

    // Security handshake
    let mut num_sec = [0u8; 1];
    stream.read_exact(&mut num_sec)?;
    let n = num_sec[0] as usize;
    if n == 0 {
        // Read failure reason
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf)?;
        let len = (u32::from_be_bytes(len_buf) as usize).min(4096); // Cap auth failure reason
        let mut reason = vec![0u8; len];
        stream.read_exact(&mut reason)?;
        return Err(format!("VNC auth failed: {}", String::from_utf8_lossy(&reason)).into());
    }
    let mut sec_types = vec![0u8; n];
    stream.read_exact(&mut sec_types)?;

    // Choose security type: prefer None (1), fall back to VncAuth (2)
    if sec_types.contains(&1) {
        stream.write_all(&[1])?; // None — no auth needed
    } else if sec_types.contains(&2) {
        stream.write_all(&[2])?; // VNC Authentication
                                 // Read 16-byte challenge from server
        let mut challenge = [0u8; 16];
        stream.read_exact(&mut challenge)?;
        // Encrypt challenge with password using DES
        let pwd = password.unwrap_or("");
        let response = vnc_des_encrypt(&challenge, pwd);
        stream.write_all(&response)?;
    } else {
        return Err("VNC server requires unsupported authentication type".into());
    }

    // SecurityResult
    let mut result = [0u8; 4];
    stream.read_exact(&mut result)?;
    if u32::from_be_bytes(result) != 0 {
        return Err("VNC security handshake failed".into());
    }

    // ClientInit — shared flag = 1 (allow other connections)
    stream.write_all(&[1])?;

    // ServerInit
    let mut server_init = [0u8; 24];
    stream.read_exact(&mut server_init)?;
    let width = u16::from_be_bytes([server_init[0], server_init[1]]);
    let height = u16::from_be_bytes([server_init[2], server_init[3]]);
    let _bpp = server_init[4];
    // Read server name
    let name_len = (u32::from_be_bytes([
        server_init[20],
        server_init[21],
        server_init[22],
        server_init[23],
    ]) as usize)
        .min(4096); // Cap server name
    let mut name_buf = vec![0u8; name_len];
    stream.read_exact(&mut name_buf)?;

    // Set our pixel format: 32-bit RGBA
    let mut set_pixel = [0u8; 20];
    set_pixel[0] = 0; // SetPixelFormat message type
                      // padding [1..4]
    set_pixel[4] = 32; // bits-per-pixel
    set_pixel[5] = 24; // depth
    set_pixel[6] = 0; // big-endian flag
    set_pixel[7] = 1; // true-colour flag
                      // R max = 255
    set_pixel[8] = 0;
    set_pixel[9] = 255;
    // G max = 255
    set_pixel[10] = 0;
    set_pixel[11] = 255;
    // B max = 255
    set_pixel[12] = 0;
    set_pixel[13] = 255;
    // R shift = 0, G shift = 8, B shift = 16
    set_pixel[14] = 0; // red shift
    set_pixel[15] = 8; // green shift
    set_pixel[16] = 16; // blue shift
    stream.write_all(&set_pixel)?;

    // Set encodings: Raw(0), CopyRect(1), DesktopSize(-223), ExtendedDesktopSize(-308)
    // SECURITY: We advertise DesktopSize pseudo-encoding so we can handle
    // server-initiated resizes safely instead of desyncing.
    // ExtendedDesktopSize (-308) enables client-requested resolution changes.
    let mut set_enc = Vec::with_capacity(4 + 4 * 4);
    set_enc.extend_from_slice(&[2, 0]); // message type + padding
    set_enc.extend_from_slice(&(4u16).to_be_bytes()); // number of encodings
    set_enc.extend_from_slice(&(0i32).to_be_bytes()); // Raw
    set_enc.extend_from_slice(&(1i32).to_be_bytes()); // CopyRect
    set_enc.extend_from_slice(&(-223i32).to_be_bytes()); // DesktopSize
    set_enc.extend_from_slice(&(-308i32).to_be_bytes()); // ExtendedDesktopSize
    stream.write_all(&set_enc)?;

    // Make dimensions mutable so DesktopSize pseudo-encoding can update them
    let mut width = width;
    let mut height = height;

    // SECURITY: CWE-400 — Rate-limit resize operations to prevent memory exhaustion
    // from rapid DesktopSize pseudo-encoding messages.
    // Use a sliding window: allow up to MAX_RESIZES in any WINDOW period.
    // This handles legitimate bursts (Windows OOBE) while still catching DoS.
    let mut resize_timestamps: std::collections::VecDeque<std::time::Instant> =
        std::collections::VecDeque::new();
    const MAX_RESIZES_PER_WINDOW: usize = 50;
    const RESIZE_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

    // Initialize framebuffer
    // SECURITY: Cap framebuffer dimensions to prevent OOM from malicious server (CWE-400)
    // Max 8192x8192 = 256MB, which is generous for any real display
    const MAX_VNC_DIMENSION: u16 = 8192;
    if width == 0 || height == 0 || width > MAX_VNC_DIMENSION || height > MAX_VNC_DIMENSION {
        return Err(format!(
            "VNC framebuffer dimensions out of range: {}x{} (max {}x{})",
            width, height, MAX_VNC_DIMENSION, MAX_VNC_DIMENSION
        )
        .into());
    }
    // SECURITY: CWE-662 — Abort on poisoned mutex rather than using corrupted state.
    {
        let mut state = fb
            .inner
            .lock()
            .map_err(|_| "VNC state mutex poisoned during initialization — aborting connection")?;
        state.width = width;
        state.height = height;
        state.pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        state.pixel_generation = state.pixel_generation.wrapping_add(1);
        state.connected = true;
        state.error = None;
    }

    // Request full framebuffer
    send_fb_update_request(&mut stream, false, 0, 0, width, height)?;

    // Main loop
    loop {
        // Check if we should disconnect
        // SECURITY: CWE-662 — Treat poisoned mutex as fatal, disconnect immediately.
        {
            let state = fb
                .inner
                .lock()
                .map_err(|_| "VNC state mutex poisoned — disconnecting to avoid corrupted state")?;
            if !state.connected {
                break;
            }
        }

        // Send pending input events
        // SECURITY: CWE-662 — Abort on poisoned mutex.
        {
            let mut state = fb
                .inner
                .lock()
                .map_err(|_| "VNC state mutex poisoned while reading input events")?;
            let keys: Vec<KeyEvent> = state.pending_keys.drain(..).collect();
            let mouse = state.pending_mouse.take();
            let needs_refresh = state.needs_refresh;
            if needs_refresh {
                state.needs_refresh = false;
            }
            let pending_resize = state.pending_resize.take();
            drop(state);

            for key in keys {
                send_key_event(&mut stream, key.down, key.key)?;
            }
            if let Some(m) = mouse {
                send_pointer_event(&mut stream, m.x, m.y, m.buttons)?;
            }
            if needs_refresh {
                send_fb_update_request(&mut stream, false, 0, 0, width, height)?;
            }

            // Send SetDesktopSize if a resolution change was requested
            if let Some((new_w, new_h)) = pending_resize {
                send_set_desktop_size(&mut stream, new_w, new_h)?;
            }
        }

        // Request incremental update
        send_fb_update_request(&mut stream, true, 0, 0, width, height)?;

        // Read server messages
        let mut msg_type = [0u8; 1];
        match stream.read_exact(&mut msg_type) {
            Ok(()) => {},
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            },
            Err(e) => return Err(e.into()),
        }

        match msg_type[0] {
            0 => {
                // FramebufferUpdate
                let mut header = [0u8; 3];
                stream.read_exact(&mut header)?;
                let num_rects = u16::from_be_bytes([header[1], header[2]]);

                // SECURITY: Cap rectangle count to prevent OOM DoS from malicious VNC server (CWE-400).
                // 65535 rects × 64MB each = 4TB — a trivial DoS vector.
                const MAX_RECTS_PER_UPDATE: u16 = 4096;
                if num_rects > MAX_RECTS_PER_UPDATE {
                    return Err(format!(
                        "VNC server sent {} rectangles (max {}), possible DoS",
                        num_rects, MAX_RECTS_PER_UPDATE
                    )
                    .into());
                }

                // Track cumulative allocation across all rectangles in this update
                let mut cumulative_alloc: usize = 0;
                const MAX_CUMULATIVE_ALLOC: usize = 256 * 1024 * 1024; // 256MB per update

                for _ in 0..num_rects {
                    let mut rect = [0u8; 12];
                    stream.read_exact(&mut rect)?;
                    let rx = u16::from_be_bytes([rect[0], rect[1]]);
                    let ry = u16::from_be_bytes([rect[2], rect[3]]);
                    let rw = u16::from_be_bytes([rect[4], rect[5]]);
                    let rh = u16::from_be_bytes([rect[6], rect[7]]);
                    let encoding = i32::from_be_bytes([rect[8], rect[9], rect[10], rect[11]]);

                    match encoding {
                        0 => {
                            // Raw encoding
                            // SECURITY: Validate rectangle bounds against framebuffer dimensions (CWE-787)
                            // A malicious server can send rectangles outside the framebuffer
                            if rw == 0 || rh == 0 {
                                continue; // Skip zero-size rectangles
                            }
                            if (rx as u32) + (rw as u32) > width as u32
                                || (ry as u32) + (rh as u32) > height as u32
                            {
                                // Server sent a rectangle larger than current framebuffer.
                                // This happens when the guest resizes before a DesktopSize
                                // message arrives. Treat a full-screen rect at (0,0) as an
                                // implicit resize rather than disconnecting.
                                if rx == 0
                                    && ry == 0
                                    && rw > 0
                                    && rh > 0
                                    && rw <= MAX_VNC_DIMENSION
                                    && rh <= MAX_VNC_DIMENSION
                                {
                                    tracing::info!(
                                        "VNC implicit resize: {}x{} -> {}x{}",
                                        width,
                                        height,
                                        rw,
                                        rh
                                    );
                                    width = rw;
                                    height = rh;
                                    let mut state = fb.inner.lock().map_err(|_| {
                                        "VNC state mutex poisoned during implicit resize"
                                    })?;
                                    state.width = rw;
                                    state.height = rh;
                                    state.pixels = vec![0u8; (rw as usize) * (rh as usize) * 4];
                                    state.pixel_generation = state.pixel_generation.wrapping_add(1);
                                    // Fall through to read the pixel data below
                                } else {
                                    return Err(format!(
                                        "VNC rectangle ({},{} {}x{}) exceeds framebuffer ({}x{})",
                                        rx, ry, rw, rh, width, height
                                    )
                                    .into());
                                }
                            }

                            // SECURITY: CWE-190 — Use checked arithmetic to prevent
                            // integer overflow on pixel buffer size calculation.
                            let pixel_bytes = (rw as usize)
                                .checked_mul(rh as usize)
                                .and_then(|v| v.checked_mul(4))
                                .ok_or_else(|| {
                                    format!("VNC pixel buffer size overflow: {}x{} x 4", rw, rh)
                                })?;
                            // SECURITY: Cap per-rectangle and cumulative allocation (CWE-400)
                            const MAX_PIXEL_ALLOC: usize = 64 * 1024 * 1024;
                            if pixel_bytes > MAX_PIXEL_ALLOC {
                                return Err(format!(
                                    "VNC update too large: {}x{} ({} bytes), max {} bytes",
                                    rw, rh, pixel_bytes, MAX_PIXEL_ALLOC
                                )
                                .into());
                            }
                            cumulative_alloc = cumulative_alloc.saturating_add(pixel_bytes);
                            if cumulative_alloc > MAX_CUMULATIVE_ALLOC {
                                return Err(format!(
                                    "VNC cumulative allocation {} exceeds limit {} bytes",
                                    cumulative_alloc, MAX_CUMULATIVE_ALLOC
                                )
                                .into());
                            }
                            let mut pixel_data = vec![0u8; pixel_bytes];
                            stream.read_exact(&mut pixel_data)?;

                            // Write into framebuffer
                            // SECURITY: CWE-662 — Abort on poisoned mutex.
                            let mut state = fb
                                .inner
                                .lock()
                                .map_err(|_| "VNC state mutex poisoned during framebuffer write")?;
                            let fb_w = state.width as usize;
                            for row in 0..(rh as usize) {
                                // SECURITY: CWE-190 — Checked arithmetic on all
                                // offset calculations to prevent overflow-induced
                                // out-of-bounds writes.
                                let src_off = match row
                                    .checked_mul(rw as usize)
                                    .and_then(|v| v.checked_mul(4))
                                {
                                    Some(v) => v,
                                    None => break,
                                };
                                let dst_y = (ry as usize) + row;
                                let dst_off = match dst_y
                                    .checked_mul(fb_w)
                                    .and_then(|v| v.checked_add(rx as usize))
                                    .and_then(|v| v.checked_mul(4))
                                {
                                    Some(v) => v,
                                    None => break,
                                };
                                let row_bytes = (rw as usize) * 4; // safe: rw <= 8192
                                if dst_off + row_bytes <= state.pixels.len()
                                    && src_off + row_bytes <= pixel_data.len()
                                {
                                    // Convert RGBX -> RGBA
                                    for px in 0..(rw as usize) {
                                        let si = src_off + px * 4;
                                        let di = dst_off + px * 4;
                                        state.pixels[di] = pixel_data[si]; // R
                                        state.pixels[di + 1] = pixel_data[si + 1]; // G
                                        state.pixels[di + 2] = pixel_data[si + 2]; // B
                                        state.pixels[di + 3] = 255; // A
                                    }
                                }
                            }
                            state.pixel_generation = state.pixel_generation.wrapping_add(1);
                        },
                        1 => {
                            // CopyRect encoding
                            let mut src = [0u8; 4];
                            stream.read_exact(&mut src)?;
                            let sx = u16::from_be_bytes([src[0], src[1]]);
                            let sy = u16::from_be_bytes([src[2], src[3]]);

                            // SECURITY: CWE-787 — Validate both source and destination
                            // rectangles are within framebuffer bounds. A malicious server
                            // could send out-of-bounds source coords to leak memory or
                            // corrupt the framebuffer with overlapping OOB copies.
                            if rw == 0 || rh == 0 {
                                continue;
                            }
                            if (sx as u32) + (rw as u32) > width as u32
                                || (sy as u32) + (rh as u32) > height as u32
                            {
                                return Err(format!(
                                    "VNC CopyRect source ({},{} {}x{}) exceeds framebuffer ({}x{})",
                                    sx, sy, rw, rh, width, height
                                )
                                .into());
                            }
                            if (rx as u32) + (rw as u32) > width as u32
                                || (ry as u32) + (rh as u32) > height as u32
                            {
                                return Err(format!(
                                    "VNC CopyRect dest ({},{} {}x{}) exceeds framebuffer ({}x{})",
                                    rx, ry, rw, rh, width, height
                                )
                                .into());
                            }

                            let mut state = fb
                                .inner
                                .lock()
                                .map_err(|_| "VNC state mutex poisoned during CopyRect")?;
                            let fb_w = state.width as usize;
                            // Copy row-by-row into a temp buffer first to handle
                            // overlapping source/dest regions safely (CWE-787).
                            let row_bytes = (rw as usize) * 4;
                            let mut temp = vec![0u8; row_bytes * (rh as usize)];
                            for row in 0..(rh as usize) {
                                let src_y = (sy as usize) + row;
                                let src_off = (src_y * fb_w + sx as usize) * 4;
                                if src_off + row_bytes <= state.pixels.len() {
                                    let t_off = row * row_bytes;
                                    temp[t_off..t_off + row_bytes].copy_from_slice(
                                        &state.pixels[src_off..src_off + row_bytes],
                                    );
                                }
                            }
                            for row in 0..(rh as usize) {
                                let dst_y = (ry as usize) + row;
                                let dst_off = (dst_y * fb_w + rx as usize) * 4;
                                if dst_off + row_bytes <= state.pixels.len() {
                                    let t_off = row * row_bytes;
                                    state.pixels[dst_off..dst_off + row_bytes]
                                        .copy_from_slice(&temp[t_off..t_off + row_bytes]);
                                }
                            }
                            state.pixel_generation = state.pixel_generation.wrapping_add(1);
                        },
                        -308 => {
                            // ExtendedDesktopSize pseudo-encoding.
                            // Response to our SetDesktopSize or server-initiated resize.
                            // Format: 1 byte num-screens, 3 bytes padding, then per-screen:
                            // 4 bytes id, 2 bytes x, 2 bytes y, 2 bytes w, 2 bytes h, 4 bytes flags
                            let mut header = [0u8; 4];
                            stream.read_exact(&mut header)?;
                            let num_screens = header[0] as usize;
                            // Read screen data (16 bytes per screen)
                            let screen_data_len = num_screens.min(16) * 16;
                            let mut screen_data = vec![0u8; screen_data_len];
                            stream.read_exact(&mut screen_data)?;

                            // rx encodes the reason: 0 = server, 1 = client request success
                            // rw/rh contain the new framebuffer dimensions
                            if rw > 0
                                && rh > 0
                                && rw <= MAX_VNC_DIMENSION
                                && rh <= MAX_VNC_DIMENSION
                            {
                                let now = std::time::Instant::now();
                                // Evict timestamps outside the window
                                while resize_timestamps
                                    .front()
                                    .map_or(false, |t| now.duration_since(*t) > RESIZE_WINDOW)
                                {
                                    resize_timestamps.pop_front();
                                }
                                if resize_timestamps.len() >= MAX_RESIZES_PER_WINDOW {
                                    // Too many resizes in window — skip this one silently
                                    // rather than disconnecting (legitimate burst)
                                    tracing::warn!(
                                        "VNC ExtendedDesktopSize: rate-limited ({} resizes in {:?}), skipping {}x{}",
                                        resize_timestamps.len(), RESIZE_WINDOW, rw, rh
                                    );
                                } else {
                                    resize_timestamps.push_back(now);

                                    width = rw;
                                    height = rh;

                                    let mut state = fb.inner.lock().map_err(|_| {
                                        "VNC state mutex poisoned during ExtendedDesktopSize resize"
                                    })?;
                                    state.width = rw;
                                    state.height = rh;
                                    state.pixels = vec![0u8; (rw as usize) * (rh as usize) * 4];
                                    state.pixel_generation = state.pixel_generation.wrapping_add(1);

                                    tracing::info!(
                                        "VNC ExtendedDesktopSize: framebuffer resized to {}x{}",
                                        rw,
                                        rh,
                                    );

                                    // Request full framebuffer refresh after resize
                                    send_fb_update_request(&mut stream, false, 0, 0, rw, rh)?;
                                }
                            }
                        },
                        -223 => {
                            // DesktopSize pseudo-encoding — server is resizing the framebuffer.
                            // The new dimensions are in the rectangle's width/height fields (rw, rh).
                            // No pixel data follows this encoding.
                            //
                            // SECURITY: CWE-400 — Validate new dimensions against caps and
                            // rate-limit resizes to prevent memory exhaustion from a malicious
                            // server sending rapid resize messages.
                            if rw == 0
                                || rh == 0
                                || rw > MAX_VNC_DIMENSION
                                || rh > MAX_VNC_DIMENSION
                            {
                                return Err(format!(
                                    "VNC DesktopSize dimensions out of range: {}x{} (max {}x{})",
                                    rw, rh, MAX_VNC_DIMENSION, MAX_VNC_DIMENSION
                                )
                                .into());
                            }

                            let now = std::time::Instant::now();
                            while resize_timestamps
                                .front()
                                .map_or(false, |t| now.duration_since(*t) > RESIZE_WINDOW)
                            {
                                resize_timestamps.pop_front();
                            }
                            if resize_timestamps.len() >= MAX_RESIZES_PER_WINDOW {
                                tracing::warn!(
                                    "VNC DesktopSize: rate-limited, skipping {}x{}",
                                    rw,
                                    rh
                                );
                            } else {
                                resize_timestamps.push_back(now);

                                width = rw;
                                height = rh;

                                let mut state = fb.inner.lock().map_err(|_| {
                                    "VNC state mutex poisoned during DesktopSize resize"
                                })?;
                                state.width = rw;
                                state.height = rh;
                                state.pixels = vec![0u8; (rw as usize) * (rh as usize) * 4];
                                state.pixel_generation = state.pixel_generation.wrapping_add(1);

                                tracing::info!(
                                    "VNC DesktopSize: framebuffer resized to {}x{}",
                                    rw,
                                    rh,
                                );

                                // Request full framebuffer refresh after resize
                                send_fb_update_request(&mut stream, false, 0, 0, rw, rh)?;
                            }
                        },
                        _ => {
                            // SECURITY: CWE-20 — Unknown encodings have unknown payload
                            // sizes. We cannot skip them without knowing how many bytes
                            // to drain, so continuing would desync the protocol stream.
                            // Subsequent reads would interpret data as message types,
                            // leading to arbitrary behavior. Terminate the connection.
                            return Err(format!(
                                "VNC server sent unsupported encoding {} — \
                                 disconnecting to prevent protocol desync",
                                encoding
                            )
                            .into());
                        },
                    }
                }
            },
            1 => {
                // SetColourMapEntries — skip
                let mut header = [0u8; 5];
                stream.read_exact(&mut header)?;
                let num = (u16::from_be_bytes([header[3], header[4]]) as usize).min(4096);
                let mut skip = vec![0u8; num * 6];
                stream.read_exact(&mut skip)?;
            },
            2 => {
                // Bell — ignore
            },
            3 => {
                // ServerCutText
                let mut header = [0u8; 7];
                stream.read_exact(&mut header)?;
                let len = (u32::from_be_bytes([header[3], header[4], header[5], header[6]])
                    as usize)
                    .min(1024 * 1024); // Cap clipboard to 1MB
                let mut text = vec![0u8; len];
                stream.read_exact(&mut text)?;
            },
            _ => {
                // SECURITY: CWE-20 — Unknown server message type means the protocol
                // stream is desynced (possibly due to a malicious server). Continuing
                // would interpret arbitrary data as protocol messages.
                return Err(format!(
                    "VNC server sent unknown message type {} — \
                     disconnecting to prevent protocol desync",
                    msg_type[0]
                )
                .into());
            },
        }
    }

    Ok(())
}

fn send_fb_update_request(
    stream: &mut TcpStream,
    incremental: bool,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
) -> std::io::Result<()> {
    let mut msg = [0u8; 10];
    msg[0] = 3; // FramebufferUpdateRequest
    msg[1] = if incremental { 1 } else { 0 };
    msg[2..4].copy_from_slice(&x.to_be_bytes());
    msg[4..6].copy_from_slice(&y.to_be_bytes());
    msg[6..8].copy_from_slice(&w.to_be_bytes());
    msg[8..10].copy_from_slice(&h.to_be_bytes());
    stream.write_all(&msg)
}

fn send_key_event(stream: &mut TcpStream, down: bool, key: u32) -> std::io::Result<()> {
    let mut msg = [0u8; 8];
    msg[0] = 4; // KeyEvent
    msg[1] = if down { 1 } else { 0 };
    msg[4..8].copy_from_slice(&key.to_be_bytes());
    stream.write_all(&msg)
}

/// Send a SetDesktopSize message (RFB message type 251) to request a resolution change.
/// Format: type(1) + padding(1) + width(2) + height(2) + num-screens(1) + padding(1)
/// + per-screen: id(4) + x(2) + y(2) + w(2) + h(2) + flags(4) = 16 bytes
fn send_set_desktop_size(stream: &mut TcpStream, width: u16, height: u16) -> std::io::Result<()> {
    let mut msg = Vec::with_capacity(8 + 16);
    msg.push(251); // SetDesktopSize message type
    msg.push(0); // padding
    msg.extend_from_slice(&width.to_be_bytes());
    msg.extend_from_slice(&height.to_be_bytes());
    msg.push(1); // number of screens
    msg.push(0); // padding
                 // Screen 0
    msg.extend_from_slice(&0u32.to_be_bytes()); // screen id
    msg.extend_from_slice(&0u16.to_be_bytes()); // x-position
    msg.extend_from_slice(&0u16.to_be_bytes()); // y-position
    msg.extend_from_slice(&width.to_be_bytes()); // width
    msg.extend_from_slice(&height.to_be_bytes()); // height
    msg.extend_from_slice(&0u32.to_be_bytes()); // flags
    stream.write_all(&msg)
}

fn send_pointer_event(stream: &mut TcpStream, x: u16, y: u16, buttons: u8) -> std::io::Result<()> {
    let mut msg = [0u8; 6];
    msg[0] = 5; // PointerEvent
    msg[1] = buttons;
    msg[2..4].copy_from_slice(&x.to_be_bytes());
    msg[4..6].copy_from_slice(&y.to_be_bytes());
    stream.write_all(&msg)
}

// ============ egui Key → X11 Keysym Mapping ============

/// Convert an egui key to X11 keysym for VNC.
pub fn egui_key_to_keysym(key: egui::Key, modifiers: &egui::Modifiers) -> Option<u32> {
    use egui::Key::*;
    Some(match key {
        Escape => 0xff1b,
        Tab => 0xff09,
        Backspace => 0xff08,
        Enter => 0xff0d,
        Space => 0x0020,
        Insert => 0xff63,
        Delete => 0xffff,
        Home => 0xff50,
        End => 0xff57,
        PageUp => 0xff55,
        PageDown => 0xff56,
        ArrowLeft => 0xff51,
        ArrowUp => 0xff52,
        ArrowRight => 0xff53,
        ArrowDown => 0xff54,
        F1 => 0xffbe,
        F2 => 0xffbf,
        F3 => 0xffc0,
        F4 => 0xffc1,
        F5 => 0xffc2,
        F6 => 0xffc3,
        F7 => 0xffc4,
        F8 => 0xffc5,
        F9 => 0xffc6,
        F10 => 0xffc7,
        F11 => 0xffc8,
        F12 => 0xffc9,
        A => {
            if modifiers.shift {
                0x0041
            } else {
                0x0061
            }
        },
        B => {
            if modifiers.shift {
                0x0042
            } else {
                0x0062
            }
        },
        C => {
            if modifiers.shift {
                0x0043
            } else {
                0x0063
            }
        },
        D => {
            if modifiers.shift {
                0x0044
            } else {
                0x0064
            }
        },
        E => {
            if modifiers.shift {
                0x0045
            } else {
                0x0065
            }
        },
        F => {
            if modifiers.shift {
                0x0046
            } else {
                0x0066
            }
        },
        G => {
            if modifiers.shift {
                0x0047
            } else {
                0x0067
            }
        },
        H => {
            if modifiers.shift {
                0x0048
            } else {
                0x0068
            }
        },
        I => {
            if modifiers.shift {
                0x0049
            } else {
                0x0069
            }
        },
        J => {
            if modifiers.shift {
                0x004a
            } else {
                0x006a
            }
        },
        K => {
            if modifiers.shift {
                0x004b
            } else {
                0x006b
            }
        },
        L => {
            if modifiers.shift {
                0x004c
            } else {
                0x006c
            }
        },
        M => {
            if modifiers.shift {
                0x004d
            } else {
                0x006d
            }
        },
        N => {
            if modifiers.shift {
                0x004e
            } else {
                0x006e
            }
        },
        O => {
            if modifiers.shift {
                0x004f
            } else {
                0x006f
            }
        },
        P => {
            if modifiers.shift {
                0x0050
            } else {
                0x0070
            }
        },
        Q => {
            if modifiers.shift {
                0x0051
            } else {
                0x0071
            }
        },
        R => {
            if modifiers.shift {
                0x0052
            } else {
                0x0072
            }
        },
        S => {
            if modifiers.shift {
                0x0053
            } else {
                0x0073
            }
        },
        T => {
            if modifiers.shift {
                0x0054
            } else {
                0x0074
            }
        },
        U => {
            if modifiers.shift {
                0x0055
            } else {
                0x0075
            }
        },
        V => {
            if modifiers.shift {
                0x0056
            } else {
                0x0076
            }
        },
        W => {
            if modifiers.shift {
                0x0057
            } else {
                0x0077
            }
        },
        X => {
            if modifiers.shift {
                0x0058
            } else {
                0x0078
            }
        },
        Y => {
            if modifiers.shift {
                0x0059
            } else {
                0x0079
            }
        },
        Z => {
            if modifiers.shift {
                0x005a
            } else {
                0x007a
            }
        },
        Num0 => {
            if modifiers.shift {
                0x0029
            } else {
                0x0030
            }
        }, // ) or 0
        Num1 => {
            if modifiers.shift {
                0x0021
            } else {
                0x0031
            }
        }, // ! or 1
        Num2 => {
            if modifiers.shift {
                0x0040
            } else {
                0x0032
            }
        }, // @ or 2
        Num3 => {
            if modifiers.shift {
                0x0023
            } else {
                0x0033
            }
        }, // # or 3
        Num4 => {
            if modifiers.shift {
                0x0024
            } else {
                0x0034
            }
        }, // $ or 4
        Num5 => {
            if modifiers.shift {
                0x0025
            } else {
                0x0035
            }
        }, // % or 5
        Num6 => {
            if modifiers.shift {
                0x005e
            } else {
                0x0036
            }
        }, // ^ or 6
        Num7 => {
            if modifiers.shift {
                0x0026
            } else {
                0x0037
            }
        }, // & or 7
        Num8 => {
            if modifiers.shift {
                0x002a
            } else {
                0x0038
            }
        }, // * or 8
        Num9 => {
            if modifiers.shift {
                0x0028
            } else {
                0x0039
            }
        }, // ( or 9
        Minus => {
            if modifiers.shift {
                0x005f
            } else {
                0x002d
            }
        },
        Plus => 0x002b,
        _ => return None,
    })
}

/// Map egui modifier keys to their keysyms.
#[allow(dead_code)]
pub fn modifier_keysyms() -> Vec<(egui::Modifiers, u32)> {
    vec![
        (
            egui::Modifiers {
                shift: true,
                ..Default::default()
            },
            0xffe1,
        ), // Shift_L
        (
            egui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
            0xffe3,
        ), // Control_L
        (
            egui::Modifiers {
                alt: true,
                ..Default::default()
            },
            0xffe9,
        ), // Alt_L
    ]
}
