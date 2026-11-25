# Durable mutation safety implementation plan

> **For agentic workers:** Use the `executing-plans` skill for inline execution or a reviewed subagent workflow. Follow TDD for each behavior change; tasks use checkboxes for tracking. The user approved adding `rusqlite`, using SQLite for the local operation journal and token registry, and a breaking change without migration of the existing JSON token registry. Obtain any additional approvals required by `AGENTS.md` before other dependency, deployment or API changes; this plan alone does not authorize commits.

**Goal:** Persist token issuance and revocation and prevent an AI agent from automatically repeating a mutation whose outcome may already have been applied after a single-server restart, while providing a safe way to investigate uncertain outcomes.

**Architecture:** Use local SQLite as the authoritative persistent store for the operation journal and token registry. The journal reserves mutations before upstream side effects and retains uncertain outcomes until explicitly resolved; token issuance and revocation are confirmed only after durable commits. Multi-instance coordination is intentionally out of scope (see ADR-0012). Operation ownership remains tied to a JWT, so reissuing a token does not transfer access to earlier idempotency records; this limitation is accepted and out of scope (see ADR-0012). Any retained in-memory mode must be explicitly documented as non-durable.

**Tech stack:** Existing Rust workspace, Tokio, current MCP and Intervals.icu clients, and the approved `rusqlite` dependency. Select a compatible stable version and SQLite build options during implementation.

**Design input:** The architecture audit and subsequent user decisions: SQLite and `rusqlite` are approved; existing JSON token-registry migration is not required and the breaking change is accepted. There is no separately approved design specification; remaining security, retention and operational decisions below are gates.

## Global constraints

- Do not promise exactly-once effects: the journal and Intervals.icu do not share a transaction.
- A timeout, expired lease, process death, lost response, or decoding error does not establish that upstream did not apply a mutation.
- The operation identity remains independent of replay ownership: upstream namespace, athlete, and idempotency token identify the operation; owner authorization controls access to its result.
- Operation ownership is token-specific. A reissued JWT is a different owner; do not infer a shared principal from athlete ID or an API-key hash. This limitation is intentionally accepted and documented in ADR-0012.
- Unknown outcomes must not expire or be evicted automatically to enable another write.
- Never store or log raw API keys, JWTs, or idempotency tokens. Preserve the existing AES-256-GCM encryption of upstream API keys; SQLite alone does not encrypt database contents. Protect saved result bodies, database files, sidecars and backups; document that SQLite does not encrypt them.
- Do not modify `.env*`, `Cargo.lock`, or ignored local files. Follow the project's pathless Rust module layout.
- Test behavior first, observe the expected failure, implement minimally, and run affected tests plus the required workspace checks.

## Review focus

1. The journal is unavailable, full, corrupt, or cannot persist a transition: no new upstream write occurs unless a durable dispatch marker was committed.
2. A process dies immediately before or after dispatch, or after upstream success but before saving the result: retry never silently starts a second write.
3. Concurrent same-key requests, different fingerprints, different owners, and cancellation preserve deduplication and do not disclose another owner's result.
4. Existing JSON token registries are not imported; their loss must be explicit and cannot silently overwrite existing files. Legacy idempotency records without scope or owner must never be treated as proof that an upstream write is safe to repeat.
5. Partial batch execution and ambiguous bulk results remain unknown; similarity of events is not proof. No built-in inspection or resolution interface is in scope (ADR-0012).

## Scope and decisions before implementation

