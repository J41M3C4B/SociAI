//! The tables of the platform (ADR-003 A3): its tasks, their steps and their approvals. The platform is one more owner
//! of tables for the kernel (K4): its migrations are its own, numbered from 1, and its tables start with `ai_`. The
//! input of a task lives in the encrypted database like any datum; the trail of the task, with ids and codes only,
//! is in the kernel's audit store (K9).

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use super::outcome::Outcome;
use super::request::Channel;
use super::task::{Held, Pause, Phase, Task, TaskState, Waiting};

/// The owner of the tables of the platform, for the kernel.
pub const OWNER: &str = "ai";

pub const MIGRATIONS: &[kernel::Migration] = &[kernel::Migration {
    version: 1,
    sql: "
CREATE TABLE ai_task (
    id TEXT PRIMARY KEY,
    organization TEXT NOT NULL,
    account TEXT NOT NULL,
    channel TEXT NOT NULL CHECK (channel IN ('chat', 'screen', 'event')),
    channel_detail TEXT,
    state TEXT NOT NULL CHECK (state IN ('running', 'paused', 'done', 'denied', 'limit', 'error')),
    pause TEXT CHECK (pause IN ('person', 'approval')),
    reason TEXT,
    steps INTEGER NOT NULL DEFAULT 0,
    corrections INTEGER NOT NULL DEFAULT 0,
    spent_mxn REAL NOT NULL DEFAULT 0,
    reserved_mxn REAL NOT NULL DEFAULT 0,
    opened_ms INTEGER NOT NULL,
    turn_ms INTEGER NOT NULL,
    pause_until_ms INTEGER,
    closed_ms INTEGER,
    waiting_json TEXT,
    last_json TEXT
);
CREATE TABLE ai_step (
    task_id TEXT NOT NULL REFERENCES ai_task(id),
    n INTEGER NOT NULL,
    capability TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN ('read', 'prepare', 'apply')),
    input_hash TEXT NOT NULL,
    result TEXT NOT NULL CHECK (result IN ('started', 'ok', 'invalid', 'needs_person', 'failed', 'uncertain')),
    started_ms INTEGER NOT NULL,
    ended_ms INTEGER,
    PRIMARY KEY (task_id, n)
);
CREATE TABLE ai_approval (
    task_id TEXT NOT NULL REFERENCES ai_task(id),
    fingerprint TEXT NOT NULL,
    capability TEXT NOT NULL,
    input_json TEXT NOT NULL,
    proposal_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'approved', 'rejected', 'stale')),
    asked_ms INTEGER NOT NULL,
    decided_ms INTEGER,
    decided_by TEXT,
    PRIMARY KEY (task_id, fingerprint)
);
CREATE TABLE ai_event (
    task_id TEXT NOT NULL REFERENCES ai_task(id),
    at_ms INTEGER NOT NULL,
    event TEXT NOT NULL CHECK (event IN ('open', 'step', 'denied', 'approval', 'pause', 'close')),
    capability TEXT,
    code TEXT
);
CREATE INDEX ai_event_task ON ai_event (task_id, at_ms);
",
    },
    // the trail of the tasks goes to the kernel's audit store (K9, ERP-10): one audit store, not two (IA-5)
    kernel::Migration { version: 2, sql: "DROP TABLE ai_event;" },
    // the approval of a change is tied to the kernel's proposal (IA-7): an approval keeps the fingerprint of the
    // proposal and the one of its input, and an apply keeps the fingerprint it applies, to ask the kernel later
    // whether it was applied; a step may end stale or not applied
    kernel::Migration {
        version: 3,
        sql: "
CREATE TABLE ai_step_v3 (
    task_id TEXT NOT NULL REFERENCES ai_task(id),
    n INTEGER NOT NULL,
    capability TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN ('read', 'prepare', 'apply')),
    input_hash TEXT NOT NULL,
    fingerprint TEXT,
    result TEXT NOT NULL CHECK (result IN ('started', 'ok', 'invalid', 'needs_person', 'failed', 'stale', 'uncertain', 'not_applied')),
    started_ms INTEGER NOT NULL,
    ended_ms INTEGER,
    PRIMARY KEY (task_id, n)
);
INSERT INTO ai_step_v3 (task_id, n, capability, phase, input_hash, result, started_ms, ended_ms)
    SELECT task_id, n, capability, phase, input_hash, result, started_ms, ended_ms FROM ai_step;
