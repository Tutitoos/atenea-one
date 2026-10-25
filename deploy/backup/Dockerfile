FROM postgres:17@sha256:b8faeff7712663c2773a190181e1246e37e364b887611c04658c28b44e2408fe
RUN apt-get update && apt-get install -y --no-install-recommends pgbackrest age python3 && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/lib/pgbackrest /var/log/pgbackrest /run/pgbackrest && chown postgres:postgres /var/lib/pgbackrest /var/log/pgbackrest /run/pgbackrest
COPY deploy/backup/pgbackrest.conf /etc/pgbackrest/pgbackrest.conf
COPY deploy/backup/backup-entrypoint.sh /usr/local/bin/backup-entrypoint
COPY deploy/backup/schedule.sh /usr/local/bin/backup-schedule
COPY deploy/backup/verify.sh /usr/local/bin/backup-verify
RUN chmod 755 /usr/local/bin/backup-entrypoint /usr/local/bin/backup-schedule /usr/local/bin/backup-verify
ENTRYPOINT ["backup-entrypoint"]
CMD ["postgres", "-c", "archive_mode=on", "-c", "archive_command=pgbackrest --stanza=atenea archive-push %p", "-c", "archive_timeout=60s"]
