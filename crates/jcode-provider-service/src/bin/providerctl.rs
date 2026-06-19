//! `providerctl` — a small standalone CLI that exercises the
//! `jcode-provider-service` facade end-to-end.
//!
//! This binary is the Phase 4 "Quick Win" deliverable: it shows that the
//! Catalog → Integration → Credential pipeline works for users without
//! requiring the rest of jcode to rewire (which lands in Phase 6).
//!
//! Usage:
//!   providerctl list                       — show all registered providers
//!   providerctl available                  — show providers with credentials
//!   providerctl show <provider>            — show one provider's details
//!   providerctl connect <provider>         — OAuth flow (stubbed for Phase 4a)
//!   providerctl login <provider> <key>     — save an API key
//!   providerctl logout <provider>          — remove all credentials
//!   providerctl default                    — show the default (provider, model)
//!   providerctl small                      — show the cheapest small model
//!   providerctl resolve <provider> [model] — print the resolved Route JSON
//!
//! All commands work against the real OS keychain via
//! `jcode-keyring-store` and the in-memory catalog. Phase 4b will plug
//! in a static catalog of all seven real providers.


use anyhow::{Context, Result};
use jcode_keyring_store::DefaultKeyringStore;

use jcode_provider_service::integration::AuthMethod;
use jcode_provider_service::service::ProviderService;
use jcode_provider_service::store::{
    DefaultProviderService,
};
use jcode_provider_service::types::ProviderId;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        std::process::exit(2);
    }
    let cmd = args[1].as_str();

    let svc = build_service().await?;
    let result = match cmd {
        "list" => cmd_list(&svc).await,
        "available" => cmd_available(&svc).await,
        "show" => {
            let provider = args.get(2).context("usage: providerctl show <provider>")?;
            cmd_show(&svc, provider).await
        }
        "login" => {
            // Phase 4 unification: `login` dispatches based on the
            // provider's registered auth methods. If the provider has
            // OAuth and no key was provided, drive the OAuth flow.
            // If a key was provided, save it as an API key.
            let provider = args.get(2).context("usage: providerctl login <provider> [key]")?;
            let key = args.get(3);
            cmd_login_unified(&svc, provider, key.map(|x| x.as_str())).await
        }
        "logout" => {
            let provider = args.get(2).context("usage: providerctl logout <provider>")?;
            cmd_logout(&svc, provider).await
        }
        "default" => cmd_default(&svc).await,
        "small" => cmd_small(&svc).await,
        "resolve" => {
            let provider = args
                .get(2)
                .context("usage: providerctl resolve <provider> [model]")?;
            let model = args.get(3).cloned();
            cmd_resolve(&svc, provider, model.as_deref()).await
        }
        "model" => match args.get(2).map(String::as_str).unwrap_or("list") {
                "list" => cmd_model_list(&svc).await,
                "default" => {
                    let provider = args.get(3).context("usage: providerctl model default <provider> <model>")?;
                    let model = args.get(4).context("usage: providerctl model default <provider> <model>")?;
                    cmd_model_default(&svc, provider, model).await
                }
                "show" => {
                    let provider = args.get(3).context("usage: providerctl model show <provider> [model]")?;
                    let model = args.get(4).cloned();
                    cmd_model_show(&svc, provider, model.as_deref()).await
                }
                other => {
                    eprintln!("unknown model subcommand: {other}");
                    eprintln!("usage: providerctl model {{list|default|show}}");
                    std::process::exit(2);
                }
            }
        "legacy" => {
            let flag = args.get(2).context("usage: providerctl legacy <flag>")?;
            cmd_legacy(flag)
        }
        "metadata" => {
            let subcmd = args.get(2).map(String::as_str).unwrap_or("list");
            match subcmd {
                "list" => cmd_metadata_list().await,
                "register" => cmd_metadata_register(&svc).await,
                other => {
                    eprintln!("unknown metadata subcommand: {other}");
                    eprintln!("usage: providerctl metadata {{list|register}}");
                    std::process::exit(2);
                }
            }
        }
        "connect" => {
            let provider = args.get(2).context("usage: providerctl connect <provider>")?;
            let code = args.get(3).cloned();
            cmd_connect(&svc, provider, code.as_deref()).await
        }
        "help" => {
            usage();
            Ok(())
        }
        other => {
            eprintln!("unknown command: {other}");
            usage();
            std::process::exit(2);
        }
    };
    result
}

