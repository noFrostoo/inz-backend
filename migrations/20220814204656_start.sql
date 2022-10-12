-- Add migration script here
create table "user"
(
    id       uuid primary key default gen_random_uuid(),
    username      text unique not null,
    password      text        not null,
    game_id       uuid,
    role          text not null
);