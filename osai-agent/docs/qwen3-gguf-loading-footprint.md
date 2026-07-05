# Accelerating Qwen3-4B GGUF Loading And Reducing Footprint

> File guide:
> - Purpose: Deployment guide for fast Qwen3 GGUF startup, mmap loading, and image-size tradeoffs.
> - Where this fits in OSAI: Explains the two supported model modes used by Docker Compose.
> - Topics to know: Markdown structure, OSAI architecture, Docker services, Cognee memory, and llama.cpp/Qwen inference.
> - Operational note: Do not confuse baked model image with source zip storage; the GGUF is a runtime/build artifact.


This project uses `Qwen3-4B-Q4_K_M.gguf` through llama.cpp. The model should be treated as a runtime artifact, not normal source code.

## Best Default For OSAI

Use this layout:

```text
osai-agent/
├── docker/llama/Dockerfile     runtime image only
├── docker/llama-model/         optional runtime image plus GGUF
├── docker-compose.storage.yml  mounts the model read-only
├── docker-compose.model-image.yml contains the model inside the image
└── models/
    └── Qwen3-4B-Q4_K_M.gguf    local runtime file, ignored by Git
```

This gives the best balance:

- Docker image stays small.
- No model download happens during container startup.
- Rebuilding Rust/Cognee/llama images does not recopy a multi-GB GGUF.
- llama.cpp can memory-map the local model file.
- The same model folder can be reused by Docker, systemd, or a direct llama.cpp binary.

## Two Supported Modes

### Mode A: Host-Mounted Model

Use this for development, single-server testing, and fast rebuilds:

```bash
docker compose -f docker-compose.storage.yml up -d --build
```

What happens:

- `docker/llama/Dockerfile` builds a small llama.cpp server image.
- `docker-compose.storage.yml` mounts `./models:/models:ro`.
- The command loads `/models/Qwen3-4B-Q4_K_M.gguf` with `--mmap`.
- Rebuilding the image does not copy the GGUF.

### Mode B: Docker Image Contains The Model

Use this when you want to push one complete inference image to a server:

```bash
./scripts/build-llama-model-image.sh
docker compose -f docker-compose.model-image.yml up -d --build
```

What happens:

- `docker/llama-model/Dockerfile` copies `models/Qwen3-4B-Q4_K_M.gguf` into the image.
- `docker/llama-model/Dockerfile.dockerignore` allows only that GGUF into this special build context.
- The image is tagged `osai-llama-qwen-with-model:local`.
- The container still runs llama.cpp with `--mmap`.
- There is no runtime model download and no host model mount.

Tradeoff: the image becomes roughly runtime size plus model size. That is good for repeatable deploys, but heavier for local rebuilds.

## What Actually Speeds Up Startup

| Lever | What It Improves | OSAI Recommendation |
|---|---|---|
| Local SSD/NVMe model path | First load and mmap page-in speed | Keep `models/` on local disk, not SMB/NFS. |
| `--mmap` | Avoids copying the whole model into anonymous RAM at load time | Keep enabled. It is the llama.cpp default, but compose makes it explicit. |
| Warm OS page cache | Second and later starts become much faster | Restarting the container can be quick if the host has not evicted the model pages. |
| `--mlock` | Prevents model pages from being swapped/evicted | Use only when the machine has enough RAM; add it to compose command when needed. |
| Smaller quant | Reduces disk and RAM footprint | Use Q4_K_M by default; test Q3/Q2 only when RAM is tight. |
| Smaller context | Reduces KV cache memory | Keep `-c 2048` or `4096` for normal ops questions; avoid 32K+ unless needed. |
| Cached model layer/registry | Avoids repeated downloads across machines | Useful for fleet rollout, but not required for a single server. |

## Recommended llama.cpp Command

The compose file starts llama.cpp like this:

```bash
llama-server \
  -m /models/Qwen3-4B-Q4_K_M.gguf \
  --mmap \
  --host 0.0.0.0 \
  --port 8080 \
  --alias osai-llm \
  -c 2048 \
  --parallel 1 \
  --threads 4 \
  --threads-batch 4
```

For a machine with enough RAM where you want the model to stay resident, add:

```bash
--mlock
```

If `--mlock` fails, remove it. That usually means the container or OS is not allowed to lock enough memory.

## Quantization Choice

Qwen's official GGUF repo lists Q4_K_M as about 2.5 GB for Qwen3-4B. That is the best default for OSAI because it keeps quality reasonable while staying small enough for normal servers.

Use this decision rule:

| Machine Constraint | Model Choice |
|---|---|
| Normal 8 GB+ RAM server | `Qwen3-4B-Q4_K_M.gguf` |
| Very low RAM / tiny disk | Test Q3 or Q2 quant from a trusted GGUF repo |
| Better quality and more RAM | Q5_K_M or Q6_K |
| Best quality, larger file | Q8_0 |

Do not assume a smaller quant is always better. It loads less data, but answer quality can drop. For a troubleshooting agent, wrong advice is more expensive than a few seconds of loading.

To test another GGUF already placed in `models/`, pass the filename:

```bash
OSAI_GGUF_MODEL_FILE=Qwen3-4B-IQ3_M.gguf docker compose -f docker-compose.storage.yml up -d --build

MODEL_FILE=Qwen3-4B-IQ3_M.gguf ./scripts/build-llama-model-image.sh
OSAI_GGUF_MODEL_FILE=Qwen3-4B-IQ3_M.gguf docker compose -f docker-compose.model-image.yml up -d --build
```

## Container Strategy

For one server:

1. Download the model once into `models/`.
2. Start compose.
3. Let llama.cpp use the local file.

For many servers:

1. Build the baked model image once with `./scripts/build-llama-model-image.sh`.
2. Push `osai-llama-qwen-with-model:local` to your private registry with your own tag.
3. Pull that image on each server.
4. Start with `docker-compose.model-image.yml`.

Avoid downloading the GGUF during every container start. That is slow and fragile.

## Operational Checks

```bash
ls -lh models/Qwen3-4B-Q4_K_M.gguf
docker compose -f docker-compose.storage.yml ps
docker logs osai-llama --tail 100
curl http://127.0.0.1:8080/health
curl http://127.0.0.1:8080/v1/models
free -m
df -h
```

## References Checked

- Qwen/Qwen3-4B-GGUF model card: https://huggingface.co/Qwen/Qwen3-4B-GGUF
- llama.cpp server options: https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md
- Docker Model Runner llama.cpp notes: https://docs.docker.com/ai/model-runner/inference-engines/
