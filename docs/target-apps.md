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

## Política de aquisição (bloqueio de decisão)

- `golden-apks/fetch.sh` (GH-05) só baixa APK OSS e redistribuível do F-Droid, e
  **nenhum binário é commitado**.
- A maioria dos itens abaixo é app proprietário da Play Store. Para esses, o repositório
  não pode baixar nem versionar o APK. Seria preciso um APK fornecido pelo dono,
  fora do repositório, por caminho em variável de ambiente, com decisão de licença
  registrada.
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
| 7 | Lucky Patcher | fora da Play Store (APK de terceiro) | altera licença e pacotes de outros apps, pede privilégios que o RustyDroid não expõe; **risco legal e de ToS: exige decisão do dono antes de qualquer trabalho** | ⛔ bloqueado por decisão do dono |
| 8 | Um clone de GTA | Play Store (a escolher) | jogo 3D com render GPU; tamanho de APK e de dados; escolha do app pendente | ⬜ app a escolher |
| 9 | Photo manager | a escolher (candidato OSS: galeria do F-Droid, a confirmar) | MediaStore e ContentProvider; decodificação de imagem; file I/O | ⬜ app a escolher |
| 10 | Password manager | a escolher (candidato OSS: KeePassDX, F-Droid, a confirmar) | Autofill framework; Android Keystore; criptografia; SQLite | ⬜ app a escolher |
| 11 | Zombie Tsunami | Play Store (proprietário) | jogo com render GPU; serviços Google | ⬜ não iniciado |
| 12 | Play Store | Google (proprietário) | Google Play Services e login de conta; Play Integrity. Há shims parciais de GMS (`rd-vm`, `gms`), mas não cobrem o app | ⬜ decisão do dono sobre GMS |
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

1. O dono confirma: aquisição (APK fornecido ou só OSS), app concreto para os itens
   "a escolher", e decisão sobre Lucky Patcher e GMS.
2. Para cada app com APK acessível, criar o golden flow (suíte `rd test`) e registrar
   o status com evidência.
3. Atualizar este documento e `docs/ROADMAP.md` a cada fase concluída.
