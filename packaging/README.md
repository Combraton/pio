# M1 private packaging and user-job prototypes

This skeleton installs the **labeled fake-process service**. It does not install a native adapter, qualify a release, auto-enable login startup, or prove a product journey. Fresh install/update qualification remains M6. Python 3 and the built native binary are required.

## Layout and collision policy

Choose an explicit private prefix and an existing alias directory. `scripts/package.py install` creates:

- `<prefix>/bin/pio`: the native executable, mode 0700;
- `<prefix>/install.json` and `install.sha256`: ownership manifest, binary SHA-256, alias layout and manifest-integrity receipt;
- `<bin-dir>/pio-standalone`: an exclusively created symlink to the private executable.

The default alias is `pio-standalone`. Any existing prefix, destination, or executable of the chosen alias name anywhere on PATH causes `placement_collision` before writes. `--bin-name pio` is opt-in and refuses if the unrelated historical `pio` is present. There is no force/overwrite/upgrade option. Parent symlinks are refused; all destination file creation is exclusive. Interrupted installation may require inspection of the private prefix; this skeleton is not a transactional updater.

```sh
mkdir -p "$HOME/.local/share" "$HOME/.local/bin"
python3 scripts/package.py install --binary target/debug/pio \
  --prefix "$HOME/.local/share/pio-m1" --bin-dir "$HOME/.local/bin"
```

Uninstall validates manifest integrity, the recorded alias layout, binary hash and exact alias target before deleting any owned file. The receipt detects accidental manifest changes; it is not a security boundary against a user who can rewrite the installation. Modified files cause refusal with no deletion. It removes only the manifest/receipt, matching payload and matching alias, then empty owned directories. Unowned additions, config and durable state are preserved. Stop/bootout the user job before uninstalling. Deletion is not transactional across filesystems: a later I/O failure or interruption can leave a partial uninstall, which requires inspection. The no-deletion guarantee applies to integrity/collision refusals detected in preflight, not arbitrary storage failures.

```sh
python3 scripts/package.py uninstall --prefix "$HOME/.local/share/pio-m1"
```

## User-job prototypes

`render-job` creates a new file exclusively and renders absolute paths. Supply an existing private state directory, a private fake-service configuration (see [verification](../docs/VERIFICATION.md)), and a socket below a private directory:

```sh
python3 scripts/package.py render-job --prefix /absolute/private/install \
  --config /absolute/private/service.json --data-dir /absolute/private/state \
  --socket /absolute/private/state/pio.sock --label io.combraton.pio.prototype \
  --kind launchd --output /absolute/private/service.plist
# On Linux use --kind systemd --output /absolute/private/pio.service.
```

The launchd plist uses `ProgramArguments`, `RunAtLoad` and `KeepAlive`. For a manually installed persistent user agent, the conventional location is `~/Library/LaunchAgents`; the prototype test instead bootstraps its temporary plist into the existing `gui/<uid>` domain and bootouts that exact label. See [Apple's launchd guide](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html).

The systemd unit is `Type=simple`, with `Restart=on-failure` and `KillMode=process`: stopping the controller must not kill its independently owned execution host. This is intentional and not a claim of cgroup sandboxing. The conventional persistent user-unit location is `~/.config/systemd/user`; the test uses `$XDG_RUNTIME_DIR/systemd/user` and never enables it. See [systemctl](https://www.freedesktop.org/software/systemd/man/latest/systemctl.html) and [systemd.kill](https://www.freedesktop.org/software/systemd/man/latest/systemd.kill.html). Durable execution survival is tested separately by the public matrix.

## Reproducible lifecycle check

```sh
python3 scripts/packaging_check.py --out target/packaging
```

The same command runs on macOS arm64 and Linux x86_64 CI. It installs into `/tmp`, uses the actual user manager, queries the authenticated Unix API, records kernel process-start identity, stops the service and proves that identity is gone. It then checks modified-file refusal and uninstall preservation. Artifacts include generated job, install manifest, command receipts, schema-checked public transcript and report. On the disposable Ubuntu CI VM, the workflow first runs `sudo systemctl start "user@$(id -u).service"` and sets `XDG_RUNTIME_DIR=/run/user/$(id -u)` so the runner has its actual systemd user manager. This is CI environment setup, not a root PIO service or an installer action. On a workstation, use an existing user login session. An unavailable user manager fails the test; it never substitutes a direct process launch. Test labels and runtime units are removed in cleanup. Temporary data remains for diagnosis outside the repository. The test credential is synthetic and transcripts redact it.
