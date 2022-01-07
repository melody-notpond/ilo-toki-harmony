use std::{env, sync::{atomic::{AtomicBool, Ordering}, Arc}};

use crossterm::{event::KeyCode, execute};
use harmony_rust_sdk::{
    api::{chat::{GetGuildListRequest, EventSource, SendMessageRequest, content::{Content, TextContent}, FormattedText, self}, auth::Session},
    client::{
        api::profile::{UpdateProfile, UserStatus},
        error::ClientResult,
        Client,
    },
};

use tokio::sync::{RwLock, mpsc};
use tokio::time::Duration;
use tui::{backend::CrosstermBackend, Terminal, widgets, layout, text::{Spans, Span, Text}};

static RUNNING: AtomicBool = AtomicBool::new(true);

enum ClientEvent {
    Quit,
    Send(String),
}

#[derive(Copy, Clone)]
enum AppMode{
    TextNormal,
    TextInsert,
    Command,
}

impl Default for AppMode {
    fn default() -> Self {
        Self::TextInsert
    }
}

enum MessageContent {
    Text(String),
}

struct Message {
    author_id: u64,
    author: String,
    content: MessageContent,
}

#[derive(Default)]
struct AppState {
    mode: AppMode,

    messages: Vec<Message>,

    current_channel: u64,
    current_guild: u64,

    input: String,
    input_byte_pos: usize,
    input_char_pos: usize,
    command: String,
    command_byte_pos: usize,
    command_char_pos: usize,
}

#[tokio::main]
async fn main() -> ClientResult<()> {
    // Get auth data from .env file
    dotenv::dotenv().unwrap();
    let session_id = env::var("session_id").unwrap();
    let user_id = env::var("user_id").unwrap().parse().unwrap();
    let homeserver = env::var("homeserver").unwrap().parse().unwrap();
    let channel_id = env::var("channel_id").unwrap().parse().unwrap(); // temporary
    let guild_id = env::var("guild_id").unwrap().parse().unwrap(); // temporary

    let state = Arc::new(RwLock::new(AppState::default()));
    state.write().await.current_channel = channel_id;
    state.write().await.current_guild = guild_id;

    let (tx, mut rx) = mpsc::channel(128);

    tokio::spawn(tui(state.clone()));
    tokio::spawn(ui_events(state.clone(), tx));

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

    //let guilds = client.call(GetGuildListRequest::default()).await.unwrap();
    let events = vec![EventSource::Homeserver, EventSource::Action, EventSource::Guild(guild_id)];
    //events.extend(guilds.guilds.iter().map(|v| EventSource::Guild(v.guild_id)));

    let client = Arc::new(client);

    tokio::spawn(receive_events(state.clone(), client.clone(), events));

    while let Some(event) = rx.recv().await {
        match event {
            ClientEvent::Send(msg) => {
                let state = state.read().await;
                client.call(SendMessageRequest::new(state.current_guild, state.current_channel, Some(chat::Content::new(Some(Content::new_text_message(TextContent::new(Some(FormattedText::new(msg, vec![]))))))), None, None, None, None)).await.unwrap();
            }

            ClientEvent::Quit => break,
        }
    }

    // Change our account's status back to offline
    client
        .call(UpdateProfile::default().with_new_status(UserStatus::OfflineUnspecified))
        .await.unwrap();

    std::process::exit(0);
}

