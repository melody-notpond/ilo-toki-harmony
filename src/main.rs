use std::env;

use harmony_rust_sdk::{
    api::{chat::{GetGuildListRequest, EventSource}, auth::Session},
    client::{
        api::profile::{UpdateProfile, UserStatus},
        error::ClientResult,
        Client,
    },
};

#[tokio::main]
async fn main() -> ClientResult<()> {
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

    client
        .event_loop(events, {
            move |_client, event| {
                async move {
                    println!("{:?}", event);
                    Ok(false)
                }
            }
        })
        .await.unwrap();

    // Change our bots status back to offline
    client
        .call(UpdateProfile::default().with_new_status(UserStatus::OfflineUnspecified))
        .await.unwrap();

    Ok(())
}
