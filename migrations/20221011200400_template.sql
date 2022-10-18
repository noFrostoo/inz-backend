-- Add migration script here
create table "template"
(
    id              uuid primary key default gen_random_uuid(),
    name            text not null,
    max_players     SMALLINT     not null,
    owner_id        uuid  unique not null,
    settings        jsonb        not null,
    events          jsonb        not null
);
