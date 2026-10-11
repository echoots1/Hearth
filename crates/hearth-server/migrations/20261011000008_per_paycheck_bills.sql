-- A recurring bill can be due on every payday of the household pay schedule
-- (giving, a per-paycheck transfer) instead of every N months; its
-- `cadence_months` is then ignored. A planned one-off marked done without a
-- matching charge must say so: `paid_outside_feed` (cash, another account).
alter table recurring_bills add column per_paycheck boolean not null default false;
alter table planned_items add column paid_outside_feed boolean not null default false;
