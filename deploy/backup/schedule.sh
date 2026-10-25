#!/bin/sh
set -eu
until pg_isready -h /var/run/postgresql -U postgres; do sleep 5; done
pgbackrest --stanza=atenea stanza-create
pgbackrest --stanza=atenea check
# Completa al arrancar; cada cambio de día UTC completa, incrementales cada hora.
last_day=''
last_hour=''
while :; do
  day=$(date -u +%F)
  hour=$(date -u +%F-%H)
  if [ "$day" != "$last_day" ]; then
    pgbackrest --stanza=atenea --type=full backup
    backup-verify
    last_day=$day
    last_hour=$hour
  elif [ "$hour" != "$last_hour" ]; then
    pgbackrest --stanza=atenea --type=incr backup
    last_hour=$hour
  fi
  pgbackrest --stanza=atenea check
  sleep 60
done
