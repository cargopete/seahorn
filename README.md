# Seahorn

**Solana structured data service on The Graph Protocol's Horizon payment network.**

Seahorn indexes Solana program activity into typed, fork-correct, queryable entities and gates access via TAP v2 micropayments. It is the missing third lane alongside Subgraphs (EVM entities) and Substreams (pure-compute transforms): a Rust-native service that turns raw Solana instructions into structured data, served over a REST API and paid for per-query in GRT.

> **Disclaimer:** Seahorn is an experimental, community-led project. It is not endorsed by, affiliated with, or supported by the Graph Foundation or Edge & Node.

---

## Deployment

### SolanaDataService.sol — Arbitrum One (mainnet)

| | |
|---|---|
| **Proxy** | `0xdDE3F913cb6D1332Bc018Eb63647020a87dD7B37` |
| **Implementation** | `0x745af998718A64c1007a3D96b21cEE021CfB7599` |
| **Owner** | `0x20E59D8F41c9233B2108B10657aF5B2F8B7689A1` |
| **Horizon Controller** | `0x0a8491544221dd212964fbb96487467291b2C97e` |
| **GraphTallyCollector** | `0x8f69F5C07477Ac46FBc491B1E6D91E2bb0111A9e` |
| **Fee: burn** | 1% per collect() |
| **Fee: data service** | 1% per collect() |
| **Min provision** | 555 GRT |
| **Programs allowlisted** | Pump.fun · Raydium CLMM · Jupiter v6 |

### Status

| Component | Status |
|---|---|
| `SolanaDataService.sol` on Arbitrum One | **Live** |
| Foundry tests (37 tests) | **Passing** |
| Rust handlers: Pump.fun, Raydium CLMM, Jupiter v6 | **Working** |
| PostgresSink (fork-correct, cursor resume, finalization sweeper) | **Working** |
| seahorn-gateway (TAP v2 validation, RAV aggregation, rate limiting) | **Working** |
| PostgresSink integration tests (testcontainers) | **Written — need Docker** |
| Provider registration (stake GRT + call register/startService) | **Done** |
| Live Yellowstone → Postgres → PostgREST end-to-end | **TODO** |
| First paid query on mainnet | **TODO** |

---

## Architecture

```
Yellowstone gRPC  (confirmed Solana transactions)
    │
    │  SubstrateEvent { slot, signature, step, cursor, instructions }
    ▼
Handler  (pure Rust — deterministic, no I/O)
    │
    │  ChangeSet { slot, step, cursor, changes: Vec<EntityChange> }
    ▼
PostgresSink
    ├─ INSERT entity_changes  (commitment_status = NEW | UNDO)
    ├─ UPSERT cursors          atomic with entity writes
    └─ Sweeper: getSlot(finalized) → UPDATE status = FINAL
    ▼
PostgREST  (auto REST API over Postgres schema)
    ▼
seahorn-gateway  (Axum 0.8)
    ├─ Validates TAP-Receipt header (EIP-712 + ecrecover)
    ├─ Checks authorized_senders, data_service_address, staleness
    ├─ Persists receipt → unique (signer, nonce) prevents replay
    ├─ Proxies request to PostgREST
    └─ Background: aggregates receipts → RAVs → collect() on Arbitrum One
```

### Instruction decoders

| Program | Instructions |
|---|---|
| **Pump.fun** | Buy, Sell, Create |
| **Raydium CLMM** | Swap, SwapV2, OpenPosition, OpenPositionV2, AddLiquidity, RemoveLiquidity |
| **Jupiter v6** | SharedAccountsRoute, ExactOutRoute (full 40+ variant route-plan) |

### Query interface (PostgREST)

```
GET /buys?commitment_status=eq.FINAL&order=slot.desc&limit=100
GET /raydium_swaps?pool=eq.{pool_address}
GET /jupiter_swaps?user=eq.{wallet}&slot=gte.{from_slot}
```

Every request must carry a signed TAP receipt:
```
TAP-Receipt: {"allocation_id":"0x...","fees":"1000000","signature":"0x..."}
```

---

## Workspace