- [x] **Deployment guarantee.** Supported deployment: one process on one host using persistent local SQLite. Multi-instance coordination is intentionally excluded from this plan and unsupported by this guarantee; see ADR-0012. A JSON file or arbitrary shared filesystem does not provide the guarantee.
- [x] **Storage and dependency.** User approved SQLite and adding `rusqlite` for the journal and token registry. Verify build compatibility, durability, locking, file permissions, backup and recovery. Obtain separate approval before adding other dependencies, a shared service or unrelated deployment changes.
- [x] **Token registry breaking change.** No JSON-to-SQLite token migration is required. Existing registry-backed JWTs must be reissued after cutover; document this before deployment. Do not delete, rename, overwrite or modify existing JSON files automatically. The user did not authorize discarding idempotency history; handle any old journal separately and fail closed when its safety cannot be established.
- [x] **Operation retention and result confidentiality.** Confirmed results are encrypted and retained for 24 hours; unknown outcomes remain indefinitely. Replay requires the original JWT owner. SQLite metadata and backups are not encrypted by the application and must be kept private. No public resolution interface is provided; any operator investigation is out of band (ADR-0012).
- [x] **Upstream capabilities.** The implementation does not rely on upstream idempotency keys or atomic bulk semantics. Any ambiguous result remains unknown; do not infer that the upstream mutation was not applied.

## File map

- Modify `crates/intervals_icu_mcp/src/intents/idempotency.rs`: coordinate journal reservations, owner checks and state transitions.
- Modify `crates/intervals_icu_mcp/src/intents/router.rs`: retain operation scope and route validated actions through the journal.
- Modify `crates/intervals_icu_mcp/src/lib.rs`: initialize SQLite-backed journal and registry in supported server modes and fail safely if the configured database cannot be opened.
- Modify `crates/intervals_icu_mcp/Cargo.toml` to add the approved `rusqlite` dependency; do not stage or edit `Cargo.lock`.
- Modify `crates/intervals_icu_mcp/src/token_registry.rs` and `crates/intervals_icu_mcp/src/auth.rs`: persist encrypted token records and revocation transactionally; propagate storage errors through issuance, authentication and UI handlers rather than reporting success.
- Add a focused SQLite storage module and tests under the existing crate layout; centralize database opening, schema versioning and safe access across Tokio tasks.
- Modify `crates/intervals_icu_mcp/src/intents/handlers/plan_training.rs` and `crates/intervals_icu_mcp/src/intents/handlers/modify_training/actions.rs`: mark the side-effect boundary and report partial execution where supported.
- Modify `crates/intervals_icu_mcp/src/metrics.rs` and `README.md`: operational signals and precise guarantees.
- Add SQLite journal and registry integration tests; use existing tests `crates/intervals_icu_mcp/tests/idempotency_ownership.rs`, `crates/intervals_icu_mcp/tests/idempotency_base_url.rs` and `crates/intervals_icu_mcp/tests/idempotency_legacy_router.rs` as regression coverage.


## Task 1: Specify the write boundary and error contract

**Files:** `crates/intervals_icu_mcp/src/intents/idempotency.rs`, `crates/intervals_icu_mcp/src/intents/router.rs`, existing handler tests.

- [x] Add tests showing that a proven local validation failure does not reserve an uncertain outcome, while cancellation, panic, transport failure, and response-decode failure after possible dispatch block a retry.
- [x] Run the focused tests and record the expected failures. Do not classify an upstream error as pre-write merely by its HTTP status.
- [x] Define internal states and error handling so the transition to potentially dispatched occurs before the first side effect. Preserve owner checks and current STDIO behavior.
- [x] Run the focused tests and existing idempotency tests. Verify that another owner receives neither cached content nor permission to execute.

## Task 2: Add a transactional SQLite operation journal

**Prerequisite:** SQLite and `rusqlite` are approved; settle configuration, result-retention and confidentiality details without assuming additional approval.

**Files:** `crates/intervals_icu_mcp/src/intents/idempotency.rs`, a focused journal module under `crates/intervals_icu_mcp/src/intents/`, `crates/intervals_icu_mcp/src/lib.rs`, backend-specific tests, `README.md`.

