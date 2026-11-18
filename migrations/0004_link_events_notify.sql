-- Live updates ride on Postgres NOTIFY instead of an in-process channel, so
-- every replica (and any direct SQL writer) sees every change. NOTIFY is
-- delivered on commit only, so a rolled-back write never reaches a browser.
create function notify_link_event() returns trigger as $$
begin
    -- A click only bumps the counter. It is not an edit: re-rendering every
    -- open list on each click would reorder a "most clicked" list under the
    -- viewer, so an update that changed nothing but `clicks` stays silent.
    if tg_op = 'UPDATE' and to_jsonb(new) - 'clicks' = to_jsonb(old) - 'clicks' then
        return null;
    end if;

    perform pg_notify(
        'link_events',
        json_build_object(
            'op', case tg_op
                when 'INSERT' then 'created'
                when 'UPDATE' then 'updated'
                else 'deleted'
            end,
            'id', case tg_op
                when 'DELETE' then old.id
                else new.id
            end
        )::text
    );
    return null;
end;
$$ language plpgsql;

create trigger links_notify
after insert or update or delete on links
for each row execute function notify_link_event();
