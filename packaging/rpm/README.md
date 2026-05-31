# Building the libre-vmm .rpm

Requires `rpm-build`, `cargo`, `rust`, `libvirt-devel`, `gtk3-devel`.

## Local build

From the repo root:

    # Create source tarball
    mkdir -p ~/rpmbuild/SOURCES
    git archive --format=tar.gz --prefix=libre-vmm-0.1.0/ \
        -o ~/rpmbuild/SOURCES/libre-vmm-0.1.0.tar.gz HEAD

    # Build
    rpmbuild -ba packaging/rpm/libre-vmm.spec

The .rpm appears under `~/rpmbuild/RPMS/x86_64/`.

## Quick install (after build)

    sudo dnf install ~/rpmbuild/RPMS/x86_64/libre-vmm-0.1.0-1*.rpm

## Publishing to Copr

See `copr.yaml`. After creating the project, point Copr at the upstream Git
repo and the spec path `packaging/rpm/libre-vmm.spec`. Users then install via:

    sudo dnf copr enable libre-vmm/libre-vmm
    sudo dnf install libre-vmm

## Deferred / TODO

- Signed RPMs (`%_signature gpg` + `rpmsign`).
- COPR project registration.
- Bodhi push for Fedora repo inclusion (eventual goal).
