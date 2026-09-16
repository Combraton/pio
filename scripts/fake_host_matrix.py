#!/usr/bin/env python3
"""Process-level evidence for the experimental fake host, not Protocol conformance."""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import socket
import sqlite3
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'


def poll(action, predicate, seconds=5):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            last = action()
            if predicate(last):
                return last
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(.02)
    raise AssertionError(f'bounded observation timed out; last={last}')


def rpc(root, body):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(2)
        stream.connect(str(root / 'daemon.sock'))
        stream.sendall(json.dumps(body).encode() + b'\n')
        with stream.makefile('rb') as file:
            return json.loads(file.readline(65537))


def process_identity(pid):
    result = subprocess.run([str(BINARY), 'fake', 'identity', str(pid)], capture_output=True, text=True)
    return json.loads(result.stdout) if result.returncode == 0 else None


def terminate(identity):
    if identity and process_identity(identity['pid']) == identity:
        try:
            os.kill(identity['pid'], signal.SIGKILL)
        except ProcessLookupError:
            pass  # The identity-matched process exited between observation and kill.


def records(path):
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def single_launch(before, after, markers):
    identities = [row['identity'] for row in markers]
    if len(identities) != 1:
        return {'outcome': 'fail', 'property': 'single_launch_identity_count',
                'reason': f'expected 1 child start; observed {len(identities)}', 'identities': identities}
    if before != after or identities[0] != before:
        return {'outcome': 'fail', 'property': 'single_launch_identity_count',
                'reason': 'process-start identity changed', 'identities': identities}
    return {'outcome': 'pass', 'property': 'single_launch_identity_count', 'identities': identities}


class Case:
    def __init__(self, out, name):
        self.root = Path(tempfile.mkdtemp(prefix='pio-fh-', dir='/tmp')).resolve()
        self.out = out / name
        self.out.mkdir()
        self.daemons = []
        self.files = []
        self.identities = []

    def start(self):
        number = len(self.daemons)
        stdout = (self.out / f'daemon-{number}.stdout').open('w')
        stderr = (self.out / f'daemon-{number}.stderr').open('w')
        self.files += [stdout, stderr]
        daemon = subprocess.Popen([str(BINARY), 'fake', 'daemon', str(self.root)], stdout=stdout, stderr=stderr)
        self.daemons.append(daemon)
        status = poll(lambda: rpc(self.root, {'op':'status'}), lambda r: 'controller_generation' in r)
        return daemon, status

    def submit(self, fault='', duration=10000, **kwargs):
        return rpc(self.root, {'op':'submit', 'id':'work', 'payload':{'duration_ms':duration}, 'fault':fault, **kwargs})

    def inspect(self):
        result = rpc(self.root, {'op':'inspect', 'id':'work'})
        for key in ('host', 'child'):
            identity = result.get('invocation', {}).get(key)
            if identity and identity not in self.identities:
                self.identities.append(identity)
        return result

    def active(self):
        state = poll(self.inspect, lambda r: r.get('child_alive') and r.get('host_alive') and r['invocation']['phase'] == 'released')
        poll(lambda: records(self.root / f"release-{state['invocation']['invocation_id']}.jsonl"), bool)
        return state

    def stop_daemon(self, daemon):
        daemon.kill()
        daemon.wait(timeout=3)

    def close(self):
        # Preserve independent observations and canonical store rows before cleanup.
        for path in list(self.root.glob('*.jsonl')) + list(self.root.glob('launch-*.json')) + list(self.root.glob('attempt-*.json')) + list(self.root.glob('attempt-*.stderr')):
            (self.out / path.name).write_bytes(path.read_bytes())
        db = sqlite3.connect(self.root / 'journal.sqlite3')
        try:
            journal = [json.loads(row[0]) for row in db.execute('select record from journal order by sequence')]
            (self.out / 'journal.json').write_text(json.dumps(journal, indent=2) + '\n')
            outbox = [json.loads(row[0]) for row in db.execute('select record from outbox order by sequence')]
            (self.out / 'outbox.json').write_text(json.dumps(outbox, indent=2) + '\n')
            for row in db.execute('select state from invocations'):
                state = json.loads(row[0])
                self.identities.extend(state[k] for k in ('child','host') if state.get(k))
        finally:
            db.close()
        self.identities.extend(row['identity'] for row in records(self.root / 'spawn.jsonl'))
        for item in self.identities:
            terminate(item)
        for daemon in self.daemons:
            if daemon.poll() is None:
                self.stop_daemon(daemon)
        for file in self.files:
            file.close()
        # No recursive removal: isolated stores remain for diagnosis outside the repository.


