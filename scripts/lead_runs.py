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
- `--mutant refused-initiator-ok` admits the initiator, so its child is
  admitted too. The refusal is about the initiator having been refused, not
  about depth or budget.
- `--mutant exited-initiator-ok` submits the child while the initiator is
  still running, so it is admitted. The refusal is about the initiator having
  exited, not about initiators in general.
- `--mutant known-initiator-ok` admits the grant's subtree owner before its
  children are submitted, so they are admitted. The refusal is about PIO never
  having seen the initiator, not about grants or origins.
- `--mutant owner-submits-root` sends the root submits as the owner, with no
  grant, so they are admitted. The refusal is about a grant starting a root,
  not about root runs.
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
# How long a `serve-fake` run lasts where a case needs its lead **alive**
# while children are started. Owner decision, 2026-09-23: an initiator that
# has exited starts nothing, and the service default of 1.2 seconds would end
# a lead in the middle of the case.
LEAD_ALIVE_MS = 60_000
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
    daemon, socket_path, files = start_service(root, out, name='origin',
                                               duration_ms=LEAD_ALIVE_MS)
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

        # **And a depth that is too shallow is just as wrong.** Accepting
        # anything up to the permitted depth let a child of the depth-0 lead
        # record `depth: 0` and its grandchild `depth: 1`, so the tree's
        # shape was whatever the caller said. `--mutant shallow-ok` claims
        # the derived depth instead, and is admitted.
        claimed = 1 if mutant == 'shallow-ok' else 0
        answer = start(owner, 'too-shallow', origin('lead', claimed, 0))
        assert refusal_of(answer) == 'call_depth_understated', (
            f'depth {claimed} under a lead at depth 0 was not refused: {answer}')
        facts['depth_0_under_the_lead_at_depth_0'] = 'call_depth_understated'

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
                                              credentials=(LEAD_CREDENTIAL,),
                                              duration_ms=LEAD_ALIVE_MS)
    facts = {}
    try:
        owner = Caller(socket_path, CREDENTIAL, features=FEATURES)
        # A run the lead will steer, and an action to try to answer.
        assert admitted(start(owner, 'work', origin('lead', 1, 0), duration=4000))
        # The lead itself, alive: under a grant, a child may name only an
        # initiator PIO has seen. `work` names it too and spends one call.
        assert admitted(start(owner, 'lead', origin('lead', 0, 2)))

        rights = ['execution.submit', 'execution.steer', 'execution.read',
                  'core.events.read']
        if mutant == 'may-answer':
            rights.append('execution.respond_action')
        grant_id = str(uuid.uuid4())
        # **Scoped to the lead's own subtree.** An unscoped
        # `kind: execution.execution` let a lead read every run on the
        # service, including ones it did not start. Children are named under
        # the prefix so the scope has something to bind to.
        terms = dict(holder='lead', audience=PROVIDER, rights=rights,
                     resources=[dict(kind='execution.execution',
                                     id_prefix='lead.')],
                     delegation=dict(allowed=False, max_depth=0))
        issued = owner.call(command('core.grant.issue',
                                    dict(kind='core.grant', id=grant_id), terms,
                                    command_id=f'grant-{grant_id}'))
        assert 'result' in issued, issued
        facts['rights'] = rights

        lead = Caller(socket_path, LEAD_CREDENTIAL, grant=grant_id, features=FEATURES)
        # What the lead may do.
        started = start(lead, 'lead.run-1', origin('lead', 1, 0))
        assert admitted(started), started
        facts['submit'] = 'allowed'

        # And nothing outside the subtree. `work` is the owner's run; the
        # lead never started it and may not read it.
        outside = lead.query('execution.inspect', {'execution': 'work'})
        assert 'error' in outside, (
            f'the lead read a run outside its own subtree: {outside}')
        assert outside['error']['data']['code'] == 'permission_denied', outside
        assert outside['error']['data']['details']['reason'] == 'out_of_scope', outside
        facts['inspect_outside_the_subtree'] = 'permission_denied / out_of_scope'

        # Steer is **not** tried here either: this service advertises no
        # `execution.steering`, so it would be refused for a missing feature
        # before authorization is reached. Both halves that need a feature
        # this service does not have run against a real host path below.

        assert view(lead, 'lead.run-1')['execution']['id'] == 'lead.run-1'
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


