//! Shared harness for the B1b studio-lifecycle request tests: a fake
//! core, the stage that serves it and the queue consumer.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use backlot_model::req_wire::{ReqAction, ResAck};
use serde::de::DeserializeOwned;

use crate::cli::Env;
use crate::daemon::Stage;
use crate::doc_client;
use crate::doc_req::Consumer;
use crate::doc_req_fake_core::FakeCore;
use crate::store;

pub(crate) const FAKE_CAST: &str = r#"
[producer]
runner = "fake"
model = "model-f"
effort = "medium"
permission_profile = "standard"

[director]
runner = "fake"
model = "model-f"
effort = "medium"
permission_profile = "standard"

[writer]
runner = "fake"
model = "model-f"
effort = "medium"
permission_profile = "standard"
"#;

pub(crate) struct Server {
    pub(crate) rt: tokio::runtime::Runtime,
    pub(crate) core: FakeCore,
    pub(crate) stage: Stage,
    pub(crate) consumer: Consumer,
}

impl Server {
    pub(crate) fn start(env: &Env) -> Self {
        let rt = crate::link::runtime().unwrap();
        let base = PathBuf::from(env.get("BACKLOT_STATE_DIR").unwrap());
        fs::create_dir_all(&base).unwrap();
        let (core, stage) = rt.block_on(async {
            let core = FakeCore::spawn(base.join("hc.sock")).await;
            let mut stage = Stage::for_test(core.client().await);
            stage.base = base.clone();
            stage.env_vars = env.vars.clone();
            (core, stage)
        });
        let consumer = Consumer::load(&stage.base, crate::live::now_ms());
        Server {
            rt,
            core,
            stage,
            consumer,
        }
    }

    pub(crate) fn send(&mut self, action: &ReqAction, token: Option<String>) -> ResAck {
        let client_task = |core: &FakeCore, action: ReqAction, token: Option<String>| {
            let client_env = core.clone();
            async move {
                let client = client_env.client().await;
                let ep = client_env.endpoint();
                doc_client::send_request(&client, Some(&ep), &action, token, Duration::from_secs(2))
                    .await
            }
        };
        let core = self.core.clone();
        let action = action.clone();
        let stage = &self.stage;
        let consumer = &mut self.consumer;
        self.rt.block_on(async {
            let fut = tokio::spawn(client_task(&core, action, token));
            loop {
                if fut.is_finished() {
                    return fut.await.unwrap().unwrap();
                }
                let _ = consumer
                    .process(stage, crate::live::now_ms())
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    }

    pub(crate) fn typed<T: DeserializeOwned>(
        &mut self,
        action: &ReqAction,
        token: Option<String>,
    ) -> T {
        let ack = self.send(action, token);
        assert!(ack.ok, "ack failed: {:?}", ack.error);
        serde_json::from_value(ack.result.expect("result")).unwrap()
    }
}

pub(crate) fn err_line(ack: &ResAck) -> String {
    assert!(!ack.ok, "expected a refusal");
    ack.error.clone().unwrap_or_default()
}

pub(crate) fn base_of(env: &Env) -> PathBuf {
    PathBuf::from(env.get("BACKLOT_STATE_DIR").unwrap())
}

pub(crate) fn load(env: &Env, name: &str) -> Vec<backlot_model::Event> {
    store::load_events(&store::studio_dir(&base_of(env), name))
        .unwrap()
        .0
}
