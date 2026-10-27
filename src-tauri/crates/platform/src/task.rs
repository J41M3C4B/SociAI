//! The task (the AI blueprint §5, P2): the unit of the platform. It carries its id, which is the correlation id of
//! everything else, its state and its limits. The limits belong to the kind of task, never to an agent: the task
//! stops what does not advance, whatever the agent decides.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::request::Channel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Running,
    /// Waiting for the person: something to give or correct, or an approval.
    Paused,
    Done,
    Denied,
    /// Closed by one of its limits.
    Limit,
    Error,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::Running => "running",
            TaskState::Paused => "paused",
            TaskState::Done => "done",
            TaskState::Denied => "denied",
            TaskState::Limit => "limit",
            TaskState::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Option<TaskState> {
        [TaskState::Running, TaskState::Paused, TaskState::Done, TaskState::Denied, TaskState::Limit, TaskState::Error]
            .into_iter()
            .find(|t| t.as_str() == s)
    }

    pub fn is_closed(self) -> bool {
        !matches!(self, TaskState::Running | TaskState::Paused)
    }
}

/// What a paused task waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pause {
    /// The person gives or corrects something.
    Person,
    /// The person approves or rejects a proposal.
    Approval,
}

/// Which function of a capability a step runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Read,
    Prepare,
    Apply,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Read => "read",
            Phase::Prepare => "prepare",
            Phase::Apply => "apply",
        }
    }
}

/// What a paused task runs again when the person answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waiting {
    pub capability: String,
    pub phase: Phase,
    pub input: Value,
    /// The input had a value to correct: running it again counts as a correction.
    pub correction: bool,
    /// The full ids of the fields of the dictionary the person was asked for; none when nothing was.
    #[serde(default)]
    pub asked: Vec<String>,
    /// The capabilities the person was asked to choose among (`Ask::Choose`); none when it was not a choice.
    #[serde(default)]
    pub choices: Vec<String>,
}

/// Why a running task has not answered yet: the rest of its turn goes on apart, with a model (`Runtime::think`).
pub const THINKING: &str = "thinking";
/// The capability ran, and its output waits for a model to put it in words.
pub const ANSWERING: &str = "answering";

/// What a capability gave in the chat, kept while a model puts it in words: already what the person may see
/// (ADR-004 §6), in the encrypted database like any datum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Held {
    pub capability: String,
    pub step: u32,
    pub value: Value,
    pub withheld: Vec<String>,
}

/// The limits of a kind of task. The defaults live here, in one place; they are not in the code of any agent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Invocations of capabilities in the whole task.
    pub steps: u32,
    /// Times a value can be corrected and the step run again.
    pub corrections: u32,
    /// What the models may cost in the whole task, in pesos.
    pub spend_mxn: f64,
    /// How long one turn may run, from the request to its outcome, in milliseconds.
    pub turn_ms: i64,
    /// How long a pause waits for the person, in milliseconds.
    pub pause_ms: i64,
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// What a call to a model reserves of the spending of its task before it is made; what it really cost is counted
/// after. More than any one call of a light model costs, so the limit of the task is never passed.
pub const RESERVE_PER_CALL_MXN: f64 = 0.5;

impl Limits {
    pub fn of(channel: &Channel) -> Limits {
        match channel {
            // a screen names what it wants: a few steps, no model
            Channel::Screen { .. } => Limits { steps: 6, corrections: 3, spend_mxn: 0.0, turn_ms: 60_000, pause_ms: 7 * DAY_MS },
            Channel::Chat => Limits { steps: 12, corrections: 3, spend_mxn: 2.0, turn_ms: 120_000, pause_ms: 7 * DAY_MS },
            // nobody is there to correct anything
            Channel::Event { .. } => Limits { steps: 6, corrections: 0, spend_mxn: 0.0, turn_ms: 60_000, pause_ms: 7 * DAY_MS },
        }
    }
}

/// The limit a task reached: its code is the reason of its close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reached {
    Steps,
    Corrections,
    Spend,
    Time,
    /// Nobody answered the pause in time.
    Pause,
    /// The same capability with the same input again: a loop with no progress.
    Repeated,
}

impl Reached {
    pub fn code(self) -> &'static str {
        match self {
            Reached::Steps => "limit_steps",
            Reached::Corrections => "limit_corrections",
            Reached::Spend => "limit_spend",
            Reached::Time => "limit_time",
            Reached::Pause => "limit_pause",
            Reached::Repeated => "limit_repeated",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: String,
    pub organization: String,
    pub account: String,
    pub channel: Channel,
    pub state: TaskState,
    pub pause: Option<Pause>,
    /// The code of why it paused or closed.
    pub reason: Option<String>,
    pub steps: u32,
    pub corrections: u32,
    pub spent_mxn: f64,
    /// Spending reserved for calls under way, so two calls at once cannot pass the limit together.
    pub reserved_mxn: f64,
    pub opened_ms: i64,
    /// When the current turn started.
    pub turn_ms: i64,
    pub pause_until_ms: Option<i64>,
    pub closed_ms: Option<i64>,
    pub waiting: Option<Waiting>,
    /// The task of the chat this one goes on from: its last turn is the earlier one (E4). The router drops it when the
    /// new text does not go on from it.
    pub follows: Option<String>,
    pub held: Option<Held>,
}

impl Task {
    pub fn open(id: String, organization: &str, account: &str, channel: Channel, now: i64) -> Task {
        Task {
            id,
            organization: organization.into(),
            account: account.into(),
            channel,
            state: TaskState::Running,
            pause: None,
            reason: None,
            steps: 0,
            corrections: 0,
            spent_mxn: 0.0,
            reserved_mxn: 0.0,
            opened_ms: now,
            turn_ms: now,
            pause_until_ms: None,
            closed_ms: None,
            waiting: None,
            follows: None,
            held: None,
        }
    }

    pub fn limits(&self) -> Limits {
        Limits::of(&self.channel)
    }

    /// Before every invocation (the blueprints §3.3): another step fits, and the turn is in its time.
    pub fn may_step(&self, now: i64) -> Result<(), Reached> {
        let limits = self.limits();
        if self.steps >= limits.steps {
            return Err(Reached::Steps);
        }
        if now - self.turn_ms > limits.turn_ms {
            return Err(Reached::Time);
        }
        Ok(())
    }

    /// Whether the pause ran out of time.
    pub fn pause_expired(&self, now: i64) -> bool {
        self.state == TaskState::Paused && self.pause_until_ms.is_some_and(|until| now > until)
    }

    /// Reserves what a call to a model may cost, before it is made.
    pub fn reserve(&mut self, mxn: f64) -> Result<(), Reached> {
        if self.spent_mxn + self.reserved_mxn + mxn > self.limits().spend_mxn {
            return Err(Reached::Spend);
        }
        self.reserved_mxn += mxn;
        Ok(())
    }

    /// After the call: the reserve is freed and what it really cost is counted.
    pub fn settle(&mut self, reserved: f64, spent: f64) {
        self.reserved_mxn = (self.reserved_mxn - reserved).max(0.0);
        self.spent_mxn += spent;
    }
}
