# M11 — Engine WASM (slice S1)

> Decisão e critério de "rodar": [ADR 013](adr/013-m11-engine-execution-strategy.md).
> Este documento descreve o que **já executa** e o que **ainda não** executa.

## Status

**Parcial (S1 + S1b em nível de VM).** O núcleo está em `crates/rd-engine-wasm`:
parser, validador, interpretador do subconjunto MVP i32/i64, limites, fuel e
harness de fuzzing. A bridge `io.rustydroid.wasm.WasmBridge` está em
`crates/rd-vm/src/wasm_bridge.rs`, atrás da feature `wasm-engine` (desligada por
padrão), com um golden de VM em `crates/rd-vm/tests/wasm_bridge.rs`: um
`Activity` real chama a bridge dentro de `onCreate`, via `launch_app`, e o
resultado volta ao heap. **Ainda não** existem: APK golden de demonstração em
`golden/engines/wasm/` com `rd test`, a ligação no registro (`wasm` continua
`runtime_status = "not_implemented"` em `data/engines.toml`) e a execução pela
CLI (`rd engine check wasm` ainda responde `NOT_IMPLEMENTED`). Por isso o M11
**não** está concluído no [ROADMAP](ROADMAP.md).

## Contrato da bridge (S1b, congelado)

```text
package io.rustydroid.wasm;
public final class WasmBridge {
    public static int call(String asset, String export, int a, int b);
}
```

- `asset`: caminho relativo a `assets/` do APK. Caminhos absolutos, vazios,
  com `..`, `.` ou barra invertida são rejeitados como
  `FileNotFoundException`, antes de qualquer leitura do ZIP.
- `export`: função exportada com tipo `(i32, i32) -> i32`. Os dois inteiros
  são os argumentos; o resultado é o único `i32` devolvido.
- Cada chamada instancia o módulo do zero. Nenhum estado persiste entre
  chamadas. Orçamento de fuel por chamada: `limits::MAX_FUEL` (10 M).
- Qualquer falha vira `java.lang.IllegalStateException` cuja mensagem começa
  pelo código estável do `rd-engine-wasm` (`PARSE_ERROR`, `INVALID_FORMAT`,
  `NOT_FOUND`, `FUEL_EXHAUSTED`, `TRAP`, ...). Export com assinatura diferente
  de `i32` único também é `IllegalStateException` (`INVALID_ARG`).
- Sem `System.loadLibrary`. Sem threads, sem memória compartilhada com o host.

Testes (`cargo test -p rd-vm --features wasm-engine --test wasm_bridge`, 8/8):
retorno correto, aritmética i32 com wrap, asset ausente, caminho hostil,
export ausente, bytes inválidos, laço infinito esgotando o fuel e o ciclo de
vida completo de `Activity`. Os módulos são montados byte a byte no próprio
teste; nenhum `.wasm` binário é commitado.

## Escopo do que executa

Interpretador do subconjunto MVP (ADR 013 §4), em Rust puro sem dependências,
com `unsafe` proibido pelos lints do workspace:

- tipos `i32` e `i64`; constantes, aritmética, comparações, conversões
  (`i32.wrap_i64`, `i64.extend_i32_s/u`) e os operadores de extensão de sinal
  (`i32.extend8_s` etc.), que o LLVM emite por padrão;
- controle estruturado: `block`, `loop`, `if`/`else`, `br`, `br_if`, `br_table`,
  `return`, `unreachable`, `nop`;
- chamadas diretas `call`; `drop`, `select`;
- variáveis locais, globais mutáveis e imutáveis (inicializadas por constante);
- memória linear única: `load`/`store` de 8, 16, 32 e 64 bits com extensão,
  `memory.size`, `memory.grow`;
- seções de dados ativas (offset constante), exportações de função/memória/global,
  função `start`; seções customizadas são ignoradas.

Fora do subconjunto, com `NOT_IMPLEMENTED` (nunca um stub silencioso): ponto
flutuante (tipos, constantes, operadores), SIMD, tabelas e `call_indirect`,
imports de módulo, multi-valor, `element` e segmentos passivos, instruções
`0xFC` (bulk memory e conversões saturadas), atômicas e múltiplas memórias.

