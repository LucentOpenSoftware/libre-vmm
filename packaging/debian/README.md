# Building the libre-vmm .deb

Requires debhelper, cargo, rustc, libvirt-dev.

From the repo root:

    dpkg-buildpackage -b -us -uc

The .deb appears in the parent directory.

## Installing build dependencies

    sudo apt install debhelper cargo rustc libvirt-dev pkg-config libgtk-3-dev build-essential

## Quick install (after build)

    sudo apt install ../libre-vmm_0.1.0-1_amd64.deb

## Signed builds / repo uploads

Currently unsigned (`-us -uc`). For an official upload, sign with `debsign` and
push to a PPA (Launchpad) or a self-hosted `reprepro` mirror.
