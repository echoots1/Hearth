-- An income category paid on a different rhythm from the household (a side
-- job paid monthly next to a biweekly paycheck) carries its own pay schedule,
-- in the same JSON shape as the `pay_schedule` setting. Null = the household's.
alter table categories add column pay_schedule jsonb;
