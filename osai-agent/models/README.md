# Local Model Folder

> File guide:
> - Purpose: Explains how local GGUF model files should be placed and used by OSAI.
> - Where this fits in OSAI: Guides both host-mounted and baked-image Qwen deployment modes.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Models are runtime artifacts. They belong in ./models locally, not in Git or ordinary source zips.



Place the llama.cpp GGUF model here:

```bash
models/Qwen3-4B-Q4_K_M.gguf
```

The Docker compose file mounts this folder into the llama.cpp container and starts the server with:

```bash
/models/Qwen3-4B-Q4_K_M.gguf
```

The model binary is not bundled in this project zip unless it exists locally before packaging. It is a large runtime artifact, not Rust source code.

## Fast Loading Notes

- Keep the GGUF on local SSD/NVMe storage when possible.
- Avoid network shares for the active model path.
- The compose file mounts this folder read-only and starts llama.cpp with explicit `--mmap`.
- Use `Q4_K_M` as the normal balance of size and answer quality.
- Test Q3/Q2 only when RAM or disk is tight, because smaller files can reduce answer quality.
- Keep context modest (`-c 2048` or `4096`) for normal OSAI troubleshooting questions.

## Building An Image That Contains The Model

When `models/Qwen3-4B-Q4_K_M.gguf` exists locally, you can build a complete llama.cpp image:

```bash
./scripts/build-llama-model-image.sh
```

Then run the model-image compose stack:

```bash
docker compose -f docker-compose.model-image.yml up -d --build
```

This copies the GGUF into the image as `/models/Qwen3-4B-Q4_K_M.gguf`. The container still uses llama.cpp `--mmap`; the difference is that deployment no longer needs a host model mount or a runtime model download.
