-- Add migration script here
create table "lobby"
(
    id              uuid primary key default gen_random_uuid(),
    name            text unique  not null,
    password        text,
    connect_code    text,
    code_use_times  SMALLINT,
    max_players     SMALLINT     not null,
    started         Boolean      not null,
    owner_id        uuid  unique not null,
    settings        jsonb        not null
);
