//! SPICE client for embedding in egui.
//!
//! Uses libspice-client-glib via C FFI for full SPICE protocol support:
//! display, input, clipboard, cursor, audio, USB redirection.
//! Produces RGBA pixels identical to the VNC path so console.rs renders
//! either backend transparently.
//!
//! Architecture:
//!   GLib main loop (background thread)
//!     ├─ SpiceSession
//!     │   ├─ SpiceDisplayChannel → pixel buffer + dirty rects
//!     │   ├─ SpiceInputsChannel → keyboard/mouse (via input pump)
//!     │   ├─ SpiceMainChannel   → clipboard, agent, monitor config
//!     │   ├─ SpiceCursorChannel → hardware cursor shapes
//!     │   └─ SpiceAudio         → playback + recording (auto)
//!     └─ Shared SpiceState (Arc<Mutex<>>) ← read by egui on paint
//!
//! Input pump: a GLib idle source runs every ~16ms on the GLib thread,
//! drains pending_keys/pending_mouse from shared state, and calls the
//! SPICE inputs channel API (which must be called from the GLib thread).

use std::sync::{Arc, Mutex};

// ═══════════════════════════════════════════════════════════════════════
// Shared state
// ═══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct SpiceFramebuffer {
    inner: Arc<Mutex<SpiceState>>,
}

pub struct SpiceState {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>, // RGBA, row-major
    pub connected: bool,
    pub error: Option<String>,
    pub pending_keys: Vec<KeyEvent>,
    pub pending_mouse: Option<MouseEvent>,
    #[allow(dead_code)]
    pub needs_refresh: bool,
    pub pending_resize: Option<(u16, u16)>,
    pub pixel_generation: u64,
    // SPICE extras
    pub clipboard_from_guest: Option<String>,
    pub clipboard_to_guest: Option<String>,
    pub agent_connected: bool,
}

#[derive(Clone)]
pub struct KeyEvent {
    pub down: bool,
    pub scancode: u32,
}

#[derive(Clone)]
pub struct MouseEvent {
    pub x: u16,
    pub y: u16,
    pub buttons: u8,
}

