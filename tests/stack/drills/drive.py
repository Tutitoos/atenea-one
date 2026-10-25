"""El runner no recibe el socket Docker: solo este proceso administra su proyecto."""
import json
import subprocess
import sys
import time

runner, *compose = sys.argv[1:]
allowed = {'relay', 'coordinator', 'control-plane', 'postgres', 'haproxy'}
runner_ip = subprocess.check_output(['docker', 'inspect', '--format',
    '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}', runner], text=True).strip()
with subprocess.Popen(['docker', 'exec', '-i', runner, 'python3', '-u', '/src/tests/stack/api.py'],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1) as worker:
    for line in worker.stdout:
        if not line.startswith('ACTION '):
            print(line, end='', flush=True)
            continue
        action = json.loads(line.removeprefix('ACTION '))
        assert action['verb'] in ('stop', 'start')
        assert action['services'] and set(action['services']) <= allowed
        # Fotografía previa a cada inyección; no se interpreta como permiso de limpieza.
        if action['verb'] == 'stop':
            subprocess.run(['docker', 'ps', '-a'], check=True)
            subprocess.run(['docker', 'network', 'ls'], check=True)
        began = time.time()
        result = subprocess.run([*compose, action['verb'], *action['services']], check=False)
        if result.returncode == 0 and action['verb'] == 'start':
            # Docker recrea el namespace: la ruta a las NAT emuladas no es configuración productiva.
            for service in set(action['services']) & {'coordinator'}:
                container = subprocess.check_output([*compose, 'ps', '-q', service], text=True).strip()
                subprocess.run(['docker', 'run', '--pull=never', '--rm', '--privileged',
                    '--network', 'container:'+container, 'atenea-panel-lab',
                    'ip', 'route', 'replace', '198.51.100.0/24', 'via', runner_ip], check=True)
        reply = {'ok': result.returncode == 0, 'began': began, 'ended': time.time()}
        print('EVENT '+json.dumps(dict(action, **reply)), flush=True)
        worker.stdin.write(json.dumps(reply)+'\n')
        worker.stdin.flush()
    sys.exit(worker.wait())
