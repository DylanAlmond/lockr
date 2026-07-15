<p align="center">
  <img src="banner.png" alt="Project Banner" width="1200">
</p>

![License](https://img.shields.io/badge/License-MIT-blue)
![Status](https://img.shields.io/badge/Status-WiP-yellow)

## Introduction

Lockr is a secure, offline, cross-platform password vault built with Tauri, Rust, TypeScript, and Vue.js.

This application prioritizes security by keeping all business logic, state management, and cryptography on the Rust backend. The Vue frontend acts strictly as a "dumb" display layer, ensuring sensitive data is handled safely.

## Features

- **Multi-Vault Architecture**: Unlock and manage multiple vaults simultaneously (e.g., "Work" and "Personal"). Switch contexts seamlessly without re-entering passwords.
- **Lightning-Fast Cross-Vault Search**: A powerful in-memory filtering engine allows you to search across all open vaults instantly by username, email, or tags.
- **Flexible Organization**: Ditch rigid folder structures. Use a flat list of accounts organized by custom tags, favorites, and cross-vault text search.
- **Strong Encryption at Rest**: Vault data is encrypted using AES-256-GCM before hitting your hard drive.
- **Resistant Key Derivation**: Master passwords are hashed using Argon2id (64MB memory, 3 iterations), making brute-force attacks computationally infeasible.
- **Memory Safety**: Uses the `zeroize` crate and custom `Drop` implementations to ensure passwords and encryption keys are securely wiped from RAM when vaults are locked or swapped.
- **Zero-Knowledge Frontend**: The Vue UI never receives your passwords unless you explicitly request them. Decrypted vaults are stripped of secrets before crossing the Tauri bridge.
- **Local-First & Offline**: No servers, no cloud, no network requests. Your data stays on your machine.
- **Cross-Platform**: Built with Tauri, resulting in a small, native-feeling application for Windows, macOS, and Linux.

## Tech Stack

**Backend (Rust / Tauri)**

- `tauri`: Native windowing and IPC bridge.
- `aes-gcm`: Authenticated encryption.
- `argon2`: Password hashing and key derivation.
- `zeroize`: Secure memory zeroing.
- `serde` / `serde_json`: Efficient serialization.
- `uuid` / `chrono`: ID generation and timestamps.

**Frontend (Vue / TypeScript)**

- Vue 3 (Composition API)
- TypeScript
- `@tauri-apps/api` for IPC communication.

## Security Architecture

This project is built around a strict separation of concerns to minimize the attack surface:

1. **The Safe View Pattern**: The Rust backend maintains the true, decrypted `Vault` and `Account` structs. When sending data to the UI, it translates them into `SafeVault` and `SafeAccount` structs, intentionally stripping out all `secret` fields.
2. **On-Demand Decryption**: Passwords are only fetched individually via a dedicated `get_secret` command when the user explicitly requests to view or copy them.
3. **Multi-Vault Memory Management**: Unlocked vaults are held in a secure `HashMap` in RAM. When a specific vault is locked (or the app closes), the `Drop` trait is triggered, automatically cascading through that specific vault's structure and overwriting plain-text passwords with zeros before the memory is freed, without affecting other open vaults.

## Installation

### Prerequisites

- [Node.js](https://nodejs.org/) (v16+)
- [Rust](https://www.rust-lang.org/tools/install) (Latest stable)
- [Tauri CLI](https://tauri.app/v1/guides/getting-started/prerequisites)

### Build from Source

```bash
# Clone the repository
git clone https://github.com/DylanAlmond/lockr
cd lockr

# Install frontend dependencies
yarn install

# Run in development mode
yarn tauri dev
```

### Building for Production

```bash
# Build the optimized release binary
yarn tauri build
```

_The compiled installer/binary will be located in `src-tauri/target/release/bundle/`._

## Project Structure

```
├── src/                      # Vue Frontend
│   ├── assets/               # UI assets (icons, logos)
│   ├── components/           # UI components
│   ├── composables/          # Vue hooks (useVault, useUser)
│   ├── router/               # Vue Router
│   ├── types/                # Shared TypeScript interfaces
│   ├── style.css             # Global styling
│   |── main.ts               # Frontend entry
│   └── App.vue
└── src-tauri/                # Rust Backend
    ├── src/
    │   ├── main.rs           # Backend entry
    │   ├── lib.rs            # Tauri setup & command registration
    │   ├── commands.rs       # Tauri IPC bridge (Multi-vault state handling)
    │   ├── models.rs         # Core data structures, Safe View Models, & Filter DTOs
    │   ├── vault_manager.rs  # Multi-vault state, business logic, & aggregation engine
    │   ├── user_manager.rs   # Unencrypted user profile persistence
    │   ├── crypto.rs         # Argon2 & AES-GCM implementations
    │   └── error.rs          # Custom error handling
    └── Cargo.toml
```

## Contributing

Pull requests are welcome. For major changes, please open an issue first to discuss what you would like to change.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