impl SpiceFramebuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SpiceState {
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
                clipboard_from_guest: None,
                clipboard_to_guest: None,
                agent_connected: false,
            })),
        }
    }

    pub fn get_state(&self) -> Option<std::sync::MutexGuard<'_, SpiceState>> {
        self.inner.lock().ok()
    }

    /// Send a key event (X11 keysym — converted to scancode internally).
    pub fn send_key(&self, down: bool, keysym: u32) {
        const MAX_PENDING: usize = 1024;
        if let Ok(mut state) = self.inner.lock() {
            if state.pending_keys.len() < MAX_PENDING {
                let scancode = keysym_to_scancode(keysym);
                state.pending_keys.push(KeyEvent { down, scancode });
            }
        }
    }

    pub fn send_mouse(&self, x: u16, y: u16, buttons: u8) {
        if let Ok(mut state) = self.inner.lock() {
            state.pending_mouse = Some(MouseEvent { x, y, buttons });
        }
    }

    pub fn request_resolution(&self, width: u16, height: u16) {
        let w = width.clamp(320, 8192);
        let h = height.clamp(200, 8192);
        if let Ok(mut state) = self.inner.lock() {
            if state.width != w || state.height != h {
                state.pending_resize = Some((w, h));
            }
        }
    }

    /// Set clipboard text to send to guest.
    #[allow(dead_code)]
    pub fn set_clipboard(&self, text: &str) {
        if let Ok(mut state) = self.inner.lock() {
            state.clipboard_to_guest = Some(text.to_string());
        }
    }

    /// Take clipboard text received from guest (consumes it).
    #[allow(dead_code)]
    pub fn take_guest_clipboard(&self) -> Option<String> {
        if let Ok(mut state) = self.inner.lock() {
            state.clipboard_from_guest.take()
        } else {
            None
        }
    }

    /// Connect to a SPICE server on a background thread.
    pub fn connect(&self, host: &str, port: u16) {
        if host != "127.0.0.1" && host != "localhost" && host != "::1" {
            tracing::error!("SPICE: refusing non-localhost connection to {}", host);
            if let Ok(mut state) = self.inner.lock() {
                state.error = Some(format!("SPICE: non-localhost refused ({})", host));
            }
            return;
        }

        let inner = self.inner.clone();
        let host = host.to_string();

        if let Err(e) = std::thread::Builder::new()
            .name("spice-client".into())
            .spawn(move || {
                spice_thread(inner, &host, port);
            })
        {
            tracing::error!("Failed to spawn SPICE thread: {}", e);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Raw C FFI bindings
// ═══════════════════════════════════════════════════════════════════════

mod ffi {
    #![allow(non_camel_case_types, dead_code)]
    use std::os::raw::{c_char, c_int, c_uint, c_void};

    pub type gboolean = c_int;
    pub type guint = c_uint;
    pub type guint32 = u32;
    pub type gint = c_int;
    pub type gsize = usize;
    pub type gpointer = *mut c_void;
    pub type gconstpointer = *const c_void;
    pub type GMainLoop = c_void;
    pub type GMainContext = c_void;
    pub type GObject = c_void;
    pub type GType = usize;
    pub type GCallback = Option<unsafe extern "C" fn()>;
    pub type GClosureNotify = Option<unsafe extern "C" fn(data: gpointer, closure: gpointer)>;
    pub type GSourceFunc = Option<unsafe extern "C" fn(user_data: gpointer) -> gboolean>;

    // GValue for non-variadic property access
    #[repr(C)]
    pub struct GValue {
        pub g_type: GType,
        pub data: [u64; 2],
    }
    impl GValue {
        pub fn zeroed() -> Self {
            Self {
                g_type: 0,
                data: [0; 2],
            }
        }
    }

    // G_TYPE_STRING = G_TYPE_MAKE_FUNDAMENTAL(16) = 16 << 2 = 64
    pub const G_TYPE_STRING: GType = 64;

    /// Mirror of SpiceDisplayPrimary from channel-display.h
    #[repr(C)]
    pub struct SpiceDisplayPrimary {
        pub format: c_int,
        pub width: c_int,
        pub height: c_int,
        pub stride: c_int,
        pub shmid: c_int,
        pub data: *mut u8,
        pub marked: c_int,
    }

    pub type SpiceSession = c_void;
    pub type SpiceChannel = c_void;
    pub type SpiceDisplayChannel = c_void;
    pub type SpiceInputsChannel = c_void;
    pub type SpiceMainChannel = c_void;
    pub type SpiceCursorChannel = c_void;
    pub type SpiceAudio = c_void;

    // SPICE clipboard types
    pub const VD_AGENT_CLIPBOARD_UTF8_TEXT: guint32 = 1;
    pub const VD_AGENT_CLIPBOARD_SELECTION_CLIPBOARD: guint = 0;

    // SPICE mouse buttons
    pub const SPICE_MOUSE_BUTTON_LEFT: c_int = 1;
    pub const SPICE_MOUSE_BUTTON_MIDDLE: c_int = 2;
    pub const SPICE_MOUSE_BUTTON_RIGHT: c_int = 3;
    pub const SPICE_MOUSE_BUTTON_UP: c_int = 4;
    pub const SPICE_MOUSE_BUTTON_DOWN: c_int = 5;

    pub const SPICE_MOUSE_BUTTON_MASK_LEFT: c_int = 1 << 0;
    pub const SPICE_MOUSE_BUTTON_MASK_MIDDLE: c_int = 1 << 1;
    pub const SPICE_MOUSE_BUTTON_MASK_RIGHT: c_int = 1 << 2;

    // SpiceChannelEvent — channel-event signal values
    pub const SPICE_CHANNEL_OPENED: c_int = 10;
    pub const SPICE_CHANNEL_CLOSED: c_int = 12;
    pub const SPICE_CHANNEL_ERROR_CONNECT: c_int = 20;
    pub const SPICE_CHANNEL_ERROR_TLS: c_int = 21;
    pub const SPICE_CHANNEL_ERROR_LINK: c_int = 22;
    pub const SPICE_CHANNEL_ERROR_AUTH: c_int = 23;
    pub const SPICE_CHANNEL_ERROR_IO: c_int = 24;

    extern "C" {
        // GLib main loop
        pub fn g_main_loop_new(ctx: *mut GMainContext, is_running: gboolean) -> *mut GMainLoop;
        pub fn g_main_loop_run(loop_: *mut GMainLoop);
        pub fn g_main_loop_quit(loop_: *mut GMainLoop);
        pub fn g_main_loop_unref(loop_: *mut GMainLoop);
        pub fn g_main_context_new() -> *mut GMainContext;
        pub fn g_main_context_unref(ctx: *mut GMainContext);
        pub fn g_main_context_push_thread_default(ctx: *mut GMainContext);
        pub fn g_main_context_pop_thread_default(ctx: *mut GMainContext);

        // GLib idle/timeout sources
        pub fn g_timeout_add_full(
            priority: c_int,
            interval: guint,
            function: GSourceFunc,
            data: gpointer,
            notify: GClosureNotify,
        ) -> guint;

        // GValue — non-variadic property access (avoids Rust variadic FFI issues)
        pub fn g_value_init(value: *mut GValue, g_type: GType) -> *mut GValue;
        pub fn g_value_set_string(value: *mut GValue, v_string: *const c_char);
        pub fn g_value_unset(value: *mut GValue);

        // GObject
        pub fn g_object_new(object_type: GType, first: *const c_char, ...) -> *mut GObject;
        pub fn g_object_unref(object: gpointer);
        pub fn g_object_set_property(
            object: *mut GObject,
            property_name: *const c_char,
            value: *const GValue,
        );
        pub fn g_signal_connect_data(
            instance: gpointer,
            signal: *const c_char,
            handler: GCallback,
            data: gpointer,
            destroy: GClosureNotify,
            flags: c_uint,
        ) -> u64;
        pub fn g_type_check_instance_is_a(instance: gpointer, iface_type: GType) -> gboolean;
        pub fn g_free(mem: gpointer);

        // SPICE Session
        pub fn spice_session_get_type() -> GType;
        pub fn spice_session_connect(session: *mut SpiceSession) -> gboolean;
        pub fn spice_session_disconnect(session: *mut SpiceSession);

        // SPICE Channel base
        pub fn spice_channel_connect(channel: *mut SpiceChannel) -> gboolean;

        // SPICE Display Channel
        pub fn spice_display_channel_get_type() -> GType;
        pub fn spice_display_channel_get_primary(
            channel: *mut SpiceChannel,
            surface_id: guint32,
            primary: *mut SpiceDisplayPrimary,
        ) -> gboolean;

        // SPICE USB Redirect Channel
        pub fn spice_usbredir_channel_get_type() -> GType;

        // SPICE Inputs Channel
        pub fn spice_inputs_channel_get_type() -> GType;
        pub fn spice_inputs_channel_key_press(ch: *mut SpiceInputsChannel, scancode: guint);
        pub fn spice_inputs_channel_key_release(ch: *mut SpiceInputsChannel, scancode: guint);
        pub fn spice_inputs_channel_button_press(
            ch: *mut SpiceInputsChannel,
            button: c_int,
            state: c_int,
        );
        pub fn spice_inputs_channel_button_release(
            ch: *mut SpiceInputsChannel,
            button: c_int,
            state: c_int,
        );
        pub fn spice_inputs_channel_position(
            ch: *mut SpiceInputsChannel,
            x: c_int,
            y: c_int,
            display: c_int,
            state: c_int,
        );

        // SPICE Main Channel
        pub fn spice_main_channel_get_type() -> GType;
        pub fn spice_main_set_display(
            ch: *mut SpiceMainChannel,
            id: c_int,
            x: c_int,
            y: c_int,
            w: c_int,
            h: c_int,
        );
        pub fn spice_main_update_display(
            ch: *mut SpiceMainChannel,
            id: c_int,
            x: c_int,
            y: c_int,
            w: c_int,
            h: c_int,
            update: gboolean,
        );
        pub fn spice_main_set_display_enabled(
            ch: *mut SpiceMainChannel,
            id: c_int,
            enabled: gboolean,
        );
        pub fn spice_main_send_monitor_config(ch: *mut SpiceMainChannel) -> gboolean;
        pub fn spice_main_channel_clipboard_selection_grab(
            ch: *mut SpiceMainChannel,
            selection: guint,
            types: *const guint32,
            ntypes: c_int,
        );
        pub fn spice_main_channel_clipboard_selection_notify(
            ch: *mut SpiceMainChannel,
            selection: guint,
            type_: guint32,
            data: *const u8,
            size: c_int,
        );
        pub fn spice_main_channel_clipboard_selection_release(
            ch: *mut SpiceMainChannel,
            selection: guint,
        );
        pub fn spice_main_channel_clipboard_selection_request(
            ch: *mut SpiceMainChannel,
            selection: guint,
            type_: guint32,
        );

        // SPICE Cursor Channel
        pub fn spice_cursor_channel_get_type() -> GType;

        // SPICE Audio (auto-connects playback/record channels to PulseAudio)
        pub fn spice_audio_get(
            session: *mut SpiceSession,
            context: *mut GMainContext,
        ) -> *mut SpiceAudio;
    }

    /// Safely cast a concrete function pointer to a GCallback (Option<fn()>).
    /// This wraps in Some() so we produce a valid non-null Option function pointer.
    #[inline]
    pub unsafe fn as_gcallback<F>(f: F) -> GCallback
    where
        F: Copy,
    {
        // F must be a function pointer (same size as *const ())
        assert!(std::mem::size_of::<F>() == std::mem::size_of::<*const ()>());
        Some(std::mem::transmute_copy::<F, unsafe extern "C" fn()>(&f))
    }

    /// Connect a GObject signal. Uses a byte-literal CStr to avoid CString
    /// allocation and the NulError panic that was hitting us.
    pub unsafe fn g_signal_connect(
        instance: gpointer,
        signal: &[u8],
        handler: GCallback,
        data: gpointer,
    ) -> u64 {
        // signal must be a null-terminated byte slice, e.g. b"channel-new\0"
        g_signal_connect_data(
            instance,
            signal.as_ptr() as *const c_char,
            handler,
            data,
            None, // no destroy notify
            0,
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SPICE thread context
// ═══════════════════════════════════════════════════════════════════════

struct SpiceCtx {
    state: Arc<Mutex<SpiceState>>,
    inputs_channel: Option<*mut ffi::SpiceInputsChannel>,
    main_channel: Option<*mut ffi::SpiceMainChannel>,
    display_channel: Option<*mut ffi::SpiceDisplayChannel>,
    #[allow(dead_code)]
    main_loop: *mut ffi::GMainLoop,
    // Cached surface pointer for dirty-rect repaints
    surface_data: *mut u8,
    surface_stride: i32,
    surface_width: u16,
    surface_height: u16,
}

unsafe impl Send for SpiceCtx {}

fn spice_thread(state: Arc<Mutex<SpiceState>>, host: &str, port: u16) {
    unsafe {
        // Use the global default GMainContext (NULL). We're on a dedicated thread
        // so there's no contention. Using a custom context causes mismatches where
        // the session's internal sources don't get processed by our loop.
        let glib_ctx: *mut ffi::GMainContext = std::ptr::null_mut();
        let main_loop = ffi::g_main_loop_new(glib_ctx, 0);

        let session_type = ffi::spice_session_get_type();
        let session = ffi::g_object_new(session_type, std::ptr::null::<std::os::raw::c_char>())
            as *mut ffi::SpiceSession;

        if session.is_null() {
            if let Ok(mut s) = state.lock() {
                s.error = Some("Failed to create SPICE session".into());
            }
            ffi::g_main_loop_unref(main_loop);
            return;
        }

        // Set URI via non-variadic g_object_set_property (avoids Rust varargs UB)
        let uri = format!("spice://{}?port={}", host, port);
        let uri_c = std::ffi::CString::new(uri).unwrap();
        let mut gval = ffi::GValue::zeroed();
        ffi::g_value_init(&mut gval, ffi::G_TYPE_STRING);
        ffi::g_value_set_string(&mut gval, uri_c.as_ptr());
        ffi::g_object_set_property(
            session as *mut ffi::GObject,
            b"uri\0".as_ptr() as *const std::os::raw::c_char,
            &gval,
        );
        ffi::g_value_unset(&mut gval);

        let ctx = Box::new(SpiceCtx {
            state: state.clone(),
            inputs_channel: None,
            main_channel: None,
            display_channel: None,
            main_loop,
            surface_data: std::ptr::null_mut(),
            surface_stride: 0,
            surface_width: 0,
            surface_height: 0,
        });
        let ctx_ptr = Box::into_raw(ctx);

        ffi::g_signal_connect(
            session as ffi::gpointer,
            b"channel-new\0",
            ffi::as_gcallback(on_channel_new as unsafe extern "C" fn(_, _, _)),
            ctx_ptr as ffi::gpointer,
        );
        ffi::g_signal_connect(
            session as ffi::gpointer,
            b"channel-destroy\0",
            ffi::as_gcallback(on_channel_destroy as unsafe extern "C" fn(_, _, _)),
            ctx_ptr as ffi::gpointer,
        );

        let _audio = ffi::spice_audio_get(session, glib_ctx);
        let ok = ffi::spice_session_connect(session);

        if ok == 0 {
            if let Ok(mut s) = state.lock() {
                s.error = Some(format!(
                    "SPICE: failed to initiate connection to {}:{}",
                    host, port
                ));
            }
            let _ = Box::from_raw(ctx_ptr);
            ffi::g_object_unref(session as ffi::gpointer);
            ffi::g_main_loop_unref(main_loop);
            return;
        }

        tracing::info!("SPICE: connection initiated to {}:{}", host, port);

        // Start input pump (runs on GLib thread, drains pending input from shared state)
        ffi::g_timeout_add_full(
            200, // G_PRIORITY_DEFAULT
            16,  // 16ms ≈ 60fps input rate
            Some(input_pump),
            ctx_ptr as ffi::gpointer,
            None, // no destroy notify
        );

        ffi::g_main_loop_run(main_loop);

        // Cleanup
        if let Ok(mut s) = state.lock() {
            s.connected = false;
        }
        ffi::spice_session_disconnect(session);
        ffi::g_object_unref(session as ffi::gpointer);
        ffi::g_main_loop_unref(main_loop);
        let _ = Box::from_raw(ctx_ptr);
        tracing::info!("SPICE: thread exiting");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Input pump — runs on GLib thread every 16ms
// ═══════════════════════════════════════════════════════════════════════

unsafe extern "C" fn input_pump(user_data: ffi::gpointer) -> ffi::gboolean {
    let ctx = &*(user_data as *mut SpiceCtx);

    let Ok(mut state) = ctx.state.lock() else {
        return 1; // keep running
    };

    // Poll for primary surface if we don't have one yet
    if ctx.surface_data.is_null() {
        if let Some(display) = ctx.display_channel {
            let mut primary = std::mem::zeroed::<ffi::SpiceDisplayPrimary>();
            let ok = ffi::spice_display_channel_get_primary(
                display as *mut ffi::SpiceChannel,
                0,
                &mut primary,
            );
            if ok != 0 && !primary.data.is_null() && primary.width > 0 && primary.height > 0 {
                let w = primary.width as u16;
                let h = primary.height as u16;
                let ctx_mut = &mut *(user_data as *mut SpiceCtx);
                ctx_mut.surface_data = primary.data;
                ctx_mut.surface_stride = primary.stride;
                ctx_mut.surface_width = w;
                ctx_mut.surface_height = h;

                let rgba = copy_surface_bgra_to_rgba(primary.data, w, h, primary.stride);
                state.width = w;
                state.height = h;
                state.pixels = rgba;
                state.pixel_generation = state.pixel_generation.wrapping_add(1);
                tracing::info!("SPICE: primary surface polled {}x{}", w, h);
            }
        }
    }

    // Drain keyboard events
    if let Some(inputs) = ctx.inputs_channel {
        let keys: Vec<KeyEvent> = state.pending_keys.drain(..).collect();
        for key in &keys {
            if key.down {
                ffi::spice_inputs_channel_key_press(inputs, key.scancode);
            } else {
                ffi::spice_inputs_channel_key_release(inputs, key.scancode);
            }
        }

        // Drain mouse events
        if let Some(mouse) = state.pending_mouse.take() {
            let mut button_state: std::os::raw::c_int = 0;
            if mouse.buttons & 1 != 0 {
                button_state |= ffi::SPICE_MOUSE_BUTTON_MASK_LEFT;
            }
            if mouse.buttons & 2 != 0 {
                button_state |= ffi::SPICE_MOUSE_BUTTON_MASK_MIDDLE;
            }
            if mouse.buttons & 4 != 0 {
                button_state |= ffi::SPICE_MOUSE_BUTTON_MASK_RIGHT;
            }

            ffi::spice_inputs_channel_position(
                inputs,
                mouse.x as i32,
                mouse.y as i32,
                0,
                button_state,
            );

            // Handle button press/release for clicks
            if mouse.buttons & 1 != 0 {
                ffi::spice_inputs_channel_button_press(
                    inputs,
                    ffi::SPICE_MOUSE_BUTTON_LEFT,
                    button_state,
                );
            }
            if mouse.buttons & 4 != 0 {
                ffi::spice_inputs_channel_button_press(
                    inputs,
                    ffi::SPICE_MOUSE_BUTTON_RIGHT,
                    button_state,
                );
            }
            if mouse.buttons & 2 != 0 {
                ffi::spice_inputs_channel_button_press(
                    inputs,
                    ffi::SPICE_MOUSE_BUTTON_MIDDLE,
                    button_state,
                );
            }
            // Scroll
            if mouse.buttons & 8 != 0 {
                ffi::spice_inputs_channel_button_press(
                    inputs,
                    ffi::SPICE_MOUSE_BUTTON_UP,
                    button_state,
                );
                ffi::spice_inputs_channel_button_release(
                    inputs,
                    ffi::SPICE_MOUSE_BUTTON_UP,
                    button_state,
                );
            }
            if mouse.buttons & 16 != 0 {
                ffi::spice_inputs_channel_button_press(
                    inputs,
                    ffi::SPICE_MOUSE_BUTTON_DOWN,
                    button_state,
                );
                ffi::spice_inputs_channel_button_release(
                    inputs,
                    ffi::SPICE_MOUSE_BUTTON_DOWN,
                    button_state,
                );
            }
        }
    }

    // Handle resize requests via monitor config (only when SPICE agent is connected)
    if let Some(main) = ctx.main_channel {
        if state.agent_connected {
            if let Some((w, h)) = state.pending_resize.take() {
                ffi::spice_main_update_display(main, 0, 0, 0, w as i32, h as i32, 1);
                ffi::spice_main_set_display_enabled(main, 0, 1);
                ffi::spice_main_send_monitor_config(main);
                tracing::debug!("SPICE: requested resolution {}x{}", w, h);
            }
        } else {
            // Discard resize requests when agent isn't connected
            state.pending_resize.take();
        }

        // Handle clipboard host → guest
        if let Some(ref text) = state.clipboard_to_guest.take() {
            let bytes = text.as_bytes();
            let type_ = ffi::VD_AGENT_CLIPBOARD_UTF8_TEXT;
            let sel = ffi::VD_AGENT_CLIPBOARD_SELECTION_CLIPBOARD;
            let types = [type_];
            ffi::spice_main_channel_clipboard_selection_grab(main, sel, types.as_ptr(), 1);
            ffi::spice_main_channel_clipboard_selection_notify(
                main,
                sel,
                type_,
                bytes.as_ptr(),
                bytes.len() as i32,
            );
        }
    }

    1 // G_SOURCE_CONTINUE
}

// ═══════════════════════════════════════════════════════════════════════
// GLib signal callbacks
// ═══════════════════════════════════════════════════════════════════════

unsafe extern "C" fn on_channel_new(
    _session: *mut ffi::SpiceSession,
    channel: *mut ffi::SpiceChannel,
    user_data: ffi::gpointer,
) {
    let ctx = &mut *(user_data as *mut SpiceCtx);
    let ch = channel as ffi::gpointer;

    // Hook channel-event on EVERY channel — this is how we detect
    // connection success (OPENED) or failure (ERROR_*) and closed (CLOSED).
    ffi::g_signal_connect(
        ch,
        b"channel-event\0",
        ffi::as_gcallback(on_channel_event as unsafe extern "C" fn(_, _, _)),
        user_data,
    );

    let display_type = ffi::spice_display_channel_get_type();
    let inputs_type = ffi::spice_inputs_channel_get_type();
    let main_type = ffi::spice_main_channel_get_type();
    let cursor_type = ffi::spice_cursor_channel_get_type();

    if ffi::g_type_check_instance_is_a(ch, display_type) != 0 {
        tracing::info!("SPICE: display channel created");
        ctx.display_channel = Some(channel as *mut ffi::SpiceDisplayChannel);

        ffi::g_signal_connect(
            ch,
            b"display-primary-create\0",
            ffi::as_gcallback(on_primary_create as unsafe extern "C" fn(_, _, _, _, _, _, _, _)),
            user_data,
        );
        ffi::g_signal_connect(
            ch,
            b"display-invalidate\0",
            ffi::as_gcallback(on_display_invalidate as unsafe extern "C" fn(_, _, _, _, _, _)),
            user_data,
        );
        ffi::g_signal_connect(
            ch,
            b"display-primary-destroy\0",
            ffi::as_gcallback(on_primary_destroy as unsafe extern "C" fn(_, _)),
            user_data,
        );
        ffi::g_signal_connect(
            ch,
            b"display-mark\0",
            ffi::as_gcallback(on_display_mark as unsafe extern "C" fn(_, _, _)),
            user_data,
        );
    } else if ffi::g_type_check_instance_is_a(ch, inputs_type) != 0 {
        tracing::info!("SPICE: inputs channel created");
        ctx.inputs_channel = Some(channel as *mut ffi::SpiceInputsChannel);
    } else if ffi::g_type_check_instance_is_a(ch, main_type) != 0 {
        tracing::info!("SPICE: main channel created — marking connected");
        ctx.main_channel = Some(channel as *mut ffi::SpiceMainChannel);

        // The channel-new signal fires AFTER the channel has connected,
        // so channel-event OPENED already happened. Mark connected now.
        if let Ok(mut s) = ctx.state.lock() {
            s.connected = true;
            s.error = None;
        }

        // Connect clipboard signals
        ffi::g_signal_connect(
            ch,
            b"main-clipboard-selection-grab\0",
            ffi::as_gcallback(on_clipboard_grab as unsafe extern "C" fn(_, _, _, _, _)),
            user_data,
        );
        ffi::g_signal_connect(
            ch,
            b"main-clipboard-selection\0",
            ffi::as_gcallback(on_clipboard_data as unsafe extern "C" fn(_, _, _, _, _, _)),
            user_data,
        );
        ffi::g_signal_connect(
            ch,
            b"main-clipboard-selection-request\0",
            ffi::as_gcallback(on_clipboard_request as unsafe extern "C" fn(_, _, _, _)),
            user_data,
        );
        ffi::g_signal_connect(
            ch,
            b"main-agent-update\0",
            ffi::as_gcallback(on_agent_update as unsafe extern "C" fn(_, _)),
            user_data,
        );
    } else if ffi::g_type_check_instance_is_a(ch, cursor_type) != 0 {
        tracing::info!("SPICE: cursor channel created");
        ffi::g_signal_connect(
            ch,
            b"cursor-set\0",
            ffi::as_gcallback(on_cursor_set as unsafe extern "C" fn(_, _, _, _, _, _, _)),
            user_data,
        );
    }

    // Explicitly connect the channel — required for display data flow.
    // Skip USB redirect channels (they assert if host device isn't configured).
    let usbredir_type = ffi::spice_usbredir_channel_get_type();
    if ffi::g_type_check_instance_is_a(ch, usbredir_type) == 0 {
        ffi::spice_channel_connect(channel);
    } else {
        tracing::debug!("SPICE: skipping connect for USB redirect channel (no host configured)");
    }
}

/// Channel lifecycle event — fires for OPENED, CLOSED, and all ERROR_* states.
/// This is the ONLY place we set `connected = true` (on OPENED for main channel)
/// and where we detect connection failures to show proper error messages.
unsafe extern "C" fn on_channel_event(
    channel: *mut ffi::SpiceChannel,
    event: std::os::raw::c_int,
    user_data: ffi::gpointer,
) {
    let ctx = &mut *(user_data as *mut SpiceCtx);
    let ch = channel as ffi::gpointer;
    let main_type = ffi::spice_main_channel_get_type();
    let is_main = ffi::g_type_check_instance_is_a(ch, main_type) != 0;

    match event {
        ffi::SPICE_CHANNEL_OPENED => {
            tracing::info!("SPICE: channel opened (main={})", is_main);
            if is_main {
                // Main channel opened = real connection established
                if let Ok(mut s) = ctx.state.lock() {
                    s.connected = true;
                    s.error = None;
                }
                tracing::info!("SPICE: session fully connected");
            }
        },
        ffi::SPICE_CHANNEL_CLOSED => {
            tracing::info!("SPICE: channel closed (main={})", is_main);
            if is_main {
                if let Ok(mut s) = ctx.state.lock() {
                    s.connected = false;
                }
                // Quit the GLib loop so the thread can clean up
                ffi::g_main_loop_quit(ctx.main_loop);
            }
        },
        ffi::SPICE_CHANNEL_ERROR_CONNECT => {
            tracing::error!("SPICE: connection error (TCP refused or unreachable)");
            if let Ok(mut s) = ctx.state.lock() {
                s.connected = false;
                s.error = Some("SPICE: connection refused (is the VM running?)".into());
            }
            if is_main {
                ffi::g_main_loop_quit(ctx.main_loop);
            }
        },
        ffi::SPICE_CHANNEL_ERROR_TLS => {
            tracing::error!("SPICE: TLS error");
            if let Ok(mut s) = ctx.state.lock() {
                s.connected = false;
                s.error = Some("SPICE: TLS handshake failed".into());
            }
            if is_main {
                ffi::g_main_loop_quit(ctx.main_loop);
            }
        },
        ffi::SPICE_CHANNEL_ERROR_LINK => {
            tracing::error!("SPICE: link error (protocol mismatch)");
            if let Ok(mut s) = ctx.state.lock() {
                s.connected = false;
                s.error = Some("SPICE: protocol link failed".into());
            }
            if is_main {
                ffi::g_main_loop_quit(ctx.main_loop);
            }
        },
        ffi::SPICE_CHANNEL_ERROR_AUTH => {
            tracing::error!("SPICE: authentication failed");
            if let Ok(mut s) = ctx.state.lock() {
                s.connected = false;
                s.error = Some("SPICE: authentication failed".into());
            }
            if is_main {
                ffi::g_main_loop_quit(ctx.main_loop);
            }
        },
        ffi::SPICE_CHANNEL_ERROR_IO => {
            tracing::error!("SPICE: I/O error");
            if let Ok(mut s) = ctx.state.lock() {
                s.connected = false;
                s.error = Some("SPICE: I/O error (connection lost)".into());
            }
            if is_main {
                ffi::g_main_loop_quit(ctx.main_loop);
            }
        },
        other => {
            tracing::debug!("SPICE: channel event {} (main={})", other, is_main);
        },
    }
}

unsafe extern "C" fn on_channel_destroy(
    _session: *mut ffi::SpiceSession,
    channel: *mut ffi::SpiceChannel,
    user_data: ffi::gpointer,
) {
    let ctx = &mut *(user_data as *mut SpiceCtx);
    let ch = channel as ffi::gpointer;
    let inputs_type = ffi::spice_inputs_channel_get_type();
    let main_type = ffi::spice_main_channel_get_type();
    let display_type = ffi::spice_display_channel_get_type();

    if ffi::g_type_check_instance_is_a(ch, inputs_type) != 0 {
        ctx.inputs_channel = None;
    } else if ffi::g_type_check_instance_is_a(ch, main_type) != 0 {
        ctx.main_channel = None;
    } else if ffi::g_type_check_instance_is_a(ch, display_type) != 0 {
        ctx.display_channel = None;
        ctx.surface_data = std::ptr::null_mut();
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Display callbacks
// ═══════════════════════════════════════════════════════════════════════

/// Primary surface created — cache the pointer + copy full framebuffer.
unsafe extern "C" fn on_primary_create(
    _channel: *mut ffi::SpiceDisplayChannel,
    format: std::os::raw::c_int,
    width: std::os::raw::c_int,
    height: std::os::raw::c_int,
    stride: std::os::raw::c_int,
    _shmid: std::os::raw::c_int,
    imgdata: *mut u8,
    user_data: ffi::gpointer,
) {
    let ctx = &mut *(user_data as *mut SpiceCtx);

    if width <= 0 || height <= 0 || imgdata.is_null() {
        tracing::warn!("SPICE: invalid primary surface {}x{}", width, height);
        return;
    }

    let w = width as u16;
    let h = height as u16;
    tracing::info!(
        "SPICE: primary {}x{} fmt={} stride={}",
        w,
        h,
        format,
        stride
    );

    // Cache the surface pointer for dirty-rect repaints
    ctx.surface_data = imgdata;
    ctx.surface_stride = stride;
    ctx.surface_width = w;
    ctx.surface_height = h;

    // Full copy BGRA → RGBA
    let rgba = copy_surface_bgra_to_rgba(imgdata, w, h, stride);

    if let Ok(mut state) = ctx.state.lock() {
        state.width = w;
        state.height = h;
        state.pixels = rgba;
        state.pixel_generation = state.pixel_generation.wrapping_add(1);
    }
}

/// Display region invalidated — re-copy dirty rect from cached surface.
unsafe extern "C" fn on_display_invalidate(
    _channel: *mut ffi::SpiceDisplayChannel,
    x: std::os::raw::c_int,
    y: std::os::raw::c_int,
    w: std::os::raw::c_int,
    h: std::os::raw::c_int,
    user_data: ffi::gpointer,
) {
    let ctx = &*(user_data as *mut SpiceCtx);

    if ctx.surface_data.is_null() || ctx.surface_width == 0 {
        return;
    }

    let sw = ctx.surface_width as usize;
    let sh = ctx.surface_height as usize;
    let stride = ctx.surface_stride;
    let abs_stride = stride.unsigned_abs() as usize;

    // Clamp dirty rect to surface bounds
    let dx = (x as usize).min(sw);
    let dy = (y as usize).min(sh);
    let dw = (w as usize).min(sw - dx);
    let dh = (h as usize).min(sh - dy);

    if dw == 0 || dh == 0 {
        return;
    }

    let src_base = if stride < 0 {
        ctx.surface_data
            .offset(-((sh as isize - 1) * stride as isize))
    } else {
        ctx.surface_data
    };

    if let Ok(mut state) = ctx.state.lock() {
        if state.pixels.len() != sw * sh * 4 {
            return;
        }

        // Copy only the dirty rectangle BGRA → RGBA
        for row in dy..dy + dh {
            let src_row = src_base.add(row * abs_stride);
            let dst_offset = (row * sw + dx) * 4;
            let dst = &mut state.pixels[dst_offset..dst_offset + dw * 4];
            for col in 0..dw {
                let si = (dx + col) * 4;
                let b = *src_row.add(si);
                let g = *src_row.add(si + 1);
                let r = *src_row.add(si + 2);
                let a = *src_row.add(si + 3);
                dst[col * 4] = r;
                dst[col * 4 + 1] = g;
                dst[col * 4 + 2] = b;
                dst[col * 4 + 3] = a;
            }
        }

        state.pixel_generation = state.pixel_generation.wrapping_add(1);
    }
}

/// Primary surface destroyed.
unsafe extern "C" fn on_primary_destroy(
    _channel: *mut ffi::SpiceDisplayChannel,
    user_data: ffi::gpointer,
) {
    let ctx = &mut *(user_data as *mut SpiceCtx);
    tracing::info!("SPICE: primary surface destroyed");
    ctx.surface_data = std::ptr::null_mut();
    ctx.surface_width = 0;
    ctx.surface_height = 0;

    if let Ok(mut state) = ctx.state.lock() {
        state.width = 0;
        state.height = 0;
        state.pixels.clear();
        state.pixel_generation = state.pixel_generation.wrapping_add(1);
    }
}

/// Display ready signal — full surface is ready to show.
unsafe extern "C" fn on_display_mark(
    _channel: *mut ffi::SpiceDisplayChannel,
    mark: std::os::raw::c_int,
    user_data: ffi::gpointer,
) {
    let ctx = &*(user_data as *mut SpiceCtx);
    if mark != 0 && !ctx.surface_data.is_null() {
        // Full re-copy on mark
        let rgba = copy_surface_bgra_to_rgba(
            ctx.surface_data,
            ctx.surface_width,
            ctx.surface_height,
            ctx.surface_stride,
        );
        if let Ok(mut state) = ctx.state.lock() {
            state.pixels = rgba;
            state.pixel_generation = state.pixel_generation.wrapping_add(1);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Clipboard callbacks
// ═══════════════════════════════════════════════════════════════════════

/// Guest grabbed the clipboard (has new content).
unsafe extern "C" fn on_clipboard_grab(
    _channel: *mut ffi::SpiceMainChannel,
    selection: ffi::guint,
    types: *mut ffi::guint32,
    ntypes: ffi::guint,
    user_data: ffi::gpointer,
) {
    let ctx = &*(user_data as *mut SpiceCtx);

    if selection != ffi::VD_AGENT_CLIPBOARD_SELECTION_CLIPBOARD {
        return;
    }

    // Check if guest has UTF-8 text
    let type_slice = std::slice::from_raw_parts(types, ntypes as usize);
    let has_text = type_slice
        .iter()
        .any(|&t| t == ffi::VD_AGENT_CLIPBOARD_UTF8_TEXT);

    if has_text {
        // Request the text from the guest
        if let Some(main) = ctx.main_channel {
            ffi::spice_main_channel_clipboard_selection_request(
                main,
                ffi::VD_AGENT_CLIPBOARD_SELECTION_CLIPBOARD,
                ffi::VD_AGENT_CLIPBOARD_UTF8_TEXT,
            );
        }
    }
}

/// Guest sent clipboard data (response to our request).
unsafe extern "C" fn on_clipboard_data(
    _channel: *mut ffi::SpiceMainChannel,
    selection: ffi::guint,
    type_: ffi::guint32,
    data: *const u8,
    size: ffi::guint,
    user_data: ffi::gpointer,
) {
    let ctx = &*(user_data as *mut SpiceCtx);

    if selection != ffi::VD_AGENT_CLIPBOARD_SELECTION_CLIPBOARD {
        return;
    }
    if type_ != ffi::VD_AGENT_CLIPBOARD_UTF8_TEXT {
        return;
    }
    if data.is_null() || size == 0 {
        return;
    }

    let text_bytes = std::slice::from_raw_parts(data, size as usize);
    if let Ok(text) = std::str::from_utf8(text_bytes) {
        tracing::debug!("SPICE: clipboard from guest ({} bytes)", size);
        if let Ok(mut state) = ctx.state.lock() {
            state.clipboard_from_guest = Some(text.to_string());
        }
    }
}

/// Guest requests clipboard data from us.
unsafe extern "C" fn on_clipboard_request(
    _channel: *mut ffi::SpiceMainChannel,
    selection: ffi::guint,
    type_: ffi::guint32,
    user_data: ffi::gpointer,
) {
    let ctx = &*(user_data as *mut SpiceCtx);

    if selection != ffi::VD_AGENT_CLIPBOARD_SELECTION_CLIPBOARD {
        return;
    }
    if type_ != ffi::VD_AGENT_CLIPBOARD_UTF8_TEXT {
        return;
    }

    // Send whatever we have in clipboard_to_guest
    if let Ok(state) = ctx.state.lock() {
        if let Some(ref text) = state.clipboard_to_guest {
            if let Some(main) = ctx.main_channel {
                let bytes = text.as_bytes();
                ffi::spice_main_channel_clipboard_selection_notify(
                    main,
                    ffi::VD_AGENT_CLIPBOARD_SELECTION_CLIPBOARD,
                    ffi::VD_AGENT_CLIPBOARD_UTF8_TEXT,
                    bytes.as_ptr(),
                    bytes.len() as i32,
                );
            }
        }
    }
}

/// Agent connection status changed.
unsafe extern "C" fn on_agent_update(
    _channel: *mut ffi::SpiceMainChannel,
    user_data: ffi::gpointer,
) {
    let ctx = &*(user_data as *mut SpiceCtx);
    tracing::info!("SPICE: agent update");
    if let Ok(mut state) = ctx.state.lock() {
        state.agent_connected = true;
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Cursor callback
// ═══════════════════════════════════════════════════════════════════════

unsafe extern "C" fn on_cursor_set(
    _channel: *mut ffi::SpiceCursorChannel,
    _width: std::os::raw::c_int,
    _height: std::os::raw::c_int,
    _hot_x: std::os::raw::c_int,
    _hot_y: std::os::raw::c_int,
    _rgba: ffi::gpointer,
    user_data: ffi::gpointer,
) {
    let _ctx = &*(user_data as *mut SpiceCtx);
    // TODO: Set custom cursor shape in egui
    // For now, egui uses its own software cursor which works fine
}

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

/// Copy a BGRA surface (possibly bottom-up) to an RGBA Vec.
unsafe fn copy_surface_bgra_to_rgba(imgdata: *mut u8, w: u16, h: u16, stride: i32) -> Vec<u8> {
    let pixel_count = (w as usize) * (h as usize);
    let mut rgba = vec![0u8; pixel_count * 4];
    let abs_stride = stride.unsigned_abs() as usize;

    let src_base = if stride < 0 {
        imgdata.offset(-((h as isize - 1) * stride as isize))
    } else {
        imgdata
    };

    for y in 0..h as usize {
        let row_src = src_base.add(y * abs_stride);
        let row_dst = &mut rgba[y * w as usize * 4..][..w as usize * 4];
        for x in 0..w as usize {
            let b = *row_src.add(x * 4);
            let g = *row_src.add(x * 4 + 1);
            let r = *row_src.add(x * 4 + 2);
            let a = *row_src.add(x * 4 + 3);
            row_dst[x * 4] = r;
            row_dst[x * 4 + 1] = g;
            row_dst[x * 4 + 2] = b;
            row_dst[x * 4 + 3] = a;
        }
    }
    rgba
}

// ═══════════════════════════════════════════════════════════════════════
// Keysym → scancode
// ═══════════════════════════════════════════════════════════════════════

fn keysym_to_scancode(keysym: u32) -> u32 {
    match keysym {
        // Letters a-z
        0x61..=0x7a => {
            const SC: [u32; 26] = [
                0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31,
                0x18, 0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
            ];
            SC[(keysym - 0x61) as usize]
        },
        // Letters A-Z (same scancodes)
        0x41..=0x5a => {
            const SC: [u32; 26] = [
                0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31,
                0x18, 0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
            ];
            SC[(keysym - 0x41) as usize]
        },
        // Digits
        0x30 => 0x0b,                        // 0
        0x31..=0x39 => keysym - 0x31 + 0x02, // 1-9
        // Function keys F1-F12
        0xffbe..=0xffc9 => keysym - 0xffbe + 0x3b,
        // Special keys
        0xff0d => 0x1c, // Return
        0xff1b => 0x01, // Escape
        0xff08 => 0x0e, // BackSpace
        0xff09 => 0x0f, // Tab
        0x0020 => 0x39, // Space
        // Modifiers
        0xffe1 => 0x2a, // Shift_L
        0xffe2 => 0x36, // Shift_R
        0xffe3 => 0x1d, // Control_L
        0xffe4 => 0x9d, // Control_R
        0xffe9 => 0x38, // Alt_L
        0xffea => 0xb8, // Alt_R
        0xffe5 => 0x3a, // Caps_Lock
        0xff7f => 0x45, // Num_Lock
        0xff14 => 0x46, // Scroll_Lock
        // Navigation
        0xff51 => 0xcb, // Left
        0xff52 => 0xc8, // Up
        0xff53 => 0xcd, // Right
        0xff54 => 0xd0, // Down
        0xff50 => 0xc7, // Home
        0xff57 => 0xcf, // End
        0xff55 => 0xc9, // Page_Up
        0xff56 => 0xd1, // Page_Down
        0xffff => 0xd3, // Delete
        0xff63 => 0xd2, // Insert
        0xff13 => 0xc5, // Pause
        0xff61 => 0xb7, // Print
        // Punctuation
        0x002d => 0x0c, // minus
        0x003d => 0x0d, // equal
        0x005b => 0x1a, // bracketleft
        0x005d => 0x1b, // bracketright
        0x005c => 0x2b, // backslash
        0x003b => 0x27, // semicolon
        0x0027 => 0x28, // apostrophe
        0x0060 => 0x29, // grave
        0x002c => 0x33, // comma
        0x002e => 0x34, // period
        0x002f => 0x35, // slash
        // Super/Menu
        0xffeb => 0xdb, // Super_L
        0xffec => 0xdc, // Super_R
        0xff67 => 0xdd, // Menu
        // Fallback
        _ => keysym,
    }
}