def pass_initiator_is_bound(out, root, mutant=None):
    """A grant may not name someone else's run as the initiator.

    `origin.initiator` was an unchecked caller claim. A principal holding a
    plain submit grant could submit a run naming an unrelated run as its
    initiator; it was admitted, and that run's **next** start was then
    refused `call_budget_spent`. Forged lineage and budget theft, from a
    grant that was never meant to reach that run at all.

    The initiator is now bound to the grant: the only run a caller may name
    is the one whose subtree the grant covers. A grant that names no subtree
    names no initiator.

    `--mutant bound-initiator` scopes the stranger's grant to the lead's own
    subtree, so naming the lead is legitimate and the submit is admitted —
    which is what shows the refusal is about the binding and not about
    origins under grants.
- `--mutant unscoped-grant` hands out a grant that names no subtree, which
  therefore requires no origin, so a submit claiming no lineage is admitted.
- `--mutant shallow-ok` claims the derived depth, so the submit is admitted:
  the refusal is about the depth being wrong, not about depth being stated.
    """
    daemon, socket_path, files = start_service(root, out, name='spoof',
                                               credentials=(LEAD_CREDENTIAL,),
                                               duration_ms=LEAD_ALIVE_MS)
    facts = {}
    try:
        owner = Caller(socket_path, CREDENTIAL, features=FEATURES)
        # A lead with one call to spend, and the run it legitimately starts.
        assert admitted(start(owner, 'lead', origin('lead', 0, 1)))

        # A stranger with a plain submit grant over everything.
        scope = (dict(kind='execution.execution', id_prefix='lead.')
                 if mutant == 'bound-initiator'
                 else dict(kind='execution.execution'))
        grant_id = str(uuid.uuid4())
        issued = owner.call(command(
            'core.grant.issue', dict(kind='core.grant', id=grant_id),
            dict(holder='lead', audience=PROVIDER,
                 rights=['execution.submit', 'execution.read'],
                 resources=[scope],
                 delegation=dict(allowed=False, max_depth=0)),
            command_id=f'grant-{grant_id}'))
        assert 'result' in issued, issued
        # A second grant, scoped to the lead's subtree, for the
        # omitted-origin case below. The mutant makes it unscoped.
        scoped_id = str(uuid.uuid4())
        issued = owner.call(command(
            'core.grant.issue', dict(kind='core.grant', id=scoped_id),
            dict(holder='lead', audience=PROVIDER,
                 rights=['execution.submit', 'execution.read'],
                 resources=[dict(kind='execution.execution')
                            if mutant == 'unscoped-grant'
                            else dict(kind='execution.execution',
                                      id_prefix='lead.')],
                 delegation=dict(allowed=False, max_depth=0)),
            command_id=f'grant-{scoped_id}'))
        assert 'result' in issued, issued
        stranger = Caller(socket_path, LEAD_CREDENTIAL, grant=grant_id, features=FEATURES)

        # The spoof: a run of the stranger's, claiming the lead started it.
        spoofed = start(stranger, 'lead.stolen', origin('lead', 1, 0))
        code = spoofed.get('error', {}).get('data', {}).get('code')
        assert code == 'permission_denied', (
            'a grant that does not cover the lead named it as initiator and '
            f'was admitted — forged lineage: {spoofed}')
        assert spoofed['error']['data']['details']['reason'] == 'out_of_scope', spoofed
        facts['spoofed_initiator'] = 'permission_denied / out_of_scope'

        # And the budget it would have stolen is still there: the lead's own
        # one call still starts a run.
        assert admitted(start(owner, 'lead.run-1', origin('lead', 1, 0))), \
            "the lead's own call was spent by a submit that was refused"
        facts['lead_budget_intact'] = 'admitted'

        # **Omitting the origin is the same hole by the other door.** With
        # the lead's budget now spent, a submit under its prefix carrying no
        # origin at all was admitted and its view carried no lineage — the
        # budget checked nothing because there was nothing to check.
        scoped = Caller(socket_path, LEAD_CREDENTIAL, grant=scoped_id,
                        features=FEATURES)
        bare = start(scoped, 'lead.bare')
        code = bare.get('error', {}).get('data', {}).get('code')
        assert code == 'permission_denied', (
            'a submit under a subtree-scoped grant claimed no lineage and was '
            f'admitted: {bare}')
        assert bare['error']['data']['details']['reason'] == 'out_of_scope', bare
        facts['omitted_origin'] = 'permission_denied / out_of_scope'
        scoped.close()
        stranger.close()
        owner.close()
    finally:
        daemon.kill()
        daemon.wait(timeout=10)
        for handle in files:
            handle.close()
    return facts


