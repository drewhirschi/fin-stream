# Inbox: LLM extraction + auto-link to loans

Add a Claude extraction step after an email's body is fetched. Pull out *who the email is actually from* (walking through forwarded headers) and any *loan numbers* mentioned, and if one of those loan numbers matches a known TMO loan, link the email automatically.

## Motivation

Most of what's in the inbox is forwarded mail from someone like Val-Chris, where the real sender is buried inside the body and the loan number is somewhere in the subject line, forwarded block, or PDF attachment name. Today every one of these needs a manual **Link to loan** click. We already store the full body and the loan roster — this is the classic case where a small model pass over the body eliminates rote work and we already have the ground truth to check against.

## Target behavior

After `fetch_and_store_email` finishes and the body is on disk:

1. Kick off an extraction job (same background task, no new queue) that sends the body to Claude Haiku 4.5 with a tool-use schema asking for structured fields.
2. Store whatever comes back on the email row.
3. If the extracted loan number matches a row in `intg.tmo_import_loan`, call the existing `link_email_to_loan(email_id, loan_account)` path automatically. Show the result in the UI with a subtle "auto-linked" badge so it's distinguishable from manual links.
4. If no match or low confidence, leave the email unlinked but surface the extracted fields (original sender, candidate loan numbers, confidence) in the inbox row so manual linking is a click and a pre-filled dropdown instead of reading the whole email.

## Model choice

**Claude Haiku 4.5** (`claude-haiku-4-5-20251001`). Bodies are short to medium; this is a classification + light extraction task, not reasoning. Haiku's structured output via tool use is reliable and cheap. Expected cost: ~$0.001–0.003 per email.

Use prompt caching on the system prompt + loan roster so subsequent emails hit cache. Loan roster changes rarely; cache hit rate should be >90% in steady state.

## Structured output shape

One tool definition, one call, enforced with `tool_choice: {type: "tool", name: "extract_email"}` so the model has to emit the schema:

```json
{
  "original_sender": {
    "email": "molique@val-chris.com",
    "name": "Molique Someone",
    "confidence": "high"
  },
  "forwarded_by": ["ashirsc@gmail.com"],
  "candidate_loan_numbers": ["21172"],
  "subject_signal": "Edna Ranch LP #21172 Funding Ready",
  "summary": "one sentence",
  "classification": "loan_update" | "servicing" | "wire_instruction" | "marketing" | "other",
  "extraction_confidence": "high" | "medium" | "low"
}
```

Keep it flat and small — every field is something the UI or linking logic can consume. No free-form essays.

## Investigation findings

- Current pipeline: `src/routes/webhooks.rs:142` `fetch_and_store_email` handles body + attachments. The extraction call goes immediately after body storage, still inside this function, before it returns.
- Bodies are stored in object storage via `MediaStorage` keyed by `emails/{resend_email_id}/body.{html|txt}`. Extraction can read the bytes back or receive them directly from the fetch (preferred — avoids a storage round-trip).
- The loan roster for matching is `db::integrations::list_active_tmo_loans` — already fast (one query, < 100 rows typical).
- Auto-link target already exists: `db::emails::link_email_to_loan(pool, email_id, loan_account)`.
- The inbox list view is `templates/inbox.html` and knows `email.loan_account`; extending it with an "Auto-linked" indicator and surfaced hints is straightforward.

## Storage

Extend `intg.received_email` with the extraction payload. One migration, five columns:

```sql
ALTER TABLE intg.received_email
    ADD COLUMN extracted_at TEXT,            -- ISO timestamp, NULL until extracted
    ADD COLUMN extraction_json JSONB,        -- full model output for debugging
    ADD COLUMN extracted_original_sender TEXT,
    ADD COLUMN extracted_loan_number TEXT,   -- the candidate we matched on, if any
    ADD COLUMN auto_linked BOOLEAN DEFAULT FALSE;
```

`extraction_json` is the authoritative copy; the denormalized columns are for fast inbox filtering. If we end up wanting multiple candidate loan numbers, those live in `extraction_json` — we only denormalize the winner.

## Implementation

One PR. Keep the pieces small:

1. **Dependency + client.** Add `anthropic` Rust SDK (or a thin hand-rolled Claude client — the Anthropic SDK for Rust is stable enough to use; prefer it to reinventing). Prompt caching block on system + loan roster.
2. **`src/llm/` module.** One file, `email_extract.rs`. Exports `extract_from_email(body: &str, loans: &[TmoLoan]) -> anyhow::Result<ExtractionResult>`. Constructs the tool schema, makes the call, parses the tool-use response.
3. **Schema migration.** Add the five columns above via the existing `src/db/mod.rs` migration mechanism.
4. **DB helpers.** `mark_email_extracted(pool, id, result, auto_linked_loan: Option<&str>)`.
5. **Pipeline hook.** In `fetch_and_store_email`, after body is stored, call `extract_from_email`, persist the result, and if `extracted_loan_number` matches a loan in the roster *and* confidence is `high`, auto-link. Failures in the extraction step should log + continue; they must not fail the outer `fetch_and_store_email`.
6. **Retry path.** The existing `/inbox/{id}/retry` handler already resets state and re-runs the fetch — extraction rides on top of that for free. No separate retry.
7. **UI.** In `templates/inbox.html`:
   - New "From (original)" column when `extracted_original_sender` is present, showing the extracted identity rather than the forwarding address.
   - "Auto-linked" badge on rows where `auto_linked = true`.
   - When linking manually and `candidate_loan_numbers` is set, pre-select the first match in the loan dropdown.
   - When viewing a linked email, a small "re-extract" action for cases where we want to redo it (handler reuses the extraction path without clearing the link).
8. **Config.** New env vars: `ANTHROPIC_API_KEY`, `LLM_EXTRACTION_ENABLED` (default `false`). When the flag is off, the pipeline behaves exactly as today.

## Costs and guardrails

- At ~$0.002 per email and a few dozen emails per week, this is ~$0.50/month. Safe to leave on by default once proven.
- Prompt caching keeps the system prompt + loan roster warm; only the per-email body counts as uncached input.
- Set `max_tokens` low (~400). The tool schema is small and the output is bounded.
- Retry logic: treat 5xx from the API as retriable (2 attempts, short backoff). 4xx and content-filter responses are terminal — log the email id, don't retry, don't fail the pipeline.

## Failure modes and mitigations

| Failure | Mitigation |
|---|---|
| Model hallucinates a loan number not in roster | We only auto-link when the number matches an existing row — hallucinations become "suggested" at worst. |
| Model picks wrong loan for an email that mentions multiple | Store all `candidate_loan_numbers` in `extraction_json`. Auto-link only when exactly one candidate matches the roster. Multiple matches → manual link with pre-filtered dropdown. |
| Model times out / API is down | Email stays unlinked, no extraction. Retry path re-runs on user demand. |
| PII in request logs | Don't log the body. Log only email id + classification + confidence. |
| Forwarded PDF-only emails with nothing in body | Out of scope for v1 — attachments are not passed to the model yet. Follow-up if it matters.  |

## Validation

After landing:
- Process 10 historical emails through the retry button. Confirm `extracted_original_sender` is right for at least 8/10.
- Confirm auto-linking fires only when roster match is unambiguous.
- Confirm a wired-off flag (`LLM_EXTRACTION_ENABLED=false`) takes the whole pipeline back to today's behavior.

## Out of scope for v1

- Extracting from PDF/image attachments.
- Learning from user corrections (store corrections, feed back into the prompt).
- Multi-provider support (OpenAI, Gemini). Not needed.
- Auto-link across non-TMO integrations. Currently TMO is the only integration with loan numbers.
