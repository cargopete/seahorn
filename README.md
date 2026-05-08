# Seahorn

**A Solana data service for The Graph Protocol.**

Seahorn is a new indexing primitive — not a subgraph, not a Substreams module. It is a standalone Rust service that ingests Solana on-chain data, runs typed handler functions against it, and writes structured output to a sink. The third lane alongside Subgraphs (EVM) and Substreams (pure-compute).

---

## What it aims to deliver

- **Live Solana data on The Graph** — surfacing Solana program activity (instructions, account diffs) as queryable, structured data, eventually served through The Graph's Horizon protocol.
- **Rust-native DX** — handlers are plain Rust functions. No AssemblyScript, no protobuf IDL, no segment boundaries. You write `fn handle(event: &SubstrateEvent) -> ChangeSet` and the runtime does the rest.
- **Typed decoders for real programs** — Anchor IDL → typed Rust instruction structs via Codama/Carbon. Pump.fun, Raydium, Jupiter, Drift — decoded without boilerplate.
- **Pluggable substrates** — Firehose-Solana or Yellowstone gRPC behind a unified trait. Swap substrates without touching handler code.
- **Pluggable sinks** — stdout for dev, Postgres for v1, Kafka/ClickHouse later. The handler never touches storage directly.
- **Correct fork handling** — append-only writes with commitment markers (`NEW` / `UNDO` / `FINAL`), finalization sweeper. No silent state drift on reorgs.

---

## What it is not

- **Not a subgraph runtime.** No graph-node, no AssemblyScript ABI, no entity store, no GraphQL server (yet).
- **Not Substreams.** No pure-compute constraint, no protobuf-everywhere, no segment-parallel backfill.
- **Not multi-tenant in v0.5.** Self-host this scaffold against your own program. Multi-tenancy and Horizon integration are v1/v2.

---

## Current state (v0.5 — in progress)

### What exists

| Component | Status |
|---|---|
| `seahorn-core` | ✅ Core traits: `Substrate`, `Handler`, `Sink`, `ChangeSet`, `SubstrateEvent` |
| `seahorn-substrate-mock` | ✅ Synthetic Raydium swap stream for local dev |
| Yellowstone gRPC substrate | ✅ Live Solana data via Yellowstone gRPC (bring your own endpoint) |
| `RaydiumSwapHandler` | ✅ Example pure handler: `SubstrateEvent → ChangeSet` |
| `StdoutSink` | ✅ Prints changesets to terminal |

### What comes next

| Step | Component | What it delivers |
|---|---|---|
| 3 | First real decoder | Anchor IDL → typed Rust structs for Pump.fun |
| 4 | `PostgresSink` | Append-only writes, commitment markers, finalization sweeper |
| 5 | Docker compose | `docker compose up` → seahorn + postgres + postgrest |

---

## Architecture

```
Substrate (Firehose / Yellowstone / Mock)
    │
    │  SubstrateEvent { slot, signature, step, cursor, instructions }
    ▼
Handler (pure Rust fn — no I/O, deterministic)
    │
    │  ChangeSet { slot, step, cursor, changes: Vec<EntityChange> }
    ▼
Sink (StdoutSink / PostgresSink / KafkaSink)
```

The `Handler → ChangeSet` split is non-negotiable. It is what makes handlers deterministic, testable without a database, and eventually compatible with Proof-of-Indexing on The Graph's Horizon protocol.

---

## Running locally

```bash
# Synthetic data — no credentials needed
cargo run -- --mock

# Live Solana data (requires a Yellowstone gRPC endpoint)
cp .env.example .env
# edit .env: set YELLOWSTONE_ENDPOINT and YELLOWSTONE_TOKEN
cargo run
```

Output:
```
[slot  320000002] [NEW] Swap(aadCcd7KKNAx-320000002)  slot=320000002  SwapBaseIn  amount_in=433262389045  min_out=174095109124
[slot  320000005] [NEW] Swap(3BcckEbdZHhZ-320000005)  slot=320000005  SwapBaseIn  amount_in=326303790640  min_out=108688640659
```

---

## Workspace layout

```
seahorn/
├── crates/
│   ├── seahorn-core/            # Substrate, Handler, Sink traits + ChangeSet types
│   └── seahorn-substrate-mock/  # Synthetic Solana stream for local dev
└── src/main.rs                  # Runtime: wires substrate → handler → sink
```

---

## Yellowstone endpoints

The live substrate requires a Yellowstone gRPC endpoint. Options:

| Provider | Entry price | Notes |
|---|---|---|
| Chainstack | $49/mo | Marketplace → search "Yellowstone" |
| Triton Dragon's Mouth | PAYG, $125 deposit | `dragonmouth.triton.one` |
| Helius LaserStream | $499/mo (Business) | Sub-second latency |

---

## Substrate economics (operator target)

An operator running a Seahorn deployment needs:
- A Yellowstone gRPC subscription: ~$49–$499/month
- A compute host (8 vCPU, 32 GB RAM): ~$80–$150/month
- A Postgres instance (Hetzner AX42 class): included or ~$50/month additional

Total MVP opex: **~$200–$650/month per deployment.**
