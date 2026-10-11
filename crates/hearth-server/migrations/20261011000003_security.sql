-- Per-install salt for deriving the at-rest encryption key from
-- APP_ENCRYPTION_KEY (argon2id). One row, created on first start.
create table crypto_meta (
  id          smallint primary key default 1 check (id = 1),
  kdf_salt    bytea not null check (octet_length(kdf_salt) >= 16),
  created_at  timestamptz not null default now()
);
