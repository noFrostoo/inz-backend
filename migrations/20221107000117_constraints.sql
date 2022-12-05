-- Add migration script here

alter table "user"
   ADD CONSTRAINT fk_game
      FOREIGN KEY(game_id) 
	  REFERENCES lobby(id);


alter table "lobby"
   ADD CONSTRAINT fk_owner
      FOREIGN KEY(owner_id) 
	  REFERENCES "user"(id);


alter table "game_state"
   ADD CONSTRAINT fk_game_state
      FOREIGN KEY(game_id) 
	  REFERENCES lobby(id);


alter table "template"
   ADD CONSTRAINT fk_owner_template
      FOREIGN KEY(owner_id) 
	  REFERENCES "user"(id);