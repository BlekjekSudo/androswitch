# Target apps: must run end-to-end on RustyDroid

> Requisito do dono (2026-11-01). Lista de apps que **devem rodar de ponta a ponta**
> no RustyDroid. Este documento é o registro de escopo e de status. **Nada aqui
> está verificado como funcionando.** O status é o que foi comprovado no repositório
> até hoje.

## O que "ponta a ponta" significa (critério proposto)

Para cada app, o DoD é reproduzível a partir do repositório, no formato do M6/M7:

1. **Aquisição** reproduzível (ver a política abaixo), com sha256 registrado.
2. **Install** pelo `rd` e pelo MCP, sem erro.
3. **Launch** até o primeiro frame, sem `NOT_IMPLEMENTED` nem exceção.
4. **Golden flow**: um caminho real do usuário, com asserções de estado ou de texto
   (`rd test` com suíte JSON em `tests/suites/` ou `golden/`).
5. **Evidência**: screenshot ou árvore de UI, log e tempo, medidos nos orçamentos
   do moto-e5 (ADR 008). Números de host não contam como prova.

## Política de aquisição (decidida pelo dono em 2026-11-01)

- Os APKs são usados **só em testes internos**, não publicados. O método de aquisição
  não importa para o dono.
- **Nenhum APK é commitado.** Eles ficam fora do git (por exemplo `golden-apks/*.apk`,
  já ignorado, ou um diretório local fora do repositório) e entram nos testes por
  caminho em variável de ambiente, nunca por caminho fixo.
- `golden-apks/fetch.sh` (GH-05) continua sendo o caminho para OSS do F-Droid. Para os
  apps proprietários, o registro de procedência (origem e sha256) fica fora do repositório.
