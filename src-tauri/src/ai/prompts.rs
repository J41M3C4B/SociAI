//! Versioned prompts (files under `ai/prompts/`) and the JSON schema of each answer.
//! Changing a prompt = a new version file + running the golden case (fixtures/caso-dorado-bano.md).

use super::AiTask;
use serde_json::{json, Value};

const COMMON: &str = include_str!("prompts/common.v1.md");
/// How to treat what is known about the institution; only for the tasks that are given its profile.
const INSTITUTION_RULES: &str = include_str!("prompts/institution_rules.v1.md");
const CONVERSATION_TURN: &str = include_str!("prompts/conversation_turn.v2.md");
const SUMMARY: &str = include_str!("prompts/diagnosis_summary.v3.md");
const DRAFTING_SECTION: &str = include_str!("prompts/drafting_section.v1.md");
const DRAFTING_PLAN: &str = include_str!("prompts/drafting_plan.v1.md");
const DRAFTING_ALL: &str = include_str!("prompts/drafting_all.v1.md");
const PROPOSE_NEEDS: &str = include_str!("prompts/prioritization_propose_needs.v4.md");
const CALL_CANONICAL: &str = include_str!("prompts/canonical_read.v1.md");
const CALL_BRIEF: &str = include_str!("prompts/call_brief.v1.md");
/// How an agent asks for a tool or answers (ADR-034 §2); the same for every agent.
const AGENT_PROTOCOL: &str = include_str!("prompts/agent_protocol.v1.md");

pub const CALL_BRIEF_VERSION: &str = "call_brief.v1";
pub const CONVERSATION_TURN_VERSION: &str = "conversation_turn.v2";
pub const SUMMARY_VERSION: &str = "diagnosis_summary.v3";
pub const DRAFTING_SECTION_VERSION: &str = "drafting_section.v1";
pub const DRAFTING_PLAN_VERSION: &str = "drafting_plan.v1";
pub const DRAFTING_ALL_VERSION: &str = "drafting_all.v1";
pub const PROPOSE_NEEDS_VERSION: &str = "prioritization_propose_needs.v4";
#[allow(dead_code)] // the version of the prompt, for whoever reads the usage log
pub const CALL_CANONICAL_VERSION: &str = "canonical_read.v1";
#[allow(dead_code)] // the version of the protocol, for whoever reads the usage log
pub const AGENT_PROTOCOL_VERSION: &str = "agent_protocol.v1";

/// Does the task receive the profile of the institution (and so need the rules about how to treat it)? An agent
/// says it itself (`agent_system`).
fn knows_the_institution(task: AiTask) -> bool {
    !matches!(task, AiTask::CallCanonical | AiTask::CallBrief | AiTask::Agent { .. } | AiTask::Operator { .. } | AiTask::Embed)
}

/// Fixed text (no dates, no ids) so the provider can cache it between calls.
pub fn system_prompt(task: AiTask) -> String {
    let body = match task {
        AiTask::ConversationTurn => CONVERSATION_TURN,
        AiTask::DiagnosisSummary => SUMMARY,
        AiTask::PrioritizationProposeNeeds => PROPOSE_NEEDS,
        AiTask::DraftingSection => DRAFTING_SECTION,
        AiTask::DraftingPlan => DRAFTING_PLAN,
        AiTask::DraftingAll => DRAFTING_ALL,
        AiTask::CallCanonical => CALL_CANONICAL,
        AiTask::CallBrief => CALL_BRIEF,
        // an agent brings its whole prompt (`agent_system`); this is only the protocol
        AiTask::Agent { .. } => AGENT_PROTOCOL,
        // text to vectors: there is no prompt; an operator of the platform brings its own
        AiTask::Embed | AiTask::Operator { .. } => "",
    };
    if knows_the_institution(task) {
        format!("{}\n\n{}\n\n{}", body.trim(), INSTITUTION_RULES.trim(), COMMON.trim())
    } else {
        format!("{}\n\n{}", body.trim(), COMMON.trim())
    }
}

