# GRC-007: Seahorn — A Solana Structured Data Service on Horizon

**Stage:** RFC (Request for Comment)
**GRC:** 007
**Authors:** @cargopete (Petko Pavlovski)
**Related:** GRC-005: Dispatch · GRC-006: Mainline · GRC-004: Graphite · GIP-0066: Graph Horizon · GIP-0054: GraphTally

---

## Summary

This GRC proposes Seahorn — a Horizon data service that indexes Solana program activity into structured, queryable entities and gates access to that data via TAP v2 micropayments. It is the missing third lane alongside Subgraphs (EVM entities) and Substreams (pure-compute transforms): a Rust-native service that turns raw Solana instructions into typed, fork-correct, immediately queryable structured data.

This is not a design proposal in search of an implementation. The Rust service runs. Typed decoders exist for Pump.fun, Raydium CLMM, and Jupiter v6. The payment gateway validates TAP receipts, aggregates RAVs, and proxies queries to PostgREST. `SolanaDataService.sol` — a full Horizon DataService contract with provider registration and on-chain fee collection — is written and ready to deploy. What remains is Arbitrum Sepolia smoke-test, Foundry tests, and Arbitrum One registration.

The commercial rationale is direct: Solana does ~50% of on-chain DEX volume by value. Every dApp consuming Solana trade data currently pays Helius, Quicknode, or a custom indexer. The Graph has no answer for this. Seahorn is that answer, built on the payment and staking primitives already deployed for GRC-005 Dispatch and GRC-006 Mainline.

---

## Background

The Graph indexes EVM chains via Subgraphs and provides streaming compute for any chain via Substreams. Both exist on Solana in some form:

- **Subgraphs on Solana** do not exist. `graph-node`'s WASM runtime is built around the EVM data model — Ethereum events, contract calls, the Ethereum JSON-RPC interface. Solana's instruction-based account model does not map onto it without deep surgery to graph-node. GRC-004's Graphite approach (implement the AS ABI in Rust) solves the developer experience problem for EVM subgraphs; it offers nothing here.

- **Substreams on Solana** exists. StreamingFast runs `firehose-solana`, and Substreams modules can be written for Solana programs. But Substreams is a pure-compute primitive: a module transforms a stream of blocks and emits protobuf. A Substreams consumer still needs to run their own sink — a `substreams-sink-postgres` binary, a Kafka consumer, a custom process — to make the output queryable. Substreams doesn't store entities, doesn't serve a query interface, and doesn't handle billing.

The gap: Solana developers want structured, queryable data — not a compute pipeline. They want to ask "give me the last 100 swaps on this pool" and get a JSON response. They want fork-correct entity state, not a raw block stream. They want to pay per query and not operate infrastructure. The Graph has delivered exactly this on EVM for years. There is no equivalent for Solana.

Seahorn fills that gap.

---

## Design

### Architecture

The core abstraction is a three-stage pipeline that separates data concerns cleanly:

```
Substrate  (Yellowstone gRPC / Firehose-Solana / Mock)
    │
    │  SubstrateEvent { slot, signature, step, cursor, instructions }
    ▼
Handler  (pure Rust fn — deterministic, no I/O)
    │
    │  ChangeSet { slot, step, cursor, changes: Vec<EntityChange> }
    ▼
Sink  (PostgresSink → PostgREST REST API)
    │
    ├─ INSERT entity_changes (commitment_status = NEW | UNDO)
    ├─ UPSERT cursors  ← atomic with entity writes
    └─ Sweeper: getSlot(finalized) → UPDATE status = FINAL
```

The Handler → ChangeSet split is the most important architectural decision. Handlers are pure functions: given the same `SubstrateEvent`, they always return the same `ChangeSet`. No database access, no network calls, no side effects. This makes them:

1. **Testable with `cargo test`** — no Docker, no PostgreSQL, no Yellowstone credentials.
2. **Deterministic by construction** — the precondition for a future Proof-of-Indexing on Horizon.
3. **Independently composable** — a `MultiHandler` fans one event stream out to N handlers simultaneously.

### Fork Handling

Solana uses optimistic confirmation: a block is marked `confirmed` before it is `finalized`. In practice the finalization rate is near-perfect, but reorgs happen, and an indexer that ignores them silently corrupts state. Seahorn handles forks correctly:

- `NEW` — entity change written at `confirmed` commitment.
- `UNDO` — written if the block is later reverted (step emitted by the substrate).
- `FINAL` — promoted by a background sweeper that polls the Solana RPC `getSlot(finalized)` every 10 seconds and bulk-updates all `NEW` rows at or below the finalized slot.

All writes are append-only. Cursors are upserted atomically with the entity write in the same transaction. On crash recovery, the Yellowstone substrate re-opens the stream from the persisted cursor, discarding events at or below the last-seen slot.