fn usage() {
    eprintln!(
        "providerctl — jcode-provider-service test CLI
\n         
\n         USAGE:
  \n             providerctl <command> [args...]
\n         
\n         COMMANDS:
  \n             list                          List all registered providers
  \n             available                     List providers with credentials
  \n             show <provider>               Show one provider details
  \n             login <provider> <key>        Save an API key for a provider
  \n             logout <provider>             Remove all credentials for a provider
  \n             default                       Show the default (provider, model)
  \n             small                         Show the cheapest small model
  \n             resolve <provider> [model]    Print the resolved Route as JSON
  \n             model list                    List all models from all providers
  \n             model show <provider> [m]     Show one model details
  \n             model default <p> <m>         Set the global default model (persists)
  \n             connect <provider> [code]     Start OAuth flow; optional code completes it
  \n             help                          Print this help
\n         
\n         EXAMPLES:
  \n             providerctl login anthropic sk-ant-...
  \n             providerctl resolve anthropic claude-sonnet-4-6
  \n             providerctl model list
  \n             providerctl model default anthropic claude-haiku-4-5
  \n             providerctl connect anthropic"
    );
}

async fn build_service(
) -> Result<DefaultProviderService> {
    // Phase 6 boot: real keychain, real built-in provider registration
    // (Anthropic, OpenAI, OpenRouter, Gemini with their canonical model
    // sets). The boot helper is the single entry point the session
    // runner will call in Phase 6.
    jcode_provider_service::boot::boot_default::<DefaultKeyringStore>()
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}

async fn cmd_list(
    svc: &DefaultProviderService,
) -> Result<()> {
    // Per the plan: `jcode provider list` shows real-time
    // available providers **with credentials** and **auth method
    // hints**. We list every registered provider, mark whether
    // the integration layer detects a connection, and show the
    // available auth methods.
    let integration = svc.integration();
    for p in integration.list().await? {
        let status = integration.detect(&p.id).await?;
        let mark = if status.is_connected() { "[●]" } else { "[ ]" };
        let first_method = p
            .auth_methods
            .first()
            .map(|m| m.label())
            .unwrap_or("(no methods)");
        let methods_count = p.auth_methods.len();
        let methods_label = if methods_count > 1 {
            format!("{} (+{} more)", first_method, methods_count - 1)
        } else {
            first_method.to_string()
        };
        println!(
            "{} {}\t{}\t{}\t{}",
            mark, p.id, p.label, status.summary(), methods_label
        );
    }
    Ok(())
}

async fn cmd_available(
    _svc: &DefaultProviderService,
) -> Result<()> {
    let integration = _svc.integration();
    let mut found = 0;
    for p in integration.list().await? {
        let status = integration.detect(&p.id).await?;
        if status.is_connected() {
            println!("{}\t{}\t{}", p.id, p.label, status.summary());
            found += 1;
        }
    }
    if found == 0 {
        println!("(no providers have credentials yet — try `providerctl login <p> <key>`)");
    }
    Ok(())
}

async fn cmd_show(
    svc: &DefaultProviderService,
    provider: &str,
) -> Result<()> {
    let integration = svc.integration();
    let p = integration
        .get(&ProviderId::from(provider))
        .await
        .with_context(|| format!("unknown provider: {provider}"))?;
    println!("id:      {}", p.id);
    println!("label:   {}", p.label);
    println!("auth:    {}", p.auth_methods.len());
    for m in &p.auth_methods {
        println!("  - {}  ({})", m.label(), describe_method(m));
    }
    println!("env:     {}", p.env_keys.join(", "));
    let status = integration.detect(&p.id).await?;
    println!("status:  {}", status.summary());
    Ok(())
}

