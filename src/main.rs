use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::UNIX_EPOCH,
};

use chrono::{DateTime, Local};
use crossterm::{event::{KeyCode, KeyModifiers}, execute};

use harmony_rust_sdk::{
    api::{
        auth::{Session, auth_step::Step, next_step_request::form_fields::Field},
        chat::{
            self,
            content::{Content, TextContent},
            get_channel_messages_request::Direction,
            EventSource, FormattedText, GetGuildListRequest,
            Message as RawMessage, SendMessageRequest, DeleteMessageRequest, UpdateMessageTextRequest, GetGuildRequest, GuildListEntry, GetGuildChannelsRequest, LeaveGuildRequest, JoinGuildRequest,
        },
        profile::{GetProfileRequest, Profile, self},
    },
    client::{
        api::{
            chat::channel::GetChannelMessages,
            profile::{UpdateProfile, UserStatus}, auth::AuthStepResponse,
        },
        error::ClientResult,
        Client,
    },
};

use tokio::sync::{mpsc, RwLock};
use tokio::time::Duration;
use tui::{
    backend::CrosstermBackend,
    layout,
    text::{Span, Spans, Text},
    widgets, Terminal, style::{Style, Color, Modifier},
};

/// Determines whether the program is currently running or not
static RUNNING: AtomicBool = AtomicBool::new(true);

/// Represents an event sent by the user from the UI to other parts of the program.
enum ClientEvent {
    /// Quits the program.
    Quit,

    /// Sends a text message to the current channel.
    Send(String),

    /// Gets more messages from the current channel.
    /// arg0 - message id
    GetMoreMessages(Option<u64>),

    /// Deletes a message in the current channel.
    Delete(u64),

    /// Edits a message in the current channel.
    Edit(u64, String),

    /// Gets the channels of the current guild.
    GetChannels,

    /// Gets a user's profile from their id.
    GetUser(u64),

    /// Leaves the given guild.
    LeaveGuild(u64),

    /// Joins a guild given an invite.
    JoinGuild(String),
}

#[derive(Copy, Clone)]
/// The current mode of the application.
enum AppMode {
    /// Normal mode for text.
    TextNormal,

    /// Insert mode for text.
    TextInsert,

    /// Command mode to enter commands.
    Command,

    /// Scroll mode to scroll through messages.
    Scroll,

    /// Delete mode to delete the selected message.
    Delete,

    /// Guild select mode to select a guild.
    GuildSelect,

    /// Channel select mode to select a channel.
    ChannelSelect,

    /// Guild leave mode to leave a guild.
    GuildLeave,
}

impl Default for AppMode {
    fn default() -> Self {
        Self::TextNormal
    }
}

/// Represents the contents of a received message.
enum MessageContent {
    /// A message composed of text.
    Text(String),
}

/// Represents a received message.
struct Message {
    /// The id of the message.
    id: u64,

    /// The user id of the author.
    author_id: u64,

    /// If an override is present, sets the username to this string.
    override_username: Option<String>,

    /// The content of the message.
    content: MessageContent,

    /// The timestamp the message was created at.
    timestamp: u64,

    /// The timestamp the message was edited at.
    edited_timestamp: Option<u64>,
}

/// Represents a member of a guild.
struct Member {
    /// The name of the member
    name: String,

    /// Whether the member is a bot or not.
    is_bot: bool,
}

/// Represents a channel.
struct Channel {
    /// The id of the channel.
    id: u64,

    /// The id of the guild that contains this channel.
    guild_id: u64,

    /// The name of the channel.
    name: String,

    /// The offset from the bottom for scrolling.
    scroll_selected: usize,

    /// The map of messages in the channel.
    messages_map: HashMap<u64, Message>,

    /// The list of messages in the channel.
    messages_list: Vec<u64>,
}

/// Represents a guild.
struct Guild {
    /// The id of the guild.
    id: u64,

    /// The list of channels.
    channels_list: Vec<u64>,

    /// The current channel selected.
    channels_select: Option<usize>,

    /// The map of channels.
    channels_map: HashMap<u64, Channel>,

    /// The name of the guild.
    name: String,

    /// The current channel being viewed.
    current_channel: Option<u64>,
}

impl Guild {
    fn current_channel(&self) -> Option<&Channel> {
        self.current_channel.and_then(|v| self.channels_map.get(&v))
    }

    fn current_channel_mut(&mut self) -> Option<&mut Channel> {
        self.current_channel.and_then(|v| self.channels_map.get_mut(&v))
    }
}

#[derive(Default)]
/// Represents the current state of the app.
struct AppState {
    /// The current mode the app is in.
    mode: AppMode,

    /// The map of users.
    users: HashMap<u64, Member>,

    /// The map of guilds.
    guilds_map: HashMap<u64, Guild>,

    /// The list of guilds
    guilds_list: Vec<u64>,

    /// The currently selected guild, if any.
    guilds_select: Option<usize>,

    /// The current guild being viewed.
    current_guild: Option<u64>,

    /// The id of the user using this application.
    current_user: u64,

    /// Determines whether or not the user is currently editing a message.
    editing: bool,

    /// The input box.
    input: String,

    /// The current byte position of the cursor in the input box.
    input_byte_pos: usize,

    /// The current character position of the cursor in the input box.
    input_char_pos: usize,

    /// The old value of the input box before editing.
    old_input: String,

    /// The old value of the byte position of the input cursor before editing.
    old_input_byte_pos: usize,

    /// The old value of the char position of the input cursor before editing.
    old_input_char_pos: usize,

    /// The command prompt.
    command: String,

    /// The current byte position of the cursor in the command prompt.
    command_byte_pos: usize,

    /// The current character position of the cursor in the command prompt.
    command_char_pos: usize,
}

impl AppState {
    fn current_guild(&self) -> Option<&Guild> {
        self.current_guild.and_then(|v| self.guilds_map.get(&v))
    }

    fn current_channel(&self) -> Option<&Channel> {
        self.current_guild().and_then(Guild::current_channel)
    }

    fn current_guild_mut(&mut self) -> Option<&mut Guild> {
        self.current_guild.and_then(|v| self.guilds_map.get_mut(&v))
    }

    fn current_channel_mut(&mut self) -> Option<&mut Channel> {
        self.current_guild_mut().and_then(Guild::current_channel_mut)
    }

    /*
    fn get_channel(&self, guild_id: u64, channel_id: u64) -> Option<&Channel> {
        self.guilds_map.get(&guild_id).and_then(|v| v.channels_map.get(&channel_id))
    }
    */

    fn get_channel_mut(&mut self, guild_id: u64, channel_id: u64) -> Option<&mut Channel> {
        self.guilds_map.get_mut(&guild_id).and_then(|v| v.channels_map.get_mut(&channel_id))
    }
}

