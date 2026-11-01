# ADR 020 — M11 S2: React Native (LANG-05) — estratégia de execução

- **Status:** proposto (aceito quando o dono mergear — precedente do ADR 008 e do ADR 013); nenhuma linha de execução de RN é entregue por este ADR
- **Data:** 2026-11-01 · **Escopo:** M11 S2 (Apêndice G, LANG-05, entrada #12 `react-native`), crate futuro `rd-engine-react-native`, `data/engines.toml`, `golden/engines/react-native/`
- **Relacionados:** ADR 003 (registro ≠ execução), ADR 004 (isolamento e evidência golden), ADR 008 (piso moto-e5, orçamentos e Leis 1/2/4), ADR 013 (ordem WASM → RN → Mono → Flutter e fatiamento S1–S4)
- **Divergência da spec:** a PARTE 4 descreve LANG-05 como "engine JS embutida + ponte RN/Expo→views (experimental)". Este ADR só propõe como fatiar isso; não diverge do alvo.

## Contexto

O S1 do M11 (WASM) fechou com o adaptador `wasm` no registro (`partial_wasm`, ADR 013 adendo S1c).
O próximo slice pela ordem do ADR 013 é React Native (#12). Diferente do WASM, RN depende de uma
engine JavaScript completa: o framework do React Native (reconciler, `__fbBatchedBridge`,
módulos nativos via `NativeModules`/`TurboModules`) é JavaScript de produção, minificado e, em
APKs release, compilado para Hermes bytecode (HBC) ou exportado como JSC/JS puro.

Fatos que o registro já usa, sem executar nada:

- o bundle fica em `assets/index.android.bundle`;
- a lib nativa típica é `libreactnativejni.so` (ou `libreactnative.so`), e Hermes aparece como `libhermes.so`;
- `data/engines.toml` marca `react-native` como `not_implemented` e diz que Hermes sozinho não prova RN.

## Opções

| Opção | Descrição | Prós | Contras |
|---|---|---|---|
| A. Engine JS em Rust, bundle-fonte | Embutir um interpretador JS escrito em Rust (candidato: `boa_engine`, a verificar: MSRV, licença, conformidade ES, tamanho) e rodar bundles **não** compilados | Sem formato proprietário; testável com bundles de texto | Só cobre bundle de texto; release de RN costuma vir em HBC; o framework precisa de ES2018+ |
| B. Intérprete de Hermes bytecode | Implementar o parser e o intérprete de HBC (formato versionado por release do Hermes) | Cobre o APK real de release | Formato muda por versão; precisa dos objetos e builtins do Hermes; alto custo |
| C. Executar `libhermes.so` pelo caminho nativo | Rodar a lib C++ de Hermes no worker Unicorn do ADR 004 | Reaproveita o binário real | Exige libc++/STL e ABI que o caminho ELF/JNI limitado não cobre (`native_execution.rs` documenta esse limite); ADR 004 veda |
| D. Adiar RN e seguir para S3 (Mono) | Mantém a ordem do ADR 013 e fecha LANG-07 antes | Entrega valor sem engine JS | Quebra a ordem já decidida no ADR 013 |

**Recomendação:** começar por **A** como spike com medição, com um golden de bundle de texto que chama
um módulo nativo fake pela ponte. Só depois decidir **B** com base no tamanho, no tempo e na conformidade
medidos no spike. **C** fica rejeitado enquanto o caminho nativo for limitado. Se o spike de A
mostrar conformidade ou orçamento inaceitáveis no moto-e5, o dono decide entre B e D.

## Critério de "rodar" (proposto, espelha ADR 013 §10)

Um golden de RN só conta como execução se mostrar, com evidência:

1. o bundle (sha256, fonte e licença) é lido do APK, não de um caminho do host;
2. a ponte chama pelo menos um módulo nativo e recebe retorno (ida e volta);
3. o texto ou a árvore de UI resultante bate com um valor esperado;
4. os contadores de execução JS (instruções ou fuel) são medidos e positivos;
5. o relatório usa a frase "RN hospedado pelo RustyDroid via bridge documentada" e não promete
   Expo, TurboModules completos, Fabric nem Hermes bytecode, a não ser que a opção B tenha sido escolhida.

## Limites que este ADR não resolve

- Não há decisão ainda sobre dependência nova (`boa_engine` ou outra): exige ADR próprio de dependência,
  com MSRV, licença, tamanho e revisão de `cargo deny`.
- Não há medição no moto-e5. Qualquer número de tempo ou memória do spike é de host de desenvolvimento
  até ser medido no aparelho ou em emulação calibrada (ADR 008).

## Próximos passos

1. Dono aprova ou rejeita este ADR (ou escolhe A, B ou D).
2. Se A: ADR de dependência para o engine JS, depois spike com o golden de bundle de texto.
3. Se B ou D: nova rodada de decisão antes de qualquer código.
