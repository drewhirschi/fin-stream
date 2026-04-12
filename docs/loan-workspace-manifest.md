# Loan Workspace Manifest

## The Problem

The current loan detail view is useful, but it is still too thin for actual investment decisions.

When a new trust deed shows up, the real workflow is broader:

1. An email thread comes in asking whether I want to participate
2. I decide how much to put in
3. I look up the property on Redfin or Zillow
4. I gather images, notes, and documents
5. I want all of that context attached to the loan forever, not scattered across inbox, browser tabs, and files

Today the app only gives me TMO fields and payment history. It does not act like a working file for the loan.

## The Goal

Turn each loan into a **workspace** instead of just a record.

For every loan, I want one place where I can see:

- TMO loan facts and payment history
- Property photos
- Quick links out to Redfin and Zillow
- Linked email threads
- Linked or uploaded documents
- Personal notes about the deal

The page should help with two moments:

1. **Underwriting / deciding whether to invest**
2. **Monitoring an existing loan over time**

## Product Shape

The loan detail page becomes a **loan workspace** with five blocks.

### 1. Property block

- Full address
- Property type, occupancy, lien position, LTV, appraised value
- Redfin link
- Zillow link
- Optional APN / county link later

### 2. Gallery block

- Primary hero image
- Small gallery of property photos
- Source badge for each image: uploaded, Redfin, Zillow, manual URL
- Ability to refresh images later without losing existing metadata

### 3. Communications block

- Links to relevant email threads
- Optional labels like `initial pitch`, `servicer update`, `borrower issue`, `closing docs`
- Timeline-ish ordering by most recent linked communication

At first, "email thread" can just mean a stored URL plus subject/snippet metadata. We do not need full inbox sync on day one.

### 4. Documents block

- Deed of trust
- Appraisal
- Insurance
- Borrower financials
- Servicer statements
- Misc supporting files

This block needs object storage plus relational metadata.

### 5. Notes block

- Freeform notes
- Short structured fields for decision context:
  - target contribution
  - actual contribution
  - why I liked it
  - risks / concerns
  - follow-ups

## Principles

### Single source of working context

If I am thinking about a loan, I should not need to check five places before I remember what it is.

### Add context without breaking sync

TMO remains the source of truth for servicing and payment data. The workspace adds my context on top of that instead of trying to replace TMO.

### Manual-first, automation-friendly

The feature should be immediately useful with manual links and uploads, then get smarter with import helpers later.

### Preserve originals

If we pull in photos or files, keep the original asset and store enough metadata to know where it came from.

## Recommended First Cut

The smartest v1 is not "scrape everything from Zillow and Redfin."

The smartest v1 is:

1. Add external property links
2. Add manual photo uploads
3. Add document uploads
4. Add linked email threads as URLs
5. Add notes and underwriting fields

That gives a lot of value quickly, avoids brittle scraping work, and creates the storage model we need before we automate imports.

## External Property Sources

Redfin and Zillow are valuable mainly for:

- photos
- listing context
- rough property recall at a glance

### Recommendation

Support these in phases:

#### Phase 1

- Store `redfin_url` and `zillow_url`
- Show link buttons on the loan page
- Allow manual image upload or pasted image URL

#### Phase 2

- Add an import action that can ingest a set of image URLs or downloaded images into our own bucket
- Mark every image with source metadata

#### Phase 3

- Explore partial automation around property enrichment, but only after we are confident in storage, attribution, and failure handling

This keeps the architecture clean even if the upstream sites change.

## Storage

We do need bucket storage for this feature to feel real.

### What goes in the bucket

- property photos
- PDFs
- image derivatives later
- other uploaded attachments

### What stays in Postgres

- file metadata
- image metadata
- source URLs
- labels / notes
- document type
- ordering
- who uploaded it
- timestamps
- which loan each asset belongs to

### Recommendation

Use an S3-compatible bucket interface from the start.

That keeps the app portable and gives flexibility to use:

- AWS S3
- Cloudflare R2
- Backblaze B2 (S3-compatible)
- local MinIO for development

The app should never depend on public bucket URLs as canonical identifiers. Store stable object keys in the database and generate access URLs at runtime.

