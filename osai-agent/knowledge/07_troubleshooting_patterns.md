# Troubleshooting Patterns

> File guide:
> - Purpose: Defines repeated troubleshooting patterns across Linux, services, and infra.
> - Where this fits in OSAI: Provides reusable mental models for reasoning responses.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Patterns should guide investigation, not replace current scan evidence.



Pattern: high memory
- Find top memory processes.
- Check whether a known heavy service started unexpectedly.
- Check swap usage.
- Check OOM messages.

Pattern: high disk
- Identify mount point.
- Check logs, container images, database files, backups.
- Do not delete automatically.

Pattern: port conflict
- Identify process listening on port.
- Compare with expected app.
- Suggest config change only after confirming owner.

Pattern: Kubernetes pod stuck
- Check pod state.
- Check events.
- Check node pressure.
- Check image pull.
- Check PVC.
