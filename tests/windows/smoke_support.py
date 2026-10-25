"""Ayudas sin dependencias nativas; también se prueban fuera de Windows."""
import json
import ntpath
import os
import re
import subprocess
import sys


def powershell_environment(source):
    env = {key: value for key, value in source.items()
           if key.upper() not in {'PSMODULEPATH', 'PSHOME', 'POWERSHELL_DISTRIBUTION_CHANNEL'}}
    root = next(value for key, value in source.items() if key.upper() == 'SYSTEMROOT')
    # Solo módulos nativos de Windows PowerShell 5.1, nunca los heredados de pwsh 7.
    env['PSModulePath'] = ntpath.join(root, 'System32', 'WindowsPowerShell', 'v1.0', 'Modules')
    return env


def powershell_executable(source):
    root = next(value for key, value in source.items() if key.upper() == 'SYSTEMROOT')
    return ntpath.join(root, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe')


def call(*args, timeout=30, env=None):
    try:
        result = subprocess.run([str(arg) for arg in args], text=True, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, timeout=timeout, env=env)
    except subprocess.TimeoutExpired as error:
        if error.output:
            output = error.output.decode(errors='replace') if isinstance(error.output, bytes) else error.output
            print(output, file=sys.stderr, flush=True)
        raise
    if result.returncode:
        if result.stdout:
            print(result.stdout, file=sys.stderr, flush=True)
        raise subprocess.CalledProcessError(result.returncode, result.args, output=result.stdout)
    return result.stdout.strip()


def ps(script, timeout=30):
    return call(powershell_executable(os.environ), '-NoProfile', '-NonInteractive', '-Command',
                "$ErrorActionPreference='Stop'; " + script, timeout=timeout,
                env=powershell_environment(os.environ))


def _invalid(label, text, detail):
    return ValueError(f'{label}: {detail}; salida original: {text[:1000]}')


def _json(text, label):
    try:
        return json.loads(text) if text.strip() else None
    except ValueError as error:
        raise _invalid(label, text, f'JSON no válido ({error.msg})') from None


def _collection(value, label, text):
    count = None
    if isinstance(value, dict) and ('value' in value or 'Value' in value):
        keys = set(value)
        payload = 'value' if 'value' in value else 'Value'
        if not keys <= {payload, 'Count'}:
            raise _invalid(label, text, 'envoltorio de colección no válido')
        if 'Count' in value:
            count = value['Count']
            if type(count) is not int or count < 0:
                raise _invalid(label, text, 'Count debe ser un entero no negativo')
        value = value[payload]
    values = [] if value is None else value if isinstance(value, list) else [value]
    if count is not None and count != len(values):
        raise _invalid(label, text, 'Count no coincide con la colección')
    return values


def _strings(value, label, text):
    values = _collection(value, label, text)
    if any(not isinstance(item, str) for item in values):
        raise _invalid(label, text, 'se esperan cadenas proyectadas')
    return values


def _record(value, fields, label, text):
    if not isinstance(value, dict):
        raise _invalid(label, text, 'se espera un registro proyectado')
    result = {}
    for field, kind in fields.items():
        item = value.get(field)
        if kind is list:
            if field not in value:
                raise _invalid(label, text, f'falta {field}')
            result[field] = _strings(item, label, text)
        elif type(item) is not kind:
            raise _invalid(label, text, f'{field} debe ser {kind.__name__}')
        else:
            result[field] = item
    return result


def parse_nrpt(text):
    fields = {'Name': str, 'Namespace': list, 'NameServers': list, 'Comment': str}
    return [_record(rule, fields, 'NRPT', text)
            for rule in _collection(_json(text, 'NRPT'), 'NRPT', text)]


def parse_snapshot(text):
    label = 'Snapshot'
    value = _json(text, label)
    if not isinstance(value, dict) or not {'routes', 'dns'} <= value.keys():
        raise _invalid(label, text, 'se esperan routes y dns proyectados')
    schemas = {
        'routes': {'DestinationPrefix': str, 'NextHop': str,
                   'InterfaceIndex': int, 'RouteMetric': int},
        'dns': {'InterfaceIndex': int, 'AddressFamily': int, 'ServerAddresses': list},
    }
    return {name: [_record(item, fields, label, text)
                   for item in _collection(value[name], label, text)]
            for name, fields in schemas.items()}


def parse_service(text):
    label = 'Servicio'
    values = _collection(_json(text, label), label, text)
    if len(values) != 1:
        raise _invalid(label, text, 'se espera exactamente un servicio')
    return _record(values[0], {field: str for field in
                              ('State', 'StartMode', 'StartName', 'PathName')}, label, text)


def parse_strings(text):
    return _strings(_json(text, 'Cadenas'), 'Cadenas', text)


def parse_count(text):
    value = _json(text, 'Cantidad')
    if type(value) is not int or value < 0:
        raise _invalid('Cantidad', text, 'se espera un entero no negativo')
    return value


def parse_private_principals(text):
    label = 'SDDL'
    # Solo la DACL protegida determina los permisos; no las ACE de la SACL.
    sections = re.findall(r'D:([^:]*?)(?=S:|$)', text.strip())
    if len(sections) != 1:
        raise _invalid(label, text, 'se espera una DACL protegida D:P')
    dacl = sections[0]
    flags, separator, entries = dacl.partition('(')
    if not re.fullmatch(r'P(?:AI|AR)*', flags):
        raise _invalid(label, text, 'se espera una DACL protegida D:P')
    entries = separator + entries
    aces = re.findall(r'\(([^()]*)\)', entries)
    if ''.join(f'({ace})' for ace in aces) != entries:
        raise _invalid(label, text, 'ACE no válida')
    principals = set()
    for ace in aces:
        fields = ace.split(';')
        if len(fields) != 6 or fields[0] not in {'A', 'D'} or not fields[5]:
            raise _invalid(label, text, 'ACE no válida')
        if fields[0] == 'A':
            principals.add(fields[5])
    return principals


def ask(pipe, command):
    # El proceso auxiliar permite interrumpir también open/read síncronos bloqueados.
    return json.loads(call(sys.executable, __file__, pipe, command, timeout=5))

def pipe_exchange(pipe, command):
    try:
        with open(pipe, 'r+b', buffering=0) as stream:
            stream.write((command + '\n').encode())
            return stream.readline(1024 * 1024).decode()
    except OSError as error:
        # Antes de readiness no existe instancia o todas están ocupadas; no es un fallo.
        if isinstance(error, FileNotFoundError) or getattr(error, 'winerror', None) == 231:
            return None
        raise



if __name__ == '__main__':
    reply = pipe_exchange(sys.argv[1], sys.argv[2])
    if reply is None:
        sys.exit(3)  # El sondeo conserva su plazo; no imprime un traceback transitorio.
    print(reply)
