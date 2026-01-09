create or replace function unstyled(in login text) returns text collate 'utf8mb4_unicode_ci'
begin
    return replace(regexp_replace(login, '\\$(([wWnNoOiItTsSgGzZ<>]|([lL](\\[.*\\])?))|[aAbBcCdDeEfF0-9]([aAbBcCdDeEfF0-9]{2})?)', ''),
                   '$$', '$');
end;
