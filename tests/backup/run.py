#!/usr/bin/env python3
"""Dos desastres reproducibles. Nunca usa volúmenes de otros laboratorios."""
import datetime
import json
from pathlib import Path
import secrets
import statistics
import subprocess
import tempfile
import time
import uuid

ROOT = Path.cwd()
PREFIX = 'atenea-backup-' + uuid.uuid4().hex[:8]
IMAGE = 'atenea-backup-lab'
results = []

def run(*args, check=True):
    p = subprocess.run(['docker', *map(str,args)], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if check and p.returncode:
        raise RuntimeError('docker '+ ' '.join(map(str,args[:5]))+'\n'+p.stdout)
    return p.stdout.strip() if check else p

def wait(fn):
    for _ in range(180):
        try:
            return fn()
        except (RuntimeError, AssertionError):
            time.sleep(.25)
    return fn()

def cycle(n):
    tag = PREFIX + '-' + str(n)
    pg, lab, dex = tag+'-pg', tag+'-lab', tag+'-dex'
    repo, state, data = tag+'-repo', tag+'-state', tag+'-data'
    volumes = [repo,state,data]
    def sql(q):
        return run('exec',pg,'psql','-U','postgres','-d','atenea','-At','-v','ON_ERROR_STOP=1','-c',q)
    def back(*args, check=True):
        return run('exec','-u','postgres',pg,'backup-entrypoint','pgbackrest','--stanza=atenea',*args,check=check)
    def client(mode):
        return run('exec',lab,'python3','/src/tests/backup/client.py',mode)
    def start_pg():
        run('run','-d','--name',pg,'--network',tag,'--network-alias','postgres','--user','postgres','--read-only','--cap-drop','ALL','--tmpfs','/tmp:uid=999,gid=999,size=64m','--tmpfs','/var/run/postgresql:uid=999,gid=999','--tmpfs','/run/pgbackrest:uid=999,gid=999','-v',data+':/var/lib/postgresql/data','-v',repo+':/var/lib/pgbackrest','-v',secret+':/run/secrets/pgbackrest_cipher:ro','-e','POSTGRES_PASSWORD='+password,'-e','POSTGRES_DB=atenea',IMAGE)
        wait(lambda: sql('select 1'))
    def destroy():
        started=time.monotonic()
        run('rm','-f',lab)
        run('rm','-f',pg)
        run('volume','rm',data)
        run('volume','create',data)
        return started
    def restore(*options):
        run('run','--rm','-u','postgres','-v',data+':/var/lib/postgresql/data','-v',repo+':/var/lib/pgbackrest','-v',secret+':/run/secrets/pgbackrest_cipher:ro',IMAGE,'pgbackrest','--stanza=atenea',*options,'restore')
        start_pg()
    def start_lab(migrate=False):
        env = {
            'ATENEA_CP_DATABASE_URL':f'postgres://atenea_app:{password}@postgres/atenea',
            'ATENEA_CP_LISTEN':'0.0.0.0:8080','ATENEA_CP_PUBLIC_URL':'http://localhost:8080',
            'ATENEA_CP_OIDC_ISSUER':'http://dex:5556/dex','ATENEA_CP_OIDC_CLIENT_ID':'atenea-control-plane',
            'ATENEA_CP_OIDC_CLIENT_SECRET':'dev-only-secret-not-for-production',
            'ATENEA_CP_OIDC_REDIRECT_URL':'http://localhost:8080/auth/callback',
            'ATENEA_CP_ISSUER_KEY_FILE':'/lab/keys/issuer.key','ATENEA_CP_DEV_INSECURE_COOKIES':'true',
            'CP_URL':'http://localhost:8080','CP_JAR':'/lab/cookies', 'PATH':'/cache/target/release:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'}
        args=[]
        for k,v in env.items(): args += ['-e',k+'='+v]
        run('run','-d','--privileged','--name',lab,'--network',tag,'-v',str(ROOT)+':/src:ro','-v','atenea-lab-target:/cache:ro','-v',state+':/lab',*args,'atenea-lab','sleep','infinity')
        if migrate:
            run('exec','-e',f'ATENEA_CP_DATABASE_URL=postgres://postgres:{password}@postgres/atenea','-e','ATENEA_CP_APPLICATION_ROLE=atenea_app',lab,'atenea-control-plane','--migrate')
        run('exec','-d',lab,'bash','-c','exec atenea-control-plane > /lab/control-plane.log 2>&1')
    def key_exec(script):
        return run('run','--rm','--entrypoint','sh','-v',str(ROOT)+':/src:ro','-v',state+':/lab',IMAGE,'-ec',script)
    password=secrets.token_hex(16)
    with tempfile.TemporaryDirectory(prefix='atenea-backup-') as tmp:
        secret=str(Path(tmp)/'cipher')
        Path(secret).write_text(secrets.token_hex(32))
        Path(secret).chmod(0o444)  # Solo secreto efímero; necesita leerlo el uid postgres del contenedor.
        run('network','create',tag)
        try:
            for v in volumes: run('volume','create',v)
            start_pg()
            sql(f"CREATE ROLE atenea_app LOGIN PASSWORD '{password}'")
            run('run','-d','--name',dex,'--network',tag,'--network-alias','dex','-v',str(ROOT/'deploy/compose/dev/dex.yaml')+':/etc/dex/config.yaml:ro','ghcr.io/dexidp/dex:v2.41.1','dex','serve','/etc/dex/config.yaml')
            key_exec("umask 077; mkdir /lab/keys; for n in issuer.key relay-signing.hex application.key oidc-client-secret; do od -An -N32 -tx1 /dev/urandom | tr -d ' \\n' > /lab/keys/$n; done")
            start_lab(True)
            client('setup')
            net=run('exec',lab,'cat','/lab/network')
            dev=run('exec',lab,'cat','/lab/device-id')
            sql(f"INSERT INTO device_routes(id,network_id,device_id,prefix,status,snat) VALUES(gen_random_uuid(),'{net}','{dev}','10.88.0.0/24','approved',true)")
            sql("CREATE TABLE restore_markers(id integer PRIMARY KEY, committed_at timestamptz NOT NULL DEFAULT clock_timestamp())")
            back('stanza-create'); back('check'); back('--type=full','backup')
            sql("INSERT INTO restore_markers(id) VALUES(1)")
            back('--type=incr','backup')
            info=json.loads(back('--output=json','info'))
            label=info[0]['backup'][-1]['label']
            sql("INSERT INTO restore_markers(id) VALUES(2)")
            target=sql("SELECT clock_timestamp()")
            time.sleep(.1)
            sql(f"DELETE FROM device_routes WHERE network_id='{net}'")
            sql("INSERT INTO restore_markers(id) VALUES(3)")
            last_commit=sql('SELECT max(committed_at) FROM restore_markers')
            sql('SELECT pg_switch_wal()'); back('check')
            # Repositorio sin clave: el propio lector debe fallar.
            missing=run('exec','-u','postgres',pg,'pgbackrest','--stanza=atenea','info',check=False)
            assert missing.returncode != 0, missing.stdout
            run('exec','-u','postgres',pg,'backup-entrypoint','backup-verify')
            corrupt=run('exec','-u','postgres',pg,'backup-entrypoint','sh','-ec',
                'cp -a /var/lib/pgbackrest /tmp/corrupt-repo; '
                'f=$(find /tmp/corrupt-repo/backup/atenea -name PG_VERSION.gz | sort | head -n 1); '
                '[ -n "$f" ]; printf corrupt | dd of="$f" bs=1 seek=32 conv=notrunc 2>/dev/null; '
                'backup-verify --repo1-path=/tmp/corrupt-repo',check=False)
            assert corrupt.returncode != 0 and 'invalid checksum' in corrupt.stdout, corrupt.stdout
            # Corrupción en copia aislada, no en el repositorio válido.
            key_exec('age-keygen -o /lab/age-identity 2>/dev/null; age-keygen -o /lab/age-identity2 2>/dev/null; r1=$(age-keygen -y /lab/age-identity); r2=$(age-keygen -y /lab/age-identity2); sh /src/deploy/backup/keys-backup.sh /lab/keys /lab/keys.age "$r1" "$r2"; cp /lab/keys.age /lab/corrupt.age; printf corrupt | dd of=/lab/corrupt.age bs=1 seek=100 conv=notrunc 2>/dev/null; if sh /src/deploy/backup/keys-restore.sh /lab/corrupt.age /lab/age-identity /lab/bad 2>/dev/null; then exit 1; fi')
            key_exec('sh /src/deploy/backup/keys-restore.sh /lab/keys.age /lab/age-identity /lab/first-recipient; cmp /lab/keys/issuer.key /lab/first-recipient/issuer.key; rm -rf /lab/first-recipient')
            started=destroy(); restore('--type=default'); start_lab(); client('probe')
            latest_rto=time.monotonic()-started
            assert sql('select count(*) from restore_markers')=='3'
            assert sql('select count(*) from device_routes')=='0'
            assert sql('select max(committed_at) from restore_markers')==last_commit
            run('exec',pg,'pg_amcheck','-U','postgres','-d','atenea','--install-missing')
            # Un esquema futuro real (columna + migración 4) no se abre con el migrador 0001–0003.
            sql("ALTER TABLE devices ADD COLUMN restore_future boolean; INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES(4,'esquema futuro del simulacro',true,decode('00','hex'),0)")
            mismatch=run('exec','-e',f'ATENEA_CP_DATABASE_URL=postgres://postgres:{password}@postgres/atenea',lab,'atenea-control-plane','--migrate',check=False)
            assert mismatch.returncode != 0, mismatch.stdout
            started=destroy(); restore('--type=time','--target='+target,'--target-action=promote'); start_lab(); client('probe')
            pitr_rto=time.monotonic()-started
            assert sql('select count(*) from restore_markers')=='2'
            assert sql('select count(*) from device_routes')=='1'
            assert sql('select count(*) from networks')=='1'
            assert sql('select count(*) from devices')=='1'
            assert int(sql('select count(*) from audit_events'))>=4
            run('exec',pg,'pg_amcheck','-U','postgres','-d','atenea','--install-missing')
            # Constraints del esquema real, no solo conteos.
            invalid=run('exec',pg,'psql','-U','postgres','-d','atenea','-v','ON_ERROR_STOP=1','-c',f"INSERT INTO device_routes(id,network_id,device_id,prefix,status,snat) VALUES(gen_random_uuid(),'{net}','{dev}','10.90.0.0/24','approved',NULL)",check=False)
            assert invalid.returncode != 0
            key_start=time.monotonic()
            key_exec('rm -rf /lab/keys; sh /src/deploy/backup/keys-restore.sh /lab/keys.age /lab/age-identity2 /lab/keys')
            client('key')
            key_rto=time.monotonic()-key_start
            # Eliminar realmente el archivo WAL; archive-copy conserva solo el necesario para la copia.
            started=destroy()
            run('run','--rm','--entrypoint','sh','-v',repo+':/var/lib/pgbackrest',IMAGE,'-ec','rm -rf /var/lib/pgbackrest/archive/atenea/17-*')
            restore('--set='+label,'--type=immediate','--target-action=promote'); start_lab(); client('probe')
            assert sql('select count(*) from restore_markers')=='1'
            lost_seconds=float(sql(f"SELECT EXTRACT(EPOCH FROM '{last_commit}'::timestamptz - max(committed_at)) FROM restore_markers"))
            row={'ciclo':n,'latest_rto_s':round(latest_rto,3),'latest_rpo_tx':0,'latest_rpo_s':0,'pitr_rto_s':round(pitr_rto,3),'pitr_excluidas_tx':2,'sin_wal_perdidas_tx':3,'sin_wal_rpo_s':round(lost_seconds,3),'clave_rto_s':round(key_rto,3)}
            print(json.dumps(row),flush=True)
            print('OK: completa + incremental + WAL; pérdida volumen; latest; PITR antes DELETE; healthz + configuración aceptada por agente; pg_amcheck; constraints; pgBackRest verify detecta corrupción; age corrupto; sin clave; esquema distinto; archivo WAL eliminado; emisor restaurado y atenea issue aceptado',flush=True)
            results.append(row)
        finally:
            for c in [lab,pg,dex]: run('rm','-f',c,check=False)
            for v in volumes: run('volume','rm',v,check=False)
            run('network','rm',tag,check=False)

run('build','-t',IMAGE,'-f','deploy/backup/Dockerfile','.')
print('Fecha UTC:',datetime.datetime.now(datetime.timezone.utc).isoformat(),flush=True)
print('Commit:',subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),flush=True)
print('Docker:',run('version','--format','{{.Server.Version}} / {{.Server.Arch}}'),flush=True)
print(run('run','--rm','--entrypoint','sh',IMAGE,'-c','postgres --version; pgbackrest version; age --version'),flush=True)
for n in (1,2): cycle(n)
for key in ('latest_rto_s','pitr_rto_s','clave_rto_s'):
    values=[r[key] for r in results]
    print(f'{key}: media={statistics.mean(values):.3f}s peor={max(values):.3f}s',flush=True)
print('BACKUP: DOS CICLOS OK',flush=True)
