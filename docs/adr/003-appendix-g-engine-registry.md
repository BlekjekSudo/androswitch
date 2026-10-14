# ADR 003 — Registro de engines separado de execução

- **Status:** aceito para registro/detecção; novos runtimes continuam pendentes
- **Data:** 2026-10-14
- **Escopo:** Apêndice G, 28 entradas e LANG-01…LANG-12

## Contexto

O Apêndice G mistura linguagens de origem, formatos compartilhados (DEX/ELF) e
frameworks. Um `classes.dex` não prova Java em vez de Kotlin/Scala; uma `.so` não
prova C em vez de Rust/Zig. JNI/NDK ainda são stubs e M9/M11 não estão concluídos.
O dono do repositório escolheu completar **registro e detecção**, não anunciar
execução dos 28 ecossistemas.

## Decisão

1. `data/engines.toml` é o catálogo versionado das 28 entradas, módulos,
   dependências, estratégias, assinaturas e estado REAL de runtime.
2. A crate segura `rd-engine-registry` valida o catálogo e analisa nomes de
   entradas ZIP e declarações do manifest. Não extrai arquivos, carrega `.so`,
   executa código, consulta rede nem descomprime assets. O payload dos artefatos
   NÃO é validado; nomes/declarações podem ser falsificados.
3. A detecção separa `signature` (marcadores de framework), `candidate`
   (formato compatível/ambíguo) e `not_detected` (sem evidência, não exclusão).
   Um APK híbrido pode ter várias assinaturas; nenhuma delas prova execução.
4. O catálogo está sempre disponível, mesmo no build mínimo. Features Cargo
   `lang-01`…`lang-12` expressam seleção e transitividade para os contratos;
   **não baixam nem implementam engines**. Seleção de runtimes no manifest e
   checkboxes do spec permanece inalterada nesta entrega.
5. O contrato público é `EngineAdapter::parse(artifact) -> Process`,
   `event(process, event)` e `syscall(process, call)`. Os adaptadores do registro
   são indisponíveis e SEMPRE respondem `NOT_IMPLEMENTED(LANG-xx)`, com ou sem
   feature. Não retornam um processo fictício. A VM DEX existente continua
   acessível pelas rotas `rd vm`/`rd app`, com cobertura parcial documentada.
6. `rd engine list|detect|check`, `rd inspect --json` e a tool MCP `app_info`
   expõem o mesmo catálogo/relatório. Não há nova tool MCP nem alteração no
   protocolo/contagem. `--data` explícito nunca cai silenciosamente no embedded.
7. Relatórios/spec gerados e testes travam IDs, 28 casos golden de assinaturas,
   features/dependências e isolamento (mínimo, individual e all-features).
   Evidências e diagnósticos têm amostras limitadas; excesso de entradas é
   erro tipado, não alocação ilimitada.

## Consequências

- Todo o Apêndice G é consultável e auditável agora, sem promessas falsas.
- TWA/PWA podem não ter payload local; ausência de assinatura não as descarta.
  DEX/ELF comuns continuam candidatos, nunca atribuição da linguagem original.
- Registro/detecção não requer novas dependências nativas ou `unsafe`; os
  budgets de execução E5 não foram certificados por esta mudança.
- Uma engine futura precisa de sua própria crate `rd-engine-<nome>`, ADR,
  feature, adaptador real e golden flow com APK OSS antes de mudar o estado
  de runtime. Testes de detecção NÃO satisfazem esse critério de execução.

## Alternativas rejeitadas

- Marcar todas as linguagens como executáveis ao reconhecer um nome de arquivo.
- Criar dezenas de crates vazias ou embutir Flutter/Mono/CPython/wasmtime só para
  registrar metadados: mais peso, sem ganho funcional e fora do escopo aprovado.
- Inferir linguagem de origem a partir de uma `.so` genérica ou bibliotecas
  de dependências Kotlin presentes no APK.
