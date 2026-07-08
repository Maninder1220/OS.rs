# Allowed Commands

> File guide:
> - Purpose: Lists command families considered safe or expected for operator workflows.
> - Where this fits in OSAI: Supports action guardrails and recommended checks.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Allowed command text is guidance, not permission to execute without approval.



Phase 1: no command execution.

Future command execution rules:

- No free-form shell strings.
- No command chaining.
- No pipes in autonomous mode.
- No destructive action without approval.
- Every command must have timeout.
- Every command must be logged.

Example future read-only commands:

- systemctl status <service>
- journalctl -u <service> --since <time>
- df -h
- free -m
- ss -tulpen
- kubectl get pods -A
- kubectl describe pod <pod> -n <namespace>
