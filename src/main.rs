use std::env;

use client::{DiscordApp, DiscordClient, ReadyEvent};
use gateway::Intents;

mod client;
mod gateway;

#[tokio::main]
async fn main() {
    let token = match env::var("DISCORD_TOKEN") {
        Ok(token) => token,
        Err(_) => panic!("Missing token, try setting one using:\n DISCORD_TOKEN=..."),
    };

    let _ = DiscordClient::new(token, Intents::GuildMessages as u32)
        .await
        .run(App)
        .await;
}

struct App;

impl DiscordApp for App {
    async fn ready(&self, client: &DiscordClient, event: ReadyEvent) {
        println!("Ich bin {}!", event.user.username);
    }
}
