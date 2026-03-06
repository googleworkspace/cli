# Copyright 2026 Google LLC
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Stage 1: Build the Rust binary
FROM rust:1.83-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

RUN cargo build --profile dist

# Stage 2: Minimal runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/dist/gws /usr/local/bin/gws

# Cloud Run sets PORT env var; default to 8080
ENV PORT=8080

EXPOSE 8080

# Run the MCP server in SSE mode, binding to 0.0.0.0 so Cloud Run can route traffic
ENTRYPOINT ["gws", "mcp", "--transport", "sse", "--host", "0.0.0.0", "-s", "all"]
CMD ["--port", "8080"]
