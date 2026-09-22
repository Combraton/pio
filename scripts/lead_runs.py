#!/usr/bin/env python3
"""M4b step 3 — a lead that starts runs, and the scope it holds.

Two things, proven against `serve-fake` over the public Unix API, because
both are Protocol-level and neither depends on a harness.

**`origin` on `execution.submit`.** `{initiator, depth, call_budget}` is a
field of the submit payload and of the inspect result, both under the same
closed shape. PIO refused it outright before this: the gate required a
feature, `execution.context_revalidation`, that appears nowhere in the
pinned schemas and that nothing advertises. So nothing could say a run had a
lead. Now a run carries who started it, how deep it sits, and how many calls
its initiator may still make — and PIO refuses the two ways that can go
wrong.

**The lead's grant.** A lead may submit, steer, read and follow the stream.
It may **not** answer an approval: deciding for the user is the one thing a
lead must never do, and the grant is where that is said. Before this,
`execution.steer` and `execution.respond_action` both fell to the default
arm of the authorizer and were refused whatever the grant said — so a grant
could not express steer at all, and refusing an answer proved nothing. Both
are grantable now, which is what makes the refusal attributable to the
right being withheld.

Mutants, one per case, each flipping the single thing the case is about:

Each mutant changes **only the setup** and leaves every assertion alone, so
it dies on the claim it undermines:

- `--mutant deeper` names a run already at depth 1 as the initiator, so the
  same depth 2 is one level down and is admitted. The refusal is about the
  relationship, not about the number.
- `--mutant richer` gives the lead a second call, so the start that was
  refused is admitted. The refusal is about the budget, not about starting
  runs.
- `--mutant no-origin` submits the child with no `origin`, so the view has
  none. The field is echoed from the caller and never invented.
- `--mutant may-answer` puts `execution.respond_action` in the lead's grant,
  so the answer is not refused for scope. The refusal is about the right
  being withheld, not about leads.
- `--mutant owner-steers` sends the same steer as the owner, with no grant,
  so no grant is recorded against it. The key says who held a grant, not
  that a steer happened.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import case_cleanup
from board_fold import Caller, start_service
from public_api import CREDENTIAL, command, digest

ROOT = Path(__file__).resolve().parents[1]
PROVIDER = 'conformance-provider'
LEAD_CREDENTIAL = 'ccred1.lead.' + 'l' * 43
FEATURES = ('core.events', 'core.capabilities', 'core.effects', 'core.grants')


def content_digest(data):
    """A digest of the bytes themselves.

    `public_api.digest` digests a JSON *value*, which is right for an intent
    and wrong for content: it JSON-encodes what it is given, so handing it
    bytes raises rather than hashing them.
    """
    return 'sha256:' + hashlib.sha256(data).hexdigest()


def subject(identity):
    return dict(kind='execution.execution', id=identity)


def origin(initiator, depth, call_budget):
    return dict(initiator=subject(initiator), depth=depth, call_budget=call_budget)


def start(caller, identity, where=None, duration=400):
    """One `execution.submit`, with or without an `origin`.

    Built rather than patched: `command()` digests the intent it is given, so
    adding a field to the payload afterwards leaves the digest describing an
    envelope that was never sent.
    """
    payload = dict(brief=dict(
        digest=content_digest(f'fake work {duration}'.encode()),
        media_type='text/plain'))
    if where is not None:
        payload['origin'] = where
    return caller.call(command('execution.submit', subject(identity), payload,
                               command_id=identity))


def view(caller, identity):
    return caller.query('execution.inspect', {'execution': identity})['result']


def refusal_of(answer):
    """How a submit was refused, from wherever the refusal was recorded."""
    if 'error' in answer:
        return answer['error']['data'].get('code')
    return answer['result']['outcome'].get('reason')


def admitted(answer):
    return 'result' in answer and answer['result']['outcome']['admission'] != 'refused'


def pass_origin(out, root, mutant=None):
    """Who started whom, how deep, and how many calls are left."""
    budget = 2 if mutant == 'richer' else 1
    daemon, socket_path, files = start_service(root, out, name='origin')
    facts = {}
    try:
        owner = Caller(socket_path, CREDENTIAL, features=FEATURES)
        # The lead itself: depth 0, its own initiator — which is how "started
        # by nobody" is said in a shape whose `initiator` is required.
        answer = start(owner, 'lead', origin('lead', 0, budget))
        assert admitted(answer), answer
        seen = view(owner, 'lead')
        assert seen['origin'] == origin('lead', 0, budget), seen.get('origin')
        # Closed shape: `origin` is `unevaluatedProperties: false`, so the
        # view carries these three and nothing else.
        assert set(seen['origin']) == {'initiator', 'depth', 'call_budget'}, seen['origin']
        facts['lead'] = seen['origin']

        # 3. A run started by the lead names the lead's execution.
        child = origin('lead', 1, 0) if mutant != 'no-origin' else None
        answer = start(owner, 'run-1', child)
        assert admitted(answer), answer
        seen = view(owner, 'run-1')
        assert seen.get('origin') == origin('lead', 1, 0), (
            'the view does not carry the origin the caller sent: '
            f"{seen.get('origin')}")
        assert seen['origin']['initiator'] == subject('lead'), seen['origin']
        facts['run_1'] = seen['origin']

        # 1. Depth above the permitted depth is refused. The permitted
        #    depth is the initiator's own, plus one — so the same depth that
        #    is refused under the lead is admitted under a run that already
        #    sits a level down. `--mutant deeper` names that run instead,
        #    which is what makes the refusal about the relationship rather
        #    than about the number 2.
        under = 'run-1' if mutant == 'deeper' else 'lead'
        answer = start(owner, 'too-deep', origin(under, 2, 0))
        assert refusal_of(answer) == 'call_depth_exceeded', (
            f'depth 2 under {under!r} was not refused: {answer}')
        assert view(owner, 'too-deep')['admission'] == 'refused'
        facts['depth_2_under_the_lead_at_depth_0'] = 'call_depth_exceeded'

        # 2. A spent call budget refuses the next start.
        answer = start(owner, 'run-2', origin('lead', 1, 0))
        assert refusal_of(answer) == 'call_budget_spent', (
            f"the lead's second start was not refused at budget {budget}: "
            f'{answer}')
        facts['second_start_with_budget_1'] = 'call_budget_spent'
        # And the refusal is about *this* lead, not about starting runs: a
        # run with no origin at all still starts.
        assert admitted(start(owner, 'unled')), 'an unled run was refused'
        facts['unled_run'] = 'admitted'
        owner.close()
    finally:
        daemon.kill()
        daemon.wait(timeout=10)
        for handle in files:
            handle.close()
    return facts


def pass_lead_grant(out, root, mutant=None):
    """What a lead may do, and the one thing it may not."""
    daemon, socket_path, files = start_service(root, out, name='grant',
                                              credentials=(LEAD_CREDENTIAL,))
    facts = {}
    try:
        owner = Caller(socket_path, CREDENTIAL, features=FEATURES)
        # A run the lead will steer, and an action to try to answer.
        assert admitted(start(owner, 'work', origin('lead', 1, 0), duration=4000))

        rights = ['execution.submit', 'execution.steer', 'execution.read',
                  'core.events.read']
        if mutant == 'may-answer':
            rights.append('execution.respond_action')
        grant_id = str(uuid.uuid4())
        terms = dict(holder='lead', audience=PROVIDER, rights=rights,
                     resources=[dict(kind='execution.execution')],
                     delegation=dict(allowed=False, max_depth=0))
        issued = owner.call(command('core.grant.issue',
                                    dict(kind='core.grant', id=grant_id), terms,
                                    command_id=f'grant-{grant_id}'))
        assert 'result' in issued, issued
        facts['rights'] = rights

        lead = Caller(socket_path, LEAD_CREDENTIAL, grant=grant_id, features=FEATURES)
        # What the lead may do.
        started = start(lead, 'lead-run', origin('lead', 1, 0))
        assert admitted(started), started
        facts['submit'] = 'allowed'

        # Steer is **not** tried here either: this service advertises no
        # `execution.steering`, so it would be refused for a missing feature
        # before authorization is reached. Both halves that need a feature
        # this service does not have run against a real host path below.

        assert view(lead, 'work')['execution']['id'] == 'work'
        facts['read'] = 'allowed'
        events = lead.query('core.events.read',
                            {'limit': 50, 'from': 'start',
                             'kinds': ['execution.execution']})
        assert 'result' in events, events
        facts['events'] = 'allowed'

        # The answer is **not** tried here: `serve-fake` advertises no
        # `execution.actions`, so it would be refused
        # `unsupported_required_feature` before authorization is reached —
        # a refusal that says nothing about the lead's scope. That half runs
        # through a real host path, in `pass_lead_cannot_answer`.
        lead.close()
        owner.close()
    finally:
        daemon.kill()
        daemon.wait(timeout=10)
        for handle in files:
            handle.close()
    return facts


def pass_lead_cannot_answer(out, mutant=None):
    """The one thing a lead may not do, proven where an action exists.

    `serve-fake` advertises no `execution.actions`, so an
    `execution.respond_action` there is refused
    `unsupported_required_feature` **before** authorization is reached — a
    refusal that says nothing about the lead's scope. So this half runs
    through `serve-opencode` with the labeled ACP fake, where the feature is
    real and a request is genuinely waiting.
    """
    import opencode_host_matrix as matrix
    from approval_desk import ASKS
    facts = {}
    case = matrix.ServiceCase(out, 'lead-answer', model=matrix.REQUESTED,
                              extra_credentials=(LEAD_CREDENTIAL,),
                              scenario={'permission_request': ASKS})
    try:
        case.start()
        case.submit(identity='work', delivery_timeout=300)
        waiting = None
        for _ in range(240):
            waiting = case.inspect('work')
            if waiting['runtime'] == 'requires_action':
                break
            time.sleep(0.5)
        assert waiting['runtime'] == 'requires_action', waiting
        action = waiting['runtime_detail']['action_id']

        rights = ['execution.submit', 'execution.steer', 'execution.read',
                  'core.events.read']
        if mutant == 'may-answer':
            rights.append('execution.respond_action')
        grant_id = str(uuid.uuid4())
        # The matrix's own client negotiates no `core.grants`, and a session
        # that has not negotiated it cannot issue one.
        owner = Caller(case.socket, CREDENTIAL, features=FEATURES,
                       execution_features=matrix.FEATURES)
        audience = PROVIDER
        issued = owner.call(command(
            'core.grant.issue', dict(kind='core.grant', id=grant_id),
            dict(holder='lead', audience=audience, rights=rights,
                 resources=[dict(kind='execution.execution')],
                 delegation=dict(allowed=False, max_depth=0)),
            command_id=f'grant-{grant_id}'))
        assert 'result' in issued, issued
        facts['rights'] = rights
        facts['action'] = action

        # The session has to have negotiated the feature an operation needs,
        # whatever the grant says — which is why the first attempt at this
        # came back `unsupported_required_feature` rather than anything about
        # scope. The matrix's own list does not carry steering because its
        # cases never steer.
        lead = Caller(case.socket, LEAD_CREDENTIAL, grant=grant_id, features=FEATURES,
                      execution_features=(*matrix.FEATURES, 'execution.steering'))
        # What the grant **does** carry, where the feature is real: a steer
        # under it is not refused for scope. Whether this harness then
        # accepts the steer is its own business; the claim here is about the
        # grant, so the assertion is about `permission_denied` and nothing
        # else.
        note = b'keep to the fixture'
        steer = command('execution.steer', subject('work'),
                        dict(message=dict(digest=content_digest(note),
                                          media_type='text/plain')),
                        command_id='lead-steer', revision=case.inspect('work')['revision'])
        steer['extensions'] = {'pio.combraton.dev/content':
                               dict(media_type='text/plain', text=note.decode())}
        steered = (owner if mutant == 'owner-steers' else lead).call(steer)
        steer_code = steered.get('error', {}).get('data', {}).get('code')
        assert steer_code != 'permission_denied', (
            'the grant carries execution.steer and the steer was refused for '
            f'scope: {steered}')
        facts['steer'] = steer_code or 'allowed'

        # Who the steer came from, as far as PIO can say it. The Protocol
        # has no authorship field: `steering_entry` is closed, and the event
        # record names a command and a principal but never who held the
        # grant. So the grant rides in the payload under a namespaced key,
        # labelled as PIO's own record. `--mutant owner-steers` has the
        # owner send the same steer with no grant, and the key is absent.
        under = None
        for item in owner.query('core.events.read',
                                {'limit': 200, 'from': 'start',
                                 'kinds': ['execution.execution']})['result']['items']:
            event = item.get('event')
            if event and event['type'] == 'execution.steer.requested':
                under = event['payload'].get('pio.combraton.dev/under-grant')
        assert under is not None, (
            'the steer came under a grant and the stream does not say whose')
        assert under['grant'] == grant_id, under
        assert under['holder'] == 'lead', under
        assert under['recorded_by'] == 'pio', under
        facts['under_grant'] = under

        body = json.dumps({'decision': 'allow'}).encode()
        attempt = command('execution.respond_action', subject('work'),
                          dict(action_id=action,
                               response=dict(digest=content_digest(body),
                                             media_type='application/json')),
                          command_id='lead-answer', revision=waiting['revision'])
        attempt['extensions'] = {'pio.combraton.dev/content':
                                 dict(media_type='application/json',
                                      text=body.decode())}
        refused = lead.call(attempt)
        code = refused.get('error', {}).get('data', {}).get('code')
        assert code == 'permission_denied', (
            f'the answer was not refused for scope under rights {rights}: '
            f'{refused}')
        assert refused['error']['data']['details']['reason'] == 'right_missing', refused
        facts['without_the_right'] = 'permission_denied / right_missing'
        # And the refusal is about the right, not about the action: the
        # owner answers the same one.
        answered = case.respond(action, 'allow', case.inspect('work')['revision'],
                                identity='work')
        assert answered.get('result', {}).get('outcome', {}).get(
            'state') == 'answered', answered
        facts['owner_answers_the_same_action'] = 'answered'
        lead.close()
        owner.close()
    finally:
        case.cleanup()
    return facts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=ROOT / 'target/lead-runs')
    parser.add_argument('--mutant', choices=['deeper', 'richer', 'no-origin',
                                             'may-answer', 'owner-steers'])
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    roots = []
    try:
        for label, work in (('origin', pass_origin), ('lead_grant', pass_lead_grant)):
            root = Path(tempfile.mkdtemp(prefix='pio-lead-', dir='/tmp')).resolve()
            os.chmod(root, 0o700)
            roots.append(root)
        record = dict(
            origin=pass_origin(args.out, roots[0], args.mutant),
            lead_grant=pass_lead_grant(args.out, roots[1], args.mutant),
            lead_cannot_answer=pass_lead_cannot_answer(args.out, args.mutant))
        (args.out / 'lead-runs.json').write_text(
            json.dumps(record, indent=2, sort_keys=True) + '\n')
        print(json.dumps(record, indent=2, sort_keys=True))
        print('lead runs: pass')
    finally:
        for root in roots:
            try:
                case_cleanup.release(root)
            except Exception:
                shutil.rmtree(root, ignore_errors=True)


if __name__ == '__main__':
    main()
