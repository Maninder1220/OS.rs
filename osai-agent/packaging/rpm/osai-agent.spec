# =============================================================================
# File: packaging/rpm/osai-agent.spec
# Purpose:
#   RPM package specification for installing OSAI binaries, config, and systemd service files.
#
# Where this fits in OSAI:
#   Used by scripts/build-rpm.sh to produce an installable package.
#
# Topics to know before editing:
#   RPM packaging, systemd install hooks, and Linux package layout.
#
# Important operational notes:
#   Package paths must stay aligned with systemd unit paths and release binary names.
# =============================================================================
Name:           osai-agent
Version:        0.2.0
Release:        1%{?dist}
Summary:        Rust-first local OS AI agent with guarded actions
License:        MIT
URL:            https://example.invalid/osai-agent
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
Requires(pre):  shadow-utils

%description
OSAI Agent scans Linux hosts, stores local scan history, evaluates rules, reasons over Markdown runbooks, and exposes guarded command execution behind an approval workflow.

%prep
%autosetup

%build
cargo build --release

%install
install -D -m 0755 target/release/osai-agent %{buildroot}%{_bindir}/osai-agent
install -D -m 0644 packaging/systemd/osai-agent.service %{buildroot}%{_unitdir}/osai-agent.service
install -D -m 0644 packaging/systemd/osai-agent.env %{buildroot}%{_sysconfdir}/osai-agent/osai-agent.env
mkdir -p %{buildroot}%{_sysconfdir}/osai-agent/knowledge
cp -a knowledge/*.md %{buildroot}%{_sysconfdir}/osai-agent/knowledge/
install -D -m 0644 README.md %{buildroot}%{_docdir}/osai-agent/README.md
mkdir -p %{buildroot}%{_sharedstatedir}/osai-agent

%pre
getent group osai >/dev/null || groupadd -r osai
getent passwd osai >/dev/null || useradd -r -g osai -d %{_sharedstatedir}/osai-agent -s /sbin/nologin osai
exit 0

%post
%systemd_post osai-agent.service

%preun
%systemd_preun osai-agent.service

%postun
%systemd_postun_with_restart osai-agent.service

%files
%license LICENSE
%doc %{_docdir}/osai-agent/README.md
%{_bindir}/osai-agent
%{_unitdir}/osai-agent.service
%config(noreplace) %{_sysconfdir}/osai-agent/osai-agent.env
%config(noreplace) %{_sysconfdir}/osai-agent/knowledge/*.md
%dir %attr(0750,osai,osai) %{_sharedstatedir}/osai-agent

%changelog
* Tue Jun 30 2026 OSAI <osai@example.invalid> - 0.2.0-1
- Add scan history, rule engine, Markdown reasoning, guarded actions, plugins, systemd, and RPM skeleton.