- Para itens sem app definido ("qualquer GTA clone", "um photo manager", "um password
  manager", "auto-clicker", "auto-scroll", "organizador de contatos") a escolha do app
  concreto é do dono. Sugestões de OSS são candidatas e precisam de confirmação.

## Lista e status

| # | App | Origem | Bloqueios principais (ver ROADMAP) | Status |
|---|---|---|---|---|
| 1 | TikTok | Play Store (proprietário) | APK grande e multi-DEX; câmera e mídia (M10 parcial); rede; serviços Google; Play Integrity | ⬜ não iniciado |
| 2 | Block Blast | Play Store (proprietário) | jogo com render GPU; render atual é só software (ADR 019 pendente) | ⬜ não iniciado |
| 3 | Chrome | Play Store (proprietário) | Chromium multiprocesso; WebView não implementado (SYS-06); código nativo muito além da rota ELF/JNI limitada (ADR 004) | ⬜ não iniciado |
| 4 | Plants vs. Zombies 1 | Play Store (proprietário) | jogo com render GPU; licenciamento e serviços Google | ⬜ não iniciado |
| 5 | Brave | Play Store (proprietário) | mesmo caso do Chrome (Chromium) | ⬜ não iniciado |
| 6 | Opera GX | Play Store (proprietário) | mesmo caso do Chrome (Chromium) | ⬜ não iniciado |
| 7 | Lucky Patcher | fora da Play Store (APK de terceiro) | altera licença e pacotes de outros apps; pede privilégios (package manager, acesso a outros pacotes, `ACCESS_SUPERUSER`) que o RustyDroid não expõe. **Obtido e verificado só o instalador:** `LuckyPatcherInstaller.apk` de `chelpus.com/luckypatcher/` (15.000.487 bytes, SHA-256 `ce85434f99153a0d99b2562cf60eef6fef180da3ec09e23e67cafbae3c62b8bf`; `rd inspect`: package `ru.aaaaabas.installer`, 12.10.9, versionCode 1979, assinado com o certificado genérico "Android Debug"). Um segundo APK de `chelpus.com` (`LuckyPatchers.com_Official_Installer_12.10.8.apk`, SHA-256 `173d4597…354cb354d`) é o mesmo tipo de instalador, com package `ru.aaaaabar.installer`, e foi rejeitado. O app Lucky Patcher real (`com.dimonvideo.luckypatcher`) **não foi obtido**: o instalador baixa componentes em tempo de execução e não contém o app. Nada foi executado nem commitado (GH-05). | 🟠 em andamento; **aquisição do app não verificada**, nenhum launch. Próximo passo: o dono fornece o APK `com.dimonvideo.luckypatcher` com SHA-256 conferido |
| 8 | Um clone de GTA | Play Store (a escolher) | jogo 3D com render GPU; tamanho de APK e de dados; escolha do app pendente | ⬜ app a escolher |
| 9 | Photo manager | a escolher (candidato OSS: galeria do F-Droid, a confirmar) | MediaStore e ContentProvider; decodificação de imagem; file I/O | ⬜ app a escolher |
| 10 | Password manager | a escolher (candidato OSS: KeePassDX, F-Droid, a confirmar) | Autofill framework; Android Keystore; criptografia; SQLite | ⬜ app a escolher |
| 11 | Zombie Tsunami | Play Store (proprietário) | jogo com render GPU; serviços Google | ⬜ não iniciado |
| 12 | Play Store | Google (proprietário) | Google Play Services e login de conta; Play Integrity. Há shims parciais de GMS (`rd-vm`, `gms`). **Suporte a Google Play Services é um dos últimos itens** (ver a seção "Apps alvo" do ROADMAP) | ⬜ último item (depende de M9–M10 e dos apps acima) |
| 13 | Comprix | https://github.com/nadabgfccvd/Comprix (app do dono) | launch termina antes do primeiro frame com erro tipado; ver [comprix-e2e.md](comprix-e2e.md) | 🟡 launch parcial |
| 14 | Auto-clicker | a escolher (OSS, a confirmar) | AccessibilityService (`dispatchGesture`) e overlay; M9 accessibility parcial | ⬜ app a escolher |
| 15 | Auto-scroll | a escolher (OSS, a confirmar) | AccessibilityService; overlay; mesmo caso do auto-clicker | ⬜ app a escolher |
| 16 | RAR | Play Store (proprietário) | file I/O e formatos de arquivo; código nativo a verificar | ⬜ não iniciado |
| 17 | Organizador de contatos | a escolher (OSS, a confirmar) | ContactsContract e ContentProvider no dispositivo virtual | ⬜ app a escolher |

Legenda: ⬜ não iniciado · 🟡 parcial, com evidência · ⛔ bloqueado por decisão do dono.

## Bloqueios transversais

- **Escopo do roadmap.** Hoje a validação real é o APK demo do WASM, o golden do
  Comprix (parcial) e os OSS do M8. Nenhum app desta lista tem golden flow.
  Os itens 1–12 e 16 exigem fases que ainda não existem (M4 render completo, M9/M10
  em profundidade, WebView, GMS). O prazo para "rodar perfeitamente" não cabe
  no roadmap atual, e o dono precisa decidir a ordem.
- **Render.** Jogos (4, 8, 11, 2) precisam de GPU. O render atual é software e
  headless (ADR 019 pendente).
- **Navegadores (3, 5, 6).** Dependem de WebView/Chromium (SYS-06), que não existe.
- **Play Store e serviços Google (1, 12 e vários jogos).** Dependem de GMS, que só
  tem shims parciais.

## Próximos passos

1. Lucky Patcher: falta o APK `com.dimonvideo.luckypatcher` (os instaladores de `chelpus.com` não o contêm; ver linha 7). Aguarda o dono fornecer o APK com SHA-256 ou aprovar a fonte; depois, primeiro launch.
2. Dono escolhe os apps concretos para os itens "a escolher".
3. Para cada app com APK acessível, criar o golden flow (suíte `rd test`) e registrar
   o status com evidência.
4. Google Play Services: último item, depois que os demais tiverem golden flow.
5. Atualizar este documento e `docs/ROADMAP.md` a cada fase concluída.
