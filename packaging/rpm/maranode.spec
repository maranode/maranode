Name:           maranode
Version:        %{version}
Release:        1%{?dist}
Summary:        Private, air-gapped AI inference runtime
License:        Apache-2.0
URL:            https://maranode.com
Source0:        maranode-%{version}-%{_arch}-unknown-linux-gnu.tar.gz

BuildArch:      x86_64 aarch64
ExclusiveOS:    linux

Requires:       libstdc++ >= 11
Requires:       libgomp
Requires(pre):  shadow-utils
Requires(post): systemd
Requires(preun): systemd
Requires(postun): systemd

%description
Maranode runs large language models entirely on-premises.
No data leaves the machine. Provides a REST API compatible
with the OpenAI chat completions format, a web UI, RAG
support, and a tamper-evident audit log.

%prep
%setup -q -n maranode-%{version}-%{_arch}-unknown-linux-gnu

%install
install -D -m 0755 maranoded  %{buildroot}%{_bindir}/maranoded
install -D -m 0755 maranode   %{buildroot}%{_bindir}/maranode

install -D -m 0644 /dev/stdin %{buildroot}%{_unitdir}/maranoded.service <<'EOF'
[Unit]
Description=Maranode AI Inference Daemon
Documentation=https://github.com/maranode/maranode
After=network.target
ConditionPathExists=%{_bindir}/maranoded

[Service]
Type=simple
User=maranode
Group=maranode
ExecStart=%{_bindir}/maranoded
Restart=on-failure
RestartSec=5
TimeoutStopSec=30
NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
ProtectSystem=strict
ProtectHome=true
ProtectKernelTunables=true
ProtectControlGroups=true
RestrictNamespaces=true
RestrictRealtime=true
LockPersonality=true
ReadWritePaths=/var/lib/maranode /var/log/maranode

[Install]
WantedBy=multi-user.target
EOF

install -d -m 0750 %{buildroot}/var/lib/maranode
install -d -m 0750 %{buildroot}/var/log/maranode

%pre
getent group  maranode >/dev/null || groupadd -r maranode
getent passwd maranode >/dev/null || \
    useradd -r -g maranode -d /var/lib/maranode \
            -s /sbin/nologin -c "Maranode daemon" maranode
exit 0

%post
%systemd_post maranoded.service

%preun
%systemd_preun maranoded.service

%postun
%systemd_postun_with_restart maranoded.service

%files
%license LICENSE
%{_bindir}/maranoded
%{_bindir}/maranode
%{_unitdir}/maranoded.service
%attr(750, maranode, maranode) %dir /var/lib/maranode
%attr(750, maranode, maranode) %dir /var/log/maranode

%changelog
* Thu Jun 11 2026 ondercsn <ondercsn@users.noreply.github.com> - 0.1.0-1
- Initial RPM package