/// The fixed prompt of an agent: its trade, the protocol, its tools and the rules (those about the institution only
/// when it gets its sheet). The same text on every call, so the provider can cache it.
pub fn agent_system(trade: &str, tools: &str, knows_institution: bool) -> String {
    let mut s = format!("{}\n\n{}\n\nHerramientas que puede pedir:\n{}", trade.trim(), AGENT_PROTOCOL.trim(), tools.trim());
    if knows_institution {
        s.push_str(&format!("\n\n{}", INSTITUTION_RULES.trim()));
    }
    s.push_str(&format!("\n\n{}", COMMON.trim()));
    s
}

fn string_list() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

pub fn schema(task: AiTask) -> Value {
    match task {
        AiTask::ConversationTurn => json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" },
                "cause": {
                    "anyOf": [
                        {
                            "type": "object",
                            "properties": { "text": { "type": "string" }, "quote": { "type": "string" } },
                            "required": ["text", "quote"],
                            "additionalProperties": false
                        },
                        { "type": "null" }
                    ]
                },
                "root_hypothesis": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                "options": string_list(),
                "fit": { "anyOf": [{ "type": "string", "enum": ["fits", "partial", "mismatch"] }, { "type": "null" }] },
                "fit_note": { "type": "string" }
            },
            "required": ["message", "cause", "root_hypothesis", "options", "fit", "fit_note"],
            "additionalProperties": false
        }),
        AiTask::DiagnosisSummary => json!({
            "type": "object",
            "properties": {
                "problem_statement": { "type": "string" },
                "affected": {
                    "type": "object",
                    "properties": {
                        "group": { "type": "string" },
                        "count": { "anyOf": [{ "type": "integer" }, { "type": "null" }] },
                        "description": { "type": "string" }
                    },
                    "required": ["group", "count", "description"],
                    "additionalProperties": false
                },
                "current_consequences": string_list(),
                "root_causes": string_list(),
                "reframed_need": { "type": "string" },
                "alternatives": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "title": { "type": "string" }, "pros": string_list(), "cons": string_list() },
                        "required": ["title", "pros", "cons"],
                        "additionalProperties": false
                    }
                },
                "suggested_indicators": string_list(),
                "open_questions": string_list()
            },
            "required": ["problem_statement", "affected", "current_consequences", "root_causes",
                         "reframed_need", "alternatives", "suggested_indicators", "open_questions"],
            "additionalProperties": false
        }),
        AiTask::DraftingSection => json!({
            "type": "object",
            "properties": { "content": { "type": "string" }, "open_points": string_list() },
            "required": ["content", "open_points"],
            "additionalProperties": false
        }),
        AiTask::DraftingPlan => json!({
            "type": "object",
            "properties": {
                "sections": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "key": { "type": "string" }, "title": { "type": "string" }, "plain": { "type": "string" } },
                        "required": ["key", "title", "plain"],
                        "additionalProperties": false
                    }
                },
                "budget_lines": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "description": { "type": "string" },
                            "category": { "type": "string" },
                            "quantity": { "anyOf": [{ "type": "number" }, { "type": "null" }] },
                            "unit": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                            "funded_by": { "type": "string", "enum": ["requested", "institution", "other"] },
                            "administrative": { "type": "boolean" }
                        },
                        "required": ["description", "category", "quantity", "unit", "funded_by", "administrative"],
                        "additionalProperties": false
                    }
                },
                "activities": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "title": { "type": "string" }, "start_month": { "type": "integer" }, "end_month": { "type": "integer" } },
                        "required": ["title", "start_month", "end_month"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["sections", "budget_lines", "activities"],
            "additionalProperties": false
        }),
        AiTask::DraftingAll => json!({
            "type": "object",
            "properties": {
                "sections": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "key": { "type": "string" }, "content": { "type": "string" }, "open_points": string_list() },
                        "required": ["key", "content", "open_points"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["sections"],
            "additionalProperties": false
        }),
        // the whole canonical schema as the model sees it; a block call passes its own (`pipeline::Custom`)
        AiTask::CallCanonical => crate::documents::canonical::contract::model_schema(&crate::documents::canonical::contract::canonical_schema(), None, false),
        AiTask::CallBrief => json!({
            "type": "object",
            "properties": { "brief": { "type": "string" } },
            "required": ["brief"],
            "additionalProperties": false
        }),
        // the bare shape of a step; each agent sends its own, with the arguments of its tools (`agent::step_schema`)
        // an operator of the platform brings its own schema
        AiTask::Embed | AiTask::Operator { .. } => json!({}),
        AiTask::Agent { .. } => json!({
            "type": "object",
            "properties": {
                "tool": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                "args": { "anyOf": [{ "type": "object" }, { "type": "null" }] },
                "answer": { "anyOf": [{ "type": "object" }, { "type": "null" }] }
            },
            "required": ["tool", "args", "answer"],
            "additionalProperties": false
        }),
        AiTask::PrioritizationProposeNeeds => json!({
            "type": "object",
            "properties": {
                "needs": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "title": { "type": "string" }, "description": { "type": "string" } },
                        "required": ["title", "description"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["needs"],
            "additionalProperties": false
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::ALL_TASKS;

    #[test]
    fn every_task_has_a_prompt_and_a_valid_schema() {
        for t in ALL_TASKS {
            let p = system_prompt(t);
            assert!(p.contains("Reglas que aplican siempre"), "{t:?}");
            assert!(p.contains("No inventes"), "{t:?}");
            jsonschema::validator_for(&schema(t)).expect("schema compiles");
        }
    }

    #[test]
    fn the_tasks_that_read_the_profile_are_told_how_to_treat_it_and_the_call_reader_is_not() {
        for t in ALL_TASKS {
            let p = system_prompt(t);
            if matches!(t, AiTask::CallCanonical | AiTask::CallBrief) {
                assert!(!p.contains("No capturado"), "{t:?} never sees the institution");
            } else {
                assert!(p.contains("única fuente de hechos sobre la institución"), "{t:?}");
                assert!(p.contains("«No capturado» quiere decir que no se sabe"), "{t:?}");
                assert!(p.contains("una propuesta no prueba") || p.contains("Una propuesta no prueba"), "{t:?}");
            }
        }
    }

    #[test]
    fn prompts_are_in_spanish_and_have_no_volatile_content() {
        for t in ALL_TASKS {
            let p = system_prompt(t);
            assert!(p.contains("usted"));
            // anything that changes between calls would break caching
            assert!(!p.contains("{{") && !p.contains("2026"));
        }
    }

    #[test]
    fn schemas_accept_a_sample_and_reject_extras() {
        let v = jsonschema::validator_for(&schema(AiTask::ConversationTurn)).unwrap();
        let turn = json!({
            "message": "¿Por qué?", "cause": {"text": "t", "quote": "q q"}, "root_hypothesis": null,
            "options": [], "fit": "partial", "fit_note": "n"
        });
        assert!(v.is_valid(&turn));
        assert!(v.is_valid(&json!({"message": "m", "cause": null, "root_hypothesis": "h", "options": ["a"], "fit": null, "fit_note": ""})));
        assert!(!v.is_valid(&json!({"message": "m"})), "every field is required");
        assert!(!v.is_valid(&json!({"message": "m", "cause": null, "root_hypothesis": null, "options": [], "fit": "maybe", "fit_note": ""})));
        assert!(!v.is_valid(&json!({"message": "m", "cause": null, "root_hypothesis": null, "options": [], "fit": null, "fit_note": "", "extra": 1})));
        let s = jsonschema::validator_for(&schema(AiTask::DiagnosisSummary)).unwrap();
        let ok = json!({
            "problem_statement": "p", "affected": {"group": "g", "count": null, "description": "d"},
            "current_consequences": [], "root_causes": [], "reframed_need": "r",
            "alternatives": [{"title": "t", "pros": [], "cons": []}],
            "suggested_indicators": [], "open_questions": []
        });
        assert!(s.is_valid(&ok));
    }
}