def pass_initiator_liveness(out, root, mutant=None):
    """An initiator must be alive to ask: not refused, and not exited.

    Owner decision, 2026-09-23. The L1 rehearsal's lead was refused for a
    missing brief, and its two children were admitted anyway — lineage
    attached to a run that never started, spending a budget its caller had
    merely claimed. A run that has exited is the same case later: its turn
    is over, so it is not calling anything.
    """
    # Short runs, so the exited case can wait one out.
    daemon, socket_path, files = start_service(root, out, name='liveness',
                                               duration_ms=1500)
    facts = {}
    try:
        owner = Caller(socket_path, CREDENTIAL, features=FEATURES)

        # **Refused.** A top-level run claiming depth 1 is refused
        # `call_depth_exceeded`, and its origin is still recorded — which is
        # exactly what let a child name it. `--mutant refused-initiator-ok`
        # claims the right depth, so the initiator is admitted.
        claimed = 0 if mutant == 'refused-initiator-ok' else 1
        start(owner, 'lead-r', origin('lead-r', claimed, 2))
        lead = view(owner, 'lead-r')
        if mutant != 'refused-initiator-ok':
            assert lead['admission'] == 'refused', lead
        # The child claims the depth the recorded origin implies, so nothing
        # but the initiator's state can refuse it.
        depth = lead['origin']['depth'] + 1
        answer = start(owner, 'lead-r.child', origin('lead-r', depth, 0))
        assert refusal_of(answer) == 'initiator_refused', (
            f'a child of a refused initiator was not refused: {answer}')
        facts['child_of_a_refused_initiator'] = 'initiator_refused'

        # **Exited.** `--mutant exited-initiator-ok` does not wait, so the
        # child is submitted while the initiator is still running.
        assert admitted(start(owner, 'lead-x', origin('lead-x', 0, 2)))
        if mutant != 'exited-initiator-ok':
            deadline = time.monotonic() + 60
            while view(owner, 'lead-x')['runtime'] != 'exited':
                assert time.monotonic() < deadline, view(owner, 'lead-x')
                time.sleep(0.2)
        facts['initiator_runtime_at_submit'] = view(owner, 'lead-x')['runtime']
        answer = start(owner, 'lead-x.child', origin('lead-x', 1, 0))
        assert refusal_of(answer) == 'initiator_exited', (
            f'a child of an exited initiator was not refused: {answer}')
        facts['child_of_an_exited_initiator'] = 'initiator_exited'

        # And a run naming **itself** is untouched: it is being started, so
        # it has no state to be refused or exited in.
        assert admitted(start(owner, 'fresh', origin('fresh', 0, 1)))
        facts['a_run_naming_itself'] = 'admitted'
        owner.close()
    finally:
        daemon.kill()
        daemon.wait(timeout=10)
        for handle in files:
            handle.close()
    return facts


def pass_initiator_unknown(out, root, mutant=None):
    """Under a grant, an initiator PIO has never seen is refused.

    Review 44 probed a grant for `ghost.` with no run `ghost`: it admitted
    four children, one claiming depth 9 and a budget of 99, because an
    initiator PIO has not seen derives no depth and declares no budget. And
    children submitted before their lead existed spent the budget it was
    admitted with later. The grant already binds the initiator to its
    subtree's owner; that owner now has to exist.

    The owner, with no grant, may still name an initiator PIO has not seen:
    that is PIO recording a claim it cannot check, and the case records it.
    """
    daemon, socket_path, files = start_service(root, out, name='unknown',
                                               credentials=(LEAD_CREDENTIAL,),
                                               duration_ms=LEAD_ALIVE_MS)
    facts = {}
    try:
        owner = Caller(socket_path, CREDENTIAL, features=FEATURES)
        grant_id = str(uuid.uuid4())
        issued = owner.call(command(
            'core.grant.issue', dict(kind='core.grant', id=grant_id),
            dict(holder='lead', audience=PROVIDER,
                 rights=['execution.submit', 'execution.read'],
                 resources=[dict(kind='execution.execution', id_prefix='ghost.')],
                 delegation=dict(allowed=False, max_depth=0)),
            command_id=f'grant-{grant_id}'))
        assert 'result' in issued, issued
        if mutant == 'known-initiator-ok':
            assert admitted(start(owner, 'ghost', origin('ghost', 0, 2)))
        lead = Caller(socket_path, LEAD_CREDENTIAL, grant=grant_id, features=FEATURES)
        # The depth the tree would give, and the review's depth 9 with a
        # budget of 99: neither may be admitted in the name of nobody.
        for identity, depth, budget in (('ghost.a', 1, 0), ('ghost.deep', 9, 99)):
            answer = start(lead, identity, origin('ghost', depth, budget))
            assert refusal_of(answer) == 'initiator_unknown', (
                f'{identity} named an initiator PIO has never seen, under a '
                f'grant, and was not refused: {answer}')
            facts[f'{identity} (depth {depth}, budget {budget})'] = 'initiator_unknown'
        # The refused children spent nothing: the lead, admitted now with a
        # budget of one, still starts its one run.
        assert admitted(start(owner, 'ghost', origin('ghost', 0, 1)))
        assert admitted(start(lead, 'ghost.late', origin('ghost', 1, 0))), \
            'children refused before their lead existed spent its budget'
        facts['lead_budget_after_refusals'] = 'intact'
        # The owner may still record a claim PIO cannot check.
        owned = start(owner, 'elsewhere.child', origin('elsewhere', 1, 0))
        assert admitted(owned), owned
        facts['owner_naming_an_unseen_initiator'] = 'admitted, unchecked'
        lead.close()
        owner.close()
    finally:
        daemon.kill()
        daemon.wait(timeout=10)
        for handle in files:
            handle.close()
    return facts