### Instruction Decoders

Solana programs use Anchor discriminators (first 8 bytes of instruction data, `sha256("global:{name}")[..8]`) to identify instructions, followed by Borsh-encoded arguments. Seahorn provides typed handlers for:

| Program | Instructions decoded |
|---|---|
| **Pump.fun** | `Buy`, `Sell`, `Create` |
| **Raydium CLMM** | `Swap`, `SwapV2`, `OpenPosition`, `OpenPositionV2`, `AddLiquidity`, `RemoveLiquidity` |
| **Jupiter v6** | `SharedAccountsRoute`, `ExactOutRoute` (full 40+ variant route-plan skip table) |

Each handler is a Rust crate: no code generation, no protobuf, no IDL required. Adding a new program is a new crate with a discriminator match and a Borsh struct.

### Service Contract — SolanaDataService.sol

`SolanaDataService` inherits the same Horizon base stack as `SubgraphService` and `RPCDataService` (GRC-005): `DataService` + `DataServiceFees` + `DataServicePausableUpgradeable`, deployed as a UUPS proxy on Arbitrum One.

```solidity
contract SolanaDataService is
    OwnableUpgradeable,
    UUPSUpgradeable,
    DataService,
    DataServiceFees,
    DataServicePausableUpgradeable,
    ISolanaDataService
```

Key parameters:

| Parameter | Value | Rationale |
|---|---|---|
| `DEFAULT_MIN_PROVISION` | 555 GRT | Deliberately low for bootstrapping. Operators are scaling up infrastructure — an insurmountable stake minimum at Phase 0 would kill supply before demand exists. |
| `STAKE_TO_FEES_RATIO` | 5:1 | Matches SubgraphService. Meaningful skin-in-the-game relative to fees collected. |
| `BURN_CUT_PPM` | 10,000 (1%) | Protocol fee burned on every collect(). |
| `DATA_SERVICE_CUT_PPM` | 10,000 (1%) | Data service treasury cut. |
| `MIN_THAWING_PERIOD` | 14 days | Matches Dispatch. |

Provider lifecycle:
- `register(serviceProvider, data)` — stakes a provision, registers an endpoint and geo hint.
- `startService(serviceProvider, data)` — activates indexing for a specific Solana program ID.
- `stopService(serviceProvider, data)` — deactivates a program registration.
- `collect(serviceProvider, paymentType, data)` — submits a signed RAV; GRT flows from consumer escrow to provider after protocol cuts.
- `slash(serviceProvider, data)` — explicitly reverts. Not implemented (see open questions).

Program support is governance-controlled: the contract owner calls `addProgram(programId)` / `removeProgram(programId)` to manage the allowlist. A provider can only call `startService` for an allowlisted program ID.

### Payment Gateway — seahorn-gateway

Every query to the Seahorn REST API must carry a `TAP-Receipt` header. The gateway (`seahorn-gateway`, Axum 0.8) enforces this:

1. Extracts and parses the TAP-Receipt JSON header.
2. Recovers the signer via EIP-712 struct hash + `ecrecover`.
3. Checks the signer against the configured `authorized_senders` list.
4. Validates the receipt's `data_service_address` and `service_provider` fields.
5. Checks staleness against `max_receipt_age_ns` (default 30 seconds).
6. Persists to `tap_receipts` — a unique constraint on `(signer_address, nonce)` prevents replay; duplicate nonces return HTTP 402.
7. Proxies the request to PostgREST.
8. A background task aggregates receipts into RAVs every 60 seconds and submits `collect()` hourly.

Rate limiting (tower-governor, configurable in `gateway.toml`) applies to proxy routes. `/health` and `/ready` are exempt.

### Service Interface

The query interface is PostgREST — a REST API automatically generated from the PostgreSQL schema. There is no custom GraphQL server in v0 (see design rationale).

```
GET /buys?commitment_status=eq.FINAL&order=slot.desc&limit=100
GET /raydium_swaps?pool=eq.{pool_address}
GET /jupiter_swaps?user=eq.{wallet}&slot=gte.{from_slot}
```

The schema is append-only `entity_changes` rows with typed views layered on top:

```sql
-- Example view
CREATE VIEW buys AS
SELECT slot, tx_signature, commitment_status,
       fields->>'mint'         AS mint,
       fields->>'user'         AS user_address,
       (fields->>'token_amount')::bigint AS token_amount,
       (fields->>'sol_cost')::bigint     AS sol_cost
FROM   entity_changes
WHERE  entity_type = 'Buy';
```

---

## Implementation Status

