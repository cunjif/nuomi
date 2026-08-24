//! AC9a–AC9e integration tests for the group-chat executor.
//! All provider behavior is scripted with FakeLlm — fully deterministic.

use async_trait::async_trait;
use nuomi_core::domain::{new_id, Role, Team, TeamTopology};
use nuomi_core::orchestrator::{
    GroupChatExecutor, GroupState, OrchestratorError, ProviderResolver, RoundRobinSelector,
    SpeakerSelector, TeamRunInput, WhiteBoardService, HANDOFF_TOOL,
};
use nuomi_core::providers::{ChatResponse, FakeLlm, LlmProvider, ToolCall};
use serde_json::{json, Value};
use std::sync::Arc;

// ---------------------------------------------------------------- fixtures --

fn role(name: &str, provider_id: &str) -> Role {
    Role {
        id: new_id(),
        name: name.into(),
        provider_id: Some(provider_id.into()),
        system_prompt_override: None,
        tool_allowlist: vec![],
        temperature: None,
        max_tokens: None,
        params: json!({}),
        created_at: 0,
        updated_at: 0,
    }
}

fn team(members: &[&Role], config: Value) -> Team {
    Team {
        id: new_id(),
        name: "design crew".into(),
        topology: TeamTopology::GroupChat,
        member_role_ids: members.iter().map(|r| r.id.clone()).collect(),
        config,
        created_at: 0,
        updated_at: 0,
    }
}

fn handoff(content: &str, target: &str) -> ChatResponse {
    ChatResponse {
        content: content.into(),
        tool_calls: vec![ToolCall {
            id: new_id(),
            name: HANDOFF_TOOL.into(),
            arguments: json!({ "target": target, "message": "over to you" }),
        }],
        ..ChatResponse::default()
    }
}

/// Seeds session + role rows so whiteboard FKs hold.
/// Returns the service plus the db path (for event assertions).
async fn seeded_board(session_id: &str, roles: &[Role]) -> (WhiteBoardService, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gc.db").to_string_lossy().to_string();
    // Test-scoped leak keeps the temp dir alive without threading a guard around.
    std::mem::forget(dir);

    let db = nuomi_core::store::Db::open(&path).unwrap();
    nuomi_core::store::migrations::run(&db.0).unwrap();
    db.0.execute(
        "INSERT INTO sessions (id, title, created_at, updated_at) VALUES (?1, '', 1, 1)",
        (session_id,),
    )
    .unwrap();
    for r in roles {
        db.0.execute(
            "INSERT INTO roles (id, name, created_at, updated_at) VALUES (?1, ?2, 1, 1)",
            (r.id.as_str(), r.name.as_str()),
        )
        .unwrap();
    }
    (WhiteBoardService::new(path.clone()), path)
}

/// Selector that lets member 0 open the discussion, then signals convergence
/// (`members.len()`) on every later consult — so a run continues only while
/// explicit handoffs keep arriving.
#[derive(Default)]
struct OpenThenConverge(std::sync::atomic::AtomicBool);

#[async_trait]
impl SpeakerSelector for OpenThenConverge {
    async fn select(&self, state: &GroupState) -> usize {
        use std::sync::atomic::Ordering;
        if self.0.swap(true, Ordering::SeqCst) {
            state.members.len()
        } else {
            0
        }
    }
}

/// Always picks the same member — exercises the consecutive-speech cap.
struct StubSelector(usize);

#[async_trait]
impl SpeakerSelector for StubSelector {
    async fn select(&self, _state: &GroupState) -> usize {
        self.0
    }
}

fn resolver(per_role: Vec<(String, Arc<FakeLlm>)>, default: Arc<FakeLlm>) -> ProviderResolver {
    let mut r = ProviderResolver::new(default as Arc<dyn LlmProvider>);
    for (id, llm) in per_role {
        r = r.with(id, llm as Arc<dyn LlmProvider>);
    }
    r
}

// ------------------------------------------------------------------ tests --

