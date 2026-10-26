#!/usr/bin/env python3
# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""Cruza ventanas reales con /proc; resume soak sin convertir datos ausentes en éxito."""
import json
from pathlib import Path
import re
import sys

def log_error_count(text):
    count=0
    for line in text.splitlines():
        if re.search(r'\b(?:ERROR|PANIC|FATAL)\b|(?:^|\s)level=(?:error|fatal|panic)\b',line):
            count+=1
            continue
        start=line.find('{')
        if start>=0:
            try:
                if json.loads(line[start:]).get('level') in ('error','fatal','panic'):count+=1
            except (ValueError,AttributeError):pass
    return count



def read(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def rss(row):
    return int(re.search(r'VmRSS:\s+(\d+)',row['status'])[1])


def cpu(row):
    fields=row['stat'].rsplit(')',1)[1].split()
    return (int(fields[11])+int(fields[12]))/row['clock_ticks']


def resources(samples, began, ended):
    grouped={}
    for row in samples:
        grouped.setdefault((row['service'],row['pid']),[]).append(row)
    result={}
    for (service,pid),items in grouped.items():
        items.sort(key=lambda r:r['sample_unix'])
        inside=[r for r in items if began<=r['sample_unix']<=ended]
        intervals=[(a,b) for a,b in zip(items,items[1:]) if b['sample_unix']>=began and a['sample_unix']<=ended and b['sample_unix']>a['sample_unix']]
        if not inside or not intervals:continue
        result[service+'/'+str(pid)]=dict(name=service,process_name=items[0]['name'],pid=pid,
            cpu_percent=max(100*(cpu(b)-cpu(a))/(b['sample_unix']-a['sample_unix']) for a,b in intervals),
            rss_peak_kib=max(map(rss,inside)),fds=max(r['fds'] for r in inside),samples=len(inside))
    return result

def handshake(directory):
    metadata=[r for r in read(directory/'metadata.jsonl') if r['scenario']=='handshake']
    output=[];last=None
    for row in read(directory/'handshake.jsonl'):
        versions=[r for r in metadata if r['wall_unix']<=row['wall_unix']]
        if not versions:continue
        row['agent_sha256']=versions[-1]['binaries']['atenea-agent']['sha256']
        if row['kind']=='handshake-flood' and 'timings' in row:
            agents=[r for r in row['resources'].values() if r['name']=='atenea-agent' and 'cpu_s' in r]
            row['responder']=max(agents,key=lambda r:r['cpu_s'])
            last=row
        elif row['kind']=='handshake-retry-audit':
            assert last is not None and last['agent_sha256']==row['agent_sha256']
            row['responder']=last['responder']
            times=[(attempt['matching_response_after_mac2_s']-attempt['first_init_s'])*1000 for attempt in row['traces']['a']['candidates'] if attempt['matching_response_after_mac2_s'] is not None]
            row['handshake_observed']=bool(times)
            if times:row['handshake_worst_ms']=max(times)
        output.append(row)
    assert output,'No hay resultados handshake con metadatos'
    return output

def relay(directory):
    latest={}
    for name in ('relay.jsonl','1.jsonl','2.jsonl'):
        for row in read(directory/name):
            latest[(row['kind'],row['repetition'])]=row
    return list(latest.values())




def control(directory):
    samples=read(directory/'processes.jsonl')
    output=[]
    for path in sorted((directory/'control').glob('*.jsonl')):
        rows=read(path)
        runs=[r for r in rows if r.get('kind')=='run']
        if not runs:continue
        run=runs[-1]
        repetition=re.search(r'-r(\d+)-',path.name)
        for summary in (r for r in rows if r.get('kind')=='summary'):
            requests=[r for r in rows if r.get('kind')=='request' and r.get('phase')==summary['phase']]
            if not requests:continue
            began=min(r['at']-r['latency_ms']/1000 for r in requests)
            ended=max(r['at'] for r in requests)
            output.append(dict(kind='control-summary',mode=run['mode'],devices=run['devices'],
                concurrency=run['concurrency'],workload=run['workload'],quota=run['quota'],
                repetition=int(repetition[1]) if repetition else 0,**{k:v for k,v in summary.items() if k not in ('kind','at')},
                began_unix=began,ended_unix=ended,resources=resources(samples,began,ended)))
        for row in rows:
            if row.get('kind') in ('db-size','presence','websocket-summary'):
                values=dict(row,devices=run['devices'],repetition=int(repetition[1]) if repetition else 0)
                if row['kind']=='websocket-summary':
                    values.update(began_unix=run['at'],ended_unix=row['at'],resources=resources(samples,run['at'],row['at']))
                output.append(values)
    assert output,'No hay mediciones de control'
    return output


def steady_counters(traffic):
    marks = [r for r in traffic if r.get('kind') == 'soak-steady']
    if not marks:
        return dict(steady_since=None, steady_errors=1, steady_ok=False,
                    steady_samples=0, steady_counter_deltas={}, warmup_counter_deltas={})
    mark = marks[-1]
    rows = [r for r in traffic if r.get('kind') == 'soak-sample' and r['wall_unix'] >= mark['steady_since']]
    deltas, warmup, expected = {}, {}, {}
    errors = int(not rows)
    final = [r for r in traffic if r.get('kind') == 'soak-summary']
    duration = max(0, (final[-1]['wall_unix'] if final else rows[-1]['wall_unix'] if rows else mark['steady_since'])-mark['steady_since'])
    relay_limit = int(duration//300)+1
    known, relay_ready = {}, {}
    for agent in ('a', 'b'):
        baseline = mark['agent_states'][agent]['counters']
        before = mark['warmup_counters'][agent]
        warmup[agent] = {k:v-before.get(k, 0) for k,v in baseline.items()}
        deltas[agent], expected[agent] = {}, {}
        previous = baseline
        for row in rows:
            current = row['agent_counters'][agent]
            state = row.get('agent_states', {}).get(agent, {})
            peers = state.get('peers', [])
            relay_active = bool(peers) and all(p.get('path') == 'relay-udp' for p in peers)
            if not relay_active:
                errors += 1
            for key in current.keys() | previous.keys():
                delta = current.get(key, 0)-previous.get(key, 0)
                deltas[agent][key] = deltas[agent].get(key, 0)+delta
                # El directo bloqueado sigue sondeándose con backoff aun usando relay.
                allowed = key in ('path_lost', 'probe_failed') and relay_active and mark.get('expected_blocked_path') == 'direct'
                if allowed:
                    expected[agent][key] = expected[agent].get(key, 0)+delta
                if delta < 0 or (delta > 0 and re.search(r'error|bad|fail|lost|down|drop|invalid', key) and not allowed and key != 'relay_down'):
                    errors += abs(delta)
            previous = current
        down = deltas[agent].get('relay_down', 0)
        ready = previous.get('relay_ready', 0)
        if down > relay_limit or ready != previous.get('relay_down', 0)+1 or deltas[agent].get('relay_ready', 0) != down:
            errors += 1
        known[agent] = down
        relay_ready[agent] = ready
    return dict(steady_since=mark['steady_since'], steady_errors=errors, steady_ok=not errors,
                steady_samples=len(rows), steady_counter_deltas=deltas,
                warmup_counter_deltas=warmup, expected_blocked_counter_deltas=expected,
                known_finding='relay-token-cycle', known_finding_events=known,
                known_finding_limit_per_agent=relay_limit, steady_duration_s=duration,
                steady_relay_ready=relay_ready)


def soak(directory):
    samples=read(directory/'processes.jsonl')
    states=read(directory/'states.jsonl')
    traffic=read(directory/'soak.jsonl')
    final=[r for r in traffic if r.get('kind')=='soak-summary']
    assert final,'El soak no alcanzó su resumen final'
    began=min(r['sample_unix'] for r in samples)
    ended=max(r['sample_unix'] for r in samples)
    assert ended>began,'Faltan intervalos reales de muestreo'
    warmup=min(300,(ended-began)/4)
    failures=[]
    steady = steady_counters(traffic)
    if not steady['steady_ok']:
        failures.append('Errores/transiciones inesperadas en estado estable: '+str(steady['steady_errors']))
    summary = final[-1]
    duration = summary.get('requested_s', summary['elapsed_s'])
    start = summary['wall_unix'] - summary['elapsed_s']
    def enough(rows, timestamp):
        return all(sum(start+minute*60 <= row[timestamp] < start+(minute+1)*60
                       for row in rows) >= 5 for minute in range(int(duration//60)))
    if summary['elapsed_s'] < duration or summary.get('failures'):
        failures.append('Duración incompleta o iteraciones fallidas')
    agent_samples = [r for r in traffic if r.get('kind') == 'soak-sample']
    if not enough(agent_samples, 'wall_unix'):
        failures.append('Menos de 5 muestras/min de agentes')
    for service in ('control-plane', 'coordinator', 'relay', 'postgres'):
        if not enough([r for r in samples if r['service'] == service], 'sample_unix'):
            failures.append('Menos de 5 muestras/min de procesos: '+service)
    if not states:
        failures.append('Faltan estados de servicios')
    for name in {r['name'] for r in states}:
        if not enough([r for r in states if r['name'] == name], 'sample_unix'):
            failures.append('Menos de 5 muestras/min de estados: '+name)
    transfers = [r for r in traffic if r.get('kind') == 'soak-transfer']
    if any(not any(start+minute*60 <= r['wall_unix'] < start+(minute+1)*60
                       and r.get('bytes') == 16*1024*1024 and len(r.get('sha256', '')) == 64
                       for r in transfers) for minute in range(int(duration//60))):
        failures.append('Falta transferencia SHA-256 por minuto')
    for row in states:
        state=row['state']
        if row['restarts'] or state.get('OOMKilled') or state.get('Status')!='running' or state.get('Health',{}).get('Status','healthy')!='healthy':
            failures.append('Estado/reinicio/OOM: '+row['name'])
    grouped={}
    for row in samples:
        if row['sample_unix']>=began+warmup:
            grouped.setdefault((row['service'],row['pid']),[]).append(row)
    growth={}
    for (service,pid),items in grouped.items():
        items.sort(key=lambda r:r['sample_unix'])
        if len(items)<3:continue
        first,last=items[0],items[-1]
        seconds=last['sample_unix']-first['sample_unix']
        delta=rss(last)-rss(first)
        fd_delta=last['fds']-first['fds']
        growth[service+'/'+str(pid)]=dict(rss_growth_kib=delta,fds_growth=fd_delta,
            rss_kib_per_minute=delta*60/seconds,elapsed_s=seconds,samples=len(items))
        if delta>16384 or fd_delta>8:
            failures.append('Crecimiento tras calentamiento: '+service+'/'+str(pid))
    for service in ('control-plane','coordinator','relay','postgres'):
        if not any(k.startswith(service+'/') for k in growth):failures.append('Faltan muestras de '+service)
    log=(directory/'stack.log').read_text()
    log_errors=log_error_count(log)
    if log_errors:failures.append('Errores en logs del stack: '+str(log_errors))
    ping_path = directory/'soak-ping.log'
    ping = re.search(r'(\d+) packets transmitted, (\d+) received', ping_path.read_text() if ping_path.exists() else '')
    ping_loss = 100*(1-int(ping[2])/int(ping[1])) if ping and int(ping[1]) else 100.0
    if ping_loss > 1:
        failures.append('Pérdida de ping ausente o superior al 1 %')
    prometheus=[r for r in traffic if r.get('kind')=='soak-prometheus']
    if len(prometheus)<max(1,int(final[-1]['elapsed_s']//60)) or any(set(r.get('metrics',{}))!={'control-plane','coordinator','relay'} for r in prometheus):
        failures.append('Faltan scrapes Prometheus')
    return [dict(kind='soak-stack-summary',elapsed_s=ended-began,warmup_s=warmup,meets_local_20min=ended-began>=1200,process_resources=resources(samples,began,ended),
                 growth=growth,log_errors=log_errors,prometheus_samples=len(prometheus),ping_loss_percent=ping_loss,
                 **steady,failures=failures,ok=not failures)]


def main():
    if len(sys.argv)!=3 or sys.argv[1] not in ('handshake','relay','control','soak'):
        raise SystemExit('uso: summarize.py handshake|relay|control|soak DIRECTORIO_RESULTADOS')
    mode=sys.argv[1];directory=Path(sys.argv[2])
    rows={'handshake':handshake,'relay':relay,'control':control,'soak':soak}[mode](directory)
    output=directory/(mode+'-summary.jsonl')
    with output.open('w') as file:
        for row in rows:
            text=json.dumps(row);print(text,flush=True);file.write(text+'\n')
            if row.get('known_finding_events') and any(row['known_finding_events'].values()):
                print('CONOCIDO: relay-token-cycle; eventos por agente='+json.dumps(row['known_finding_events'])+
                      '; máximo explícito='+str(row['known_finding_limit_per_agent']))
    if any(row.get('ok') is False for row in rows):raise SystemExit(1)

if __name__=='__main__':main()
