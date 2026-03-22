# Installation

## Requirements

- Ubuntu with PulseAudio or PipeWire's Pulse compatibility layer
- `sudo` privileges if installing the Debian package system-wide
- Rust toolchain if running from source
- ElevenLabs API key for live transcription
- OpenAI API key for answer, summary, and commentary actions

## Install from Debian package

1. Download the latest `soundmind_*_amd64.deb` from GitHub Releases.

2. Install it:

```bash
sudo apt install ./soundmind_0.2.1_amd64.deb
```

3. Prepare your user config and service:

```bash
soundmind-setup-user
```

4. Launch the app:

```bash
soundmind
```

The Debian package installs:

- `/usr/bin/soundmind`
- `/usr/bin/soundmind-backend`
- `/usr/bin/soundmind-setup-user`
- `/usr/lib/systemd/user/soundmind-backend.service`
- `/usr/share/soundmind/config.example.toml`
- the desktop entry and icon

## Run from source

1. Copy the example config:

```bash
cp config.example.toml config.toml
```

2. Create `keys.env` with your provider keys:

```bash
OPENAI_API_KEY=...
ELEVENLABS_API_KEY=...
```

3. Start the backend:

```bash
cargo run -p app_backend
```

4. Start the desktop UI:

```bash
cargo run -p app_ui
```

5. Optional debug client:

```bash
cargo run -p app_ui --bin terminal_ui
```

## Install as a local user service

This uses the packaging assets in the repository and installs Soundmind under
`~/.local`.

```bash
./scripts/install-user-service.sh
```

What it does:

- builds `app_backend` and `app_ui` in release mode
- installs wrappers to `~/.local/bin/soundmind` and
  `~/.local/bin/soundmind-backend`
- installs config to `~/.config/soundmind`
- installs a desktop entry and icon
- installs and enables `soundmind-backend.service` with `systemctl --user`

After installation:

```bash
soundmind
```

## Configuration locations

When installed, the backend looks in this order:

1. `SOUNDMIND_CONFIG` and `SOUNDMIND_KEYS_ENV`
2. local `config.toml` and `keys.env`
3. `~/.config/soundmind/config.toml` and `~/.config/soundmind/keys.env`

## Build a release bundle

```bash
./scripts/package-release.sh
```

This creates:

- `dist/soundmind-linux-<version>.tar.gz`
- `dist/soundmind_<version>_amd64.deb`
- `dist/soundmind_<version>_SHA256SUMS`

## Publish a GitHub release

```bash
./scripts/release-github.sh
```

This builds the release artifacts, creates or reuses a `v<version>` git tag,
pushes it to GitHub, and uploads the tarball, Debian package, and checksum file
to a GitHub release.
