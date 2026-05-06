# just-voip

Tiny peer-to-peer voice chat widget. LAN-first, works across networks too.

## Features
- Auto-discovers peers via UDP broadcast (no config needed on same LAN)
- Supports any number of peers
- Small native GUI widget (macOS + Windows)
- ~3MB binary, zero dependencies at runtime

## Building

```
cargo build --release
```

Binary at `target/release/just-voip`

### Cross-compile for Windows (from macOS)

```
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

## Usage

Just run it. If you're on the same LAN, peers auto-discover each other via broadcast.

```
./just-voip
```

The widget shows:
- Your ID (random hex)
- Connected peers with status dots (● fresh, ◐ stale, ○ expired)
- Mute toggle
- Volume slider

## How it works

1. **Discovery**: Broadcasts `HELLO <id> <port>` on UDP 49999 every 2s
2. **Audio**: Captures mic at device native sample rate, sends raw PCM over UDP 5000
3. **Playback**: Mixes incoming audio from all peers with volume control
4. **Mute**: Stops sending audio entirely (no silent packets)

## Ports

- UDP 5000: audio
- UDP 49999: discovery

For cross-LAN use, open these ports and manually configure peer IPs (TODO: add manual peer entry in GUI).
