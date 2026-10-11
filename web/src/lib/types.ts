/** Row and view shapes as the API returns them. Money is cents; dates ISO. */

export type Autonomy = "backseat" | "driver";
export type AutonomyArea = "categorizing_notes" | "plans_bills" | "debts_goals";
export type AssistantMode = "pull" | "api";

export type AssistantSettings = {
  mode: AssistantMode;
  autonomy: Autonomy;
  categorizing_notes: Autonomy | null;
  plans_bills: Autonomy | null;
  debts_goals: Autonomy | null;
  chat_model: string;
  pass_model: string;
  monthly_cap_cents: number;
  pass_time: string;
};

export type Today = {
  today: string;
  month: string;
  day_of_month: number;
  days_in_month: number;
  days_left: number;
  next_payday: string;
  household: string;
  timezone: string;
  currency: string;
  cash_floor_cents: number;
  assistant: AssistantSettings;
};

export type Me = {
  id: string;
  email: string;
  display_name: string;
  role: "owner" | "member";
  household: string;
  timezone: string;
};

export type AuthStatus = { needs_setup: boolean; signed_in: boolean };

export type Account = {
  id: string;
  name: string;
  type: "checking" | "savings" | "credit_card" | "virtual_card" | "cash" | "loan" | "investment" | "other";
  source: "plaid" | "privacy" | "manual";
  institution_label: string | null;
  mask: string | null;
  is_active: boolean;
  include_in_cash: boolean;
  reported_balance_cents: number | null;
  reported_balance_as_of: string | null;
  bank_balance_cents: number | null;
  bank_balance_as_of: string | null;
};

export type NextPhase = { label: string | null; plan_cents: number; starts_month: string };

export type CategoryView = {
  id: string;
  name: string;
  sign: "income" | "spend";
  note: string;
  is_everyday: boolean;
  plan_cents: number;
  actual_cents: number;
  activity_cents: number;
  remaining_cents: number;
  used_bps: number;
  bills_total_cents: number;
  bills_over_plan_cents: number;
  next_phase: NextPhase | null;
};

export type GroupView = {
  id: string;
  name: string;
  plan_cents: number;
  activity_cents: number;
  remaining_cents: number;
  categories: CategoryView[];
};

export type MonthView = {
  month: string;
  groups: GroupView[];
  totals: {
    planned_income_cents: number;
    planned_spend_cents: number;
    actual_income_cents: number;
    actual_spend_cents: number;
    left_to_assign_cents: number;
    spend_remaining_cents: number;
    net_cents: number;
  };
  uncategorized_cents: number;
};

export type Category = {
  id: string;
  group_id: string;
  name: string;
  sign: "income" | "spend";
  note: string;
  is_everyday: boolean;
  sort_order: number;
  is_archived: boolean;
};

export type Group = { id: string; name: string; sort_order: number };

export type CategoryPlan = {
  id: string;
  category_id: string;
  starts_month: string;
  amount_cents: number;
  label: string | null;
  note: string | null;
};

export type Transaction = {
  id: string;
  account_id: string;
  date: string;
  description: string;
  original_description: string;
  amount_cents: number;
  category_id: string | null;
  source: "plaid" | "privacy" | "csv" | "manual";
  pending: boolean;
  note: string;
  planned_item_id: string | null;
  recurring_bill_id: string | null;
  bill_drift_cents: number | null;
  rule_id: string | null;
  review_status: "none" | "pending" | "resolved";
  created_at: string;
};

export type ReviewItem = Transaction & { account_name?: string; suggestions?: Suggestion[] };
export type Suggestion = { category_id: string; name: string; reason: string };

export type CategoryDetail = {
  category: Category;
  view: CategoryView;
  plans: CategoryPlan[];
  bills: Bill[];
  transactions: Transaction[];
};

export type Page<T> = { items: T[]; next_cursor: string | null };

export type Bill = {
  id: string;
  name: string;
  amount_cents: number;
  cadence_months: number;
  next_due: string;
  category_id: string | null;
  category_name?: string | null;
  account_id: string | null;
  match_pattern: string | null;
  note: string;
  status: "active" | "paused" | "canceled";
  last_matched_at: string | null;
  monthly_equivalent_cents?: number;
  splits?: { category_id: string; amount_cents: number }[];
};

export type PlannedItem = {
  id: string;
  date: string;
  item: string;
  amount_cents: number;
  category_id: string | null;
  status: "planned" | "done" | "dropped";
  decided_by: string;
  note: string;
  matched_transaction_id: string | null;
};

export type LowPoint = {
  today: string;
  next_payday: string;
  days_until_payday: number;
  estimate_cents: number;
  bills: { bill_id: string; name: string; due_date: string; amount_cents: number }[];
  bills_due_cents: number;
  everyday: { category_id: string; name: string; amount_cents: number; pace_cents_per_day: number }[];
  everyday_expected_cents: number;
  planned: { planned_item_id: string; item: string; date: string; amount_cents: number }[];
  planned_items_cents: number;
  low_point_cents: number;
};

export type BalanceOverview = {
  accounts: {
    account_id: string;
    name: string;
    include_in_cash: boolean;
    reported_balance_cents: number | null;
    reported_balance_as_of: string | null;
    reported_age_days: number | null;
    bank_balance_cents: number | null;
    bank_balance_as_of: string | null;
    since_cents: number;
    since_count: number;
    estimate_cents: number;
  }[];
  household_estimate_cents: number;
  cash_floor_cents: number;
  headroom_cents: number;
  low_point: LowPoint | null;
};

