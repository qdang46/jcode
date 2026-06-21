//! `modelpicker` — an interactive TUI picker for the provider/model
//! catalog. Implements Phase 5 of `docs/plans/JCODE_PROVIDER.md`.
//!
//! This is a *stand-alone* TUI built on top of the cross-platform
//! `crossterm` crate so it doesn't depend on the (currently broken)
//! `jcode-tui` crate. When `jcode-tui` is repaired, the rendering
//! surface in this binary can be ported into the in-process picker
//! without changing the data model (which lives in
//! `jcode_provider_service::tui_picker`).
//!
//! Usage:
//!   modelpicker                 — open the picker (uses ~/.jcode
//!                                for the credential store; falls
//!                                back to the mock keyring under
//!                                the MOCK_KEYRING env var for
//!                                testing)

use std::collections::HashSet;
use std::io::{Stdout, stdout};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use jcode_keyring_store::{DefaultKeyringStore, MockKeyringStore};
use jcode_provider_service::catalog::CatalogService;
use jcode_provider_service::integration::IntegrationService;
use jcode_provider_service::service::ProviderService;
use jcode_provider_service::store::DefaultProviderService;
use jcode_provider_service::tui_picker::{Filter, PickerState, RowOrigin};
use jcode_provider_service::types::ModelId;
use jcode_provider_service::types::ProviderId;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

type Term = Terminal<CrosstermBackend<Stdout>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let svc = build_service().await?;
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, &svc).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn build_service() -> Result<DefaultProviderService> {
    // The mock keyring is used under MOCK_KEYRING=1 for tests, so the
    // picker can be exercised without touching the real OS keychain.
    let svc = if std::env::var("MOCK_KEYRING").is_ok() {
        let keyring = Arc::new(MockKeyringStore::new());
        let credentials: Arc<dyn jcode_provider_service::credential::CredentialService> = Arc::new(
            jcode_provider_service::store::KeyringCredentialStore::new(keyring),
        );
        let integration: Arc<dyn IntegrationService> =
            Arc::new(jcode_provider_service::store::PersistentIntegration::<
                MockKeyringStore,
            >::new(credentials.clone()));
        let catalog: Arc<dyn CatalogService> =
            Arc::new(jcode_provider_service::catalog::InMemoryCatalog::new());
        jcode_provider_service::boot::register_builtins::<MockKeyringStore>(
            catalog.as_ref(),
            integration.as_ref(),
        )
        .await?;
        DefaultProviderService::new(catalog, integration, credentials)
    } else {
        let keyring = Arc::new(DefaultKeyringStore::new());
        let credentials: Arc<dyn jcode_provider_service::credential::CredentialService> = Arc::new(
            jcode_provider_service::store::KeyringCredentialStore::new(keyring),
        );
        let integration: Arc<dyn IntegrationService> =
            Arc::new(jcode_provider_service::store::PersistentIntegration::<
                DefaultKeyringStore,
            >::new(credentials.clone()));
        let catalog: Arc<dyn CatalogService> =
            Arc::new(jcode_provider_service::catalog::InMemoryCatalog::new());
        jcode_provider_service::boot::register_builtins::<DefaultKeyringStore>(
            catalog.as_ref(),
            integration.as_ref(),
        )
        .await?;
        DefaultProviderService::new(catalog, integration, credentials)
    };
    Ok(svc)
}

fn setup_terminal() -> Result<Term> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(term: &mut Term) -> Result<()> {
    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    term.show_cursor()?;
    Ok(())
}

async fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    svc: &DefaultProviderService,
) -> Result<()> {
    // Build the connected-provider set.
    let mut connected = HashSet::new();
    for p in svc.integration().list().await? {
        if svc.integration().detect(&p.id).await?.is_connected() {
            connected.insert(p.id);
        }
    }
    let mut state = PickerState::new();
    // Load persisted favorites from ~/.jcode/model_prefs.json
    // (per the plan: 'f' toggles favorite, persisted to
    // model_prefs.json). Falls back to empty if the file is
    // missing or malformed.
    let (initial_favorites, initial_recents): (
        std::collections::HashSet<(ProviderId, ModelId)>,
        Vec<(ProviderId, ModelId)>,
    ) = if let Some(path) = jcode_provider_service::model_prefs::default_path() {
        jcode_provider_service::model_prefs::ModelPrefs::load(&path)
            .map(|p| {
                (
                    p.favorites_set(),
                    p.recents
                        .iter()
                        .map(|e| (e.provider.clone(), e.model.clone()))
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or_default()
    } else {
        (std::collections::HashSet::new(), Vec::new())
    };
    // Seed the picker's in-memory recents from the persistent
    // store. PickerState::rebuild_rows() reads from state.recent
    // and surfaces them under the 'Recent' section header.
    state.recent = initial_recents;
    state
        .rebuild_rows(svc.catalog(), &connected, &initial_favorites)
        .await?;

    let mut filter_input = String::new();
    let mut filter_mode = false;
    let mut should_quit = false;
    let mut selected: Option<(ProviderId, jcode_provider_service::types::ModelId)> = None;

    while !should_quit {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(5),
                    Constraint::Length(3),
                ])
                .split(f.area());
            let header = Paragraph::new(Line::from(vec![
                Span::styled("modelpicker", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("  jcode provider/model picker — "),
                Span::raw(format!("{} rows", state.visible().len())),
            ]))
            .block(Block::default().borders(Borders::ALL).title(" Catalog "));
            f.render_widget(header, chunks[0]);

            let items: Vec<ListItem> = state
                .visible()
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    let marker = if i == state.cursor { "▶ " } else { "  " };
                    let origin = match row.origin {
                        RowOrigin::Favorite => "[F] ",
                        RowOrigin::Recent => "[R] ",
                        RowOrigin::Connected => "[●] ",
                        RowOrigin::Catalog => "[○] ",
                    };
                    let line = format!(
                        "{}{}{:<28} {:<14}",
                        marker,
                        origin,
                        format!("{}/{}", row.provider, row.model),
                        row.label
                    );
                    let style = if i == state.cursor {
                        Style::default()
                            .bg(Color::Blue)
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else if !row.connected {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Line::from(Span::styled(line, style)))
                })
                .collect();
            let list =
                List::new(items).block(Block::default().borders(Borders::ALL).title(" Models "));
            f.render_widget(list, chunks[1]);

            let footer = if filter_mode {
                Paragraph::new(format!("filter: {}_", filter_input))
                    .block(Block::default().borders(Borders::ALL).title(" Filter "))
            } else {
                let hint = "↑/↓ move  / filter  enter select  f favorite  q quit";
                Paragraph::new(hint).block(Block::default().borders(Borders::ALL).title(" Keys "))
            };
            f.render_widget(footer, chunks[2]);
        })?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if filter_mode {
                    match key.code {
                        KeyCode::Esc => {
                            filter_mode = false;
                            filter_input.clear();
                            state.set_filter(Filter::default());
                        }
                        KeyCode::Enter => {
                            filter_mode = false;
                            state.set_filter(Filter::new(filter_input.clone()));
                        }
                        KeyCode::Backspace => {
                            filter_input.pop();
                            state.set_filter(Filter::new(filter_input.clone()));
                        }
                        KeyCode::Char(c) => {
                            filter_input.push(c);
                            state.set_filter(Filter::new(filter_input.clone()));
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => should_quit = true,
                        KeyCode::Char('/') => filter_mode = true,
                        KeyCode::Char('f') => {
                            state.toggle_selected_favorite();
                            // Rebuild with the new favorites set.
                            let favorites = state.favorites.clone();
                            state
                                .rebuild_rows(svc.catalog(), &connected, &favorites)
                                .await?;
                            // Persist favorites to ~/.jcode/model_prefs.json
                            // (per the plan: 'f' toggles favorite,
                            // persisted to model_prefs.json).
                            if let Some(path) = jcode_provider_service::model_prefs::default_path()
                            {
                                let mut prefs =
                                    jcode_provider_service::model_prefs::ModelPrefs::load(&path)
                                        .unwrap_or_default();
                                // Sync in-memory favorites to disk.
                                prefs.favorites = state
                                    .favorites
                                    .iter()
                                    .map(|(p, m)| {
                                        jcode_provider_service::model_prefs::FavoriteEntry {
                                            provider: p.clone(),
                                            model: m.clone(),
                                        }
                                    })
                                    .collect();
                                let _ = prefs.save(&path);
                            }
                        }
                        KeyCode::Down => state.move_down(1),
                        KeyCode::Up => state.move_up(1),
                        KeyCode::PageDown => state.move_down(10),
                        KeyCode::PageUp => state.move_up(10),
                        KeyCode::Enter => {
                            if let Some(row) = state.selected() {
                                selected = Some((row.provider.clone(), row.model.clone()));
                                should_quit = true;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if let Some((p, m)) = selected {
        println!("{}/{}", p, m);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use jcode_keyring_store::MockKeyringStore;
    use jcode_provider_service::boot::{BUILTIN_PROVIDERS, register_builtins};
    use jcode_provider_service::catalog::{InMemoryCatalog, ModelTier};
    use jcode_provider_service::integration::InMemoryIntegration;
    use jcode_provider_service::tui_picker::{Filter, PickerState, RowOrigin};
    use jcode_provider_service::types::ModelId;
    use std::collections::HashSet;

    #[tokio::test]
    async fn picker_state_loads_all_builtin_models() {
        let catalog = InMemoryCatalog::new();
        let integration = InMemoryIntegration::new();
        register_builtins::<MockKeyringStore>(&catalog, &integration)
            .await
            .unwrap();
        let mut state = PickerState::new();
        let mut connected = HashSet::new();
        for p in BUILTIN_PROVIDERS {
            connected.insert(p.id.into());
        }
        state
            .rebuild_rows(&catalog, &connected, &HashSet::new())
            .await
            .unwrap();
        // 7 models registered (3 anthropic + 2 openai + 1 openrouter + 1 gemini).
        assert_eq!(state.visible().len(), 7, "all 7 built-in models visible");
    }

    #[tokio::test]
    async fn picker_filter_narrows_to_one_match() {
        let catalog = InMemoryCatalog::new();
        let integration = InMemoryIntegration::new();
        register_builtins::<MockKeyringStore>(&catalog, &integration)
            .await
            .unwrap();
        let mut state = PickerState::new();
        let mut connected = HashSet::new();
        for p in BUILTIN_PROVIDERS {
            connected.insert(p.id.into());
        }
        state
            .rebuild_rows(&catalog, &connected, &HashSet::new())
            .await
            .unwrap();
        state.set_filter(Filter::new("haiku"));
        let visible = state.visible();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].model.as_str(), "claude-haiku-4-5");
    }

    #[tokio::test]
    async fn picker_favorites_appear_first() {
        let catalog = InMemoryCatalog::new();
        let integration = InMemoryIntegration::new();
        register_builtins::<MockKeyringStore>(&catalog, &integration)
            .await
            .unwrap();
        let mut state = PickerState::new();
        let mut connected = HashSet::new();
        for p in BUILTIN_PROVIDERS {
            connected.insert(p.id.into());
        }
        let mut favs = HashSet::new();
        favs.insert(("openai".into(), "gpt-5.1".into()));
        state
            .rebuild_rows(&catalog, &connected, &favs)
            .await
            .unwrap();
        assert_eq!(state.visible()[0].origin, RowOrigin::Favorite);
        assert_eq!(state.visible()[0].model.as_str(), "gpt-5.1");
    }

    #[test]
    fn model_tier_id_heuristic_recognizes_small() {
        // The picker uses the id-suggests-small heuristic via the
        // ModelTier::id_suggests_small() helper. Verify the well-known
        // small-model names still match.
        assert!(ModelTier::id_suggests_small("claude-haiku-4-5"));
        assert!(ModelTier::id_suggests_small("gpt-5-mini"));
        assert!(!ModelTier::id_suggests_small("claude-opus-4-8"));
    }
}
