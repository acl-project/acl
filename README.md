# ACL - Adwaita Craft Launcher

ACL is a Minecraft launcher built with GTK4, libadwaita, and Relm4, featuring a native GNOME-style interface.

## Project Architecture

This project uses a Cargo Workspace to strictly separate the UI from the core business logic:

- `crates/acl-gui`: GUI layer built with GTK4, libadwaita, and Relm4.
- `crates/acl-core`: Core launcher logic, including configuration, instance management, Java discovery, and game process launching.
- `crates/acl-minecraft`: Minecraft version manifest parsing, concurrent asset downloads, and mod loader installation.
- `crates/acl-auth`: Account authentication management, including offline accounts and Microsoft OAuth 2.0 authentication.

## Building from Source

### 1. Install System Dependencies

Before building this project, make sure you have the Rust toolchain installed (supporting `edition = "2024"`; the latest stable release is recommended), along with the GTK4 and libadwaita development libraries.

**Ubuntu / Debian:**

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libgtk-4-dev libadwaita-1-dev libssl-dev libsecret-1-dev
```

**Fedora:**

```bash
sudo dnf install -y gcc pkg-config gtk4-devel libadwaita-devel openssl-devel libsecret-devel
```

**Arch Linux:**

```bash
sudo pacman -S --needed base-devel pkgconf gtk4 libadwaita openssl libsecret
```

### 2. Build and Run

Clone the repository:

```bash
git clone https://github.com/zhenyan121/acl.git
cd acl
```

Run in development mode:

```bash
cargo run
```

Build the release version:

```bash
cargo build --release
```

### 3. Development and Code Quality

This project uses `pre-commit` to automatically run `cargo fmt` and `cargo clippy` checks before each commit.

Install the pre-commit hooks:

```bash
pre-commit install
```

Run the checks manually:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## License

This project is open source under the GPL-3.0 license. See `/LICENSE` for details.