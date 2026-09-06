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

Edit `config.toml`.

For a local OpenAI-compatible server:

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

## 3. Start the model server

Adapt does not start the model. Start your inference server **before** `./run.sh`.

For local mode, that is whatever serves the URL in `config.toml` (llama.cpp, vLLM, SGLang, LM Studio, Ollama’s OpenAI-compatible endpoint, and similar).

If the server is down, Adapt will fail to talk to the model. That is not an Adapt install failure.

If tool-output summarization is enabled, start that endpoint too.

---

## 4. Build Adapt

```bash
./build.sh
```

The compiled executable is:

```text
target/release/Adapt_v5
```

If you modify the Rust source, rebuild before testing the changes.

---

## 5. Run Adapt

**First test:** normal mode. It is the simplest.

```bash
./run.sh
```

Runs Adapt as your signed-in user. Most host access, least isolation.

**Restricted mode** — dedicated `model-user`, Linux user/group boundary:

```bash
sudo ./setup_restricted_model_user.sh
./run.sh --restricted
```

The model user gets its own tree under `/home/model-user/` (workspace, SQLite tool database, JSONL transcript, persistent Python venv).

**Lockdown mode** — restricted user plus Bubblewrap:

```bash
sudo ./setup_restricted_model_user.sh
sudo ./setup_lockdown.sh
./run.sh --lockdown
```

Lockdown keeps networking available so Adapt can still reach model endpoints, web tools, APIs, and package repos. It adds a stronger filesystem/process boundary.

### Why lockdown may ask for your password twice

This is expected.

1. Switch from your account to `model-user`.
2. Launch the Bubblewrap runtime.

Adapt does not read, store, or pass your sudo password to the model. Depending on your sudo cache, one or both prompts may not appear.

---

## Something broke?

Open a GitHub issue and include whatever you know:

- Linux distribution / WSL2
- terminal emulator
- model server or provider
- model
- launch mode (`run.sh`, `--restricted`, or `--lockdown`)
- error output

Short reports help. Example: `Works on Fedora` or `Lockdown fails on Ubuntu with this error`.

Architecture, security, providers, tools, memory, and async sessions are in [README.md](README.md).
