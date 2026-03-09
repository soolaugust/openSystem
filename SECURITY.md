# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅ Yes    |

## Reporting a Vulnerability

**Do not open a public GitHub Issue for security vulnerabilities.**

Report security issues by emailing: **soolaugust@gmail.com**

Include in your report:
- Description of the vulnerability
- Steps to reproduce
- Affected component(s) and version
- Potential impact assessment
- Any suggested mitigations (optional)

You will receive an acknowledgement within **48 hours** and a status update within **7 days**.

## Disclosure Policy

We follow [coordinated disclosure](https://en.wikipedia.org/wiki/Coordinated_vulnerability_disclosure):

1. Report received → acknowledged within 48 hours
2. We investigate and develop a fix
3. Fix released → you are notified
4. Public disclosure after **90 days** from initial report (or sooner if both parties agree)

We will credit you in the release notes unless you prefer to remain anonymous.

## Security Design Notes

### API Key Storage

API keys are stored in `/etc/os-agent/model.conf` with:
- File permissions `0o600` (owner read-only)
- XOR obfuscation keyed to `/etc/machine-id`

This is **obfuscation, not encryption**. The key can be recovered by anyone with read access to the file and `/etc/machine-id`. Protect the file with appropriate OS-level access controls.

### AI in the Syscall Path

AIOS routes OS operations through an LLM. This is an intentional architectural choice with known implications:
- AI responses are not deterministic
- A compromised or manipulated LLM endpoint can influence system behavior
- Use a trusted, self-hosted inference endpoint in sensitive environments

### Network Dependency

AIOS requires a remote LLM endpoint by design. Ensure your inference endpoint is accessed over TLS and that the API key has the minimum required permissions.

### WASM App Sandbox

Apps run inside a Wasmtime sandbox (`wasm32-wasip1`). Capabilities are restricted to what is explicitly granted via `os-syscall-bindings`. Do not grant filesystem or network capabilities beyond what an app requires.
