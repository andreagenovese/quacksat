# Reference bridge for the quacksat agent protocol.
# Build:  docker compose build      (or: docker build -t quacksat-bridge .)
# Run:    docker compose up -d      (config.toml mounted read-only)
FROM python:3.12-slim

WORKDIR /app
RUN pip install --no-cache-dir websockets "mcp>=2" uvicorn
COPY bridge.py mcp_server.py ./

# 8765 = satellite WebSocket, 8766 = MCP server (when enabled).
EXPOSE 8765 8766
CMD ["python", "bridge.py", "--config", "/config/config.toml"]
