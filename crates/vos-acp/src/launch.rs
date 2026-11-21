//! How to start an agent process.

use agent_client_protocol::{
    schema::ProtocolVersion, v1::schema::InitializeRequest, AcpAgent, AcpAgentConfig, Client,
};
use std::{collections::BTreeMap, path::PathBuf};
use vos_domain::Agent;

/// A program and its arguments and environment.
#[derive(Clone, Debug)]
pub struct AgentLaunch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl AgentLaunch {
    #[must_use]
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>) -> Self {
        Self { program: program.into(), args, env: BTreeMap::new() }
    }

    /// An agent contact's runtime.
    #[must_use]
    pub fn of(agent: &Agent) -> Self {
        Self {
            program: PathBuf::from(&agent.runtime.command),
            args: agent.runtime.args.clone(),
            env: agent.runtime.env.clone(),
        }
    }

    pub(crate) fn acp_agent(&self) -> AcpAgent {
        AcpAgent::new(
            AcpAgentConfig::new(&self.program).args(self.args.iter().cloned()).envs(self.env.clone()),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentHello {
    pub name: Option<String>,
    pub version: Option<String>,
    pub protocol_version: String,
}

/// Spawn the agent, run the `initialize` handshake, shut it down. No prompts, no model calls.
pub async fn handshake(launch: &AgentLaunch) -> Result<AgentHello, agent_client_protocol::Error> {
    Client
        .v1()
        .name("vos")
        .connect_with(launch.acp_agent(), async |cx| {
            let r = cx.send_request(InitializeRequest::new(ProtocolVersion::V1)).block_task().await?;
            Ok(AgentHello {
                name: r.agent_info.as_ref().map(|i| i.name.clone()),
                version: r.agent_info.as_ref().map(|i| i.version.clone()),
                protocol_version: format!("{:?}", r.protocol_version),
            })
        })
        .await
}
