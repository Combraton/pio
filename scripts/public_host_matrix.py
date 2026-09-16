#!/usr/bin/env python3
"""Public Unix Protocol process matrix; kernel/journal witnesses are independent.
No request uses the diagnostic daemon or its JSON operations.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import platform
import sqlite3
import subprocess
import time
import fake_host_matrix as witness
from public_api import Client, CREDENTIAL, submit, command

ROOT, BINARY = witness.ROOT, witness.BINARY
poll, records = witness.poll, witness.records
CASES = ['detach_restart_reattach','duplicate_and_conflicting_command','journal_failure_no_spawn','after_intent','after_claim','after_release','after_receipt','lost_host_no_respawn','stale_controller','restore_barrier','same_generation_restore','j3_duplicate_launch_mutant','mutant_admit_replay_flag','mutant_launch_guard','mutant_host_phase','mutant_wrong_reason_control','fenced_release_known_not_released','store_readonly_no_spawn']
FAULTS = dict(j3_duplicate_launch_mutant='duplicate_launch',mutant_admit_replay_flag='replay_relaunch',mutant_launch_guard='replay_without_launch_guard',mutant_host_phase='replay_without_host_phase',mutant_wrong_reason_control='replay_relaunch',fenced_release_known_not_released='before_release')

class Case(witness.Case):
    def __init__(self,out,name):
        super().__init__(out,name)
        self.name = name.rsplit('-',1)[0]
        self.transcript = self.out/'public-transcript.jsonl'
        duration = 100 if self.name=='after_receipt' else 10000
        protocol = dict(format='combraton-conformance-config/1',principal='owner',credentials=[dict(credential=CREDENTIAL)],executor=dict(host_id='durable-fake-host'))
        if self.name=='journal_failure_no_spawn': protocol['faults']={'commit_unavailable':[{'operation':'execution.submit','times':1}]}
        self.config = dict(format='pio-fake-service/1',protocol=protocol,fake_host=dict(duration_ms=duration,fault=FAULTS.get(self.name,self.name if self.name in ('after_intent','after_claim','after_release','after_receipt') else '')))
        self.config_path=self.root/'service.json'
        self.config_path.write_text(json.dumps(self.config))
        self.request = submit(duration)
    def argv(self):
        return [str(BINARY),'serve-fake','--data-dir',str(self.root),'--config',str(self.config_path),'--socket',str(self.root/'public.sock')]
    def start(self):
        number=len(self.daemons)
        stdout=(self.out/f'daemon-{number}.stdout').open('w');stderr=(self.out/f'daemon-{number}.stderr').open('w')
        self.files += [stdout,stderr]
        daemon=subprocess.Popen(self.argv(),stdout=stdout,stderr=stderr)
        self.daemons.append(daemon)
        def ready():
            with Client(self.root/'public.sock',self.transcript) as c:
                return c.query('core.describe',{})
        poll(ready,lambda r:'result' in r)
        with sqlite3.connect(self.root/'journal.sqlite3') as db: generation=db.execute('select generation from meta').fetchone()[0]
        return daemon,dict(controller_generation=generation)
    def close(self):
        try:
            if self.transcript.exists():
                validation=subprocess.run([str(BINARY),'check-transcript',str(self.transcript)],capture_output=True,text=True)
                (self.out/'schema-validation.txt').write_text(validation.stdout+validation.stderr)
                assert validation.returncode==0,validation.stderr
        finally: super().close()
    def submit(self,fault='',duration=10000,**kwargs):
        p = self.request if duration in (10000,100) else submit(duration)
        with Client(self.root/'public.sock',self.transcript) as c: response=c.call(p)
        if 'error' in response: return dict(error=response['error']['data']['code'],public=response)
        if fault.startswith('replay_'):
            def mutation():
                with sqlite3.connect(self.root/'journal.sqlite3') as db:
                    row=db.execute("select value from protocol_projection where key='execution/work'").fetchone()
                    return json.loads(row[0]).get('host_mutant',{}) if row else {}
            v=poll(mutation,lambda v:v.get('reason') or v.get('outcome'))
            return dict(error=v.get('reason') or v.get('outcome',{}).get('reason'),public=response)
        return dict(response['result'],public=response)
    def inspect(self):
        with Client(self.root/'public.sock',self.transcript) as c: response=c.query('execution.inspect',{'execution':'work'})
        if 'error' in response: return dict(error=response['error']['data']['code'],public=response)
        with sqlite3.connect(self.root/'journal.sqlite3') as db:
            row=db.execute('select state from invocations').fetchone()
            generation=db.execute('select generation from meta').fetchone()[0]
        if not row: return dict(public=response)
        state=json.loads(row[0]);host=state.get('host');child=state.get('child')
        for identity in (host,child):
            if identity and identity not in self.identities:self.identities.append(identity)
        alive=lambda identity:bool(identity and witness.process_identity(identity['pid'])==identity)
        recovery=response['result']['deliveries'][0].get('evidence',{}).get('class')
        return dict(public=response,witness_source='read-only journal plus kernel identity',invocation=state,controller_generation=generation,host_alive=alive(host),child_alive=alive(child),recovery=recovery)

# Reuse identity/count property definitions, not diagnostic calls. The process
# cut points remain launch configuration, never fields in public commands.
def run_case(case,name):
    if name in ('store_readonly_no_spawn','same_generation_restore','restore_barrier','stale_controller','journal_failure_no_spawn'):
        daemon,status=case.start()
        if name=='journal_failure_no_spawn':
            refused=case.submit()
            assert refused.get('error')=='unavailable',refused
            time.sleep(.1)
            table=subprocess.check_output(['ps','-axww','-o','pid=','-o','command='],text=True)
            matches=[line for line in table.splitlines() if f'fake child {case.root}' in line or f'fake host {case.root}' in line]
            assert records(case.root/'spawn.jsonl')==[] and matches==[]
            assert case.inspect()['error']=='not_found'
            return dict(outcome='pass',refusal=refused,independent_process_table_matches=matches,spawn_count=0,independent_spawn_markers=[])
        if name=='store_readonly_no_spawn':
            case.stop_daemon(daemon)
            files=list(case.root.glob('journal.sqlite3*'))
            try:
                for path in files:path.chmod(0o400)
                case.root.chmod(0o500)
                result=subprocess.run(case.argv(),capture_output=True,text=True,timeout=5)
            finally:
                case.root.chmod(0o700)
                for path in files:path.chmod(0o600)
            assert result.returncode==2 and 'readonly' in result.stderr,result
            table=subprocess.check_output(['ps','-axww','-o','pid=','-o','command='],text=True)
            matches=[line for line in table.splitlines() if f'fake child {case.root}' in line or f'fake host {case.root}' in line]
            assert records(case.root/'spawn.jsonl')==[] and matches==[]
            return dict(outcome='pass',fault_source='filesystem permissions',database_mode='0400',directory_mode='0500',reason=result.stderr,spawn_count=0,independent_process_table_matches=matches)
        if name=='same_generation_restore':
            with sqlite3.connect(case.root/'journal.sqlite3') as source,sqlite3.connect(case.root/'backup.sqlite3') as backup:source.backup(backup)
        case.submit();before=case.active()
        if name=='stale_controller':
            with Client(case.root/'public.sock',case.transcript) as c:
                target=dict(kind='execution.controller',id='durable-fake-host')
                claim=command('execution.controller.claim',target,{},'claim')
                first=c.call(claim);assert 'result' in first,first
            case.stop_daemon(daemon);_,restarted=case.start()
            with Client(case.root/'public.sock',case.transcript) as c:
                refused=c.call(submit(identity='stale-new'))
            assert refused['error']['data']['code']=='stale_authority_epoch',refused
            assert len(records(case.root/'spawn.jsonl'))==1
            return dict(outcome='pass',before_generation=status,after_generation=restarted,refusal=refused,spawn_count=1)
        if name=='restore_barrier':
            with sqlite3.connect(case.root/'journal.sqlite3') as source,sqlite3.connect(case.root/'backup.sqlite3') as backup:source.backup(backup)
            case.stop_daemon(daemon);daemon,_=case.start()
        case.stop_daemon(daemon)
        with sqlite3.connect(case.root/'backup.sqlite3') as source,sqlite3.connect(case.root/'journal.sqlite3') as dest:source.backup(dest)
        result=subprocess.run(case.argv(),capture_output=True,text=True,timeout=5)
        assert result.returncode==2 and 'restore_barrier' in result.stderr,result
        assert len(records(case.root/'spawn.jsonl'))==1
        return dict(outcome='pass',reason=result.stderr,surviving_child=witness.process_identity(before['invocation']['child']['pid']),spawn_count=1)
    return witness.run_case(case,name)

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--out',type=Path,required=True);parser.add_argument('--repetitions',type=int,default=3);parser.add_argument('--case',choices=CASES)
    args=parser.parse_args();assert args.repetitions>0;args.out.mkdir(parents=True,exist_ok=False)
    results=[]
    for name in ([args.case] if args.case else CASES):
        for repetition in range(1,args.repetitions+1):
            case=Case(args.out,f'{name}-{repetition}');result=dict(source='fake-host/process',path='public Unix Protocol',case=name,repetition=repetition,attempted=1)
            try:result.update(run_case(case,name))
            except Exception as error:result.update(outcome='wrong_reason' if name.startswith('mutant_') else 'harness_or_assertion_failure',reason=f'{type(error).__name__}: {error}')
            finally:
                try:case.close()
                except Exception as error:result.update(outcome='harness_or_assertion_failure',cleanup_error=str(error))
            results.append(result);print(name,repetition,result['outcome'],result.get('reason',''),flush=True)
    counts=dict(Counter(r['outcome'] for r in results))
    tracked=subprocess.check_output(['git','ls-files','-z'],cwd=ROOT).split(b'\0')
    inventory={p.decode():hashlib.sha256((ROOT/p.decode()).read_bytes()).hexdigest() for p in tracked if p and (ROOT/p.decode()).is_file()}
    report=dict(format='pio-public-host-matrix/1',source='fake-host/process',real_adapter=False,head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),dirty_diff_sha256=hashlib.sha256(subprocess.check_output(['git','diff','HEAD'],cwd=ROOT)).hexdigest(),source_tree_sha256=hashlib.sha256(json.dumps(inventory,sort_keys=True).encode()).hexdigest(),platform=platform.platform(),binary_sha256=hashlib.sha256(BINARY.read_bytes()).hexdigest(),source_inventory=inventory,attempted=len(results),repetitions=args.repetitions,counts=counts,results=results)
    (args.out/'matrix.json').write_text(json.dumps(report,indent=2)+'\n')
    assert set(counts)<= {'pass','expected_property_failure','expected_defense_refusal','expected_classifier_failure'},counts
if __name__=='__main__':main()
