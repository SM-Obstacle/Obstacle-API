create or replace function rm_mp_style(in login text) returns text
begin
    return replace(regexp_replace(login, '\\$([wWnNoOiItTsSgGzZ<>]|[aAbBcCdDeEfF0-9]([aAbBcCdDeEfF0-9]{2})?)', ''),
                   '$$', '$') collate 'utf8mb4_unicode_ci';
end;
