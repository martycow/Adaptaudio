//! Assistant and validator tests. No network: a scripted [`Transport`] answers.

use crate::*;
use aa_core::{Clip, CurveTarget, PitchClass, Project, ScaleKind, TrackRole};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::VecDeque;

struct FakeTransport {
    answers: RefCell<VecDeque<Result<Value, AssistantError>>>,
    requests: RefCell<Vec<Value>>,
}

impl FakeTransport {
    fn new(answers: Vec<Result<Value, AssistantError>>) -> Self {
        FakeTransport {
            answers: RefCell::new(answers.into()),
            requests: RefCell::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<Value> {
        self.requests.borrow().clone()
    }
}

impl Transport for FakeTransport {
    fn send(&self, body: &Value) -> Result<Value, AssistantError> {
        self.requests.borrow_mut().push(body.clone());
        self.answers
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok(text_message("(script ended)")))
    }
}

fn message(stop_reason: &str, content: Value) -> Value {
    json!({"type": "message", "role": "assistant", "stop_reason": stop_reason, "content": content})
}

fn text_message(text: &str) -> Value {
    message(
        "end_turn",
        json!([{"type": "thinking", "thinking": "", "signature": "sig-end"}, {"type": "text", "text": text}]),
    )
}

fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!({"type": "tool_use", "id": id, "name": name, "input": input})
}

fn project() -> Project {
    let mut p = Project::new("Test");
    let bass = p.add_track("Bass", TrackRole::Bass);
    p.add_clip(bass, Clip::new("Walk", 16).with_note(0, 38, 0.8, 2.0));
    p.add_section("Stealth", 4);
    p
}

fn messages(body: &Value) -> Vec<Value> {
    body["messages"].as_array().cloned().unwrap_or_default()
}

fn count_blocks(messages: &[Value], kind: &str) -> usize {
    messages
        .iter()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|b| b["type"] == kind)
        .count()
}

// ---------------------------------------------------------------------------------------------
// Tool loop

