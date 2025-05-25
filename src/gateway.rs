use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_repr::{Deserialize_repr, Serialize_repr};
use tokio::{
    net::TcpStream,
    time::{self, Instant, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, protocol::CloseFrame},
};

#[derive(Debug)]
pub enum DiscordGatewayError {
    SendFail(String),
    SocketIsNone,
    SocketClosed(Option<CloseFrame>),
    ConnectionFail,
}

#[derive(Serialize_repr, Deserialize_repr, Debug)]
#[repr(u32)]
pub enum Intents {
    Guilds = 1 << 0,
    GuildMembers = 1 << 1,
    GuildModeration = 1 << 2,
    GuildExpressions = 1 << 3,
    GuildIntegrations = 1 << 4,
    GuildWebhooks = 1 << 5,
    GuildInvites = 1 << 6,
    GuildVoiceStates = 1 << 7,
    GuildPresences = 1 << 8,
    GuildMessages = 1 << 9,
    GuildMessageReactions = 1 << 10,
    GuildMessageTyping = 1 << 11,
    DirectMessages = 1 << 12,
    DirectMessageReactions = 1 << 13,
    DirectMessageTyping = 1 << 14,
    MessageContent = 1 << 15,
    GuildScheduledEvents = 1 << 16,
    AutoModerationConfiguration = 1 << 20,
    GuildMessagePolls = 1 << 24,
    DirectMessagePolls = 1 << 25,
}

#[derive(Serialize_repr, Deserialize_repr, Debug)]
#[repr(u8)]
pub enum Opcode {
    Unknown = 0,
    Heartbeat = 1,
    Identify = 2,
    Hello = 10,
    Acknowledge = 11,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventType {
    Hello = 0,
    Ready = 1,
    MessageCreate = 2,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GatewayEvent {
    #[serde(rename = "op")]
    pub opcode: Opcode,

    // When converting to **Event structs use
    // serde_json::from_value(event.data.unwrap)
    #[serde(rename = "d")]
    #[serde(default)]
    pub data: Option<serde_json::Value>,

    #[serde(rename = "s")]
    #[serde(default)]
    pub sequence: Option<u32>,

    #[serde(rename = "t")]
    #[serde(default)]
    pub event_type: Option<EventType>,
}

pub struct DiscordGateway {
    socket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    heartbeat_interval: u64,
    last_heartbeat: time::Instant,
    // TODO: Track last acknowledgement and resume thrice if it is too old
    socket_closed: bool,
    sequence: u32,
    token: String,
    intents: u32,
}

impl DiscordGateway {
    pub async fn new(token: String, intents: u32) -> Self {
        Self {
            socket: None,
            heartbeat_interval: 0,
            last_heartbeat: time::Instant::now(),
            socket_closed: true,
            sequence: 0,
            token,
            intents, // TODO: I don't like that you have to use "as u32"
        }
    }

    pub async fn connect(&mut self) -> Result<(), DiscordGatewayError> {
        match connect_async("wss://gateway.discord.gg/?v=10&encoding=json").await {
            Ok(socket) => {
                self.socket = Some(socket.0);
                self.socket_closed = false;
            }
            Err(err) => return Err(DiscordGatewayError::ConnectionFail),
        };

        Ok(())
    }

    async fn send_msg(&mut self, json: GatewayEvent) -> Result<(), DiscordGatewayError> {
        match self.socket.as_mut() {
            Some(socket) => socket
                .send(Message::Text(serde_json::to_string(&json).unwrap().into()))
                .await
                .map_err(|e| DiscordGatewayError::SendFail(e.to_string())),
            None => Err(DiscordGatewayError::SocketIsNone),
        }
    }

    // Immediatly returns if it's not time to beat, or heartbeat interval is 0 (uninitialised)
    pub async fn heart_beat(&mut self) {
        if (self.last_heartbeat.elapsed().as_millis() < u128::from(self.heartbeat_interval))
            || self.heartbeat_interval == 0
        {
            return;
        }

        let _ = self.send_heartbeat().await;
    }

    async fn send_heartbeat(&mut self) -> Result<(), DiscordGatewayError> {
        let s = if self.sequence == 0 {
            Some(serde_json::Value::Null)
        } else {
            Some(serde_json::Value::Number(self.sequence.into()))
        };

        self.send_msg(GatewayEvent {
            opcode: Opcode::Heartbeat,
            data: s,
            sequence: None,
            event_type: None,
        })
        .await?;

        self.last_heartbeat = Instant::now();

        println!("Ba-dump");

        Ok(())
    }

    async fn send_identify(
        &mut self,
        token: &String,
        intents: u32,
    ) -> Result<(), DiscordGatewayError> {
        self.send_msg(GatewayEvent {
            opcode: Opcode::Identify,
            data: Some(json!({
                "token": token,
                "properties": {
                  "os": "linux",
                  "browser": "disco",
                  "device": "disco"
                },
                "intents": intents
            })),
            sequence: None,
            event_type: None,
        })
        .await?;

        Ok(())
    }

    pub async fn poll_event(&mut self) -> Result<Option<GatewayEvent>, DiscordGatewayError> {
        let socket = match &mut self.socket {
            Some(socket) => socket,
            None => return Err(DiscordGatewayError::SocketIsNone),
        };

        // Lord above, please smite me and these damn blocking APIs.
        // I thoroughly hate this, serenity-rs basically does the same thing.
        let message = match timeout(Duration::from_millis(500), socket.next()).await {
            Ok(Some(Ok(message))) => Some(message),
            Ok(Some(Err(err))) => panic!("{}", err),
            Ok(None) => {
                let _ = self.close(None).await;
                return Err(DiscordGatewayError::SocketClosed(None));
            }
            Err(_) => return Ok(None), // either we reached the timeout, or things are blowing up
        };

        let event: GatewayEvent = match message {
            Some(Message::Close(frame)) => {
                self.close(None).await;
                return Err(DiscordGatewayError::SocketClosed(frame));
            }
            Some(Message::Text(text)) => serde_json::from_str(text.as_str()).unwrap(),
            _ => return Ok(None),
        };

        match event.sequence {
            Some(sequence) => self.sequence = sequence,
            None => (),
        }

        // process internal events
        match event.opcode {
            Opcode::Heartbeat => self.send_heartbeat().await?,
            Opcode::Hello => {
                let token = self.token.clone();
                let intents = self.intents.clone();
                self.heartbeat_interval = event.data.as_ref().unwrap()["heartbeat_interval"]
                    .as_u64()
                    .unwrap();
                self.send_identify(&token, intents).await?;
                println!("Started heatbeat at interval {}ms", self.heartbeat_interval);
            }
            Opcode::Acknowledge => println!("Senpai noticed me- >_<"),
            _ => (),
        }

        Ok(Some(event))
    }

    pub async fn is_closed(&self) -> bool {
        self.socket_closed
    }

    pub async fn close(&mut self, message: Option<CloseFrame>) {
        match &mut self.socket {
            Some(socket) => {
                let _ = socket.close(message).await;
            }
            None => (),
        };

        self.socket_closed = true;
    }
}
