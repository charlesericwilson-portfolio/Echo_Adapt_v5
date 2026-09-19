# Echo Adapt v5 — Quick Start

Want to try Adapt without reading the full documentation? Start here.

**Platform:** Linux or Windows 11 through WSL2. Native Windows is not supported.

---

## 1. Clone and install

```bash
git clone https://github.com/charlesericwilson-portfolio/Echo_Adapt_v5
cd Echo_Adapt_v5

chmod +x *.sh
./install_deps.sh
```

---

## 2. Connect a model

Edit Adapt's root `config.toml`.

For the bundled local ADAPT Server:

```toml
[endpoint]
provider = "local"
url = "http://localhost:8080/v1/chat/completions"
model = "Echo"
api_key = ""
temperature = 0.7
max_tokens = 2048
```

You can also use a remote OpenAI-compatible provider by changing `provider`, `url`, `model`, and `api_key`.

Cloud-provider support may send conversation history and local tool output off your machine. Do not use a cloud provider for data that must stay local or confidential.

---

## 3. Build the ADAPT Server

Adapt includes a lightweight local GGUF inference server under:

```text
adapt_server/
```

The server preserves the simple inference contract ADAPT expects:

```text
conversation context in
        ↓
model inference / reasoning
        ↓
reasoning and model-control channels removed
        ↓
clean assistant text out
        ↓
ADAPT handles tools, safety, memory, and orchestration
```

The server does not maintain conversation state between turns. ADAPT owns conversation history and sends the relevant context with each request.
The server does include a simple browser based chat interface to test models without tool interaction at http://127.0.0.1:8080/

Enter the server directory:

```bash
cd adapt_server
```

Create your local server config:

```bash
cp config.example.toml config.toml
```

Edit `config.toml` and set your GGUF model path and runtime settings.

Example:

```toml
[server]
bind = "127.0.0.1:8080"
debug_requests = false

[model]
path = "/path/to/your/model.gguf"
context_size = 32768
batch_size = 2048
gpu_layers = 999

[generation]
max_tokens = 2048
temperature = 0.7
top_p = 0.95
top_k = 40

[output]
strip_reasoning = true
```

Build for your hardware.

### AMD / ROCm

```bash
./build.sh rocm
```

### NVIDIA / CUDA

```bash
./build.sh cuda
```

### CPU only

```bash
./build.sh cpu
```

The compiled executable is:

```text
adapt_server/target/release/adapt_server
```

---

## 4. Start the ADAPT Server

From inside `adapt_server/`:

```bash
./target/release/adapt_server
```

By default, the server reads:

```text
config.toml
```

You can optionally use another config file:

```bash
./target/release/adapt_server --config other-config.toml
```

The server must be running before you start Adapt.

When finished, return to the repository root:

```bash
cd ..
```

---

## 5. Other inference servers

The bundled ADAPT Server is the recommended local path, but it is not required.

You can also use another OpenAI-compatible inference server such as:

- llama.cpp
- vLLM
- SGLang
- LM Studio
- Ollama's OpenAI-compatible endpoint
- another compatible local or remote provider

Set the appropriate URL and model in Adapt's root `config.toml`.

Some modern inference backends expose separate reasoning channels, native tool-call formats, or backend-specific response parsing. ADAPT expects normal assistant text containing ADAPT's own tool protocol. The bundled ADAPT Server exists to preserve that simple interface for local GGUF models.

If the server is down, Adapt will fail to talk to the model. That is not an Adapt install failure.

If tool-output summarization is enabled, start that endpoint too.

---

## 6. Build Adapt

From the repository root:

```bash
./build.sh
```

The compiled executable is:

```text
target/release/Adapt_v5
```

If you modify the Rust source, rebuild before testing the changes.

---

## 7. Run Adapt

**First test:** normal mode. It is the simplest.

```bash
./run.sh
```

Runs Adapt as your signed-in user. Most host access, least isolation.

### Restricted mode

Dedicated `model-user`, Linux user/group boundary:

```bash
sudo ./setup_restricted_model_user.sh
./run.sh --restricted
```

The model user gets its own tree under `/home/model-user/`:

- workspace
- SQLite tool database
- JSONL transcript
- persistent Python venv

### Lockdown mode

Restricted user plus Bubblewrap:

```bash
sudo ./setup_restricted_model_user.sh
sudo ./setup_lockdown.sh
./run.sh --lockdown
```

Lockdown keeps networking available so Adapt can still reach model endpoints, web tools, APIs, and package repositories. It adds a stronger filesystem/process boundary.

