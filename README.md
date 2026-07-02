# Lockr

A secure, offline, cross-platform password vault built with Tauri, Rust, TypeScript, and Vue.js.

This application prioritizes security by keeping all business logic, state management, and cryptography on the Rust backend. The Vue frontend acts strictly as a "dumb" display layer, ensuring sensitive data is handled safely.

## Features

- **Strong Encryption at Rest**: Vault data is encrypted using AES-256-GCM before hitting your hard drive.
- **Resistant Key Derivation**: Master passwords are hashed using Argon2id (64MB memory, 3 iterations), making brute-force attacks computationally infeasible.
- **Memory Safety**: Uses the `zeroize` crate to ensure passwords and encryption keys are securely wiped from RAM when no longer needed.
- **Zero-Knowledge Frontend**: The Vue UI never receives your passwords unless you explicitly click "Reveal". Decrypted vaults are stripped of secrets before crossing the Tauri bridge.
- **Local-First & Offline**: No servers, no cloud, no network requests. Your data stays on your machine.
- **Cross-Platform**: Built with Tauri, resulting in a small, native-feeling application for Windows, macOS, and Linux.
- **Hierarchical Structure**: Organize credentials logically by Vault → Service → Account.

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

1. **The Safe View Pattern**: The Rust backend maintains the true, decrypted `Vault` struct. When sending data to the UI, it translates it into `SafeVault`, `SafeService`, and `SafeAccount` structs, intentionally stripping out the `secret` fields.
2. **On-Demand Decryption**: Passwords are only fetched individually via a dedicated `get_secret` command when the user explicitly requests to view or copy them.
3. **Zeroizing on Lock**: When a vault is locked, the `ZeroizeOnDrop` trait automatically cascades through the vault structure, overwriting plain-text passwords in memory with zeros before the memory is freed.

## Installation

### Prerequisites

- [Node.js](https://nodejs.org/) (v16+)
- [Rust](https://www.rust-lang.org/tools/install) (Latest stable)
- [Tauri CLI](https://tauri.app/v1/guides/getting-started/prerequisites)

### Build from Source

```bash
# Clone the repository
git clone https://github.com/DylanAlmond
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

## Usage (W.I.P)

1. **Create a Vault**: Open the app and click "Create New Vault". Enter a name and a strong master password.
2. **Unlock**: Once created, the vault unlocks automatically. If you restart the app, select your vault from the list and enter your master password to unlock it.
3. **Add Services**: Click "Add" in the Services column to create categories (e.g., GitHub, Gmail, Netflix).
4. **Add Accounts**: Select a service and fill in the Username, Password, and optional Display Name/Email fields.
5. **View & Copy Passwords**: Click the "Reveal" button next to an account to fetch the password from Rust securely into local component state. Click "Copy" to send it to your clipboard.
6. **Editing**: Double-click or click "Edit" on Vaults, Services, or Accounts to modify them. Leaving optional fields (like Email) blank and saving will clear them.
7. **Lock**: Click the red "Lock Vault" button at the top left. This immediately wipes the vault and master password from the application's memory.

## Project Structure

```
├── src/                      # Vue Frontend
│   ├── components/           # UI components
│   ├── composables/          # Vue hooks
│   └── App.vue
└── src-tauri/                # Rust Backend
    ├── src/
    │   ├── main.rs           # Rust project root (unchanged)
    │   ├── lib.rs            # Tauri setup & command registration
    │   ├── commands.rs       # Tauri IPC bridge
    │   ├── models.rs         # Core data structures & Safe View Models
    │   ├── vault_manager.rs  # Business logic & state management
    │   ├── crypto.rs         # Argon2 & AES-GCM implementations
    │   └── error.rs          # Custom error handling
    └── Cargo.toml
```

## Contributing

Pull requests are welcome. For major changes, please open an issue first to discuss what you would like to change.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
