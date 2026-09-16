//! Experimental local fake host. No native harness or Protocol capability claim.
pub mod script;
use anyhow::{Context, Result, bail, ensure};
use pio_core::{ProcessIdentity, Store};
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub const IMPLEMENTATION: &str = "experimental fake-host; no real adapter";

pub fn identity(pid: u32) -> Result<ProcessIdentity> {
    ensure!(pid > 0 && pid <= i32::MAX as u32, "invalid PID");
    #[cfg(target_os = "linux")]
    let start = {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let fields: Vec<&str> = stat
            .rsplit_once(") ")
            .context("malformed proc stat")?
            .1
            .split_whitespace()
            .collect();
        ensure!(fields.first() != Some(&"Z"), "process is a zombie");
        let ticks = fields.get(19).context("missing process starttime")?;
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        format!("linux:{}:{ticks}", boot.trim())
    };
    #[cfg(target_os = "macos")]
    let start = {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        // The buffer has the exact layout and size required by PROC_PIDTBSDINFO.
        let size = std::mem::size_of_val(&info) as i32;
        let bytes = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        ensure!(
            bytes == size && info.pbi_status != 5,
            "process identity unavailable"
        );
        let mut boot = [0u8; 128];
        let mut len = boot.len();
        let rc = unsafe {
            libc::sysctlbyname(
                c"kern.bootsessionuuid".as_ptr(),
                boot.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        ensure!(rc == 0 && len <= boot.len(), "boot identity unavailable");
        let boot = std::str::from_utf8(&boot[..len])?.trim_end_matches('\0');
        format!(
            "macos:{boot}:{}:{}",
            info.pbi_start_tvsec, info.pbi_start_tvusec
        )
    };
    Ok(ProcessIdentity { pid, start })
}

pub fn is_same_process(expected: &ProcessIdentity) -> bool {
    identity(expected.pid).is_ok_and(|current| current == *expected)
}

pub fn secure_root(root: &Path) -> Result<PathBuf> {
    if !root.exists() {
        std::fs::create_dir(root)?;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    }
    let meta = std::fs::symlink_metadata(root)?;
    ensure!(
        meta.is_dir()
            && !meta.file_type().is_symlink()
            && meta.uid() == unsafe { libc::geteuid() }
            && meta.mode() & 0o077 == 0,
        "unsafe fake store directory"
    );
    Ok(root.canonicalize()?)
}

struct Lock(File);
impl Lock {
    fn acquire(path: &Path) -> Result<Self> {
        Self::acquire_mode(path, libc::LOCK_EX | libc::LOCK_NB)
    }
    fn acquire_mode(path: &Path, mode: i32) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let result = unsafe { libc::flock(file.as_raw_fd(), mode) };
        ensure!(result == 0, "lifetime lock held");
        Ok(Self(file))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn atomic_json(path: &Path, value: &Value) -> Result<()> {
    let tmp = path.with_extension("pending");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    std::fs::rename(tmp, path)?;
    File::open(path.parent().context("missing parent")?)?.sync_all()?;
    Ok(())
}

fn append_json(path: &Path, value: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    // A marker record is one write by the child. Serialize competing children with flock.
    ensure!(
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0,
        "marker lock failed"
    );
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

pub fn same_user(stream: &UnixStream) -> bool {
    #[cfg(target_os = "linux")]
    {
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of_val(&cred) as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut size,
            )
        };
        rc == 0 && cred.uid == unsafe { libc::geteuid() }
    }
    #[cfg(target_os = "macos")]
    {
        let (mut uid, mut gid) = (0, 0);
        unsafe {
            libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) == 0 && uid == libc::geteuid()
        }
    }
}

fn read_request(stream: &mut UnixStream) -> Result<Value> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut bytes = Vec::new();
    BufReader::new(stream)
        .take(65537)
        .read_until(b'\n', &mut bytes)?;
    ensure!(
        bytes.len() <= 65536 && bytes.last() == Some(&b'\n'),
        "invalid fake frame"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn request(socket: &Path, request: &Value) -> Result<Value> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut bytes = serde_json::to_vec(request)?;
    bytes.push(b'\n');
    stream.write_all(&bytes)?;
    read_request(&mut stream)
}

fn check_launch_guards(root: &Path, store: &Store) -> Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("launch-") {
            continue;
        }
        let guard: Value = serde_json::from_slice(&std::fs::read(entry.path())?)
            .context("restore_barrier: invalid launch guard")?;
        let command = guard["command_id"]
            .as_str()
            .context("restore_barrier: invalid guard identity")?;
        let state = store
            .get(command)?
            .context("restore_barrier: launched intent missing from journal")?;
        ensure!(
            guard
                == json!({"command_id":state.command_id,"invocation_id":state.invocation_id,"store_id":state.store_id,"digest":state.digest}),
            "restore_barrier: launched intent differs from journal"
        );
    }
    Ok(())
}

