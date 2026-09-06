# Echo Adapt v5 — Quick Start

Want to try Adapt without reading the full documentation? Start here.

> **Platform:** Linux or Windows 11 through WSL2. Native Windows is not supported.

## 1. Clone and install

```bash
git clone https://github.com/charlesericwilson-portfolio/Echo_Adapt_v5
cd Echo_Adapt_v5

chmod +x *.sh
./install_deps.sh
```

## 2. Connect a model

Edit:

```text
config.toml
```

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

Start your model server before launching Adapt.

You can also use a remote OpenAI-compatible provider by changing:

```text
provider
url
model
api_key
```

Cloud-provider support may cause conversation history and local tool output to leave your machine. Do not use a cloud provider for workflows containing data you need to keep local or confidential.

## 3. Build Adapt

```bash
./build.sh
```

The compiled executable is created at:

```text
target/release/Adapt_v5
```

If you modify the Rust source, rebuild before testing the changes.

## 4. Choose how to run Adapt

### Normal mode

```bash
./run.sh
```

Runs Adapt with the permissions of your currently signed-in user.

This provides the most direct access to the host system and the least isolation.

### Restricted mode

First create the dedicated model user:

```bash
sudo ./setup_restricted_model_user.sh
```

Then launch:

```bash
./run.sh --restricted
```

Restricted mode runs Adapt as the dedicated:

```text
model-user
```

instead of your signed-in account.

The model user receives its own runtime environment under:

```text
/home/model-user/
```

including its workspace, SQLite tool database, JSONL transcript, and persistent Python virtual environment.

### Lockdown mode

Lockdown builds on restricted mode and adds Bubblewrap isolation.

First configure the restricted user if you have not already:

```bash
sudo ./setup_restricted_model_user.sh
```

Then configure the lockdown prerequisites:

```bash
sudo ./setup_lockdown.sh
```

Launch with:

```bash
./run.sh --lockdown
```

Lockdown keeps networking available so Adapt can still reach model endpoints, web tools, APIs, package repositories, and other network resources while adding a stronger filesystem/process boundary.

### Why lockdown may ask for your password twice

This is expected.

The two prompts authorize separate operations involved in launching the isolated environment:

1. switching from your signed-in account to the dedicated `model-user`;
2. launching the Bubblewrap-isolated runtime.

Adapt does not read, store, or pass your sudo password to the model.

Depending on your sudo credential cache, one or both prompts may not appear.

## 5. Pick the mode you want

```text
./run.sh
    Current-user permissions
    Maximum host access

./run.sh --restricted
    Dedicated model-user
    Linux user/group permission boundary

./run.sh --lockdown
    Dedicated model-user + Bubblewrap
    Stronger filesystem/process isolation
```

For a first test, normal mode is the simplest.

If you want the model separated from your signed-in account, use restricted mode.

If you want the stronger optional isolation boundary, use lockdown mode.

## Something broke?

Please open a GitHub issue and include whatever you know about your:

```text
Linux distribution / WSL2 environment
terminal emulator
model server or provider
model
launch mode
error output
```

Even a short report such as:

```text
Works on Fedora
```

or:

```text
Lockdown fails on Ubuntu with this error
```

is useful.

For configuration, architecture, security, providers, tools, memory, asynchronous sessions, and implementation details, see the main:

```text
README.md
```
