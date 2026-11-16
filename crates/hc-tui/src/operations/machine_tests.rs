use super::tests::*;
use super::*;
use crate::testcore;
use serde_json::json;

async fn machine_call(method: &'static str, op: MachineOp, expected: serde_json::Value) {
    let (mut store, mut core) = store();
    let server = tokio::spawn(async move {
        hello(&mut core, &[method]).await;
        let req = core.read_request().await;
        assert_eq!(req["method"], method);
        assert_eq!(req["params"], expected);
        core.reply(&req["id"], json!({})).await;
    });
    store.submit(request(Action::Machine(op))).unwrap();
    let out = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(out.unwrap(), Outcome::Done));
    server.await.unwrap();
}

#[tokio::test]
async fn machine_add_dispatches_machine_add_with_the_target() {
    machine_call(
        "machine.add",
        MachineOp::Add {
            target: "alice@host-b".into(),
        },
        json!({"target": "alice@host-b"}),
    )
    .await;
}

#[tokio::test]
async fn machine_connect_dispatches_machine_connect_with_the_name() {
    machine_call(
        "machine.connect",
        MachineOp::Connect {
            machine: "host-b".into(),
        },
        json!({"machine": "host-b"}),
    )
    .await;
}

#[tokio::test]
async fn machine_rename_dispatches_machine_rename_with_the_new_name() {
    machine_call(
        "machine.rename",
        MachineOp::Rename {
            machine: "host-b".into(),
            new_name: "build-box".into(),
        },
        json!({"machine": "host-b", "new_name": "build-box"}),
    )
    .await;
}

#[tokio::test]
async fn machine_remove_dispatches_machine_rm_with_the_name() {
    machine_call(
        "machine.rm",
        MachineOp::Remove {
            machine: "host-b".into(),
        },
        json!({"machine": "host-b"}),
    )
    .await;
}
