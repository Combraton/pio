#!/usr/bin/env python3
"""Offline Claude adapter matrix: the pre-spawn admission decisions and the
stream shapes, driven against the labeled fake CLI. Never a real Claude Code,
never a live run, and no model call at any point.

Independent witnesses: the fake CLI's own marker file (turn received, the
permission decision it was given, whether that decision carried a widening
field), the admission record the service produces before any stream starts,
and the process exit status. Every execution is labeled `pio-fake-claude-cli`.

Each case runs three times; a case passes only when all three agree.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'
STREAM_ARGS = ['--print', '--input-format', 'stream-json', '--output-format', 'stream-json',
               '--verbose', '--replay-user-messages']
CASES = [
    'turn_completes',
    'replay_acknowledges_delivery',
    'permission_denied',
    'permission_allowed',
    'out_of_fixture_request_declined_by_pio',
    'unclassifiable_request_surfaced_not_auto_allowed',
    'widening_decision_never_sent',
    'unqualified_executable_refused',
    'missing_credential_route_refused',
    'permission_mode_not_the_configured_default_refused',
    'surface_drift_refused',
]


class Case:
    """One offline case: a fixture workspace, a labeled fake, and a settings
    file standing in for the user's own."""

    def __init__(self, out, name, scenario=None, settings=None, permission_mode='acceptEdits',
                 labeled_fake=True, executable=None):
        self.name = name
        self.out = out / name
        self.out.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix='pio-cl-', dir='/tmp')).resolve()
        os.chmod(self.root, 0o700)
        self.fixtures = self.root / 'fixtures'
        self.config_dir = self.root / 'claude-config'
        self.markers = self.root / 'markers'
        self.work = self.root / 'work'
        for directory in (self.fixtures, self.config_dir, self.markers, self.work):
            directory.mkdir()
        (self.fixtures / 'README.md').write_text('PIO M3 offline fixture. No task runs here.\n')
        # Stands in for the user's settings: an accept-edits default, so a
        # request for anything else must be refused before a spawn.
        (self.config_dir / 'settings.json').write_text(json.dumps(
            settings if settings is not None else
            {'permissions': {'defaultMode': 'acceptEdits', 'allow': ['Bash(cat)']}}))
        self.wrapper = self.root / 'fake-claude'
        self.wrapper.write_text(f"#!/bin/sh\nexec '{BINARY}' claude fake-cli \"$@\"\n")
        self.wrapper.chmod(0o755)
        self.scenario = dict(scenario or {}, markers=str(self.markers))
        self.config_path = self.root / 'service.json'
        self.config_path.write_text(json.dumps({'claude': {
            'executable': str(executable or self.wrapper),
            'env': {'PATH': '/usr/bin:/bin', 'HOME': str(self.root), 'USER': os.environ.get('USER', 'pio')},
            'config_dir': str(self.config_dir), 'home': str(self.root),
            'fixture_root': str(self.fixtures),
            'permission_mode': permission_mode, 'labeled_fake': labeled_fake}}))
        if labeled_fake:
            # The fake's scenario travels in the service environment, which the
            # admission check accepts only because `labeled_fake` is true.
            config = json.loads(self.config_path.read_text())
            config['claude']['env']['PIO_CLAUDE_FAKE_SCENARIO'] = json.dumps(self.scenario)
            self.config_path.write_text(json.dumps(config))

    def env(self):
        return {'PATH': '/usr/bin:/bin', 'HOME': str(self.root),
                'USER': os.environ.get('USER', 'pio'),
                'PIO_CLAUDE_FAKE_SCENARIO': json.dumps(self.scenario)}

    def admit(self):
        """The decisions a service makes before it spawns anything for a turn."""
        result = subprocess.run(
            [str(BINARY), 'claude', 'service-admit', '--config', str(self.config_path),
             '--work', str(self.work)],
            capture_output=True, text=True, env=self.env(), timeout=120)
        record = json.loads(result.stdout) if result.stdout.strip() else {}
        (self.out / 'admission.json').write_text(json.dumps(record, indent=2, sort_keys=True))
        return result.returncode, record

    def turn(self, decision=None, brief='do the fixture task'):
        """Drive one turn against the fake, answering any permission request."""
        argv = [str(self.wrapper), *STREAM_ARGS, '--permission-mode', 'acceptEdits']
        child = subprocess.Popen(argv, cwd=str(self.fixtures), env=self.env(),
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 text=True, bufsize=1)
        sent = {'type': 'user', 'message': {'role': 'user',
                                            'content': [{'type': 'text', 'text': brief}]}}
        child.stdin.write(json.dumps(sent) + '\n')
        child.stdin.flush()
        messages = []
        for line in child.stdout:
            if not line.strip():
                continue
            message = json.loads(line)
            messages.append(message)
            if message.get('type') == 'control_request' and decision is not None:
                child.stdin.write(json.dumps({'type': 'control_response', 'response': {
                    'subtype': 'success', 'request_id': message['request_id'],
                    'response': decision}}) + '\n')
                child.stdin.flush()
        child.stdin.close()
        child.wait(timeout=30)
        (self.out / 'transcript.jsonl').write_text(
            ''.join(json.dumps(m) + '\n' for m in messages))
        return sent, messages

    def markers_of(self, event):
        path = self.markers / 'fake-claude-cli.jsonl'
        if not path.exists():
            return []
        return [json.loads(line) for line in path.read_text().splitlines()
                if line.strip() and json.loads(line)['event'] == event]

    def cleanup(self):
        shutil.rmtree(self.root, ignore_errors=True)


