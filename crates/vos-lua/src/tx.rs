//! The transaction `db:tx` opens, and how statements reach it.
//!
//! The transaction lives in a task of the app's `Life` that owns the port's [`DataTx`] and works through a queue
//! of jobs in the order they were sent: a statement is *sent* at the moment the script calls it (synchronously),
//! and the commit is sent after every statement the body started. So the transaction never waits for a future
//! that something else may have stopped polling: a statement whose host call was abandoned (Lua could not suspend
//! at it) still runs, inside the transaction, before the commit, and a script cannot reach the point where the
//! body is over but a statement of it is stuck. Nothing holds the transaction across an await; letting go of the
//! [`OpenTx`] without ending it rolls the transaction back at once, a statement that is running included.
//!
//! A transaction belongs to the turn that opened it ([`OpenTx::owner`]): it holds the database for as long as it
//! is open, so that turn never lends the VM (see `lifecycle`), and a statement from any other call is refused
//! instead of running inside someone else's transaction.
//!
//! The task is registered in the app's `Life`: unloading the app, or stopping its background work for a reload,
//! aborts it, and the transaction is rolled back (the call that was running it gets "the transaction has ended").

use neoc_vm::lifecycle::Life;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use vos_application::{
    data::{DataTx, ExecOutcome, Params, PortResult},
    PortError,
};

/// A statement for the transaction's task to run.
enum Statement {
    Query { sql: String, params: Params, reply: oneshot::Sender<PortResult<Vec<Value>>> },
    Exec { sql: String, params: Params, reply: oneshot::Sender<PortResult<ExecOutcome>> },
}

/// What the transaction's task is asked to do.
enum Job {
    Statement(Statement),
    /// Commit or roll back, and finish.
    End {
        commit: bool,
        reply: oneshot::Sender<PortResult<()>>,
    },
}

/// The answer to a statement or the end of the transaction, still to come.
pub(crate) type Reply<T> = oneshot::Receiver<PortResult<T>>;

fn ended_error() -> PortError {
    PortError::Rejected("the transaction has ended".to_owned())
}

/// Wait for an answer: the transaction's task going away without one means the transaction has ended.
pub(crate) async fn answer<T>(reply: Reply<T>) -> PortResult<T> {
    reply.await.unwrap_or_else(|_| Err(ended_error()))
}

/// An open transaction, as its opener holds it. Dropping it without [`OpenTx::end`] rolls the transaction back.
pub(crate) struct OpenTx {
    owner: u64,
    jobs: mpsc::UnboundedSender<Job>,
    /// Told when the transaction is ended properly; dropped otherwise, which the task takes as "let go".
    ended_properly: oneshot::Sender<()>,
}

impl OpenTx {
    /// Start the task that runs `tx` for the turn `owner`.
    pub fn start(life: &Life, tx: Box<dyn DataTx>, owner: u64) -> Self {
        let (jobs, queue) = mpsc::unbounded_channel();
        let (ended_properly, let_go) = oneshot::channel();
        life.spawn(run(tx, queue, let_go));
        Self { owner, jobs, ended_properly }
    }

    /// The turn that opened the transaction (a turn opens at most one at a time).
    pub fn owner(&self) -> u64 {
        self.owner
    }

    /// Queue a read behind everything already sent. Synchronous: nothing waits while the transaction is borrowed.
    pub fn query(&self, sql: String, params: Params) -> Reply<Vec<Value>> {
        let (reply, rx) = oneshot::channel();
        self.send(Job::Statement(Statement::Query { sql, params, reply }), rx)
    }

    /// Queue a write behind everything already sent.
    pub fn exec(&self, sql: String, params: Params) -> Reply<ExecOutcome> {
        let (reply, rx) = oneshot::channel();
        self.send(Job::Statement(Statement::Exec { sql, params, reply }), rx)
    }

    /// Queue the end of the transaction behind every statement already sent, and let go of it.
    pub fn end(self, commit: bool) -> Reply<()> {
        let (reply, rx) = oneshot::channel();
        let answer = self.send(Job::End { commit, reply }, rx);
        let _ = self.ended_properly.send(());
        answer
    }

    fn send<T>(&self, job: Job, rx: Reply<T>) -> Reply<T> {
        if self.jobs.send(job).is_ok() {
            return rx;
        }
        let (reply, rx) = oneshot::channel();
        let _ = reply.send(Err(ended_error()));
        rx
    }
}

/// What the transaction's task is told while it waits: the opener let go (it ended without ending the transaction), or
/// it is gone altogether.
enum Released {
    LetGo,
    Gone,
}

/// Poll the opener's `let_go` signal (not again once it has fired).
async fn released(let_go: &mut oneshot::Receiver<()>, ending: bool) -> Released {
    if ending {
        return std::future::pending().await;
    }
    match let_go.await {
        Ok(()) => Released::LetGo,
        Err(_) => Released::Gone,
    }
}

/// The transaction's task: statements in the order they were sent, then the end. When the opener lets go without
/// ending the transaction (its call was cancelled, or ended), whatever is running is dropped and so is `tx`,
/// which rolls it back.
async fn run(
    mut tx: Box<dyn DataTx>,
    mut queue: mpsc::UnboundedReceiver<Job>,
    mut let_go: oneshot::Receiver<()>,
) {
    let mut ending = false;
    loop {
        let job = tokio::select! {
            biased;
            gone = released(&mut let_go, ending) => match gone {
                Released::LetGo => { ending = true; continue }
                Released::Gone => return,
            },
            job = queue.recv() => match job {
                Some(job) => job,
                None => return,
            },
        };
        match job {
            // only ever sent together with the "ended properly" signal: nothing cuts it short
            Job::End { commit, reply } => {
                let done = if commit { tx.commit().await } else { tx.rollback().await };
                let _ = reply.send(done);
                return;
            }
            Job::Statement(statement) => {
                let work = serve(tx.as_mut(), statement);
                tokio::pin!(work);
                loop {
                    tokio::select! {
                        biased;
                        gone = released(&mut let_go, ending) => match gone {
                            Released::LetGo => ending = true,
                            Released::Gone => return,
                        },
                        () = &mut work => break,
                    }
                }
            }
        }
    }
}

async fn serve(tx: &mut dyn DataTx, statement: Statement) {
    match statement {
        Statement::Query { sql, params, reply } => {
            let _ = reply.send(tx.query(sql, params).await);
        }
        Statement::Exec { sql, params, reply } => {
            let _ = reply.send(tx.exec(sql, params).await);
        }
    }
}
