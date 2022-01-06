use std::{env, sync::{atomic::{AtomicBool, Ordering}, Arc}};

use crossterm::event::KeyCode;
use harmony_rust_sdk::{
    api::{chat::{GetGuildListRequest, EventSource}, auth::Session},
    client::{
        api::profile::{UpdateProfile, UserStatus},
        error::ClientResult,
        Client,
    },
};

use tokio::sync::RwLock;
use tokio::time::Duration;
use tui::{backend::CrosstermBackend, Terminal, widgets, layout, text::{Spans, Span}};

static RUNNING: AtomicBool = AtomicBool::new(true);

#[derive(Copy, Clone)]
enum AppMode{
    TextNormal,
    TextInsert,
    Command,
}

impl Default for AppMode {
    fn default() -> Self {
        Self::TextNormal
    }
}

#[derive(Default)]
struct AppState {
    mode: AppMode,

    // TODO: replace with something that doesn't take O(n) to insert a character
    input: String,
    command: String,
}

#[tokio::main]
async fn main() -> ClientResult<()> {
    let state = Arc::new(RwLock::new(AppState::default()));

    let tui_handler = tokio::spawn(tui(state.clone()));
    let events = tokio::spawn(ui_events(state));

    /*
    // Get auth data from .env file
    dotenv::dotenv().unwrap();
    let session_id = env::var("session_id").unwrap();
    let user_id = env::var("user_id").unwrap().parse().unwrap();
    let homeserver = env::var("homeserver").unwrap().parse().unwrap();

    // Create client
    let client = Client::new(homeserver, Some(Session::new(user_id, session_id))).await.unwrap();

    // Change our status to online
    client
        .call(
            UpdateProfile::default()
                .with_new_status(UserStatus::Online)
                .with_new_is_bot(false),
        )
        .await.unwrap();

    // Our account's user id
    //let self_id = client.auth_status().session().unwrap().user_id;

    let guilds = client.call(GetGuildListRequest::default()).await.unwrap();
    let mut events = vec![EventSource::Homeserver, EventSource::Action];
    events.extend(guilds.guilds.iter().map(|v| EventSource::Guild(v.guild_id)));

    let client = Arc::new(client);

    tokio::spawn(receive_events(client.clone(), events));
    */

    println!("nya");
    while RUNNING.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_micros(200)).await;
    }

    /*
    // Change our account's status back to offline
    client
        .call(UpdateProfile::default().with_new_status(UserStatus::OfflineUnspecified))
        .await.unwrap();
        */

    events.abort();
    tui_handler.await.unwrap().unwrap();
    std::process::exit(0);
}

async fn receive_events(client: Arc<Client>, events: Vec<EventSource>) {
    client
        .event_loop(events, {
            move |_client, event| {
                async move {
                    if !RUNNING.load(Ordering::Acquire) {
                        Ok(true)
                    } else {
                        println!("{:?}", event);
                        Ok(false)
                    }
                }
            }
        }).await.unwrap();
}

async fn tui(state: Arc<RwLock<AppState>>) -> Result<(), std::io::Error> {
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    crossterm::terminal::enable_raw_mode()?;
    terminal.clear()?;

    while RUNNING.load(Ordering::Acquire) {
        let state = state.read().await;
        terminal.draw(|f| {
            let size = f.size();

            // Create horizontal split
            let horizontal = layout::Layout::default()
                .direction(layout::Direction::Horizontal)
                .constraints([
                    layout::Constraint::Length(20),
                    layout::Constraint::Percentage(90),
                ]).split(size);

            let sidebar = layout::Layout::default()
                .direction(layout::Direction::Vertical)
                .constraints([
                    layout::Constraint::Percentage(50),
                    layout::Constraint::Percentage(50),
                ])
                .split(horizontal[0]);

            let content = layout::Layout::default()
                .direction(layout::Direction::Vertical)
                .constraints([
                    layout::Constraint::Min(3),
                    layout::Constraint::Length(3),
                    layout::Constraint::Length(1),
                ])
                .split(horizontal[1]);

            let servers = widgets::Block::default()
                .borders(widgets::Borders::ALL);
            f.render_widget(servers, sidebar[0]);

            let channels = widgets::Block::default()
                .borders(widgets::Borders::ALL);
            f.render_widget(channels, sidebar[1]);

            let messages = widgets::Block::default()
                .borders(widgets::Borders::ALL);
            f.render_widget(messages, content[0]);

            let input = widgets::Block::default()
                .borders(widgets::Borders::ALL);

            let input = widgets::Paragraph::new(state.input.as_str())
                .block(input);
            f.render_widget(input, content[1]);

            let status = {
                match state.mode {
                    AppMode::TextNormal => widgets::Paragraph::new("normal"),
                    AppMode::TextInsert => widgets::Paragraph::new("insert"),
                    AppMode::Command => widgets::Paragraph::new(Spans::from(vec![
                        Span::raw(":"),
                        Span::raw(&state.command),
                    ])),
                }
            };
            f.render_widget(status, content[2]);
        })?;

        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    terminal.clear()?;
    crossterm::terminal::disable_raw_mode()?;
    terminal.set_cursor(0, 0)?;

    Ok(())
}

async fn ui_events(state: Arc<RwLock<AppState>>) {
    while let Ok(Ok(event)) = tokio::task::spawn_blocking(crossterm::event::read).await {
        let mode = state.read().await.mode;
        match event {
            crossterm::event::Event::Key(key) => {
                match mode {
                    AppMode::TextNormal => {
                        match key.code {
                            KeyCode::Char('i') => {
                                state.write().await.mode = AppMode::TextInsert;
                            }

                            KeyCode::Char(':') => {
                                let mut state = state.write().await;
                                state.mode = AppMode::Command;
                                state.command.clear();
                            }

                            _ => (),
                        }
                    }

                    AppMode::TextInsert => {
                        match key.code {
                            KeyCode::Esc => {
                                state.write().await.mode = AppMode::TextNormal;
                            }

                            KeyCode::Char(c) => {
                                state.write().await.input.push(c);
                            }

                            _ => (),
                        }
                    }

                    AppMode::Command => {
                        match key.code {
                            KeyCode::Esc => {
                                state.write().await.mode = AppMode::TextNormal;
                            }

                            KeyCode::Enter => {
                                state.write().await.mode = AppMode::TextNormal;
                                match state.read().await.command.as_str() {
                                    "q" | "quit" => RUNNING.store(false, Ordering::Release),
                                    _ => (),
                                }
                            }

                            KeyCode::Char(c) => {
                                state.write().await.command.push(c);
                            }

                            _ => (),
                        }
                    }
                }
            }

            crossterm::event::Event::Mouse(_) => {
                // TODO: mouse events
            }

            crossterm::event::Event::Resize(_, _) => (),
        }
    }
}
