use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::time::Duration;

use super::app::App;
use super::events::{DetailHandler, EventsHandler, MainHandler, ModalHandler};

pub async fn handle_events(app: &mut App) -> Result<()> {
    if event::poll(Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.should_quit = true;
                    return Ok(());
                }
                handle_key_event(app, key.code, key.modifiers)?;
            }
        }
    }
    Ok(())
}

fn handle_key_event(app: &mut App, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    if app.is_loading {
        return Ok(());
    }

    if app.showing_confirmation {
        return ModalHandler::handle_confirmation_key(app, key);
    }

    if app.modal_state.showing_versions_modal {
        return ModalHandler::handle_versions_key(app, key);
    }

    if app.events_state.showing_events {
        return EventsHandler::handle_key(app, key);
    }

    if app.detail_state.showing_detail {
        return DetailHandler::handle_key(app, key);
    }

    if app.search_state.search_mode {
        return MainHandler::handle_search_key(app, key);
    }

    MainHandler::handle_key(app, key, modifiers)
}
