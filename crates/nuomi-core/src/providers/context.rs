//! Send-view context pruning pipeline (hermes-style, purely local).
//!
//! All functions operate on a *view* destined for the LLM. Persisted
//! transcripts are never mutated; callers pass a clone.
//!
//! Pipeline: `repair_message_sequence` (role alternation) → duplicate tool
//! output collapse → tail cut under an approximate token budget (chars/4,
//! an intentionally rough estimate) with tool_call/tool_result pairs kept
//! atomic, folding the cut middle into one structured summary user message.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::providers::types::{ChatMessage, MessageRole, ToolCall};

/// Rough chars-per-token ratio used for budget math (approximation only).
const CHARS_PER_TOKEN: usize = 4;
/// Hard cap on the first line of a tool result quoted in a summary.
const SUMMARY_HEAD_CHARS: usize = 60;
/// Cap for the Goal / Completed lines inside the folded summary message.
const SUMMARY_LINE_CHARS: usize = 200;
/// Placeholder prefix replacing an older copy of identical tool output.
const DUPLICATE_PLACEHOLDER: &str =
    "[Duplicate tool output — identical to the one kept at message #";

/// Statistics about what the pruning pipeline collapsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneStats {
    pub duplicates_collapsed: usize,
    pub results_summarized: usize,
    pub folded_messages: usize,
}

/// The pruned send view plus diagnostics.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrunedView {
    pub messages: Vec<ChatMessage>,
    /// Approximate token count (chars/4) of the returned messages.
    pub approx_tokens: usize,
    pub stats: PruneStats,
}

/// Prunes a message view to fit `budget_tokens` (approximate). See module
/// docs for the pipeline stages and guarantees.
pub fn prune_view(messages: &[ChatMessage], budget_tokens: usize) -> PrunedView {
    let mut stats = PruneStats::default();
    let mut view = repair_message_sequence(messages.to_vec());
    collapse_duplicates(&mut view, &mut stats);

    let groups = group_messages(&view);
    let tail_start = select_tail(&groups, budget_tokens);

    let mut out: Vec<ChatMessage> = Vec::new();
    let mut folded: Vec<Vec<ChatMessage>> = Vec::new();
    // Hoist system messages out of the fold region; fold the rest in order.
    for group in &groups[..tail_start] {
        if group.kind == GroupKind::System {
            out.extend(group.messages.iter().cloned());
        } else {
            folded.push(group.messages.clone());
        }
    }
    if !folded.is_empty() {
        out.push(build_fold_summary(&folded));
    }
    let kept: Vec<ChatMessage> = groups[tail_start..]
        .iter()
        .flat_map(|g| g.messages.iter().cloned())
        .collect();
    stats.folded_messages = folded.iter().map(Vec::len).sum();

    out.extend(keep_within_budget(kept, budget_tokens, &mut stats));

    let approx_tokens = approx_tokens(&out);
    PrunedView {
        messages: out,
        approx_tokens,
        stats,
    }
}

/// Repairs role alternation before sending:
/// 1. drops tool results whose `tool_call_id` matches no earlier assistant
///    call (orphans);
/// 2. merges adjacent same-role messages (System/User/Assistant; tool
///    results each carry their own id and are never merged);
/// 3. keeps tool results that do answer a call, preserving pairing.
pub fn repair_message_sequence(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut answered: Vec<String> = Vec::new();
    for m in &messages {
        if m.role == MessageRole::Assistant {
            answered.extend(m.tool_calls.iter().map(|c| c.id.clone()));
        }
    }

    let mut repaired: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    for m in messages {
        if m.role == MessageRole::Tool {
            let is_answered = m
                .tool_call_id
                .as_ref()
                .is_some_and(|id| answered.contains(id));
            if !is_answered {
                continue;
            }
        }
        merge_into(&mut repaired, m);
    }
    repaired
}

fn merge_into(repaired: &mut Vec<ChatMessage>, m: ChatMessage) {
    let mergeable = matches!(
        m.role,
        MessageRole::System | MessageRole::User | MessageRole::Assistant
    );
    if let Some(last) = repaired.last_mut() {
        if mergeable && last.role == m.role {
            if !last.content.is_empty() && !m.content.is_empty() {
                last.content.push_str("\n\n");
            }
            last.content.push_str(&m.content);
            last.tool_calls.extend(m.tool_calls);
            return;
        }
    }
    repaired.push(m);
}

