FROM postgres:18@sha256:edb0c08dc33befd0262ab3b5cf1c4f3f8cfd8b09e88c05b45adee29d6761ddb6
RUN apt-get update && apt-get install -y --no-install-recommends pgbackrest age python3 && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/lib/pgbackrest /var/log/pgbackrest /run/pgbackrest && chown postgres:postgres /var/lib/pgbackrest /var/log/pgbackrest /run/pgbackrest
COPY deploy/backup/pgbackrest.conf /etc/pgbackrest/pgbackrest.conf
COPY deploy/backup/backup-entrypoint.sh /usr/local/bin/backup-entrypoint
COPY deploy/backup/schedule.sh /usr/local/bin/backup-schedule
COPY deploy/backup/verify.sh /usr/local/bin/backup-verify
RUN chmod 755 /usr/local/bin/backup-entrypoint /usr/local/bin/backup-schedule /usr/local/bin/backup-verify
ENTRYPOINT ["backup-entrypoint"]
CMD ["postgres", "-c", "archive_mode=on", "-c", "archive_command=pgbackrest --stanza=atenea archive-push %p", "-c", "archive_timeout=60s"]