#[test]
fn tool_loop_collects_valid_actions_and_reports_invalid_ones() {
    let first = message(
        "tool_use",
        json!([
            {"type": "thinking", "thinking": "", "signature": "sig-1"},
            {"type": "text", "text": "Вот тревожный бас."},
            tool_use("t1", "create_clip", json!({
                "track_name": "bass", "clip_name": "Anxious", "length_steps": 16,
                "notes": [{"step": 0, "pitch": 38, "velocity": 0.9, "length_steps": 2},
                          {"step": 6, "pitch": 39, "velocity": 0.6, "length_steps": 1}]
            })),
            tool_use("t2", "create_clip", json!({
                "track_name": "Bass", "clip_name": "Broken", "length_steps": 16,
                "notes": [{"step": 0, "pitch": 200, "velocity": 0.9, "length_steps": 1}]
            }))
        ]),
    );
    let fake = FakeTransport::new(vec![Ok(first), Ok(text_message("Послушай шаг 6."))]);
    let mut assistant = Assistant::new();
    let reply = assistant
        .ask(
            &fake,
            &project(),
            "Track \"Bass\" selected",
            "тревожный бас",
        )
        .unwrap();

    assert_eq!(reply.text, "Вот тревожный бас.\n\nПослушай шаг 6.");
    assert_eq!(reply.actions.len(), 1);
    match &reply.actions[0] {
        Action::CreateClip {
            track_name,
            clip_name,
            notes,
            ..
        } => {
            assert_eq!(
                track_name, "Bass",
                "name resolved to the project's spelling"
            );
            assert_eq!(clip_name, "Anxious");
            assert_eq!(notes.len(), 2);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(reply.warnings.len(), 1);
    assert!(
        reply.warnings[0].contains("pitch 200"),
        "{:?}",
        reply.warnings
    );

    let requests = fake.requests();
    assert_eq!(requests.len(), 2);
    let second = messages(&requests[1]);
    let results = second[2]["content"].as_array().unwrap();
    assert_eq!(results.len(), 2, "all results in one user message");
    assert_eq!(results[0]["tool_use_id"], "t1");
    assert!(
        results[0]["content"]
            .as_str()
            .unwrap()
            .contains("Proposed to the user")
    );
    assert_eq!(results[1]["is_error"], true);

    assert_eq!(assistant.history.len(), 1);
    assert_eq!(assistant.history[0].messages.len(), 4);
    assert_eq!(assistant.history[0].user_text, "тревожный бас");
}

#[test]
fn request_follows_the_model_rules() {
    let fake = FakeTransport::new(vec![Ok(text_message("ok"))]);
    let mut assistant = Assistant::new();
    assistant.ask(&fake, &project(), "", "привет").unwrap();
    let body = &fake.requests()[0];

    assert_eq!(body["model"], DEFAULT_MODEL);
    assert_eq!(body["max_tokens"], DEFAULT_MAX_TOKENS);
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert_eq!(
        body["thinking"]["block_binding"]["prefix_mismatch_behavior"],
        "drop_block"
    );
    assert_eq!(body["output_config"]["effort"], DEFAULT_EFFORT);
    assert_eq!(body["tool_choice"]["type"], "auto");
    assert_eq!(body["fallbacks"], "default");
    for forbidden in ["temperature", "top_p", "top_k"] {
        assert!(body.get(forbidden).is_none());
    }
    assert_eq!(body["system"][0]["text"], SYSTEM_PROMPT);
    assert_eq!(required_betas(body).len(), 2);

    let user = &body["messages"][0];
    assert_eq!(user["role"], "user");
    let context = user["content"][0]["text"].as_str().unwrap();
    assert!(context.contains("<project_state>") && context.contains("nothing selected"));
    assert_eq!(user["content"][1]["text"], "привет");
}

#[test]
fn history_is_append_only_across_questions() {
    let fake = FakeTransport::new(vec![
        Ok(message(
            "tool_use",
            json!([{"type": "thinking", "thinking": "", "signature": "s1"},
            tool_use("t1", "set_tempo", json!({"bpm": 100}))]),
        )),
        Ok(text_message("Темп 100.")),
        Ok(text_message("Второй ответ.")),
    ]);
    let mut assistant = Assistant::new();
    let p = project();
    assistant.ask(&fake, &p, "", "медленнее").unwrap();
    assistant.ask(&fake, &p, "", "ещё").unwrap();

    let requests = fake.requests();
    for pair in requests.windows(2) {
        assert_eq!(pair[0]["system"], pair[1]["system"]);
        assert_eq!(pair[0]["tools"], pair[1]["tools"]);
        let (a, b) = (messages(&pair[0]), messages(&pair[1]));
        assert_eq!(
            a[..],
            b[..a.len()],
            "earlier messages replayed byte-identical"
        );
    }
    // Thinking blocks are kept and replayed verbatim.
    assert_eq!(count_blocks(&messages(&requests[2]), "thinking"), 2);
}

#[test]
fn trimming_drops_the_oldest_half_and_its_thinking() {
    let fake = FakeTransport::new(Vec::new());
    let mut assistant = Assistant::new();
    assistant.max_history_turns = 4;
    let p = project();
    for i in 0..5 {
        assistant
            .ask(&fake, &p, "", &format!("вопрос {i}"))
            .unwrap();
    }
    assert_eq!(assistant.history.len(), 2);
    assert_eq!(assistant.history[0].user_text, "вопрос 3");
    let all: Vec<Value> = assistant
        .history
        .iter()
        .flat_map(|t| t.messages.clone())
        .collect();
    assert_eq!(count_blocks(&all, "thinking"), 0);
    assert_eq!(all[0]["role"], "user");
}

#[test]
fn endless_tool_calls_end_with_a_tool_free_round() {
    let calls: Vec<_> = (0..MAX_ROUNDS)
        .map(|i| {
            Ok(message(
                "tool_use",
                json!([tool_use(
                    &format!("t{i}"),
                    "explain_term",
                    json!({"term": "groove"})
                )]),
            ))
        })
        .collect();
    let fake = FakeTransport::new(calls);
    let mut assistant = Assistant::new();
    let reply = assistant.ask(&fake, &project(), "", "объясни").unwrap();

    let requests = fake.requests();
    assert_eq!(requests.len(), MAX_ROUNDS);
    assert_eq!(requests[MAX_ROUNDS - 1]["tool_choice"]["type"], "none");
    assert_eq!(requests[0]["tool_choice"]["type"], "auto");
    // The scripted last answer ignored tool_choice; the turn still ends on an assistant message.
    let last = assistant.history[0].messages.last().unwrap();
    assert_eq!(last["role"], "assistant");
    assert!(reply.actions.iter().all(|a| !a.is_edit()));
}

#[test]
fn refusal_is_an_error_and_leaves_history_alone() {
    let refusal = json!({"type": "message", "stop_reason": "refusal", "content": [],
        "stop_details": {"type": "refusal", "category": "cyber"}});
    let fake = FakeTransport::new(vec![Ok(refusal)]);
    let mut assistant = Assistant::new();
    let err = assistant.ask(&fake, &project(), "", "что-то").unwrap_err();
    assert_eq!(
        err,
        AssistantError::Refused {
            category: Some("cyber".into())
        }
    );
    assert!(assistant.history.is_empty());
}

#[test]
fn transport_errors_pass_through_without_touching_history() {
    let fake = FakeTransport::new(vec![Err(AssistantError::NoApiKey)]);
    let mut assistant = Assistant::new();
    assert_eq!(
        assistant.ask(&fake, &project(), "", "hi"),
        Err(AssistantError::NoApiKey)
    );
    assert!(assistant.history.is_empty());
    assert_eq!(
        assistant.ask(&fake, &project(), "", "   "),
        Err(AssistantError::EmptyMessage)
    );
}

#[test]
fn prefix_mismatch_is_recovered_by_stripping_thinking() {
    let mismatch = AssistantError::Http {
        status: 400,
        kind: "invalid_request_error".into(),
        message: "messages.1.content.0: Invalid `signature` in `thinking` block. The block is bound to a different conversation.".into(),
    };
    let fake = FakeTransport::new(vec![
        Ok(text_message("one")),
        Err(mismatch),
        Ok(text_message("two")),
    ]);
    let mut assistant = Assistant::new();
    let p = project();
    assistant.ask(&fake, &p, "", "first").unwrap();
    let reply = assistant.ask(&fake, &p, "", "second").unwrap();
    assert_eq!(reply.text, "two");
    let requests = fake.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(count_blocks(&messages(&requests[1]), "thinking"), 1);
    assert_eq!(count_blocks(&messages(&requests[2]), "thinking"), 0);
}

#[test]
fn rejected_beta_falls_back_to_plain_requests() {
    let rejected = AssistantError::Http {
        status: 400,
        kind: "invalid_request_error".into(),
        message:
            "Unexpected value(s) `server-side-fallback-2026-07-01` for the `anthropic-beta` header."
                .into(),
    };
    let fake = FakeTransport::new(vec![Err(rejected), Ok(text_message("ok"))]);
    let mut assistant = Assistant::new();
    assistant.ask(&fake, &project(), "", "hi").unwrap();
    let requests = fake.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].get("fallbacks").is_none());
    assert!(requests[1]["thinking"].get("block_binding").is_none());
    assert!(required_betas(&requests[1]).is_empty());
}

