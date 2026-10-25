//! A request (the AI blueprint §6): what reaches the runtime from a screen, the chat or an event. Who asks is never
//! part of it: the runtime takes the account and the organization from the session.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where a request comes from, and so where its outcome goes back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Channel {
    Chat,
    /// A screen of a module, by its place in the program.
    Screen { place: String },
    /// A fact or a due date of the calendar (K8).
    Event { fact: String },
}

impl Channel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Channel::Chat => "chat",
            Channel::Screen { .. } => "screen",
            Channel::Event { .. } => "event",
        }
    }

    /// The place or the fact: an id of the program, never a datum of the institution.
    pub fn detail(&self) -> Option<&str> {
        match self {
            Channel::Chat => None,
            Channel::Screen { place } => Some(place),
            Channel::Event { fact } => Some(fact),
        }
    }

    pub fn from_parts(kind: &str, detail: Option<String>) -> Option<Channel> {
        match kind {
            "chat" => Some(Channel::Chat),
            "screen" => Some(Channel::Screen { place: detail.unwrap_or_default() }),
            "event" => Some(Channel::Event { fact: detail.unwrap_or_default() }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Content {
    /// A screen names the capability and its input: a deterministic flow, with no router and no model (ADR-003 A4).
    Invoke { capability: String, input: Value },
    /// What the person wrote in the chat. It needs the router, which comes with IA-4.
    Text {
        #[allow(dead_code)] // the router reads it (IA-4)
        text: String,
    },
    /// The person answers what a paused task asked: the values join the input the task waits with. To fields of the
    /// dictionary, each value goes under the full id of its field (`muestra.note.phone`).
    Answer { values: Value },
    /// The person approves or rejects a proposal, named by its fingerprint.
    Decision { fingerprint: String, approve: bool },
}

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub channel: Channel,
    /// The task it continues; with none, a new task opens.
    #[serde(default)]
    pub task: Option<String>,
    pub content: Content,
}