async fn receive_events(state: Arc<RwLock<AppState>>, client: Arc<Client>, events: Vec<EventSource>) {
    client.event_loop(events, {
        move |_client, event| {
            let state2 = state.clone();
            async move {
                if !RUNNING.load(Ordering::Acquire) {
                    Ok(true)
                } else {
                    match event {
                        chat::Event::Chat(event) => {
                            match event {
                                chat::stream_event::Event::GuildAddedToList(_) => {}
                                chat::stream_event::Event::GuildRemovedFromList(_) => {}
                                chat::stream_event::Event::ActionPerformed(_) => {}

                                chat::stream_event::Event::SentMessage(message) => {
                                    let mut state = state2.write().await;
                                    if message.guild_id == state.current_guild && message.channel_id == state.current_channel {
                                        if let Some(message) = message.message {
                                            if let Some(content) = message.content {
                                                if let Some(content) = content.content {
                                                    match content {
                                                        Content::TextMessage(text) => {
                                                            if let Some(text) = text.content {
                                                                state.messages.push(Message {
                                                                    author_id: message.author_id,
                                                                    author: format!("id_{}", message.author_id),
                                                                    content: MessageContent::Text(text.text),
                                                                });
                                                            }
                                                        }

                                                        Content::EmbedMessage(_) => {}
                                                        Content::AttachmentMessage(_) => {}
                                                        Content::PhotoMessage(_) => {}
                                                        Content::InviteRejected(_) => {}
                                                        Content::InviteAccepted(_) => {}
                                                        Content::RoomUpgradedToGuild(_) => {}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                chat::stream_event::Event::EditedMessage(_) => {}
                                chat::stream_event::Event::DeletedMessage(_) => {}
                                chat::stream_event::Event::CreatedChannel(_) => {}
                                chat::stream_event::Event::EditedChannel(_) => {}
                                chat::stream_event::Event::DeletedChannel(_) => {}
                                chat::stream_event::Event::EditedGuild(_) => {}
                                chat::stream_event::Event::DeletedGuild(_) => {}
                                chat::stream_event::Event::JoinedMember(_) => {}
                                chat::stream_event::Event::LeftMember(_) => {}
                                chat::stream_event::Event::Typing(_) => {}
                                chat::stream_event::Event::RoleCreated(_) => {}
                                chat::stream_event::Event::RoleDeleted(_) => {}
                                chat::stream_event::Event::RoleMoved(_) => {}
                                chat::stream_event::Event::RoleUpdated(_) => {}
                                chat::stream_event::Event::RolePermsUpdated(_) => {}
                                chat::stream_event::Event::UserRolesUpdated(_) => {}
                                chat::stream_event::Event::PermissionUpdated(_) => {}
                                chat::stream_event::Event::ChannelsReordered(_) => {}
                                chat::stream_event::Event::EditedChannelPosition(_) => {}
                                chat::stream_event::Event::MessagePinned(_) => {}
                                chat::stream_event::Event::MessageUnpinned(_) => {}
                                chat::stream_event::Event::ReactionUpdated(_) => {}
                                chat::stream_event::Event::OwnerAdded(_) => {}
                                chat::stream_event::Event::OwnerRemoved(_) => {}
                                chat::stream_event::Event::InviteReceived(_) => {}
                                chat::stream_event::Event::InviteRejected(_) => {}
                            }
                        }

                        chat::Event::Profile(_) => {
                        }

                        chat::Event::Emote(_) => {
                        }
                    }
                    Ok(false)
                }
            }
        }
    }).await.unwrap();
}

async fn tui(state: Arc<RwLock<AppState>>) -> Result<(), std::io::Error> {
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut stdout = std::io::stdout();
    let mut terminal = Terminal::new(backend)?;
    crossterm::terminal::enable_raw_mode()?;
    terminal.clear()?;

    while RUNNING.load(Ordering::Acquire) {
        let state = state.read().await;
        terminal.draw(|f| {
            let size = f.size();

            // Create layout
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

            let input_text = {
                Text::from({
                    let width = horizontal[1].width as usize - 2;
                    let mut result = vec![];
                    let mut i = 0;
                    while i + width < state.input.len() {
                        result.push(Spans::from(&state.input[i..i + width]));
                        i += width;
                    }
                    result.push(Spans::from(&state.input[i..]));

                    result
                })
            };

            let content = layout::Layout::default()
                .direction(layout::Direction::Vertical)
                .constraints([
                    layout::Constraint::Min(3),
                    layout::Constraint::Length(input_text.height() as u16 + 2),
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

            let messages_list: Vec<_> = state.messages.iter().map(|v| {
                widgets::ListItem::new(Text::from({
                    let inner = messages.inner(content[0]);
                    let mut result = vec![Spans::from(Span::raw(v.author.as_str()))];
                    match &v.content {
                        MessageContent::Text(text) => {
                            let mut i = 0;
                            while i + (inner.width as usize) < text.len() {
                                result.push(Spans::from(&text[i..i + inner.width as usize]));
                                i += inner.width as usize;
                            }
                            result.push(Spans::from(&text[i..]));
                        }
                    }

                    result
                }))
            }).collect();

            let messages = widgets::List::new(messages_list)
                .block(messages);
            f.render_widget(messages, content[0]);

            let input = widgets::Block::default()
                .borders(widgets::Borders::ALL);

            let input = widgets::Paragraph::new(input_text)
                .block(input);
            f.render_widget(input, content[1]);

            let status = {
                match state.mode {
                    AppMode::TextNormal => widgets::Paragraph::new("normal"),
                    AppMode::TextInsert => widgets::Paragraph::new("insert"),

                    AppMode::Command => {
                        widgets::Paragraph::new(Spans::from(vec![
                            Span::raw(":"),
                            Span::raw(state.command.as_str()),
                        ]))
                    }
                }
            };
            f.render_widget(status, content[2]);

            match state.mode {
                AppMode::TextNormal => {
                    use crossterm::cursor::{CursorShape, SetCursorShape};
                    execute!(stdout, SetCursorShape(CursorShape::Block)).unwrap();
                    let m = state.input_char_pos as u16 % (content[1].width - 2);
                    if m == 0 && state.input_char_pos != 0 {
                        f.set_cursor(content[1].x + content[1].width - 1, content[1].y + (state.input_char_pos as u16 - 1) / (content[1].width - 2) + 1);
                    } else {
                        f.set_cursor(content[1].x + m + 1, content[1].y + state.input_char_pos as u16 / (content[1].width - 2) + 1);
                    }
                }

                AppMode::TextInsert => {
                    use crossterm::cursor::{CursorShape, SetCursorShape};
                    execute!(stdout, SetCursorShape(CursorShape::Line)).unwrap();
                    let m = state.input_char_pos as u16 % (content[1].width - 2);
                    if m == 0 && state.input_char_pos != 0 {
                        f.set_cursor(content[1].x + content[1].width - 1, content[1].y + (state.input_char_pos as u16 - 1) / (content[1].width - 2) + 1);
                    } else {
                        f.set_cursor(content[1].x + m + 1, content[1].y + state.input_char_pos as u16 / (content[1].width - 2) + 1);
                    }
                }

                AppMode::Command => {
                    use crossterm::cursor::{CursorShape, SetCursorShape};
                    execute!(stdout, SetCursorShape(CursorShape::Line)).unwrap();
                    f.set_cursor(content[2].x + state.command_char_pos as u16 + 1, content[2].y + 1);
                }
            }
        })?;

        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    terminal.clear()?;
    crossterm::terminal::disable_raw_mode()?;
    terminal.set_cursor(0, 0)?;

    Ok(())
}

async fn ui_events(state: Arc<RwLock<AppState>>, tx: mpsc::Sender<ClientEvent>) {
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

                            // TODO: up/down
                            KeyCode::Char('h') | KeyCode::Left => {
                                let mut state = state.write().await;

                                if state.input_byte_pos > 0 {
                                    let mut i = 1;
                                    while !state.input.is_char_boundary(state.input_byte_pos - i) {
                                        i += 1;
                                    }
                                    state.input_byte_pos -= i;
                                    state.input_char_pos -= 1;
                                }
                            }

                            KeyCode::Char('l') | KeyCode::Right => {
                                let mut state = state.write().await;

                                if state.input_byte_pos < state.input.bytes().len() {
                                    let mut i = 1;
                                    while !state.input.is_char_boundary(state.input_byte_pos + i) {
                                        i += 1;
                                    }
                                    state.input_byte_pos += i;
                                    state.input_char_pos += 1;
                                }
                            }

                            KeyCode::Char(':') => {
                                let mut state = state.write().await;
                                state.mode = AppMode::Command;
                                state.command.clear();
                                state.command_byte_pos = 0;
                                state.command_char_pos = 0;
                            }

                            KeyCode::Enter => {
                                let mut state = state.write().await;
                                let mut message = String::new();
                                std::mem::swap(&mut message, &mut state.input);
                                state.input_byte_pos = 0;
                                state.input_char_pos = 0;
                                state.mode = AppMode::TextInsert;

                                let _ = tx.send(ClientEvent::Send(message)).await;
                            }

                            _ => (),
                        }
                    }

                    AppMode::TextInsert => {
                        match key.code {
                            KeyCode::Esc => {
                                state.write().await.mode = AppMode::TextNormal;
                            }

                            // TODO: up/down
                            KeyCode::Left => {
                                let mut state = state.write().await;

                                if state.input_byte_pos > 0 {
                                    let mut i = 1;
                                    while !state.input.is_char_boundary(state.input_byte_pos - i) {
                                        i += 1;
                                    }
                                    state.input_byte_pos -= i;
                                    state.input_char_pos -= 1;
                                }
                            }

                            KeyCode::Right => {
                                let mut state = state.write().await;

                                if state.input_byte_pos < state.input.bytes().len() {
                                    let mut i = 1;
                                    while !state.input.is_char_boundary(state.input_byte_pos + i) {
                                        i += 1;
                                    }
                                    state.input_byte_pos += i;
                                    state.input_char_pos += 1;
                                }
                            }

                            KeyCode::Backspace => {
                                let mut state = state.write().await;

                                if state.input_byte_pos > 0 {
                                    let mut i = 1;
                                    while !state.input.is_char_boundary(state.input_byte_pos - i) {
                                        i += 1;
                                    }
                                    state.input_byte_pos -= i;
                                    state.input_char_pos -= 1;
                                    let pos = state.input_byte_pos;
                                    state.input.remove(pos);
                                }
                            }

                            KeyCode::Char(c) => {
                                let mut state = state.write().await;
                                let pos = state.input_byte_pos;
                                state.input.insert(pos, c);
                                state.input_byte_pos += c.len_utf8();
                                state.input_char_pos += 1;
                            }

                            KeyCode::Enter => {
                                let mut state = state.write().await;
                                let mut message = String::new();
                                std::mem::swap(&mut message, &mut state.input);
                                state.input_byte_pos = 0;
                                state.input_char_pos = 0;

                                let _ = tx.send(ClientEvent::Send(message)).await;
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
                                    "q" | "quit" => {
                                        RUNNING.store(false, Ordering::Release);
                                        let _ = tx.send(ClientEvent::Quit).await;
                                    }
                                    _ => (),
                                }
                            }

                            // TODO: up/down to scroll through history
                            KeyCode::Left => {
                                let mut state = state.write().await;

                                if state.command_byte_pos > 0 {
                                    let mut i = 1;
                                    while !state.command.is_char_boundary(state.command_byte_pos - i) {
                                        i += 1;
                                    }
                                    state.command_byte_pos -= i;
                                    state.command_char_pos -= 1;
                                }
                            }

                            KeyCode::Right => {
                                let mut state = state.write().await;

                                if state.command_byte_pos < state.command.bytes().len() {
                                    let mut i = 1;
                                    while !state.command.is_char_boundary(state.command_byte_pos + i) {
                                        i += 1;
                                    }
                                    state.command_byte_pos += i;
                                    state.command_char_pos += 1;
                                }
                            }

                            KeyCode::Backspace => {
                                let mut state = state.write().await;

                                if state.command_byte_pos > 0 {
                                    let mut i = 1;
                                    while !state.command.is_char_boundary(state.command_byte_pos - i) {
                                        i += 1;
                                    }
                                    state.command_byte_pos -= i;
                                    state.command_char_pos -= 1;
                                    let pos = state.command_byte_pos;
                                    state.command.remove(pos);
                                } else if state.command.is_empty() {
                                    state.mode = AppMode::TextNormal;
                                }
                            }

                            KeyCode::Char(c) => {
                                let mut state = state.write().await;
                                let pos = state.command_byte_pos;
                                state.command.insert(pos, c);
                                state.command_byte_pos += c.len_utf8();
                                state.command_char_pos += 1;
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