#[tokio::main]
async fn main() -> ClientResult<()> {
    // Set up the state
    let state = Arc::new(RwLock::new(AppState::default()));

    // Create a mpsc channel
    let (tx, mut rx) = mpsc::channel(128);

    // Get auth data
    let homeserver_default = "https://chat.harmonyapp.io:2289";
    let auth_data = dirs::data_dir().and_then(|v| std::fs::read_to_string(v.join("ilo-toki/auth")).ok());

    // Create client
    let client = if let Some(auth_data) = auth_data {
        let mut split = auth_data.split('\n');
        let homeserver = split.next().unwrap_or(homeserver_default);
        let token = split.next();
        let user_id = split.next().and_then(|v| v.parse().ok());
        let session = match (token, user_id) {
            (Some(token), Some(user_id)) => Some(Session::new(user_id, String::from(token))),
            _ => None,
        };
        Client::new(homeserver.parse().unwrap_or_else(|_| homeserver_default.parse().unwrap()), session)
            .await
            .unwrap()
    } else {
        Client::new(homeserver_default.parse().unwrap(), None)
            .await
            .unwrap()
    };
    if !client.auth_status().is_authenticated() {
        auth(&client).await;
    }

    if !RUNNING.load(Ordering::Acquire) {
        clear();
        return Ok(());
    } else if let Some(auth_path) = dirs::data_dir() {
        std::fs::create_dir(auth_path.join("ilo-toki/")).ok();
        let auth_status = client.auth_status();
        let auth = auth_status.session().unwrap();
        std::fs::write(auth_path.join("ilo-toki/auth"), format!("{}\n{}\n{}\n", client.homeserver_url(), auth.session_token, auth.user_id)).unwrap();
    }

    // Spawn UI stuff
    tokio::spawn(tui(state.clone()));
    tokio::spawn(ui_events(state.clone(), tx.clone()));

    // Change our status to online
    client
        .call(
            UpdateProfile::default()
                .with_new_status(UserStatus::Online)
                .with_new_is_bot(false),
        )
        .await
        .unwrap();

    // Our account's user id
    let self_id = client.auth_status().session().unwrap().user_id;
    state.write().await.current_user = self_id;

    // Event filters
    let guilds = client.call(GetGuildListRequest::default()).await.unwrap();
    let mut events = vec![
        EventSource::Homeserver,
        EventSource::Action,
    ];
    events.extend(guilds.guilds.iter().map(|v| EventSource::Guild(v.guild_id)));

    {
        let mut state = state.write().await;
        for GuildListEntry { guild_id, .. } in guilds.guilds {
            let guild = client.call(GetGuildRequest::new(guild_id)).await.unwrap();
            if let Some(guild) = guild.guild {
                let guild = Guild {
                    id: guild_id,
                    channels_list: vec![],
                    channels_select: None,
                    channels_map: HashMap::new(),
                    name: guild.name,
                    current_channel: None,
                };
                state.guilds_list.push(guild_id);
                state.guilds_map.insert(guild_id, guild);
            }
        }
    }

    // Spawn event loop
    let client = Arc::new(client);
    tokio::spawn(receive_events(state.clone(), client.clone(), events, tx));

    // Send events
    while let Some(event) = rx.recv().await {
        match event {
            // Send messages
            ClientEvent::Send(msg) => {
                let state = state.read().await;
                if let Some(guild) = state.current_guild() {
                    if let Some(channel_id) = guild.current_channel {
                        client
                            .call(SendMessageRequest::new(
                                guild.id,
                                channel_id,
                                Some(chat::Content::new(Some(Content::new_text_message(
                                    TextContent::new(Some(FormattedText::new(msg, vec![]))),
                                )))),
                                None,
                                None,
                                None,
                                None,
                            ))
                            .await
                            .unwrap();
                    }
                }
            }

            // Quit
            ClientEvent::Quit => break,

            // Get more messages
            ClientEvent::GetMoreMessages(message_id) => {
                // Construct request
                let request = {
                    let state = state.read().await;
                    if let Some(channel) = state.current_channel() {
                        let mut request = GetChannelMessages::new(channel.guild_id, channel.id)
                            .with_direction(Some(Direction::BeforeUnspecified))
                            .with_count(51);
                        if let Some(message_id) = message_id {
                            request = request.with_message_id(message_id);
                        }
                        request
                    } else {
                        continue;
                    }
                };

                // Get the messages
                let messages = client.call(request).await.unwrap();

                // Save the messages
                let mut state = state.write().await;
                if let Some(channel) = state.current_channel() {
                    let guild_id = channel.guild_id;
                    let channel_id = channel.id;
                    for message in messages.messages.into_iter().skip(1) {
                        let message_id = message.message_id;
                        if let Some(message) = message.message {
                            if let Some(author_id) = handle_message(&mut *state, message, guild_id, channel_id, message_id, 0) {
                                let user = client.call(GetProfileRequest::new(author_id)).await.unwrap().profile;
                                if let Some(profile) = user {
                                    handle_user(&mut *state, author_id, profile);
                                }
                            }
                        }
                    }
                }
            }

            // Delete a message
            ClientEvent::Delete(message_id) => {
                let state = state.read().await;
                if let Some(guild) = state.current_guild() {
                    if let Some(channel_id) = guild.current_channel {
                        client.call(DeleteMessageRequest::new(guild.id, channel_id, message_id)).await.unwrap();
                    }
                }
            }

            // Edit a message
            ClientEvent::Edit(message_id, edit) => {
                let state = state.read().await;
                if let Some(guild) = state.current_guild() {
                    if let Some(channel_id) = guild.current_channel {
                        client.call(UpdateMessageTextRequest::new(guild.id, channel_id, message_id, Some(FormattedText::new(edit, vec![])))).await.unwrap();
                    }
                }
            }

            ClientEvent::GetChannels => {
                let mut state = state.write().await;
                if let Some(guild) = state.current_guild_mut() {
                    let channels = client.call(GetGuildChannelsRequest::new(guild.id)).await.unwrap();
                    for channel in channels.channels {
                        let channel_id = channel.channel_id;
                        if let Some(channel) = channel.channel {
                            guild.channels_list.push(channel_id);
                            guild.channels_map.insert(channel_id, Channel {
                                id: channel_id,
                                guild_id: guild.id,
                                name: channel.channel_name,
                                scroll_selected: 0,
                                messages_map: HashMap::new(),
                                messages_list: vec![],
                            });
                        }
                    }
                }
            }

            ClientEvent::GetUser(user_id) => {
                let user = client.call(GetProfileRequest::new(user_id)).await.unwrap();
                if let Some(profile) = user.profile {
                    let mut state = state.write().await;
                    handle_user(&mut *state, user_id, profile);
                }
            }

            ClientEvent::LeaveGuild(guild_id) => {
                client.call(LeaveGuildRequest::new(guild_id)).await.unwrap();
            }

            ClientEvent::JoinGuild(invite) => {
                let guild = client.call(JoinGuildRequest::new(invite)).await.unwrap();
                let guild_id = guild.guild_id;

                let guild = client.call(GetGuildRequest::new(guild_id)).await.unwrap();
                if let Some(guild) = guild.guild {
                    let guild = Guild {
                        id: guild_id,
                        channels_list: vec![],
                        channels_select: None,
                        channels_map: HashMap::new(),
                        name: guild.name,
                        current_channel: None,
                    };

                    let mut state = state.write().await;
                    state.guilds_list.push(guild_id);
                    state.guilds_map.insert(guild_id, guild);
                }
            }
        }
    }

    // Change our account's status back to offline
    client
        .call(UpdateProfile::default().with_new_status(UserStatus::OfflineUnspecified))
        .await
        .unwrap();

    // Die! :D
    clear();
    std::process::exit(0);
}

