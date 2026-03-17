# skill-sidecar

## What's this

`skill-sidecar` is a sandboxed skill execution framework for AI agents. It runs as a sidecar container alongside your agent, exposing a high-performance **Rust HTTP API** that executes CLI tools and scripts on the agent's behalf.

The agent never touches credentials or CLI tools directly — it just calls `POST /skill/<name>`.

## Why should I care

Without a sidecar, your agent container needs every CLI tool, secret, and credential baked in. That means:

- A bloated image that's hard to update
- Credentials co-located with untrusted LLM-generated code
- No isolation between the reasoning layer and the execution layer

With `skill-sidecar`:

```
┌─────────────────────────┐         ┌──────────────────────────────────────┐
│   Agent Container       │         │   Sidecar Container (Rust)           │
│                         │         │                                      │
│  ┌───────────────────┐  │  POST   │  ┌────────────────────────────────┐  │
│  │ Agent Logic       │──┼────────▶│  │ Rust HTTP Server (Axum/Tokio)  │  │
│  │                   │  │         │  └───────────────┬────────────────┘  │
│  │ curl localhost    │  │         │                  │ execve            │
│  │   :8080/skill/    │  │  JSON   │  ┌───────────────▼────────────────┐  │
│  │   <name>          │◀─┼─────────│  │ /usr/local/bin/                │  │
│  └───────────────────┘  │         │  │  ├── gh                        │  │
│                         │         │  │  ├── aws                       │  │
│  ✗ no credentials       │         │  │  ├── my-script.sh              │  │
│  ✗ no CLI tools         │         │  │  └── my-binary                 │  │
│                         │         │                                      │
└─────────────────────────┘         │  ┌────────────────────────────────┐  │
                                    │  │ Credentials (env / secrets)    │  │
                                    │  │  ├── GH_TOKEN                  │  │
                                    │  │  ├── AWS_*                     │  │
                                    │  │  └── ...                       │  │
                                    │  └────────────────────────────────┘  │
                                    └──────────────────────────────────────┘
```

## How it works

`skill-sidecar` is a **base image** — it ships no skill binaries. You bring your own:

```
┌─────────────────────────────────┐
│   skill-sidecar (base image)    │
│                                 │
│   • Rust HTTP server            │
│   • /usr/local/bin/  (empty)    │
└────────────────┬────────────────┘
                 │ FROM skill-sidecar
                 ▼
┌─────────────────────────────────┐
│   your-sidecar (your image)     │
│                                 │
│   COPY gh        /usr/local/bin/│
│   COPY aws       /usr/local/bin/│
│   COPY my-script /usr/local/bin/│
└─────────────────────────────────┘
```

Any executable in `/usr/local/bin/` is automatically available for dispatch.

### API

```
POST /skill/<name>   # execute a skill
GET  /task/<id>      # poll async task result
GET  /healthz        # health check
```

**Request:**
```json
{
  "args":    ["arg1", "arg2"],
  "env":     { "SKILL_FOO": "bar" },
  "stdin":   "optional input",
  "timeout": 30
}
```

- `env` — only `SKILL_*` keys accepted; others rejected with `400`
- `timeout` — default 30s, max 300s; requests > 30s return a `task_id` for async polling
- Request body capped at **1 MB**

**Response (sync):**
```json
{ "status": "ok", "stdout": "...", "stderr": "...", "exit_code": 0 }
```

**Response (async):**
```json
{ "status": "pending", "task_id": "550e8400-e29b-41d4-a716-446655440000" }
```

### Security

| Concern | Mitigation |
|---------|------------|
| Credential hijack via `env` | Only `SKILL_*` keys accepted |
| Command injection via `args` | `execve` directly — no shell |
| Unauthenticated callers | `X-Skill-Token` shared-secret header |
| Resource exhaustion | 300s timeout cap; 1 MB body limit |
| Task enumeration | Task IDs are UUID v4 |

### Implementation

- **Rust** + [Axum](https://github.com/tokio-rs/axum) + [Tokio](https://tokio.rs) — async, single static binary
- Binds `127.0.0.1:8080` only — not exposed outside the pod
- Completed tasks expire after 1 hour; background reaper runs every 10 minutes

## Helm

```bash
helm repo add skill-sidecar https://thepagent.github.io/skill-sidecar
helm repo update
helm install my-agent skill-sidecar/skill-sidecar

# Use your own image (e.g. with yt-dlp bundled)
helm install my-agent skill-sidecar/skill-sidecar \
  --set image.repository=myrepo/my-sidecar \
  --set image.tag=latest \
  --set env.GH_TOKEN=xxx \
  --set skillToken=my-secret
```

## Examples

| Example | Description |
|---------|-------------|
| [`examples/with-yt-dlp`](examples/with-yt-dlp) | Base image + yt-dlp |
| [`examples/with-gh`](examples/with-gh) | Base image + GitHub CLI |

## Structure

```
skill-sidecar/
├── src/           # Rust HTTP server (Axum)
├── examples/      # Sample custom images
├── charts/        # Helm chart
└── Dockerfile
```
