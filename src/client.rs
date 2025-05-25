use reqwest::Method;
use serde::Deserialize;
use serde_repr::Deserialize_repr;

use crate::gateway::{DiscordGateway, DiscordGatewayError, EventType, GatewayEvent};

#[derive(Deserialize_repr)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[repr(u32)]
enum UserFlags {
    Staff = 1 << 0,
    Partner = 1 << 1,
    HypeSquad = 1 << 2,
    BugHunterLevel1 = 1 << 3,
    #[serde(rename = "HYPESQUAD_ONLINE_HOUSE_1")]
    HypeSquadBravey = 1 << 6,
    #[serde(rename = "HYPESQUAD_ONLINE_HOUSE_2")]
    HypeSquadBrilliance = 1 << 7,
    #[serde(rename = "HYPESQUAD_ONLINE_HOUSE_3")]
    HypeSquadBalance = 1 << 8,
    PremiumEarlySupporter = 1 << 9,
    TeamPseudoUser = 1 << 10,
    BugHunterLevel2 = 1 << 14,
    VerifiedBot = 1 << 16,
    VerifiedDeveloper = 1 << 17,
    CertifiedModerator = 1 << 18,
    BotHttpInteractions = 1 << 19,
    ActiveDeveloper = 1 << 22,
}

#[derive(Deserialize_repr)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[repr(u8)]
enum PremiumType {
    None,
    NitroClassic,
    Nitro,
    NitroBasic,
}

#[derive(Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub discriminator: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub bot: Option<bool>,
    #[serde(default)]
    pub system: Option<bool>,
    #[serde(default)]
    pub mfa_enabled: Option<bool>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub accent_color: Option<u32>,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub verified: Option<bool>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub flags: Option<UserFlags>,
    #[serde(default)]
    pub premium_type: Option<PremiumType>,
    #[serde(default)]
    pub public_flags: Option<UserFlags>,
    #[serde(default)]
    pub avatar_decoration_data: Option<AvatarDecorationData>,
}

#[derive(Deserialize)]
pub struct AvatarDecorationData {
    pub asset: String,
    pub sku_id: Snowflake,
}

// TODO: Impl
#[derive(Deserialize)]
pub struct Snowflake {
    id: u64,
}

impl Snowflake {
    fn new(
        timestamp: u64,
        internal_worker_id: u64,
        internal_process_id: u64,
        increment: u64,
    ) -> Self {
        let ret = Self { id: 0 };
        ret
    }

    pub fn get_id(&self) -> u64 {
        self.id
    }

    // Converts from Discord to Unix Epoch
    pub fn get_timestamp(&self) -> u64 {
        (self.id >> 22) + 1420070400000
    }

    // convert to u8?
    pub fn get_internal_worker_id(&self) -> u64 {
        (self.id & 0x3E0000) >> 17
    }

    pub fn get_internal_process_id(&self) -> u64 {
        (self.id & 0x1F000) >> 12
    }

    pub fn get_increment(&self) -> u64 {
        self.id & 0xFFF
    }

    // TODO: Impl setters
    //
    // pub fn set_timestamp(&mut self, timestamp: u64) -> u64 {
    //     (self.id >> 22) + 1420070400000
    // }
    //
    // // convert to u8?
    // pub fn set_internal_worker_id(&mut self, internal_worker_id: u64) -> u64 {
    //     (self.id & 0x3E0000) >> 17
    // }
    //
    // pub fn set_internal_process_id(&mut self, internal_process_id: u64) -> u64 {
    //     (self.id & 0x1F000) >> 12
    // }
    //
    // pub fn set_increment(&mut self, increment: u64) -> u64 {
    //     self.id & 0xFFF
    // }
}

// TODO: From<String> for Snowflake // Some Snowflakes are returned as a String in the Discord API

impl From<u64> for Snowflake {
    fn from(value: u64) -> Self {
        Self { id: value }
    }
}

pub struct DiscordClient {
    gateway: DiscordGateway,
    reqwest: reqwest::Client,
    token: String,
}

impl DiscordClient {
    pub async fn new(token: String, intents: u32) -> Self {
        Self {
            gateway: DiscordGateway::new(token.clone(), intents.clone()).await,
            reqwest: reqwest::Client::new(),
            token,
        }
    }

    pub async fn run(&mut self, app: impl DiscordApp) -> Result<(), DiscordGatewayError> {
        let _ = self.gateway.connect().await?;

        while !self.gateway.is_closed().await {
            let _ = self.gateway.heart_beat().await;
            match self.gateway.poll_event().await {
                Ok(Some(event)) => self.process_gatewayevent(&app, event).await,
                Err(DiscordGatewayError::SocketClosed(Some(frame))) => {
                    println!("{}", frame);
                    return Err(DiscordGatewayError::SocketClosed(Some(frame)));
                }
                _ => (),
            }
        }

        Err(DiscordGatewayError::SocketClosed(None))
    }

    pub async fn process_gatewayevent(&self, app: &impl DiscordApp, event: GatewayEvent) {
        match event.event_type {
            Some(EventType::Ready) => {
                app.ready(self, serde_json::from_value(event.data.unwrap()).unwrap())
                    .await
            }
            _ => (),
        }
    }

    pub async fn get_current_user(&self) -> Result<User, reqwest::Error> {
        let response = self
            .get_authorised_builder(
                Method::GET,
                "https://discord.com/api/v10/users/@me".to_string(),
            )
            .await
            .send()
            .await;
        match response {
            Ok(response) => response.json::<User>().await,
            Err(err) => Err(err),
        }
    }

    async fn get_authorised_builder(
        &self,
        method: reqwest::Method,
        endpoint: String,
    ) -> reqwest::RequestBuilder {
        self.reqwest
            .request(method, endpoint)
            .header("Authorization", format!("Bot {}", self.token))
            .header("User-Agent", "DiscordBot ($url, $versionNumber)")
    }
}

#[derive(Deserialize)]
pub struct ReadyEvent {
    #[serde(rename = "v")]
    pub api_version: u32,
    pub user: User,
    pub guilds: serde_json::Value,
    pub session_id: String,
    pub resume_gateway_url: String,
    pub shard: Option<Vec<(u64, u64)>>,
    pub application: serde_json::Value,
}

pub trait DiscordApp {
    async fn ready(&self, client: &DiscordClient, event: ReadyEvent) {}
}
