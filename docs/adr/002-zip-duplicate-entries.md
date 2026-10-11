# ADR 002 — Entradas ZIP duplicadas: aceitar, sinalizar e contar por nome único

Issue: [#84](https://github.com/nadabgfccvd/RustyDroid/issues/84) · Status: aceito · Data: 2026-10-11

## Contexto

O leitor de ZIP (`crates/rd-apk/src/zip.rs`) monta a lista de entradas a
partir do central directory sem verificar nomes repetidos. Um APK com duas
entradas `classes.dex` era descrito como se fossem dois arquivos distintos:
o resumo do `rd inspect` contava **4 dex** num APK com 3 DEX reais e uma
duplicata — sem nenhum aviso. As buscas por nome (`Zip::find`) devolviam a
primeira ocorrência, silenciosamente.

Princípios envolvidos: "Sem falha silenciosa" (`RustyDroid.md`) e "never a
silent failure" (`README.md`). O `inspect` é a base das decisões de
compatibilidade do projeto, então descrever o APK com contagem errada tem
impacto alto.

Limitação conhecida: **não foi verificado** neste ambiente como o instalador
Android resolve nomes duplicados (não há emulador nem dispositivo). Esta ADR
trata do comportamento do `rd` e não afirma qual decisão o sistema toma.

## Opções

1. **Rejeitar o APK** (`INVALID_FORMAT` no parse, com o nome duplicado).
   Simples e "seguro", mas impede justamente a análise do caso ambíguo:
   nomes duplicados são um artifício conhecido de empacotadores/obfuscadores
   para que leitores ingênuos vejam um arquivo diferente do que o runtime
   vê. Uma ferramenta de análise que se recusa a abrir esse APK entrega
   menos informação exatamente onde ela importa.
2. **Aceitar e sinalizar** (escolhida): o ZIP continua parseando, o nome
   repetido é exposto em `Zip::duplicates()`/`Apk.zip_duplicates`, o
   inventário passa a contar por **nome único** e o `inspect` imprime uma
   linha de aviso (e o JSON ganha a chave `zip_duplicates`).
3. **Aceitar em silêncio, só corrigir a contagem**: remove a distorção, mas
   mantém a escolha invisível — viola "sem falha silenciosa".

## Escolha

**Opção 2.** Aceitar e sinalizar.

- `Zip::parse` detecta repetições (ordem de aparição no central directory) e
  guarda em `Zip::duplicates()`;
- `Apk::parse` monta o inventário (dex, `lib/*.so`, `assets/*`, `res/*`)
  pulando nomes já vistos — a contagem de DEX no `inspect` deixa de inflar;
- `rd inspect` imprime
  `zip duplicates     ⚠ N nome(s) repetido(s) no ZIP: <nomes> — rd usa a 1ª ocorrência`
  e, no `--json`, a chave `zip_duplicates` (omitida quando vazia);
- **Leitura por nome continua na 1ª ocorrência** (`Zip::find`/`Zip::read`).
  É a decisão conservadora: mantém o comportamento atual de todos os
  consumidores (`rd dex`, `rd vm`, assinatura) em vez de adotar "última
  ocorrência" sem evidência do que o Android faz.

## Consequências

- APKs sem duplicatas — a imensa maioria — não mudam: nenhuma linha nova na
  saída humana, nenhuma chave nova no JSON.
- Um APK ambíguo passa a ser **descrito como ambíguo** em vez de descrito
  como se tivesse mais arquivos do que tem.
- Se futuramente ficar estabelecido (com evidência) qual ocorrência o
  Android usa, a política de leitura pode mudar num único ponto
  (`Zip::find`) — a sinalização e a contagem por nome único permanecem.