/// Replaces older copies of byte-identical tool output with a placeholder
/// pointing at the message number of the newest copy (1-based, within the
/// view produced by this pass).
fn collapse_duplicates(view: &mut [ChatMessage], stats: &mut PruneStats) {
    let mut last_kept: HashMap<String, usize> = HashMap::new();
    for (i, m) in view.iter().enumerate() {
        if m.role == MessageRole::Tool {
            last_kept.insert(content_hash(&m.content), i);
        }
    }
    for (i, m) in view.iter_mut().enumerate() {
        if m.role != MessageRole::Tool {
            continue;
        }
        let hash = content_hash(&m.content);
        if let Some(&kept_at) = last_kept.get(&hash) {
            if kept_at != i {
                m.content = format!("{DUPLICATE_PLACEHOLDER}{}]", kept_at + 1);
                stats.duplicates_collapsed += 1;
            }
        }
    }
}

fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    System,
    Plain,
    /// An assistant turn with tool calls plus its answers; kept atomic.
    ToolCycle,
}

#[derive(Debug)]
struct Group {
    kind: GroupKind,
    messages: Vec<ChatMessage>,
}

impl Group {
    fn approx_tokens(&self) -> usize {
        approx_tokens(&self.messages)
    }
}

/// Splits a view into atomic groups: a tool_call/tool_result pairing plus
/// every single standalone message.
fn group_messages(view: &[ChatMessage]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut i = 0;
    while i < view.len() {
        let m = &view[i];
        match m.role {
            MessageRole::System => {
                groups.push(Group {
                    kind: GroupKind::System,
                    messages: vec![m.clone()],
                });
                i += 1;
            }
            MessageRole::Assistant if !m.tool_calls.is_empty() => {
                let call_ids: Vec<&str> = m.tool_calls.iter().map(|c| c.id.as_str()).collect();
                let mut messages = vec![m.clone()];
                i += 1;
                while i < view.len() {
                    let next = &view[i];
                    let answers = next.role == MessageRole::Tool
                        && next
                            .tool_call_id
                            .as_deref()
                            .is_some_and(|id| call_ids.contains(&id));
                    if !answers {
                        break;
                    }
                    messages.push(next.clone());
                    i += 1;
                }
                groups.push(Group {
                    kind: GroupKind::ToolCycle,
                    messages,
                });
            }
            _ => {
                groups.push(Group {
                    kind: GroupKind::Plain,
                    messages: vec![m.clone()],
                });
                i += 1;
            }
        }
    }
    groups
}

/// Chooses the kept suffix of `groups` under the budget. Cuts only at group
/// boundaries; the last group is always kept, and the tail is anchored back
/// to the group containing the last user message (the final assistant turn
/// follows it, so both anchors stay inside the kept tail).
fn select_tail(groups: &[Group], budget_tokens: usize) -> usize {
    let mut used = 0usize;
    let mut start = groups.len().saturating_sub(1);
    for idx in (0..groups.len()).rev() {
        let t = groups[idx].approx_tokens();
        if used + t > budget_tokens && idx < groups.len() - 1 {
            break;
        }
        used += t;
        start = idx;
    }
    let last_user = groups
        .iter()
        .rposition(|g| g.messages.iter().any(|m| m.role == MessageRole::User));
    if let Some(user_group) = last_user {
        if user_group < start {
            start = user_group;
        }
    }
    start
}

/// Caps oversized tool results inside the kept tail when the tail alone
/// exceeds the budget: each capped result is replaced by a one-line
/// semantic summary (largest first, only while over budget).
fn keep_within_budget(
    mut messages: Vec<ChatMessage>,
    budget_tokens: usize,
    stats: &mut PruneStats,
) -> Vec<ChatMessage> {
    while approx_tokens(&messages) > budget_tokens {
        let Some((idx, summary)) = largest_summarizable(&messages).map(|idx| {
            let id = messages[idx].tool_call_id.clone();
            let (name, args) = call_lookup(&messages, id.as_deref());
            let summary = summarize_tool_result(&name, args, &messages[idx].content);
            (idx, summary)
        }) else {
            break;
        };
        messages[idx].content = summary;
        stats.results_summarized += 1;
    }
    messages
}

/// Index of the largest tool result worth summarizing (placeholders and
/// already-short outputs are skipped so the cap loop always terminates).
fn largest_summarizable(messages: &[ChatMessage]) -> Option<usize> {
    const MIN_RESULT_CHARS: usize = 64;
    let mut best: Option<(usize, usize)> = None;
    for (i, m) in messages.iter().enumerate() {
        if m.role != MessageRole::Tool || m.content.starts_with('[') {
            continue;
        }
        if m.content.len() <= MIN_RESULT_CHARS {
            continue;
        }
        match best {
            Some((_, len)) if len >= m.content.len() => {}
            _ => best = Some((i, m.content.len())),
        }
    }
    best.map(|(i, _)| i)
}

