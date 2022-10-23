
INSERT INTO "user" (id, username, password, role)
VALUES
    -- username: "alice"; password: "alice
    ('51b374f1-93ae-4c5c-89dd-611bda8412ce', 'alice',
     '$argon2id$v=19$m=4096,t=3,p=1$2dT4Yay43+XevGqR+xFSow$hb2/4PMw0RFg2AH/5zHPEXl9oDDM5+qsbcU2qfR2GE8', 'admin'),
    -- username: "bob"; password: "bob"
    ('c994b839-84f4-4509-ad49-59119133d6f5', 'bob',
     '$argon2id$v=19$m=4096,t=3,p=1$/6XXIkFwpibpEe4sq8Qs4w$UG575rlLgt0THTBSsFrynPm/hpy7F1xzJ4DdpZ47mYc', 'user'),
    ('bbb4b839-84f4-4519-ad49-59119133d6f5', 'bob2',
     '$argon2id$v=19$m=4096,t=3,p=1$/6XXIkFwpibpEe4sq8Qs4w$UG575rlLgt0THTBSsFrynPm/hpy7F1xzJ4DdpZ47mYc', 'gameadmin'),
    ('c994b839-84f4-4509-ad49-59429133d6f5', 'bob3',
     '$argon2id$v=19$m=4096,t=3,p=1$/6XXIkFwpibpEe4sq8Qs4w$UG575rlLgt0THTBSsFrynPm/hpy7F1xzJ4DdpZ47mYc', 'user');
