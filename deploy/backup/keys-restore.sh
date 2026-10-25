#!/bin/sh
set -eu
[ "$#" -eq 3 ] || { echo 'Uso: keys-restore.sh COPIA IDENTITY DESTINO_NUEVO' >&2; exit 2; }
umask 077
[ ! -e "$3" ] || { echo 'Destino existente: no se sobrescribe' >&2; exit 1; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
age -d -i "$2" -o "$tmp/keys.tar" "$1"
python3 - "$tmp/keys.tar" "$3" <<'PY'
import os, pathlib, sys, tarfile
expected = {'issuer.key', 'relay-signing.hex', 'application.key', 'oidc-client-secret'}
with tarfile.open(sys.argv[1]) as archive:
    members = archive.getmembers()
    names = {m.name for m in members}
    required = expected - {'application.key'}
    if len(members) != len(names) or not required <= names <= expected or any(not m.isfile() for m in members):
        raise SystemExit('Archivo de claves inesperado')
    dest = pathlib.Path(sys.argv[2])
    dest.mkdir(mode=0o700)
    for member in members:
        with os.fdopen(os.open(dest / member.name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'wb') as out:
            out.write(archive.extractfile(member).read())
PY
