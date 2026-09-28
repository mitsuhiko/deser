//! Sessions of the pi coding agent (`benchmark/data/pi-sessions`).
//!
//! A session is a JSONL file, every line is an entry: the header, messages
//! of the user, the model and tools, model changes, compactions and so on.
//! The two sessions are long sessions of real work, one with an OpenAI and
//! one with an Anthropic model, with screenshots.  They were stripped of
//! all personal data with `scripts/scrub-pi-session.mjs` (all text is
//! replaced by random words of the same length, see there).
//!
//! What makes them interesting: large strings with a lot of escapes (code,
//! diffs and tool output, JSON encoded in strings), base64 images of
//! several hundred KiB, internally tagged enums on three levels (`type` of
//! the entry, `role` of the message, `type` of the content), untagged enums
//! (the arguments and details of tools), recursive JSON schemas and many
//! optional fields.
//!
//! The types follow the TypeScript types of pi (`SessionEntry` of the
//! coding agent and `Message` of pi-ai) for everything the sessions
//! contain, nothing is left as a dynamic value.  `check` makes sure that
//! the types do not lose anything.
use std::collections::BTreeMap;

use deser::{Deserialize, Serialize};

/// An entry (a line) of a session.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(tag = "type", rename_all = "snake_case")]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    Session(SessionHeader),
    Message(MessageEntry),
    ModelChange(ModelChangeEntry),
    ThinkingLevelChange(ThinkingLevelChangeEntry),
    Compaction(CompactionEntry),
    Usage(UsageEntry),
    ContextEdit(ContextEditEntry),
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    version: Option<u32>,
    id: String,
    timestamp: String,
    cwd: String,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_session: Option<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct MessageEntry {
    id: String,
    parent_id: Option<String>,
    timestamp: String,
    message: Message,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct ModelChangeEntry {
    id: String,
    parent_id: Option<String>,
    timestamp: String,
    provider: String,
    model_id: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct ThinkingLevelChangeEntry {
    id: String,
    parent_id: Option<String>,
    timestamp: String,
    thinking_level: ThinkingLevel,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct CompactionEntry {
    id: String,
    parent_id: Option<String>,
    timestamp: String,
    summary: String,
    first_kept_entry_id: String,
    tokens_before: u64,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    details: Option<CompactionDetails>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage: Option<Usage>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from_hook: Option<bool>,
}

/// The files the compacted part of the session read and modified.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct CompactionDetails {
    read_files: Vec<String>,
    modified_files: Vec<String>,
}

/// Usage outside of messages (such as keeping the prompt cache warm).
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    id: String,
    parent_id: Option<String>,
    timestamp: String,
    kind: String,
    provider: String,
    model: String,
    usage: Usage,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

/// Replaces (or with `null` removes) the content of an earlier entry.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct ContextEditEntry {
    id: String,
    parent_id: Option<String>,
    timestamp: String,
    target_id: String,
    replacement: Option<Replacement>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Replacement {
    content: ReplacementContent,
}

/// The content of any message.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(untagged)]
#[serde(untagged)]
enum ReplacementContent {
    Text(String),
    User(Vec<UserContent>),
    Assistant(Vec<AssistantContent>),
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(tag = "role", rename_all = "camelCase")]
#[serde(tag = "role", rename_all = "camelCase")]
enum Message {
    System(SystemMessage),
    User(UserMessage),
    Assistant(AssistantMessage),
    ToolResult(ToolResultMessage),
}

/// The system prompt and the tools.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct SystemMessage {
    content: String,
    /// Named sections of the prompt, `null` removes a section.
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sections: Option<BTreeMap<String, Option<String>>>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tools_added: Option<Vec<Tool>>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tools_removed: Option<Vec<ToolReference>>,
    timestamp: u64,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct Tool {
    name: String,
    description: String,
    parameters: Schema,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    constrained_sampling: Option<ConstrainedSampling>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct ToolReference {
    name: String,
}

/// The JSON schema of the parameters of a tool.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(
    tag = "type",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
#[serde(
    tag = "type",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
enum Schema {
    Object {
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required: Option<Vec<String>>,
        properties: BTreeMap<String, Schema>,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        additional_properties: Option<bool>,
    },
    Array {
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        items: Box<Schema>,
    },
    String {
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Number {
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Integer {
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minimum: Option<i64>,
    },
    Boolean {
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(tag = "type", rename_all = "snake_case")]
#[serde(tag = "type", rename_all = "snake_case")]
enum ConstrainedSampling {
    JsonSchema { strict: Strictness },
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
enum Strictness {
    Prefer,
    Require,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct UserMessage {
    content: Vec<UserContent>,
    timestamp: u64,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct AssistantMessage {
    content: Vec<AssistantContent>,
    api: String,
    provider: String,
    model: String,
    /// The model that answered if it is not `model`.
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response_model: Option<String>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response_id: Option<String>,
    /// The effort level as the provider calls it.
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_thinking_level: Option<String>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thinking_level: Option<ThinkingLevel>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<Diagnostic>>,
    usage: Usage,
    stop_reason: StopReason,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error_message: Option<String>,
    /// The stop reason as the provider calls it.
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw_stop_reason: Option<String>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end_turn: Option<bool>,
    timestamp: u64,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
enum StopReason {
    Pending,
    Stop,
    Length,
    ToolUse,
    Error,
    Aborted,
    Deferred,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct Usage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    /// The part of `cache_write` that is kept for an hour (Anthropic).
    #[deser(rename = "cacheWrite1h", default, skip_serializing_if = Option::is_none)]
    #[serde(
        rename = "cacheWrite1h",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    cache_write_1h: Option<u64>,
    /// The part of `output` that is reasoning.
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning: Option<u64>,
    total_tokens: u64,
    cost: Cost,
}

/// The cost in dollars.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct Cost {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
    total: f64,
}

/// What went wrong talking to the provider.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum Diagnostic {
    ProviderTransportFailure {
        timestamp: u64,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<DiagnosticError>,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<TransportFailure>,
    },
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct DiagnosticError {
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    message: String,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stack: Option<String>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    code: Option<ErrorCode>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(untagged)]
#[serde(untagged)]
enum ErrorCode {
    Number(i64),
    String(String),
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct TransportFailure {
    configured_transport: Transport,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fallback_transport: Option<Transport>,
    phase: String,
    events_emitted: bool,
    request_bytes: u64,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
enum Transport {
    Auto,
    Sse,
    Websocket,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct ToolResultMessage {
    tool_call_id: String,
    tool_name: String,
    content: Vec<UserContent>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    details: Option<ToolDetails>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage: Option<Usage>,
    is_error: bool,
    timestamp: u64,
}

/// The content of user messages and tool results.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(tag = "type", rename_all = "camelCase")]
#[serde(tag = "type", rename_all = "camelCase")]
enum UserContent {
    Text(TextContent),
    Image(ImageContent),
}

/// The content of assistant messages.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(tag = "type", rename_all = "camelCase")]
#[serde(tag = "type", rename_all = "camelCase")]
enum AssistantContent {
    Text(TextContent),
    Thinking(ThinkingContent),
    ToolCall(ToolCall),
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct TextContent {
    text: String,
    /// Metadata of the provider (OpenAI: JSON in a string).
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text_signature: Option<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct ThinkingContent {
    thinking: String,
    /// The encrypted reasoning (OpenAI: JSON in a string, Anthropic: base64).
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thinking_signature: Option<String>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    redacted: Option<bool>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct ImageContent {
    /// The image in base64.
    data: String,
    mime_type: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct ToolCall {
    id: String,
    name: String,
    arguments: ToolArguments,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thought_signature: Option<String>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
}

/// The arguments of the tools.  The first variant that matches wins, the
/// variants with more required fields come first.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(untagged)]
#[serde(untagged)]
enum ToolArguments {
    Edit {
        path: String,
        edits: Vec<Edit>,
    },
    Write {
        path: String,
        content: String,
    },
    Bash {
        command: String,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
    },
    WebSearch {
        query: String,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        objective: Option<String>,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_results: Option<u32>,
    },
    Read {
        path: String,
        /// The line to start at (sometimes `null`).
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<u64>,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u64>,
    },
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct Edit {
    old_text: String,
    new_text: String,
}

/// The details of tool results (not shown to the model).
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(untagged, rename_all_fields = "camelCase")]
#[serde(untagged, rename_all_fields = "camelCase")]
enum ToolDetails {
    Edit {
        diff: String,
        patch: String,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_changed_line: Option<u64>,
    },
    WebSearch {
        kind: String,
        provider: String,
        query: String,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        objective: Option<String>,
        search_id: String,
        results: Vec<SearchResult>,
    },
    /// Of `read` and `bash`, all fields are optional so it comes last.
    Output {
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        truncation: Option<Truncation>,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        full_output_path: Option<String>,
    },
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct SearchResult {
    url: String,
    title: String,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publish_date: Option<String>,
    text: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct Truncation {
    content: String,
    truncated: bool,
    truncated_by: Option<TruncatedBy>,
    total_lines: u64,
    total_bytes: u64,
    output_lines: u64,
    output_bytes: u64,
    last_line_partial: bool,
    first_line_exceeds_limit: bool,
    max_lines: u64,
    max_bytes: u64,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
enum TruncatedBy {
    Lines,
    Bytes,
}

/// Makes sure that an entry has everything of its line, that the types do
/// not drop any field or value.
///
/// The entry is serialized again and compared with the line.  A missing
/// field and `null` count as equal (the types leave out `None`) and numbers
/// are compared by value (costs of `0` are floats in Rust).  Floats may
/// differ in the last bits, serde_json (without `float_roundtrip`) does
/// not round all of them correctly.
pub fn check(line: &str, entry: &Entry) -> Result<(), String> {
    let expected: serde_json::Value = serde_json::from_str(line).map_err(|err| err.to_string())?;
    let value = serde_json::to_value(entry).map_err(|err| err.to_string())?;
    compare(&value, &expected, &mut String::new())
}

fn compare(
    value: &serde_json::Value,
    expected: &serde_json::Value,
    path: &mut String,
) -> Result<(), String> {
    use serde_json::Value;
    match (value, expected) {
        (Value::Number(a), Value::Number(b))
            if a.as_f64()
                .zip(b.as_f64())
                .is_some_and(|(a, b)| a == b || (a - b).abs() <= a.abs().max(b.abs()) * 1e-15) =>
        {
            Ok(())
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                let len = path.len();
                path.push_str(&format!("[{}]", index));
                compare(a, b, path)?;
                path.truncate(len);
            }
            Ok(())
        }
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys()) {
                let len = path.len();
                path.push('.');
                path.push_str(key);
                match (a.get(key), b.get(key)) {
                    (Some(a), Some(b)) => compare(a, b, path)?,
                    (None | Some(Value::Null), None | Some(Value::Null)) => {}
                    (None, Some(_)) => return Err(format!("{} is lost", path)),
                    (Some(_), None) => return Err(format!("{} is added", path)),
                }
                path.truncate(len);
            }
            Ok(())
        }
        _ if value == expected => Ok(()),
        _ => Err(format!("{} differs: {} != {}", path, value, expected)),
    }
}