def pass_root_runs_are_the_owners(out, root, mutant=None):
    """A root run is the owner's act; a grant starts runs only inside a
    lead's subtree.

    Owner decision, 2026-09-24 (review 45). A run naming itself as its
    initiator is a root. Under a grant it is refused `owner_authority_required`,
    whichever run the grant names: one run by id, or a subtree whose owner is
    the run being submitted. Without an origin, a grant that names a subtree
    owner refuses the submit `out_of_scope`, as before. The owner, with no
    grant, starts a root as it always has.
    """
    daemon, socket_path, files = start_service(root, out, name='roots',
                                               credentials=(LEAD_CREDENTIAL,),
                                               duration_ms=LEAD_ALIVE_MS)
    facts = {}
    try:
        owner = Caller(socket_path, CREDENTIAL, features=FEATURES)

        def grant(resource):
            grant_id = str(uuid.uuid4())
            issued = owner.call(command(
                'core.grant.issue', dict(kind='core.grant', id=grant_id),
                dict(holder='lead', audience=PROVIDER,
                     rights=['execution.submit', 'execution.read'],
                     resources=[dict(kind='execution.execution', **resource)],
                     delegation=dict(allowed=False, max_depth=0)),
                command_id=f'grant-{grant_id}'))
            assert 'result' in issued, issued
            return Caller(socket_path, LEAD_CREDENTIAL, grant=grant_id, features=FEATURES)

        solo, subtree = grant(dict(id='solo')), grant(dict(id_prefix='tree.'))
        # `--mutant owner-submits-root` sends these as the owner instead.
        by = {'solo': owner if mutant == 'owner-submits-root' else solo,
              'tree': owner if mutant == 'owner-submits-root' else subtree}
        for label, identity in (('the run a grant names by id', 'solo'),
                                ("a subtree's owner, under that subtree's grant", 'tree')):
            answer = start(by[identity], identity, origin(identity, 0, 1))
            data = answer.get('error', {}).get('data', {})
            assert (data.get('code'), (data.get('details') or {}).get('reason')) == (
                'permission_denied', 'owner_authority_required'), (
                f'{label} was started as a root under a grant: {answer}')
            facts[f'root under a grant: {label}'] = 'permission_denied / owner_authority_required'
        bare = start(solo, 'solo')
        assert bare.get('error', {}).get('data', {}).get('details', {}).get('reason') \
            == 'out_of_scope', bare
        facts['no origin, under a grant naming one run'] = 'permission_denied / out_of_scope'
        # The owner starts the same root, and the lead it becomes may then
        # start a child through the subtree grant.
        assert admitted(start(owner, 'tree', origin('tree', 0, 1)))
        child = start(subtree, 'tree.child', origin('tree', 1, 0))
        assert admitted(child), child
        facts['the same root, started by the owner'] = 'admitted, and its child under the grant'
        solo.close()
        subtree.close()
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
        case.submit(identity='lead.work', delivery_timeout=300)
        waiting = None
        for _ in range(240):
            waiting = case.inspect('lead.work')
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
        steer = command('execution.steer', subject('lead.work'),
                        dict(message=dict(digest=content_digest(note),
                                          media_type='text/plain')),
                        command_id='lead-steer',
                        revision=case.inspect('lead.work')['revision'])
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
        attempt = command('execution.respond_action', subject('lead.work'),
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
        answered = case.respond(action, 'allow',
                                case.inspect('lead.work')['revision'],
                                identity='lead.work')
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
                                             'may-answer', 'owner-steers',
                                             'bound-initiator', 'unscoped-grant',
                                             'shallow-ok', 'refused-initiator-ok',
                                             'exited-initiator-ok',
                                             'known-initiator-ok',
                                             'owner-submits-root'])
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    roots = []
    try:
        for _ in range(6):
            root = Path(tempfile.mkdtemp(prefix='pio-lead-', dir='/tmp')).resolve()
            os.chmod(root, 0o700)
            roots.append(root)
        record = dict(
            origin=pass_origin(args.out, roots[0], args.mutant),
            lead_grant=pass_lead_grant(args.out, roots[1], args.mutant),
            initiator_is_bound=pass_initiator_is_bound(args.out, roots[2],
                                                       args.mutant),
            initiator_liveness=pass_initiator_liveness(args.out, roots[3],
                                                       args.mutant),
            initiator_unknown=pass_initiator_unknown(args.out, roots[4], args.mutant),
            root_runs=pass_root_runs_are_the_owners(args.out, roots[5], args.mutant),
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