```
crates/
  seahorn-core             — Substrate / Handler / Sink traits
  seahorn-handler-pumpfun  — Pump.fun Buy/Sell/Create decoder
  seahorn-handler-raydium  — Raydium CLMM decoder
  seahorn-handler-jupiter  — Jupiter v6 decoder
  seahorn-substrate-mock   — Synthetic event streams for testing
  seahorn-sink-postgres    — Append-only Postgres sink + cursor + sweeper
  seahorn-gateway          — TAP v2 payment gateway (Axum 0.8)
contracts/
  SolanaDataService.sol    — Horizon DataService contract
  interfaces/ISolanaDataService.sol
test/
  SolanaDataService.t.sol  — 37 Foundry unit tests
script/
  Deploy.s.sol             — UUPS proxy deploy script
```

---

## Running

### Prerequisites

- Rust (edition 2024)
- PostgreSQL 15+
- PostgREST
- Yellowstone gRPC endpoint (Chainstack / Triton / Helius)
- Foundry (for contract work)

### Configuration

Copy `.env.example` to `.env` and fill in:

```env
# Solana indexer
YELLOWSTONE_ENDPOINT=https://your-endpoint.example.com
YELLOWSTONE_TOKEN=your_api_token_here
DATABASE_URL=postgres://seahorn:seahorn@localhost:5432/seahorn
SOLANA_RPC_URL=https://api.mainnet-beta.solana.com
RUST_LOG=seahorn=info

# Gateway
GATEWAY_CONFIG=gateway.toml    # path to gateway.toml
```

See `crates/seahorn-gateway/gateway.example.toml` for gateway configuration (TAP authorized senders, rate limits, PostgREST URL, etc).

### Start services

```bash
# Postgres + PostgREST
docker compose up -d

# Index Pump.fun (runs migrations, starts from last cursor)
cargo run -- --pumpfun

# Index all three programs
cargo run -- --all

# Run gateway
cd crates/seahorn-gateway && cargo run
```

### Flags

| Flag | Program |
|---|---|
| `--pumpfun` | Pump.fun |
| `--raydium` | Raydium CLMM |
| `--jupiter` | Jupiter v6 |
| `--all` | All three simultaneously |

---

## Testing

### Rust unit tests (no infrastructure required)

```bash
cargo test --workspace
# 15 unit tests: 4 Pump.fun, 3 Raydium, 2 Jupiter, 6 TAP gateway
```

### PostgresSink integration tests (requires Docker)

```bash
cargo test -p seahorn-sink-postgres
# 9 integration tests — spins up throwaway Postgres via testcontainers
# Skips gracefully if Docker is not running
```

### Foundry contract tests

```bash
# Install deps first (one-time)
forge install graphprotocol/contracts --no-git
forge install OpenZeppelin/openzeppelin-contracts-upgradeable --no-git

forge test
# 37 tests — full SolanaDataService lifecycle
```

---

## Provider registration

Lodestar is registered as a provider on mainnet. For reference, the steps were:

1. **Stake GRT** — call `HorizonStaking.provision(yourAddress, 0xdDE3F913..., 555e18, maxVerifierCut, thawingPeriod)` on Arbitrum One
2. **Register** — call `SolanaDataService.register(yourAddress, abi.encode(endpoint, geoHash, paymentsDestination))`
3. **Add programs to allowlist** — owner calls `SolanaDataService.addProgram(programId)` for each supported program
4. **Start indexing** — call `SolanaDataService.startService(yourAddress, abi.encode(programId, endpoint))` for each program
5. **Run the stack** — Yellowstone → seahorn → Postgres → PostgREST → seahorn-gateway

Supported program IDs:

| Program | ID |
|---|---|
| Pump.fun | `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P` |
| Raydium CLMM | `CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK` |
| Jupiter v6 | `JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4` |

---

## References

- [GRC-007: Seahorn RFC](./rfc.md)
- [GIP-0066: Graph Horizon](https://forum.thegraph.com/t/gip-0066-graph-horizon/5587)
- [GIP-0054: GraphTally (TAP v2)](https://forum.thegraph.com/t/gip-0054-graph-tally/5346)
- [Yellowstone gRPC](https://docs.yellowstone.io)