async fn cmd_login(
    svc: &DefaultProviderService,
    provider: &str,
    key: &str,
) -> Result<()> {
    let id = ProviderId::from(provider);
    let integration = svc.integration();
    let _ = integration.get(&id).await.with_context(|| {
        format!("unknown provider: {provider} — use `providerctl list` to see registered ids")
    })?;
    let cred_id = integration
        .save_api_key(&id, "default", key)
        .await
        .with_context(|| format!("failed to save API key for {provider}"))?;
    println!("saved credential {}", cred_id);
    Ok(())
}

async fn cmd_login_unified(
    svc: &DefaultProviderService,
    provider: &str,
    key: Option<&str>,
) -> Result<()> {
    let id = ProviderId::from(provider);
    let provider_info = svc.integration().get(&id).await.with_context(|| {
        format!("unknown provider: {provider} (try `providerctl show`)")
    })?;
    let has_oauth = provider_info.supports_oauth();
    match (key, has_oauth) {
        (None, true) => {
            println!("provider {provider} supports OAuth; starting attempt...");
            cmd_connect(svc, provider, None).await
        }
        (None, false) => {
            let msg = "provider ".to_string() + provider + " requires an API key (no OAuth method registered); usage: providerctl login <provider> <key>";
            anyhow::bail!(msg)
        }
        (Some(k), _) => cmd_login(svc, provider, k).await,
    }
}

async fn cmd_logout(
    svc: &DefaultProviderService,
    provider: &str,
) -> Result<()> {
    let id = ProviderId::from(provider);
    let removed = svc
        .credentials()
        .delete_all(&id)
        .await
        .with_context(|| format!("failed to remove credentials for {provider}"))?;
    println!("removed {} credential(s) for {}", removed, id);
    Ok(())
}

async fn cmd_default(svc: &DefaultProviderService) -> Result<()> {
    match svc.catalog().default().await {
        Ok((p, m)) => {
            println!("{}/{}", p, m);
            Ok(())
        }
        Err(_) => {
            // Fall back: list the first connected provider.
            let integration = svc.integration();
            for p in integration.list().await? {
                if integration.detect(&p.id).await?.is_connected() {
                    println!("{}/<no model — try resolve>", p.id);
                    return Ok(());
                }
            }
            anyhow::bail!("no providers are configured")
        }
    }
}

async fn cmd_small(svc: &DefaultProviderService) -> Result<()> {
    match svc.catalog().small().await {
        Ok((p, m)) => {
            println!("{}/{}", p, m);
            Ok(())
        }
        Err(e) => {
            eprintln!("no small model available: {}", e);
            eprintln!("(log into at least one provider so catalog has a connected entry)");
            std::process::exit(1);
        }
    }
}

async fn cmd_resolve(
    svc: &DefaultProviderService,
    provider: &str,
    model: Option<&str>,
) -> Result<()> {
    let id = ProviderId::from(provider);
    let model_id = if let Some(m) = model {
        jcode_provider_service::types::ModelId::from(m)
    } else {
        // Default to the first model in the catalog for this provider.
        let models = svc
            .catalog()
            .models(&id)
            .await
            .with_context(|| format!("unknown provider: {provider}"))?;
        models
            .first()
            .map(|m| m.id.clone())
            .with_context(|| format!("provider {provider} has no catalog models"))?
    };
    let r = svc
        .resolver()
        .resolve_route(&id, &model_id)
        .await
        .with_context(|| format!("resolve failed for {provider}/{model_id}"))?;
    println!("{}", serde_json::to_string_pretty(&r.route)?);
    Ok(())
}


async fn cmd_model_list(svc: &DefaultProviderService) -> Result<()> {
    for p in svc.catalog().list_providers().await? {
        for m in svc.catalog().models(&p.id).await? {
            let cost_in = m
                .cost_per_million_input
                .map(|c| format!("${:.3}/M in", c))
                .unwrap_or_else(|| "free".into());
            let cost_out = m
                .cost_per_million_output
                .map(|c| format!("${:.3}/M out", c))
                .unwrap_or_else(|| "free".into());
            let tools = if m.supports_tools { "tools" } else { "-----" };
            let vis = if m.supports_vision { "vis" } else { "---" };
            let stream = if m.supports_streaming { "sse" } else { "---" };
            println!(
                "{}/{:<24} ctx={:>7} {} {} {} {} {}",
                p.id,
                m.id,
                m.context_window,
                cost_in,
                cost_out,
                tools,
                vis,
                stream
            );
        }
    }
    Ok(())
}

