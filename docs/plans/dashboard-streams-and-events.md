# Dashboard: streams + raw events

The global dashboard (`/`) is currently a welcome card with a single "Open TMO workspace" button (`templates/index.html`). Now that the stream-events model is the authoritative layer under all payment/balance data, the dashboard should be a plain window into *what streams exist* and *what raw events have been recorded recently* — the kind of view you glance at first thing in the morning to see what changed.

## Motivation

- The TMO integration overview already answers "how's my portfolio" for that one integration.
- The generic dashboard should answer *"what does the app know about, and what's moved lately?"* — integration-agnostic, data-layer focused.
- Keeps the stream-event model visible as we migrate more things onto it; if something stops writing events, we'll see the gap here.

## Target content

Two sections, top to bottom on desktop, stacked on mobile:

### Streams

Table (or card grid — pick whatever reads cleanly at 390px) of every stream from `db::streams::list_streams`, showing:

- Stream name + type badge (income / expense / transfer)
- Default account
- Count of events (total and last-30d)
- Latest event date
- Link into the per-stream view on `/streams` or `/canvas` (whichever is the canonical single-stream surface)

### Raw events

A flat list of the 50 most recent `stream_event` rows across all streams, newest first, showing:

- Stream name
- Event status (`confirmed` / `received` / `scheduled` / etc.)
- Amount (signed by stream type — credits positive, debits negative)
- Date (`actual_date` if present, else `expected_date`, else `scheduled_date`)
- Source type (`tmo_history`, `manual`, whatever else exists)
- Source id (small, muted — useful for debugging; not the primary read)

No filters in v1. If this list gets long, add a "load more" later.

## Investigation findings

- `src/db/streams.rs:435` — `pub async fn list_streams(pool) -> Vec<StreamConfigView>` exists. `StreamConfigView` probably needs extending (or a new view type) to include count + latest date; check before adding a duplicate.
- `src/db/events.rs` exists and already has query helpers used elsewhere (e.g., `get_recent_payments`). Likely one of them is close to what we want — check if a generic `list_recent_events(pool, limit)` is already there, or add one alongside.
- The dashboard template (`templates/index.html`) has almost nothing in it; no risk of disruption.
- Display formatting must use the shared Askama filters per `CLAUDE.md` (money, date, datetime).

## Implementation

One PR:

1. **DB helpers.** Add whatever's missing:
   - `db::streams::list_streams_with_counts(pool)` — one query joining `stream` to an aggregate over `stream_event` giving `event_count`, `events_last_30d`, `latest_event_date`. If `StreamConfigView` already covers enough, extend it rather than making a new struct.
   - `db::events::list_recent_events(pool, limit: i64)` — newest 50 across all streams, joined to stream name. Returns a view struct with the fields listed above.
2. **Template struct.** Extend `IndexTemplate` with the two vecs:
   ```rust
   pub struct IndexTemplate {
       pub title: String,
       pub streams: Vec<StreamWithCountsView>,
       pub recent_events: Vec<StreamEventView>,
   }
   ```
3. **Route handler.** `index` in `src/routes/pages.rs` fetches both in parallel with `tokio::join!`.
4. **Template.** Rewrite `templates/index.html` with the two sections. Keep it plain: two cards with DaisyUI `table` inside, matching the visual weight of existing cards elsewhere (no hero, no stats, no splash).
5. **Empty states.** If no streams: "No streams configured yet — add one from Streams." If no events: "No stream events yet." Both pointed at the existing `/streams` and `/canvas` surfaces.

## Mobile considerations

Per `CLAUDE.md`, the dashboard is one of the phone-primary surfaces. Tables should collapse or scroll gracefully:

- Streams: at < 640px, switch to a card list (one stream per card) rather than a cramped table.
- Raw events: keep as a tight table with horizontal scroll; the identifying column (stream name + amount) stays leftmost so the important information is visible without scrolling.

## Validation

- `/` on desktop: both sections visible, data looks right, counts match a hand-checked stream.
- `/` on phone: no horizontal scroll on the streams section; events table scrolls horizontally only.
- With 0 streams and 0 events (fresh DB): empty states render.
- Filters in `src/filters.rs` are used for all dates and amounts — no raw ISO strings or `{:.2}` formatting in the template.

## Out of scope

- Filters, date range pickers, per-stream drill-in from the dashboard.
- Anything that isn't `stream_event`-shaped (TMO-specific widgets stay on the TMO integration overview).
- Editing or confirming events from this page — it's a read-only glance surface.
