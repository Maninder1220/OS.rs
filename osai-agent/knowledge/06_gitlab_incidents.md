# GitLab Incidents

> File guide:
> - Purpose: Captures GitLab incident patterns relevant to OSAI troubleshooting.
> - Where this fits in OSAI: Supplements GitLab plugin hints and Ask OSAI memory.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Keep incident guidance evidence-based and avoid blind restarts.



Known incident memory:

Incident:
GitLab services were running automatically on Red Hat after reboot.

Fix:
Stopped GitLab using gitlab-ctl stop, stopped gitlab-runsvdir, and disabled gitlab-runsvdir from systemd boot.

Result:
Server CPU and RAM usage became low.

Rule:
If GitLab is detected and memory/CPU is high, check whether GitLab auto-started again before suggesting unrelated fixes.
