//! `patchbay bridge`: the gateway over stdio, for clients that only run commands (Claude Desktop, Zed). It holds no
//! secrets of the servers'; it passes each message on to the running gateway with the gateway's token.

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};

use super::*;

struct Bridge {
    gateway: Peer<RoleClient>,
}

impl ServerHandler for Bridge {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("patchbay", patchbay_core::VERSION))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.gateway
            .list_all_tools()
            .await
            .map(ListToolsResult::with_all_items)
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.gateway
            .call_tool_once(request)
            .await
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))
    }
}

/// Bridges stdin and stdout to the gateway at `url` until the client hangs up.
pub async fn bridge(url: &str, token: &str) -> anyhow::Result<()> {
    let config = StreamableHttpClientTransportConfig::with_uri(url).auth_header(token.to_string());
    let gateway = ().serve(StreamableHttpClientTransport::from_config(config)).await?;
    let server = Bridge {
        gateway: gateway.peer().clone(),
    }
    .serve(rmcp::transport::stdio())
    .await?;
    server.waiting().await?;
    let _ = gateway.cancel().await;
    Ok(())
}
