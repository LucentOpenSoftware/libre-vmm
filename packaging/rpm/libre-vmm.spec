Name:           libre-vmm
Version:        0.1.0
Release:        1%{?dist}
Summary:        Libre VMM — A libre alternative to VMware Workstation

License:        GPL-3.0-or-later
URL:            https://github.com/LucentOpenSoftware/libre-vmm
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo, rust, libvirt-devel, pkgconfig, gtk3-devel
Requires:       qemu-kvm, libvirt, libvirt-daemon-driver-qemu, libvirt-client, edk2-ovmf, swtpm, swtpm-tools, spice-gtk
Recommends:     virt-manager
Suggests:       looking-glass-client

%description
A QEMU/KVM-based virtual machine manager with a modern egui-based GUI,
REST API, CLI, and 24-architecture support. Built in Rust for security
and reliability.

Features:
- Live migration between hosts (VMware Workstation can't)
- GPU passthrough (single and multi-GPU)
- LUKS-encrypted disks, TPM 2.0 emulation
- Per-VM nftables firewall rules
- Cloud-init / autounattend.xml unattended installs
- Native Wayland support
- Import from VMware (.vmx), VirtualBox (.vbox), Libvirt XML, Quickemu (.conf)

%prep
%autosetup

%build
cargo build --release --workspace

%install
install -Dm755 target/release/vmm-gui %{buildroot}%{_bindir}/libre-vmm
install -Dm755 target/release/vmm-cli %{buildroot}%{_bindir}/vmm
install -Dm755 target/release/vmm-api %{buildroot}%{_bindir}/vmm-api
install -Dm644 libre-vmm.desktop %{buildroot}%{_datadir}/applications/libre-vmm.desktop
if [ -f assets/icon.png ]; then
    install -Dm644 assets/icon.png %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/libre-vmm.png
fi

%post
systemctl enable --now libvirtd 2>/dev/null || true
echo "Libre VMM installed. To run VMs without sudo:"
echo "  sudo usermod -aG libvirt,kvm \$USER"
echo "  Log out and back in for group membership to take effect."

%files
%license LICENSE
%{_bindir}/libre-vmm
%{_bindir}/vmm
%{_bindir}/vmm-api
%{_datadir}/applications/libre-vmm.desktop
%{_datadir}/icons/hicolor/256x256/apps/libre-vmm.png

%changelog
* Thu Jun 01 2026 Libre VMM Contributors <noreply@github.com> - 0.1.0-1
- Initial release.