/// Resolves the name/arguments of a tool call by id within a message view.
fn call_lookup<'a>(
    messages: &'a [ChatMessage],
    call_id: Option<&str>,
) -> (String, Option<&'a serde_json::Value>) {
    let Some(id) = call_id else {
        return ("unknown".into(), None);
    };
    for m in messages {
        if m.role == MessageRole::Assistant {
            for call in &m.tool_calls {
                if call.id == id {
                    return (call.name.clone(), Some(&call.arguments));
                }
            }
        }
    }
    ("unknown".into(), None)
}

/// Builds one semantic line describing a cut tool result:
/// `[read_file] read config.py (3,400 chars)` or
/// `[terminal] ran \`npm test\` -> exit 0, 47 lines`.
pub fn summarize_tool_result(
    tool_name: &str,
    arguments: Option<&serde_json::Value>,
    result: &str,
) -> String {
    let verb = verb_for(tool_name);
    let subject = primary_argument(arguments).unwrap_or_default();
    let head = result.lines().next().unwrap_or_default().trim();
    let head = truncate_chars(head, SUMMARY_HEAD_CHARS);
    let mut line = if subject.is_empty() {
        format!("[{tool_name}] {verb}")
    } else if verb == "ran" {
        format!("[{tool_name}] {verb} `{subject}`")
    } else {
        format!("[{tool_name}] {verb} {subject}")
    };
    if verb == "ran" {
        if !head.is_empty() {
            line.push_str(" -> ");
            line.push_str(&head);
        }
    } else {
        line.push_str(&format!(
            " ({} chars)",
            format_thousands(result.chars().count())
        ));
        if !head.is_empty() {
            line.push_str(&format!("; head: {head}"));
        }
    }
    line
}

fn verb_for(tool_name: &str) -> &'static str {
    let n = tool_name.to_ascii_lowercase();
    if n.starts_with("read")
        || n.starts_with("get")
        || n.starts_with("show")
        || n.starts_with("view")
    {
        "read"
    } else if n.starts_with("write") || n.starts_with("create") || n.starts_with("save") {
        "wrote"
    } else if n.starts_with("edit")
        || n.starts_with("update")
        || n.starts_with("patch")
        || n.starts_with("replace")
    {
        "edited"
    } else if n.starts_with("terminal")
        || n.starts_with("shell")
        || n.starts_with("bash")
        || n.starts_with("run")
        || n.starts_with("exec")
    {
        "ran"
    } else if n.starts_with("grep")
        || n.starts_with("search")
        || n.starts_with("find")
        || n.starts_with("glob")
    {
        "searched"
    } else if n.starts_with("list") || n.starts_with("ls") || n.starts_with("dir") {
        "listed"
    } else if n.starts_with("fetch") || n.starts_with("http") || n.starts_with("web") {
        "fetched"
    } else {
        "called"
    }
}

/// Picks the most human-meaningful string argument for a summary line.
fn primary_argument(arguments: Option<&serde_json::Value>) -> Option<String> {
    let obj = arguments?.as_object()?;
    for key in [
        "file_path",
        "path",
        "file",
        "filename",
        "command",
        "cmd",
        "url",
        "query",
        "pattern",
    ] {
        if let Some(v) = obj.get(key).and_then(|v| v.as_str()) {
            return Some(truncate_chars(v, 60));
        }
    }
    obj.values()
        .find_map(|v| v.as_str())
        .map(|s| truncate_chars(s, 60))
}

fn format_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(d);
    }
    out
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

