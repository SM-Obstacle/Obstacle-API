INSERT INTO
    role (id, role_name, privileges)
VALUES
    (0, 'player', 1),
    (1, 'mod', 3),
    (2, 'admin', 255);

INSERT INTO
    api_status (status_id, status_name)
VALUES
    (1, 'normal'),
    (2, 'maintenance');

INSERT INTO
    api_status_history (status_id, status_history_date)
VALUES
    (1, SYSDATE());