enum AuthFormFieldType {
    Text,
    Email,
    Number,
    Password,
    NewPassword,
}

enum AuthInput {
    Initial,

    Choice {
        choices: Vec<String>,
        current_choice: Option<usize>,
    },

    Form {
        fields: Vec<(String, AuthFormFieldType, String, Option<String>)>,
        selected: Option<usize>,
        selected_second: bool,
        editing: bool,
    },

    Waiting(String),
}

impl Default for AuthInput {
    fn default() -> Self {
        Self::Initial
    }
}

#[derive(Default)]
struct AuthState {
    can_go_back: bool,
    title: String,
    input: AuthInput,
}

async fn auth(client: &Client) {
    client.begin_auth().await.unwrap();
    let state = Arc::new(RwLock::new(AuthState::default()));

    let (tx, mut rx) = mpsc::channel(128);
    let tui = tokio::spawn(auth_tui(state.clone()));
    let ui_events = tokio::spawn(auth_ui_events(state.clone(), tx));

    let mut step = client.next_auth_step(AuthStepResponse::Initial).await.unwrap_or(None).and_then(|v| v.step);
    'a: while RUNNING.load(Ordering::Acquire) {
        if let Some(step) = step {
            let can_go_back = step.can_go_back;
            if let Some(step) = step.step { // why are there so many nested optionals
                let mut state = state.write().await;
                state.can_go_back = can_go_back;

                match step {
                    Step::Choice(mut choice) => {
                        for choice in choice.options.iter_mut() {
                            *choice = choice.replace('-', " ");
                        }

                        state.title = choice.title.replace('-', " ");

                        state.input = AuthInput::Choice {
                            choices: choice.options,
                            current_choice: None,
                        };
                    }

                    Step::Form(form) => {
                        state.title = form.title.replace('-', " ");
                        let fields = form.fields.iter().map(|v| (v.name.replace('-', " "), match v.r#type.as_str() {
                            "password" => AuthFormFieldType::Password,
                            "new-password" => AuthFormFieldType::NewPassword,
                            "email" => AuthFormFieldType::Email,
                            "number" => AuthFormFieldType::Number,
                            _ => AuthFormFieldType::Text,
                        }, String::new(), if v.r#type == "new-password" {
                            Some(String::new())
                        } else {
                            None
                        })).collect();

                        state.input = AuthInput::Form {
                            fields,
                            selected: None,
                            selected_second: false,
                            editing: false,
                        };
                    }

                    // I don't think this is reachable
                    Step::Session(_) => (),

                    Step::Waiting(wait) => {
                        state.input = AuthInput::Waiting(wait.description);
                    }
                }
            }
        }

        loop {
            let request = match rx.recv().await {
                Some(v) => v,
                None => break 'a,
            };
            if matches!(request, AuthStepResponse::Initial) {
                let response = client.prev_auth_step().await;
                if let Ok(back) = response {
                    step = back.step;
                    break;
                }
            } else {
                let response = client.next_auth_step(request).await;
                match response {
                    Ok(Some(forwards)) => {
                        step = forwards.step;
                        break;
                    }

                    Ok(None) => break 'a,
                    Err(_) => (),
                }
            }
        }
    }

    tui.abort();
    ui_events.abort();
}