- [x] Define a small internal journal interface with atomic reserve/read and conditional transitions, storing a versioned scope hash, fingerprint hash, owner, state, timestamps and an authorized completed result. The backend is the source of truth; local locks alone are insufficient.
- [x] Write failing tests for concurrent same-key reservations, same key with a different fingerprint or owner, a full or unavailable journal, and failed transition writes. Assert upstream call counts and absence of result leakage.
- [x] Implement the approved backend and wire it into the explicitly supported mode. Mark dispatch before upstream write; write completion only after a confirmed result. A failed journal operation must not degrade silently to an empty cache.
- [x] Add a subprocess test that starts a mutation, terminates the process at controlled boundaries, starts a new process on the same journal and verifies that the repeated key cannot dispatch twice. Include termination after upstream accepts the mutation and before completion is recorded.
- [x] Test corruption and schema-version mismatch. JSON token-registry import is explicitly out of scope; for existing idempotency records without scope or owner, do not silently ignore history and permit a duplicate. Define and test a fail-closed cutover or obtain explicit authorization for another policy.
- [x] Run targeted tests and the full workspace checks. Document the single-node guarantee, retention, configuration and recovery procedure.

## Task 2B: Store the token registry in SQLite

**Files:** `crates/intervals_icu_mcp/src/token_registry.rs`, `crates/intervals_icu_mcp/src/auth.rs`, affected UI/server call sites and tests, approved SQLite storage module, `README.md`.

- [x] Write a failing integration test that issues a token, restarts against the same database and authenticates it; revoke it, restart again and assert authentication is rejected. Keep the encrypted upstream API key decryptable with the existing master key.
- [x] Run the test and confirm that it fails because current storage is JSON/best-effort rather than the required SQLite contract; then implement transactional insert, lookup and revoke.
- [x] Write failing fault-injection tests for unavailable, read-only, full or corrupt SQLite: failed issuance must not return a usable token; failed revocation must not report success; authentication on registry read failure must fail closed. Implement error propagation through callers and run the tests again.
- [x] Test concurrent issue/revoke and same-token lookups against the database as source of truth; do not serve stale revocation state from an unsynchronised memory cache.
- [x] Test first startup with a pre-existing JSON token registry: no automatic import, deletion, overwrite or silent reinterpretation as SQLite. Require explicit operator configuration of a separate SQLite path, or fail with an actionable error if the old JSON path is supplied.
- [x] Document the breaking change and reissue procedure, local-disk and backup requirements, path and permissions, encrypted API keys, unencrypted SQLite metadata/results, and failure behavior. Never log secrets or actual file contents.

## Task 3: Reconcile unknown and partial operations

**Prerequisite:** Task 2. No upstream idempotency or atomic-bulk guarantee is assumed; ambiguous results remain blocked.

**Files:** `crates/intervals_icu_mcp/src/intents/handlers/plan_training.rs`, `crates/intervals_icu_mcp/src/intents/handlers/modify_training/actions.rs`, approved journal module and associated tests.

- [x] Add regression tests for a lost bulk-create response, an undecodable response and a multi-event modification that stops after its first confirmed update. Assert that same-key retries do not repeat the whole batch.
- [x] Report confirmed progress where available and clearly distinguish partial/uncertain outcomes; do not infer completion from similar event names or dates.
- [x] Keep unknown outcomes non-retryable across process restarts. A built-in inspection/resolution API and its authorization/audit policy are intentionally excluded until separately approved; operators must investigate upstream out of band and must not clear unknown records to enable retries (ADR-0012).

## Task 4: Protect capacity and add operational signals

**Files:** approved journal module, `crates/intervals_icu_mcp/src/metrics.rs`, server setup, tests and `README.md`.

- [x] Write failing concurrent tests where tenant A exhausts its own quota while tenant B can still reserve within the global limit. Assert that rejection happens before a side effect and existing completed replay remains available.
- [x] Separate quotas for active work, unresolved outcomes and completed-result storage. Keep a global safety limit; never delete unknown outcomes to free capacity.
- [x] Add bounded waiting and metrics for journal errors, quota rejection, unknown counts and age. Do not place user, token or operation identifiers in metric labels.
- [x] Test restart with unresolved records. Document out-of-band diagnosis using upstream records; do not expose an in-server inspection/resolution API or clear unknown records, per ADR-0012.


## Final verification

- [x] `cargo fmt --all -- --check`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] `cargo test --all-targets --all-features`
- [x] `cargo test --all --all-features --no-fail-fast`
- [x] Review failure-injection coverage, legacy-journal cutover, confidentiality and operational documentation. Report unverified upstream guarantees and remaining limitations explicitly.