| Component | Status |
|---|---|
| `seahorn-core` — Substrate/Handler/Sink traits, MultiHandler | Working |
| `seahorn-handler-pumpfun` — Buy/Sell/Create | Working |
| `seahorn-handler-raydium` — Swap, Position, Liquidity | Working |
| `seahorn-handler-jupiter` — SharedAccountsRoute, ExactOutRoute | Working |
| `seahorn-substrate-mock` — Synthetic event streams, AllProgramsMockSubstrate | Working |
| `seahorn-sink-postgres` — Append-only writes, cursor persistence, finalization sweeper | Working |
| `seahorn-gateway` — TAP validation, nonce replay prevention, RAV aggregation, rate limiting, health probes | Working |
| `SolanaDataService.sol` — Full Horizon DataService contract | Written, not yet deployed |
| Unit tests — 15 total (4 Pump.fun, 3 Raydium, 2 Jupiter, 6 TAP) | Passing |
| Docker Compose — Postgres + PostgREST | Working |
| Foundry tests for SolanaDataService.sol | Not written |
| PostgresSink integration tests (testcontainers) | Not written |
| Arbitrum Sepolia deploy | Not done |
| Arbitrum One registration | Not done |

### What is not implemented

**Slashing.** `slash()` reverts unconditionally. The Seahorn service produces structured data that has no cheap on-chain verifiability property — unlike a Firehose block (GRC-006), a Solana swap entity's field values cannot be compared against a canonical root without re-executing the program. Slashing requires either a POI-equivalent for Solana entity state (not designed yet) or economic accountability via quorum comparison (feasible but not built). This is the same position Dispatch launched in.

**POI.** Handlers are deterministic and pure, which is the necessary condition for Proof-of-Indexing. But the POI computation over `entity_changes` is not implemented. It is the intended path; it is not the current one.

**Permissionless program registration.** Programs are added by the contract owner. A bond-based permissionless model (pay X GRT to get a program added to the allowlist) is the natural direction. It is not built.

**GRT issuance rewards.** Providers earn query fees only. GRT issuance would require Graph governance approval and is not proposed here.

**GraphQL.** The query interface is PostgREST REST, not GraphQL. Graph-node's entity store and GraphQL server are not involved.

---

## Rationale

**Why not a Substreams module?**

Substreams is a streaming compute primitive. It produces protobuf output that must be consumed by something — a sink binary, a custom process. A developer who wants "give me the last 100 swaps" from a Substreams module still needs to run `substreams-sink-postgres`, manage its lifecycle, handle fork events in their sink, and build their own query interface on top. Seahorn does all of that and gates access to the result via The Graph's payment layer. These are complementary, not competing: a Substreams module on Solana could use Seahorn as its sink and billing layer.

**Why PostgREST and not graph-node / GraphQL?**

Two reasons. First, speed: PostgREST is a binary that runs in front of an existing Postgres schema. There is no `schema.graphql`, no codegen step, no WASM runtime. Typed SQL views give consumers a clean query interface in minutes. Second, correctness: graph-node's entity store and WASM runtime are deeply EVM-coupled. Adding a Solana data model to graph-node without modifying it is not feasible; adding it with modifications is a large graph-node contribution with high review risk. PostgREST is pragmatic. GraphQL over the same schema is a future addition (PostgREST → Hasura or pg_graphql is one hop).

**Why Yellowstone gRPC over Firehose-Solana?**

Yellowstone gRPC is the Solana-native streaming protocol for confirmed transactions. It operates at the `confirmed` commitment level with sub-second latency and provides per-program filtering. Firehose-Solana (`firesol`) is instrumentated at the validator level and produces full block objects — correct and useful, but higher overhead for transaction-level use cases. Seahorn abstracts this via the `Substrate` trait: swap the substrate, keep the handler and sink unchanged. Yellowstone is the default because it is simpler to operate and has three commercial providers at accessible price points. Firehose-Solana becomes the right default as GRC-006 Mainline matures.

**Why a separate DataService contract and not SubgraphService?**

SubgraphService is wired to subgraph-specific concepts: allocations, POIs, curation signal, `maxPOIStaleness`. None of these exist for Seahorn. `SolanaDataService` follows the same logic as GRC-005: one contract per data service type, using the DataService framework as intended. The payment primitives (GraphTallyCollector, PaymentsEscrow, HorizonStaking) are reused unchanged.

**Why 555 GRT minimum provision?**

Bootstrapping. The commercial case for staking 10,000 GRT (Dispatch's minimum) or 100,000 GRT (SubgraphService's minimum) does not exist before there are consumers paying query fees. 555 GRT is low enough that a serious operator can participate without a speculative capital commitment. The governance owner can raise `DEFAULT_MIN_PROVISION` as demand materialises. It is easier to tighten a parameter than to explain to early operators why the number was set to kill them before the market existed.

---

## Open Questions

**On verification:** Is economic accountability (stake-to-fees locking, no slashing) sufficient to bootstrap a real market? The same question was asked about Dispatch, and the answer has so far been "yes, for bootstrapping." Is a POI-equivalent for Solana entity state — a deterministic hash over the `entity_changes` output at a given slot — both feasible and sufficient for a future slashing mechanism?

