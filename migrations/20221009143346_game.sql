-- Add migration script here
create table "game"
(
    id              uuid primary key default gen_random_uuid(),
    lobby           uuid unique not null,
    data            jsonb
);