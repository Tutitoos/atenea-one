"""Ayudas sin dependencias nativas; también se prueban fuera de Windows."""
import json
import ntpath
import os
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
