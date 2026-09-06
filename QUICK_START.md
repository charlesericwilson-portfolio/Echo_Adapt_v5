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

Start your model server before launching Adapt.

You can also use a remote OpenAI-compatible provider by changing `provider`, `url`, `model`, and `api_key`.

## 3. Build and run

```bash
./build.sh
./run.sh
```

That's it.

For the dedicated restricted model user:

```bash
sudo ./setup_restricted_model_user.sh
./run.sh --restricted
```

## Something broke?

Please open a GitHub issue and include your Linux distribution/WSL2 environment, model server, model, and the error output.

For configuration, architecture, security, tools, providers, memory, and everything else, see the main `README.md`.
