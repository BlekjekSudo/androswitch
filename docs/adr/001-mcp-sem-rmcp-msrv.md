# ADR 001 — Servidor MCP do rd-agent sem rmcp (JSON-RPC 2.0 próprio, zero deps)

## Contexto

A spec (RustyDroid.md, "DECISÕES TÉCNICAS JÁ TOMADAS") fixa `rmcp p/ MCP;
tokio só no rd-agent`. O M5 exige um servidor MCP v1 com ~20 tools.

Fato novo verificado em 2026-10: todas as versões publicadas do `rmcp`
(até a 3.5.1) declaram MSRV **1.88**; o workspace é **MSRV 1.76**
(decisão também vinculante da spec, guardada pelo job `rust-msrv` do CI).
Adotar rmcp hoje quebraria o MSRV — violação de contrato, não escolha.

## Opções

1. **rmcp + subir MSRV para 1.88** — viola decisão vinculante + exclui
   contribuidores/CI de toolchains mais velhas; o piso é produto.
2. **rmcp 0.x antigo** — versões sem `rust_version` declarado, APIs
   instáveis pré-1.0; mesmo assim puxa tokio full + tower para o que, no
   v1 stdio, é um envelope JSON-RPC.
3. **JSON-RPC 2.0 sobre stdio implementado no rd-agent** (serde_json já é
   dependência) — o MCP em transporte stdio É newline-delimited JSON-RPC
   2.0 (`initialize`, `notifications/initialized`, `tools/list`,
   `tools/call`, `ping`); envelope pequeno e testável; mantém o padrão do
   projeto (ZIP/AXML/ARSC/PNG/zlib próprios, zero deps externas).

## Escolha

**Opção 3.** O protocolo na fio é idêntico ao MCP (clientes oficiais —
Claude, Gemini, OpenAI, MCP Inspector — conectam sem saber a diferença);
o envelope de sessão v1 é ~200 linhas testadas; zero deps novas; MSRV 1.76
intacto.

## Consequências

- Quando o MSRV do workspace subir para ≥1.88 (M7+ plausível), re-avaliar
  rmcp; o contrato das tools (nomes/args/resultados) não muda — só o
  envelope.
- Features de MCP que o v1 não usa (resources, prompts, sampling,
  streaming SSE/HTTP) ficam fora do envelope até demandarem — mesmas
  respostas de erro tipado do resto do projeto.
- tokio segue FORA do workspace (a fila do agente é single-thread
  determinística — decisão M3); o servidor stdio é um loop de leitura
  bloqueante, uma sessão por processo.
