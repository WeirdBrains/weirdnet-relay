# weirdnet-relay RPM, built from the release tarball by packaging/rpm/build.sh:
#   rpmbuild -bb --target ARCH --define "ver X.Y.Z" --define "_sourcedir DIR" packaging/rpm/weirdnet-relay.spec
# The binary is prebuilt (SQLCipher and OpenSSL built in); nothing is compiled here. A
# pre-release X.Y.Z-alpha.N becomes RPM version X.Y.Z~alpha.N so it sorts before X.Y.Z.
%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
%global source_date_epoch_from_changelog 0

Name:           weirdnet-relay
Version:        %{lua: print((rpm.expand('%{ver}'):gsub('-', '~')))}
Release:        1
Summary:        Weirdnet relay and bootstrap node
License:        BSD-3-Clause
URL:            https://github.com/WeirdBrains/weirdnet-relay
Source0:        weirdnet-relay-%{ver}-%{_target_cpu}-linux.tar.gz
Requires:       glibc
Recommends:     systemd >= 250

%description
weirdnet-relay is a relay and bootstrap node for the Weirdnet network: a libp2p circuit
relay v2 with no caps by default, a Kademlia server, AutoNAT and hole-punch coordination,
a names server for signed peer records, and an info page. Its node key and state live in
a SQLCipher database keyed from a systemd credential. The service starts only once that
credential exists (see /usr/share/doc/weirdnet-relay/DEPLOY.md).

%prep
%setup -q -n weirdnet-relay-%{ver}-%{_target_cpu}-linux

%install
install -Dm755 weirdnet-relay %{buildroot}%{_bindir}/weirdnet-relay
install -Dm644 systemd/weirdnet-relay.service %{buildroot}%{_unitdir}/weirdnet-relay.service
install -Dm644 relay.toml %{buildroot}%{_sysconfdir}/weirdnet-relay/relay.toml
install -Dm644 examples/relay.toml %{buildroot}%{_docdir}/weirdnet-relay/examples/relay.toml

%files
%license LICENSE
%doc README.md DEPLOY.md CHANGELOG.md
%{_docdir}/weirdnet-relay/examples/relay.toml
%{_bindir}/weirdnet-relay
%{_unitdir}/weirdnet-relay.service
%dir %{_sysconfdir}/weirdnet-relay
%config(noreplace) %{_sysconfdir}/weirdnet-relay/relay.toml

%post
if [ -d /run/systemd/system ]; then
  systemctl daemon-reload >/dev/null 2>&1 || :
  if [ -f /etc/weirdnet-relay/db-key.cred ]; then
    if [ "$1" -eq 1 ]; then
      systemctl enable --now weirdnet-relay.service || :
    else
      systemctl try-restart weirdnet-relay.service || :
    fi
  fi
fi
if [ ! -f /etc/weirdnet-relay/db-key.cred ]; then
  cat <<MSG
weirdnet-relay is installed but not started: it needs its database key first.
  weirdnet-relay gen-key | sudo systemd-creds encrypt --name=db-key - /etc/weirdnet-relay/db-key.cred
  sudo \$EDITOR /etc/weirdnet-relay/relay.toml
  sudo systemctl enable --now weirdnet-relay
See /usr/share/doc/weirdnet-relay/DEPLOY.md.
MSG
fi

%preun
if [ "$1" -eq 0 ] && [ -d /run/systemd/system ]; then
  systemctl disable --now weirdnet-relay.service >/dev/null 2>&1 || :
fi

%postun
if [ -d /run/systemd/system ]; then
  systemctl daemon-reload >/dev/null 2>&1 || :
fi
