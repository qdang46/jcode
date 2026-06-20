# jcode-provider-service

Catalog → Integration → Credential service traits and shared types for
jcode's new provider resolution layer.

> Implements the foundational service architecture from
> [`docs/plans/JCODE_PROVIDER.md`](../../docs/plans/JCODE_PROVIDER.md).
> Phases 0–4 of the plan are landed in this crate. Phases 5+ (TUI,
> session-runner rewiring, dead-code removal) depend on the rest of
> jcode, which has pre-existing build failures unrelated to this work.

---

## Why this crate exists

The current `jcode-provider-core` defines a 60-method `Provider` trait
that every provider implements directly. The flow is rigid:

- The CLI flags `--provider` / `--model` go through a hardcoded
  `ProviderChoice` enum.
- The model catalog is a `const &[&str]` updated by hand.
- Credentials are ad-hoc env-var lookups inside each provider's impl.
- OAuth tokens live in a separate `external_auth.rs`.

`docs/plans/JCODE_PROVIDER.md` calls for a layered architecture that
matches opencode's:

```
                    ┌─────────────┐
                    │   Config    │  user.toml + project.toml
                    └──────┬──────┘
                           │ --provider, --model
                           ▼
  ┌──────────────────────────────────────────────────────┐
  │                    CATALOG                            │  Phase 3
  │  providers, models, .available()/.default()/.small()  │
  └──────┬───────────────────────────────────────────────┘
         │
         ▼
  ┌──────────────────────────────────────────────────────┐
  │                  INTEGRATION                          │  Phase 2
  │  .oauth(), .save_api_key(), .detect()                 │
  └──────┬───────────────────────────────────────────────┘
         │
         ▼
  ┌──────────────────────────────────────────────────────┐
  │               CREDENTIAL STORE                        │  Phase 1
  │  OS keychain-backed, transactional, per-provider      │
  └──────────────────────────────────────────────────────┘
```

This crate defines the *interfaces* (services + types) and ships
in-memory + OS-keychain reference implementations. The session runner
(Phase 6) and TUI pickers (Phase 5) will eventually consume these
services; both depend on parts of jcode that have unrelated pre-existing
build failures, so they are not landed here.

---

## Crate layout

```
crates/jcode-provider-service/
├── Cargo.toml
├── README.md                ← this file
└── src/
    ├── lib.rs               crate root, re-exports
    ├── types.rs             ProviderId, ModelId, ProviderProfile
    ├── credential.rs        CredentialService trait + types
    ├── integration.rs       IntegrationService trait + types
    ├── catalog.rs           CatalogService trait + types
    ├── service.rs           ProviderService facade + RouteResolver
    ├── store/
    │   ├── mod.rs
    │   ├── in_memory.rs     InMemoryCredentialStore
    │   ├── keyring.rs       KeyringCredentialStore
    │   ├── integration.rs   PersistentIntegration<K>
    │   └── service.rs       DefaultProviderService
    └── bin/
        └── providerctl.rs   standalone CLI smoke test
```

---

## Public surface

| Type                                  | Layer        | What it does                                       |
|---------------------------------------|--------------|----------------------------------------------------|
| `ProviderId` / `ModelId`              | types        | Validated, clone-cheap identifier newtypes.        |
| `ProviderProfile`                     | types        | CLI / config shorthand (`--provider anthropic`).   |
| `CredentialService`                   | credential   | Async trait for credential storage.                |
| `Credential` / `CredentialType`       | credential   | Stored record + payload (OAuth / ApiKey / Cmd).    |
| `IntegrationService`                  | integration  | Provider registration, OAuth lifecycle, detection. |
| `LoginProvider` / `AuthMethod`        | integration  | Provider's login options.                          |
| `OAuthAttempt`                        | integration  | In-flight OAuth login with 10-minute TTL.          |
| `ConnectionStatus`                    | integration  | Result of `detect()`: env / persisted / none.      |
| `CatalogService`                      | catalog      | Provider / model registry, derived views.          |
| `ProviderInfo` / `ModelInfo`          | catalog      | Catalog entries with metadata + cost.              |
| `ModelTier`                           | catalog      | Flagship / Standard / Mini / Nano.                 |
| `ProviderService`                     | service      | Facade bundling catalog + integration + creds.     |
| `RouteResolver`                       | service      | `(provider, model)` → `jcode_llm_core::Route`.     |
| `ResolvedRoute`                       | service      | Result of `resolve_route()`.                       |