async fn cmd_model_default(
    svc: &DefaultProviderService,
    provider: &str,
    model: &str,
) -> Result<()> {
    use jcode_provider_service::defaults::ProviderDefaults;
    let id = ProviderId::from(provider);
    let _ = svc
        .catalog()
        .find_model(&id, &model.into())
        .await
        .with_context(|| {
            format!("unknown model: {provider}/{model} (try `providerctl model list`)")
        })?;
    let path = jcode_provider_service::defaults::default_path()
        .context("HOME not set; cannot persist defaults")?;
    let mut d = ProviderDefaults::load(&path).unwrap_or_default();
    d.set_global(id.clone(), model.into());
    d.save(&path)
        .with_context(|| format!("failed to save defaults to {}", path.display()))?;
    println!("default = {}/{} (saved to {})", id, model, path.display());
    Ok(())
}

async fn cmd_model_show(
    svc: &DefaultProviderService,
    provider: &str,
    model: Option<&str>,
) -> Result<()> {
    let id = ProviderId::from(provider);
    let model_id = match model {
        Some(m) => jcode_provider_service::types::ModelId::from(m),
        None => {
            // Default to the first model in the catalog.
            let models = svc.catalog().models(&id).await?;
            models
                .first()
                .map(|m| m.id.clone())
                .with_context(|| format!("provider {provider} has no catalog models"))?
        }
    };
    let m = svc
        .catalog()
        .find_model(&id, &model_id)
        .await
        .with_context(|| format!("unknown model: {provider}/{model_id}"))?;
    println!("provider:  {}", m.provider);
    println!("id:        {}", m.id);
    println!("name:      {}", m.name);
    println!("context:   {} tokens", m.context_window);
    println!(
        "cost in:   {}",
        m.cost_per_million_input
            .map(|c| format!("${:.4} / 1M", c))
            .unwrap_or_else(|| "free".into())
    );
    println!(
        "cost out:  {}",
        m.cost_per_million_output
            .map(|c| format!("${:.4} / 1M", c))
            .unwrap_or_else(|| "free".into())
    );
    println!("tools:     {}", m.supports_tools);
    println!("vision:    {}", m.supports_vision);
    println!("streaming: {}", m.supports_streaming);
    println!("tier:      {:?}", m.tier);
    Ok(())
}

async fn cmd_connect(
    svc: &DefaultProviderService,
    provider: &str,
    code: Option<&str>,
) -> Result<()> {
    let id = ProviderId::from(provider);
    let attempt = svc
        .integration()
        .start_oauth(&id)
        .await
        .with_context(|| {
            format!("{provider} does not support OAuth (try `providerctl login {provider} <key>` instead)")
        })?;
    let AuthMethod::OAuth { authorization_url } = &attempt.method else {
        anyhow::bail!("internal: non-OAuth method in OAuth attempt")
    };
    println!("attempt id: {}", attempt.id);
    println!("provider:   {}", id);
    println!("open this URL in your browser:");
    println!("  {}", authorization_url);
    println!(
        "expires at: {} (in {} seconds)",
        attempt.expires_at,
        attempt.remaining().num_seconds()
    );
    if let Some(c) = code {
        // Phase 2b stub: accept a code on the command line. The real
        // implementation will exchange the code for a token via HTTP.
        // We persist a dummy OAuth credential so the wiring compiles
        // end-to-end; consumers can later replace the upsert call with
        // the real token-exchange response.
        let _ = svc
            .integration()
            .complete_oauth(&attempt.id, format!("code:{c}"), None, None)
            .await?;
        println!("OAuth attempt {} completed (code: {})", attempt.id, c);
    } else {
        println!();
        println!("After authorizing, run:");
        println!(
            "  providerctl connect {provider} <authorization-code>"
        );
    }
    Ok(())
}