fn create_launch_guard(root: &Path, state: &pio_core::Invocation) -> Result<()> {
    let path = root.join(format!("launch-{}.json", state.command_id));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                anyhow::anyhow!("duplicate_launch_guard: launch already recorded")
            } else {
                anyhow::anyhow!("launch_guard_unavailable: {error}")
            }
        })?;
    file.write_all(&serde_json::to_vec(&json!({"command_id":state.command_id,"invocation_id":state.invocation_id,"store_id":state.store_id,"digest":state.digest}))?)?;
    file.sync_all()?;
    File::open(root)?.sync_all()?;
    Ok(())
}

pub fn daemon(root: &Path) -> Result<()> {
    let root = secure_root(root)?;
    let _lock = Lock::acquire(&root.join("daemon.lock"))?;
    let controller_gate = Lock::acquire_mode(&root.join("controller-gate.lock"), libc::LOCK_EX)?;
    let mut store = Store::open(&root)?;
    check_launch_guards(&root, &store)?;
    let before = store.identity()?;
    let witness = root.join("controller-witness.json");
    if witness.exists() {
        let recorded: Value = serde_json::from_slice(&std::fs::read(&witness)?)?;
        ensure!(
            recorded == json!({"store_id":before.0,"generation":before.1}),
            "restore_barrier: journal differs from external controller witness"
        );
    } else {
        ensure!(before.1 == 0, "restore_barrier: controller witness missing");
    }
    let (store_id, generation) = store.advance_controller()?;
    atomic_json(
        &witness,
        &json!({"store_id":store_id,"generation":generation}),
    )?;
    drop(controller_gate);
    let socket = root.join("daemon.sock");
    if let Ok(meta) = std::fs::symlink_metadata(&socket) {
        ensure!(
            meta.file_type().is_socket(),
            "refusing to remove non-socket"
        );
        std::fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    println!(
        "{}",
        json!({"source":"fake-host","ready":true,"controller_generation":generation,"store_id":store_id})
    );
    for stream in listener.incoming() {
        let mut stream = stream?;
        if !same_user(&stream) {
            continue;
        }
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let result = read_request(&mut stream)
            .and_then(|request| dispatch(&root, &mut store, generation, &request));
        let result = match result {
            Ok(value) => value,
            Err(e) => json!({"source":"fake-host","error":e.to_string()}),
        };
        let mut bytes = serde_json::to_vec(&result)?;
        bytes.push(b'\n');
        let _ = stream.write_all(&bytes); // Lost caller does not stop service or host.
    }
    Ok(())
}

fn dispatch(root: &Path, store: &mut Store, generation: u64, request: &Value) -> Result<Value> {
    let operation = request["op"].as_str().context("missing op")?;
    if operation == "status" {
        return Ok(json!({"source":"fake-host","controller_generation":generation}));
    }
    if operation == "journal" {
        return Ok(json!({"source":"fake-host","records":store.journal()?}));
    }
    let command = request["id"].as_str().context("missing id")?;
    match operation {
        "submit" => {
            let payload = request.get("payload").cloned().unwrap_or(json!({}));
            pio_core::require_payload(&payload)?;
            let fault = request["fault"].as_str().unwrap_or("");
            ensure!(
                matches!(
                    fault,
                    "" | "journal_failure"
                        | "after_intent"
                        | "after_claim"
                        | "after_release"
                        | "after_receipt"
                        | "duplicate_launch"
                        | "replay_relaunch"
                        | "replay_without_launch_guard"
                        | "replay_without_host_phase"
                        | "before_release"
                ),
                "unknown fake fault"
            );
            let requested_generation = request
                .get("generation")
                .map(|v| v.as_u64().context("invalid generation"))
                .transpose()?
                .unwrap_or(generation);
            check_launch_guards(root, store)?;
            let (invocation, inserted) = store.admit(
                command,
                payload,
                requested_generation,
                fault == "journal_failure",
            )?;
            let mutated_replay = matches!(
                fault,
                "replay_relaunch" | "replay_without_launch_guard" | "replay_without_host_phase"
            );
            let mut attempt = None;
            if inserted || mutated_replay {
                if fault == "after_intent" {
                    std::process::exit(91);
                }
                if !matches!(
                    fault,
                    "replay_without_launch_guard" | "replay_without_host_phase"
                ) {
                    create_launch_guard(root, &invocation)?;
                }
                let attempt_id = uuid::Uuid::new_v4().to_string();
                attempt = Some(attempt_id.clone());
                let stderr_path = root.join(format!("attempt-{attempt_id}.stderr"));
                let stderr_file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .mode(0o600)
                    .open(&stderr_path)?;
                let outcome_path = root.join(format!("attempt-{attempt_id}.json"));
                let mut child = Command::new(std::env::current_exe()?)
                    .args(["fake", "host"])
                    .arg(root)
                    .arg(command)
                    .arg(&invocation.invocation_id)
                    .arg(fault)
                    .env_clear()
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::from(stderr_file))
                    .spawn()?;
                std::thread::spawn(move || {
                    let result = child.wait();
                    let error = std::fs::read_to_string(&stderr_path).unwrap_or_default();
                    let _ = atomic_json(
                        &outcome_path,
                        &json!({"source":"fake-host","exit_code":result.ok().and_then(|s|s.code()),"reason":error.trim().strip_prefix("PIO: ").unwrap_or(error.trim())}),
                    );
                });
            }
            Ok(
                json!({"source":"fake-host","replay":!inserted,"launch_attempt":attempt,"launch_decision":if inserted || mutated_replay {"attempted"}else{"admit_replay_suppressed"},"invocation":invocation,"controller_generation":generation}),
            )
        }
        "inspect" => {
            let state = store.get(command)?.context("not_found")?;
            let host_alive = state.host.as_ref().is_some_and(is_same_process);
            let child_alive = state.child.as_ref().is_some_and(is_same_process);
            let recovery = if state.phase == "known_not_released" {
                "known_not_released"
            } else if matches!(state.phase.as_str(), "intent" | "host_claimed" | "parked") {
                "not_released_pending"
            } else if state.receipt.is_some() {
                "receipt_recorded"
            } else if child_alive && host_alive {
                "same_process_observed"
            } else {
                "uncertain_no_respawn"
            };
            Ok(
                json!({"source":"fake-host","invocation":state,"controller_generation":generation,"host_alive":host_alive,"child_alive":child_alive,"recovery":recovery}),
            )
        }
        _ => bail!("unknown fake operation"),
    }
}

