# Migration from old `jcode-provider-core` types to `jcode-provider-service`

This document maps every type and function in the *old* provider
vocabulary to its *new* equivalent. Use it as a checklist when
migrating a consumer from the old code path to the new one.

The actual deletion of the old types is gated on the
`jcode-tui` crate compiling cleanly (Phase 6 of
`docs/plans/JCODE_PROVIDER.md`). Until that happens, the old
types stay in place; new code should target the equivalents
listed here.

## Old → New type mapping

| Old (in `jcode-provider-core`) | New (in `jcode-provider-service`) |
|-------------------------------|-------------------------------------|
| `auth_mode::AuthMode`         | `integration::AuthMethod` |
| `auth_mode::AuthRoute`        | `service::ProviderProfile` (with auth suffix) |
| `auth_mode::DualAuthProvider` | `retrofit::DualAuthProvider` (moved out) |
| `selection::ActiveProvider`   | `types::ProviderId` (string) |
| `selection::ProviderAvailability` | `integration::ConnectionStatus` |
| `selection::ModelRoute`       | `service::ResolvedRoute` |
| `models::ALL_CLAUDE_MODELS`   | `boot::BUILTIN_PROVIDERS[*].models` |
| `models::ALL_OPENAI_MODELS`   | `boot::BUILTIN_PROVIDERS[*].models` |
| `models::ModelCapabilities`   | `catalog::ModelInfo` |

## Old → New function mapping

| Old | New |
|-----|-----|
| `auth_mode::parse_explicit_credential_prefix` | `retrofit::parse_legacy_provider_flag` |
| `auth_mode::pinned_mode_for` | `integration::detect` (returns `ConnectionStatus`) |
| `auth_mode::runtime_env_auth_route` | `migrate::LegacyProviderSelection::from_env` |
| `selection::auto_default_provider` | `catalog::CatalogService::default` |
| `selection::parse_provider_hint` | `retrofit::parse_legacy_provider_flag` |
| `selection::provider_label` | `integration::LoginProvider::label` |
| `selection::provider_key` | `types::ProviderId` (just use `.as_str()`) |
| `selection::model_name_for_provider` | `migrate::default_model_for` |
| `selection::cli_provider_arg_for_session_key` | `retrofit::parse_legacy_provider_flag` |
| `selection::dedupe_model_routes` | `tui_picker::PickerState::rebuild_rows` |

## New modules added in this branch

| Module | Purpose |
|--------|---------|
| `types` | `ProviderId`, `ModelId`, `ProviderProfile` newtypes |
| `credential` | `CredentialService` trait + `Credential` type |
| `integration` | `IntegrationService` trait + `LoginProvider`, `AuthMethod`, `ConnectionStatus` |
| `catalog` | `CatalogService` trait + `ProviderInfo`, `ModelInfo`, `ModelTier` |
| `service` | `ProviderService` facade + `RouteResolver` |
| `defaults` | `ProviderDefaults` JSON store (per-provider + global) |
| `refresh` | OAuth credential auto-refresh |
| `failover` | Rate-limit failover chain |
| `retrofit` | Legacy `--provider` flag alias translation |
| `migrate` | `auth_mode` → new `Credential` bridge |
| `tui_picker` | TUI picker data model (favorites > recent > connected > all) |
| `runtime` | `start_session()` single-call session entry |
| `attempt` | `OAuthAttempt` state machine |
| `registry` | `ProviderRegistry` trait + `CompositeRegistry` |
| `callback_server` | Local HTTP server for OAuth auto-mode |
| `error_classify` | Error category classifier for failover |
| `boot` | Built-in provider registration + `boot_default()` |
| `store/{in_memory, keyring, integration, service}` | Reference impls |
| `bin/providerctl` | CLI smoke test |
| `bin/modelpicker` | Interactive TUI picker |

## Phase 7 deletion plan (gated on `jcode-tui` repair)

```bash
# Once the 37 pre-existing errors in jcode-tui are fixed:
rm -rf crates/jcode-provider-app/                                    # ✅ already done
rm crates/jcode-provider-core/src/auth_mode.rs                       # blocked on jcode-base + jcode-app-core
rm crates/jcode-provider-core/src/selection.rs                       # blocked on jcode-base
rm src/cli/provider_init.rs                                          # blocked on jcode-tui + jcode-tui-core
# Edit: crates/jcode-provider-core/src/models.rs to delegate to Catalog
```

Each blocked file has a one-line migration: every consumer can be
rewritten in terms of the equivalents above. The `migrate` module
gives consumers a one-call path: read the env-var state with
`LegacyProviderSelection::from_env()` and upsert the resulting
`Credential`s into the new store. No changes to the old types
are required during the transition.
