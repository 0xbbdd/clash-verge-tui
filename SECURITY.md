# Security Policy

## Supported versions

Security fixes are applied to the latest `0.1.x` release and the `main` branch while the project is pre-1.0.

## Reporting a vulnerability

Please use a [private GitHub security advisory](https://github.com/0xbbdd/clash-verge-tui/security/advisories/new). Do not open a public issue for controller credential exposure, arbitrary file access, path traversal, unsafe subscription handling, or a way to bypass confirmation for destructive operations.

Include the affected revision, operating system, controller transport, minimal reproduction, expected boundary, and observed impact. Remove controller secrets and subscription credentials from all evidence.

## Deployment guidance

- Prefer Clash Verge's local Unix Controller Socket when the TUI runs on the same machine.
- If an HTTP Controller is required, bind it to a trusted interface, configure a strong Mihomo secret, and use an authenticated tunnel or HTTPS for remote access.
- Prefer `CVT_SECRET` to `--secret` so the credential does not appear in shell history or process arguments.
- Treat subscription URLs and local profiles as secrets. The TUI intentionally does not display subscription URLs in its profile table.
- Profile updates and system proxy changes affect the current user. Review the confirmation prompt and keep backups appropriate to your environment.

The TUI is not a privilege boundary. It runs with the invoking user's filesystem and desktop-session permissions.
