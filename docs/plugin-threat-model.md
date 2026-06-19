# Plugin System Threat Model

This document applies the STRIDE framework to jcode's plugin runtime. Each threat category identifies a class of attack, evaluates how jcode's architecture mitigates it, and documents any residual risk.

## Scope

**In scope:**
- Plugin loading pipeline (discovery, preflight, transpilation, sandbox evaluation)
- Plugin runtime (QuickJS sandbox, RcuDispatcher, API bindings)
- Capability system (CapabilityChain, access decisions, audit trail)
- Tool registration and event dispatch
- Distribution paths (local path, git clone, Rust workspace crate)

**Out of scope:**
- Build-time supply chain attacks on jcode's own dependencies
- Physical attacks on the host machine
- OS-level sandbox escapes (jcode's plugin sandbox is a userspace sandbox, not an OS sandbox)

---

## Spoofing

**Threat:** A plugin claims an identity it does not own (e.g., a malicious plugin registers with the `package_name` of a trusted plugin, causing the user to load it in place of the legitimate one).

### Mitigation: package_name uniqueness check

When a plugin is loaded, the `PluginManager` or `PluginRegistry` checks that no already-loaded plugin has the same `package_name`. This check runs before any code from the new plugin is evaluated:

1. `PluginLoader::load_one()` reads the manifest from `package.json`.
2. The `package_name` field is extracted and validated against the set of loaded plugin IDs.
3. If a duplicate is found, loading is rejected with a `PluginError::DuplicatePackageName` error.

This prevents a local-path plugin from spoofing a git-cloned plugin, or a git-cloned plugin from spoofing a workspace-crate plugin. The check applies to all three distribution paths uniformly.

### Residual risk

- The `package_name` uniqueness check relies on the manifest being parsed before code execution. If a manifest parser bug allows two different `package_name` values to hash/collide to the same internal identifier, a plugin could bypass the check by using a visually identical but byte-different string (Unicode confusability). The runtime should normalize `package_name` to NFC Unicode form before comparison.
- Rust workspace crates registered via `inventory::submit!` are compiled into the binary and cannot be spoofed by runtime-loaded plugins, but two workspace crates with the same `package_name` would silently conflict at link time. A workspace-level CI check should enforce `package_name` uniqueness across all jcode-ext-* crates.

---

## Tampering

**Threat:** A plugin modifies data outside its declared capabilities, or modifies data it is allowed to read but not write. This includes tampering with session state, other plugins' KV stores, or user files.

### Mitigation: FsConnector scope check

Every capability access goes through the `CapabilityChain` before reaching the resource. For filesystem operations this is the `FsConnector`, which checks that every read/write path falls within one of the plugin's declared `fs_read` or `fs_write` path prefixes:

1. The plugin declares `fs_read` and `fs_write` path patterns in its manifest.
2. The `CapabilityChain` evaluates each access: deny list -> global deny -> allow list -> global default.
3. The `FsConnector` matches paths by prefix: a request to read `/home/user/.jcode/data/my-plugin/stats.json` is allowed if `fs_read` contains `/home/user/.jcode/data/my-plugin`.

For other resource types:
- **HTTP hosts**: `http_hosts` is matched by exact host or `*.suffix` suffix glob.
- **Environment variables**: `env_read` is matched by exact variable name.
- **Shell commands**: `shell_commands` is matched by command prefix glob (e.g., `git *` matches `git log` but not `git commit` if the allow list only permits `git log`).

### Mitigation: Audit trail

Every capability access decision is logged to the `AuditTrail`, a thread-safe ring buffer (`VecDeque` with configurable capacity). Each entry records:

- `timestamp` -- when the access happened
- `plugin_id` -- which plugin made the access
- `resource` -- what resource was accessed
- `action` -- what operation was attempted (Read, Write, Exec, etc.)
- `decision` -- allowed, denied, or needs_approval
- `reason` -- explanation from the capability chain

Logs are accessible via `jcode plugin audit` and can be exported as JSON. This provides tamper evidence: if a plugin accesses data outside its declared capabilities, the access attempt (and its denial) is recorded.

### Residual risk

- Path prefix matching can be bypassed by symbolic links. If a plugin has `fs_read: ["/tmp"]` and the user has a symlink `/tmp/links -> /home/user/secrets`, the plugin can read secrets through the symlink. The runtime does not resolve symlinks before checking path prefixes.
- The `AuditTrail` is an in-memory ring buffer. If the process crashes, all audit entries are lost. A persistent audit log is not yet implemented.
- The capability check runs within the same process. A compromised QuickJS sandbox that achieves Rust-side code execution could bypass the check entirely.

---

## Repudiation

**Threat:** A plugin performs a security-relevant action and leaves no record that would allow an operator to attribute the action to that plugin.

### Mitigation: AuditTrail logging every call

Every call from plugin code to a host resource passes through the `CapabilityChain::check()` method, and every call's result is logged via `AuditTrail::log_access()`. The audit trail includes:

- **Plugin identity**: the `PluginId` (e.g., `npm:analytics-plugin` or `file:/path/to/plugin.ts`).
- **Resource identity**: the exact resource string (file path, host, env var name, tool name).
- **Action type**: what operation was attempted (`Read`, `Write`, `Exec`, `Config`, etc.).
- **Decision**: whether access was `allowed`, `denied`, or `needs_approval`, plus the reason.

Because the audit trail is populated by the host (`jcode-plugin-runtime` code), the plugin itself cannot suppress or modify these entries. A plugin that tries to access a resource outside its capabilities will find both the attempt and the denial logged.

### Residual risk

- The audit trail is not authenticated or signed. If an attacker gains write access to the jcode process's memory, they could inject bogus entries or clear the trail.
- The audit trail is not currently persisted to disk. Process crash or restart loses all entries. Work is tracked under a future requirement for persistent audit logging.
- For workspace crate plugins (Rust, compiled-in), the audit trail entries are the same, but the plugin's code runs natively rather than in the QuickJS sandbox. A native Rust plugin has more avenues to avoid audit logging by calling jcode internals directly instead of going through the capability chain.

---

## Information Disclosure

**Threat:** A plugin reads secret information it should not have access to, such as API keys, environment variables, or user files outside its declared capability scope.

### Mitigation: env_read capability

Environment variables are gated by the `env_read` capability. A plugin must explicitly declare each environment variable it needs:

```json
{
  "jcode": {
    "capabilities": {
      "env_read": ["HOME", "JCODE_API_KEY"]
    }
  }
}
```

Variables not in this list are redacted: the runtime returns an empty string or a sentinel value for undeclared variables.

### Mitigation: Secret redaction

Configuration values declared with `"secret": true` in the plugin's `SettingSchema` are redacted from:
- Log output (replaced with `****`)
- Audit trail entries (replaced with `[REDACTED]`)
- Plugin debug output
- `jcode plugin info` display

The runtime's `SecretRedactor` runs on all logged strings before they enter the tracing system or audit trail. This happens in `PluginApiBindings` before values are passed to plugin code, and again before values are written to the audit log.

### Mitigation: Filesystem scope

The `fs_read` and `fs_write` capabilities define path prefixes. Any access attempt that does not match a declared prefix is denied before the file is opened. This prevents a plugin from reading `/etc/passwd` or `~/.ssh/id_rsa` unless those paths are explicitly allowed.

### Mitigation: Hosts scope

The `http_hosts` capability defines which remote hosts a plugin may contact. A plugin with `http_hosts: []` or an omitted `http_hosts` field may not make any HTTP requests. Host matching uses substring containment for flexibility (e.g., `api.github.com` matches `https://api.github.com/v3/...`), but a malicious plugin cannot use this to reach arbitrary hosts.

### Residual risk

- **Side-channel attacks**: A plugin without network capability could encode data in timing, error messages, or other observable behaviors of the host. Mitigating side channels fully requires formal information-flow control, which is not implemented.
- **Capacity-based inference**: A plugin without `fs_read` access could still observe how long certain operations take, potentially inferring the existence of files. This is mitigated by the small API surface but not eliminated.
- **Environment variable enumeration**: A plugin could try common environment variable names to infer which are available, even if it cannot read their values. This is rate-limited by `max_hostcalls_per_sec` but not prevented.

---

## Denial of Service

**Threat:** A plugin makes excessive calls to host resources, consumes excessive CPU time or memory in the QuickJS sandbox, or holds onto resources indefinitely, degrading or denying service for other plugins or the main jcode process.

### Mitigation: max_hostcalls_per_sec quota

Each plugin declares a `max_hostcalls_per_sec` quota in its capabilities. The runtime enforces this with a token-bucket rate limiter in the bridge layer:

```json
{
  "jcode": {
    "capabilities": {
      "max_hostcalls_per_sec": 100
    }
  }
}
```

The rate limiter counts all capability-gated calls (fs_read, fs_write, http_hosts access, env_read, shell_commands) and drops or delays calls that exceed the quota. The default quota for plugins that do not declare this field is derived from the configured policy mode:
- `Strict` mode: 50 calls/sec
- `Prompt` mode: 100 calls/sec
- `Permissive` mode: 500 calls/sec

### Mitigation: Timeout

Each tool invocation and event handler has a configurable timeout. The `PluginTimer` tracks wall-clock time and forcibly terminates handler execution when the timeout expires:

| Event type | Default timeout |
|------------|-----------------|
| Informational events (SessionEnd, TurnEnd, PostCompact) | 500 ms |
| Actionable events (PreToolUse, PostToolUse, etc.) | 5000 ms |
| Permission events | 3600000 ms (1 hour, for user interaction) |

For Rust workspace crate plugins, timeouts are enforced at the async task level using `tokio::time::timeout`. For QuickJS plugins, the sandbox's `DualTimeout` mechanism uses both a JavaScript-level timer and a Rust-side watchdog thread to interrupt long-running scripts.

### Mitigation: Resource limits

The QuickJS sandbox imposes limits that prevent runaway plugins from exhausting host resources:

| Resource | Limit | Enforcement |
|----------|-------|-------------|
| Sandbox memory | 64 MiB default | QuickJS runtime memory limit |
| Sleep duration | 5000 ms per call | Hard cap in `make_sleep_fn` |
| Concurrent event handlers | Unlimited by design | Async `join_all` dispatches all handlers concurrently, but each runs in its own micro-task |
| File descriptor usage | N/A (no direct FD access from sandbox) | Sandbox has no `fs` module |

### Residual risk

- **Memory exhaustion via repeated large objects**: The QuickJS memory limit prevents a single large allocation from exhausting host memory, but a plugin could repeatedly allocate and free large objects to trigger GC thrashing that slows the host. The 64 MiB sandbox limit bounds this, but does not eliminate it.
- **CPU spin loops**: A plugin executing `while (true) {}` in JavaScript would block the QuickJS thread indefinitely. The `DualTimeout` watchdog should catch this, but the timeout granularity depends on the watchdog poll interval (currently 100 ms). Tight spin loops may degrade responsiveness for up to one interval.
- **Audit trail exhaustion**: A plugin that triggers many capability checks in quick succession could fill the audit trail ring buffer, pushing legitimate entries out. The `max_hostcalls_per_sec` quota limits this rate.

---

## Elevation of Privilege

**Threat:** A plugin gains the ability to perform an action that its declared capabilities do not permit, or that its declared `ToolTier` should forbid.

### Mitigation: declared_tier() immutability

Every registered tool has a `ToolTier` determined at registration time by the plugin's manifest and approval policy:

```rust
pub enum ToolTier {
    Read,   // Pure read, no I/O, no mutation
    Write,  // Mutates workspace/session state, no subprocesses
    Exec,   // Subprocesses, network, code execution
}
```

The tool manifest's `tier` field is set in `package.json` and parsed once at load time. Once a tool's tier is assigned, it cannot be changed at runtime. The `ApprovalGate` reads the tier from the immutable tool record:

1. Tool is registered via `jcode.registerTool()` -> `PluginRegistry::register_js_tool()`.
2. Tool tier defaults to the plugin's manifest `tier` field (or `Exec` if unset).
3. `ApprovalGate` consults `ToolTier` + `CapabilityChain` before every invocation.
4. If the current permission mode forbids the tool's tier, the call is denied before any handler code runs.

The immutability of the tier table is enforced by the RCU (Read-Copy-Update) pattern in `RcuDispatcher`: the handler registry is snapshotted atomically. A plugin cannot mutate its tools' tiers after registration without a full unload-reload cycle.

### Mitigation: Preflight analysis

Before any plugin code executes, the `PreflightAnalyzer` statically analyzes the source for dangerous patterns:

- `eval()` and `new Function()` -- detected and warned.
- `exec()` / `spawn()` calls -- detected; if no `shell_commands` capability is declared, loading is blocked.
- Dangerous shell commands (`rm -rf`, `sudo`, `chmod 777`) -- detected and loading is blocked regardless of declared capabilities.

The preflight analysis runs outside the sandbox (in pure Rust, on the source text), so even a malicious plugin cannot interfere with it. The analyzer is a `PreflightAnalyzer` struct that operates on the file contents before they are passed to SWC or QuickJS.

### Mitigation: 5-layer capability chain

The `CapabilityChain` implements 5 evaluation layers to prevent privilege escalation:

| Layer | Name | Effect |
|-------|------|--------|
| 1 | Plugin deny list | Explicit deny rules from the plugin's own manifest (self-restriction) |
| 2 | Global deny list | System-wide deny rules set by the user or administrator |
| 3 | Plugin allow list | Capabilities the plugin has declared |
| 4 | Global allow list | System-wide allow rules |
| 5 | Mode fallback | Default behavior from the current policy mode (Strict/Prompt/Permissive/Disabled) |

Even if a plugin somehow bypasses its own deny list (layer 1), the global deny (layer 2) still applies. And even if a plugin is allowed by both allow lists (layers 3+4), the mode fallback (layer 5) can deny it if the current policy mode defaults to deny for the requested action's tool tier.

A tool with `ToolTier::Exec` in `Strict` mode:
- Layer 5 default is "deny" (Strict mode)
- `Exec` tools require explicit allow in Strict mode
- Call is denied before execution

A tool with `ToolTier::Read` in `Permissive` mode:
- Layer 5 default is "allow" (Permissive mode)
- `Read` tools are always permitted
- Call proceeds

### Residual risk

- **QuickJS sandbox escape**: If an attacker achieves arbitrary code execution within the QuickJS runtime (via a vulnerability in `rquickjs` or the QuickJS C engine), they would control the sandbox thread. From there they could:
  - Call the injected Rust functions (`make_sleep_fn`, `make_uuid_fn`, `make_kv_get_fn`) with arbitrary arguments.
  - Access the `__jcode_api` global (aliased as `jcode`) which exposes all `PluginApiBindings`.
  - Attempt to find memory corruption bugs in the FFI boundary between QuickJS and Rust.
  
  Mitigation: the API surface is intentionally small (on, registerTool, logger, kv, sleep, uuid, getConfig, cwd). Each function validates its arguments at the QuickJS-to-Rust boundary. There is no `require()`, no `fetch()`, no `process` access.

- **Capability confusion between distribution paths**: A Rust workspace crate plugin bypasses the QuickJS sandbox entirely -- it runs native Rust code linked into the jcode binary. It must still go through the `CapabilityChain` for resource access, but a malicious Rust plugin could:
  - Directly call jcode internal functions if they are public.
  - Access plugin resource registries directly instead of through the audit-trailed path.
  
  Mitigation: workspace crate plugins are compiled into the binary and reviewed as part of the build. They are not loadable at runtime, so the trust model is different from dynamic plugins. The `jcode-ext-*` convention assumes first-party trust.

- **`inventory::submit!` trust**: The `inventory` crate collects registrations at link time. Any `jcode-ext-*` crate in the workspace has its `inventory::submit!` entry unconditionally registered. There is no capability check at registration time -- the check only happens at invocation time.

---

## Summary

| Threat | Primary mitigation | Residual risk level |
|--------|-------------------|---------------------|
| Spoofing | `package_name` uniqueness check | Low -- Unicode normalization gap |
| Tampering | `FsConnector` scope check + `AuditTrail` | Medium -- symlink bypass, in-memory audit |
| Repudiation | `AuditTrail` logging every call | Medium -- no persistent audit |
| Information disclosure | `env_read` capability + secret redaction | Low -- side channels not mitigated |
| Denial of service | `max_hostcalls_per_sec` quota + timeout | Medium -- CPU spin loops, GC thrash |
| Elevation of privilege | `declared_tier()` immutability + preflight + 5-layer chain | Low -- QuickJS escape risk for TS plugins, higher for Rust workspace crates |

## References

- [Plugin Author Guide](./plugins.md) -- how to write plugins
- [Plugin API Reference](./plugins/api-reference.md) -- complete API surface
- [Plugin System v2 Hardening Plan](./plugin-hardening-v2.md) -- implementation plan for the security model
- [Safety System](./SAFETY_SYSTEM.md) -- jcode's wider safety architecture
- [Audit Trail implementation](../crates/jcode-plugin-runtime/src/audit.rs) -- ring-buffer audit source
- [CapabilityChain implementation](../crates/jcode-plugin-core/src/security.rs) -- capability evaluation source
- [PluginApiBindings implementation](../crates/jcode-plugin-runtime/src/api.rs) -- JS-to-Rust bridge source