## Limites duros

Constantes em `rd_engine_wasm::limits` (fonte única):

| Limite | Valor | Origem |
|---|---|---|
| Tamanho do módulo | 2 MiB | ADR 013 §5 |
| Memória linear | 256 páginas = 16 MiB | ADR 013 §5 |
| Fuel por invocação | 10 000 000 instruções | ADR 013 §5 |
| Argumentos por chamada | 16 | ADR 013 §5 (igual a `native_execution.rs`) |
| Payload de chamada/resposta | 16 KiB | ADR 013 §5 (exportado; aplicado pela bridge) |
| Profundidade de chamadas | 256 quadros | S1 |
| Pilha de operandos | 65 536 valores | S1 |
| Locais vivos na pilha de chamadas | 65 536 slots (~1 MiB) | S1 |
| Locais declarados por função | 50 000 | MVP |

Excesso ou falta gera `RdError`-like `WasmError` com código estável:
`PARSE_ERROR`, `INVALID_FORMAT`, `NOT_IMPLEMENTED`, `LIMIT_EXCEEDED`,
`INSTANTIATION_FAILED`, `NOT_FOUND`, `INVALID_ARG`, `TRAP`, `FUEL_EXHAUSTED`,
`STACK_EXHAUSTED`. Nenhum caminho entra em pânico (Lei 1).

## Determinismo e isolamento

- Fuel é debitado uma vez por instrução executada; `fuel_used` é a contagem real
  (Lei 4). Mesma entrada, mesmo resultado e mesmo `fuel_used`
  (`execution_is_deterministic`).
- Sem relógio, sem aleatoriedade, sem rede, sem imports e sem I/O no convidado.
- O módulo é validado por completo antes da instanciação: tipos de operandos,
  altura da pilha (incluindo código inalcançável), índices, alinhamento de memória,
  imutabilidade de globais e nomes de exportação.

## Medições (host de desenvolvimento, não moto-e5)

`cargo run --release -p rd-engine-wasm --example measure` (fib recursivo):

| Entrada | Resultado | Instruções | Tempo |
|---|---|---|---|
| fib(10) | 55 | 1 855 | 0,018 ms |
| fib(20) | 6 765 | 229 852 | 1,856 ms |
| fib(25) | 75 025 | 2 549 239 | 20,405 ms |

Isto dá cerca de 125 milhões de instruções por segundo no host de build. O
orçamento de 10M de fuel corresponde, neste host, a algo como 80 ms. A conversão
para moto-e5 (fator 0,25 do ADR 008) e a escolha final do limite dependem de
medição no próprio aparelho ou em emulação calibrada. Este número ainda não foi
medido lá.

## Testes e fuzzing

- `tests/wasm_mvp.rs` (25 testes): módulos montados à mão em `tests/support/`,
  cobrindo semântica, traps, fuel, limites, validação e negativos.
- `tests/fuzz_smoke.rs` (3 testes): mutação determinística de 20 000 módulos e
  5 000 entradas aleatórias. A propriedade é "nunca pânico, só erro estruturado".
  Cerca de 2,5% das mutações chegam ao interpretador, o suficiente para exercitá-lo.
- `fuzz/fuzz_targets/wasm_module.rs`: alvo libFuzzer (`cargo fuzz build` compila
  no CI, job `fuzz-build`). Entrou na campanha noturna `fuzz.yml`. A execução
  libFuzzer não foi feita neste ambiente, por falta de toolchain nightly.
- Verificado no MSRV 1.76 (`cargo +1.76 test -p rd-engine-wasm`).

## Próximos passos (S1b restante)

1. `golden/engines/wasm/`: APK demo de verdade, construído do fonte, com um
   `.wasm` real (hoje o golden é de VM, e os módulos são montados no teste).
   Asserções via `rd test` (JSON → JUnit) com `observe`/`wait_for`.
2. Ligar `wasm-engine` em `rd-engine-registry` e `rd-cli`, e só então mudar o
   estado para "runtime parcial", com a frase
   **"WASM hospedado pelo RustyDroid via bridge documentada"**.
3. Só depois do merge do S1b começam S2 (React Native), S3 (Mono) e S4 (Flutter).
