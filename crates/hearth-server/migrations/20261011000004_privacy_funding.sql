-- Privacy.com cards are no longer accounts. Card charges are transactions on
-- the funding account (the bank account Privacy draws from), pending until
-- settled, and the bank's later "PwP*…" line for each one is paired as its
-- shadow. Declines remember which card they were on.
--
-- Card accounts created by earlier versions stop showing up at once; the next
-- Privacy sync moves their transactions and declines to the funding account
-- and removes them.
alter table card_events
  add column card_token text,
  add column card_label text;

update accounts set is_active = false, updated_at = now()
 where source = 'privacy' and is_active;
