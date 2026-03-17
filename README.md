# skill-sidecar

A **sandboxed skill execution framework** for [OpenClaw](https://github.com/openclaw/openclaw) agent, powered by a high-performance **Rust HTTP API** in the sidecar container.

## Concept

The agent calls skills via HTTP POST instead of local `exec`. The sidecar is implemented in Rust for minimal latency and maximum throughput.

```
┌─────────────────────────┐         ┌──────────────────────────────────────┐
│   Agent Container       │         │   Sidecar Container (Rust)           │
│                         │         │                                      │
│  ┌───────────────────┐  │  POST   │  ┌────────────────────────────────┐  │
│  │ Agent Logic       │──┼────────▶│  │ Rust HTTP Server (Axum/Tokio)  │  │
│  │                   │  │         │  └───────────────┬────────────────┘  │
│  │ curl localhost    │  │         │                  │ exec              │
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

## Base Image vs Your Image

`skill-sidecar` ships **no skill binaries**. It is a base image — you bring your own tools:

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

Any executable placed in `/usr/local/bin/` is automatically available for skill dispatch.

## API

```
POST /skill/<name>   # execute a skill
GET  /task/<id>      # poll async task result
GET  /healthz        # health check
```

### Request

```json
{
  "args":    ["arg1", "arg2"],
  "env":     { "SKILL_FOO": "bar" },
  "stdin":   "optional input",
  "timeout": 30
}
```

- `args` — passed directly to `/usr/local/bin/<name>` via `execve` (no shell)
- `env` — **only `SKILL_*` keys accepted**; others are rejected with `400`
- `stdin` — optional; max 1 MB
- `timeout` — seconds; default 30, server-side cap 300

### Response (sync)

```json
{
  "status":    "ok" | "error",
  "stdout":    "...",
  "stderr":    "...",
  "exit_code": 0
}
```

### Response (async, long-running)

```json
{
  "status":  "pending",
  "task_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

Poll result via `GET /task/<task_id>`.

## Security

| Concern | Mitigation |
|---------|------------|
| Credential hijack via `env` | Only `SKILL_*` env keys accepted; all others rejected |
| Command injection via `args` | Executed with `execve` directly, never via shell |
| Unauthenticated callers | `X-Skill-Token` shared-secret header (injected via downward API) |
| Resource exhaustion | Server-side timeout cap (300s); max request body 1 MB |
| Task enumeration | Task IDs are UUID v4 |

## Design Principles

- Sidecar binds `127.0.0.1` only — not exposed outside the pod
- Agent container holds **no** credentials or CLI tools
- Long-running skills are async: POST returns `task_id`, poll via `GET /task/<id>`

## Why Rust

- Minimal overhead per skill invocation
- Memory-safe sandboxed execution
- Single static binary — easy to containerize
- Async-first with [Tokio](https://tokio.rs) + [Axum](https://github.com/tokio-rs/axum)

## Structure

```
skill-sidecar/
├── src/           # Rust HTTP server (Axum)
└── Dockerfile
```
