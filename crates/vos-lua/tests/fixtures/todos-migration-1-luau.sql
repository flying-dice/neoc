    create table lists (
      id integer primary key,
      name text not null,
      color text not null
    );
    create table todos (
      id integer primary key,
      list_id integer not null,
      title text not null,
      notes text not null default '',
      due text not null default '',
      flagged integer not null default 0,
      done integer not null default 0,
      completed_at text
    );
    create index todos_by_list on todos(list_id, id);
    create index todos_by_completion on todos(done, completed_at desc, id desc);
    insert into lists(name, color) values ('Reminders', 'blue');
  