async fn auth_tui(state: Arc<RwLock<AuthState>>) -> Result<(), std::io::Error> {
    // Set up
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    crossterm::terminal::enable_raw_mode()?;
    terminal.clear()?;

    while RUNNING.load(Ordering::Acquire) {
        let state = state.read().await;

        terminal.draw(|f| {
            let size = f.size();
            let vertical = layout::Layout::default()
                .direction(layout::Direction::Vertical)
                .constraints([
                    layout::Constraint::Min(1),
                    layout::Constraint::Length(1),
                ]).split(size);

            let block = widgets::Block::default()
                .borders(widgets::Borders::ALL)
                .title(state.title.as_str());

            match &state.input {
                AuthInput::Initial => (),

                AuthInput::Choice { choices, current_choice } => {
                    let list: Vec<_> = choices.iter().map(|v| widgets::ListItem::new(v.as_str())).collect();
                    let list = widgets::List::new(list)
                        .block(block)
                        .highlight_style(Style::default().bg(Color::Yellow));
                    let mut list_state = widgets::ListState::default();
                    list_state.select(*current_choice);
                    f.render_stateful_widget(list, vertical[0], &mut list_state);
                }

                AuthInput::Form { fields, selected, selected_second, editing }=> {
                    let layout_vec: Vec<_> = fields
                        .iter()
                        .map(|v| if let AuthFormFieldType::NewPassword = v.1 {
                            layout::Constraint::Length(7)
                        } else {
                            layout::Constraint::Length(4)
                        })
                        .collect();
                    let fields_layout = layout::Layout::default()
                        .direction(layout::Direction::Vertical)
                        .constraints(layout_vec)
                        .split(block.inner(vertical[0]));
                    f.render_widget(block, vertical[0]);

                    for (i, ((name, type_, input, input2), rect)) in fields.iter().zip(fields_layout.into_iter()).enumerate() {
                        let partial = layout::Layout::default()
                            .direction(layout::Direction::Vertical)
                            .constraints([
                                layout::Constraint::Length(1),
                                layout::Constraint::Length(3),
                                layout::Constraint::Length(3),
                            ])
                            .split(rect);

                        let label = widgets::Paragraph::new(Span::styled(name.as_str(), Style::default().add_modifier(Modifier::BOLD)));
                        f.render_widget(label, partial[0]);

                        let input_box = widgets::Block::default()
                            .borders(widgets::Borders::ALL)
                            .style(if matches!(*selected, Some(j) if j == i) && !selected_second {
                                Style::default().bg(Color::Yellow)
                            } else {
                                Style::default()
                            });
                        let input_box = if let AuthFormFieldType::Password | AuthFormFieldType::NewPassword = type_ {
                            widgets::Paragraph::new("*".repeat(input.len()))
                        } else {
                            widgets::Paragraph::new(input.as_str())
                        }.block(input_box);
                        f.render_widget(input_box, partial[1]);

                        if let Some(input) = input2 {
                            let input_box = widgets::Block::default()
                                .borders(widgets::Borders::ALL)
                                .style(if matches!(*selected, Some(j) if j == i) && *selected_second {
                                    Style::default().bg(Color::Yellow)
                                } else {
                                    Style::default()
                                });
                            let input_box = widgets::Paragraph::new("*".repeat(input.len()))
                                .block(input_box);
                            f.render_widget(input_box, partial[2]);
                        }
                    }
                }

                // TODO
                AuthInput::Waiting(_) => {}
            }

            let status = if state.can_go_back {
                widgets::Paragraph::new("press right arrow to go back, q to quit")
            } else {
                widgets::Paragraph::new("press q to quit")
            };
            f.render_widget(status, vertical[1]);
        }).unwrap();

        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    Ok(())
}

async fn auth_ui_events(state: Arc<RwLock<AuthState>>, tx: mpsc::Sender<AuthStepResponse>) {
    while let Ok(event) = tokio::task::spawn_blocking(crossterm::event::read).await.unwrap() {
        match event {
            crossterm::event::Event::Key(key) => {
                let mut state = state.write().await;
                let can_go_back = state.can_go_back;

                match &mut state.input {
                    AuthInput::Initial => {
                        match key.code {
                            KeyCode::Char('h') | KeyCode::Right if can_go_back => {
                                let _ = tx.send(AuthStepResponse::Initial).await;
                            }

                            KeyCode::Char('q') => {
                                RUNNING.store(false, Ordering::Release);
                                break;
                            }

                            _ => (),
                        }
                    }

                    AuthInput::Choice { choices, current_choice } => {
                        match key.code {
                            KeyCode::Char('h') | KeyCode::Right if can_go_back => {
                                let _ = tx.send(AuthStepResponse::Initial).await;
                            }

                            KeyCode::Char('q') => {
                                RUNNING.store(false, Ordering::Release);
                                break;
                            }

                            KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                                if let Some(choice) = current_choice.as_mut() {
                                    if *choice + 1 < choices.len() {
                                        *choice += 1;
                                    }
                                } else {
                                    *current_choice = Some(0);
                                }
                            }

                            KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                                if let Some(choice) = current_choice.as_mut() {
                                    if *choice > 0 {
                                        *choice -= 1;
                                    }
                                } else {
                                    *current_choice = Some(choices.len() - 1);
                                }
                            }

                            KeyCode::Enter => {
                                if let Some(choice) = current_choice {
                                    let _ = tx.send(AuthStepResponse::Choice(choices.get(*choice).unwrap().replace(' ', "-"))).await;
                                }
                            }

                            _ => (),
                        }
                    }

                    AuthInput::Form { fields, selected, selected_second, editing } => {
                        match key.code {
                            KeyCode::Char('h') | KeyCode::Right if can_go_back && !*editing => {
                                let _ = tx.send(AuthStepResponse::Initial).await;
                            }

                            KeyCode::Char('q') if !*editing => {
                                RUNNING.store(false, Ordering::Release);
                                break;
                            }

                            KeyCode::Esc => {
                                *editing = false;
                            }

                            KeyCode::Tab => {
                                if let Some(selection) = selected.as_mut() {
                                    if *selection + 1 < fields.len() {
                                        *selection += 1;
                                    }
                                } else {
                                    *selected = Some(0);
                                }
                            }

                            KeyCode::BackTab => {
                                if let Some(selection) = selected.as_mut() {
                                    if *selection > 0 {
                                        *selection -= 1;
                                    }
                                } else {
                                    *selected = Some(fields.len() - 1);
                                }
                            }

                            KeyCode::Char('i') if !*editing && selected.is_some() => {
                                *editing = true;
                            }

                            KeyCode::Char('j') | KeyCode::Down if !*editing => {
                                if let Some(selection) = selected.as_mut() {
                                    if *selection + 1 < fields.len() {
                                        *selection += 1;
                                    }
                                } else {
                                    *selected = Some(0);
                                }
                            }

                            KeyCode::Char('k') | KeyCode::Up if !*editing => {
                                if let Some(selection) = selected.as_mut() {
                                    if *selection > 0 {
                                        *selection -= 1;
                                    }
                                } else {
                                    *selected = Some(fields.len() - 1);
                                }
                            }

                            KeyCode::Char(c) if *editing => {
                                if let Some((_, _, input, input2)) = selected.and_then(|v| fields.get_mut(v)) {
                                    let input = if *selected_second {
                                        input2.as_mut().unwrap()
                                    } else {
                                        input
                                    };
                                    input.push(c);
                                }
                            }

                            KeyCode::Backspace if *editing => {
                                if let Some((_, _, input, input2)) = selected.and_then(|v| fields.get_mut(v)) {
                                    let input = if *selected_second {
                                        input2.as_mut().unwrap()
                                    } else {
                                        input
                                    };
                                    input.pop();
                                }
                            }

                            // TODO: arrow keys and vim controls (or maybe not; after all, this is
                            // just login stuff)

                            KeyCode::Enter => {
                                let mut result = vec![];
                                for (_, type_, input, input2) in fields.iter() {
                                    match type_ {
                                        AuthFormFieldType::Text => {
                                            result.push(Field::String(input.clone()));
                                        }

                                        AuthFormFieldType::Email => {
                                            // TODO: verification
                                            result.push(Field::String(input.clone()));
                                        }

                                        AuthFormFieldType::Number => {
                                            // TODO: what if this is an error?
                                            result.push(Field::Number(input.parse().unwrap()));
                                        }

                                        AuthFormFieldType::Password => {
                                            result.push(Field::Bytes(input.bytes().collect()));
                                        }

                                        AuthFormFieldType::NewPassword => {
                                            // TODO: what if they aren't the same?
                                            assert_eq!(input, input2.as_ref().unwrap());
                                            result.push(Field::Bytes(input.bytes().collect()));
                                        }
                                    }
                                }

                                let _ = tx.send(AuthStepResponse::Form(result)).await;
                            }

                            _ => (),
                        }
                    }

                    AuthInput::Waiting(_) => {
                        match key.code {
                            KeyCode::Char('h') | KeyCode::Right if can_go_back => {
                                let _ = tx.send(AuthStepResponse::Initial).await;
                            }

                            KeyCode::Char('q') => {
                                RUNNING.store(false, Ordering::Release);
                                break;
                            }

                            _ => (),
                        }
                    }
                }
            }

            // TODO
            crossterm::event::Event::Mouse(_) => {
            }

            crossterm::event::Event::Resize(_, _) => (),
        }
    }
}

