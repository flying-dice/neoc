#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! A task that waits for the next item of a module's stream (`mlua_ir::Stream`: Docker's events, a pull's progress,
//! a container's log) does not hold the app: other calls run meanwhile, as they do while it waits at any other
//! host call that does not use the VM. The Containers, DNS and Pages apps each keep such a task for as long as they
//! are loaded (the watcher of Docker's events).

use crate::common;

use common::{load, timed_tool};
use std::sync::Arc;

/// A Docker that takes the connection and never answers: a read of its events waits for as long as it is asked to.
async fn silent_docker() -> (tokio::net::TcpListener, String) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = format!("tcp://{}", listener.local_addr().unwrap());
    (listener, host)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_task_waiting_for_a_streams_next_item_lends_the_app_to_other_calls() {
    let (listener, host) = silent_docker().await;
    let held = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            open.push(socket);
        }
    });
    let vm = Arc::new(
        load(&format!(
            r#"
            local bollard = require("vnd:bollard")
            local task = require("std:task")
            local reading = false
            app.tool {{ name = "watch", run = function()
              local docker = assert(bollard.connect {{ host = "{host}", timeout = 60 }})
              task.spawn(function()
                reading = true
                docker:events():next()
              end)
              return reading
            end }}
            app.tool {{ name = "ask", readOnly = true, run = function()
              return "answered"
            end }}
            "#
        ))
        .await,
    );
    assert_eq!(timed_tool(&vm, "watch", 10).await.unwrap(), true);
    // the watcher's task is now at `next()`, and waits there: the app answers all the same
    for _ in 0..3 {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(timed_tool(&vm, "ask", 5).await.unwrap(), "answered");
    }
    held.abort();
}