DROP TABLE ai_step;
ALTER TABLE ai_step_v3 RENAME TO ai_step;
ALTER TABLE ai_approval ADD COLUMN input_hash TEXT;
UPDATE ai_approval SET input_hash = fingerprint;
",
    },
    // the chat (IA-4): what the person wrote in each turn, the task a turn goes on from, what waits to be put in
    // words, and every call to a model as a step of its task, with what it cost
    kernel::Migration {
        version: 4,
        sql: "
ALTER TABLE ai_task ADD COLUMN follows TEXT;
ALTER TABLE ai_task ADD COLUMN held_json TEXT;
CREATE TABLE ai_turn (
    task_id TEXT NOT NULL REFERENCES ai_task(id),
    n INTEGER NOT NULL,
    text TEXT NOT NULL,
    at_ms INTEGER NOT NULL,
    PRIMARY KEY (task_id, n)
);
CREATE TABLE ai_model_call (
    task_id TEXT NOT NULL REFERENCES ai_task(id),
    n INTEGER NOT NULL,
    operator TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cost_mxn REAL NOT NULL DEFAULT 0,
    result TEXT NOT NULL,
    started_ms INTEGER NOT NULL,
    ended_ms INTEGER,
    PRIMARY KEY (task_id, n)
);
",
    },
];

/// Applies the migrations of the platform that the database does not have yet.
pub fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    kernel::migrate_owner(conn, OWNER, MIGRATIONS)
}

fn to_json<T: serde::Serialize>(v: &T) -> rusqlite::Result<String> {
    serde_json::to_string(v).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

fn from_json<T: serde::de::DeserializeOwned>(s: &str) -> rusqlite::Result<T> {
    serde_json::from_str(s).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
}

fn pause_str(p: Pause) -> &'static str {
    match p {
        Pause::Person => "person",
        Pause::Approval => "approval",
    }
}

pub fn insert_task(conn: &Connection, t: &Task) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO ai_task (id, organization, account, channel, channel_detail, state, opened_ms, turn_ms, follows)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![t.id, t.organization, t.account, t.channel.as_str(), t.channel.detail(), t.state.as_str(), t.opened_ms, t.turn_ms, t.follows],
    )?;
    Ok(())
}

/// Writes the task as it is now, with the last outcome it handed over.
pub fn save_task(conn: &Connection, t: &Task, last: &Outcome) -> rusqlite::Result<()> {
    save_progress(conn, t)?;
    conn.execute("UPDATE ai_task SET last_json = ?2 WHERE id = ?1", params![t.id, to_json(last)?])?;
    Ok(())
}

/// Writes the task as it is now, keeping the last outcome it handed over: before a model is called, so its steps and
/// its reserve are written while the database is let go.
pub fn save_progress(conn: &Connection, t: &Task) -> rusqlite::Result<()> {
    let waiting = t.waiting.as_ref().map(to_json).transpose()?;
    let held = t.held.as_ref().map(to_json).transpose()?;
    conn.execute(
        "UPDATE ai_task SET state = ?2, pause = ?3, reason = ?4, steps = ?5, corrections = ?6, spent_mxn = ?7,
                reserved_mxn = ?8, turn_ms = ?9, pause_until_ms = ?10, closed_ms = ?11, waiting_json = ?12, follows = ?13,
                held_json = ?14
         WHERE id = ?1",
        params![
            t.id,
            t.state.as_str(),
            t.pause.map(pause_str),
            t.reason,
            t.steps,
            t.corrections,
            t.spent_mxn,
            t.reserved_mxn,
            t.turn_ms,
            t.pause_until_ms,
            t.closed_ms,
            waiting,
            t.follows,
            held,
        ],
    )?;
    Ok(())
}