#[test]
fn fallback_marker_drops_blocks_of_the_declined_attempt() {
    let content = json!([
        {"type": "thinking", "thinking": "", "signature": "declined"},
        {"type": "fallback", "from": {"model": "claude-opus-5-5"}, "to": {"model": "claude-opus-4-8"}},
        {"type": "text", "text": "answer"}
    ]);
    let fake = FakeTransport::new(vec![Ok(message("end_turn", content))]);
    let mut assistant = Assistant::new();
    let reply = assistant.ask(&fake, &project(), "", "hi").unwrap();
    assert_eq!(reply.text, "answer");
    let echoed = assistant.history[0].messages[1]["content"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(echoed.len(), 2);
    assert_eq!(echoed[0]["type"], "fallback");
}

#[test]
fn truncated_tool_calls_are_not_proposed() {
    let cut = message(
        "max_tokens",
        json!([tool_use("t1", "set_tempo", json!({"bpm": 90}))]),
    );
    let fake = FakeTransport::new(vec![Ok(cut), Ok(text_message("done"))]);
    let mut assistant = Assistant::new();
    let reply = assistant.ask(&fake, &project(), "", "hi").unwrap();
    assert!(reply.actions.is_empty());
    assert!(reply.warnings[0].contains("max_tokens"));
}

// ---------------------------------------------------------------------------------------------
// Tool schemas and validation

#[test]
fn tool_definitions_are_well_formed_and_stable() {
    let tools = tool_definitions();
    let tools = tools.as_array().unwrap();
    assert_eq!(tools.len(), 10);
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for tool in tools {
        assert_eq!(tool["input_schema"]["type"], "object");
        assert!(!tool["description"].as_str().unwrap().is_empty());
    }
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 10);
    assert_eq!(
        tool_definitions().to_string(),
        tool_definitions().to_string()
    );

    let roles = &tools.iter().find(|t| t["name"] == "add_track").unwrap()["input_schema"]["properties"]
        ["role"]["enum"];
    let expected: Vec<&str> = TrackRole::ALL.iter().map(|r| r.name()).collect();
    assert_eq!(roles, &json!(expected));
}