/// AC9a + AC9d: explicit handoff transfers control in order, every utterance
/// lands on the whiteboard/events, and later speakers see earlier context.
#[tokio::test]
async fn handoff_transfers_control_and_syncs_context() {
    let alice = role("Alice", "p-alice");
    let bob = role("Bob", "p-bob");
    let carol = role("Carol", "p-carol");
    let session_id = new_id();

    let llm_alice = Arc::new(FakeLlm::new(
        "p-alice",
        vec![handoff("alice opening finding", "Bob")],
    ));
    let llm_bob = Arc::new(FakeLlm::new(
        "p-bob",
        vec![handoff("bob counter finding", "Carol")],
    ));
    let llm_carol = Arc::new(FakeLlm::new(
        "p-carol",
        vec![FakeLlm::response("carol final conclusion")],
    ));

    let input = TeamRunInput {
        session_id: session_id.clone(),
        task: "design the parser".into(),
        roles: vec![alice.clone(), bob.clone(), carol.clone()],
        team: team(&[&alice, &bob, &carol], json!({})),
        model: "m".into(),
    };
    let (wb, db_path) = seeded_board(&session_id, &input.roles).await;
    let providers = resolver(
        vec![
            ("p-alice".into(), llm_alice.clone()),
            ("p-bob".into(), llm_bob.clone()),
            ("p-carol".into(), llm_carol.clone()),
        ],
        Arc::new(FakeLlm::new("d", vec![])),
    );

    let executor = GroupChatExecutor::new(Arc::new(OpenThenConverge::default()));
    let outcome = executor.run(&input, &providers, &wb).await.expect("run");

    // Control followed the explicit handoffs A→B→C, then converged.
    assert!(outcome.converged);
    assert_eq!(outcome.rounds, 3);
    let speakers: Vec<usize> = outcome.transcript.iter().map(|t| t.speaker).collect();
    assert_eq!(speakers, vec![0, 1, 2]);
    assert_eq!(outcome.transcript[2].text, "carol final conclusion");

    // WhiteBoard is append-only visible: one note per speaker, in order.
    let notes = wb.read_all(&session_id).await.unwrap();
    assert_eq!(
        notes.iter().map(|n| n.body.as_str()).collect::<Vec<_>>(),
        vec![
            "alice opening finding",
            "bob counter finding",
            "carol final conclusion"
        ]
    );

    // Events are appended per turn with monotonic seq.
    let db = nuomi_core::store::Db::open(&db_path).unwrap();
    let events =
        nuomi_core::store::repos::events::list_by_aggregate(&db.0, "session", &session_id, None)
            .unwrap();
    assert_eq!(
        events.iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(events.iter().all(|e| e.kind == "message"));

    // Context sync: Bob's request must contain Alice's utterance (shared history).
    let bob_requests = llm_bob.requests.lock().unwrap();
    assert!(bob_requests[0].messages[0]
        .content
        .contains("alice opening finding"));
    // And Carol sees both predecessors plus the whiteboard digest.
    let carol_requests = llm_carol.requests.lock().unwrap();
    let ctx = &carol_requests[0].messages[0].content;
    assert!(ctx.contains("alice opening finding"));
    assert!(ctx.contains("bob counter finding"));
    assert!(ctx.contains("WhiteBoard"));
}

/// AC9a (error path): handing off to a nonexistent participant fails clearly.
#[tokio::test]
async fn handoff_to_unknown_target_is_a_clear_error() {
    let alice = role("Alice", "p-a");
    let bob = role("Bob", "p-b");
    let session_id = new_id();

    let llm_alice = Arc::new(FakeLlm::new(
        "p-a",
        vec![handoff("to whom it may concern", "Nobody")],
    ));
    let input = TeamRunInput {
        session_id: session_id.clone(),
        task: "t".into(),
        roles: vec![alice.clone(), bob.clone()],
        team: team(&[&alice, &bob], json!({})),
        model: "m".into(),
    };
    let (wb, _path) = seeded_board(&session_id, &input.roles).await;
    let providers = resolver(
        vec![("p-a".into(), llm_alice)],
        Arc::new(FakeLlm::new("d", vec![])),
    );

    let executor = GroupChatExecutor::new(Arc::new(OpenThenConverge::default()));
    let err = executor
        .run(&input, &providers, &wb)
        .await
        .expect_err("must fail");
    match err {
        OrchestratorError::MemberNotFound { member, .. } => assert_eq!(member, "Nobody"),
        other => panic!("expected MemberNotFound, got {other}"),
    }
}

/// AC9c: A→B→A revisits beyond max_hops are rejected — termination guaranteed.
#[tokio::test]
async fn handoff_loop_beyond_max_hops_is_detected() {
    let alice = role("Alice", "p-a");
    let bob = role("Bob", "p-b");
    let session_id = new_id();

    let llm_alice = Arc::new(FakeLlm::new(
        "p-a",
        vec![
            handoff("a1", "Bob"),
            handoff("a2", "Bob"), // second receipt of Bob exceeds max_hops=1
        ],
    ));
    let llm_bob = Arc::new(FakeLlm::new("p-b", vec![handoff("b1", "Alice")]));

    let input = TeamRunInput {
        session_id: session_id.clone(),
        task: "t".into(),
        roles: vec![alice.clone(), bob.clone()],
        team: team(&[&alice, &bob], json!({ "max_hops": 1 })),
        model: "m".into(),
    };
    let (wb, _path) = seeded_board(&session_id, &input.roles).await;
    let providers = resolver(
        vec![("p-a".into(), llm_alice), ("p-b".into(), llm_bob)],
        Arc::new(FakeLlm::new("d", vec![])),
    );

    let executor = GroupChatExecutor::new(Arc::new(OpenThenConverge::default()));
    let err = executor
        .run(&input, &providers, &wb)
        .await
        .expect_err("must fail");
    match err {
        OrchestratorError::HandoffLoopDetected { chain, max } => {
            assert_eq!(max, 1);
            assert!(chain.contains("Alice") && chain.contains("Bob"));
        }
        other => panic!("expected HandoffLoopDetected, got {other}"),
    }
}

/// AC9b (executor side): consecutive speech is capped — the executor forces a
/// speaker change even when the selector keeps nominating the same agent.
#[tokio::test]
async fn same_agent_consecutive_speech_is_capped() {
    let a = role("A", "p0");
    let b = role("B", "p1");
    let session_id = new_id();

    let llm0 = Arc::new(FakeLlm::new(
        "p0",
        vec![FakeLlm::response("x"), FakeLlm::response("x")],
    ));
    let llm1 = Arc::new(FakeLlm::new(
        "p1",
        vec![FakeLlm::response("y"), FakeLlm::response("y")],
    ));
    let input = TeamRunInput {
        session_id: session_id.clone(),
        task: "t".into(),
        roles: vec![a.clone(), b.clone()],
        team: team(&[&a, &b], json!({ "max_consecutive": 1, "max_rounds": 4 })),
        model: "m".into(),
    };
    let (wb, _path) = seeded_board(&session_id, &input.roles).await;
    let providers = resolver(
        vec![("p0".into(), llm0), ("p1".into(), llm1)],
        Arc::new(FakeLlm::new("d", vec![])),
    );

    let executor = GroupChatExecutor::new(Arc::new(StubSelector(0)));
    let outcome = executor.run(&input, &providers, &wb).await.expect("run");

    let speakers: Vec<usize> = outcome.transcript.iter().map(|t| t.speaker).collect();
    assert_eq!(speakers, vec![0, 1, 0, 1]); // forced alternation
}

/// AC9c: exhausting max_rounds terminates the chat without convergence.
#[tokio::test]
async fn max_rounds_exhaustion_terminates() {
    let a = role("A", "p0");
    let session_id = new_id();

    let llm = Arc::new(FakeLlm::new(
        "p0",
        vec![
            FakeLlm::response("mono 1"),
            FakeLlm::response("mono 2"),
            FakeLlm::response("mono 3"),
        ],
    ));
    let input = TeamRunInput {
        session_id: session_id.clone(),
        task: "t".into(),
        roles: vec![a.clone()],
        team: team(&[&a], json!({ "max_rounds": 3, "max_consecutive": 10 })),
        model: "m".into(),
    };
    let (wb, _path) = seeded_board(&session_id, &input.roles).await;
    let providers = resolver(
        vec![("p0".into(), llm)],
        Arc::new(FakeLlm::new("d", vec![])),
    );

    let executor = GroupChatExecutor::new(Arc::new(StubSelector(0)));
    let outcome = executor.run(&input, &providers, &wb).await.expect("run");

    assert!(!outcome.converged);
    assert_eq!(outcome.rounds, 3);
    assert_eq!(outcome.transcript.len(), 3);
}

/// AC9e: Round-Robin degradation baseline rotates deterministically, with a
/// heuristic-selector contrast run under identical conditions.
#[tokio::test]
async fn round_robin_baseline_contrast() {
    use nuomi_core::orchestrator::HeuristicSelector;

    let a = role("A", "p0");
    let b = role("B", "p1");
    let c = role("C", "p2");
    let session_id = new_id();

    let mk_llm = |tag: &str| {
        Arc::new(FakeLlm::new(
            tag,
            vec![
                FakeLlm::response(&format!("{tag} says something")),
                FakeLlm::response(&format!("{tag} adds detail")),
                FakeLlm::response(&format!("{tag} concludes")),
            ],
        ))
    };
    let input = TeamRunInput {
        session_id: session_id.clone(),
        task: "t".into(),
        roles: vec![a.clone(), b.clone(), c.clone()],
        team: team(&[&a, &b, &c], json!({ "max_rounds": 3 })),
        model: "m".into(),
    };
    let (wb, _path) = seeded_board(&session_id, &input.roles).await;
    let providers = resolver(
        vec![
            ("p0".into(), mk_llm("p0")),
            ("p1".into(), mk_llm("p1")),
            ("p2".into(), mk_llm("p2")),
        ],
        Arc::new(FakeLlm::new("d", vec![])),
    );

    // Baseline: strict rotation, never repeats within a cycle.
    let rr = GroupChatExecutor::new(Arc::new(RoundRobinSelector));
    let outcome = rr.run(&input, &providers, &wb).await.expect("run");
    let speakers: Vec<usize> = outcome.transcript.iter().map(|t| t.speaker).collect();
    assert_eq!(speakers, vec![0, 1, 2]);

    // Contrast: the heuristic selector also terminates and, thanks to the
    // freshness factor, never lets one agent speak twice in a row here.
    let heuristic = GroupChatExecutor::new(Arc::new(HeuristicSelector));
    let outcome2 = heuristic.run(&input, &providers, &wb).await.expect("run");
    assert_eq!(outcome2.transcript.len(), 3);
    for pair in outcome2.transcript.windows(2) {
        assert_ne!(pair[0].speaker, pair[1].speaker);
    }
}
