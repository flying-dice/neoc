//! Driven adapter: ACP client. Launches agent processes over stdio and speaks ACP v1 to them.
//! Implements the `AgentConnector` port.

use agent_client_protocol::{
    schema::ProtocolVersion,
    v1::schema::{InitializeRequest, NewSessionRequest},
    AcpAgent, Client,
};
use agentos_application::{
    ports::{AgentConnector, PortResult, RunId},
    PortError,
};
use agentos_domain::AgentId;
use async_trait::async_trait;
use std::{collections::HashMap, path::PathBuf};
use tokio_util::sync::CancellationToken;

/// How to launch one agent (from host config).
#[derive(Clone, Debug)]
pub struct AgentLaunch {
    pub program: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentHello {
    pub name: Option<String>,
    pub version: Option<String>,
    pub protocol_version: String,
}

fn agent_of(l: &AgentLaunch) -> AcpAgent {
    use agent_client_protocol::AcpAgentConfig;
    AcpAgent::new(AcpAgentConfig::new(&l.program).args(l.args.iter().cloned()))
}

/// Spawn the agent, run the `initialize` handshake, shut it down. No prompts, no model calls.
pub async fn handshake(launch: &AgentLaunch) -> Result<AgentHello, agent_client_protocol::Error> {
    Client
        .v1()
        .name("agentos")
        .connect_with(agent_of(launch), async |cx| {
            let r = cx.send_request(InitializeRequest::new(ProtocolVersion::V1)).block_task().await?;
            Ok(AgentHello {
                name: r.agent_info.as_ref().map(|i| i.name.clone()),
                version: r.agent_info.as_ref().map(|i| i.version.clone()),
                protocol_version: format!("{:?}", r.protocol_version),
            })
        })
        .await
}

/// `start_run` = initialize + `session/new`, returns the session id. (Prompting and streaming
/// `session/update` into AG-UI events is the next milestone, wave B3.)
///
/// Agent processes live exactly as long as the connection future that owns them: dropping the future ends
/// the process (the ACP crate's guard terminates its process group), which is how
/// [`AgentConnector::stop_all`] stops them.
pub struct AcpConnector {
    launches: HashMap<AgentId, AgentLaunch>,
    stopping: CancellationToken,
}

impl AcpConnector {
    pub fn new(launches: HashMap<AgentId, AgentLaunch>) -> Self {
        Self { launches, stopping: CancellationToken::new() }
    }
}

#[async_trait]
impl AgentConnector for AcpConnector {
    async fn start_run(&self, agent: &AgentId, _prompt: &str) -> PortResult<RunId> {
        let launch = self.launches.get(agent).ok_or_else(|| PortError::NotFound(agent.0.clone()))?;
        let cwd = std::env::current_dir().map_err(|e| PortError::Internal(e.to_string()))?;
        let run = Client.v1().name("agentos").connect_with(agent_of(launch), async |cx| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1)).block_task().await?;
            let s = cx.send_request(NewSessionRequest::new(cwd)).block_task().await?;
            Ok(RunId(s.session_id.to_string()))
        });
        tokio::select! {
            () = self.stopping.cancelled() => Err(PortError::Rejected("the host is shutting down".into())),
            run = run => run.map_err(|e| PortError::Rejected(e.to_string())),
        }
    }

    async fn stop_all(&self) {
        self.stopping.cancel();
    }
}