pub fn load_task(conn: &Connection, id: &str) -> rusqlite::Result<Option<Task>> {
    conn.query_row(
        "SELECT id, organization, account, channel, channel_detail, state, pause, reason, steps, corrections, spent_mxn,
                reserved_mxn, opened_ms, turn_ms, pause_until_ms, closed_ms, waiting_json, follows, held_json
         FROM ai_task WHERE id = ?1",
        [id],
        |r| {
            let bad = |i: usize| rusqlite::Error::InvalidColumnType(i, "value".into(), rusqlite::types::Type::Text);
            let channel: String = r.get(3)?;
            let state: String = r.get(5)?;
            let pause: Option<String> = r.get(6)?;
            let waiting: Option<String> = r.get(16)?;
            let held: Option<String> = r.get(18)?;
            Ok(Task {
                id: r.get(0)?,
                organization: r.get(1)?,
                account: r.get(2)?,
                channel: Channel::from_parts(&channel, r.get(4)?).ok_or_else(|| bad(3))?,
                state: TaskState::parse(&state).ok_or_else(|| bad(5))?,
                pause: match pause.as_deref() {
                    None => None,
                    Some("person") => Some(Pause::Person),
                    Some("approval") => Some(Pause::Approval),
                    Some(_) => return Err(bad(6)),
                },
                reason: r.get(7)?,
                steps: r.get(8)?,
                corrections: r.get(9)?,
                spent_mxn: r.get(10)?,
                reserved_mxn: r.get(11)?,
                opened_ms: r.get(12)?,
                turn_ms: r.get(13)?,
                pause_until_ms: r.get(14)?,
                closed_ms: r.get(15)?,
                waiting: waiting.as_deref().map(from_json::<Waiting>).transpose()?,
                follows: r.get(17)?,
                held: held.as_deref().map(from_json::<Held>).transpose()?,
            })
        },
    )
    .optional()
}

pub fn last_outcome(conn: &Connection, id: &str) -> rusqlite::Result<Option<Outcome>> {
    let last: Option<Option<String>> = conn.query_row("SELECT last_json FROM ai_task WHERE id = ?1", [id], |r| r.get(0)).optional()?;
    last.flatten().as_deref().map(from_json).transpose()
}

// ------------------------------------------------------------------ steps

/// A step about to run. An apply keeps the fingerprint of what it applies, to ask the kernel later.
#[allow(clippy::too_many_arguments)]
pub fn start_step(conn: &Connection, task: &str, n: u32, capability: &str, phase: Phase, hash: &str, fingerprint: Option<&str>, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO ai_step (task_id, n, capability, phase, input_hash, fingerprint, result, started_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'started', ?7)",
        params![task, n, capability, phase.as_str(), hash, fingerprint, now],
    )?;
    Ok(())
}

pub fn end_step(conn: &Connection, task: &str, n: u32, result: &'static str, now: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE ai_step SET result = ?3, ended_ms = ?4 WHERE task_id = ?1 AND n = ?2", params![task, n, result, now])?;
    Ok(())
}

/// Whether the task already ran this capability, in this phase, with this same input; an apply, with this same
/// fingerprint too: the same input applied on a proposal prepared again is not a repeat.
pub fn seen(conn: &Connection, task: &str, capability: &str, phase: Phase, hash: &str, fingerprint: Option<&str>) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM ai_step
                       WHERE task_id = ?1 AND capability = ?2 AND phase = ?3 AND input_hash = ?4 AND fingerprint IS ?5)",
        params![task, capability, phase.as_str(), hash, fingerprint],
        |r| r.get(0),
    )
}

/// An `apply` that started and never said how it ended.
pub struct Unfinished {
    pub n: u32,
    pub capability: String,
    /// What it was applying; none if it was not a change of the kernel's.
    pub fingerprint: Option<String>,
}

/// The `apply` that started and never said how it ended: the app closed or failed while it ran.
pub fn unfinished_apply(conn: &Connection, task: &str) -> rusqlite::Result<Option<Unfinished>> {
    conn.query_row(
        "SELECT n, capability, fingerprint FROM ai_step
         WHERE task_id = ?1 AND phase = 'apply' AND result = 'started' ORDER BY n DESC LIMIT 1",
        [task],
        |r| Ok(Unfinished { n: r.get(0)?, capability: r.get(1)?, fingerprint: r.get(2)? }),
    )
    .optional()
}

