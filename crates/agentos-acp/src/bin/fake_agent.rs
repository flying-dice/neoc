//! A tiny ACP agent over stdio for tests: answers `initialize` and `session/new`. No model.
use agent_client_protocol::v1::schema::{
    AgentCapabilities, Implementation, InitializeRequest, InitializeResponse, NewSessionRequest,
    NewSessionResponse,
};
use agent_client_protocol::{Agent, Result, Stdio};

#[tokio::main]
async fn main() -> Result<()> {
    Agent
        .v1()
        .name("fake-agent")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("fake-agent", "0.0.1")),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(NewSessionResponse::new("sess-1"))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(Stdio::new())
        .await
}
