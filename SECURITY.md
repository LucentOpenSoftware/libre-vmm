# Security Policy

This document explains how to report a security issue in Libre VMM and
which versions receive security fixes.

## Supported Versions

Libre VMM is in active development. The most recent release receives
security updates; older versions are best-effort.

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | ✅ active          |
| < 0.1   | ❌ pre-release dev |

When subsequent minor releases ship, this table will be updated to keep
at least the latest two minor versions supported.

## Reporting a Vulnerability

Please report security issues **privately** to the project maintainers,
not via public GitHub issues.

**Use GitHub's private vulnerability reporting** at
https://github.com/LucentOpenSoftware/libre-vmm/security/advisories/new

This channel keeps the report private, lets maintainers triage it
without disclosure, and gives both parties a single thread to track
status. Dedicated out-of-band contact channels (encrypted email, a
published key fingerprint) will be set up as the project matures; for
now, GitHub's private reporting is the only supported path.

### What to include in a report

- A short description of the issue and its potential impact
- Steps or a proof-of-concept that demonstrates the issue
- The Libre VMM version, host OS and version, and kernel version
- Whether you've discussed this with anyone else outside the project
- Your preferred name (or pseudonym) for any eventual acknowledgement

### What to expect

- Initial acknowledgement within **5 business days** of your report
- A triage decision and a target fix timeline within **15 business days**
- Coordinated disclosure on a default **90-day** clock from the
  acknowledgement date, with extensions by mutual agreement if a fix is
  in flight
- Credit in the project's advisory and changelog if you wish

We will work with you in good faith. Please give us a reasonable window
to address the issue before publishing details.

## Scope

In scope:

- Vulnerabilities in code that ships in the `vmm-core`, `vmm-types`,
  `vmm-gui`, `vmm-cli`, and `vmm-api` crates
- Vulnerabilities in the build, packaging, or distribution paths shipped
  in this repository
- Vulnerabilities in the documented REST API surface
- Reports that an audited security mitigation (path validation, name
  validation, atomic file writes, mutex poisoning handling, etc.) can be
  bypassed

Out of scope for this repository:

- Vulnerabilities in QEMU, libvirt, the Linux kernel, OVMF, swtpm, or
  other upstream dependencies — please report those to the relevant
  upstream project. We are happy to help route the report if you are
  unsure where it belongs.
- Misconfiguration on the host (e.g., running libvirtd with unintended
  privileges) that is outside Libre VMM's control
- Findings that require an attacker to already have administrator
  access on the host machine

## Hardening Stance

Libre VMM is built with the following commitments. They are not promises
of perfection; they are the principles we hold ourselves to.

- **No telemetry, no phone-home, no automatic data collection.**
- **No silent privilege escalation.** Wizards generate commands for the
  user to run; the application does not modify system configuration
  files like `/etc/sudoers.d/` on its own.
- **Input validation at every boundary.** User-supplied names, paths,
  URIs, MAC addresses, and PCI identifiers are validated before they
  reach libvirt, virsh, or QEMU command lines. The codebase carries
  inline references to the CWE categories it defends against so a
  reviewer can audit each mitigation in context.
- **Atomic file writes** for security-relevant artifacts (encryption
  policies, restriction policies, configuration files).
- **Restrictive defaults** for files containing sensitive data: owner-
  only permissions on Unix, owner-only ACLs on Windows.
- **Memory hygiene** for secret material such as disk encryption
  passphrases.

The codebase is auditable. If you find a place where the implementation
does not match this stance, that is itself a finding we want to hear
about.

## Published Advisories

| ID | Date | Component | Severity | Status |
| -- | ---- | --------- | -------- | ------ |
| _(none for v0.1.0)_ | | | | |

This table will be populated as advisories are issued.
