-- Add migration script here
create table "game"
(
    id              uuid primary key default gen_random_uuid(),
    lobby_id        uuid unique not null,
    data            jsonb not null
);

create table "game_state"
(
    id              uuid primary key default gen_random_uuid(),
    game_id         uuid unique not null,
    state           jsonb not null
);