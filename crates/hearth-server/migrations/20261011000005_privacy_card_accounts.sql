-- Earlier versions made every Privacy.com card an account; 0004 deactivated
-- them and left the next sync to move their rows to the funding account and
-- remove them. Do that here where the funding account is unambiguous (the
-- household's only active checking account), so the card accounts are gone
-- as soon as the app starts. A household with several checking accounts keeps
-- them, inactive, until the owner picks the funding account under
-- More → Integrations and the sync does the same.
create temp table privacy_rehome as
  select a.id as card_id, a.name as card_name, f.id as funding_id
    from accounts a
    join (select household_id, (array_agg(id))[1] as id from accounts
           where is_active and type = 'checking' and source <> 'privacy'
           group by household_id having count(*) = 1) f
      on f.household_id = a.household_id
   where a.source = 'privacy';

update transactions t set account_id = r.funding_id, updated_at = now()
  from privacy_rehome r where t.account_id = r.card_id;
update card_events e set account_id = r.funding_id, card_label = coalesce(e.card_label, r.card_name)
  from privacy_rehome r where e.account_id = r.card_id;
update rules x set account_id = r.funding_id, updated_at = now()
  from privacy_rehome r where x.account_id = r.card_id;
update import_batches x set account_id = r.funding_id
  from privacy_rehome r where x.account_id = r.card_id;
update recurring_bills x set account_id = r.funding_id, updated_at = now()
  from privacy_rehome r where x.account_id = r.card_id;
update alerts x set account_id = r.funding_id, updated_at = now()
  from privacy_rehome r where x.account_id = r.card_id;

-- Balance snapshots of a card are meaningless and go with it (cascade).
delete from accounts a using privacy_rehome r where a.id = r.card_id;

drop table privacy_rehome;
