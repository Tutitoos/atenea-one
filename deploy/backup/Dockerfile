FROM postgres:17@sha256:31ba9c94a03ada9988f2bb493a22a75eef3a185884533e5cdbcd498ea835b08f
RUN apt-get update && apt-get install -y --no-install-recommends pgbackrest age python3 && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/lib/pgbackrest /var/log/pgbackrest /run/pgbackrest && chown postgres:postgres /var/lib/pgbackrest /var/log/pgbackrest /run/pgbackrest
COPY deploy/backup/pgbackrest.conf /etc/pgbackrest/pgbackrest.conf
COPY deploy/backup/backup-entrypoint.sh /usr/local/bin/backup-entrypoint
COPY deploy/backup/schedule.sh /usr/local/bin/backup-schedule
COPY deploy/backup/verify.sh /usr/local/bin/backup-verify
RUN chmod 755 /usr/local/bin/backup-entrypoint /usr/local/bin/backup-schedule /usr/local/bin/backup-verify
ENTRYPOINT ["backup-entrypoint"]
CMD ["postgres", "-c", "archive_mode=on", "-c", "archive_command=pgbackrest --stanza=atenea archive-push %p", "-c", "archive_timeout=60s"]
