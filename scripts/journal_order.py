"""Read-only ordering oracle, independent of provider control flow and projections."""
import hashlib
import json
import sqlite3

ORDER_REASON = 'dispatch_intent_must_precede_invocation_intent'

def ordering(root):
    with sqlite3.connect(f'file:{root}/journal.sqlite3?mode=ro', uri=True) as db:
        facts = [dict(sequence=seq, record=json.loads(record)) for seq, record in db.execute('select sequence,record from journal order by sequence')]
    dispatch = {}
    invocations = []
    for fact in facts:
        seq, record = fact['sequence'], fact['record']
        if record['kind'] == 'protocol.commit':
            for change in record['changes']:
                value = change.get('value', {})
                effect = value.get('effect', {})
                if (change['key'].startswith('effect/') and effect.get('kind') == 'execution.prompt_submission'
                        and effect.get('target', {}).get('kind') == 'execution.execution'
                        and any(o.get('evidence', {}).get('class') == 'dispatch_intent' for o in value.get('observations', []))):
                    execution = effect['target']['id']
                    dispatch.setdefault(execution, seq)
        elif record['kind'] == 'invocation.intent':
            invocations.append((seq, record['invocation']))
    checks = []
    for seq, invocation in invocations:
        matches = [(execution, n) for execution, n in dispatch.items()
                   if hashlib.sha256(execution.encode()).hexdigest() == invocation['command_id']]
        if len(matches) != 1:
            checks.append(dict(outcome='fail', reason='dispatch_intent_fact_missing_or_ambiguous', invocation_intent_sequence=seq, invocation_id=invocation['invocation_id']))
            continue
        execution, before = matches[0]
        checks.append(dict(outcome='pass' if before < seq else 'fail', reason=ORDER_REASON,
                           execution=execution, command_id=invocation['command_id'], invocation_id=invocation['invocation_id'],
                           dispatch_intent_sequence=before, invocation_intent_sequence=seq))
    matched={c.get('execution') for c in checks}
    return dict(property=ORDER_REASON, dispatch_sequences=dispatch, unmatched_dispatches=sorted(set(dispatch)-matched), checks=checks, facts=facts)

def classify_mutant(report, expected=ORDER_REASON):
    failures = [c for c in report['checks'] if c['outcome']=='fail']
    reason = failures[0]['reason'] if len(failures)==1 else 'expected_exactly_one_ordering_failure'
    return dict(outcome='expected_property_failure' if reason==expected else 'wrong_reason',
                property=ORDER_REASON, expected_reason=expected, observed_reason=reason, failures=failures)
