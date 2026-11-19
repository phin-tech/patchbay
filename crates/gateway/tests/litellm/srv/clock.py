# A small MCP server over stdio for LiteLLM to run, on the MCP SDK LiteLLM ships (2.x, where FastMCP is MCPServer).
from mcp.server.mcpserver import MCPServer

mcp = MCPServer("clock")


@mcp.tool()
def now() -> str:
    """The time, always the same, so tests can check it."""
    return "12:00"


@mcp.tool()
def echo(text: str) -> str:
    """Says text back."""
    return text


mcp.run()
