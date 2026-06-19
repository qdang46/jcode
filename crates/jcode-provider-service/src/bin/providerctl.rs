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
            let provider = args.get(2).context("usage: providerctl login <provider> <key>")?;
            let key = args.get(3).context("missing API key")?;
            cmd_login(&svc, provider, key).await
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
        "help" | "-h" | "--help" => {
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
        "providerctl — jcode-provider-service test CLI\n\
         \n\
         USAGE:\n  \
             providerctl <command> [args...]\n\
         \n\
         COMMANDS:\n  \
             list                       List all registered providers\n  \
             available                  List providers with credentials\n  \
             show <provider>            Show one provider's details\n  \
             login <provider> <key>     Save an API key for a provider\n  \
             logout <provider>          Remove all credentials for a provider\n  \
             default                    Show the default (provider, model)\n  \
             small                      Show the cheapest small model\n  \
             resolve <provider> [model] Print the resolved Route as JSON\n  \
             help                       Print this help\n\
         \n\
         EXAMPLES:\n  \
             providerctl login anthropic sk-ant-...\n  \
             providerctl resolve anthropic claude-sonnet-4-6"
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
    _svc: &DefaultProviderService,
) -> Result<()> {
    // The Integration layer is where providers are registered in Phase 4a.
    // Use the underlying integration handle.
    let integration = _svc.integration();
    for p in integration.list().await? {
        println!("{}\t{}", p.id, p.label);
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