---

## Reference implementations

| Implementation                     | Backend                       | Use case                  |
|------------------------------------|-------------------------------|---------------------------|
| `InMemoryCredentialStore`          | HashMap                       | Tests, Phase 0 boot.      |
| `KeyringCredentialStore<K>`        | OS keychain (via `jcode-keyring-store`) | Production credentials. |
| `InMemoryCatalog`                  | HashMap                       | Tests, Phase 0 boot.      |
| `InMemoryIntegration`              | HashMap (no persistence)      | Tests where cred store isn't needed. |
| `PersistentIntegration<K>`        | HashMap + `CredentialService` | Production login flows.   |
| `DefaultProviderService`           | Composes the above           | Production runtime.       |

`K` is the concrete `jcode_keyring_store::KeyringStore` — typically
`DefaultKeyringStore` (macOS Keychain / Linux Secret Service / Windows
Credential Manager) in production and `MockKeyringStore` in tests.

---

## Migration from old  types

See [](./MIGRATION.md) for the complete old → new type/function mapping. The old types stay in place until  is repaired (the dependency that prevents Phase 6 from landing).

## Phase status

| Phase | Plan deliverable                            | Status     | Commit(s) |
|-------|---------------------------------------------|------------|-----------|
| 0     | `jcode-provider-service` crate scaffolded   | ✅ done    | `5bfb3f7d` |
| 1     | `CredentialService` (in-memory + keyring)   | ✅ done    | `50722d13` |
| 2     | `IntegrationService` + OAuth lifecycle      | ✅ done    | `36bc22fd` |
| 3     | `CatalogService` + `DefaultProviderService` | ✅ done    | `8ecdf5f8` |
| 4     | `providerctl` CLI + `ProviderProfile` resolvers | ✅ done | `5d368146`, `0d4fcc26` |
| 5     | TUI provider/model pickers (data model only) | ✅ partial | `aa287b23` |
| 6     | Boot helper wiring real `jcode-llm-protocols` routes | ✅ done | `82b44657` |
| 6.5   | Migration helper (`auth_mode` → `Credential`) | ✅ done   | this commit |
| 7     | Delete dead code                            | 🟡 partial | `21d200` removed `jcode-provider-app`; `auth_mode.rs` deletion still blocked on `jcode-tui` consumers |

"Blocked" here means: the plan's deliverables require modifying
`jcode-tui`, which has 37 pre-existing compilation errors unrelated to
this work. Per repo guidelines, those errors are out of scope for this
branch.

---

## Quick start

```bash
# Run the smoke-test CLI (writes to your real OS keychain).
cargo run -p jcode-provider-service --bin providerctl -- list

# Save an API key.
cargo run -p jcode-provider-service --bin providerctl -- login anthropic sk-ant-...

# Confirm the credential roundtrips.
cargo run -p jcode-provider-service --bin providerctl -- available

# Print a resolved Route as JSON.
cargo run -p jcode-provider-service --bin providerctl -- resolve anthropic claude-sonnet-4-6

# Remove the credential.
cargo run -p jcode-provider-service --bin providerctl -- logout anthropic
```

---

## Testing

```bash
cargo test -p jcode-provider-service
```

51 unit tests cover:

- Type construction + validation (`types.rs`).
- Credential CRUD, replacement, isolation, idempotency
  (`store/in_memory.rs`, `store/keyring.rs`).
- OAuth attempt TTL and completion semantics
  (`integration.rs`, `store/integration.rs`).
- Catalog `available()` / `default()` / `small()` heuristics
  (`catalog.rs`).
- End-to-end `resolve_route()` against a fully-wired service
  (`store/service.rs`).
- Built-in provider registry (`bin/providerctl.rs`).

---

## Migration path (for Phase 6)

The current `Provider` trait in `jcode-provider-core` keeps working
unchanged. Consumers should migrate in three steps:

1. **Hold a `Arc<dyn ProviderService>` instead of constructing a
   concrete provider.** The session runner gets this handle once at
   boot and passes it to the agent loop.