## Proposed Data Model

These tables are additive and sit beside `intg.tmo_import_loan`.

### `loan_workspace`

One row per loan account for user-owned context.

- `loan_account` TEXT PK/FK
- `redfin_url` TEXT
- `zillow_url` TEXT
- `primary_image_asset_id` BIGINT NULL
- `decision_status` TEXT
- `target_contribution` DOUBLE PRECISION
- `actual_contribution` DOUBLE PRECISION
- `notes` TEXT
- `created_at`
- `updated_at`

### `loan_asset`

Shared asset record for anything stored in the bucket.

- `id` BIGSERIAL PK
- `loan_account` TEXT FK
- `asset_kind` TEXT
  - `image`
  - `document`
- `storage_key` TEXT
- `content_type` TEXT
- `byte_size` BIGINT
- `original_filename` TEXT
- `title` TEXT
- `source_type` TEXT
  - `upload`
  - `redfin`
  - `zillow`
  - `manual_url`
- `source_url` TEXT
- `checksum_sha256` TEXT
- `sort_order` INTEGER
- `created_at`
- `updated_at`

### `loan_email_link`

Lightweight email thread reference.

- `id` BIGSERIAL PK
- `loan_account` TEXT FK
- `thread_url` TEXT NOT NULL
- `subject` TEXT
- `sender` TEXT
- `snippet` TEXT
- `thread_date` TEXT
- `label` TEXT
- `created_at`
- `updated_at`

### `loan_note`

Optional later, if notes should become structured/history-preserving instead of one big text field.

- `id` BIGSERIAL PK
- `loan_account` TEXT FK
- `body` TEXT
- `note_type` TEXT
- `created_at`

## UX Sketch

On `/integrations/tmo/loans/:loan_account`, keep the existing summary and payment timeline, then add a second layer below the current detail cards:

1. A property actions row with `Open in Redfin`, `Open in Zillow`, `Add photo`, `Add document`, `Link email`
2. A two-column content area:
   - left: image gallery + notes
   - right: documents + communications

The page should still work if a loan has zero attachments. Empty states are important here.

## Upload Flow

### Images

1. User selects one or more files
2. App validates type and size
3. App stores object in bucket
4. App creates `loan_asset` row
5. Page re-renders gallery

### Documents

Same flow, but document-specific metadata can include:

- document type
- optional effective date
- optional note

### Email links

1. User pastes a thread URL
2. User optionally adds subject and label
3. App stores link metadata
4. Loan page shows a clickable list

## Security and Privacy

This feature can hold sensitive material, so it should be designed with restraint.

- Bucket objects should be private by default
- Serve assets through signed URLs or authenticated handlers
- Avoid hotlinking third-party images directly in the UI long-term
- Track source URLs for provenance
- Set conservative file size limits
- Only allow expected MIME types

## Implementation Plan

### Phase 1: Workspace metadata

- Add `loan_workspace`
- Add Redfin and Zillow URLs
- Add notes and contribution fields
- Show new workspace panel on loan detail page

### Phase 2: Bucket-backed uploads

- Choose S3-compatible storage
- Add `loan_asset`
- Add upload endpoint for images and documents
- Render gallery and document shelf

### Phase 3: Communication links

- Add `loan_email_link`
- Add paste-a-link flow
- Show linked threads on the loan page

### Phase 4: Import helpers

- Support importing remote images into our bucket
- Add source attribution and refresh behavior

### Phase 5: Richer underwriting workflow

- Decision states like `new`, `reviewing`, `committed`, `passed`, `funded`
- Notes history
- Reminder/follow-up hooks

## Success Criteria

This feature is successful if, for any loan, I can answer:

- What property is this again?
- Where can I quickly view it externally?
- What did I decide to invest?
- What docs and images do I have on it?
- Which email thread had the original conversation?

without leaving the app to reconstruct the story from memory.

## Near-Term Recommendation

Build this as a **loan workspace** feature, not as a fragile "scrape Redfin/Zillow" feature.

Start with:

- external links
- bucket-backed uploads
- email thread links
- notes

Then layer import helpers on top once the storage and metadata model exists.
