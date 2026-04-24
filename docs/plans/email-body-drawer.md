# Move inline email body into the slide-out drawer

The inline HTML body on the loan detail page's Mail tab (and the inbox email detail page) breaks scroll flow — long forwarded threads stretch the page out of shape and bury everything below them. Render the body in the existing `_slide_panel.html` drawer behind a **View email** button instead.

## Current state

- `templates/_slide_panel.html` already exists: right-anchored Alpine-driven slide-out, triggered by `openEmail(url)` / closed by `close()`. It loads HTML content from a URL into `#panel-content`.
- `/inbox/{email_id}/panel` route (`inbox_email_panel` in `src/routes/pages.rs`) already returns the right HTML fragment (`EmailPanelPartial`, `templates/_email_panel.html`).
- The pieces are wired in the inbox list via attachments/viewer already. The body itself is still rendered inline in two places.

So the infrastructure exists. This is a UI refactor, not new plumbing.

## Places the body renders inline today

1. `templates/integration_loan_detail.html` — Mail tab lists linked emails; each row expands into the body. Remove the inline expansion; replace with a **View email** button that calls `openEmail('/inbox/{{ email.id }}/panel')`.
2. `templates/inbox_email_detail.html` — full-page email detail. The metadata card stays; the body section becomes a **View email** button that opens the same drawer. Attachments stay listed on the page.

Everything other than the body HTML itself stays in place — from/to, subject, received_at, status, loan link, attachments. The drawer is for *reading* the message; the page is for *acting on* it (link, unlink, retry, open attachments).

## Implementation

One PR, template-only changes:

1. **`templates/integration_loan_detail.html`** — in the Mail tab list of `loan_emails`, each row renders sender/subject/date + a `<button @click="openEmail('/inbox/{{ email.id }}/panel')">View email</button>`. Drop the inline body block. Ensure the page includes `_slide_panel.html` at the bottom of its `{% block content %}` inside an Alpine `x-data` scope that provides `open`, `url`, `openEmail()`, `close()` (copy the existing pattern used elsewhere).
2. **`templates/inbox_email_detail.html`** — same treatment; replace the inline body card with a button and include the slide panel.
3. No route changes. `/inbox/{id}/panel` already serves the fragment.
4. No Rust changes.

## Validation

- On loan detail → Mail tab: clicking View email slides the panel in from the right with the rendered body; pressing Esc or the backdrop closes it.
- On inbox email detail page: same button, same drawer, same content.
- Scroll on both pages no longer stretches vertically based on email body length.
- Mobile (< 640px): drawer goes full-width per existing `_slide_panel.html` styling.

## Out of scope

- Any changes to attachment rendering (already in a viewer).
- Any route or model changes.
- Redesigning the metadata card.
