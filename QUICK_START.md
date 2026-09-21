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

The server also includes a simple browser-based chat interface for testing models without tool interaction:

```text
http://127.0.0.1:8080/
```

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

* llama.cpp
* vLLM
* SGLang
* LM Studio
* Ollama's OpenAI-compatible endpoint
* another compatible local or remote provider

Set the appropriate URL and model in Adapt's root `config.toml`.

Some modern inference backends expose separate reasoning channels, native tool-call formats, or backend-specific response parsing. ADAPT expects normal assistant text containing ADAPT's own tool protocol. The bundled ADAPT Server exists to preserve that simple interface for local GGUF models.

If the inference server is down, Adapt will fail to talk to the model. That is not an Adapt installation failure.

If tool-output summarization is enabled, start that endpoint too.

---

## 6. Configure the Remote Tool Server

Adapt includes a separate remote tool server under:

```text
tool_server/
```

The tool server provides an authenticated boundary for tools that need server-side credentials or access to external services.

Create its configuration:

```bash
cd tool_server
cp tool_server.example.toml tool_server.toml
```

Edit `tool_server.toml`.

A basic development configuration looks like:

```toml
[server]
bind_address = "127.0.0.1:9000"
auth_token = "CHANGE_ME"
```

Then return to the repository root:

```bash
cd ..
```

The authentication token must match the tool-server configuration in Adapt's root `config.toml`:

```toml
[tool_server]
enabled = true
url = "http://127.0.0.1:9000"
auth_token = "CHANGE_ME"
```

For real deployments, use a strong token.

Remote tools are deliberately registered by the developer. Running the server does not turn it into an arbitrary remote command executor.

A remote tool can provide access to services such as:

* web APIs
* databases
* internal APIs
* SaaS platforms
* ticketing systems
* remote workflow services
* other application-specific backends

Service credentials belong on the **tool-server side**, not in the model prompt.

Conceptually:

```text
Model
    ↓
Adapt
    ↓ authenticated request
Remote Tool Server
    ↓
registered service/API
    ↓
Remote Tool Server
    ↓
Adapt
    ↓
Model
```

During startup, Adapt can retrieve the registered remote tool definitions from the server. Remote tools are then exposed to the model through Adapt's normal JSON tool interface.

Example:

```json
{
  "name": "tool_name",
  "arguments": {
    "example": "value"
  }
}
```

The remote tool server owns service-specific authentication, argument handling, API translation, response parsing, and error handling.

---

## 7. Build Adapt

From the repository root:

```bash
./build.sh
```

The root build script builds both:

```text
target/release/Adapt_v5
tool_server/target/release/Adapt_tool_server
```

The bundled GGUF inference server is built separately because its build depends on the selected hardware backend.

If you modify the Rust source, rebuild before testing the changes.

---

## 8. Run Adapt

**First test:** normal mode. It is the simplest.

```bash
./run.sh
```

`run.sh` starts the remote tool server, waits for it to report ready, and then starts Adapt.

When Adapt exits, the launcher shuts down the tool server automatically.

You do **not** need to manually start the remote tool server.

### Restricted mode

Dedicated `model-user`, Linux user/group boundary:

```bash
sudo ./setup_restricted_model_user.sh
./run.sh --restricted
```

The model user gets its own tree under `/home/model-user/`:

* workspace
* SQLite tool database
* JSONL transcript
* persistent Python venv

The remote tool server remains outside the restricted model-user boundary and is managed automatically by `run.sh`.

### Lockdown mode

Restricted user plus Bubblewrap:

```bash
sudo ./setup_restricted_model_user.sh
sudo ./setup_lockdown.sh
./run.sh --lockdown
```

Lockdown adds a stronger filesystem and process boundary around Adapt while preserving the networking currently required for the inference server and remote tool server.

The remote tool server runs outside the Bubblewrap sandbox. This allows server-side tools to keep their credentials and network functionality separate from the model runtime.

### Why lockdown may ask for your password more than once

This is expected.

The launcher may need elevated privileges while preparing the restricted environment and launching the Bubblewrap runtime as `model-user`.

Adapt does not read, store, or pass your sudo password to the model.

Depending on your sudo cache, some prompts may not appear.

---

## Terminal Hotkeys

ADAPT includes built-in terminal controls:

* `Ctrl+Alt+N` — Open a new ADAPT process/tab
* `Ctrl+C` — Exit the current ADAPT chat
* `Enter` — Submit the current message
* `Backspace` — Delete input

---

## Remote Tool Architecture

The remote tool server is intentionally small and application-specific.

It is **not** a universal arbitrary-tool executor.

A developer deliberately implements and registers the capabilities their application needs. Each remote tool generally defines:

* tool name
* model-facing description
* expected arguments
* argument validation
* downstream API, SDK, or database logic
* server-side credentials
* request translation
* response parsing
* error handling

Adapt performs authenticated tool discovery during startup:

```text
Adapt starts
    ↓
authenticated GET /tools
    ↓
tool server returns registered tool definitions
    ↓
Adapt caches the remote registry
    ↓
model can call registered tools
```

Execution follows the same authenticated boundary:

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

Credentials for downstream services remain on the server side.

For example:

```text
Adapt
    ↓ authenticated request
Tool Server
    ├── web API credentials
    ├── PostgreSQL credentials
    ├── Jira credentials
    ├── GitHub service token
    └── internal API credentials
```

Adapt only needs the credential required to authenticate to the remote tool server itself.

---

## Something broke?

Open a GitHub issue and include whatever you know:

* Linux distribution / WSL2
* terminal emulator
* model server or provider
* model
* GPU backend (`rocm`, `cuda`, or `cpu`)
* launch mode (`run.sh`, `--restricted`, or `--lockdown`)
* error output

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
