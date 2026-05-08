# Seahorn

**A Solana data service for The Graph Protocol.**

Seahorn is a new indexing primitive — not a subgraph, not a Substreams module. It is a standalone Rust service that ingests Solana on-chain data, runs typed handler functions against it, and writes structured output to a sink. The third lane alongside Subgraphs (EVM) and Substreams (pure-compute).

---

## What it aims to deliver

- **Live Solana data on The Graph** — surfacing Solana program activity (instructions, account diffs) as queryable, structured data, eventually served through The Graph's Horizon protocol.
- **Rust-native DX** — handlers are plain Rust functions. No AssemblyScript, no protobuf IDL, no segment boundaries. You write `fn handle(event: &SubstrateEvent) -> ChangeSet` and the runtime does the rest.
- **Typed decoders for real programs** — Anchor discriminators + Borsh decode → typed Rust instruction structs. Pump.fun, Raydium, Jupiter, Drift — decoded without boilerplate.
- **Pluggable substrates** — Firehose-Solana or Yellowstone gRPC behind a unified `Substrate` trait. Swap substrates without touching handler code.
- **Pluggable sinks** — `StdoutSink` for dev, `PostgresSink` for v1, Kafka/ClickHouse later. The handler never touches storage directly.
- **Correct fork handling** — append-only writes with commitment markers (`NEW` / `UNDO` / `FINAL`), finalization sweeper, cursor persistence. No silent state drift on reorgs.

---

## What it is not

- **Not a subgraph runtime.** No graph-node, no AssemblyScript ABI, no entity store, no GraphQL server (yet).
- **Not Substreams.** No pure-compute constraint, no protobuf-everywhere, no segment-parallel backfill.
- **Not multi-tenant in v0.5.** Self-host this scaffold against your own program. Multi-tenancy and Horizon integration are v1/v2.

---

## Architecture

```
Substrate  (Firehose-Solana / Yellowstone gRPC / Mock)
    │
    │  SubstrateEvent { slot, signature, step, cursor, instructions }
    ▼
Handler  (pure Rust fn — no I/O, deterministic)
    │
    │  ChangeSet { slot, step, cursor, changes: Vec<EntityChange> }
    ▼
Sink  (StdoutSink / PostgresSink)
    │
    ├─ INSERT entity_changes (commitment_status = NEW | UNDO)
    ├─ UPSERT cursors  ← atomic with entity writes
    └─ Sweeper task: getSlot(finalized) → UPDATE status = FINAL
```

The `Handler → ChangeSet` split is non-negotiable. It is what makes handlers deterministic, testable without a database, and eventually compatible with Proof-of-Indexing on The Graph's Horizon protocol.

---

## Current state

| Crate | Status | What it does |
|---|---|---|
| `seahorn-core` | ✅ | `Substrate`, `Handler`, `Sink` traits + `ChangeSet`, `SubstrateEvent`, `Step`, `Cursor`, `Value` types |
| `seahorn-substrate-mock` | ✅ | `PumpfunMockSubstrate` — synthetic buy/sell/create stream, no credentials needed |
| `seahorn-handler-pumpfun` | ✅ | Anchor discriminators via SHA-256, Borsh decode, typed `Buy` / `Sell` / `Create` structs |
| `seahorn-sink-postgres` | ✅ | Append-only writes, JSONB fields, cursor persistence, finalization sweeper |
| `seahorn-handler-raydium` | ✅ | Raydium CLMM — swap, openPosition, addLiquidity, removeLiquidity (+ v2 variants) |
| `seahorn-handler-jupiter` | ✅ | Jupiter v6 — shared_accounts_route, exact_out_route, route_plan skip table (40+ Swap variants) |
| `MultiHandler` (in core) | ✅ | Run any combination of handlers against the same stream simultaneously |
| `StdoutSink` (in runtime) | ✅ | Pretty-prints each `ChangeSet` to terminal |
| Yellowstone gRPC substrate (in runtime) | ✅ | Live data via any Yellowstone endpoint, multi-program filter |
| Docker Compose | ✅ | `postgres` + `postgrest` — REST API at `localhost:3000` |

### What comes next

| Step | What it delivers |
|---|---|
| Horizon integration | `SolanaDataService.sol` + `seahorn-gateway` — TAP v2 payments, GRT settlement, provider registration |
| More programs | Orca Whirlpools, Drift, Marginfi via the same `Handler` trait |

---

## Running locally

### stdout (no dependencies)

```bash
cargo run -- --mock                    # Pump.fun
cargo run -- --mock --raydium          # Raydium CLMM
cargo run -- --mock --jupiter          # Jupiter v6
cargo run -- --mock --all              # all three simultaneously
```

```
[slot    320000001] [NEW  ] 🟢 Buy    mint=So111111…  user=7GGYZKiR…  tokens=   32814388798  sol=0.5353
[slot    320000004] [NEW  ] 🔴 Sell   mint=7dHbWXmc…  user=2CwSqTNe…  tokens=    1452378735  sol=0.4344
[slot    320000010] [NEW  ] ✨ Create mint=9mRt3xKp…  name=PumpMoon    sym=PMOON  creator=3aKw7xPq…
```

### Postgres + PostgREST (full stack)

```bash
# 1. Start postgres + postgrest
docker compose up -d

# 2. Configure
cp .env.example .env
# Set DATABASE_URL (already matches docker-compose defaults)
# Set SOLANA_RPC_URL to enable the finalization sweeper (optional)

# 3. Run with mock data
cargo run -- --mock --postgres

# 4. Query via PostgREST
curl 'http://localhost:3000/entity_changes?entity_type=eq.Buy&order=slot.desc&limit=10'
```

Apply typed views for cleaner queries:

```bash
psql $DATABASE_URL -f docker/views.sql
curl 'http://localhost:3000/buys?commitment_status=eq.NEW&order=slot.desc'
```

### Live Solana data

```bash
cp .env.example .env
# Set YELLOWSTONE_ENDPOINT and YELLOWSTONE_TOKEN
cargo run -- --postgres
```

---

## Workspace layout

```
seahorn/
├── crates/
│   ├── seahorn-core/              # Core traits, types, MultiHandler
│   ├── seahorn-substrate-mock/    # Mock substrates (Pump.fun, Raydium CLMM, Jupiter v6)
│   ├── seahorn-handler-pumpfun/   # Pump.fun decoder (Anchor + Borsh)
│   ├── seahorn-handler-raydium/   # Raydium CLMM decoder (swap, position, liquidity)
│   ├── seahorn-handler-jupiter/   # Jupiter v6 decoder (route_plan skip table)
│   └── seahorn-sink-postgres/     # PostgresSink with cursor persistence + sweeper
├── docker/
│   ├── init.sql                   # PostgREST web_anon role
│   └── views.sql                  # Typed views: buys, sells, creates
├── docker-compose.yml
└── src/main.rs                    # Runtime: wires substrate → handler → sink
```

---

## Yellowstone endpoints

The live substrate requires a Yellowstone gRPC endpoint.

| Provider | Entry price | Notes |
|---|---|---|
| Chainstack | $49/mo | Marketplace → search "Yellowstone" |
| Triton Dragon's Mouth | PAYG, $125 deposit | production-grade, cursor resume |
| Helius LaserStream | $499/mo (Business) | sub-second latency, 24h replay |

---

## Operator economics (target)

| Item | Cost |
|---|---|
| Yellowstone gRPC subscription | $49–$499/mo |
| Compute host (8 vCPU, 32 GB) | $80–$150/mo |
| Postgres (Hetzner AX42 class) | $50/mo |
| **Total MVP** | **~$200–$700/mo** |
