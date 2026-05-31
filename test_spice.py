#!/usr/bin/env python3
"""Quick test: connect to SPICE and see if display-primary-create fires."""
import sys, signal
import gi
gi.require_version('SpiceClientGLib', '2.0')
from gi.repository import SpiceClientGLib, GLib

def on_channel_new(session, channel):
    ctype = type(channel).__name__
    print(f"[+] channel-new: {ctype}")
    channel.connect("channel-event", on_channel_event)

    if isinstance(channel, SpiceClientGLib.DisplayChannel):
        channel.connect("display-primary-create", on_primary_create)
        channel.connect("display-invalidate", on_invalidate)
        channel.connect("display-mark", on_mark)
        # Explicitly connect the channel (required for display data)
        print(f"[+] calling channel.connect() on display...")
        channel.connect_channel()  # This is spice_channel_connect()
        # Try polling
        GLib.timeout_add(1000, poll_primary, channel)

    elif isinstance(channel, SpiceClientGLib.MainChannel):
        channel.connect("main-agent-update", lambda c: print("[+] agent-update"))

    else:
        # Connect all channels explicitly
        channel.connect_channel()

def on_channel_event(channel, event):
    print(f"[+] channel-event: {event} on {type(channel).__name__}")

def on_primary_create(channel, fmt, w, h, stride, shmid, data):
    print(f"[+] PRIMARY CREATE: {w}x{h} fmt={fmt} stride={stride}")

def on_invalidate(channel, x, y, w, h):
    print(f"[+] invalidate: {x},{y} {w}x{h}")

def on_mark(channel, mark):
    print(f"[+] display-mark: {mark}")

def poll_primary(channel):
    ok, primary = channel.get_primary(0)
    print(f"[?] poll get_primary: ok={ok}, w={primary.width if ok else '?'}, h={primary.height if ok else '?'}")
    return True  # keep polling

session = SpiceClientGLib.Session()
session.set_property("uri", f"spice://127.0.0.1?port={sys.argv[1] if len(sys.argv)>1 else 5900}")
session.connect("channel-new", on_channel_new)
session.connect("channel-destroy", lambda s, c: print(f"[-] channel-destroy: {type(c).__name__}"))

print(f"Connecting to port {sys.argv[1] if len(sys.argv)>1 else 5900}...")
if not session.connect():
    print("FAILED to connect")
    sys.exit(1)

loop = GLib.MainLoop()
signal.signal(signal.SIGINT, lambda *a: loop.quit())
print("Running main loop (Ctrl+C to quit)...")
loop.run()
