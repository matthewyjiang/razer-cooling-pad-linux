# Fedora package for the GUI; the CLI and daemon are razer-coolingpad-linux.spec. Build from a
# checkout (needs network: cargo fetches Rune and crates.io dependencies):
#   git archive --prefix=razer-coolingpad-linux-gui-VERSION/ \
#     -o ~/rpmbuild/SOURCES/razer-coolingpad-linux-gui-VERSION.tar.gz HEAD
#   rpmbuild -bb gui/razer-coolingpad-linux-gui.spec
# .github/workflows/publish-fedora-package.yml does exactly this.

# The release profile strips the binary, so there is no debuginfo to split out.
%global debug_package %{nil}

Name:           razer-coolingpad-linux-gui
# x-release-please-start-version
Version:        0.3.0
# x-release-please-end
Release:        1%{?dist}
Summary:        Desktop GUI for the Razer Laptop Cooling Pad fan curve controller
License:        MIT
URL:            https://github.com/matthewyjiang/razer-cooling-pad-linux
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  libxkbcommon-devel
BuildRequires:  wayland-devel
BuildRequires:  desktop-file-utils
Requires:       razer-coolingpad-linux >= %{version}
Requires:       polkit
# winit and fontconfig are dlopened, so rpm's dependency generator can't see them.
Requires:       fontconfig
Requires:       libwayland-client
Requires:       libxkbcommon
Requires:       libxkbcommon-x11
Requires:       libX11
Requires:       libX11-xcb
Requires:       libXcursor
Requires:       libXi
Recommends:     vulkan-loader

%description
A small window for the Razer Laptop Cooling Pad daemon: live temperature, preset
picker and a draggable fan-curve editor. Changes are applied through
pkexec razer-coolingpad.

%prep
%autosetup

%build
cd gui
cargo build --release --locked

%install
install -Dm755 gui/target/release/razer-coolingpad-gui %{buildroot}%{_bindir}/razer-coolingpad-gui
install -Dm644 gui/razer-coolingpad-gui.desktop \
  %{buildroot}%{_datadir}/applications/razer-coolingpad-gui.desktop
install -Dm644 gui/razer-coolingpad-gui.svg \
  %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/razer-coolingpad-gui.svg

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/razer-coolingpad-gui.desktop

%files
%license LICENSE
%{_bindir}/razer-coolingpad-gui
%{_datadir}/applications/razer-coolingpad-gui.desktop
%{_datadir}/icons/hicolor/scalable/apps/razer-coolingpad-gui.svg
