# Loan workspace mail tab + slide-out document viewer

Tie the email inbox to each loan and build a document viewer so the owner can click into emails and their attachments from inside the loan workspace without breaking flow.

## Problems

1. **From a loan, there's no way to see linked emails.** The link exists in the data (`loan_account` column on `intg.received_email`), but the loan detail page doesn't surface it.
2. **Attachments are download-only.** No in-app viewer; every click launches a new tab / downloads a file.
3. **Inbox page heading has an off-looking dark background** — the `.vault` wrapper's background bleeds into the heading area.

## Investigation findings

### Loan detail page
- `templates/integration_loan_detail.html` has no tab structure. Sections are stacked:
  - Lines 28-49 — loan metadata
  - Lines 51-110 — loan details
  - Lines 111-148 — payment timeline
  - Lines 151-318 — loan workspace
- `templates/integration_detail.html:47-80` already has a DaisyUI/Alpine tab pattern we can copy: `<div role="tablist" class="tabs tabs-boxed">` + `<section x-show="tab === '…'">`.

### Email ↔ loan linkage — already exists
- `intg.received_email.loan_account` VARCHAR column.
- `src/db/emails.rs`:
  - `link_email_to_loan()` at line 232.
  - `unlink_email()` at line 334.
  - `list_emails_for_loan()` at line 150 — selects `WHERE loan_account = $1`.
  - `list_unlinked_emails()` at line 121.
- Linkage is manual via an existing dropdown on `/inbox/{email_id}` that POSTs to `/inbox/{email_id}/link`.

### Inbox page heading
- `templates/inbox.html:27-35` — heading lives inside `<div class="vault"><div class="flex flex-col gap-8"><section>…`.
- `.vault` styling in `static/vault.css:8-12` applies a dark surface color. Heading has no explicit `bg-base-100` wrapper, so the dark surface bleeds through.

### Attachments
- Stored per email in `intg.received_email_attachment` (fields: `filename`, `content_type`, `size_bytes`, `s3_key`, `processing_state`).
- Served via `/media/emails/{key}` — see `src/media_storage.rs:66` (`MediaStorage::store`) and the media route.
- `templates/inbox_email_detail.html:95-142` shows attachments in a table, each row has a `Download` link (`:131`). No preview.
- `media_storage.rs:258` `guess_content_type()` only handles jpg/png/webp. Other MIMEs pass through from the attachment record.

### Drawer/modal primitives already available
- DaisyUI drawer — `templates/base.html:19-44` (sidebar). Alpine-controlled.
- DaisyUI modal — `templates/inbox.html:7-18` (error modal).
- Tab pattern — `templates/integration_detail.html:47-80`.
- **No slide-out side panel pattern exists**; build one with a right-anchored DaisyUI drawer or a transform + Alpine `x-transition`.

## Fix

### Fix 1 — inbox heading background

- Wrap the heading block in a `bg-base-100 rounded-2xl p-4 sm:p-6` container inside the outer `.vault` section.
- If the issue is actually that the heading should pick up a different token (e.g., the vault surface should be *lighter* than base, not darker), adjust `static/vault.css` instead. Prefer the template-level wrapper fix since it's scoped.
- Verify on both light and dark DaisyUI themes.

### Fix 2 — tabbed loan workspace with Mail tab

Add tabs to `templates/integration_loan_detail.html` using the `integration_detail.html:47-80` pattern. Tabs:

1. **Overview** — existing loan metadata + loan details (current lines 28-110).
2. **Payments** — existing payment timeline (current lines 111-148).
3. **Workspace** — existing loan workspace card (current lines 151-318).
4. **Mail** — new. Shows a list of emails linked to this loan.

Implementation:

- `src/routes/pages.rs` — extend the loan detail handler to also query `db::emails::list_emails_for_loan(loan_account)` and pass into the template.
- New template partial `templates/_loan_mail_list.html` rendering each email with sender, subject, received_at, attachment count. Each row links to the email detail, which opens in the slide-out (see Fix 3).
- Empty state: "No emails linked to this loan yet — link one from the inbox."
- Keep all four tabs mounted in the DOM with `x-show` (the existing pattern) so HTMX interactions inside the Mail tab don't break when switching tabs.

### Fix 3 — slide-out email + document viewer

A right-anchored panel that slides in over the loan detail page and shows either an email (iframe of the HTML body + attachment list) or a single attachment (image preview, PDF embed, or file-info fallback).

- Markup: a `<div>` fixed to `right-0 top-0 h-full w-full sm:w-[min(640px,90vw)]` with an overlay backdrop. Alpine `x-data="{ open: false, url: '' }"` and `x-show` with `x-transition:enter-start="translate-x-full"` / `enter-end="translate-x-0"` for the slide.
- Trigger: clicking an email row in the Mail tab sets `url = /inbox/{id}/panel` and `open = true`. The panel does `hx-get="{url}"` into its content div.
- New route: `GET /inbox/{id}/panel` returns a compact partial — subject + from + body iframe + attachments list. Clicking an attachment row sets the panel's body to the document viewer partial for that attachment.
- Document viewer partial `templates/_doc_viewer.html`:
  - If `content_type` starts with `image/` → render `<img>` with the media URL.
  - If `content_type` is `application/pdf` → render an `<iframe>` pointing at the media URL.
  - Otherwise → show filename, MIME type, size, and a large "Download" button.
- Keyboard: `Escape` closes the panel (Alpine `@keydown.escape.window`).
- Mobile: the panel goes full-width (`w-full`) and slides from the right.

### Fix 4 — reuse from `/inbox/{id}` detail page

- Extract the email body + attachments markup from `templates/inbox_email_detail.html` into a partial that both the full-page view and the slide-out panel include. Keeps the two in sync when we add PDF preview.
- Add the same document viewer (Fix 3) to the full inbox detail page too — clicking an attachment there opens the slide-out over the inbox (or swaps in place if the screen is narrow).

## Acceptance

- `/inbox` heading no longer has the dark-bleed look on either theme.
- `/integrations/tmo/loans/{id}` has four tabs: Overview, Payments, Workspace, Mail. Switching tabs does not re-fetch the page.
- The Mail tab lists all emails linked to that loan; clicking one slides a panel in from the right.
- Inside the panel, clicking an image attachment previews it inline; clicking a PDF embeds it; other types offer download.
- `Escape` and backdrop click both close the panel; underlying loan workspace state is preserved (no scroll jump).
- Works on 390px mobile (panel full-width) and 1440px desktop (panel at 640px).

## Out of scope

- Fuzzy auto-linking of emails to loans based on subject/body (manual linking stays).
- Rich document OCR or full-text search over attachments.
- Document annotation / highlighting.
- Generalizing the viewer to non-email documents (e.g., TMO document imports) — possible follow-up, but not here.
