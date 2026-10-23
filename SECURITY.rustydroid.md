# Security Policy

> 🇧🇷 [Política de segurança em português abaixo](#política-de-segurança-pt-br)

## Supported versions

RustyDroid is pre-1.0 (spec phase → M0+). Security fixes target the **latest commit on `main`**.

## Reporting a vulnerability

**Please do NOT open a public issue for security reports.**

Use GitHub's private vulnerability reporting:
**[Security → Report a vulnerability](https://github.com/nadabgfccvd/RustyDroid/security/advisories/new)**

- Include: affected component (`rd-apk`, `rd-dex`, `rd-vm`, `rd-agent`…), reproduction
  steps or a malicious-input sample (e.g., a crafted DEX/AXML/ARSC/APK that triggers
  the issue), and impact assessment
- Response target: **72 hours** for triage, fix timeline agreed privately

## Security-relevant design notes

- RustyDroid **executes untrusted bytecode** (APKs) — the runtime is the security
  boundary. Parser bugs (DEX/AXML/ARSC/ZIP/ELF) are treated as security-critical:
  they are fuzz targets in CI (nightly `cargo-fuzz`)
- The **agent control plane (MCP/JSON-RPC)** can drive apps: snapshots, input
  injection, file access. If you embed RustyDroid as a service, put your own
  authentication/authorization in front — the MVP ships no auth layer by design
- Native code loading (planned `rd-ndk`/`rd-jni`) is the highest-risk area and is
  strictly feature-gated + audited (`unsafe` is forbidden everywhere else)

## 🇧🇷 Política de segurança (PT-BR)

**Não abra issue pública para relatos de segurança.**

Use o canal privado: **[Security → Report a vulnerability](https://github.com/nadabgfccvd/RustyDroid/security/advisories/new)**

O RustyDroid executa bytecode não confiável (APKs) — bugs de parser são críticos e
são alvo de fuzzing noturno no CI. O plano de controle do agente (MCP) permite
injetar input e acessar arquivos: se você expor o RustyDroid como serviço, adicione
sua própria camada de autenticação/autorização.