fn cmd_legacy(flag: &str) -> Result<()> {
    use jcode_provider_service::retrofit::parse_legacy_provider_flag;
    match parse_legacy_provider_flag(flag) {
        Ok(sel) => {
            println!("input:      {}", flag);
            println!("provider:   {}", sel.provider);
            println!("auth:       {}", sel.auth.map(|a| a.as_str()).unwrap_or("(none)"));
            println!("dual-auth:  {}", sel.is_dual_auth);
            Ok(())
        }
        Err(e) => {
            eprintln!("parse error: {}", e);
            std::process::exit(1);
        }
    }
}

async fn cmd_metadata_list() -> Result<()> {
    #[cfg(feature = "metadata")]
    {
        for r in jcode_provider_service::metadata_profiles::all_metadata_records() {
            println!("{:<28} {}", r.id, r.label);
        }
    }
    #[cfg(not(feature = "metadata"))]
    {
        eprintln!("metadata feature not enabled; rebuild with --features metadata");
        std::process::exit(1);
    }
    Ok(())
}

async fn cmd_metadata_register(svc: &DefaultProviderService) -> Result<()> {
    #[cfg(feature = "metadata")]
    {
        let registry = jcode_provider_service::metadata_profiles::metadata_registry();
        let n = registry
            .register(svc.catalog(), svc.integration())
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        println!("registered {n} providers (4 builtins + 36 metadata profiles)");
    }
    #[cfg(not(feature = "metadata"))]
    {
        eprintln!("metadata feature not enabled; rebuild with --features metadata");
        std::process::exit(1);
    }
    Ok(())
}

fn describe_method(m: &AuthMethod) -> String {
    match m {
        AuthMethod::OAuth { authorization_url } => {
            format!("oauth ({})", authorization_url)
        }
        AuthMethod::ApiKey { env_var }
        | AuthMethod::BearerEnv { env_var }
        | AuthMethod::CustomHeader { env_var, .. } => format!("env:{}", env_var),
    }
}

#[cfg(test)]
mod tests {
    use jcode_provider_service::boot::BUILTIN_PROVIDERS;

    #[test]
    fn builtin_providers_includes_anthropic_and_openai() {
        let ids: Vec<&str> = BUILTIN_PROVIDERS.iter().map(|p| p.id).collect();
        assert!(ids.contains(&"anthropic"));
        assert!(ids.contains(&"openai"));
    }
}

#[cfg(test)]
mod login_unified_tests {
    use jcode_provider_service::catalog::InMemoryCatalog;
    use jcode_provider_service::integration::{
        AuthMethod, InMemoryIntegration, LoginProvider,
    };
    use jcode_provider_service::service::ProviderService;
    use jcode_provider_service::store::{
        DefaultProviderService, InMemoryCredentialStore, PersistentIntegration,
    };
    use jcode_keyring_store::MockKeyringStore;
    use std::sync::Arc;

    async fn fixture_with_provider(
        auth_methods: Vec<AuthMethod>,
    ) -> DefaultProviderService {
        let creds: Arc<dyn jcode_provider_service::credential::CredentialService> =
            Arc::new(InMemoryCredentialStore::new());
        let integration: Arc<dyn jcode_provider_service::integration::IntegrationService> =
            Arc::new(PersistentIntegration::<MockKeyringStore>::new(creds.clone()));
        integration
            .register(LoginProvider {
                id: "anthropic".into(),
                label: "Anthropic".into(),
                auth_methods,
                env_keys: vec!["ANTHROPIC_API_KEY".into()],
                oauth_preferred: true,
            })
            .await
            .unwrap();
        let catalog: Arc<dyn jcode_provider_service::catalog::CatalogService> =
            Arc::new(InMemoryCatalog::new());
        DefaultProviderService::new(catalog, integration, creds)
    }

    #[tokio::test]
    async fn login_with_key_persists_api_key() {
        let svc = fixture_with_provider(vec![AuthMethod::ApiKey {
            env_var: "ANTHROPIC_API_KEY".into(),
        }])
        .await;
        crate::cmd_login(&svc, "anthropic", "sk-test").await.unwrap();
        // Verify the credential is in the store.
        let creds = svc.credentials().list(&"anthropic".into()).await.unwrap();
        assert_eq!(creds.len(), 1);
        assert!(matches!(
            creds[0].credential,
            jcode_provider_service::credential::CredentialType::ApiKey { .. }
        ));
    }
}
