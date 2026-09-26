#!/usr/bin/env python3
"""Durable caller lost-response recovery and content-addressed output proof."""
import argparse
import base64
import hashlib
import case_cleanup
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile
from public_host_matrix import Case, BINARY, poll, records
from public_api import Client, CREDENTIAL

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--out',type=Path,required=True);args=parser.parse_args();args.out.mkdir(parents=True,exist_ok=False)
    case=Case(args.out,'client-recovery-1')
    caller=Path(tempfile.mkdtemp(prefix='pio-caller-',dir='/tmp'));callers=[caller]
    credential=caller/'credential';credential.write_text(CREDENTIAL);credential.chmod(0o600)
    request=caller/'request.json';request.write_text(json.dumps(case.request))
    basis=caller/'basis.json';basis.write_text(json.dumps({'scope':'labeled fake work','policy':'explicit caller selection','packet_bindings':[]}))
    case.config['fake_host']=dict(duration_ms=100,fault='')
    case.config['protocol']['faults']={'response_internal_error':[{'operation':'execution.submit','times':1}]}
    case.config_path.write_text(json.dumps(case.config))
    common=['--store',str(caller),'--socket',str(case.root/'public.sock'),'--credential-file',str(credential),'--json']
    try:
        # Unavailable endpoint: the request survives before any network I/O.
        initial=subprocess.run([str(BINARY),'client','submit',*common,'--request',str(request),'--basis',str(basis)],capture_output=True,text=True)
        assert initial.returncode==2,initial
        with sqlite3.connect(caller/'caller.sqlite3') as db:
            pending=db.execute('select id,request,basis,status from operations').fetchall()
        assert len(pending)==1 and pending[0][3]=='pending' and json.loads(pending[0][1])==case.request
        assert records(case.root/'spawn.jsonl')==[]
        case.start()
        empty=subprocess.run([str(BINARY),'client','reconcile',*common],capture_output=True,text=True,check=True)
        assert json.loads(empty.stdout)['operations'][0]['status']=='pending'
        assert records(case.root/'spawn.jsonl')==[]
        # Use a second ledger to exercise initial send and an intentionally lost
        # success response. No direct writes to either caller ledger.
        caller2=Path(tempfile.mkdtemp(prefix='pio-caller-',dir='/tmp'));callers.append(caller2);common2=common.copy();common2[1]=str(caller2)
        sent=subprocess.run([str(BINARY),'client','submit',*common2,'--request',str(request),'--basis',str(basis)],capture_output=True,text=True,check=True)
        assert json.loads(sent.stdout)['response']['error']['data']['code']=='internal_error'
        with sqlite3.connect(caller2/'caller.sqlite3') as db:assert db.execute('select status from operations').fetchone()[0]=='pending'
        recovered=subprocess.run([str(BINARY),'client','reconcile',*common2],capture_output=True,text=True,check=True)
        receipt=json.loads(recovered.stdout)['operations'][0]
        assert receipt['response']['result']['replay'] is True
        state=poll(case.inspect,lambda v:v.get('public',{}).get('result',{}).get('runtime')=='exited')
        assert len(records(case.root/'spawn.jsonl'))==1
        with Client(case.root/'public.sock',case.transcript) as c:output=c.query('execution.output.read',{'execution':'work','offset':0,'max_bytes':65536})
        data=base64.b64decode(output['result']['data_base64'])
        assert b'deterministic fake work started' in data and b'deterministic fake work ended' in data
        with sqlite3.connect(case.root/'journal.sqlite3') as db:
            execution=json.loads(db.execute("select value from protocol_projection where key='execution/work'").fetchone()[0])
            journal='\n'.join(r[0] for r in db.execute('select record from journal'))
        ref=execution['output_ref'];blob=case.root/'spool'/ref['digest'].split(':')[1]
        assert blob.read_bytes()==data and 'sha256:'+hashlib.sha256(data).hexdigest()==ref['digest']
        assert 'deterministic fake work started' not in journal and '"output":[' not in journal
        with sqlite3.connect(caller2/'caller.sqlite3') as db:
            rows=db.execute('select request,basis,status,observation from operations').fetchall()
        assert len(rows)==1 and rows[0][2]=='acknowledged'
        assert CREDENTIAL not in json.dumps(rows)
        report=dict(source='fake-host/process',real_adapter=False,outcome='pass',pre_submission_pending=True,empty_reconciliation_did_not_spawn=True,recovery=receipt,spawn_count=1,output_ref=ref,output_bytes=len(data),inline_output_in_journal=False,caller_rows=[dict(request=json.loads(r[0]),basis=json.loads(r[1]),status=r[2],observation=json.loads(r[3])) for r in rows],public_exit=state['public'])
        (args.out/'client-recovery.json').write_text(json.dumps(report,indent=2)+'\n');print('caller recovery and content-addressed output: pass')
    finally:
        case.close()
        # The caller directories are this script's own, so they are released
        # here rather than retained; a successful run left three behind.
        for path in callers:
            case_cleanup.release(path)
if __name__=='__main__':main()