export type SinkingOverview = {
  id: string;
  name: string;
  funded_balance_cents: number;
  balance_as_of: string | null;
  view: {
    month: string;
    funded_balance_cents: number;
    should_have_saved_cents: number;
    shortfall_cents: number;
    surplus_cents: number;
    monthly_set_aside_cents: number;
    annualized_cents: number;
    items: {
      id: string;
      name: string;
      amount_cents: number;
      cycle_months: number;
      due_month: string;
      months_elapsed: number;
      monthly_set_aside_cents: number;
      should_have_saved_cents: number;
      overdue: boolean;
    }[];
  };
};

export type Debt = {
  id: string;
  name: string;
  kind: "credit_card" | "loan" | "mortgage" | "other";
  balance_cents: number;
  balance_as_of: string;
  apr_bps: number;
  min_payment_cents: number;
  planned_payment_cents: number | null;
  due_day: number;
  account_id: string | null;
  custom_order: number | null;
  is_closed: boolean;
  note: string;
  interest_per_day_cents: number;
  monthly_interest_cents: number;
};

export type DebtProjection = {
  strategy: "avalanche" | "snowball" | "custom";
  start_month: string;
  extra_payment_cents: number;
  total_monthly_payment_cents: number;
  total_interest_cents: number;
  debt_free_month: string | null;
  debts: {
    debt_id: string;
    name: string;
    apr_bps: number;
    starting_balance_cents: number;
    monthly_payment_cents: number;
    interest_per_day_cents: number;
    months_to_payoff: number;
    payoff_month: string | null;
    total_interest_cents: number;
    total_paid_cents: number;
  }[];
};

export type DebtComparison = {
  current: "avalanche" | "snowball" | "custom";
  avalanche: DebtProjection;
  snowball: DebtProjection;
  custom: DebtProjection;
};

export type Goal = {
  id: string;
  name: string;
  target_cents: number;
  target_date: string | null;
  account_id: string | null;
  category_id: string | null;
  saved_cents: number;
  saved_as_of: string | null;
  status: "active" | "reached" | "paused" | "dropped";
  note: string;
  projection: {
    monthly_cents: number;
    remaining_cents: number;
    progress_bps: number;
    months_to_reach: number | null;
    reach_month: string | null;
    needed_monthly_cents: number | null;
    on_track: boolean | null;
  } | null;
};

export type Alert = {
  id: string;
  type: string;
  severity: "info" | "warn" | "critical";
  status: "open" | "notified" | "ack" | "done";
  title: string;
  detail: string;
  amount_cents: number | null;
  transaction_id: string | null;
  category_id: string | null;
  bill_id: string | null;
  debt_id: string | null;
  goal_id: string | null;
  snoozed_until: string | null;
  nudge_count: number;
  resolution: string | null;
  first_seen_at: string;
  created_at: string;
  updated_at: string;
};

export type Proposal = { tool: string; args: Record<string, unknown>; summary: string };

export type Question = {
  id: string;
  kind: "ask" | "proposal";
  status: "open" | "answered" | "approved" | "declined" | "dismissed";
  asked_by: string;
  question: string;
  options: { key: string; label: string }[] | null;
  answer: string | null;
  answered_by: string | null;
  answered_at: string | null;
  proposal: Proposal | null;
  area: AutonomyArea | null;
  transaction_id: string | null;
  bill_id: string | null;
  applied_at: string | null;
  created_at: string;
};

export type AssistantAction = {
  id: string;
  area: AutonomyArea;
  tool: string;
  args: Record<string, unknown>;
  summary: string;
  entity: string | null;
  entity_id: string | null;
  before: unknown;
  after: unknown;
  undo: Proposal | null;
  undone_at: string | null;
  undone_by: string | null;
  created_at: string;
};

export type Rule = {
  id: string;
  name: string;
  pattern: string;
  match_type: "contains" | "regex";
  priority: number;
  account_id: string | null;
  amount_min_cents: number | null;
  amount_max_cents: number | null;
  action: "categorize" | "ignore";
  category_id: string | null;
  set_note: string | null;
  is_active: boolean;
  hit_count: number;
  last_hit_at: string | null;
};

export type ApiKey = {
  id: string;
  name: string;
  key_prefix: string;
  scope: "read" | "read_write";
  created_at: string;
  last_used_at: string | null;
  revoked_at: string | null;
};

export type UserInfo = { id: string; email: string; display_name: string; role: "owner" | "member" };

export type Settings = {
  pay_schedule:
    | { kind: "weekly"; anchor: string }
    | { kind: "biweekly"; anchor: string }
    | { kind: "semimonthly"; days: [number, number] }
    | { kind: "monthly"; day: number };
  cash_floor_cents: number;
  large_charge_threshold_cents: number;
  pace_alert: { ratio_pct: number; from_day: number };
  stale_balance_days: number;
  notifications: { quiet_hours: { start: string; end: string }; quiet_hours_override_critical: boolean };
  nag: { first_after_minutes: number; every_minutes: number; max_nudges: number };
  debt_strategy: { kind: "avalanche" | "snowball" | "custom"; extra_payment_cents: number; custom_order: string[] };
  assistant: AssistantSettings;
  [k: string]: unknown;
};

export type AssistantStatus = {
  assistant: AssistantSettings;
  has_api_key: boolean;
  encryption_configured: boolean;
  month_calls: number;
  month_spent_cents: number;
};

export type AuditRow = {
  id: number;
  at: string;
  actor_kind: "user" | "api_key" | "assistant" | "job" | "system";
  actor_id: string | null;
  action: string;
  entity: string;
  entity_id: string;
  before: unknown;
  after: unknown;
};

export type Gated<T> =
  | { outcome: "applied"; result: T; action: AssistantAction | null }
  | { outcome: "proposed"; question: Question };