### Why lockdown may ask for your password twice

This is expected.

1. Switch from your account to `model-user`.
2. Launch the Bubblewrap runtime.

Adapt does not read, store, or pass your sudo password to the model.

Depending on your sudo cache, one or both prompts may not appear.

---

## Terminal Hotkeys

ADAPT includes a few built-in terminal shortcuts:

- `Ctrl+Alt+N` — Open a new ADAPT process/tab
- `Ctrl+C` — Exit the current ADAPT chat
- `Enter` — Submit the current message
- `Backspace` — Delete input

## Optional: Build and Run the Remote Tool Server

The remote tool server is **not required to use Adapt**.

If you only want Adapt's normal local functionality, you can skip this section entirely.

The tool server is intended for developers who want Adapt to call application-specific remote services such as:

* databases
* internal APIs
* SaaS platforms
* ticketing systems
* remote workflow services
* other network-accessible application backends

Remote tools are **not automatically available** just because the server is running. A developer must define and register the tools their application needs, including the expected arguments, validation, downstream connection logic, authentication, and response handling.

The tool server runs as a separate process from Adapt.

### 1. Enter the tool-server directory

From the repository root:

```bash
cd tool_server
```

### 2. Create the server configuration

If an example configuration is included:

```bash
cp tool_server.example.toml tool_server.toml
```

Otherwise create:

```text
tool_server.toml
```

A basic development configuration looks like:

```toml
[server]
bind_address = "127.0.0.1:9000"
auth_token = "CHANGE_ME"
```

The token must match the token configured on the Adapt side.

In Adapt's root `config.toml`:

```toml
[tool_server]
enabled = true
url = "http://127.0.0.1:9000"
auth_token = "CHANGE_ME"
```

For real deployments, do not use a trivial development token.

### 3. Build the tool server

From inside `tool_server/`:

```bash
cargo build --release
```

The compiled executable is:

```text
tool_server/target/release/Adapt_tool_server
```

### 4. Start the tool server

From inside `tool_server/`:

```bash
./target/release/Adapt_tool_server
```

The tool server must be running before Adapt starts if remote tool support is enabled.

Adapt performs authenticated tool discovery during startup.

Conceptually:

```text
Adapt starts
    ↓
authenticated GET /tools
    ↓
tool server returns registered tool definitions
    ↓
Adapt caches the remote registry
    ↓
model can call those tools through normal JSON tool syntax
```

Remote execution uses the same authenticated server connection:

```text
model requests remote tool
    ↓
Adapt checks remote registry
    ↓
authenticated POST /execute
    ↓
tool server validates and dispatches tool
    ↓
remote service
    ↓
result returned to Adapt
```

### 5. Define the tools your project needs

The tool server is not a universal arbitrary-tool executor.

A developer must deliberately implement the remote capabilities their project needs.

Each tool generally needs:

* a tool name
* a model-facing description
* expected arguments
* argument validation
* remote endpoint, SDK, or database logic
* server-side credentials
* request translation
* response parsing
* error handling

The model itself continues using the normal Adapt JSON format:

```json
{
  "name": "tool_name",
  "arguments": {
    "example": "value"
  }
}
```

The tool server owns the translation from that simple request into the downstream API, database, SDK, or service-specific operation.

### 6. Server-side credentials

Credentials for remote services belong on the **tool-server side**, not in the model prompt.

For example:

```text
Adapt
    ↓ authenticated request
Tool Server
    ├── PostgreSQL credentials
    ├── Jira credentials
    ├── GitHub service token
    └── internal API credentials
```

Adapt only needs the credential required to authenticate to the tool server itself.

The tool server should only be given access to the remote systems and files required by its registered tools.

### 7. Return to the repository root

When finished:

```bash
cd ..
```

You can now start Adapt normally.

If remote tool support is disabled in `config.toml`, Adapt does not require the tool server to be running.


## Something broke?

Open a GitHub issue and include whatever you know:

- Linux distribution / WSL2
- terminal emulator
- model server or provider
- model
- GPU backend (`rocm`, `cuda`, or `cpu`)
- launch mode (`run.sh`, `--restricted`, or `--lockdown`)
- error output

Short reports help.

Examples:

```text
Works on Fedora with CUDA
```

```text
ROCm server build fails on Ubuntu with this error
```

```text
Lockdown fails on WSL2 with this error
```

Architecture, security, providers, tools, memory, async sessions, and inference-server details are in [README.md](README.md).
