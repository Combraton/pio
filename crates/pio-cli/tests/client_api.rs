//! `pio-client` against a real `pio serve-fake`, over the public socket and
//! nothing else: every operation the client covers, and the refusals that
//! make its fences mean something.
use pio_client::{
    Client, Credential, Failure, Fence, Options, Pinned, Position, Reconcile, execution,
};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const OWNER: &str = "ccred1.owner.mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm";
const BOARD: &str = "ccred1.board.bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HOST: &str = "durable-fake-host";

struct Service {
    child: Child,
    socket: PathBuf,
    _root: tempfile::TempDir,
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn service(duration_ms: u64) -> Service {
    // A short path: a Unix socket path is limited to about 104 bytes.
    let root = tempfile::Builder::new()
        .prefix("pio-ca-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = root.path().join("service.json");
    std::fs::write(
        &config,
        serde_json::to_vec(&json!({
            "format": "pio-fake-service/1",
            "protocol": {"format": "combraton-conformance-config/1", "principal": "owner",
                         "provider_id": "conformance-provider",
                         "credentials": [{"credential": OWNER}, {"credential": BOARD}],
                         "executor": {"host_id": HOST}},
            "fake_host": {"duration_ms": duration_ms, "fault": ""}}))
        .unwrap(),
    )
    .unwrap();
    let socket = root.path().join("public.sock");
    let child = Command::new(env!("CARGO_BIN_EXE_pio"))
        .args(["serve-fake", "--data-dir"])
        .arg(root.path().join("store"))
        .arg("--config")
        .arg(&config)
        .arg("--socket")
        .arg(&socket)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let service = Service {
        child,
        socket,
        _root: root,
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if connect(&service.socket, OWNER, None).is_ok() {
            return service;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("serve-fake did not become ready");
}

fn connect(socket: &Path, credential: &str, grant: Option<&str>) -> Result<Client, Failure> {
    Client::connect(
        socket,
        &Credential::parse(credential).unwrap(),
        &Options {
            grant: grant.map(str::to_owned),
            ..Options::default()
        },
    )
}

fn submit_envelope(id: &str) -> Value {
    let mut command = pio_client::Command::new(
        "execution.submit",
        execution(id),
        0,
        json!({"brief": {"digest": pio_client::digest(format!("fake work {id}").as_bytes()),
                         "media_type": "text/plain"}}),
    );
    command.command_id = id.into();
    command.envelope()
}

fn refused(reply: Result<Value, Failure>) -> pio_client::Refusal {
    match reply {
        Err(Failure::Refused(refusal)) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn every_operation_goes_over_the_public_socket() {
    let service = service(1500);
    let mut owner = connect(&service.socket, OWNER, None).unwrap();
    assert_eq!(owner.principal(), "owner");
    assert_eq!(
        owner.described["provider"]["name"],
        "pio-journal-fake-executor"
    );
    assert!(owner.describe().unwrap()["profiles"].is_array());
    assert!(owner.capabilities().unwrap()["predicates"].is_array());
    assert!(owner.discovery().unwrap()["installations"].is_array());

    // A subscription pushes; the client keeps the push rather than drop it.
    let subscribed = owner
        .events_subscribe(&Position::Now, &["execution.execution"])
        .unwrap();
    let subscription = subscribed["subscription"].as_str().unwrap().to_owned();
    let admitted = owner.submit(&submit_envelope("run-1")).unwrap();
    assert_eq!(admitted["outcome"]["admission"], "admitted");
    let pushed = owner
        .notification(Duration::from_secs(10))
        .unwrap()
        .expect("a notification for the new run");
    assert_eq!(pushed["subscription"], subscription.as_str());
    assert!(!pushed["items"].as_array().unwrap().is_empty());
    owner.events_unsubscribe(&subscription).unwrap();

    until("run-1 to exit", || {
        owner.inspect("run-1").unwrap()["runtime"] == "exited"
    });
    let chunk = owner.output_read("run-1", 0, 65536).unwrap();
    assert!(!chunk.data.is_empty());
    assert_eq!(chunk.next_offset, chunk.end_offset);
    assert_eq!(chunk.coverage, "complete");
    let rest = owner
        .output_read("run-1", chunk.next_offset, 65536)
        .unwrap();
    assert!(rest.data.is_empty(), "a read from the end returns nothing");
    let reconciled = owner
        .reconcile(&Reconcile::CommandId("run-1".into()))
        .unwrap();
    assert_eq!(reconciled["executions"][0], execution("run-1"));

    // Paging: every event, then an empty page at the cursor.
    let page = owner
        .events_read(&Position::Start, &["execution.execution"], 1000)
        .unwrap();
    assert!(!page["items"].as_array().unwrap().is_empty());
    let cursor = page["next_cursor"].as_str().unwrap().to_owned();
    let tail = owner
        .events_read(&Position::Cursor(cursor), &["execution.execution"], 1000)
        .unwrap();
    assert!(tail["items"].as_array().unwrap().is_empty());

    // A grant is a boundary: the holder reads and may not submit, and after
    // the revoke it may not read either.
    let terms = json!({"holder": "board", "audience": "conformance-provider",
                       "rights": ["core.events.read", "execution.read"],
                       "resources": [{"kind": "execution.execution"}],
                       "delegation": {"allowed": false, "max_depth": 0}});
    owner.grant_issue("g-board", terms).unwrap();
    let record = owner.grant_get("g-board").unwrap();
    let mut board = connect(&service.socket, BOARD, Some("g-board")).unwrap();
    assert_eq!(board.inspect("run-1").unwrap()["execution"]["id"], "run-1");
    assert_eq!(
        refused(board.submit(&submit_envelope("board-run"))).code(),
        "permission_denied"
    );
    owner
        .grant_revoke("g-board", record["revision"].as_u64().unwrap())
        .unwrap();
    assert!(matches!(board.inspect("run-1"), Err(Failure::Refused(_))));

    // The authority epoch: never claimed is epoch 0; a claim moves it to 1,
    // and a command fenced at 0 is then refused, verbatim.
    assert_eq!(owner.controller_epochs().unwrap().get(HOST), None);
    let claimed = owner.claim_controller(HOST, 0).unwrap();
    assert_eq!(claimed["outcome"]["epoch"], 1);
    assert_eq!(owner.controller_epochs().unwrap()[HOST], 1);
    assert_eq!(
        refused(owner.submit(&submit_envelope("run-2"))).code(),
        "stale_authority_epoch"
    );
    let (fence, view) = owner.fence("run-1").unwrap();
    assert_eq!(fence.epoch, 1);
    assert_eq!(Some(fence.revision), view["revision"].as_u64());
    let stale = Fence { epoch: 0, ..fence };
    assert_eq!(
        refused(owner.cancel("run-1", stale, None)).code(),
        "stale_authority_epoch"
    );
    let behind = Fence {
        revision: fence.revision - 1,
        ..fence
    };
    assert_eq!(
        refused(owner.cancel("run-1", behind, None)).code(),
        "precondition_failed"
    );

    // The fake offers neither steering nor actions, and says so itself.
    assert_eq!(
        refused(owner.steer("run-1", "hello", fence)).code(),
        "unsupported_required_feature"
    );
    assert_eq!(
        refused(owner.respond_action("run-1", "run-1.action-1", "allow", fence)).code(),
        "unsupported_required_feature"
    );
}

#[test]
fn a_fence_that_moved_is_read_again_and_nothing_else_is_retried() {
    let service = service(20000);
    let mut owner = connect(&service.socket, OWNER, None).unwrap();
    owner.submit(&submit_envelope("long")).unwrap();
    // The run moves while it starts, so the fence is re-read when the
    // service says the revision moved on. The durable fake host cannot
    // cancel, and that refusal is returned as it came, not retried.
    let mut attempts = 0;
    let refusal = refused(
        owner.fenced("long", Pinned::default(), 5, |client, fence, _| {
            attempts += 1;
            client.cancel("long", fence, Some("test"))
        }),
    );
    assert_eq!(refusal.code(), "capability_unavailable", "{refusal}");
    assert!(attempts <= 5);
    // A pinned revision is never replaced: stale, it is refused as it is.
    let refusal = refused(owner.fenced(
        "long",
        Pinned {
            revision: Some(0),
            epoch: None,
        },
        5,
        |client, fence, _| client.cancel("long", fence, None),
    ));
    assert_eq!(refusal.code(), "precondition_failed");
}
