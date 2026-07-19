export interface Account { id: number; name: string; kind: string; balance?: number | null; balance_as_of_date?: string | null; is_primary: boolean; notes?: string | null }
export interface Schedule { id: number; label?: string | null; amount: number; frequency: string; day_of_month?: number | null; start_date: string; end_date?: string | null; is_active: boolean }
export interface Stream { id: number; name: string; stream_type: string; kind: string; direction: string; amount_certainty: string; description?: string | null; default_account_name?: string | null; schedule_amount?: number | null; schedule_frequency?: string | null; due_day?: number | null; schedules: Schedule[] }
export interface View { id: number; name: string; description?: string | null; is_default: boolean; stream_ids?: number[]; streams?: Array<{id: number; name: string}> }
export interface FinanceData { accounts: Account[]; streams: Stream[]; views: View[]; canvas_streams: Array<{id: number; name: string; kind: string}> }

export interface Connection { id: number; slug: string; name: string; provider: string; status: string; sync_cadence: string; last_synced_at?: string | null; last_error?: string | null; next_scheduled_at?: string | null; record_count: number; normalized_count: number; pending_count: number }
export interface LoanList { loan_account: string; borrower_name?: string | null; property_address?: string | null; property_city?: string | null; property_state?: string | null; featured_image_url?: string | null; property_type?: string | null; percent_owned?: number | null; note_rate?: number | null; principal_balance?: number | null; regular_payment?: number | null; maturity_date?: string | null; next_payment_date?: string | null; interest_paid_to?: string | null; is_delinquent?: number | null }
export interface Payment { id: number; loan_account: string; borrower_name: string; property_name: string; check_number?: string | null; check_date: string; amount: number; service_fee: number; interest: number; principal: number; charges: number; late_charges: number; other: number; processing_state: string; raw_payload?: string | null }
export interface Overview { snapshot_date: string; portfolio_value?: number | null; portfolio_yield?: number | null; portfolio_count?: number | null; ytd_interest?: number | null; trust_balance?: number | null; outstanding_checks?: number | null }
export interface IntegrationData { connection: Connection; loans: LoanList[]; payments: Payment[]; normalized_payments: Array<Record<string, unknown>>; overviews: Overview[]; captured_records: Array<Record<string, unknown>>; sync_logs: SyncRun[]; control: { mode: string; scheduler_enabled: boolean; updated_at: string } }
export interface IntegrationsData { connections: Connection[] }

export interface LoanDetail extends LoanList { id: number; connection_id: number; property_zip?: string | null; property_description?: string | null; occupancy?: string | null; appraised_value?: number | null; ltv?: number | null; interest_rate?: number | null; original_balance?: number | null; loan_balance?: number | null; payment_frequency?: string | null; billed_through?: string | null; term_left_months?: number | null }
export interface Workspace { redfin_url: string; redfin_link?: string | null; zillow_url: string; zillow_link?: string | null; decision_status: string; target_contribution?: number | null; actual_contribution?: number | null; notes: string; updated_at?: string | null }
export interface Photo { id: number; provider: string; caption?: string | null; source_url?: string | null; image_url?: string | null; is_featured: boolean }
export interface Email { id: number; from_address: string; to_addresses: string; subject?: string | null; received_at: string; body_s3_key?: string | null; body_content_type?: string | null; loan_account?: string | null; processing_state: string; error_message?: string | null }
export interface Attachment { id: number; filename: string; content_type: string; size_bytes?: number | null; s3_key?: string | null; processing_state: string }
export interface LoanData { connection: Connection; loan: LoanDetail; workspace: Workspace; photos: Photo[]; payments: Payment[]; emails: Email[] }
export interface InboxData { emails: Array<{ email: Email; attachment_count: number }>; loans: LoanList[]; show_linked: boolean }
export interface EmailData { email: Email; attachments: Attachment[]; recipients: string[]; loans: LoanList[] }
import type { SyncRun } from "./generated/model/syncRun";

export type { SyncRun } from "./generated/model/syncRun";
