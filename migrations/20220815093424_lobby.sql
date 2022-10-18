-- Add migration script here
create table "lobby"
(
    id              uuid primary key default gen_random_uuid(),
    name            text unique  not null,
    password        text,
    public          Boolean      not null,
    connect_code    text unique,
    code_use_times  SMALLINT     not null,
    max_players     SMALLINT     not null,
    started         Boolean      not null,
    owner_id        uuid  unique not null,
    settings        jsonb        not null,
    events          jsonb        not null
);