fn check(project: &Project, tool: &str, input: Value) -> Result<Action, String> {
    ActionValidator::new(project).check(tool, &input)
}

#[test]
fn validator_checks_clip_notes() {
    let p = project();
    let note = |step: u64, pitch: u64| json!({"step": step, "pitch": pitch, "velocity": 0.5, "length_steps": 1});
    let clip = |notes: Value| json!({"track_name": "Bass", "clip_name": "New", "length_steps": 16, "notes": notes});

    assert!(check(&p, "create_clip", clip(json!([note(15, 127)]))).is_ok());
    assert!(
        check(&p, "create_clip", clip(json!([note(16, 40)])))
            .unwrap_err()
            .contains("outside the clip")
    );
    assert!(
        check(&p, "create_clip", clip(json!([note(0, 128)])))
            .unwrap_err()
            .contains("MIDI range")
    );
    let loud = json!({"step": 0, "pitch": 40, "velocity": 100, "length_steps": 1});
    assert!(
        check(&p, "create_clip", clip(json!([loud])))
            .unwrap_err()
            .contains("velocity")
    );
    let existing =
        json!({"track_name": "Bass", "clip_name": "walk", "length_steps": 16, "notes": []});
    assert!(
        check(&p, "create_clip", existing)
            .unwrap_err()
            .contains("replace_clip_notes")
    );
    let nowhere = json!({"track_name": "Lead", "clip_name": "X", "length_steps": 16, "notes": []});
    assert!(
        check(&p, "create_clip", nowhere)
            .unwrap_err()
            .contains("no track")
    );

    let replace =
        json!({"track_name": "Bass", "clip_name": "Walk", "notes": [note(3, 41), note(3, 41)]});
    match check(&p, "replace_clip_notes", replace).unwrap() {
        Action::ReplaceClipNotes { notes, .. } => assert_eq!(notes.len(), 1, "duplicates merged"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn validator_knows_items_proposed_earlier_in_the_reply() {
    let p = project();
    let mut v = ActionValidator::new(&p);
    assert!(
        v.check(
            "assign_clip",
            &json!({"section": "stealth", "track_name": "Lead", "clip_name": "Hook"})
        )
        .is_err()
    );
    v.check("add_track", &json!({"name": "Lead", "role": "lead"}))
        .unwrap();
    v.check(
        "create_clip",
        &json!({"track_name": "Lead", "clip_name": "Hook", "length_steps": 32, "notes": []}),
    )
    .unwrap();
    let assign = v.check(
        "assign_clip",
        &json!({"section": "stealth", "track_name": "lead", "clip_name": "hook"}),
    );
    assert_eq!(
        assign,
        Ok(Action::AssignClip {
            section: "Stealth".into(),
            track_name: "Lead".into(),
            clip_name: "Hook".into()
        })
    );
    assert!(
        v.check("add_track", &json!({"name": "Lead", "role": "Pad"}))
            .unwrap_err()
            .contains("already exists")
    );
    assert!(
        v.check("add_track", &json!({"name": "Drone", "role": "Choir"}))
            .unwrap_err()
            .contains("unknown role")
    );
}

#[test]
fn validator_checks_curves_mixer_and_globals() {
    let p = project();
    let curve = |param: &str, target: &str, points: Value| json!({"track_name": "Bass", "param": param, "target": target, "points": points});
    match check(
        &p,
        "set_curve",
        curve("intensity", "Cutoff", json!([[1.0, 1.0], [0.0, 0.2]])),
    )
    .unwrap()
    {
        Action::SetCurve {
            param,
            target,
            points,
            ..
        } => {
            assert_eq!(param, "Intensity");
            assert_eq!(target, CurveTarget::FilterCutoff);
            assert_eq!(points, vec![[0.0, 0.2], [1.0, 1.0]], "sorted by input");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(
        check(&p, "set_curve", curve("Fear", "Volume", json!([[0, 1]])))
            .unwrap_err()
            .contains("unknown game parameter")
    );
    assert!(
        check(&p, "set_curve", curve("Tension", "Pitch", json!([[0, 1]])))
            .unwrap_err()
            .contains("unknown target")
    );
    assert!(check(&p, "set_curve", curve("Tension", "Volume", json!([[0, 2]]))).is_err());
    assert!(
        check(
            &p,
            "set_curve",
            curve("Tension", "Detune", json!([[0, 40]]))
        )
        .is_ok()
    );

    assert!(check(&p, "set_mixer", json!({"track_name": "Bass"})).is_err());
    assert_eq!(
        check(&p, "set_mixer", json!({"track_name": "Bass", "pan": -0.5})),
        Ok(Action::SetMixer {
            track_name: "Bass".into(),
            volume_db: None,
            pan: Some(-0.5),
            reverb_send: None
        })
    );
    assert!(
        check(
            &p,
            "set_influence",
            json!({"track_name": "Bass", "value": 1.5})
        )
        .is_err()
    );
    assert!(check(&p, "set_tempo", json!({"bpm": 400})).is_err());
    assert_eq!(
        check(&p, "set_key_scale", json!({"key": "Eb", "scale": "dorian"})),
        Ok(Action::SetKeyScale {
            key: PitchClass(3),
            scale: ScaleKind::Dorian
        })
    );
    assert!(check(&p, "set_key_scale", json!({"key": "H", "scale": "Minor"})).is_err());
    assert!(
        check(&p, "delete_everything", json!({}))
            .unwrap_err()
            .contains("unknown tool")
    );
    assert!(check(&p, "set_tempo", json!("fast")).is_err());
}

#[test]
fn actions_round_trip_through_serde() {
    let action = Action::CreateClip {
        track_name: "Bass".into(),
        clip_name: "A".into(),
        length_steps: 16,
        notes: vec![NoteSpec {
            step: 0,
            pitch: 38,
            velocity: 0.8,
            length_steps: 2.0,
        }],
    };
    let json = serde_json::to_value(&action).unwrap();
    assert_eq!(json["type"], "CreateClip");
    assert_eq!(serde_json::from_value::<Action>(json).unwrap(), action);
    assert!(action.describe().contains("\"A\""));
}