/// Handles a message, returning the author id if the author is unknown.
fn handle_message(state: &mut AppState, message: RawMessage, guild_id: u64, channel_id: u64, message_id: u64, index: usize) -> Option<u64> {
    // Get content
    let author_id = message.author_id;

    if let Some(channel) = state.get_channel_mut(guild_id, channel_id) {
        if let Some(content) = message.content {
            if let Some(content) = content.content {
                match content {
                    // Text message
                    Content::TextMessage(text) => {
                        if let Some(text) = text.content {
                            let message = Message {
                                id: message_id,
                                author_id,
                                override_username: message.overrides.and_then(|v| v.username),
                                content: MessageContent::Text(text.text),
                                timestamp: message.created_at,
                                edited_timestamp: message.edited_at,
                            };

                            if index >= channel.messages_list.len() {
                                channel.messages_list.push(message_id);
                            } else {
                                channel.messages_list.insert(index, message_id);
                            }

                            channel.messages_map.insert(message_id, message);
                        }
                    }

                    // TODO
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

    if !state.users.contains_key(&author_id) {
        Some(author_id)
    } else {
        None
    }
}

fn handle_user(state: &mut AppState, user_id: u64, user: Profile) {
    state.users.insert(user_id, Member {
        name: user.user_name,
        is_bot: user.is_bot,
    });
}

/// Event loop to process incoming events.
async fn receive_events(
    state: Arc<RwLock<AppState>>,
    client: Arc<Client>,
    events: Vec<EventSource>,
    tx: mpsc::Sender<ClientEvent>,
) {
    client
        .event_loop(events, {
            move |_client, event| {
                // This has to be done for ownership reasons
                let state2 = state.clone();
                let tx = tx.clone();

                async move {
                    // Stop if not running
                    if !RUNNING.load(Ordering::Acquire) {
                        Ok(true)
                    } else {
                        match event {
                            // Chat events
                            chat::Event::Chat(event) => {
                                match event {
                                    chat::stream_event::Event::GuildAddedToList(_) => {}

                                    chat::stream_event::Event::GuildRemovedFromList(guild) => {
                                        let mut state = state2.write().await;
                                        state.guilds_map.remove(&guild.guild_id);
                                        let mut index = None;
                                        for (i, &id) in state.guilds_list.iter().enumerate() {
                                            if id == guild.guild_id {
                                                index = Some(i);
                                                break;
                                            }
                                        }

                                        if let Some(id) = state.current_guild {
                                            if id == guild.guild_id {
                                                state.current_guild = None;
                                            }
                                        }

                                        if let Some(i) = index {
                                            state.guilds_list.remove(i);

                                            if let Some(j) = state.guilds_select {
                                                if i == j {
                                                    state.guilds_select = None;
                                                }
                                            }
                                        }
                                    }

                                    chat::stream_event::Event::ActionPerformed(_) => {}

                                    // Received a message
                                    chat::stream_event::Event::SentMessage(message) => {
                                        // Get state
                                        let mut state = state2.write().await;

                                        // Get message
                                        let guild_id = message.guild_id;
                                        let channel_id = message.channel_id;
                                        let message_id = message.message_id;
                                        if let Some(message) = message.message {
                                            if let Some(author_id) = handle_message(&mut *state, message, guild_id, channel_id, message_id, usize::MAX) {
                                                drop(state);
                                                let _ = tx.send(ClientEvent::GetUser(author_id)).await;
                                            }
                                        }
                                    }

                                    // Edited a message
                                    chat::stream_event::Event::EditedMessage(message) => {
                                        // Get state
                                        let mut state = state2.write().await;

                                        // Edit
                                        let id = message.message_id;
                                        let edited_at = message.edited_at;

                                        // Get channel
                                        if let Some(channel) = state.get_channel_mut(message.guild_id, message.channel_id) {
                                            if let Some(content) = message.new_content {
                                                if let Some(message) = channel.messages_map.get_mut(&id) {
                                                    // TODO: more patterns
                                                    #[allow(irrefutable_let_patterns)]
                                                    if let MessageContent::Text(_) = message.content {
                                                        message.content = MessageContent::Text(content.text);
                                                        message.edited_timestamp = Some(edited_at);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Deleted a message
                                    chat::stream_event::Event::DeletedMessage(message) => {
                                        // Get state
                                        let mut state = state2.write().await;
                                        let id = message.message_id;

                                        // Get channel
                                        if let Some(channel) = state.get_channel_mut(message.guild_id, message.channel_id) {
                                            // Delete
                                            channel.messages_map.remove(&id);

                                            // Find in list and remove
                                            let mut index = None;
                                            for (i, &id2) in channel.messages_list.iter().enumerate() {
                                                if id2 == id {
                                                    index = Some(i);
                                                    break;
                                                }
                                            }
                                            if let Some(i) = index {
                                                channel.messages_list.remove(i);

                                                if channel.scroll_selected >= channel.messages_list.len() {
                                                    channel.scroll_selected = channel.messages_list.len() - 1;
                                                }
                                            }
                                        }
                                    }

                                    // TODO
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

                            chat::Event::Profile(event) => {
                                match event {
                                    profile::stream_event::Event::ProfileUpdated(profile) => {
                                        let mut state = state2.write().await;
                                        if let Some(user) = state.users.get_mut(&profile.user_id) {
                                            if let Some(username) = profile.new_username {
                                                user.name = username;
                                            }

                                            if let Some(is_bot) = profile.new_is_bot {
                                                user.is_bot = is_bot;
                                            }
                                        }
                                    }
                                }
                            }

                            // TODO
                            chat::Event::Emote(_) => {}
                        }
                        Ok(false)
                    }
                }
            }
        })
        .await
        .unwrap();
}

/// Handles rendering the terminal UI.
async fn tui(state: Arc<RwLock<AppState>>) -> Result<(), std::io::Error> {
    // Set up
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut stdout = std::io::stdout();
    let mut terminal = Terminal::new(backend)?;
    crossterm::terminal::enable_raw_mode()?;
    terminal.clear()?;

    // Draw
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
                ])
                .split(size);

            let sidebar = layout::Layout::default()
                .direction(layout::Direction::Vertical)
                .constraints([
                    layout::Constraint::Percentage(50),
                    layout::Constraint::Percentage(50),
                ])
                .split(horizontal[0]);

            // Generate input text
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

            // More layout stuff
            let content = layout::Layout::default()
                .direction(layout::Direction::Vertical)
                .constraints([
                    layout::Constraint::Min(3),
                    layout::Constraint::Length(input_text.height() as u16 + 2),
                    layout::Constraint::Length(1),
                ])
                .split(horizontal[1]);

            // Guild list
            let guilds_list: Vec<_> = state
                .guilds_list
                .iter()
                .filter_map(|v| state.guilds_map.get(v))
                .map(|v| widgets::ListItem::new(Text::from(v.name.as_str())))
                .collect();
            let guilds = widgets::Block::default().borders(widgets::Borders::ALL);
            let guilds = widgets::List::new(guilds_list)
                .block(guilds)
                .highlight_style(Style::default().bg(if matches!(state.mode, AppMode::GuildLeave) {
                    Color::Red
                } else {
                    Color::Yellow
                }));
            let mut list_state = widgets::ListState::default();
            list_state.select(state.guilds_select);
            f.render_stateful_widget(guilds, sidebar[0], &mut list_state);

            // Channel list
            let empty = vec![];
            let channels_list: Vec<_> = state
                .current_guild()
                .map(|v| &v.channels_list)
                .unwrap_or(&empty)
                .iter()
                .filter_map(|v| {
                    if let Some(guild) = state.current_guild() {
                        guild.channels_map.get(v)
                    } else {
                        None
                    }
                })
                .map(|v| widgets::ListItem::new(Text::from(v.name.as_str())))
                .collect();
            let channels = widgets::Block::default().borders(widgets::Borders::ALL);
            let channels = widgets::List::new(channels_list)
                .block(channels)
                .highlight_style(Style::default().bg(Color::Yellow));
            let mut list_state = widgets::ListState::default();
            list_state.select(state.current_guild().and_then(|v| v.channels_select));
            f.render_stateful_widget(channels, sidebar[1], &mut list_state);

            // Messages
            let messages = widgets::Block::default().borders(widgets::Borders::ALL);

            // Format current list of messages
            let header = Style::default()
                .add_modifier(Modifier::BOLD);
            let messages_list: Vec<_> = state
                .current_channel()
                .map(|v| &v.messages_list)
                .unwrap_or(&empty)
                .iter()
                .rev()
                .filter_map(|v| {
                    let inner = messages.inner(content[0]);
                    let mut result = vec![];

                    if let Some(channel) = state.current_channel() {
                        if let Some(v) = channel.messages_map.get(v) {
                            // Metadata
                            let (author, is_bot) = state
                                .users
                                .get(&v.author_id)
                                .map(|v| (v.name.as_str(), v.is_bot))
                                .unwrap_or(("<unknown user>", true));
                            let mut metadata = vec![];
                            if let Some(override_username) = &v.override_username {
                                metadata.push(Span::styled(override_username.as_str(), header));
                                metadata.push(Span::styled(" [OVR]", header));
                            } else {
                                metadata.push(Span::styled(author, header));
                            }

                            if is_bot {
                                metadata.push(Span::styled(" [BOT]", header));
                            }
                            let time: DateTime<Local> =
                                DateTime::from(UNIX_EPOCH + Duration::from_secs(v.timestamp));
                            let format = time.format(" - %H:%M (%x)").to_string();
                            metadata.push(Span::styled(format, header));

                            if v.edited_timestamp.is_some() {
                                metadata.push(Span::styled(" (edited)", header));
                            }
                            result.push(Spans::from(metadata));

                            // Content
                            match &v.content {
                                // Text wraps
                                MessageContent::Text(text) => {
                                    let mut i = 0;
                                    while i < text.len() {
                                        let mut j = i;
                                        let mut k = 0;
                                        while k < inner.width && j < text.bytes().len() {
                                            j += 1;
                                            if text.is_char_boundary(j) {
                                                k += 1;
                                            }
                                        }

                                        result.push(Spans::from(&text[i..j]));
                                        i = j;
                                    }
                                    result.push(Spans::from(&text[i..]));
                                }
                            }

                            Some(result)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .map(|v| widgets::ListItem::new(Text::from(v)))
                .collect();

            // Render messages
            let messages = widgets::List::new(messages_list)
                .block(messages)
                .start_corner(layout::Corner::BottomLeft)
                .highlight_style(Style::default().bg(if matches!(state.mode, AppMode::Delete) {
                    Color::Red
                } else if state.editing {
                    Color::Green
                } else {
                    Color::Yellow
                }));
            let mut list_state = widgets::ListState::default();
            list_state.select(if matches!(state.mode, AppMode::Scroll | AppMode::Delete) || state.editing {
                state.current_channel().map(|v| v.scroll_selected)
            } else {
                None
            });
            f.render_stateful_widget(messages, content[0], &mut list_state);

            // Input
            let input = widgets::Block::default().borders(widgets::Borders::ALL);

            let input = widgets::Paragraph::new(input_text).block(input);
            f.render_widget(input, content[1]);

            // Status bar (mode and who is typing)
            let status = {
                match state.mode {
                    AppMode::TextNormal => widgets::Paragraph::new("normal"),
                    AppMode::TextInsert => widgets::Paragraph::new("insert"),
                    AppMode::Scroll => widgets::Paragraph::new("scroll"),

                    AppMode::Command => widgets::Paragraph::new(Spans::from(vec![
                        Span::raw(":"),
                        Span::raw(state.command.as_str()),
                    ])),

                    AppMode::Delete => widgets::Paragraph::new("are you sure you want to delete this message? (y/n)"),

                    AppMode::GuildSelect => widgets::Paragraph::new("select a guild"),

                    AppMode::ChannelSelect => widgets::Paragraph::new("select a channel"),

                    AppMode::GuildLeave => widgets::Paragraph::new("are you sure you want to leave this guild? (y/n)"),
                }
            };
            f.render_widget(status, content[2]);

            // Cursor stuff is dependent on mode
            match state.mode {
                // Normal mode -> draw cursor as a block in input
                AppMode::TextNormal => {
                    use crossterm::cursor::{CursorShape, SetCursorShape};
                    execute!(stdout, SetCursorShape(CursorShape::Block)).unwrap();
                    let m = state.input_char_pos as u16 % (content[1].width - 2);
                    if m == 0 && state.input_char_pos != 0 {
                        f.set_cursor(
                            content[1].x + content[1].width - 1,
                            content[1].y
                                + (state.input_char_pos as u16 - 1) / (content[1].width - 2)
                                + 1,
                        );
                    } else {
                        f.set_cursor(
                            content[1].x + m + 1,
                            content[1].y + state.input_char_pos as u16 / (content[1].width - 2) + 1,
                        );
                    }
                }

                // Insert mode -> draw cursor as a line in input
                AppMode::TextInsert => {
                    use crossterm::cursor::{CursorShape, SetCursorShape};
                    execute!(stdout, SetCursorShape(CursorShape::Line)).unwrap();
                    let m = state.input_char_pos as u16 % (content[1].width - 2);
                    if m == 0 && state.input_char_pos != 0 {
                        f.set_cursor(
                            content[1].x + content[1].width - 1,
                            content[1].y
                                + (state.input_char_pos as u16 - 1) / (content[1].width - 2)
                                + 1,
                        );
                    } else {
                        f.set_cursor(
                            content[1].x + m + 1,
                            content[1].y + state.input_char_pos as u16 / (content[1].width - 2) + 1,
                        );
                    }
                }

                // Command mode -> draw cursor as a line in prompt
                AppMode::Command => {
                    use crossterm::cursor::{CursorShape, SetCursorShape};
                    execute!(stdout, SetCursorShape(CursorShape::Line)).unwrap();
                    f.set_cursor(
                        content[2].x + state.command_char_pos as u16 + 1,
                        content[2].y + 1,
                    );
                }

                // Everything else -> don't draw cursor
                _ => (),
            }
        })?;

        // Good night! :3
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Reset terminal
    terminal.clear()?;
    crossterm::terminal::disable_raw_mode()?;
    terminal.set_cursor(0, 0)?;

    Ok(())
}

/// Handles UI events such as key presses and mouse events.
async fn ui_events(state: Arc<RwLock<AppState>>, tx: mpsc::Sender<ClientEvent>) {
    // Event loop
    while let Ok(Ok(event)) = tokio::task::spawn_blocking(crossterm::event::read).await {
        // Get mode
        let mode = state.read().await.mode;
        match event {
            // Key events
            crossterm::event::Event::Key(key) => {
                match mode {
                    // Normal mode
                    AppMode::TextNormal => {
                        match key.code {
                            // Exit editing if editing
                            KeyCode::Esc if state.read().await.editing => {
                                let mut state = state.write().await;
                                state.mode = AppMode::Scroll;
                                state.editing = false;
                                state.input_byte_pos = state.old_input_byte_pos;
                                state.input_char_pos = state.old_input_char_pos;
                                let mut temp = String::new();
                                std::mem::swap(&mut temp, &mut state.old_input);
                                std::mem::swap(&mut temp, &mut state.input);
                            }

                            // Enter insert mode
                            KeyCode::Char('i') => {
                                state.write().await.mode = AppMode::TextInsert;
                            }

                            // Enter scroll mode
                            KeyCode::Char('s') => {
                                state.write().await.mode = AppMode::Scroll;
                            }

                            // Enter guild select mode
                            KeyCode::Char('g') => {
                                state.write().await.mode = AppMode::GuildSelect;
                            }

                            // Enter channel select mode
                            KeyCode::Char('c') => {
                                state.write().await.mode = AppMode::ChannelSelect;
                            }

                            // TODO: up/down

                            // Move left
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

                            // Move right
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

                            // Enter command prompt
                            KeyCode::Char(':') => {
                                let mut state = state.write().await;
                                state.mode = AppMode::Command;
                                state.command.clear();
                                state.command_byte_pos = 0;
                                state.command_char_pos = 0;
                            }

                            // Send message
                            KeyCode::Enter => {
                                send_message(&state, &tx).await;
                            }

                            // Don't do anything on invalid input
                            _ => (),
                        }
                    }

                    // Insert mode
                    AppMode::TextInsert => {
                        match key.code {
                            // Exit insert mode into normal mode
                            KeyCode::Esc => {
                                state.write().await.mode = AppMode::TextNormal;
                            }

                            // TODO: up/down

                            // Move left
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

                            // Move right
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

                            // Backspace
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

                            // Insert character
                            KeyCode::Char(c) => {
                                let mut state = state.write().await;
                                let pos = state.input_byte_pos;
                                state.input.insert(pos, c);
                                state.input_byte_pos += c.len_utf8();
                                state.input_char_pos += 1;
                            }

                            // Send message
                            KeyCode::Enter => {
                                send_message(&state, &tx).await;
                            }

                            // Nothing else is valid
                            _ => (),
                        }
                    }

                    // Command mode
                    AppMode::Command => {
                        match key.code {
                            // Exit command mode into normal mode
                            KeyCode::Esc => {
                                state.write().await.mode = AppMode::TextNormal;
                            }

                            // Process command
                            KeyCode::Enter => {
                                state.write().await.mode = AppMode::TextNormal;
                                let state = state.read().await;

                                // TODO: better command system
                                if state.command == "q" || state.command == "quit" {
                                    RUNNING.store(false, Ordering::Release);
                                    let _ = tx.send(ClientEvent::Quit).await;
                                } else if let Some(invite) =  state.command.strip_prefix("join ") {
                                    let _ = tx.send(ClientEvent::JoinGuild(invite.to_owned())).await;
                                }
                            }

                            // TODO: up/down to scroll through history

                            // Move left
                            KeyCode::Left => {
                                let mut state = state.write().await;

                                if state.command_byte_pos > 0 {
                                    let mut i = 1;
                                    while !state
                                        .command
                                        .is_char_boundary(state.command_byte_pos - i)
                                    {
                                        i += 1;
                                    }
                                    state.command_byte_pos -= i;
                                    state.command_char_pos -= 1;
                                }
                            }

                            // Move right
                            KeyCode::Right => {
                                let mut state = state.write().await;

                                if state.command_byte_pos < state.command.bytes().len() {
                                    let mut i = 1;
                                    while !state
                                        .command
                                        .is_char_boundary(state.command_byte_pos + i)
                                    {
                                        i += 1;
                                    }
                                    state.command_byte_pos += i;
                                    state.command_char_pos += 1;
                                }
                            }

                            // Backspace
                            KeyCode::Backspace => {
                                let mut state = state.write().await;

                                if state.command_byte_pos > 0 {
                                    let mut i = 1;
                                    while !state
                                        .command
                                        .is_char_boundary(state.command_byte_pos - i)
                                    {
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

                            // Insert character
                            KeyCode::Char(c) => {
                                let mut state = state.write().await;
                                let pos = state.command_byte_pos;
                                state.command.insert(pos, c);
                                state.command_byte_pos += c.len_utf8();
                                state.command_char_pos += 1;
                            }

                            // Invalid does nothing
                            _ => (),
                        }
                    }

                    // Scroll mode
                    AppMode::Scroll => {
                        match key.code {
                            // Escape exits to normal mode
                            KeyCode::Esc => {
                                state.write().await.mode = AppMode::TextNormal;
                            }

                            // Scroll up
                            KeyCode::Up | KeyCode::Char('k') => {
                                let mut state = state.write().await;
                                if let Some(channel) = state.current_channel_mut() {
                                    if channel.scroll_selected < channel.messages_list.len() {
                                        channel.scroll_selected += 1;

                                        if channel.scroll_selected >= channel.messages_list.len() {
                                            let _ = tx.send(ClientEvent::GetMoreMessages(channel.messages_list.first().and_then(|v| channel.messages_map.get(v)).map(|v| v.id))).await;
                                        }
                                    }
                                }
                            }

                            // Scroll down
                            KeyCode::Down | KeyCode::Char('j') => {
                                let mut state = state.write().await;
                                if let Some(channel) = state.current_channel_mut() {
                                    if channel.scroll_selected > 0 {
                                        channel.scroll_selected -= 1;
                                    }
                                }
                            }

                            // Go to top
                            KeyCode::Char('g') => {
                                let mut state = state.write().await;
                                if let Some(channel) = state.current_channel_mut() {
                                    channel.scroll_selected = channel.messages_list.len() - 1;
                                }
                            }

                            // Go to bottom
                            KeyCode::Char('G') => {
                                let mut state = state.write().await;
                                if let Some(channel) = state.current_channel_mut() {
                                    channel.scroll_selected = 0;
                                }
                            }

                            // Delete message without prompt
                            KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                                delete_message(&state, &tx).await;
                            }

                            // Delete message with prompt
                            KeyCode::Char('d') => {
                                state.write().await.mode = AppMode::Delete;
                            }

                            // Edit message
                            KeyCode::Char('e') => {
                                let mut state = state.write().await;
                                let current_user = state.current_user;

                                // Get contents
                                if let Some(channel) = state.current_channel_mut() {
                                    let mut temp = if let Some(message) = channel.messages_list.get(channel.messages_list.len() - channel.scroll_selected - 1).and_then(|v| channel.messages_map.get(v)) {
                                        if message.author_id == current_user {
                                            #[allow(irrefutable_let_patterns)]
                                            if let MessageContent::Text(text) = &message.content {
                                                text.clone()
                                            } else {
                                                continue;
                                            }
                                        } else {
                                            continue;
                                        }
                                    } else {
                                        continue;
                                    };

                                    // Switch mode
                                    state.mode = AppMode::TextInsert;
                                    state.editing = true;

                                    // Do some moving
                                    state.old_input_byte_pos = state.input_byte_pos;
                                    state.input_byte_pos = temp.bytes().len();
                                    state.old_input_char_pos = state.input_char_pos;
                                    state.input_char_pos = temp.len();
                                    std::mem::swap(&mut temp, &mut state.input);
                                    std::mem::swap(&mut temp, &mut state.old_input);
                                }
                            }

                            // TODO: more controls

                            // Nothing
                            _ => ()
                        }
                    }

                    // Deletion prompt
                    AppMode::Delete => {
                        // Delete if user chose to delete
                        if let KeyCode::Char('y') = key.code {
                            delete_message(&state, &tx).await;
                        }

                        // Go back to scroll mode
                        state.write().await.mode = AppMode::Scroll;
                    }

                    AppMode::GuildSelect => {
                        match key.code {
                            // Exit guild select mode
                            KeyCode::Esc => {
                                state.write().await.mode = AppMode::TextNormal;
                            }

                            // Move down
                            KeyCode::Char('j') | KeyCode::Down => {
                                let mut state = state.write().await;
                                let guilds_count = state.guilds_list.len();

                                if let Some(current_guild) = state.guilds_select.as_mut() {
                                    if *current_guild + 1 < guilds_count {
                                        *current_guild += 1;
                                    }
                                } else if !state.guilds_list.is_empty() {
                                    state.guilds_select = Some(0);
                                }
                            }

                            // Move up
                            KeyCode::Char('k') | KeyCode::Up => {
                                let mut state = state.write().await;
                                let guilds_count = state.guilds_list.len();

                                if let Some(current_guild) = state.guilds_select.as_mut() {
                                    if *current_guild > 0 {
                                        *current_guild -= 1;
                                    }
                                } else if !state.guilds_list.is_empty() {
                                    state.guilds_select = Some(guilds_count - 1);
                                }
                            }

                            // Select guild
                            KeyCode::Enter => {
                                let mut state = state.write().await;
                                state.current_guild = state.guilds_select.and_then(|v| state.guilds_list.get(v)).cloned();

                                if let Some(guild) = state.current_guild() {
                                    if guild.channels_list.is_empty() {
                                        let _ = tx.send(ClientEvent::GetChannels).await;
                                    }

                                    state.mode = AppMode::ChannelSelect;
                                }
                            }

                            KeyCode::Char('l') => {
                                state.write().await.mode = AppMode::GuildLeave;
                            }

                            _ => (),
                        }
                    }

                    AppMode::ChannelSelect => {
                        match key.code {
                            KeyCode::Esc => {
                                state.write().await.mode = AppMode::TextNormal;
                            }

                            // Move down
                            KeyCode::Char('j') | KeyCode::Down => {
                                let mut state = state.write().await;

                                if let Some(guild) = state.current_guild_mut() {
                                    let channel_count = guild.channels_list.len();
                                    if let Some(current_channel) = guild.channels_select.as_mut() {
                                        if *current_channel + 1 < channel_count {
                                            *current_channel += 1;
                                        }
                                    } else if !guild.channels_list.is_empty() {
                                        guild.channels_select = Some(0);
                                    }
                                }
                            }

                            // Move up
                            KeyCode::Char('k') | KeyCode::Up => {
                                let mut state = state.write().await;

                                if let Some(guild) = state.current_guild_mut() {
                                    let channel_count = guild.channels_list.len();

                                    if let Some(current_channel) = guild.channels_select.as_mut() {
                                        if *current_channel > 0 {
                                            *current_channel -= 1;
                                        }
                                    } else if !guild.channels_list.is_empty() {
                                        guild.channels_select = Some(channel_count - 1);
                                    }
                                }
                            }

                            // Select channel
                            KeyCode::Enter => {
                                let mut state = state.write().await;
                                if let Some(guild) = state.current_guild_mut() {
                                    guild.current_channel = guild.channels_select.and_then(|v| guild.channels_list.get(v)).cloned();

                                    if let Some(channel) = guild.current_channel() {
                                        if channel.messages_list.is_empty() {
                                            let _ = tx.send(ClientEvent::GetMoreMessages(None)).await;
                                        }

                                        state.mode = AppMode::TextNormal;
                                    }
                                }

                            }

                            _ => (),
                        }
                    }

                    AppMode::GuildLeave => {
                        // Leave if user chose to leave
                        if let KeyCode::Char('y') = key.code {
                            let state = state.read().await;
                            let selected_guild = state.guilds_select.and_then(|v| state.guilds_list.get(v)).cloned();

                            if let Some(guild_id) = selected_guild {
                                let _ = tx.send(ClientEvent::LeaveGuild(guild_id)).await;
                            }
                        }

                        // Go back to guild select mode
                        state.write().await.mode = AppMode::GuildSelect;
                    }
                }
            }

            // Mouse events
            crossterm::event::Event::Mouse(_) => {
                // TODO: mouse events
            }

            // Ignore this
            crossterm::event::Event::Resize(_, _) => (),
        }
    }
}

async fn send_message(state: &Arc<RwLock<AppState>>, tx: &mpsc::Sender<ClientEvent>) {
    let mut state = state.write().await;
    if state.editing {
        state.editing = false;
        let mut message = String::new();
        std::mem::swap(&mut message, &mut state.input);

        if let Some(channel) = state.current_channel() {
            if let Some(&message_id) = channel.messages_list.get(channel.messages_list.len() - channel.scroll_selected - 1) {
                if !message.is_empty() {
                    let _ = tx.send(ClientEvent::Edit(message_id, message)).await;
                }
            }
        }

        state.mode = AppMode::Scroll;
        state.editing = false;
        state.input_byte_pos = state.old_input_byte_pos;
        state.input_char_pos = state.old_input_char_pos;
        let mut temp = String::new();
        std::mem::swap(&mut temp, &mut state.old_input);
        std::mem::swap(&mut temp, &mut state.input);
    } else {
        let mut message = String::new();
        std::mem::swap(&mut message, &mut state.input);
        state.input_byte_pos = 0;
        state.input_char_pos = 0;

        if !message.is_empty() {
            let _ = tx.send(ClientEvent::Send(message)).await;
        }
    }
}

async fn delete_message(state: &Arc<RwLock<AppState>>, tx: &mpsc::Sender<ClientEvent>) {
    let state = state.read().await;
    if let Some(channel) = state.current_channel() {
        if let Some(message) = channel.messages_list.get(channel.messages_list.len() - channel.scroll_selected - 1).and_then(|v| channel.messages_map.get(v)) {
            if message.author_id == state.current_user {
                let _ = tx.send(ClientEvent::Delete(message.id)).await;
            }
        }
    }
}

fn clear() {
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.clear().unwrap();
    crossterm::terminal::disable_raw_mode().unwrap();
    terminal.set_cursor(0, 0).unwrap();
}
