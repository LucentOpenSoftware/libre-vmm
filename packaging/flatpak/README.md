# Libre VMM — Flatpak Packaging

This directory contains the Flatpak manifest and supporting files for building
Libre VMM as a self-contained Flatpak. Flatpak is the recommended distribution
channel for users who:

- Are coming from VMware Workstation and don't want to install system packages
- Are on distros without good `libvirt-dev` coverage (Silverblue, SteamOS, etc.)
- Want an atomic, easily-removable install
- Need to run multiple versions side-by-side

The Flatpak runs everything in `qemu:///session` mode — no system `libvirtd`
required. QEMU, swtpm, OVMF, and libvirt itself are bundled inside the Flatpak.

---

## Files

- `org.libre_vmm.LibreVmm.yml` — main Flatpak manifest
- `org.libre_vmm.LibreVmm.metainfo.xml` — AppStream metadata (Flathub requirement)
- `cargo-sources.json` — vendored Cargo dependency manifest (stub; must be generated)
- `README.md` — this file

---

## Hash placeholders

Before a real build, replace every `REPLACE_WITH_REAL_HASH*` in the manifest with
the actual SHA-256 of the upstream tarball. The placeholders are deliberately
invalid so a build attempt will fail loudly rather than silently fetching the
wrong artifact.

The current placeholders are:

| Module      | URL                                                         | What to fill in                    |
| ----------- | ----------------------------------------------------------- | ---------------------------------- |
| `libvirt`   | `https://libvirt.org/sources/libvirt-10.0.0.tar.xz`         | SHA-256 from libvirt.org           |
| `swtpm`     | `https://github.com/stefanberger/swtpm/archive/v0.8.0.tar.gz` | SHA-256 of the GitHub tag tarball |
| `edk2-ovmf` | `REPLACE_OVMF_TARBALL_URL`                                  | Both URL and SHA-256               |

Generate hashes with:

```bash
curl -L <url> | sha256sum
```

---

## Generating `cargo-sources.json`

Flatpak builds run offline (`--offline`) so all Cargo dependencies must be
vendored as Flatpak sources. Generate the manifest from `Cargo.lock`:

```bash
pip install --user flatpak-cargo-generator
cd /home/neindev8/Escritorio/VM-Soft/libre-vmm
flatpak-cargo-generator Cargo.lock -o packaging/flatpak/cargo-sources.json
```

This must be re-run any time `Cargo.lock` changes (i.e. after any
`cargo update` or dependency bump).

---

## Test-building locally

```bash
cd /home/neindev8/Escritorio/VM-Soft/libre-vmm
flatpak install --user flathub \
    org.gnome.Platform//46 \
    org.gnome.Sdk//46 \
    org.freedesktop.Sdk.Extension.rust-stable//23.08
flatpak-builder --user --install --force-clean \
    build-dir packaging/flatpak/org.libre_vmm.LibreVmm.yml
```

---

## Running

```bash
flatpak run org.libre_vmm.LibreVmm
```

The CLI is also available inside the sandbox:

```bash
flatpak run --command=vmm org.libre_vmm.LibreVmm list
flatpak run --command=vmm-api org.libre_vmm.LibreVmm
```

---

## Known limitations

1. **GPU passthrough** — VFIO passthrough requires direct access to PCI devices
   under `/sys/bus/pci/...`. Flatpak's sandbox blocks this by default. Users
   needing GPU passthrough should fall back to a native package (deb/rpm/AUR) or
   run with `--filesystem=/sys/bus/pci:ro --filesystem=/sys/devices` overrides
   (still experimental).
2. **Bridged networking** — Creating a host bridge requires `CAP_NET_ADMIN` on
   the host. Inside Flatpak only user-mode SLIRP and the libvirt user network
   work out of the box.
3. **Host paths outside `/home`** — disks, ISOs, and migrations stored in
   `/var`, `/opt`, etc. are invisible to the sandbox. Users either move them
   under `~/` or grant `--filesystem=...` overrides at install time.
4. **Host kernel modules** — the sandbox uses host `/dev/kvm` but cannot load
   `kvm_intel`/`kvm_amd` if they aren't loaded already. We surface a clear
   error in the GUI if `/dev/kvm` is absent.
5. **systemd integration** — auto-start of VMs at boot doesn't work from a
   Flatpak; that's a deliberate design choice for sandboxed apps.

---

## Flathub submission plan

Once the build is verified end-to-end (booting a real VM through the bundled
QEMU + libvirt + OVMF), the steps for Flathub submission are:

1. Fork `flathub/flathub`, create branch `new-pr/org.libre_vmm.LibreVmm`.
2. Add this manifest + the generated `cargo-sources.json`.
3. Wait for the Flathub CI build + reviewer feedback.
4. Address review comments (often: tighten finish-args, fix metainfo screenshots).
5. Add screenshots once the GUI is stable — currently deferred to Wave 14.
6. Once merged, set up signed releases via the repo's `tag-trigger.yml`.

See <https://docs.flathub.org/docs/for-app-authors/submission> for the full
process.
