# Security Policy

> **Experimental project.** openSystem is not recommended for production use. Security guarantees are limited at this stage.

## Supported Versions

| Version | Status |
|---------|--------|
| v1.0.x  | Active development — best-effort fixes |
| 0.x.x   | Experimental — best-effort fixes only |

## Reporting a Vulnerability

**Please do not open a public GitHub Issue for security vulnerabilities.**

Report security issues by emailing: **soolaugust@gmail.com**

Include in your report:
- Description of the vulnerability
- Steps to reproduce
- Affected component(s) and version
- Potential impact assessment
- Any suggested mitigations (optional)

This is a personal project maintained on a best-effort basis. Response times may vary, but we will acknowledge receipt and work toward a fix as quickly as possible.

## Disclosure Policy

We follow [coordinated disclosure](https://en.wikipedia.org/wiki/Coordinated_vulnerability_disclosure):

1. Report received → acknowledged as soon as possible
2. We investigate and develop a fix
3. Fix released → you are notified
4. Public disclosure coordinated with reporter

We will credit you in the release notes unless you prefer to remain anonymous.

## Security Design Notes

### API Key Storage

API keys are stored in `/etc/os-agent/model.conf` with:
- File permissions `0o600` (owner read-only)
- XOR obfuscation keyed to `/etc/machine-id`

This is **obfuscation, not encryption**. The key can be recovered by anyone with read access to the file and `/etc/machine-id`. Protect the file with appropriate OS-level access controls.

### AI in the Syscall Path

openSystem routes OS operations through an LLM. This is an intentional architectural choice with known implications:
- AI responses are not deterministic
- A compromised or manipulated LLM endpoint can influence system behavior
- Use a trusted, self-hosted inference endpoint in sensitive environments

### Network Dependency

openSystem requires a remote LLM endpoint by design. Ensure your inference endpoint is accessed over TLS and that the API key has the minimum required permissions.

### WASM App Sandbox

Apps run inside a Wasmtime sandbox (`wasm32-wasip1`). Capabilities are restricted to what is explicitly granted via `os-syscall-bindings`. Do not grant filesystem or network capabilities beyond what an app requires.

### App Store Upload Authentication

The app store (`POST /api/apps/upload`) supports optional API key authentication:

- Set `OPENSYSTEM_STORE_API_KEY` environment variable to a secret key.
- Clients must pass the key in the `X-Api-Key` request header.
- When the env var is **not set** (or empty), authentication is skipped — this is the default development-mode behavior for local testing.

**Known limitations:**
- Authentication is a simple bearer token comparison; there is no per-user access control or key rotation mechanism.
- No rate limiting is implemented. A public-facing store should be placed behind a reverse proxy (e.g. nginx) with connection-rate and request-rate limiting.
- HTTPS is not enforced by the server itself; terminate TLS at the reverse proxy layer.
