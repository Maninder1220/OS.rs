# Linux Runbook

> File guide:
> - Purpose: Provides safe Linux host troubleshooting guidance.
> - Where this fits in OSAI: Supplements scanner findings with operator next steps.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Commands should be diagnostic by default and destructive only with approval.



First checks:

1. CPU: high usage, load average, runaway process.
2. Memory: used memory, swap, OOM signals.
3. Disk: full filesystem, inode pressure.
4. Network: listening ports, failed DNS, firewall.
5. Services: systemd failed units.
6. Logs: recent critical journal messages.
7. Security: failed SSH, sudo usage, SELinux denials.

Safe order:

1. Observe.
2. Compare with previous snapshot.
3. Identify likely cause.
4. Suggest least-risk check.
5. Ask before changing state.
