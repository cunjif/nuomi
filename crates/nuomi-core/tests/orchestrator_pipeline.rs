//! AC7: fake-provider end-to-end pipeline test — Team-driven serial execution,
//! WhiteBoard visibility, ordered events, final summary persisted.

use nuomi_core::domain::{new_id, Role, Team, TeamTopology};
use nuomi_core::orchestrator::{
    PipelineExecutor, ProviderResolver, TeamRunInput, WhiteBoardService,
};
use nuomi_core::providers::{FakeLlm, LlmProvider};
use serde_json::json;
use std::sync::Arc;

fn role(name: &str, provider_id: &str) -> Role {
    Role {
        id: new_id(),
        name: name.into(),
        provider_id: Some(provider_id.into()),
        provider_ids: vec![provider_id.into()],
        system_prompt_override: Some(format!("You are {name}, the specialist.")),
        tool_allowlist: vec![],
        required_capabilities: vec![],
        temperature: None,
        max_tokens: None,
        params: json!({}),
        builtin: false,
        generated: false,
        ephemeral: false,
        source: None,
        created_at: 0,
        updated_at: 0,
    }
}

#[tokio::test]
async fn pipeline_runs_three_roles_end_to_end_with_whiteboard_and_events() {
    let r1 = role("analyst", "p1");
    let r2 = role("engineer", "p2");
    let r3 = role("reviewer", "p3");
    let session_id = new_id();

    // Each stage consumes the previous stage's output as its user input.
    let llm1 = Arc::new(FakeLlm::new("p1", vec![FakeLlm::response("analysis done")]));
    let llm2 = Arc::new(FakeLlm::new(
        "p2",
        vec![FakeLlm::response("implementation done")],
    ));
    let llm3 = Arc::new(FakeLlm::new(
        "p3",
        vec![FakeLlm::response("review approved")],
    ));

    let team = Team {
        id: new_id(),
        name: "feature crew".into(),
        topology: TeamTopology::Pipeline,
        member_role_ids: vec![r1.id.clone(), r2.id.clone(), r3.id.clone()],
        config: json!({}),
        created_at: 0,
        updated_at: 0,
    };
    let roles = vec![r1.clone(), r2.clone(), r3.clone()];

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("pipe.db").to_string_lossy().to_string();
    let db = nuomi_core::store::Db::open(&db_path).unwrap();
    nuomi_core::store::migrations::run(&db.0).unwrap();
    db.0.execute(
        "INSERT INTO sessions (id, title, created_at, updated_at) VALUES (?1, '', 1, 1)",
        (session_id.as_str(),),
    )
    .unwrap();
    for r in &roles {
        db.0.execute(
            "INSERT INTO roles (id, name, created_at, updated_at) VALUES (?1, ?2, 1, 1)",
            (r.id.as_str(), r.name.as_str()),
        )
        .unwrap();
    }

    let input = TeamRunInput {
        session_id: session_id.clone(),
        task: "ship the feature".into(),
        roles,
        team,
        model: "test-model".into(),
        workspace_id: None,
    };

    let mut resolver =
        ProviderResolver::new(Arc::new(FakeLlm::new("default", vec![])) as Arc<dyn LlmProvider>);
    resolver = resolver
        .with("p1", llm1.clone() as Arc<dyn LlmProvider>)
        .with("p2", llm2.clone() as Arc<dyn LlmProvider>)
        .with("p3", llm3.clone() as Arc<dyn LlmProvider>);

    let wb = WhiteBoardService::new(db_path.clone());
    let outcome = PipelineExecutor::run(&input, &resolver, &wb)
        .await
        .expect("pipeline run");

    // Serial hand-off of outputs between stages.
    assert_eq!(outcome.steps.len(), 3);
    assert_eq!(outcome.steps[0].output, "analysis done");
    assert_eq!(outcome.final_output, "review approved");

    let (second_input, third_input) = {
        let reqs2 = llm2.requests.lock().unwrap();
        let reqs3 = llm3.requests.lock().unwrap();
        (
            reqs2[0].messages[0].content.clone(),
            reqs3[0].messages[0].content.clone(),
        )
    };
    assert_eq!(second_input, "analysis done");
    assert_eq!(third_input, "implementation done");
    {
        let reqs2 = llm2.requests.lock().unwrap();
        assert_eq!(
            reqs2[0].system_prompt.as_deref(),
            Some("You are engineer, the specialist.")
        );
    }

    // WhiteBoard: one finding per member, in pipeline order.
    let notes = wb.read_all(&session_id).await.unwrap();
    assert_eq!(notes.len(), 3);
    assert_eq!(
        notes.iter().map(|n| n.body.as_str()).collect::<Vec<_>>(),
        vec!["analysis done", "implementation done", "review approved"]
    );
    assert_eq!(
        notes
            .iter()
            .map(|n| n.note_type.as_str())
            .collect::<Vec<_>>(),
        vec!["finding"; 3]
    );

    // Events: since ADR-0002 mirroring, each stage persists a `whiteboard`
    // note mirror followed by its `message` record, seq-ordered.
    let events =
        nuomi_core::store::repos::events::list_by_aggregate(&db.0, "session", &session_id, None)
            .unwrap();
    assert_eq!(events.len(), 6);
    assert_eq!(
        events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
        vec![
            "whiteboard",
            "message",
            "whiteboard",
            "message",
            "whiteboard",
            "message"
        ]
    );
    assert_eq!(events[5].payload["content"], "review approved");

    // Final summary contains the last output.
    assert!(outcome.final_output.contains("review approved"));
}
