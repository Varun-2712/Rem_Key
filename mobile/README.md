# Rem Key mobile

The phone side, laid out as in `Rem_Key_Mobile_Architecture.md`:

| Path | What |
|------|------|
| `rust-bridge/` | `pir_mobile`, the UniFFI bridge. A `RelayClient` with coarse commands over the existing protocol, session and QUIC crates. |
| `uniffi-bindgen/` | The workspace's own `uniffi-bindgen`, so bindings always match the `uniffi` version in `Cargo.lock`. |
| `shared/` | Kotlin Multiplatform. `commonMain` has the `RelayClient` contract, commands and touchpad gesture logic; `androidMain` adapts the generated bindings. |
| `androidApp/` | Jetpack Compose app: a connect screen and a touchpad/keyboard screen. |

## Prerequisites

- JDK 17 or newer
- Android SDK with platform 35 (Android Studio installs it). AGP installs the NDK version pinned in `gradle/libs.versions.toml` on first build.
- Rust, the Android targets, and cargo-ndk:

```bash
rustup target add aarch64-linux-android x86_64-linux-android
```

```bash
cargo install cargo-ndk
```

## Build

Open `mobile/` in Android Studio, or from the command line (once, to create `gradlew`, with a local Gradle 8.10+):

```bash
cd mobile && gradle wrapper
```

```bash
cd mobile && ./gradlew :androidApp:installDebug
```

`preBuild` runs `:shared:cargoBuildAndroid` (cargo-ndk into `shared/build/rustJniLibs`) and then `:shared:generateUniffiBindings` (into `shared/build/generated/uniffi/kotlin`). Neither output is checked in.

Gradle properties you can override in `~/.gradle/gradle.properties` or with `-P`:

- `remkey.cargo`: path to `cargo` (default `~/.cargo/bin/cargo`; Android Studio often doesn't have it on `PATH`)
- `remkey.rustAbis`: comma-separated ABIs (default `arm64-v8a,x86_64`; use `arm64-v8a` for a faster phone-only build)

## Try it

1. Start the desktop agent with `--pair`. Next to the QR code it prints a `Pairing data: {...}` line.
2. In the app, paste that JSON into the pairing code field and tap Pair. The desktop saves the phone as trusted, and the app remembers the desktop.
3. Later, tap Connect; no secret is needed again. `pir-agent --forget <prefix>` revokes the phone, after which it has to pair again.

The phone's certificate and key live in the app's `noBackupFilesDir`, so they are never restored onto another device. Scanning the QR code with the camera and mDNS discovery are still to come.

## Testing without a phone

```bash
cargo test -p pir_mobile
```

The bridge's integration test runs `RelayClient` against a real QUIC server. It covers the handshake, sub-pixel motion accumulation, event ordering, and releasing held keys and buttons on disconnect. `tests/pairing.rs` pairs with the real desktop agent, reconnects as the same phone, and checks that unpaired phones and wrong secrets are refused. The touchpad gesture tests are in `shared/src/commonTest` and run with `./gradlew :shared:testDebugUnitTest`.

## Design

The UI follows Scandinavian design principles: warm off-white and birch tones instead of pure white, charcoal instead of black, one muted fjord-blue accent, flat surfaces separated by tone rather than shadows or outlines, light type with generous whitespace, and only the controls each screen needs. The palette, type scale and spacing live in `androidApp/.../ui/theme/Theme.kt`; light and dark follow the system.