def kinds(messages):
    return [m['type'] + ('/' + m['subtype'] if m.get('subtype') else '') for m in messages]


def run_case(out, name):
    """Each case asserts a named property and returns nothing; an assertion
    failure is the report."""
    if name == 'turn_completes':
        case = Case(out, name)
        sent, messages = case.turn()
        assert kinds(messages) == ['system/init', 'user', 'assistant', 'result/success'], kinds(messages)
        assert messages[-1]['is_error'] is False, messages[-1]
        assert case.markers_of('turn_complete'), 'the fake never recorded a completed turn'

    elif name == 'replay_acknowledges_delivery':
        case = Case(out, name)
        sent, messages = case.turn(brief='a brief that must come back byte for byte')
        replay = [m for m in messages if m.get('isReplay')]
        assert len(replay) == 1, kinds(messages)
        # The delivery proof is an exact echo, not a receipt the harness invented.
        assert replay[0]['message'] == sent['message'], replay[0]

    elif name == 'permission_denied':
        case = Case(out, name, scenario={
            'permission_request': {'tool_name': 'Bash', 'input': {'command': 'ls /etc'}}})
        _, messages = case.turn(decision={'behavior': 'deny', 'message': 'outside the fixture'})
        assert 'control_request' in kinds(messages), kinds(messages)
        assert messages[-1]['permission_denials'] == 1, messages[-1]
        recorded = case.markers_of('permission_decision')
        assert [r['behavior'] for r in recorded] == ['deny'], recorded
        assert recorded[0]['widening_fields_received'] == [], recorded

    elif name == 'permission_allowed':
        command = {'command': 'cat README.md'}
        case = Case(out, name, scenario={
            'permission_request': {'tool_name': 'Bash', 'input': command}})
        _, messages = case.turn(decision={'behavior': 'allow', 'updatedInput': command})
        assert messages[-1]['permission_denials'] == 0, messages[-1]
        recorded = case.markers_of('permission_decision')
        assert [r['behavior'] for r in recorded] == ['allow'], recorded
        # An allow must echo the input unchanged: rewriting it would alter the
        # tool call the user's harness decided to make.
        assert recorded[0]['input_echoed_unchanged'] is True, recorded

    elif name == 'out_of_fixture_request_declined_by_pio':
        # PIO classifies the request and encodes the decision. Until the service
        # binding lands the script only *transports* that decision to the fake;
        # it does not choose it. The traversal is the point: a prefix test would
        # have called this target contained.
        case = Case(out, name)
        (case.root / 'outside').mkdir(exist_ok=True)
        (case.root / 'outside' / 'secret.txt').write_text('not yours\n')
        # Written the way a prefix test gets *wrong*: an absolute path that
        # begins with the fixture and then climbs out of it. A relative
        # `../outside/...` would be declined even by the broken classifier, so
        # it would not prove anything.
        escape = f'{case.fixtures}/../outside/secret.txt'
        case.scenario = dict(case.scenario,
                             permission_request={'tool_name': 'Read',
                                                 'input': {'file_path': escape}},
                             tool_uses=[{'name': 'Read', 'input': {'file_path': escape}}])
        request = {'type': 'control_request', 'request_id': 'req_1_fake',
                   'request': {'subtype': 'can_use_tool', 'tool_name': 'Read',
                               'input': {'file_path': escape},
                               'tool_use_id': 'toolu_fake_1'}}
        classification = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'classify-request', '--fixture', str(case.fixtures),
             '--cwd', str(case.fixtures)],
            input=json.dumps(request), capture_output=True, text=True, check=True).stdout)
        assert classification['disposition'] == 'decline', classification
        assert classification['reason'] == 'target_outside_the_fixture_workspace', classification
        assert classification['target_label'] == '<outside>', classification
        assert classification['auto_allowed'] is False, classification
        encoded = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'encode-decision', '--behavior', 'deny',
             '--reason', classification['reason']],
            input=json.dumps(request), capture_output=True, text=True, check=True).stdout)
        _, messages = case.turn(decision=encoded['envelope']['response']['response'])
        assert messages[-1]['permission_denials'] == 1, messages[-1]
        recorded = case.markers_of('permission_decision')
        assert [r['behavior'] for r in recorded] == ['deny'], recorded
        # The decline covers the prompt. The tool use is still recorded, because
        # a target outside the fixture is an observed effect with unresolved
        # liability, not a containment claim. ADR 004 §5.
        uses = [b for m in messages if m.get('type') == 'assistant'
                for b in m['message']['content'] if b.get('type') == 'tool_use']
        assert len(uses) == 1, uses
        record = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'tool-uses', '--fixture', str(case.fixtures),
             '--cwd', str(case.fixtures)],
            input=''.join(json.dumps(m) + '\n' for m in messages),
            capture_output=True, text=True, check=True).stdout)
        assert record['out_of_fixture_effect_observed'] is True, record
        assert record['liability'] == 'unresolved', record
        assert record['tool_uses'][0]['target_label'] == '<outside>', record
        # A receipt carries labels and digests, never raw paths.
        assert str(case.fixtures) not in json.dumps(record), record

    elif name == 'unclassifiable_request_surfaced_not_auto_allowed':
        # A shell command names no path PIO can resolve. It is surfaced to the
        # caller, never auto-allowed, and the run stops rather than guessing.
        case = Case(out, name)
        request = {'type': 'control_request', 'request_id': 'req_1_fake',
                   'request': {'subtype': 'can_use_tool', 'tool_name': 'Bash',
                               'input': {'command': 'cat README.md'},
                               'tool_use_id': 'toolu_fake_1'}}
        classification = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'classify-request', '--fixture', str(case.fixtures),
             '--cwd', str(case.fixtures)],
            input=json.dumps(request), capture_output=True, text=True, check=True).stdout)
        assert classification['disposition'] == 'surface_as_action', classification
        assert classification['placement'] == 'not_classifiable', classification
        assert classification['auto_allowed'] is False, classification
        assert classification['target_label'] is None, classification

    elif name == 'widening_decision_never_sent':
        command = {'command': 'cat README.md'}
        case = Case(out, name, scenario={
            'permission_request': {'tool_name': 'Bash', 'input': command}})
        # The harness offers a rule update in every request. The decision PIO
        # forwards is built by the adapter, which cannot encode one.
        request = {'type': 'control_request', 'request_id': 'req_1_fake',
                   'request': {'subtype': 'can_use_tool', 'tool_name': 'Bash', 'input': command,
                               'permission_suggestions': [{'type': 'addRules',
                                                           'destination': 'userSettings'}]}}
        encoded = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'encode-decision', '--behavior', 'allow'],
            input=json.dumps(request), capture_output=True, text=True, check=True).stdout)
        decision = encoded['envelope']['response']['response']
        assert encoded['suggestions_offered'] == 1 and encoded['suggestions_acted_on'] == 0, encoded
        _, messages = case.turn(decision=decision)
        recorded = case.markers_of('permission_decision')
        # The fake would have recorded a widening field had one arrived; this
        # case is only evidence because that detector is proven to work.
        assert recorded[0]['widening_fields_received'] == [], recorded
        assert recorded[0]['input_echoed_unchanged'] is True, recorded

    elif name == 'unqualified_executable_refused':
        # A real (non-fake) configuration pointed at an executable that is not
        # the qualified Claude Code.
        case = Case(out, name, labeled_fake=False)
        status, record = case.admit()
        assert status == 3, record
        reasons = [r['reason'] for r in record['refusals']]
        assert 'claude_not_qualified' in reasons, record
        assert record['stream_spawned'] is False, record

    elif name == 'missing_credential_route_refused':
        case = Case(out, name, scenario={'route': None})
        status, record = case.admit()
        assert status == 3, record
        reasons = [r['reason'] for r in record['refusals']]
        assert reasons == ['missing_credential_route'], record
        assert record['credential_route']['observed']['loggedIn'] is False, record
        assert record['stream_spawned'] is False, record

    elif name == 'permission_mode_not_the_configured_default_refused':
        for requested in ('bypassPermissions', 'plan', 'dontAsk', 'default'):
            case = Case(out, f'{name}-{requested}', permission_mode=requested)
            status, record = case.admit()
            assert status == 3, record
            reasons = [r['reason'] for r in record['refusals']]
            assert 'permission_mode_refused' in reasons, record
            assert record['permission_mode']['configured'] == 'acceptEdits', record
            assert record['stream_spawned'] is False, record
            case.cleanup()
        # The configured default itself is admitted.
        case = Case(out, name)
        status, record = case.admit()
        assert status == 0, record
        assert record['permission_mode']['allowed'] is True, record

    elif name == 'surface_drift_refused':
        # A genuine drift, not merely an unqualified executable: pin the fake's
        # own surface as the baseline, then move one help and show the refusal
        # names the command that changed.
        case = Case(out, name, scenario={'help_suffix': ' drifted'})
        baseline = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'surface-identity', '--executable', str(case.wrapper),
             '--work', str(case.work / 'baseline'), '--fake-scenario', '{}'],
            capture_output=True, text=True, check=True, env=case.env()).stdout)
        baseline_path = case.root / 'baseline-surface.json'
        baseline_path.write_text(json.dumps(baseline))

        def qualify(scenario):
            work = case.work / f'q{abs(hash(scenario))}'
            result = subprocess.run(
                [str(BINARY), 'claude', 'qualify', '--executable', str(case.wrapper),
                 '--work', str(work), '--expected', str(baseline_path),
                 '--fake-scenario', scenario],
                capture_output=True, text=True, env=case.env())
            return result.returncode, json.loads(result.stdout)

        status, unchanged = qualify('{}')
        assert status == 0 and unchanged['qualified'] is True, unchanged
        assert unchanged['surface']['drift_count'] == 0, unchanged

        status, drifted = qualify('{"help_suffix":" drifted"}')
        assert status == 3, drifted
        assert drifted['qualified'] is False, drifted
        assert drifted['refusals'] == [{'reason': 'surface_drift', 'commands': 8}], drifted
        # Every command's help moved, and each is named.
        assert {d['change'] for d in drifted['surface']['drift']} == {'changed'}, drifted
        (case.out / 'drift.json').write_text(json.dumps(drifted, indent=2))

    else:
        raise AssertionError(f'unknown case {name}')
    case.cleanup()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--case', action='append')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    selected = args.case or CASES
    results, failures = [], []
    for name in selected:
        for repetition in range(args.repetitions):
            out = args.out / f'{name}-{repetition}'
            try:
                run_case(out, name)
                results.append((name, repetition, 'pass', None))
            except Exception as error:
                results.append((name, repetition, 'fail', repr(error)))
                failures.append((name, repetition, repr(error)))
    summary = dict(format='pio-claude-host-matrix/1', platform=platform.platform(),
                   harness='pio-fake-claude-cli', model_calls=0, live_run=False,
                   cases=len(selected), repetitions=args.repetitions,
                   attempts=len(results), failures=len(failures),
                   results=[dict(case=c, repetition=r, status=s, error=e) for c, r, s, e in results])
    (args.out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    for name, repetition, error in failures:
        print(f'FAIL {name}#{repetition}: {error}')
    print(f"{len(selected)} cases x {args.repetitions} = {len(results)} attempts, "
          f"{len(failures)} failures")
    raise SystemExit(1 if failures else 0)


if __name__ == '__main__':
    main()
