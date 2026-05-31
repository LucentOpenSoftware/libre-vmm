# Submitting libre-vmm to the AUR

This directory contains a `PKGBUILD` and a stub `.SRCINFO` for the
Arch User Repository.

## Local build / install

From this directory:

    makepkg -si

`makepkg` will fetch the source, build with cargo, and install via pacman.

## Regenerating .SRCINFO

After editing the PKGBUILD:

    makepkg --printsrcinfo > .SRCINFO

## Submitting to AUR

1. Create an AUR account and add an SSH key.
2. Clone the empty AUR repo:

       git clone ssh://aur@aur.archlinux.org/libre-vmm.git aur-libre-vmm

3. Copy `PKGBUILD` and `.SRCINFO` into it.
4. Commit and push:

       git add PKGBUILD .SRCINFO
       git commit -m "Initial upload — libre-vmm 0.1.0"
       git push

5. End users then install with `yay -S libre-vmm` (or `paru`, `pikaur`, etc.).

## Deferred / TODO

- Replace `sha256sums=('SKIP')` with a real hash once a v0.1.0 release tag
  exists on GitHub.
- Run `namcap PKGBUILD libre-vmm-*.pkg.tar.zst` before publishing.
- Optionally add a `libre-vmm-git` PKGBUILD for nightly users.
