//! API mode: chat threads with Claude and the daily pass, both over the tool
//! registry with the same autonomy gate as MCP. Every call is metered and
//! stops at the monthly cap.

use chrono::Utc;
use hearth_core::alerts::{AlertCandidate, AlertType, Severity};
use hearth_core::money::Cents;
use hearth_core::settings::AssistantMode;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Actor, Ctx, Error, Result, invalid};
use crate::api::AppState;
use crate::db::models::{ChatMessage, ChatThread};
use crate::integrations::claude::{self, Claude};
use crate::tools::{self, ToolCtx};

const MAX_ITERATIONS: usize = 16;
const MAX_RESULT_CHARS: usize = 12_000;
/// Output cap per API call. A pass categorizes one transaction per tool call
/// and may issue dozens in one response, so this has to hold them all; the
/// cap is not a cost (only generated tokens are billed).
const MAX_TOKENS: u32 = 16_000;
/// How many times one turn may pick up after the model hit `MAX_TOKENS`.
const MAX_CONTINUATIONS: usize = 3;
const CONTINUE_PROMPT: &str = "You ran out of room. Continue exactly where you left off without repeating what you already said.";
const CUT_OFF_RESULT: &str = "This call was cut off by the output limit and did not run. Call it again, fewer calls at a time.";

/// The Claude client for this household, or why there isn't one.
pub async fn client(ctx: &Ctx) -> Result<(Claude, hearth_core::settings::AssistantSettings)> {
    let settings = super::settings::load(ctx).await?;
    if settings.assistant.mode != AssistantMode::Api {
        return Err(invalid(
            "the assistant is in Pull mode; switch to API mode under More → Assistant",
        ));
    }
    if ctx.crypto.is_none() {
        return Err(invalid("APP_ENCRYPTION_KEY is required for API mode"));
    }
    let key = super::secrets::get(ctx, super::secrets::ANTHROPIC_API_KEY)
        .await?
        .ok_or_else(|| invalid("no Anthropic API key saved yet (More → Assistant)"))?;
    Ok((Claude::new(key).map_err(Error::Other)?, settings.assistant))
}

/// Refuse to spend past the cap; raise the alert once per month.
async fn check_cap(ctx: &Ctx, cap: Cents) -> Result<()> {
    let (spent, _) = super::assistant::usage_this_month(ctx).await?;
    if cap.0 > 0 && spent >= cap {
        let a = AlertCandidate {
            alert_type: AlertType::ApiSpendCap,
            severity: Severity::Info,
            title: format!("Claude API cap reached ({})", cap),
            detail: format!(
                "{} spent this month; Hearth stopped calling Claude until next month or a higher cap.",
                spent
            ),
            amount_cents: Some(spent),
            dedupe_key: format!("api_cap:{}", ctx.month()),
            transaction_id: None,
            category_id: None,
            bill_id: None,
            debt_id: None,
            account_id: None,
            planned_item_id: None,
            goal_id: None,
            context: json!({ "cap_cents": cap, "spent_cents": spent }),
        };
        // Once per month: a resolved cap alert stays resolved for the month.
        super::alerts::upsert_candidates(ctx, "api", &[a], None).await?;
        return Err(invalid(format!(
            "monthly Claude API cap reached ({cap}); raise it under More → Assistant"
        )));
    }
    Ok(())
}

