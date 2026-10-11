-- A category's plan phases may be per paycheck instead of per month: the
-- month's plan is then the phase amount times the paydays in that month, so
-- a biweekly income gets three paychecks' worth in a three-payday month.
alter table categories add column plan_per_paycheck boolean not null default false;
