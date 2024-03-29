-- Add migration script here
create table "game_state"
(
    id              uuid primary key default gen_random_uuid(),
    round           BIGINT not null,
    user_states     jsonb not null,
    round_orders    jsonb not null,
    send_orders     jsonb not null,
    players_classes jsonb not null,
    flow            jsonb not null,
    demand          BIGINT not null,
    supply          BIGINT not null,
    game_id         uuid   not null
);