```
                                              _      
                                             | |     
  ___ _   _ _ __   ___ _ __ ___  ___  ___  __| |_ __ 
 / __| | | | '_ \ / _ \ '__/ __|/ _ \/ _ \/ _` | '__|
 \__ \ |_| | |_) |  __/ |  \__ \  __/  __/ (_| | |   
 |___/\__,_| .__/ \___|_|  |___/\___|\___|\__,_|_|   
           | |                                       
           |_|                                    
```
# Superseedr - A Rust BitTorrent Client in your Terminal

[![Rust](https://github.com/Jagalite/superseedr/actions/workflows/rust.yml/badge.svg)](https://github.com/Jagalite/superseedr/actions/workflows/rust.yml) [![Nightly Fuzzing](https://github.com/Jagalite/superseedr/actions/workflows/nightly.yml/badge.svg)](https://github.com/Jagalite/superseedr/actions/workflows/nightly.yml) ![Verification](https://img.shields.io/badge/Logic_Verification-Model--Based_Fuzzing-blueviolet?style=flat-square)

![GitHub release](https://img.shields.io/github/v/release/Jagalite/superseedr) ![crates.io](https://img.shields.io/crates/v/superseedr) ![License](https://img.shields.io/github/license/Jagalite/superseedr) [![Built With Ratatui](https://ratatui.rs/built-with-ratatui/badge.svg)](https://ratatui.rs/) <a title="This tool is Tool of The Week on Terminal Trove, The $HOME of all things in the terminal" href="https://terminaltrove.com/"><img src="https://cdn.terminaltrove.com/media/badges/tool_of_the_week/png/terminal_trove_tool_of_the_week_gold_transparent.png" alt="Terminal Trove Tool of The Week" /></a>

Superseedr is a modern Rust BitTorrent client featuring a high-performance terminal UI, real-time swarm observability, secure VPN-aware Docker setups, and zero manual network configuration. It is fast, privacy-oriented, and built for both desktop users and homelab/server workflows.

![Feature Demo](https://github.com/Jagalite/superseedr-assets/blob/main/superseedr_landing.webp)

### 🎯 Why Superseedr?

Superseedr brings the BitTorrent into the modern terminal environment, focusing on speed, visibility, and reliability.

* **Deep Swarm Analytics:** Moves beyond simple progress bars by providing high performance real-time heatmaps, peer metrics, and network graphs for complete swarm observability.
* **Modern Rust Engine:** Leverages Rust and Model-Based Testing to ensure memory safety, high performance, and unparalleled reliability.
* **Seamless Networking:** Designed for resilient connectivity, featuring automatic listener reloading for dynamic VPN ports to maintain uptime without manual intervention.

## 🚀 Features
- 🎨 Animated, high-performance TUI (1–60 FPS)
- 🧲 OS-level magnet link support
- 📊 Real-time network graphs and swarm analytics
- 🔐 Official Docker + VPN setup with automatic port forwarding
- 🔄 Dynamic inbound port reloading without restarting the client
- ✅ Unparalleled Reliability & Correctness through Model-Based Testing
- 🛡️ Private Tracker Builds without PEX and DHT
- ⚡ Rust-based engine for performance and safety
- 💾 Persistent state with crash recovery
- 🧵 Peer-level metrics and availability heatmaps

## Installation

[![Packaging status](https://repology.org/badge/vertical-allrepos/superseedr.svg)](https://repology.org/project/superseedr/versions)

Download the latest release for your platform:
- Windows (.msi)
- macOS (.pkg)
- Debian (.deb)
- Arch Linux (pkgbuild) from [AUR](https://aur.archlinux.org/packages/superseedr)

👉 Available on the [releases page](https://github.com/Jagalite/superseedr/releases).

## Usage
Open up a terminal and run:
```bash
superseedr
```
### ⌨️ Key Controls
| Key | Action |
| :--- | :--- |
| `m` | **Open full manual / help** |
| `q` | Quit |
| `↑` `↓` `←` `→` | Navigate |
| `c` | Configure Settings |

> [!TIP]
> Press `m` inside the app to see the full list of shortcuts, legends, and features.

> [!TIP]  
> Add torrents by clicking magnet links in your browser or opening .torrent files.
> Copying and pasting (crtl + v) magnet links or paths to torrent files will also work.

> [!NOTE]  
> For optimal performance, consider increasing file descriptor limits: `ulimit -n 65536`

## More Info
- 🤝[Contributing](CONTRIBUTING.md): How you can contribute to the project (technical and non-technical).
- ❓[FAQ](FAQ.md): Find answers to common questions about Superseedr.
- 📜[Changelog](CHANGELOG.md): See what's new in recent versions of Superseedr.
- 🗺️[Roadmap](ROADMAP.md): Discover upcoming features and future plans for Superseedr.
- 🧑‍🤝‍🧑[Code of Conduct](CODE_OF_CONDUCT.md): Understand the community standards and expectations.

## ⚡ Quick Start (Advanced)
```bash
# Recommended (native install)
cargo install superseedr

# Docker (No VPN):
# Uses internal container storage. Data persists until the container is removed.
docker run -it jagatranvo/superseedr:latest

# Docker Compose (Gluetun with your VPN):
# Requires .env and .gluetun.env configuration (see below).
docker compose up -d && docker compose attach superseedr

```

## Running with Docker (Advanced)

Superseedr offers a fully secured Docker setup using Gluetun. All BitTorrent traffic is routed through a VPN tunnel with dynamic port forwarding and zero manual network configuration.

If you want privacy and simplicity, Docker is the recommended way to run Superseedr.

Follow steps below to create .env and .gluetun.env files to configure OpenVPN or WireGuard.

<details>
<summary><strong>Click to expand Docker Setup</strong></summary>

### Setup

1.  **Get the Docker configuration files:**
    You only need the Docker-related files to run the pre-built image, not the full source code.

    **Option A: Clone the repository (Simple)**
    This gets you everything, including the source code.
    ```bash
    git clone https://github.com/Jagalite/superseedr.git
    cd superseedr
    ```
    
    **Option B: Download only the necessary files (Minimal)**
    This is ideal if you just want to run the Docker image.
    ```bash
    mkdir superseedr
    cd superseedr

    # Download all compose and example config files
    curl -sL \
      -O https://raw.githubusercontent.com/Jagalite/superseedr/main/docker-compose.yml \
      -O https://raw.githubusercontent.com/Jagalite/superseedr/main/docker-compose.common.yml \
      -O https://raw.githubusercontent.com/Jagalite/superseedr/main/docker-compose.standalone.yml \
      -O https://raw.githubusercontent.com/Jagalite/superseedr/main/.env.example \
      -O https://raw.githubusercontent.com/Jagalite/superseedr/main/.gluetun.env.example

    # Note the example files might be hidden run the commands below to make a copy.
    cp .env.example .env
    cp .gluetun.env.example .gluetun.env
    ```

2.  **Recommended: Create your environment files:**
    * **App Paths & Build Choice:** Edit your `.env` file from the example. This file controls your data paths and which build to use.
        ```bash
        cp .env.example .env
        ```
        Edit `.env` to set your absolute host paths (e.g., `HOST_SUPERSEEDR_DATA_PATH=/my/path/data`). **This is important:** it maps the container's internal folders (like `/superseedr-data`) to real folders on your computer. This ensures your downloads and config files are saved safely on your host machine, so no data is lost when the container stops or is updated.

    * **VPN Config:** Edit your `.gluetun.env` file from the example.
        ```bash
        cp .gluetun.env.example .gluetun.env
        ```
        Edit `.gluetun.env` with your VPN provider, credentials, and server region.

#### Option 1: VPN with Gluetun (Recommended)

Gluetun provides:
- A VPN kill-switch
- Automatic port forwarding
- Dynamic port changes from your VPN provider

Many VPN providers frequently assign new inbound ports. Most BitTorrent clients must be restarted when this port changes, breaking connectability and slowing downloads.
Superseedr can detect Gluetun’s updated port and reload the listener **live**, without a restart, preserving swarm performance.

1.  Make sure you have created and configured your `.gluetun.env` file.
2.  Run the stack using the default `docker-compose.yml` file:

```bash
docker compose up -d && docker compose attach superseedr
```
> [!TIP]
> To detach from the TUI without stopping the container, use the Docker key sequence: `Ctrl+P` followed by `Ctrl+Q`.
> **Optional:** press `[z]` first to enter power-saving mode.

---

#### Option 2: Standalone

This runs the client directly, exposing its port to your host. It's simpler but provides no VPN protection.

1.  Run using the `docker-compose.standalone.yml` file:

```bash
docker compose -f docker-compose.standalone.yml up -d && docker compose attach superseedr
```
> [!TIP]
> To detach from the TUI without stopping the container, use the Docker key sequence: `Ctrl+P` followed by `Ctrl+Q`.
> **Optional:** press `[z]` first to enter power-saving mode.

</details>
