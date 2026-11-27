# Fedora package; mirrors the files PKGBUILD installs. Build from a checkout:
#   git archive --prefix=razer-coolingpad-linux-VERSION/ \
#     -o ~/rpmbuild/SOURCES/razer-coolingpad-linux-VERSION.tar.gz HEAD
#   rpmbuild -bb razer-coolingpad-linux.spec
# .github/workflows/publish-fedora-package.yml does exactly this.

Name:           razer-coolingpad-linux
# x-release-please-start-version
Version:        0.3.0
# x-release-please-end
Release:        1%{?dist}
Summary:        Fan curve controller for the Razer Laptop Cooling Pad
License:        MIT
URL:            https://github.com/matthewyjiang/razer-cooling-pad-linux
Source0:        %{name}-%{version}.tar.gz
BuildArch:      noarch

BuildRequires:  systemd-rpm-macros
Requires:       python3
Requires:       python3-hidapi
Requires:       systemd

%description
Temperature-driven fan curve daemon and CLI for the Razer Laptop Cooling Pad
(USB 1532:0f43). Ships a systemd service, a udev rule, a default config, and
preset curves.

%prep
%autosetup

%build
# Nothing to build: a single Python script plus data files.

%install
install -Dm755 razer-coolingpad %{buildroot}%{_bindir}/razer-coolingpad
install -Dm644 razer-coolingpad.service %{buildroot}%{_unitdir}/razer-coolingpad.service
install -Dm644 99-razer-coolingpad.rules %{buildroot}%{_udevrulesdir}/99-razer-coolingpad.rules
install -Dm644 config.json %{buildroot}%{_sysconfdir}/%{name}/config.json
install -Dm644 -t %{buildroot}%{_datadir}/%{name}/curves \
  curves/balanced.json curves/performance.json curves/silent.json

%post
%systemd_post razer-coolingpad.service
# Apply the udev rule to an already-connected pad (same as the Arch .install).
if command -v udevadm >/dev/null 2>&1; then
  udevadm control --reload-rules >/dev/null 2>&1 || :
  udevadm trigger --subsystem-match=hidraw >/dev/null 2>&1 || :
fi

%preun
%systemd_preun razer-coolingpad.service

%postun
%systemd_postun_with_restart razer-coolingpad.service

%files
%license LICENSE
%doc README.md
%{_bindir}/razer-coolingpad
%{_unitdir}/razer-coolingpad.service
%{_udevrulesdir}/99-razer-coolingpad.rules
%dir %{_sysconfdir}/%{name}
%config(noreplace) %{_sysconfdir}/%{name}/config.json
%{_datadir}/%{name}/