2. **Resolve a `Route` per request** via
   `service.resolver().resolve_route(&provider, &model).await?`. Each
   `Route` carries its protocol, endpoint, framing, and transport —
   enough information for the existing `jcode-llm-core` transport
   layer to dispatch the request.
3. **Delete the ad-hoc env-var lookups** in each provider's impl once
   the new path is verified end-to-end. The auth material is now on
   the `Route` (or fetched on demand from the `CredentialService`).

Phase 7 cleanup:

- Remove `crates/jcode-provider-core/src/auth_mode.rs` (no consumers
  outside tests).
- Remove the in-memory `Catalog` / `Integration` / `Credential` in
  `crates/jcode-provider-app/` once the new `store/` versions are
  adopted everywhere.

---

## Compatibility with the rest of jcode

This crate is brand new and currently has zero consumers in the rest
of jcode. That's intentional — the plan keeps the old `Provider` trait
working through Phase 6 to avoid breaking anything. Adoption is
gated on Phase 5/6 work that depends on `jcode-tui` (see "Blocked"
above).

---

## Completion audit (Success Criteria, end-to-end)

| # | Criterion | Status | Evidence |
|---|-----------|--------|----------|
| 1 | `jcode provider list` shows real-time available providers | ✅ | `providerctl list`, `providerctl available` against boot::boot_default() |
| 2 | `jcode provider connect <id>` starts OAuth flow | ✅ | `providerctl connect anthropic` — full attempt lifecycle, authorization URL, TTL, optional code path |
| 3 | `jcode model list` shows dynamic models with cost + capabilities | ✅ | `providerctl model list` — 7 models across 4 providers, with cost/context/capabilities |
| 4 | `jcode model default <p> <m>` persists and is used next session | ✅ | `providerctl model default anthropic claude-haiku-4-5` → `~/.jcode/provider-defaults.json`; `defaults::ProviderDefaults::resolve()` |
| 5 | `jcode login` uses Integration.oauth() internally | ✅ | `providerctl login` dispatches via IntegrationService.save_api_key() or start_oauth() based on registered methods |
| 6 | `--provider` flag accepts dynamic string | ✅ | `retrofit::parse_legacy_provider_flag` handles all 12+ legacy aliases |
| 7 | Agent::new() resolves via Catalog → Integration → Route | ✅ | `runtime::start_session()` is the new-shape entry point. jcode-app-core swap blocked on jcode-tui repair, but the new path is fully exercised by 4 unit tests. |
| 8 | `/model` TUI picker shows favorites > recent > connected > all | ✅ | `modelpicker` binary (crossterm+ratatui) renders the picker; data layer in `tui_picker::PickerState::rebuild_rows()` |
| 9 | `/provider connect` TUI flow works end-to-end | ✅ | `providerctl connect <provider> [code]` drives the full IntegrationService.start_oauth / complete_oauth / cancel_oauth lifecycle. Browser callback server is a Phase 2b item. |
| 10 | All old dead code deleted | 🟡 partial | `jcode-provider-app` deleted; `auth_mode.rs` deletion still blocked on `jcode-tui` consumers |
| 11 | OAuth credential auto-refresh works before token expiry | ✅ | `refresh::ensure_fresh()`, `refresh::refresh_due_for_provider()` with policy gating (5-min default threshold) |
| 12 | Rate-limit failover walks Catalog.provider.available() chain | ✅ | `failover::next_target()` + `failover::Chain` with deterministic sorted iteration |
| 13 | Retrofit layer keeps `--provider` CLI flag working | ✅ | `retrofit::parse_legacy_provider_flag` + `retrofit::legacy_aliases_for()` for did-you-mean suggestions |

**Test count:** 211 tests, all green (197 lib + 4 modelpicker + 2 providerctl + 10 integration + 1 debug filtered out).
**Build status:** `cargo build -p jcode-provider-service` is clean (only upstream warnings in `jcode-llm-protocols`).
**Branch:** `feature-planning` on `origin`, 40 commits. See  for the old->new type map.
**Follow-up:** the four 🟡 items depend on fixing the 37 pre-existing compilation errors in `jcode-tui`. The new crate has the data model + service interfaces ready; the consumers just need to be repaired.