pub fn host(root: &Path, command: &str, invocation_id: &str, fault: &str) -> Result<()> {
    // A terminal session ending must not send its hangup to the durable host.
    ensure!(
        unsafe { libc::setsid() } != -1,
        "cannot detach fake host session"
    );
    let root = secure_root(root)?;
    let mut store = Store::open(&root)?;
    let mut state = store.get(command)?.context("missing invocation")?;
    ensure!(
        state.invocation_id == invocation_id,
        "invocation_identity_fenced"
    );
    check_launch_guards(&root, &store)?;
    if fault != "replay_without_host_phase" {
        ensure!(
            state.phase == "intent",
            "host_phase_fence: launch attempt already recorded"
        );
    }
    let _slot = Lock::acquire(&root.join(format!("slot-{}.lock", state.host_slot)))
        .map_err(|_| anyhow::anyhow!("host_slot_fence: slot already owned"))?;
    state = store.transition(
        &state,
        "host_claimed",
        Some(identity(std::process::id())?),
        None,
        None,
    )?;
    if fault == "after_claim" {
        std::process::exit(92);
    }
    let mut children = Vec::new();
    let count = if fault == "duplicate_launch" { 2 } else { 1 };
    for _ in 0..count {
        let child = Command::new(std::env::current_exe()?)
            .args(["fake", "child"])
            .arg(&root)
            .arg(command)
            .arg(invocation_id)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        children.push(child);
    }
    let mut release_attempted = false;
    let outcome = (|| -> Result<()> {
        // Child is parked behind stdin. Capture kernel identity before releasing work.
        let child_id = identity(children[0].id())?;
        state = store.transition(&state, "parked", None, Some(child_id), None)?;
        if fault == "before_release" {
            atomic_json(
                &root.join("before-release.ready"),
                &json!({"source":"fake-host","invocation_id":state.invocation_id}),
            )?;
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !root.join("before-release.continue").exists() {
                ensure!(
                    std::time::Instant::now() < deadline,
                    "fake release barrier timeout"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let release_gate = Lock::acquire_mode(&root.join("controller-gate.lock"), libc::LOCK_SH)?;
        state = store.transition(&state, "released", None, None, None)?;
        release_attempted = true;
        for child in &mut children {
            child
                .stdin
                .as_mut()
                .context("missing child pipe")?
                .write_all(b"GO\n")?;
        }
        drop(release_gate);
        if fault == "after_release" {
            std::process::exit(93);
        }
        let spool_path = root.join(format!("output-{}.jsonl", state.invocation_id));
        let mut spool = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&spool_path)?;
        let mut statuses = Vec::new();
        for child in &mut children {
            let mut output = child.stdout.take().context("missing output")?;
            std::io::copy(&mut output, &mut spool)?;
            statuses.push(child.wait()?.code());
        }
        spool.sync_all()?;
        let bytes = std::fs::read(&spool_path)?;
        let receipt = json!({"source":"fake-host","kind":"observed_process_exit","exit_codes":statuses,"output_digest":pio_core::digest(&bytes),"output_bytes":bytes.len(),"completion_is_acceptance":false});
        store.transition(&state, "completed", None, None, Some(receipt))?;
        if fault == "after_receipt" {
            std::process::exit(94);
        }
        Ok(())
    })();
    if outcome.is_err() {
        // Use owned Child handles, never a bare PID from a journal lookup.
        let mut reaped = true;
        for child in &mut children {
            let _ = child.kill();
            reaped &= child.wait().is_ok();
        }
        let marker = root.join(format!("release-{}.jsonl", state.invocation_id));
        if !release_attempted
            && reaped
            && !marker.exists()
            && matches!(state.phase.as_str(), "host_claimed" | "parked")
        {
            store.transition(&state,"known_not_released",None,None,Some(json!({"source":"fake-host","kind":"known_not_released","release_attempted":false,"children_reaped":true,"release_marker_absent":true,"reason":outcome.as_ref().err().map(ToString::to_string)})))?;
        }
    }
    outcome
}

pub fn child(root: &Path, command: &str, invocation_id: &str) -> Result<()> {
    let root = secure_root(root)?;
    // Independent child-side observation before opening or trusting the journal.
    let me = identity(std::process::id())?;
    append_json(
        &root.join("spawn.jsonl"),
        &json!({"source":"fake-host","kind":"child_started","identity":me,"command_id":command}),
    )?;
    let store = Store::open(&root)?;
    let state = store.get(command)?.context("missing child invocation")?;
    ensure!(
        state.invocation_id == invocation_id,
        "invocation_identity_fenced"
    );
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    ensure!(line == "GO\n", "fake launch barrier not released");
    append_json(
        &root.join(format!("release-{}.jsonl", state.invocation_id)),
        &json!({"source":"fake-host","kind":"prompt_released","identity":me,"invocation_id":state.invocation_id}),
    )?;
    println!(
        "{}",
        json!({"source":"fake-host","kind":"output","identity":me,"text":"deterministic fake work started"})
    );
    // Deliberate fake work duration, not readiness detection.
    std::thread::sleep(Duration::from_millis(pio_core::require_payload(
        &state.payload,
    )?));
    println!(
        "{}",
        json!({"source":"fake-host","kind":"output","text":"deterministic fake work ended"})
    );
    Ok(())
}