// ------------------------------------------------------------------ the chat

/// What the person wrote in one turn of the chat, kept with its task like any datum.
pub fn add_turn(conn: &Connection, task: &str, text: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO ai_turn (task_id, n, text, at_ms)
         VALUES (?1, (SELECT COALESCE(MAX(n), 0) + 1 FROM ai_turn WHERE task_id = ?1), ?2, ?3)",
        params![task, text, now],
    )?;
    Ok(())
}

/// The last thing the person wrote in a task.
pub fn last_turn(conn: &Connection, task: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT text FROM ai_turn WHERE task_id = ?1 ORDER BY n DESC LIMIT 1", [task], |r| r.get(0)).optional()
}

/// A call to a model about to be made: a step of its task.
pub fn start_model_call(conn: &Connection, task: &str, n: u32, operator: &str, provider: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO ai_model_call (task_id, n, operator, provider, result, started_ms) VALUES (?1, ?2, ?3, ?4, 'started', ?5)",
        params![task, n, operator, provider, now],
    )?;
    Ok(())
}

/// How a call to a model ended: the model that answered, the text it took and gave, and what it cost.
pub struct ModelCallEnd<'a> {
    pub model: Option<&'a str>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_mxn: f64,
    pub result: &'a str,
}

pub fn end_model_call(conn: &Connection, task: &str, n: u32, end: &ModelCallEnd, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE ai_model_call SET model = ?3, input_tokens = ?4, output_tokens = ?5, cost_mxn = ?6, result = ?7, ended_ms = ?8
         WHERE task_id = ?1 AND n = ?2",
        params![task, n, end.model, end.input_tokens as i64, end.output_tokens as i64, end.cost_mxn, end.result, now],
    )?;
    Ok(())
}

// ------------------------------------------------------------------ approvals

/// Asks for an approval, tied to `fingerprint` (the proposal's) and to `input_hash` (the input's). Any other one still
/// waiting in the task is stale now: its input is no longer the one.
#[allow(clippy::too_many_arguments)]
pub fn ask_approval(
    conn: &Connection,
    task: &str,
    fingerprint: &str,
    input_hash: &str,
    capability: &str,
    input: &Value,
    proposal: &Value,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute("UPDATE ai_approval SET state = 'stale' WHERE task_id = ?1 AND state = 'pending'", [task])?;
    conn.execute(
        "INSERT OR REPLACE INTO ai_approval (task_id, fingerprint, input_hash, capability, input_json, proposal_json, state, asked_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
        params![task, fingerprint, input_hash, capability, to_json(input)?, to_json(proposal)?, now],
    )?;
    Ok(())
}

/// The approval the task waits for.
pub struct Pending {
    /// What the person approves: for a modification, the fingerprint of the kernel's proposal.
    pub fingerprint: String,
    /// The fingerprint of the capability and its input, to know the same input asked again.
    pub input_hash: String,
    pub capability: String,
    pub input: Value,
    pub proposal: Value,
}

pub fn pending_approval(conn: &Connection, task: &str) -> rusqlite::Result<Option<Pending>> {
    let row: Option<(String, String, String, String, String)> = conn
        .query_row(
            "SELECT fingerprint, input_hash, capability, input_json, proposal_json FROM ai_approval
             WHERE task_id = ?1 AND state = 'pending'",
            [task],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    row.map(|(fingerprint, input_hash, capability, input, proposal)| {
        Ok(Pending { fingerprint, input_hash, capability, input: from_json(&input)?, proposal: from_json(&proposal)? })
    })
    .transpose()
}

pub fn decide_approval(conn: &Connection, task: &str, fingerprint: &str, approved: bool, by: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE ai_approval SET state = ?3, decided_ms = ?4, decided_by = ?5 WHERE task_id = ?1 AND fingerprint = ?2",
        params![task, fingerprint, if approved { "approved" } else { "rejected" }, now, by],
    )?;
    Ok(())
}
