# Contributing to Libre VMM

Thanks for considering a contribution. Libre VMM is community-maintained
and patches, bug reports, packaging help, documentation polish, and
translation work are all in scope.

This document is the contributor handbook. Read [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
and [SECURITY.md](SECURITY.md) before contributing.

---

## Code of Conduct

This project adopts the Contributor Covenant 2.1.
See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for the full text. By
participating you agree to abide by it.

---

## Development environment

### Prerequisites

- Rust 1.75 or newer (install via [rustup](https://rustup.rs))
- QEMU 8.2+, libvirt 10.0+, OVMF / LibreUEFI, `swtpm`
- `libvirt-dev` headers (or the in-repo workaround — see Build Notes below)
- A Linux desktop session (Wayland preferred, X11 supported)

### Install system dependencies

```bash
git clone https://github.com/LucentOpenSoftware/libre-vmm.git
cd libre-vmm
./scripts/setup-deps.sh
```

`setup-deps.sh` detects your distribution (Debian/Ubuntu, Fedora/RHEL,
Arch) and installs the right package set. Read it before running.

### Build

```bash
./build.sh                  # debug build
./build.sh release          # release build
```

`build.sh` sets `PKG_CONFIG_PATH` to the in-repo `lib/pkgconfig/`
workaround so the workspace builds even when distro `libvirt-dev` is not
installed.

If you prefer running `cargo` directly:

```bash
export PKG_CONFIG_PATH=$PWD/lib/pkgconfig:$PKG_CONFIG_PATH
cargo build --workspace
```

### Run tests

```bash
PKG_CONFIG_PATH=$PWD/lib/pkgconfig cargo test --workspace
```

Expected: **438 tests pass** (403 in `vmm-core`, 30 in `vmm-types`, 5 in `vmm-api`).
A patch that drops the test count without justification will not be merged.

### Cross-compile to Windows (foundation)

```bash
rustup target add x86_64-pc-windows-gnu
cargo build -p vmm-types --target x86_64-pc-windows-gnu
```

`vmm-types` compiles for Windows today. If you touch `vmm-types` make
sure this still passes. The full Windows host port is tracked in
[`docs/WINDOWS-PORT.md`](docs/WINDOWS-PORT.md).

---

## Code style

### Formatting

Run `cargo fmt --all` before committing. CI will reject unformatted code.

### Lints

Run `cargo clippy --workspace --all-targets -- -D warnings` and resolve
every warning. Five pre-existing dead-code warnings are intentional
(infrastructure for upcoming waves); do not "fix" them by deleting code,
and do not add new ones without justification.

### Conventions

- Edition 2021. Workspace dependencies (in the root `Cargo.toml`) are
  the single source of truth for `serde`, `tokio`, `anyhow`, `tracing`,
  etc. Add new shared deps there.
- Errors: `thiserror` for typed library errors, `anyhow` for binary
  entry points.
- Logging: `tracing`. Spans for VM lifecycle operations.
- Async: `tokio`. Avoid blocking calls inside async contexts.
- Paths and command arguments: never interpolate untrusted input into a
  shell command or libvirt XML. Reuse the existing safe helpers
  (see `vmm-core/src/xml_builder.rs` and the path-traversal guards).
- New `unsafe` blocks must have a `// SAFETY:` comment.

---

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
Examples:

```
feat(snapshot): visual tree renderer with branch lines
fix(spice): guard monitor_config send on agent presence
docs(roadmap): mark Wave 12.1 live migration GUI shipped
refactor(vmm-types): extract DisplayProtocol enum
test(vmm-core): regression for quiesced snapshot drop-guard
```

Allowed types: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`,
`build`, `ci`, `chore`. Scopes are usually the crate (`vmm-core`) or
module (`snapshot`, `network`, `xml_builder`).

Subject line under 72 characters. Body explains the *why* and references
the parity matrix or roadmap row when applicable (e.g. "Closes Wave 11.7").

### DCO sign-off (required)

Every commit must carry a Developer Certificate of Origin sign-off. Use
`git commit -s` so each commit gets:

```
Signed-off-by: Your Name <you@example.com>
```

The DCO text is at <https://developercertificate.org>. By signing off you
certify you have the right to contribute the code under
GPL-3.0-or-later. PRs containing unsigned commits will be asked to
rebase.

---

## Pull request process

1. **Open an issue first** for non-trivial changes. Link the parity-matrix
   row or roadmap wave entry you're addressing. This avoids duplicated
   work and surprise rewrites.
2. **Branch from `main`.** Name the branch `feat/<scope>-<short-desc>`,
   `fix/<scope>-<short-desc>`, etc.
3. **Keep the PR focused.** One feature or one fix per PR. Refactors
   that touch many files should be their own PR.
4. **Update tests.** Every behavioral change ships with a test. New
   modules ship with module-level tests.
5. **Update docs.** If you change user-facing behavior, update
   `README.md`, `docs/VMWARE-PARITY.md`, or `CHANGELOG.md` (the
   `[Unreleased]` section).
6. **Run the full check locally:**

   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   PKG_CONFIG_PATH=$PWD/lib/pkgconfig cargo test --workspace
   ```

7. **Open the PR** with a description that states the problem, the
   solution, the test plan, and any user-visible changes.
8. **Respond to review.** PRs typically need two maintainer approvals
   before merge.

---

## How to claim a task

The roadmap and parity matrix are pre-scoped to make picking work easy.

- [`docs/VMWARE-PARITY.md`](docs/VMWARE-PARITY.md) — every `📋 planned`
  row is fair game. Open an issue saying you intend to take it; cite the
  section number (e.g. "Section 2.5, LAN segment auto-create").
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — every unticked wave row has a
  task number, file pointer, and effort estimate. Reference the wave
  number in your PR title (e.g. `feat(network): wave 5.2 port forwarding presets`).

If you're not sure where to start, the areas listed below welcome help
right now.

---

## Areas where help is welcome

- **Packaging.** AUR upload, Debian repository, Fedora Copr, Flatpak
  Flathub submission. See [`packaging/`](packaging/).
- **Translation / i18n.** The GUI is currently English-only. We have not
  yet wired `fluent`/`gettext`; a contribution that lands the framework
  is welcome.
- **Wayland compatibility testing.** See
  [`docs/WAYLAND-COMPATIBILITY.md`](docs/WAYLAND-COMPATIBILITY.md) for
  the sign-off table. We need confirmed reports across Sway, Hyprland,
  GNOME, KDE, Cosmic.
- **Documentation polish.** End-user docs, screenshots, video walk-throughs.
- **Wave 14 ecosystem work.** Terraform provider, Ansible collection,
  Vagrant provider. The REST API and OpenAPI spec are ready.
- **VFIO and Looking Glass field reports.** Real hardware testing
  surfaces edge cases unit tests miss.
- **Windows port foundation.** See `docs/WINDOWS-PORT.md` Phase A
  (path helpers, process spawning, cross-platform CI). The Windows
  hypervisor backend is multi-quarter and not yet open for contribution.

---

## Security

Do not file public issues for security vulnerabilities. See
[SECURITY.md](SECURITY.md) for the disclosure policy and contact email.
Coordinated disclosure with credit is the norm.

---

## License of contributions

Libre VMM is GPL-3.0-or-later. By submitting a contribution with a DCO
sign-off you license it under the same terms. Do not paste code from
incompatible licenses (proprietary, GPL-2.0-only, etc.) into the
codebase. When borrowing from compatibly-licensed upstreams (Proxmox
AGPL-3.0, vm-curator MIT, Vectras VM GPL-3.0, etc.) preserve attribution
and call out the source in the PR description and in the relevant module
header.

---

Thank you for helping make a libre VMware alternative real.