def refusal_property(expected, observed):
    return {'property':'defense_layer_refusal','expected_reason':expected,'observed_reason':observed,
            'outcome':'pass' if observed == expected else 'wrong_reason'}


def run_case(case, name):
    daemon, status = case.start()
    if name == 'store_readonly_no_spawn':
        case.stop_daemon(daemon)
        files = list(case.root.glob('journal.sqlite3*'))
        modes = {str(p.name): oct(p.stat().st_mode & 0o777) for p in files}
        try:
            for path in files: path.chmod(0o400)
            case.root.chmod(0o500)
            result = subprocess.run([str(BINARY),'fake','daemon',str(case.root)],capture_output=True,text=True,timeout=3)
        finally:
            case.root.chmod(0o700)
            for path in files: path.chmod(0o600)
        reason = result.stderr.strip()
        observed = records(case.root/'spawn.jsonl')
        assert result.returncode == 2 and 'readonly' in reason, reason
        assert observed == []
        return {'outcome':'pass','fault_source':'filesystem permissions','database_mode':'0400','directory_mode':'0500',
                'original_modes':modes,'startup_exit':result.returncode,'refusal_reason':reason,'independent_spawn_markers':observed,'spawn_count':0}
    if name == 'fenced_release_known_not_released':
        case.submit('before_release')
        poll(lambda:(case.root/'before-release.ready').exists(),bool)
        before = case.inspect()
        case.stop_daemon(daemon)
        _,new = case.start()
        (case.root/'before-release.continue').write_text('continue\n')
        after = poll(case.inspect,lambda r:r.get('recovery')=='known_not_released' and not r.get('child_alive'))
        release = records(case.root/f"release-{after['invocation']['invocation_id']}.jsonl")
        assert release == [] and after['invocation']['receipt']['release_attempted'] is False
        return {'outcome':'pass','before':before,'after':after,'after_generation':new,'release_markers':release}
    if name == 'journal_failure_no_spawn':
        before = records(case.root / 'spawn.jsonl')
        result = case.submit('journal_failure')
        assert 'error' in result and 'readonly' in result['error'], result
        after = records(case.root / 'spawn.jsonl')
        table = subprocess.check_output(['ps','-axww','-o','pid=','-o','command='], text=True)
        observed = [line for line in table.splitlines() if f'fake child {case.root}' in line or f'fake host {case.root}' in line]
        assert before == after == [] and observed == [], (after, observed)
        assert case.inspect().get('error') == 'not_found'
        return {'outcome':'pass', 'refusal':result, 'independent_spawn_markers':after,
                'independent_process_table_matches':observed, 'spawn_count':0}
    if name == 'after_intent':
        try:
            case.submit(name)
        except (OSError, ValueError):
            pass
        assert daemon.wait(timeout=3) == 91
        _, restarted = case.start()
        replay = case.submit()
        state = case.inspect()
        assert replay['replay'] and state['recovery'] == 'not_released_pending'
        assert records(case.root / 'spawn.jsonl') == []
        return {'outcome':'pass', 'before_generation':status, 'after_generation':restarted,
                'after':state, 'spawn_count':0, 'fault_exit':91}
    if name in ('after_claim','after_release','after_receipt'):
        duration = 100 if name == 'after_receipt' else 10000
        case.submit(name, duration)
        phase = {'after_claim':'host_claimed','after_release':'released','after_receipt':'completed'}[name]
        state = poll(case.inspect, lambda r: r.get('invocation',{}).get('phase') == phase and not r.get('host_alive'))
        expected = 0 if name == 'after_claim' else 1
        markers = poll(lambda:records(case.root/'spawn.jsonl'), lambda r:len(r)==expected)
        replay = case.submit(duration=duration)
        assert replay['replay'] and len(records(case.root/'spawn.jsonl')) == expected
        return {'outcome':'pass','after':state,'markers':markers,'spawn_count':expected,'replay':True}
    if name == 'same_generation_restore':
        with sqlite3.connect(case.root/'journal.sqlite3') as source, sqlite3.connect(case.root/'backup.sqlite3') as backup:
            source.backup(backup)
    case.submit('duplicate_launch' if name == 'j3_duplicate_launch_mutant' else '')
    before = case.active()
    if name.startswith('mutant_'):
        variants = {
            'mutant_admit_replay_flag':('replay_relaunch','duplicate_launch_guard: launch already recorded'),
            'mutant_launch_guard':('replay_without_launch_guard','host_phase_fence: launch attempt already recorded'),
            'mutant_host_phase':('replay_without_host_phase','host_slot_fence: slot already owned'),
            'mutant_wrong_reason_control':('replay_relaunch','host_phase_fence: launch attempt already recorded'),
        }
        fault, expected = variants[name]
        response = case.submit(fault)
        observed = response.get('error')
        if not observed and response.get('launch_attempt'):
            outcome = poll(lambda:json.loads((case.root/f"attempt-{response['launch_attempt']}.json").read_text()),lambda r:'exit_code' in r)
            observed = outcome.get('reason') if outcome.get('exit_code') != 0 else 'mutant unexpectedly succeeded'
        check = refusal_property(expected, observed or 'no refusal observation')
        markers = records(case.root/'spawn.jsonl')
        assert len(markers) == 1, markers
        if name == 'mutant_wrong_reason_control':
            assert check['outcome']=='wrong_reason',check
            outcome_name = 'expected_classifier_failure'
        else:
            outcome_name = 'expected_defense_refusal' if check['outcome']=='pass' else 'wrong_reason'
        return {'outcome':outcome_name,'mutant':fault,'check':check,'spawn_count':len(markers),'markers':markers}
    if name == 'detach_restart_reattach':
        # Each RPC caller disconnects. Kill only daemon; dedicated host and child survive.
        case.stop_daemon(daemon)
        independent = process_identity(before['invocation']['child']['pid'])
        _, restarted = case.start()
        after = case.inspect()
        replay = case.submit()
        assert replay['replay']
        assert restarted['controller_generation'] > status['controller_generation']
        assert before['invocation']['host_slot'] == after['invocation']['host_slot']
        assert before['invocation']['host_generation'] == after['invocation']['host_generation']
        assert before['invocation']['child'] == independent
        check = single_launch(before['invocation']['child'], after['invocation']['child'], records(case.root/'spawn.jsonl'))
        assert check['outcome'] == 'pass', check
        return {'outcome':'pass','before':before,'after':after,'independent_child_while_daemon_absent':independent,'check':check}
    if name == 'lost_host_no_respawn':
        terminate(before['invocation']['host'])
        state = poll(case.inspect, lambda r:not r.get('host_alive'))
        assert state['recovery'] == 'uncertain_no_respawn' and state['child_alive']
        assert case.submit()['replay']
        check = single_launch(before['invocation']['child'], state['invocation']['child'],records(case.root/'spawn.jsonl'))
        assert check['outcome'] == 'pass'
        return {'outcome':'pass','before':before,'after':state,'check':check}
    if name == 'duplicate_and_conflicting_command':
        from concurrent.futures import ThreadPoolExecutor
        with ThreadPoolExecutor(max_workers=4) as pool:
            replies = list(pool.map(lambda _:case.submit(),range(4)))
        assert all(r['replay'] for r in replies)
        conflict = case.submit(duration=9999)
        assert conflict.get('error') == 'idempotency_conflict'
        after = case.inspect()
        check = single_launch(before['invocation']['child'],after['invocation']['child'],records(case.root/'spawn.jsonl'))
        assert check['outcome'] == 'pass'
        return {'outcome':'pass','retransmissions_attempted':4,'check':check,'conflict':conflict}
    if name == 'stale_controller':
        case.stop_daemon(daemon)
        _, restarted = case.start()
        refused = rpc(case.root, {'op':'submit','id':'stale-new','generation':status['controller_generation'],'payload':{}})
        assert refused.get('error') == 'stale_controller_generation'
        assert len(records(case.root/'spawn.jsonl')) == 1
        return {'outcome':'pass','before_generation':status,'after_generation':restarted,'refusal':refused,'spawn_count':1}
    if name == 'same_generation_restore':
        case.stop_daemon(daemon)
        with sqlite3.connect(case.root/'backup.sqlite3') as backup, sqlite3.connect(case.root/'journal.sqlite3') as destination:
            backup.backup(destination)
        result = subprocess.run([str(BINARY),'fake','daemon',str(case.root)],capture_output=True,text=True,timeout=3)
        assert result.returncode == 2 and 'restore_barrier' in result.stderr
        assert len(records(case.root/'spawn.jsonl')) == 1
        return {'outcome':'pass','generation':status,'startup_exit':result.returncode,'reason':result.stderr.strip(),'surviving_child':process_identity(before['invocation']['child']['pid']),'spawn_count':1}
    if name == 'restore_barrier':
        with sqlite3.connect(case.root/'journal.sqlite3') as source, sqlite3.connect(case.root/'backup.sqlite3') as backup:
            source.backup(backup)
        case.stop_daemon(daemon)
        daemon2, restarted = case.start()
        case.stop_daemon(daemon2)
        with sqlite3.connect(case.root/'backup.sqlite3') as backup, sqlite3.connect(case.root/'journal.sqlite3') as destination:
            backup.backup(destination)
        result = subprocess.run([str(BINARY),'fake','daemon',str(case.root)], capture_output=True,text=True,timeout=3)
        assert result.returncode == 2 and 'restore_barrier' in result.stderr
        assert len(records(case.root/'spawn.jsonl')) == 1
        return {'outcome':'pass','before_generation':status,'advanced_generation':restarted,'startup_exit':result.returncode,'reason':result.stderr.strip(),'surviving_child':process_identity(before['invocation']['child']['pid'])}
    if name == 'j3_duplicate_launch_mutant':
        markers = poll(lambda:records(case.root/'spawn.jsonl'),lambda r:len(r)==2)
        check = single_launch(before['invocation']['child'],before['invocation']['child'],markers)
        assert check['outcome'] == 'fail' and check['reason'] == 'expected 1 child start; observed 2', check
        return {'outcome':'expected_property_failure','mutant':'duplicate_launch','check':check,'spawn_count':2}
    raise AssertionError(f'unknown matrix case {name}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--repetitions',type=int,default=3)
    args = parser.parse_args()
    assert args.repetitions > 0
    args.out.mkdir(parents=True,exist_ok=False)
    cases = ['detach_restart_reattach','duplicate_and_conflicting_command','journal_failure_no_spawn','after_intent','after_claim','after_release','after_receipt','lost_host_no_respawn','stale_controller','restore_barrier','same_generation_restore','j3_duplicate_launch_mutant','mutant_admit_replay_flag','mutant_launch_guard','mutant_host_phase','mutant_wrong_reason_control','fenced_release_known_not_released','store_readonly_no_spawn']
    results = []
    for name in cases:
        for repetition in range(1,args.repetitions+1):
            case = Case(args.out, f'{name}-{repetition}')
            result = {'source':'fake-host','case':name,'repetition':repetition,'attempted':1}
            try:
                result.update(run_case(case,name))
            except Exception as error:
                result.update(outcome='wrong_reason' if name.startswith('mutant_') else 'harness_or_assertion_failure',reason=f'{type(error).__name__}: {error}')
            finally:
                try:
                    case.close()
                except Exception as error:
                    result.update(outcome='cleanup_failure',cleanup_error=str(error))
            (case.out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
            results.append(result)
            print(f'{name} #{repetition}: {result["outcome"]}',flush=True)
    files = subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=ROOT).decode().split('\0')
    inventory = {name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in sorted(set(files)) if name and (ROOT/name).is_file()}
    (args.out/'source-files.json').write_text(json.dumps(inventory,sort_keys=True,indent=2)+'\n')
    report = {'source_tree_sha256':hashlib.sha256(json.dumps(inventory,sort_keys=True).encode()).hexdigest(),'source':'fake-host','real_adapter':False,'protocol_conformance':False,
              'os':platform.system(),'arch':platform.machine(),'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
              'dirty_diff_sha256':hashlib.sha256(subprocess.check_output(['git','diff','HEAD'],cwd=ROOT)).hexdigest(),
              'binary_sha256':hashlib.sha256(BINARY.read_bytes()).hexdigest(),
              'repetitions_per_case':args.repetitions,'planned_attempts':len(cases)*args.repetitions,'attempted':len(results),
              'outcomes':dict(Counter(r['outcome'] for r in results)), 'results':results}
    (args.out/'matrix.json').write_text(json.dumps(report,indent=2)+'\n')
    return int(any(r['outcome'] not in ('pass','expected_property_failure','expected_defense_refusal','expected_classifier_failure') for r in results))


if __name__ == '__main__':
    raise SystemExit(main())
