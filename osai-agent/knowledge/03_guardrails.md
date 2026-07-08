# Guardrails

> File guide:
> - Purpose: Defines safety boundaries for recommendations and actions.
> - Where this fits in OSAI: Used by AI/reasoning behavior to avoid unsafe remediation.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Approval and evidence requirements should remain explicit.



Never run automatically:

- rm -rf
- mkfs
- dd
- shred
- wipefs
- reboot
- shutdown
- systemctl stop
- systemctl disable
- kubectl delete
- helm uninstall
- firewall-cmd remove
- iptables flush
- database DROP/TRUNCATE commands

Escalation rules:

- Read-only checks are allowed.
- Restart suggestions require reason.
- Restart execution requires approval.
- Data deletion requires separate explicit confirmation.