async fn system_prompt(ctx: &Ctx, who: &str) -> Result<String> {
    let h = super::household::get(ctx).await?;
    let s = super::settings::load(ctx).await?;
    Ok(format!(
        "{}\n\nYou are inside the Hearth app for the household \"{}\", talking with {}. Today is {} ({} timezone). \
Autonomy: global {:?}; categorizing & notes {:?}; plans & bills {:?}; debts & goals {:?}. \
Tool names use double underscores where the docs use dots (budget__month is budget.month). \
Answer in a few short sentences; use plain numbers in dollars when talking to people. Never invent a number: read it with a tool.",
        crate::mcp::INSTRUCTIONS,
        h.name,
        who,
        ctx.today(),
        h.timezone,
        s.assistant.autonomy,
        s.assistant
            .autonomy_for(hearth_core::settings::AutonomyArea::CategorizingNotes),
        s.assistant
            .autonomy_for(hearth_core::settings::AutonomyArea::PlansBills),
        s.assistant
            .autonomy_for(hearth_core::settings::AutonomyArea::DebtsGoals),
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCall {
    pub name: String,
    pub args: Value,
    pub ok: bool,
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Turn {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub cost_cents: Cents,
    pub stop_reason: String,
    /// The API messages appended during this turn (assistant and tool-result rows).
    pub messages: Vec<Value>,
}

/// The `tool_use` blocks of a response.
fn tool_uses(content: &[Value]) -> Vec<&Value> {
    content
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
        .collect()
}

/// A response the output cap cut off mid tool call carries tool inputs that may
/// be truncated, so none of them runs; each gets an error result instead and the
/// model gets to redo them. Every `tool_use` needs a `tool_result` or the API
/// rejects the next request.
fn cut_off_results(content: &[Value]) -> Vec<Value> {
    tool_uses(content)
        .into_iter()
        .map(|b| {
            let id = b.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            json!({ "type": "tool_result", "tool_use_id": id, "content": CUT_OFF_RESULT, "is_error": true })
        })
        .collect()
}

/// Run the model until it stops asking for tools. `history` is the thread so far.
/// `purpose` is `chat` (a person is reading) or `pass` (nobody is).
#[allow(clippy::too_many_arguments)]
pub async fn run_turn(
    state: &AppState,
    ctx: &Ctx,
    client: &Claude,
    model: &str,
    cap: Cents,
    purpose: &str,
    system: &str,
    history: &[Value],
) -> Result<Turn> {
    let tool_defs = claude::tool_definitions();
    let mut messages: Vec<Value> = history.to_vec();
    let mut appended: Vec<Value> = Vec::new();
    let mut calls = Vec::new();
    let mut cost = Cents::ZERO;
    let mut text = String::new();
    let mut stop = String::new();
    let mut continuations = 0usize;
    // The last response was cut off mid-sentence: glue the next one on.
    let mut mid_sentence = false;
    let assistant = Actor::Assistant {
        key_id: None,
        name: "api".into(),
    };

    for _ in 0..MAX_ITERATIONS {
        check_cap(ctx, cap).await?;
        let res = client
            .messages(claude::Request {
                model,
                system,
                messages: &messages,
                tools: &tool_defs,
                max_tokens: MAX_TOKENS,
            })
            .await
            .map_err(Error::Other)?;
        let c = claude::cost_cents(model, &res.usage);
        cost += c;
        super::assistant::record_usage(
            ctx,
            super::assistant::UsageRecord {
                purpose: purpose.into(),
                model: model.into(),
                input_tokens: (res.usage.input_tokens
                    + res.usage.cache_read_input_tokens
                    + res.usage.cache_creation_input_tokens)
                    .min(i32::MAX as i64) as i32,
                output_tokens: res.usage.output_tokens.min(i32::MAX as i64) as i32,
                cost_cents: c,
            },
        )
        .await?;
        stop = res.stop_reason.clone().unwrap_or_default();
        let assistant_msg = json!({ "role": "assistant", "content": res.content });
        messages.push(assistant_msg.clone());
        appended.push(assistant_msg);
        for block in &res.content {
            if block.get("type").and_then(|t| t.as_str()) == Some("text")
                && let Some(t) = block.get("text").and_then(|t| t.as_str())
            {
                if !text.is_empty() && !mid_sentence {
                    text.push('\n');
                }
                mid_sentence = false;
                text.push_str(t);
            }
        }
        if stop == "max_tokens" {
            // The output cap cut the response off. Let the model pick up again a
            // bounded number of times: a pass has nobody to ask it to continue.
            continuations += 1;
            if continuations > MAX_CONTINUATIONS {
                break;
            }
            let cut = cut_off_results(&res.content);
            let user_msg = if cut.is_empty() {
                mid_sentence = true;
                json!({ "role": "user", "content": [{ "type": "text", "text": CONTINUE_PROMPT }] })
            } else {
                json!({ "role": "user", "content": cut })
            };
            messages.push(user_msg.clone());
            appended.push(user_msg);
            continue;
        }
        if stop != "tool_use" {
            break;
        }
        let mut results = Vec::new();
        for block in tool_uses(&res.content) {
            let id = block
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let name = claude::registry_tool_name(
                block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            );
            let args = block.get("input").cloned().unwrap_or(Value::Null);
            let tc = ToolCtx {
                state: state.clone(),
                ctx: ctx.clone(),
                actor: assistant.clone(),
                can_write: true,
            };
            let (content, is_error, ok, outcome) = match tools::call(tc, &name, args.clone()).await
            {
                Ok(v) => {
                    let outcome = v
                        .get("outcome")
                        .and_then(|o| o.as_str())
                        .map(str::to_string);
                    let mut s = serde_json::to_string(&v).unwrap_or_default();
                    if s.len() > MAX_RESULT_CHARS {
                        s.truncate(MAX_RESULT_CHARS);
                        s.push_str("… (truncated)");
                    }
                    (s, false, true, outcome)
                }
                Err(e) => (e.to_string(), true, false, None),
            };
            calls.push(ToolCall {
                name,
                args,
                ok,
                outcome,
            });
            results.push(json!({ "type": "tool_result", "tool_use_id": id, "content": content, "is_error": is_error }));
        }
        let user_msg = json!({ "role": "user", "content": results });
        messages.push(user_msg.clone());
        appended.push(user_msg);
    }
    if stop == "max_tokens" {
        text.push_str(if purpose == "chat" {
            "\n\n(Claude ran out of room; ask it to continue.)"
        } else {
            "\n\n(Claude ran out of room before finishing; the next pass picks up where this one stopped.)"
        });
    }
    if stop == "tool_use" {
        // The iteration cap ended the loop with tool results the model never saw.
        text.push_str(&format!(
            "\n\n(Claude stopped after {MAX_ITERATIONS} rounds of tool calls; the rest waits for the next pass or message.)"
        ));
    }
    if stop == "refusal" {
        text.push_str("\n\n(Claude declined to continue that request.)");
    }
    Ok(Turn {
        text,
        tool_calls: calls,
        cost_cents: cost,
        stop_reason: stop,
        messages: appended,
    })
}

// ── threads ──

pub async fn threads(ctx: &Ctx) -> Result<Vec<ChatThread>> {
    Ok(sqlx::query_as::<_, ChatThread>(
        "select * from chat_threads where household_id = $1 order by updated_at desc limit 50",
    )
    .bind(ctx.household_id)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn create_thread(ctx: &Ctx, user_id: Uuid) -> Result<ChatThread> {
    Ok(sqlx::query_as::<_, ChatThread>(
        "insert into chat_threads (household_id, user_id) values ($1, $2) returning *",
    )
    .bind(ctx.household_id)
    .bind(user_id)
    .fetch_one(&ctx.pool)
    .await?)
}

pub async fn get_thread(ctx: &Ctx, id: Uuid) -> Result<ChatThread> {
    sqlx::query_as::<_, ChatThread>(
        "select * from chat_threads where household_id = $1 and id = $2",
    )
    .bind(ctx.household_id)
    .bind(id)
    .fetch_optional(&ctx.pool)
    .await?
    .ok_or(Error::NotFound)
}

pub async fn messages(ctx: &Ctx, thread_id: Uuid) -> Result<Vec<ChatMessage>> {
    get_thread(ctx, thread_id).await?;
    Ok(sqlx::query_as::<_, ChatMessage>(
        "select * from chat_messages where thread_id = $1 order by created_at, id",
    )
    .bind(thread_id)
    .fetch_all(&ctx.pool)
    .await?)
}

async fn store(
    ctx: &Ctx,
    thread_id: Uuid,
    role: &str,
    content: Value,
    text: &str,
    cost: Cents,
) -> Result<ChatMessage> {
    Ok(sqlx::query_as::<_, ChatMessage>(
        "insert into chat_messages (thread_id, role, content, text, cost_cents) values ($1, $2, $3, $4, $5) returning *",
    )
    .bind(thread_id)
    .bind(role)
    .bind(content)
    .bind(text)
    .bind(cost)
    .fetch_one(&ctx.pool)
    .await?)
}

/// Text from an API message's content (for display and titles).
fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A person says something in a thread; Claude answers with tools. Returns the new rows.
pub async fn send(
    state: &AppState,
    ctx: &Ctx,
    user: &crate::db::models::User,
    thread_id: Uuid,
    text: &str,
) -> Result<Vec<ChatMessage>> {
    if text.trim().is_empty() {
        return Err(invalid("say something"));
    }
    let (client, settings) = client(ctx).await?;
    let thread = get_thread(ctx, thread_id).await?;
    let history: Vec<Value> = messages(ctx, thread_id)
        .await?
        .into_iter()
        .map(|m| json!({ "role": m.role, "content": m.content }))
        .collect();
    let mut new_rows = Vec::new();
    let user_content = json!([{ "type": "text", "text": text.trim() }]);
    new_rows.push(
        store(
            ctx,
            thread_id,
            "user",
            user_content.clone(),
            text.trim(),
            Cents::ZERO,
        )
        .await?,
    );
    let mut all = history;
    all.push(json!({ "role": "user", "content": user_content }));
    let system = system_prompt(ctx, &user.display_name).await?;
    let turn = match run_turn(
        state,
        ctx,
        &client,
        &settings.chat_model,
        settings.monthly_cap_cents,
        "chat",
        &system,
        &all,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            // Don't leave a dangling user message when Claude never answered.
            sqlx::query("delete from chat_messages where id = $1")
                .bind(new_rows[0].id)
                .execute(&ctx.pool)
                .await?;
            return Err(e);
        }
    };
    let n = turn.messages.len();
    for (i, m) in turn.messages.into_iter().enumerate() {
        let role = m
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("assistant")
            .to_string();
        let content = m.get("content").cloned().unwrap_or(Value::Null);
        let t = if role == "assistant" {
            text_of(&content)
        } else {
            String::new()
        };
        let cost = if i + 1 == n {
            turn.cost_cents
        } else {
            Cents::ZERO
        };
        new_rows.push(store(ctx, thread_id, &role, content, &t, cost).await?);
    }
    let title = if thread.title.is_empty() {
        text.trim().chars().take(60).collect::<String>()
    } else {
        thread.title.clone()
    };
    sqlx::query("update chat_threads set title = $2, updated_at = now() where id = $1")
        .bind(thread_id)
        .bind(title)
        .execute(&ctx.pool)
        .await?;
    Ok(new_rows)
}

/// The scheduled pass: work the queue, then leave a summary.
pub async fn pass(state: &AppState, ctx: &Ctx) -> Result<Turn> {
    let (client, settings) = client(ctx).await?;
    let system = system_prompt(ctx, "nobody (this is your scheduled pass)").await?;
    let prompt = "Take your daily pass over the household's budget. Call assistant__work first and work through what it returns: \
categorize what you can with confidence, ask with questions__ask when a category is genuinely ambiguous, match or note overdue bills, \
and look at open alerts. Respect your autonomy position: in Backseat your writes become proposals. \
Finish by calling assistant__summary_write with two or three plain lines about what you did and what needs a person.";
    let history = vec![json!({ "role": "user", "content": [{ "type": "text", "text": prompt }] })];
    let turn = run_turn(
        state,
        ctx,
        &client,
        &settings.pass_model,
        settings.monthly_cap_cents,
        "pass",
        &system,
        &history,
    )
    .await?;
    if !turn
        .tool_calls
        .iter()
        .any(|c| c.name == "assistant.summary_write")
    {
        let summary = if turn.text.trim().is_empty() {
            "Pass finished without a summary.".to_string()
        } else {
            turn.text.trim().chars().take(600).collect()
        };
        super::assistant::write_summary(
            ctx,
            &Actor::Assistant {
                key_id: None,
                name: "api".into(),
            },
            summary,
        )
        .await?;
    }
    Ok(turn)
}

/// Is a pass due now: API mode, a key, past `pass_time` today, none run since.
pub async fn pass_due(ctx: &Ctx) -> Result<bool> {
    let s = super::settings::load(ctx).await?;
    if s.assistant.mode != AssistantMode::Api || ctx.crypto.is_none() {
        return Ok(false);
    }
    if !super::secrets::exists(ctx, super::secrets::ANTHROPIC_API_KEY).await? {
        return Ok(false);
    }
    let today = ctx.today();
    let due_at = hearth_core::calendar::local_datetime_to_utc(
        ctx.tz,
        today,
        s.assistant.pass_time.hour,
        s.assistant.pass_time.minute,
    );
    if Utc::now() < due_at {
        return Ok(false);
    }
    let last: Option<(chrono::DateTime<Utc>,)> = sqlx::query_as(
        "select started_at from job_runs where job = 'assistant:pass' and status in ('ok','failed') order by started_at desc limit 1",
    )
    .fetch_optional(&ctx.pool)
    .await?;
    Ok(last.is_none_or(|(t,)| t < due_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cut_off_tool_calls_get_error_results_not_runs() {
        let content = vec![
            json!({ "type": "text", "text": "Categorizing…" }),
            json!({ "type": "tool_use", "id": "toolu_1", "name": "transactions__categorize", "input": { "transaction_id": "a" } }),
            json!({ "type": "tool_use", "id": "toolu_2", "name": "transactions__categorize", "input": {} }),
        ];
        let results = cut_off_results(&content);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["tool_use_id"], "toolu_1");
        assert_eq!(results[1]["tool_use_id"], "toolu_2");
        assert!(results.iter().all(|r| r["is_error"] == true));
        assert!(cut_off_results(&[json!({ "type": "text", "text": "just words" })]).is_empty());
    }
}
