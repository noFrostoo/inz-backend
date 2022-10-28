-- Add migration script here
create table "game_state"
(
    id              uuid primary key default gen_random_uuid(),
    round           BIGINT not null,
    user_states     jsonb not null,
    round_orders    jsonb not null
);