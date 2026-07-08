# Kubernetes Runbook

> File guide:
> - Purpose: Provides safe Kubernetes inspection guidance.
> - Where this fits in OSAI: Supplements Kubernetes plugin hints in operator answers.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Prefer read-only kubectl commands unless explicit approval exists.



First checks:

1. Is kubelet running?
2. Is container runtime running?
3. Are nodes Ready?
4. Are pods Pending, CrashLoopBackOff, ImagePullBackOff, or Evicted?
5. Are there disk, memory, or CPU pressure conditions?
6. Are CNI pods healthy?
7. Are DNS pods healthy?

Safe read-only commands for future action mode:

- kubectl get nodes -o wide
- kubectl get pods -A -o wide
- kubectl describe node <node>
- kubectl describe pod <pod> -n <namespace>
- kubectl logs <pod> -n <namespace> --previous
