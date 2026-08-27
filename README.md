# sipcall

**Standalone Rust SIP phone - no hosted services, direct peer-to-peer calling**

Two laptops can call each other over a LAN or VPN with zero infrastructure. A real cell phone still needs a SIP trunk you control (your own Asterisk/FreeSWITCH PBX, not a vendor API).

## What This Is

- ✅ Direct SIP-to-SIP calling between computers
- ✅ Optional registration to your own PBX/trunk
- ✅ G.711 μ-law (PCMU) audio codec
- ✅ UDP SIP and RTP
- ✅ MIT licensed

## What This Is NOT

- ❌ Not a phone company
- ❌ Not Twilio/Telnyx/Vonage/Bandwidth
- ❌ Not a robocaller
- ❌ Not connected to the PSTN (public phone network) by default
- ❌ Not a bulk calling tool

To reach actual phone numbers, you need your own SIP trunk or PBX. This tool is just a SIP user agent.

## Quick Start

### Installation

```bash
git clone https://github.com/abinesha312/sipcall.git
cd sipcall
cargo build --release
```

The binary will be at `target/release/sipcall`.

### Two-Laptop Recipe (Direct P2P)

**Laptop A (192.168.1.10):**
```bash
sipcall listen --bind 0.0.0.0:5060
```

**Laptop B (192.168.1.20):**
```bash
sipcall call 192.168.1.10:5060
```

Both laptops will establish a direct SIP call with audio. Press Ctrl-C to hang up.

### Calling Modes

#### 1. Direct SIP-to-SIP (Default)

No server needed. Two instances of `sipcall` can talk directly:

```bash
# Machine 1
sipcall listen

# Machine 2
sipcall call <machine1-ip>:5060
```

You can also use full SIP URIs:
```bash
sipcall call sip:user@192.168.1.100:5060
```

#### 2. Register to Your Own PBX (Optional)

If you have a SIP account on your own Asterisk, FreeSWITCH, or SIP provider:

```bash
export SIP_SERVER=sip.yourpbx.com
export SIP_USER=your-extension
export SIP_PASSWORD=your-password

# This will be supported in a future version
# Currently only direct P2P is implemented
```

**Note:** Registration mode is not yet implemented. This version only supports direct peer-to-peer calls.

## Usage

### Listen for incoming calls
```bash
sipcall listen [--bind 0.0.0.0:5060]
```

Default bind address is `0.0.0.0:5060`.

### Make a call
```bash
sipcall call <target>
```

Target can be:
- `192.168.1.100:5060` (IP:port)
- `192.168.1.100` (defaults to port 5060)
- `sip:user@host:port` (full SIP URI)
- `sip:user@host` (defaults to port 5060)

### During a call

- **Ctrl-C** to hang up
- Status messages:
  - `📞 Calling...` - INVITE sent
  - `📱 Ringing...` - Remote side is ringing
  - `✅ Call connected` - Call established, audio flowing
  - `🎙️ Audio session active` - Microphone and speaker active
  - `📴 Call ended` - Call terminated

## Architecture

- **SIP signaling**: UDP on port 5060 (configurable)
- **RTP audio**: Dynamically allocated UDP port
- **Codec**: G.711 μ-law (PCMU) at 8kHz
- **Audio I/O**: CPAL for cross-platform mic/speaker access

### Stack

- `rsip` - SIP message parsing
- `tokio` - Async UDP networking
- `cpal` - Audio device access
- `clap` - CLI parsing

## Testing

```bash
cargo test
```

Tests cover:
- SIP URI parsing
- INVITE/ACK/BYE message construction
- SDP parsing
- RTP packet building
- μ-law encoding/decoding

No real PSTN calls are made in tests. Tests use localhost sockets and in-memory signaling.

## Audio Fallback

If no audio devices are detected (e.g., in CI or headless environments), the tool runs in "silent mode" - SIP signaling still works, but audio is replaced with silence. This allows testing and CI without hardware.

## Network Requirements

- **Direct P2P mode**: Both machines must be on the same network or have routing/VPN between them
- **Firewall**: UDP ports 5060 (SIP) and dynamic RTP ports must be open
- **NAT**: Direct P2P works best on a LAN. For NAT traversal, you'll need STUN/TURN (not yet implemented)

## Security Notes

- **No encryption**: SIP and RTP are sent in cleartext. Use a VPN if privacy is required.
- **No authentication**: Direct mode has no authentication. Anyone who can reach your port can call you.
- **Caller ID spoofing**: When registered to a PBX, the From-URI is your registered identity. This tool does not support spoofing caller IDs.

## Honest Disclosure

This is a **learning tool and minimal SIP client**. It is not:
- A replacement for your phone company
- A way to make free phone calls to real numbers
- A bulk calling / robocalling platform
- Production-ready telecom software

To reach actual phone numbers (PSTN), you need:
1. A SIP trunk from a carrier (which costs money)
2. Or your own PBX connected to the PSTN
3. Compliance with telecom regulations in your jurisdiction

This tool is just a SIP **user agent**. It speaks the protocol. You bring the infrastructure.

## Roadmap

- [ ] SIP REGISTER for PBX authentication
- [ ] STUN/TURN for NAT traversal
- [ ] DTMF (touch-tone) support
- [ ] Multiple codec support (G.722, Opus)
- [ ] TLS/SRTP encryption
- [ ] Contact list / speed dial

## Contributing

PRs welcome! Please ensure:
- Tests pass (`cargo test`)
- No dependencies on hosted CPaaS services
- No features that facilitate robocalling or spam

## License

MIT - see [LICENSE](LICENSE)

## Credits

Built with Rust 2021 and the following libraries:
- [rsip](https://github.com/vasilakisfil/rsip) - SIP parsing
- [tokio](https://tokio.rs) - Async runtime
- [cpal](https://github.com/RustAudio/cpal) - Audio I/O
- [clap](https://github.com/clap-rs/clap) - CLI parsing

---

**Not affiliated with or endorsed by any telecom carrier.**
