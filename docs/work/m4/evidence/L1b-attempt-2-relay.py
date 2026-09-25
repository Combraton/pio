"""L1b attempt 2's desk relay.

Answers exactly one request, the one the owner decided in advance (decision
of 2026-09-25, item 3), and brings every other to the owner. Its argv never
names the live tree: the tree is read from the runner's log, and polled with
os.listdir and open(), with no subprocess. The cleanup kills every process
group whose argv names the tree.
"""
import datetime
import json
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
WORDS = ("Owner decision, 2026-09-25 (after review 50), item 3, quoted: \"when "
         "L1b.beta asks to read <fixture>/beta.env with OpenCode's read tool, "
         "answer allow (sent as Allow once) as soon as it arrives\". Applied by the "
         "builder's relay as soon as the request arrived.")
pid = int(open(os.path.join(HERE, 'runner.pid')).read())
done_path = os.path.join(HERE, 'answered.txt')
seen = set(open(done_path).read().split()) if os.path.exists(done_path) else set()


def alive(p):
    try:
        os.kill(p, 0)
        return True
    except ProcessLookupError:
        return False


def decided_in_advance(item):
    c = (item.get('approval') or {}).get('classification') or {}
    return (item.get('run') == 'L1b.beta' and c.get('tool_name') == 'read'
            and c.get('target_label') == '<fixture>/beta.env'
            and c.get('placement') == 'inside_fixture')


while True:
    if not alive(pid):
        print('RUNNER EXITED')
        sys.exit(0)
    root = None
    for line in open(os.path.join(HERE, 'runner.log'), errors='replace'):
        if line.startswith('ROOT '):
            root = line[5:].strip()
            break
    desk = os.path.join(root, 'desk') if root else None
    if desk and os.path.isdir(desk):
        names = os.listdir(desk)
        for name in sorted(names):
            if not (name.startswith('pending-') and name.endswith('.json')):
                continue
            action = name[len('pending-'):-len('.json')]
            if action in seen or f'answer-{action}.json' in names:
                continue
            item = json.load(open(os.path.join(desk, name)))
            if decided_in_advance(item):
                answer = dict(decision='allow', decided_by='owner', words=WORDS)
                target = os.path.join(desk, f'answer-{action}.json')
                with open(target + '.tmp', 'w') as out:
                    json.dump(answer, out)
                os.replace(target + '.tmp', target)
                seen.add(action)
                with open(done_path, 'a') as out:
                    out.write(action + '\n')
                stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%H:%M:%SZ')
                print(f'ANSWERED IN ADVANCE {action} allow at {stamp}', flush=True)
                continue
            print('DESK PENDING', action)
            print(json.dumps(item, indent=2))
            sys.exit(0)
    time.sleep(0.5)