/// Folds the cut middle into one structured summary user message.
fn build_fold_summary(folded: &[Vec<ChatMessage>]) -> ChatMessage {
    let flat: Vec<&ChatMessage> = folded.iter().flatten().collect();
    let goal = flat
        .iter()
        .find(|m| m.role == MessageRole::User)
        .map(|m| truncate_chars(m.content.trim(), SUMMARY_LINE_CHARS))
        .unwrap_or_else(|| "(not recorded)".into());

    let mut completed = Vec::new();
    for group in folded {
        let assistant = group.iter().find(|m| m.role == MessageRole::Assistant);
        for m in group {
            match m.role {
                MessageRole::Tool => {
                    let (name, args) = call_for(assistant, m.tool_call_id.as_deref());
                    completed.push(summarize_tool_result(&name, args, &m.content));
                }
                MessageRole::Assistant if m.tool_calls.is_empty() => {
                    if !m.content.trim().is_empty() {
                        completed.push(format!(
                            "- assistant: {}",
                            truncate_chars(m.content.trim(), SUMMARY_LINE_CHARS)
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    let active = flat
        .iter()
        .rev()
        .find(|m| m.role == MessageRole::Assistant && !m.content.trim().is_empty())
        .map(|m| truncate_chars(m.content.trim(), SUMMARY_LINE_CHARS))
        .unwrap_or_else(|| "tool cycle in progress".into());

    ChatMessage::user(format!(
        "[Context folded — older messages trimmed to fit the token budget]\n\
         ## Goal\n{goal}\n\
         ## Completed\n{}\n\
         ## Active State\n{active}",
        if completed.is_empty() {
            "- (no tool activity)".into()
        } else {
            completed
                .into_iter()
                .map(|l| format!("- {l}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    ))
}

/// Finds the name/arguments of `call_id` inside an assistant turn.
fn call_for<'a>(
    assistant: Option<&'a ChatMessage>,
    call_id: Option<&str>,
) -> (String, Option<&'a serde_json::Value>) {
    let Some((assistant, call_id)) = assistant.zip(call_id) else {
        return ("unknown".into(), None);
    };
    assistant
        .tool_calls
        .iter()
        .find(|c| c.id == call_id)
        .map(
            |ToolCall {
                 name, arguments, ..
             }| (name.clone(), Some(arguments)),
        )
        .unwrap_or_else(|| ("unknown".into(), None))
}

fn approx_tokens(messages: &[ChatMessage]) -> usize {
    let chars: usize = messages.iter().map(|m| m.content.len()).sum();
    chars / CHARS_PER_TOKEN
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn big_result(chars: usize) -> String {
        "x".repeat(chars)
    }

    #[test]
    fn repair_merges_adjacent_same_role() {
        let msgs = vec![
            ChatMessage::system("a"),
            ChatMessage::system("b"),
            ChatMessage::user("hello"),
            ChatMessage::user("world"),
        ];
        let out = repair_message_sequence(msgs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].content, "a\n\nb");
        assert_eq!(out[1].content, "hello\n\nworld");
    }

    #[test]
    fn repair_drops_orphan_tool_results() {
        let msgs = vec![
            ChatMessage::user("q"),
            ChatMessage::tool_result("ghost", "orphan output"),
        ];
        let out = repair_message_sequence(msgs);
        assert!(out.iter().all(|m| m.role != MessageRole::Tool));
    }

    #[test]
    fn repair_keeps_paired_results_and_survives_assistant_merge() {
        let mut a1 = ChatMessage::assistant("first");
        a1.tool_calls.push(ToolCall {
            id: "c1".into(),
            name: "read_file".into(),
            arguments: json!({}),
        });
        let mut a2 = ChatMessage::assistant("second");
        a2.tool_calls.push(ToolCall {
            id: "c2".into(),
            name: "terminal".into(),
            arguments: json!({}),
        });
        let msgs = vec![
            a1,
            ChatMessage::tool_result("c1", "out1"),
            a2,
            ChatMessage::tool_result("c2", "out2"),
        ];
        let out = repair_message_sequence(msgs);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].tool_calls.len(), 1);
        assert_eq!(out[0].content, "first");
        assert_eq!(out[2].tool_calls[0].id, "c2");
        assert_eq!(out[1].content, "out1");
        assert_eq!(out[3].content, "out2");
    }

    #[test]
    fn duplicates_collapse_to_placeholder_pointing_at_kept_copy() {
        let mut asst = ChatMessage::assistant("run");
        asst.tool_calls.push(ToolCall {
            id: "c1".into(),
            name: "terminal".into(),
            arguments: json!({}),
        });
        let mut asst2 = ChatMessage::assistant("run again");
        asst2.tool_calls.push(ToolCall {
            id: "c2".into(),
            name: "terminal".into(),
            arguments: json!({}),
        });
        let msgs = vec![
            ChatMessage::user("q"),
            asst,
            ChatMessage::tool_result("c1", "same output"),
            asst2,
            ChatMessage::tool_result("c2", "same output"),
        ];
        let view = prune_view(&msgs, 10_000);
        assert_eq!(view.stats.duplicates_collapsed, 1);
        let dup = view
            .messages
            .iter()
            .find(|m| m.content.contains("Duplicate tool output"))
            .expect("placeholder present");
        assert_eq!(
            dup.content,
            "[Duplicate tool output — identical to the one kept at message #5]"
        );
        assert!(view.messages.iter().any(|m| m.content == "same output"));
    }

    #[test]
    fn summary_is_semantic_and_names_the_tool() {
        let args = json!({ "file_path": "config.py" });
        let line = summarize_tool_result("read_file", Some(&args), &big_result(3_400));
        assert!(line.starts_with("[read_file] read config.py"), "{line}");
        assert!(line.contains("(3,400 chars)"), "{line}");

        let args = json!({ "command": "npm test" });
        let line = summarize_tool_result("terminal", Some(&args), "exit 0, 47 lines");
        assert!(line.starts_with("[terminal] ran `npm test`"), "{line}");
        assert!(line.contains("exit 0, 47 lines"), "{line}");
    }

    #[test]
    fn cut_point_never_splits_a_tool_pair() {
        // [sys][user small][cycle ~100 tokens][user small][cycle ~80 tokens]
        // Budget admits the last user + last cycle, so the cut must land on
        // the group boundary between the two cycles, never inside a pair.
        let mut a1 = ChatMessage::assistant("calling");
        a1.tool_calls.push(ToolCall {
            id: "c1".into(),
            name: "read_file".into(),
            arguments: json!({ "file_path": "a.py" }),
        });
        let mut a2 = ChatMessage::assistant("calling again");
        a2.tool_calls.push(ToolCall {
            id: "c2".into(),
            name: "read_file".into(),
            arguments: json!({ "file_path": "b.py" }),
        });
        let msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("first task"),
            a1,
            ChatMessage::tool_result("c1", big_result(400)),
            ChatMessage::user("second task"),
            a2,
            ChatMessage::tool_result("c2", big_result(320)),
        ];
        let view = prune_view(&msgs, 100);
        // Every tool result in the view still has its call present.
        let call_ids: Vec<&str> = view
            .messages
            .iter()
            .flat_map(|m| m.tool_calls.iter().map(|c| c.id.as_str()))
            .collect();
        for m in &view.messages {
            if m.role == MessageRole::Tool {
                let id = m.tool_call_id.as_deref().expect("tool id kept");
                assert!(call_ids.contains(&id), "orphan result {id}");
            }
        }
        // The folded summary exists and carries the earlier goal.
        let summary = view
            .messages
            .iter()
            .find(|m| m.role == MessageRole::User && m.content.contains("## Goal"))
            .expect("fold summary present");
        assert!(summary.content.contains("first task"));
        // The last user message stays verbatim outside the summary.
        assert!(view
            .messages
            .iter()
            .any(|m| m.role == MessageRole::User && m.content == "second task"));
    }

    #[test]
    fn tail_is_anchored_on_the_last_user_message() {
        let mut asst = ChatMessage::assistant("mid");
        asst.tool_calls.push(ToolCall {
            id: "c1".into(),
            name: "read_file".into(),
            arguments: json!({}),
        });
        let msgs = vec![
            ChatMessage::user("first task"),
            asst,
            ChatMessage::tool_result("c1", big_result(800)),
            ChatMessage::assistant("intermediate"),
            ChatMessage::user("final task — must stay"),
        ];
        // Tiny budget: nothing but the anchor would fit, yet it must be kept.
        let view = prune_view(&msgs, 5);
        assert!(view
            .messages
            .iter()
            .any(|m| m.role == MessageRole::User && m.content.contains("final task")));
    }

    #[test]
    fn oversized_kept_results_are_summarized_semantically() {
        let mut asst = ChatMessage::assistant("read big file");
        asst.tool_calls.push(ToolCall {
            id: "c1".into(),
            name: "read_file".into(),
            arguments: json!({ "file_path": "huge.log" }),
        });
        let msgs = vec![
            ChatMessage::user("tail task"),
            asst,
            ChatMessage::tool_result("c1", big_result(40_000)),
        ];
        // Budget far below the result size: it must become a one-line summary.
        let view = prune_view(&msgs, 100);
        assert!(view.stats.results_summarized >= 1);
        let summarized = view
            .messages
            .iter()
            .find(|m| m.role == MessageRole::Tool)
            .expect("tool message kept");
        assert!(summarized.content.starts_with("[read_file] read huge.log"));
        assert!(summarized.content.contains("(40,000 chars)"));
        assert!(view.approx_tokens <= 100 + 64, "{}", view.approx_tokens);
    }
}
