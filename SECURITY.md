# Security Policy

## Reporting a vulnerability

If you discover a security vulnerability, please report it responsibly:

1. **Do not** open a public issue.
2. Email security concerns to the maintainers via the contact information on the [GitHub profile](https://github.com/vivekpal1).
3. Include a description of the vulnerability, steps to reproduce, and potential impact.

We will acknowledge receipt within 48 hours and aim to release a fix within 7 days for critical issues.

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

## Scope

kdo processes workspace manifests and source files. Security concerns include:

- Path traversal during workspace discovery
- Arbitrary code execution via malicious manifest files
- Information disclosure through the MCP server

kdo does **not** execute build commands, install dependencies, or make network requests.