**On the query interface:** PostgREST is pragmatic but REST is not The Graph's native query language. What is the right path to serving Seahorn data via GraphQL? Is it `pg_graphql` as a PostgREST replacement? A future graph-node mode that speaks to an external Postgres entity store? Or is REST-first correct for a v0 that needs to move fast?

**On program coverage:** The initial handler library covers Pump.fun, Raydium CLMM, and Jupiter v6 — the three highest-volume programs on Solana by transaction count. What are the community's priorities for expansion? Drift (perps), Orca Whirlpools (CLMM alternative), Marinade (liquid staking), Metaplex (NFT), SPL Token (account-level balance tracking)?

**On the Yellowstone dependency:** Yellowstone gRPC is provided by commercial operators (Chainstack, Triton, Helius). As GRC-006 Mainline matures, the natural move is to route Seahorn substrates through decentralised Firehose-Solana operators rather than centralised Yellowstone endpoints. What is the right timing for that transition, and should it be a protocol-level requirement or an operator-level choice?

**On program governance:** The owner-controlled program allowlist is safe but not permissionless. A bond-based model — lock X GRT to get a Solana program ID added to the allowlist — is the obvious direction. What is the right bond amount, and who controls the treasury that holds those bonds?

**On multi-tenancy:** Seahorn v0 is self-hosted — one operator, one set of programs, one Postgres instance. A multi-tenant model where a single operator serves multiple consumers' queries against different program subsets (like Helius' Enhanced APIs) requires row-level security, per-consumer billing, and a more sophisticated gateway. Is the community interested in building toward that, or is single-operator self-hosting the right scope for this data service?

---

## Phased Rollout

| Phase | Scope | Exit criterion |
|---|---|---|
| **Phase 0 — Testnet** | Foundry tests for SolanaDataService.sol. Deploy to Arbitrum Sepolia. Single operator (cargopete) running live Yellowstone → Postgres → PostgREST → seahorn-gateway. Full TAP payment loop demonstrated end-to-end on testnet. | Working collect() cycle on Sepolia; at least one external consumer pulling query data through the payment gateway. |
| **Phase 1 — Arbitrum One bootstrap** | SolanaDataService.sol on Arbitrum One. Single operator, Pump.fun + Raydium CLMM + Jupiter v6. Program allowlist: three programs. Governance-controlled. Slashing disabled. | First paid query fee collected on mainnet. |
| **Phase 2 — Multi-operator** | 3–5 invited operators. Permissionless operator registration (anyone with 555+ GRT provision can join). Bond-based program allowlist. Quality scoring in gateway. | 10 paying consumers; 99.5% uptime across all operators for 30 consecutive days. |
| **Phase 3 — POI + slashing** | Deterministic POI over entity_changes output. `slash()` enabled with evidence-based dispute. Minimum provision revised upward based on real demand data. | First successful testnet slash. |

---

## Operator Economics

These are estimates for a single operator serving Pump.fun + Raydium CLMM + Jupiter v6 on mainnet.

| Cost line | Monthly estimate |
|---|---|
| Yellowstone gRPC endpoint | $49–$499 |
| Compute (8 vCPU, 32 GB RAM) | $80–$150 |
| Postgres storage (Hetzner AX42 class) | $50 |
| **Total MVP** | **~$200–$700/mo** |

At 555 GRT minimum provision and a 5:1 stake-to-fees ratio, an operator can collect at most 111 GRT per 14-day thawing period before their provision is fully locked. At $0.10/GRT that is $11 per cycle — not commercially meaningful. This is by design: Phase 0 and Phase 1 are about proving the payment loop and building consumer demand, not extracting revenue. The provision minimum should rise in lockstep with demonstrated query fee volume.

A credible Phase 2 target — 10 paying consumers generating 100,000 queries/day at $0.001 per query — yields $100/day = $3,000/month in fees across the operator set. At five operators, that is $600/operator/month, covering the infrastructure cost and leaving a meaningful margin. This is achievable and not speculative: Helius charges $499/month for their LaserStream plan and has thousands of paying customers. The Graph's payment model is better (pay per query, not flat rate) but requires a gateway and a functional data service. Seahorn provides both.

---

## References

- GRC-005: Dispatch — An Experimental JSON-RPC Data Service on Horizon
- GRC-006: Mainline — A Firehose Data Service on Horizon
- GRC-004: Graphite — Rust as a first-class subgraph mapping language
- GIP-0066: Graph Horizon
- GIP-0054: GraphTally (TAP v2)
- Yellowstone gRPC documentation
- Lodestar — How to Build a Horizon Data Service
- cargopete/seahorn (GitHub)
