# Worklog — RustyDroid

---
Task ID: 2
Agent: Explore
Task: Mapear APIs AXML/ARSC do rd-apk e superfícies do rd-vm para o LayoutInflater

Work Log:
- Li o repositório (worklog.md não existia — este é o primeiro registro) e mapeei crates/rd-apk por completo: axml.rs, arsc.rs, pool.rs, zip.rs, manifest.rs, lib.rs, error.rs, examples/ (dump_arsc.rs, dump_manifest_attrs.rs) e tests/integration_apk.rs.
- Mapeei crates/rd-vm: engine.rs (Engine/HostState não guardam APK), framework.rs inteiro (HostObj, dispatch call_host_instance/call_host_static, setContentView, findViewById, layout, attach_host_state, launch_app), interp.rs (pontos de despacho host: linhas ~1319 e ~1392; new-instance → attach_host_state linha ~363), value.rs, heap.rs (ObjRef/as_str/alloc_string/alloc_instance), err.rs, intrinsics.rs (alloc_string), classpath.rs (is_subtype/resolve_method).
- Verifiquei rd-cli/src/main.rs: cmd_app_run abre o APK fora da Engine (load_dex_files + launcher_of_apk) e só passa Vec<Dex> + package para launch_app — o APK/ARSC nunca chega ao Engine hoje.
- Chequei dependências: rd-vm/Cargo.toml tem rd-apk apenas como dev-dependency (dependência de produção rd-apk → rd-vm NÃO existe; workspace define "dependências só apontam para baixo": rd-apk → rd-dex → rd-vm → rd-framework).
- Inspecionei golden-apks/org.fdroid.fdroid.apk: tem resources.arsc (2 MB) mas ZERO entradas res/layout (nomes de recurso ofuscados) — testes do LayoutInflater precisarão de APK sintético com res/layout/*.xml + resources.arsc, ou AXML handcrafted (padrão dos testes unitários de axml.rs/arsc.rs).
- Li docs/ROADMAP.md (M3 round 2: LayoutInflater confirmado como próximo passo) e tests/app.rs (padrão de teste M3 com DexBuilder sintético).

Stage Summary:
- AXML: `rd_apk::axml::parse(&[u8]) -> RdResult<AxmlDocument>` (axml.rs:259). Documento = `AxmlDocument { root: XmlElement, resource_ids: Vec<u32>, strings: StringPool, namespaces: Vec<(String,String)> }`. `XmlElement { name, attrs, children, text }` com helpers `android_attr()/attr()/children_named()`. `XmlAttribute { ns: Option<String>, name, raw: Option<String>, value: AttrValue, res_id: Option<u32> }` — res_id vem do resource map indexado pelo índice do NOME do atributo no string pool (axml.rs:422).
- Enum de valores de atributo (exato, axml.rs:115): `AttrValue { Null, String(String), Reference(u32), Attribute(u32), Float(f32), Dimension(f32, ComplexUnit), Fraction(f32, ComplexUnit), Int(i32), Bool(bool), Raw { dtype: u8, data: u32 } }` + `ComplexUnit { Px, Dip, Sp, Pt, In, Mm, Percent, PercentParent, Unknown(u8) }`; conversores `as_bool/as_i32/as_u32/as_string/as_text`.
- ARSC: `Arsc::parse(&[u8]) -> RdResult<Arsc>` (arsc.rs:101); `Arsc { global_strings, packages: Vec<Package> }`; `Package { id, name, type_strings, key_strings, entries: BTreeMap<u32 /*resid*/, Vec<ResEntry>> }`; `ResEntry { config: ResConfigInfo, data_type: u8, data: u32, string: Option<String>, complex: bool, key_idx }`. Lookups existentes: `Arsc::resolve_string(res_id) -> Option<String>` (só STRING e cadeia REFERENCE, config "melhor" automática via specificity/is_default — SEM parâmetro de config), `Arsc::resolve_name(res_id) -> Option<String>` → "@type/key", `Arsc::package_for(res_id)`. NÃO EXISTE: resolve por nome (`R.string.foo` → id), resolve tipado (int/bool/dimension — data_type/data ficam crus em ResEntry; complex/bags não são desdobrados), índice nome→id. LayoutInflater precisará criar esses helpers (ou caminhar `entries` + `resolve_name`).
- Acesso a entradas: `Apk { zip: Zip, arsc: Option<Arsc>, ... }` (rd-apk/src/lib.rs:34-36); `Apk::container() -> &Zip`, `Apk::manifest_raw()`; `Zip::read(name)/read_entry/find/names/names_with_prefix("res/layout/")` — caminho pronto: zip.read(entry) → axml::parse.
- rd-vm: Engine (engine.rs:46) NÃO tem campo de APK/ARSC; HostState (framework.rs:104) só tem objects/clock_ms/main_looper/current_activity/package_name. `setContentView(I)V` hoje = NOT_IMPLEMENTED tipado (framework.rs:678-685); `setContentView(View)` (669) usa helper `set_content_view` (1092) → ensure_window + Window.content + `vm.layout(content, 0, 0, WINDOW_W)` (WINDOW_W=720/WINDOW_H=1440/ROW_H=48, linhas 46-49). `setText(int resId)` também é stub (857-865).
- Views host: `HostObj::View { id, x, y, w, h, visible, enabled, click_listener, parent, text, children, orientation }` (framework.rs:67-80); criação = `vm.heap.alloc_instance(desc, vec![])` + `vm.attach_host_state(desc, r)` (framework.rs:147, chamado do new-instance interp.rs:363); tipo real da view vem do DEX do app via is_subtype (constantes ACTIVITY/VIEW/TEXTVIEW/BUTTON/LINEARLAYOUT/FRAMELAYOUT em framework.rs:30-43). `findViewById` → `find_view_by_id` recursivo (framework.rs:558; dispatch 686; id é i32 via `args[0].as_int()`).
- Dispatch host: interp.rs chama `framework::call_host_instance(vm, mref_class, name, sig, recv, args) -> Result<Option<Value>, VmExit>` quando `mref.class.starts_with("Landroid/")` (interp.rs:1392-1406; static em 1319-1335 → framework.rs:603). Args chegam como `&[Value]` (Int/Long/Float/Double/Obj/Null); strings lidas com `vm.heap.as_str(r)` e devolvidas com `intrinsics::alloc_string(vm, s)`; None = método host não tratado.
- Manifest resolve @string via ARSC assim (manifest.rs:554-573): `arsc.and_then(|a| a.resolve_string(res).or_else(|| a.resolve_name(res)))` — padrão reutilizável para refs de atributo em layouts.
- Gaps para o M3 round 2 (decisões, não código): (1) rd-vm precisa acessar AXML/ARSC → ou dependência rd-apk em rd-vm + campo novo em HostState/Engine (ex.: arsc + leitor de layouts), ou injeção na API (Engine::new/launch_app/método set_resources); (2) resolver layout resId → entrada zip "res/layout/<name>.xml" requer índice nome→id no ARSC (não existe); (3) attributes android:layout_width/layout_height como Dimension precisam mapear ComplexUnit para px no modelo headless (layout atual usa px inteiros); (4) fixture de teste: APK sintético com res/layout + resources.arsc (golden APK F-Droid não tem layouts).

---
Task ID: 3
Agent: Z.ai Code (principal)
Task: M3 round 2 — LayoutInflater de layout XML real (AXML via rd-apk + Resources/ARSC) + issues/CI check prévio

Work Log:
- Pré-check (instrução permanente "Issues antes de M3"): API GitHub autenticada → 0 issues abertas (as 38 já fechadas); CI verde no HEAD fb37e28 (run 38026991552). Liberado para M3 round 2.
- Subagente Explore (Task ID 2) mapeou AXML/ARSC/zip do rd-apk e superfícies do rd-vm (relatório na seção anterior).
- rd-vm/Cargo.toml: rd-apk promovido a dependência de produção (rd-apk é a BASE do workspace; rd-vm→rd-apk segue apontando para baixo, sem ciclo).
- classpath.rs: builtin_hierarchy estendida com a hierarquia Android (Activity/Context/View/ViewGroup/TextView/Button/LinearLayout/FrameLayout/Intent/Bundle/Handler/Looper/Window) — APKs REAIS não definem android.* no DEX; is_subtype agora resolve sem DEX (habilita inflação em APK real).
- framework.rs: HostState.resources (Option<rd_apk::Apk>); Engine::set_resources + Engine::resources() (erro RESOURCES_MISSING tipado) + Engine::resolve_string (arsc, fallback nome qualificado); HostObj::View ganhou click_method (android:onClick); dispatch_touch refatorado em dispatch_click (listener OU click_method na ACTIVITY corrente); performClick reusa dispatch_click; dump_view mostra @id/nome via arsc.resolve_name; set_content_view/add_child/ensure_window viraram pub(crate); DENSITY 2.0 (720px = viewport 360dp).
- inflate.rs (NOVO): LayoutInflater headless — resid → @layout/key (Arsc::resolve_name) → res/layout/key.xml (ZIP, fallback configs) → axml::parse → árvore host recursiva; attrs id/text/orientation/visibility/enabled/onClick/layout_height; tags sem host falham INFLATE tipado; attrs desconhecidos ignorados com RD_FW_DEBUG.
- interp dispatch: setContentView(I)V substituiu o stub NOT_IMPLEMENTED; setText(I) resolve no arsc; novo braço getString(I).
- BUG LATENTE corrigido: braço findViewById exigia Value::Obj em args[0] (o resid é Int!) e devolvia Null SEMPRE — sem cobertura até agora; corrigido para ler o resid Int + guard de formato.
- rd-cli: cmd_app_run abre o APK UMA vez (dexes_of_apk + launcher_of(&Apk) + eng.set_resources(apk)) — antes relia o zip 3×; package agora vem do manifest (getPackageName correto com --activity explícito).
- tests/common/apkfix.rs (NOVO): fixtures byte a byte — CRC-32, ZIP STORED (local+central+EOCD), string pool UTF-16, builder AXML (elements + attrs tipados: String/Ref/Int/Bool/Dim), builder ARSC (pacote 0x7f + N tipos + config default), build_manifest_axml e build_apk.
- tests/app_xml.rs (NOVO): 6 testes de integração M3.2 — DoD (XML infla + toque via android:onClick muda texto), RESOURCES_MISSING sem resources, findViewById+setText(I)+getString(I) via arsc, tag não suportada → INFLATE tipado, visibility/enabled no hit-test, shape da árvore host.
- Smoke end-to-end manual (temporário): fixture APK com classes.dex gerado em /tmp, `rd app run` real executou lifecycle → setContentView(R.layout.main) → inflação → tap(10,170) → onBtn → dump com @id/tv/@id/btn e texto trocado; JSON com package do manifest. Teste temporário removido após validação.
- Docs: ROADMAP.md (M3 = concluído rounds 1+2 com checklist do round 2), compat-matrix.md + gerador xtask sincronizados (round-trip validado, diff vazio).

Stage Summary:
- M3 round 2 COMPLETO: setContentView(I) infla layout XML REAL do APK (AXML+ARSC), app trivial XML roda ponta a ponta com toque via android:onClick — DoD do M3 fechado nos dois rounds.
- 187 testes no workspace (0 falhas; +6 M3.2), clippy limpo, fmt ok, MSRV 1.76 ok, smoke CLI verificado.
- Decisões: rd-vm→rd-apk como dep de produção (sem ciclo); hierarquia Android builtin no classpath (chave p/ APKs reais); DENSITY 2.0 (viewport 360dp); attrs não modelados ignorados (semântica Android) com log debug; classes sem host falham tipadas.
- Pendências M4: render pixel + UI dump rico (get_ui_tree), configs específicas (land/locale/density) selecionáveis, weights/gravidade no layout.

---
Task ID: 4
Agent: Z.ai Code (principal)
Task: Auditoria read-only do DoD de M0/M1/M2 (sem tocar no GitHub)

Work Log:
- HEAD local 6d6440e (M3.2), working tree limpa; GitHub intocado por instrução do usuário.
- cargo test --workspace: 187 testes, 0 falhas (bate com worklog do M3.2).
- M0: rd inspect no APK real org.fdroid.fdroid 2.0.1 — features (10), permissões (31 classificadas em auto/runtime/special/unknown), 36 componentes (10 act + 3 alias + 4 provider + 9 recv + 10 svc), signing v1/v2/v3, label via ARSC; perm audit com 11 findings estruturados; device list mostra moto-e5 ★ como piso; behavior --target 33 mostra 17/25 switches. DoD inspect ✅.
- M1: dex summary dos 3 DEX (24.520 classes, checksum ok, map 18/18); disasm 8/8 classes reais de amostra (3 dexes) sem falha. Paridade baksmali (24.913 classes / 3 APKs / 100% idênticas) foi validação ÚNICA com comparador em /tmp — não repetível hoje (só 1 APK em disco; comparador não commitado). Campanha fuzz 24h NUNCA executada (o próprio ROADMAP.md registra "pendente de runner dedicado"). Sem nightly/cargo-fuzz nesta máquina.
- M2: harness golden re-executado fresco — 150/150 PASS vs JVM real (exit 0). Spot-check de 7 casos em métodos puros do APK REAL (ContainerHelpersKt.idealByteArraySize×3, idealLongArraySize, IntIntPair.getFirst-impl, ScatterMapKt.loadedCapacity×2): 7/7 valores corretos, verificados contra implementação Python INDEPENDENTE derivada do smali do nosso disassembler.

Stage Summary:
- Veredito: M0 ✅ completo (ressalva: CI do HEAD 6d6440e não re-verificado — GitHub intocado; último verde conhecido fb37e28/run 38026991552). M2 ✅ completo (golden 150/150 fresco + 7/7 métodos puros de APK real). M1 ⚠️ QUASE — código 100%, mas o sub-item do DoD "fuzz 24h sem crash" nunca rodou, e a paridade baksmali em 3 APKs não é repetível (comparador differential não commitado; 2 dos 3 APKs ausentes em disco).
- Recomendações: (1) campanha fuzz (24h ideal, ≥4h aceitável) em runner dedicado; (2) commitar comparador differential baksmali + fixar os 3 APKs golden no fetch.sh para paridade repetível; (3) re-verificar CI/push do HEAD atual quando liberado tocar no GitHub.
- Nenhum arquivo de código alterado; registro appendado no worklog local sem commit.

---
Task ID: 5
Agent: Z.ai Code (principal)
Task: Script de campanha fuzz 24h (DoD M1) + tutorial WSL2 para notebook Win 11 do usuário

Work Log:
- Lido docs/FUZZING.md (política: 5 targets em paralelo, crashes arquivados com input minimizado) e os 5 fuzz targets (todos byte-slice puro: zip/axml/arsc/apk_full no rd-apk, dex no rd-dex).
- Criado scripts/fuzz-campaign.sh (executável, sintaxe validada com bash -n): seeds automáticos do APK golden para os 5 corpus (APK inteiro p/ apk_full/zip, manifest+res/*.xml p/ axml, resources.arsc p/ arsc, classes*.dex p/ dex), modo -fork com -ignore_crashes/ooms/timeouts=1 (continua e arquiva achados), max_len ≥ maior seed (16MB apk/zip, 10MB dex, 4MB arsc, 2MB axml — evita truncar EOCD do ZIP), workers auto = cores/5, relatório final que distingue crash-* (P0, Lei 1) de oom/timeout, exit 1 se houver achados.
- fuzz-logs/ adicionado ao .gitignore.
- Tutorial WSL2 entregue no chat (PowerShell wsl --install → apt deps → rustup nightly → cargo-fuzz@0.13.2 --locked → clone → tmux + energia "nunca suspender" + pausar Windows Update 1 semana → rodar smoke 10 min → campanha 24h → checar artifacts).
- Nada commitado/pushed (GitHub segue intocado); script aguarda liberação para commit.

Stage Summary:
- Campanha fuzz do DoD M1 agora é 1 comando: ./scripts/fuzz-campaign.sh (padrão 24h; smoke com ./scripts/fuzz-campaign.sh 600). DoD = nenhum crash-* em crates/*/fuzz/artifacts após o tempo total.

---
Task ID: 6
Agent: Z.ai Code (principal)
Task: Revisão + validação executável do scripts/fuzz-campaign.sh + push (GitHub liberado)

Work Log:
- Revisão linha a linha encontrou 3 bugs reais (nenhum pego por bash -n):
  (1) find -path '*/fuzz/artifacts/crash-*' nunca casaria — cargo-fuzz grava
      em artifacts/<target>/crash-* (SUBDIRETÓRIO) → relatório diria "SEM CRASH"
      com crash real (falso negativo do DoD). Fix: casar por NOME dentro de
      artifacts/ (-type f -name 'crash-*' -path '*/fuzz/artifacts/*'), provado
      com artifact falso (1 encontrado, 0 após remover).
  (2) GNU find `-size -1M` arredonda PRA CIMA em unidades de 1 MiB — só casa
      arquivo VAZIO; os 329 XMLs de res/ do APK golden eram todos filtrados
      (axml ficava com 1 seed). Fix: -size -1048576c → axml foi a 183 seeds.
  (3) `find | head | while` sob pipefail morre com SIGPIPE (exit 141) no APK
      real; `wait` sem args esconde falhas de job; stat:: não existe em fork
      mode (contador real é o último pulso #N:). Todos corrigidos.
- Adições de robustez: checagem amigável de nightly/cargo-fuzz/unzip, export
  RUSTUP_TOOLCHAIN=nightly (não depende de rustup default), env.txt com
  proveniência, guarda de disco (<5 GB livres → aviso), du -sh do corpus no
  relatório, procedimento de resgate `cargo fuzz merge` no cabeçalho, gate do
  DoD = crash-* (oom/timeout são avisos, alinhado a docs/FUZZING.md).
- Validação EXECUTÁVEL nesta máquina (nightly 1.101.0-nightly + cargo-fuzz
  0.13.2 instalados aqui): 3 smokes completos (15s/15s/5s) — pipeline inteiro
  rodou, seeds corretos, corpus cresceu (axml 1→237, dex 126 MB), 0 crash/oom/
  timeout, exit 0 nas três rodadas. Corpus de smoke ficou como semente extra
  para a campanha real (gitignored).
- Descoberta de operação: corpus dex ~108 MB nos primeiros 30s — documentado
  monitoramento du -sh + resgate via cargo fuzz merge.
- Push: commits feat(fuzz) + docs(worklog) → main.

Stage Summary:
- scripts/fuzz-campaign.sh validado 3× ponta a ponta e pronto para a campanha
  24h do usuário (notebook Win 11 via WSL2). DoD = zero crash-* após 24h.
- Bugs de falso-negativo no gate de crash eram o risco real de desperdiçar as
  24h — eliminados e provados por teste.

---
Task ID: 6
Agent: Z.ai Code (sessão fuzz-campaign)
Task: Verificação completa do scripts/fuzz-campaign.sh + calibração para notebook do usuário (i5-1334U, 16 GB RAM, WSL2) + commit/push

Work Log:
- Auditoria linha a linha do scripts/fuzz-campaign.sh: 3 bugs reais encontrados e corrigidos:
  (1) veredito dava "✅ SEM CRASH — DoD satisfeito" mesmo se todos os 5 targets morressem no build (exit 0 falso) — agora exige 5/5 íntegros; falha de execução = exit 2 com o log;
  (2) artifacts de rodadas anteriores (ex.: smoke de 10 min) eram contados de novo no veredito da campanha nova — agora são arquivados em fuzz-logs/<ts>/artifacts-anteriores/ antes da contagem;
  (3) dica de merge no cabeçalho tinha a sintaxe invertida (saída/entrada do cargo fuzz merge).
- Novos preflights anti-desperdício de 24h:
  - check_asan_kernel: aborta se vm.mmap_rnd_bits > 28 (bug ASan/SEGV em kernels ≥ 6.6, incluindo WSL2 recente) com instruções de correção ([boot] command no /etc/wsl.conf); escape RD_FUZZ_SKIP_SYSCTL_CHECK=1;
  - auto-calibração de memória: lê MemAvailable, escolhe workers × rss_limit (notebook 16 GB → workers=1, rss ~1,7-1,8 GB/processo; pior caso ~10,5 GB cabe em WSL com 12 GB); RD_FUZZ_RSS=<MB> sobrescreve;
  - validação numérica de SECS/WORKERS; guard de "rodar de dentro do clone"; erro claro no unzip de APK corrompido; --help e --seeds-only.
- Testes executados nesta máquina: bash -n OK; --help OK; --seeds-only real com APK golden (corpus: apk_full=24, zip=5, axml=245, arsc=15, dex=15 — ~244 MB); guard de argumento inválido exit 1; guard fora-do-clone exit 1. (Campanha em si não executável aqui: sem nightly/cargo-fuzz no host.)
- Verificado via API (PAT do usuário, em memória): remote main = 9862711, sincronizado com local antes deste commit.
- Commit c0e9932 + push para main; CI a acompanhar.

Stage Summary:
- scripts/fuzz-campaign.sh v2: preflight completo (ASan kernel + RAM + guardas), veredito honesto (exit 0 só com 5/5 targets e zero crash-*), --seeds-only para smoke de dados em 2 min.
- Recomendação WSL2 para o usuário: .wslconfig com memory=12GB + swap=16GB; fluxo --seeds-only → 600s → 86400s dentro do tmux.
- Pendências de M1 continuam: campanha 24h real no notebook do usuário; comparador baksmali repetível ainda a commitar (fora do escopo desta task).

---
Task ID: 7
Agent: Z.ai Code (sessão issues→M4)
Task: Resolver TODOS os 14 issues abertos (#39–#52) antes de começar M4; CI verde; issues fechadas

Work Log:
- Retomada de sessão anterior: commit local 6907c81 (fix #39 #43 #48) + diff não-commitado (fix #41/#49) — validado, corrigido (3 bugs nos testes novos: tipos não registrados no TryBlob, opcode 0x33 vs 0x32, const/4 fora do range, layout de try sem op35c=3 units) e commitado (820452b).
- Move-exception: materialização do próprio OOME num heap esgotado não substitui mais a exceção original (move Null e o catch executa) — bug de design achado pelo teste novo do #49.
- Issues corrigidos um a um (commit por issue):
  #44 rd-dex fill-array-data padded em u64 (wrap u32 → payload inconsistente);
  #52 docs/dados (health-connect 36→34, photo-picker 34→33, README 175/M3/exit codes, RustyDroid.md §3.1);
  #51 RD_DATA_DIR como explícito, granted_by+Internal, inspect --json com granted_by, target_sdk() morto removido, IO_ERROR module rd-cli;
  #50 Looper drena menor deadline (min-scan estável) + postDelayed clamp u64;
  #46 Vis 3 estados (GONE sai do layout, hit-test rejeita invisible/gone, dump anota) + @dimen/@bool com log + layoutfoo;
  #42 is_subtype arrays covariantes (JLS 4.10.2/3) + aput store check correto + resolve_method fase classes→interfaces (JVMS 5.4.5);
  #40 cap 512 no inflate + drops iterativos (XmlElement/AxNode) + caps em hit_test/layout/dump;
  #45 attach_child (inflação O(N), re-layout só no setContentView) + dimension_px tipada (viewport 720px) + cap 10k views + saturating;
  #47 fixture fiel ao aapt: namespaces, resource map 0x0180, typeSpec 0x0202, 2 configs por res_id (entry index por NAME único — bug achado e corrigido), DOS time, arsc pad4.
- CI: 2 falhas pós-push (fmt --check; clippy --all-targets doc_lazy_continuation) — corrigidas (ca549a8, 0e83921).
- Issues que o GitHub não fechou por subject com múltiplos #N (#43 #48 #49) fechadas via API com comentário de evidência.
- Estado final: CI VERDE (0e83921), 0 issues abertas, suíte local ~200 testes 0 falhas, clippy --all-targets limpo.

Stage Summary:
- Todos os 14 issues (#39–#52) RESOLVIDOS e FECHADOS; commits 6907c81→0e83921 pushados; CI verde.
- Próximo: M4 (software rendering + UI dump uiautomator-compatible) — a começar.

---
Task ID: 8
Agent: Z.ai Code (sessão issues→M4)
Task: Começar e fechar o M4 (screenshot correto; get_ui_tree com ids certos)

Work Log:
- rd-vm: Engine::view_tree() — árvore de UI estruturada (UiNode com class,
  resource-id no formato package:id/nome resolvido do arsc, text, bounds,
  visibility/enabled/clickable, children) com cap de profundidade (#40).
- rd-render (de stub M0 a real): ui_dump.rs (XML uiautomator fiel: index/
  text/resource-id/class/package/bounds [x1,y1][x2,y2]/clickable/enabled;
  GONE excluído como no uiautomator real, INVISIBLE mantido) + shot.rs
  (framebuffer RGB 720×1440 determinístico, painter por classe/estado,
  glifos de bloco 3×7 com nota de que fonte real é M8/M9, encoder PNG sem
  dependências: zlib STORED + crc32/adler32 próprios).
- rd-cli: subcomandos rd app dump (uiautomator XML | --text | --json) e
  rd app shot --out FILE.png; ações de script xml/shot no rd app run;
  boot_app extraído (boot comum: APK 1×, launcher default, package do
  manifest).
- Testes: 8 unit (rd-render: dump/gone/invisible/escape/PNG
  determinístico/pixels/zlib) + 2 e2e (view_tree com ids certos; screenshot
  da árvore REAL com pixels por região) — rd-render como dev-dependency do
  rd-vm (ciclo dev permitido).
- MSRV: is_multiple_of (1.87) substituído por % (MSRV 1.76 é regra).
- Docs: ROADMAP M4 ✅ (round 2: seleção de configs por device-config) +
  README M4 DONE.
- Suíte: 23 bins ok, 0 falhas; clippy --all-targets 0 warnings; fmt ok.

Stage Summary:
- M4 DoD ATINGIDO e commitado (0c3b29f, 936681f, 8d7bf14): get_ui_tree com
  ids certos + screenshot determinístico, ponta a ponta com fixture real.
- Pendências conhecidas (registradas, não bloqueiam): fonte tipográfica real
  (M8/M9); seleção de configs por device-config no resolve (M4 round 2);
  renderers winit/web (M11).

---
Task ID: 9
Agent: Z.ai Code (sessão Issues→M5)
Task: Resolver as 14 issues da auditoria rodada 5 (#53–#66) e então começar M5

Work Log:
- 14 issues lidas via API (#53–#66) — audit pós-M4: dump uiautomator,
  layout, VM, CLI, fuzz-campaign, docs e suíte.
- #53 (high): ui_dump escapava só text/class — package/resource-id cru
  injetava nós falsos e chars < 0x20 quebravam XML. Agora todos os campos
  escapam; \t\n\r → &#x9;/&#xA;/&#xD;; proibidos → U+FFFD.
- #54: index por-irmãos (antes DFS global), focusable próprio (não fudge
  de clickable), resource-id honesto (nunca "id/123" fabricado; pid 0x01 →
  "android:", package_for SEM fallback first — arsc hostil não sequestra
  namespace); NCK documentados como compat-gap.
- #57: sget/sput resolvem a classe DECLARANTE (JVMS 5.4.3.2) — antes sget
  herdado via subclasse lia default e sput criava slot divergente;
  NoSuchFieldError capturável (hierarquia builtin).
- #58/#65 fuzz-campaign.sh reescrito no veredito: por ARTIFACTS (crash/
  leak/oom/timeout), crash/leak primeiro, falha genuína = sem pulsos;
  trap INT/TERM, SECS=0 rejeitado, guard na contagem, assert APK≤max_len,
  nota wall-clock. 8 cenários com stubs + ShellCheck 0.10.0 limpo.
- #59: teto de dimensão POR EIXO (720×1440) — 400dp de altura não é mais
  INFLATE falso; layout_width explícito respeitado (w_explicit).
- #60: divisor horizontal ignora GONE ([GONE, Button] → 720; [A,GONE,B] →
  360+360).
- #61: @dimen/@bool resolvem do arsc (resolve_value tipado, cadeias de
  Reference, INFLATE tipado se não resolver); seleção de config pontua
  qualificadores (retrato/dia); layoutfoo nunca é fallback.
- #62: saturating_add no dump e no renderer (i32::MAX não panic na API
  pública), marcador <!-- truncated -->, budget 64 MB com erro tipado.
- #55: rd_vm::HostOut (Std/Stderr/Buffer) — CLI roteia System.out do app
  para stderr (stdout = artefato); println testado por conteúdo.
- #56: path duplicado do rd app removido (exit 64 na forma documentada);
  e2e do binário exercita a linha do ROADMAP.
- #63: module_id rd-cli nos erros nascidos no CLI (sem window, scripts,
  writes de --out).
- #64: README ':76' com M4 landed; deferrals "M4" re-apontados no código.
- #66: println asserido (antes falso-verde), 3 comentários corrigidos
  (clinit inexistente → clinit real; DOS time 19:56:00; 600 níveis),
  3 probes promovidos a testes (clinit A→B, EIIE engine fresco, res_id
  do resource map) + layoutfoo negativo.
- Infra: crate rd-testsupport (fixtures DexBuilder/apkfix compartilhados
  rd-vm ⇄ rd-cli); ArscFix ganhou add_dimen_dp/add_bool.

Stage Summary:
- 14/14 issues resolvidas; suíte 220 → 241 testes, 0 falhas;
  clippy -D warnings 0; fmt ok; ShellCheck limpo.
- Commits: 057a007 (testsupport) · 6847e40 (dump/layout/refs) ·
  ce19616 (#57) · 5186295 (CLI) · 32ebb90 (fuzz script) · docs/worklog.
- Próximo: M5 — Agente MCP v1 (~20 tools): LLM instala, navega e assera
  estado só com as tools (DoD no ROADMAP).

---
Task ID: 10
Agent: Z.ai Code (sessão Issues→M5)
Task: M5 — Agente MCP v1 (~20 tools): LLM instala, navega e assera estado só com as tools

Work Log:
- DoD lido no ROADMAP (M5) + spec PARTE 4 (install/launch/input/ui_tree/
  screenshot/logs/assert/permissions/notifications).
- ADR 001 (docs/adr/): rmcp atual exige MSRV 1.88 > workspace 1.76 (contrato
  do CI) → envelope JSON-RPC 2.0 stdio próprio (protocolo idêntico na fio),
  zero deps novas; tokio continua fora.
- rd-agent::mcp: servidor MCP completo — initialize (2024-11-05) /
  notifications/initialized (sem resposta) / tools/list / tools/call / ping;
  desconhecidos → -32601/-32700; erros de tool no resultado com isError
  (contrato {code, cause, suggestion, module_id: rd-agent}).
- 20 tools: install (piso API 26 tipado), launch (LAUNCHER ou explícita),
  tap, wait, ui_tree (xml/text), screenshot (arquivo OU image base64),
  logs (buffer da SESSÃO — stdout do protocolo intocável, contrato #55),
  invoke (sonda de estado geral), assert_view/assert_text/assert_activity/
  assert_finished, app_info, permissions_list/grant/revoke/audit (motor
  fd-permissions da sessão), device_profile (moto-e5), behavior_switches
  (targetSdk), notifications (NOT_IMPLEMENTED honesto → M9).
- Sessão: 1 app por processo (ADR 001); HostOut::Buffer da sessão para o
  System.out do app; install troca de app mata a execução anterior.
- rd-cli: subcomando `rd mcp` (stdio do processo é o canal do protocolo).
- Fixtures: apkfix ganhou build_apk_sdk/build_manifest_axml_sdk (min/target
  explícitos — fixtures do agente acima do piso 26).
- Testes: +6 unit (golden flow install→launch→tap→wait→asserts→invoke→
  ui_tree→screenshot→logs; tools sem app tipadas; notifications honesta;
  envelope MCP; serve_stdio loop real; tools/list = 20) + 1 e2e do binário
  (fluxo completo na fio). HostOut global serializado nos testes (1 sessão
  por processo — mesma restrição do ADR).
- Docs: ROADMAP M5 ✅ + seção "M5 concluído"; README M5 DONE; compat-matrix
  atualizada (render M4/agente M5).

Stage Summary:
- M5 DoD ATINGIDO: um LLM conecta no `rd mcp` e instala, navega e assera
  estado sem sair das tools (e2e com o binário real prova na fio).
- Suíte: 241 → 247 testes, 0 falhas; clippy -D warnings 0; fmt ok.
- Commits: mcp server + CLI + testes + ADR + docs.
- Próximo (M6): executor de testes — suíte JSON → JUnit XML; compat-matrix
  automatizada (run_scenario + coverage AG-09).

---
Task ID: 10
Agent: Z.ai Code (sessão M5 completa)
Task: "Adicione oq falta dessa lista ao MCP M5 — Agente MCP v1" — fechar as lacunas do enunciado da milestone no servidor MCP existente (20 tools → catálogo completo + controles + DoD restante).

Work Log:
- Issue check prévio: 0 issues abertas na API (as 14 pós-M4 já fechadas) — pré-requisito satisfeito.
- Gap analysis do mcp.rs v1 (20 tools): faltavam pacote list/rem/clear, swipe/type/key, wait_for, runtime_health, compat_gaps, batch; controles de sandbox/APK/URL/tempo; sessionId/package nos resultados; --describe-tools + docs geradas.
- rd-vm (framework.rs): primitivas do agente — swipe_app (8 amostras, dedupe de views atravessadas, down+up na MESMA view = clique via dispatch_click; clock avança ms), type_text (primeira EditText visível+habilitada; replace flag; find_edit_text DFS), key_event (BACK=finish do host, DEL apaga último char da EditText, ENTER entregue sem efeito, resto NOT_IMPLEMENTED com lista), looper_pending (fila do Looper p/ health). Structs SwipeReport/CrossedView/KeyReport + SUPPORTED_KEYS.
- rd-agent (mcp.rs): Policy (RD_MCP_SANDBOX/RD_MCP_MAX_APK_MB/RD_MCP_MAX_WAIT_MS/RD_MCP_MAX_BATCH; check_read_path/check_write_path — URL recusada URL_NOT_ALLOWED, `..` e fora-das-raízes PATH_OUTSIDE_SANDBOX, APK_TOO_LARGE antes de ler); AgentSession reestruturada (session_id sess-<pid>-<nanos>, registro multi-app installed: Vec<InstalledApp>, active, engine_package, kill_engine_if_running p/ consistência); install transacional (valida tudo antes de mutar; reinstalar mesmo package mata o engine, outro package não); launch com package explícito (multi-app); 10 tools novas (package_list/package_remove/package_clear — remove mata app em execução, clear reseta PermissionEngine; swipe/type_text/key/wait_for — poll com clock virtual, found=false honesto; runtime_health sem exigir app; compat_gaps — catálogo de 9 lacunas; batch — ordem, stop_on_error, resume_from idempotente, sem aninhamento, cap de passos); wait/swipe/wait_for com LIMIT_EXCEEDED; screenshot com check_write_path; envelope tools/call carrega sessionId + package; tools_list_json público + tools_markdown (docs geradas).
- rd-cli: `rd mcp --describe-tools` (catálogo JSON = tools/list) e `rd mcp --gen-docs FILE` (escreve docs geradas; exit 0/1 com contrato no stderr).
- rd-testsupport: build_apk_sdk_perms/build_manifest_axml_sdk_perms (uses-permission no fixture — ciclo grant/clear real).
- docs/mcp-tools.md GERADA e commitada (teste generated_docs_match_catalog falha se divergir — regen com rd mcp --gen-docs).
- Testes: +8 unit no rd-agent (catálogo==dispatch+schemas required⊆props; sandbox/URL/APK/caps; batch resume/cap/aninhamento; registro multi-app lifecycle com clear resetando CAMERA; session ids + isolamento; wait_for polls/caps; swipe/type/key no fixture com formulário FQCN EditText + android:onClick; docs consistency) — rd-agent 14/14; e2e rd-cli estendido (30 tools, sessionId/package no envelope, swipe/wait_for/package_list/runtime_health/compat_gaps/batch na fio, --describe-tools == tools/list) — 2/2; rd-vm 11 ok; rd-testsupport ok.
- Suíte total: 27 suítes ok, 0 falhas; clippy --workspace --all-targets 0 warnings; fmt ok.
- Docs: README M5 atualizado (30 tools + controles + describe-tools); ROADMAP M5 "30 tools + controles".

Stage Summary:
- Enunciado M5 100% coberto: pacote (listar/instalar/remover/limpar/launch) ✓, input (tap/swipe/type/key) ✓, observação ✓, espera/assert (wait/wait_for/4 asserts) ✓, permissões ✓, estado/saúde (app_info/device_profile/runtime_health) ✓, lacunas (compat_gaps+behavior_switches) ✓, descrição automática (--describe-tools + tools/list do mesmo registro) ✓, batch idempotente (resume_from) ✓, erros {code,cause,suggestion,module_id} ✓, sessionId/package em TODO resultado ✓, controles (sandbox/APK excessivo/URLs/tempo ilimitado/estado inconsistente) ✓.
- DoD M5: cliente em processo separado (e2e McpProc com binário real) instala/inicia/interage/observa/asserta sem comandos manuais ✓; testes de schema/JSON/timeout/erro/batch/isolamento ✓; conjunto publicado == --describe-tools == docs geradas (teste trava a convergência) ✓.
- Catálogo: 20 → 30 tools. Novos erros tipados: PATH_OUTSIDE_SANDBOX, URL_NOT_ALLOWED, APK_TOO_LARGE, LIMIT_EXCEEDED, PACKAGE_NOT_INSTALLED.
- Próximo (M6): executor de testes — suíte JSON → JUnit XML; compat-matrix automatizada.

---
Task ID: M6
Agent: Agente de IA (sessão M6, rodada 1 — Never Hallucination)
Task: "Trabalhe por 3 horas; objetivo M6 até M10" — escopo escolhido pelo usuário: M6 completo; publicação via branch + PR.

Work Log:
- Leitura do roadmap: M6 = executor de testes (suíte JSON → JUnit XML; compat-matrix automatizada). Spec: TST-01, AG-07, AG-09.
- Baseline antes de qualquer mudança: `cargo test --workspace` → 288 passam, 0 falham. Toolchain stable 1.99.0; MSRV 1.76 validado com `cargo +1.76 check --workspace`.
- rd-agent::suite (novo): parse/validação (INVALID_SUITE antes de executar), run_scenario (sessão nova por cenário, parada na primeira falha, `expect` ok|error:CODIGO), run_suite, JUnit XML (escape XML, system-out com passos), resumo humano.
- Achado do teste e2e: asserções do MCP respondem isError=false com "ok": false/"found": false. Sem tratamento, asserção errada passaria em verde. Tratado em soft_failure() + teste unitário + e2e (exit 1).
- rd-cli: `rd test <suite.json> [--junit FILE] [--json]`; exit 0/1/64. Despachado antes do match (contrato de exit próprio).
- xtask compat-report: fases lidas de docs/ROADMAP.md (a lista fixa estava defasada: M4/M5 aparecia ⬜); guarda de drift (crate sem linha na tabela → erro); seção de suítes; CRATE_STATUS corrigido (rd-render M4 implementado, rd-agent M5–M6, rd-testsupport adicionado).
- CI: job Rust executa `rd test tests/suites/contract-no-app.json --junit ...` (sem APK).

Stage Summary:
- Testes: 308 passam, 0 falham (baseline 288 + 12 suite + 4 e2e rd test + 4 xtask). cargo fmt --check ok; clippy --workspace --all-targets -D warnings ok; MSRV 1.76 check ok; check_md_links ok.
- DoD M6 (ROADMAP): suíte JSON → JUnit XML ✓; compat-matrix automatizada ✓ (fases do roadmap, suítes, guarda de crates).
- Não entra no M6: coverage/get_api_usage (AG-09), run_scenario com retries/hooks (AG-07), timeout por passo.
- M7–M10: não iniciados nesta rodada (ver ROADMAP). Cada um é um milestone de escopo próprio (M7 gate de performance no piso moto-e5; M8 compat real; M9 NDK/ELF; M10 dispositivos virtuais).
- Próximo: abrir PR; próxima rodada sugerida: M7 (performance + gate E5), que depende de medições reais.

---
Task ID: fix-109
Agent: Arena Agent Mode (sessão autônoma — bugfix)
Task: Corrigir issue #109 — activity-alias declarada no manifest não era lançável pelo rd mcp (nem por `activity` explícito, nem como LAUNCHER).

Work Log:
- Leitura: worklog, issues abertas (#109 e #110), código de tool_launch (rd-agent), launch_app (rd-vm), parser de manifest (rd-apk). Baseline: cargo test --workspace verde antes das mudanças.
- Escolha: #109 antes de #110 — a #110 pede uma decisão de regra que a própria issue diz que não toma (spec silente para signature fora da lista B5); a #109 tinha correção recomendada e critérios de aceite fechados.
- rd-testsupport (apkfix.rs): builder novo build_manifest_axml_sdk_alias (activity sem filtro + alias com MAIN/LAUNCHER e targetActivity parametrizado).
- rd-agent (mcp.rs): tool_launch resolve alias → targetActivity ANTES da VM (helper launch_desc_of_component); busca LAUNCHER passa a incluir ActivityAlias; alias sem targetActivity → INVALID_FORMAT; alvo ausente do DEX → MISSING_ENTRY nomeando alias E alvo (enriquecendo o erro da VM); resultado ganha campo `alias` quando o launch foi via alias; descrição da tool atualizada e docs/mcp-tools.md regenerada (teste generated_docs_match_catalog trava a convergência).
- Testes: fixture_apk_path refatorado em fixture_parts + fixture_apk_write (mesmos bytes, ordem de entradas idêntica) + 2 fixtures novos (alias→Main, alias→Ghost); +2 testes cobrindo os 4 critérios de aceite da #109 (launch explícito do alias roda a Main com ui_tree provando execução real; launch {} com LAUNCHER no alias; activity comum segue ok sem campo alias; alvo fantasma → erro estruturado pelos dois caminhos, sem estado de sessão).
- Regressões: testes das issues #82/#83 continuam passando (mensagens de erro só ganharam /<activity-alias>).

Stage Summary:
- Suíte: workspace todo verde (rd-agent 40 → 42 testes; 0 falhas). cargo fmt --check ok; clippy --workspace --all-targets sem warnings.
- GitHub: branch fix/109-activity-alias-launch, PR com Closes #109.
- Limitações conhecidas: none novo — warm do rd perf segue recriando o Engine (M7), não tocado aqui.
- Próximo: issue #110 (BATTERY_STATS auto-granted) — depende de decidir/documentar a regra para permissões signature fora da lista B5 na spec (RustyDroid.md); sugerido decidir com o dono do repo antes de implementar. Depois: pendências do M7 (gate golden, medição em device real).

---
Task ID: fix-110
Agent: Arena Agent Mode (sessão autônoma — bugfix, decisão autorizada pelo dono do repo)
Task: Corrigir issue #110 — BATTERY_STATS seguia auto-granted apesar da #68: signature-class fora da whitelist B5 não era tratada.

Work Log:
- Leitura: issue #110 (critérios de aceite da #68 não atendidos), spec RustyDroid.md §B5, fd_permissions.rs (initial_state/granted_by), permissions.toml, inspect/perm-list no CLI, tools permissions_* do MCP.
- DECISÃO (registrada na spec): a lista B5 é uma WHITELIST — só ela é auto-concedida ao app em análise (marcador runtime-is-system); signature-class fora da whitelist recebe o estado novo DeniedSystem (serde denied_system, MCP "denied-system"): nem concedida nem concedível na sessão (grant/deny preservam; revoke restaura). Motivo: o Android real nega essas permissões a apps de terceiros (fidelidade > conveniência "runtime é system").
- data/permissions.toml: flag `b5` nas 38 entradas da whitelist (35 signature + 3 internal: DEVICE_POWER/REBOOT/FACTORY_TEST); BATTERY_STATS segue sem a flag; comentário da seção B5 registra a decisão.
- rd-framework: PermissionDef::is_b5(); GrantState::DeniedSystem; initial_state decide pela flag; grant()/deny() preservam (wildcard), revoke restaura via initial_state; is_granted() exclui.
- rd-cli: resumo do inspect ganha categoria `denied-system` (soma das categorias continua fechando com o total declarado — regra #76/#69); perm list imprime o estado via Debug/JSON serde.
- rd-agent: grant_state_str → "denied-system"; permissions_list/permissions_grant refletem o estado (critério 3 da #110).
- Spec: RustyDroid.md §B5 documenta a regra e o caso BATTERY_STATS.
- Testes: battery_stats_is_denied_system_outside_b5_whitelist (estado, sem marker, grant/deny/revoke), signature_entries_are_partitioned_by_b5_whitelist (38 b5 / 1 fora), permissions_denied_system_for_signature_outside_b5 (MCP lista+grant).
- README: bloco do `rd inspect` regenerado contra o F-Droid 2.0.1 real (fetch.sh; sha256 confere) — linha de contagens agora traz denied-system: 0; readme_inspect passa com o APK presente.
- Verificações: cargo fmt --check ok; clippy --workspace --all-targets 0 warnings; cargo test --workspace 100% verde (readme_inspect incluído); check_md_links ok; cargo xtask compat-report byte-idêntico ao versionado.

Stage Summary:
- Critérios de aceite da #110: (1) initial_state de BATTERY_STATS ≠ AutoGranted — teste unitário ✓; (2) inspect não conta em auto-granted (categoria própria) ✓; (3) permissions_list/permissions_grant refletem denied-system ✓; (4) regra documentada em RustyDroid.md §B5 ✓. Critérios 2/3 da #68 agora atendidos.
- GitHub: branch fix/110-signature-outside-b5, PR com Closes #110.
- Limitações: nenhuma nova; o marcador runtime-is-system segue sendo o contrato de confiança para as b5 auto-granted.
- Próximo: sem bugs abertos após esta; pendências do M7 (gate golden — requer APKs golden no runner, medição em Moto E5 real, warm, heap do app) ou itens fora do M6 (timeout por passo, retries/hooks AG-07, coverage AG-09).

---
Task ID: m7-warm
Agent: Arena Agent Mode (sessão autônoma 2h — iteração A)
Task: Resolver a limitação registrada no passo 1 do M7: regime warm não era mais rápido que o cold (tool_launch recriava o Engine a cada chamada).

Work Log:
- Escolha: projeto estável (0 issues/PRs); M7 é o milestone ativo e o worklog/ROADMAP listavam "aquecer o regime warm" como pendência concreta e verificável em sandbox.
- rd-agent (mcp.rs): tool_launch reutiliza o Engine quando o package já está em execução (warm start do Android: processo vivo, activity recriada no mesmo heap; ensure_initialized idempotente — clinit não repete). Falha no warm não derruba o app (regra #82 preservada). Resultado do launch ganha `warm: true/false`. install/clear/remove continuam zerando o engine (kill_engine_if_running) — launch seguinte é frio.
- Teste novo launch_warm_reuses_running_process: cold→warm→warm explícito→ui_tree funcional→clear→cold.
- Docs: descrição da tool `launch` + mcp-tools.md regenerados; ROADMAP M7 (entrega 6 ✅; pendências sem o warm); docstring do rd perf.

Stage Summary:
- Suíte: 322 passam, 0 falham (baseline 319 +1 warm; demais contagens por doc-tests). fmt ok; clippy 0 warnings.
- GitHub: branch perf/m7-warm-start → PR (Closes n/d — não há issue; resolve limitação registrada no PR #112).
- Próximo desta sessão: timeout por passo no executor de suítes (adiamento M6) ou get_api_usage (AG-09); pendências maiores do M7 (gate golden, device real, heap do app) seguem listadas no ROADMAP.

---
Task ID: ag09-api-usage
Agent: Arena Agent Mode (sessão autônoma 2h — iteração B)
Task: Entregar o adiado do M6 get_api_usage (AG-09, metade estática): tool MCP que reporta as APIs android.* que o app referencia nos DEX.

Work Log:
- Escolha: após m7-warm (iteração A), o item de maior valor restante era o AG-09 estático (alimenta análise de risco de compat e o TST-09 "declared vs used"); get_coverage exige instrumentação e segue pendente (M8+).
- rd-agent::api_usage (novo): scan estático dos DEX — method_ids (métodos invocados), field_ids (tipos implicados), superclass/interfaces dos class_defs; saída com android_types, android_methods por classe, types_by_domain, defined_classes e marker analysis="static-dex-references" (escopo honesto).
- MCP: tool get_api_usage [--package] (31ª tool); NO_APP/PACKAGE_NOT_INSTALLED tipados; dispatch + catálogo + schema.
- Testes: unidade do scan (DexBuilder sintético: Activity/setContentView/Build$VERSION + caso vazio honesto) + tool MCP no fixture (referências presentes; erros tipados). Contagens 30→31 atualizadas em todos os asserts (mcp.rs, mcp_cli.rs) e docs (README comparação/layout/PT-BR; mcp-tools.md regenerada).
- Validação real (MCP de verdade): F-Droid 2.0.1 → install responde BELOW_FLOOR (minSdk 24 < piso 26, contrato correto) e get_api_usage responde NO_APP tipado — comportamento honesto confirmado ponta a ponta.
- Achado do processo: um assert de contagem esquecido (batch tools/list) envenenava o SESSION_LOCK e derrubava a suíte em paralelo — localizado via teste isolado; todos os asserts de contagem agora travam 31.

Stage Summary:
- Suíte: 325 passam, 0 falham; fmt ok; clippy 0 warnings; links ok; compat-matrix sync; readme_inspect ok.
- GitHub: branch feat/ag09-get-api-usage → PR.
- Próximo desta sessão: timeout por passo no executor de suítes (adiado M6) ou AG-07 (retries/hooks).

---
Task ID: m6-step-timeout
Agent: Arena Agent Mode (sessão autônoma 2h — iteração C)
Task: Entregar o adiado do M6 "timeout por passo" no executor de suítes (rd test).

Work Log:
- Decisão de design: timeout_ms opcional por passo (inteiro > 0), orçamento de WALL-CLOCK medido sem preempção (a chamada completa; o executor reprova com código estável STEP_TIMEOUT se estourar). Vale mesmo com expect:"error:…" (orçamento é contrato do passo). 0/negativo/não numérico → INVALID_SUITE antes de executar (Lei 1).
- rd-agent::suite: Step.timeout_ms + parse/validação; checagem no run_scenario; docs do módulo.
- docs/suite-format.md: campo documentado + regra 7.
- Testes: parse (aceita válido; rejeita 0/-5/"logo") + execução com APK real (timeout_ms=1 → STEP_TIMEOUT falha e pula o resto; 60000 ms → passa inteiro).

Stage Summary:
- Suíte: 326 passam, 0 falham; fmt ok; clippy 0; links ok; compat-matrix sync.
- GitHub: branch feat/m6-step-timeout → PR.
- Adiados do M6 agora entregues: get_api_usage (AG-09 estático) + timeout por passo. Restam: get_coverage (instrumentação, M8+) e AG-07 (retries/hooks).
- Próximo desta sessão: AG-07 retries/hooks ou avanço do M7 (medição de heap do app no rd perf).

---
Task ID: m7-app-heap-budget
Agent: Arena Agent Mode (sessão autônoma 2h — iteração D)
Task: M7 — medir o heap do app (orçamento ram_app_heap_mb = 256 MB), pendência do DoD do M7.

Work Log:
- rd-vm::Heap: campo peak (marca d'água do used, atualizado no único funil alloc()); API pub fn peak(). Engine: heap_peak().
- rd-agent MCP: runtime_health ganha heap_peak_bytes ao lado de heap_used_bytes.
- rd-cli perf: Run.heap_peak_bytes lido via runtime_health FORA do cronômetro (novo helper call_json); PerfReport.app_heap_peak_mb; heap_budget_status not_measured → measured; novo orçamento app_heap_peak ≤ ram_app_heap_mb (falha fechado se não medido, igual ao RSS); saída humana atualizada.
- Docs: README (bullet rd perf), ROADMAP M7 (item 7 + pendências DoD sem heap), docstrings.
- Testes: rd-vm peak_is_high_water_mark_of_used; perf heap_budget_inclusive_and_fail_closed + heap_peak_mb_max_takes_worst_run_and_fail_closed; perf_cli e2e agora exige measured + orçamento heap passando + heap_peak_bytes por execução.

Stage Summary:
- Workspace: 331 testes passam, 0 falham; fmt ok; clippy 0; links ok; compat-matrix sync; readme_inspect ok.
- DoD do M7 restante: gate golden (APKs fora do repo), Moto E5 real, workflow de release.
- Próximo: AG-07 (retries/hooks no run_scenario) — última pendência do M6.

---
Task ID: ag07-retries-hooks
Agent: Arena Agent Mode (sessão autônoma 2h — iteração E)
Task: AG-07 — run_scenario com retries e hooks (última pendência do M6 além de get_coverage).

Work Log:
- Design: hooks por cenário (setup roda antes; falha aborta cenário — passos/teardown SKIP. teardown roda depois MESMO com passos falhos, best-effort; falha reprova o cenário) + retries por passo (N tentativas extras após falha; primeira que passa encerra; tempo = soma; log da última; attempts anotado).
- suite.rs: enum Phase{Setup,Step,Teardown}; Step.retries; Scenario.setup/teardown; StepOutcome.phase/attempts; attempt_step + run_step (loop de tentativas) + skipped; run_scenario em 3 fases; parse_hook + retries no parse_step (inteiro ≥ 0 senão INVALID_SUITE); JUnit com prefixo de fase e "(N tentativas)".
- Tests: 5 casos malformed novos (retries -1/1.5/"muitas"; setup não-lista; batch no teardown); parse defaults+hooks; 6 testes de execução (teardown após falha; teardown falho reprova; setup aborta; retries=2 → attempts 3; sucesso curto-circuita; JUnit marca fase/retries).
- Docs: suite-format.md (campos, exemplo com hooks+retry, regras 8-9, "Limites atuais" corrigido — removidas linhas obsoletas de timeout/AG-07, defeito de doc da iteração C); ROADMAP (Fora do M6 só get_coverage; nova seção AG-07 ✅).
- Smoke real via CLI: suíte com setup/steps(expect NO_APP+retries)/teardown → PASS; cenário com teardown falho → FAIL "teardown 0 (launch)"; exit 1; JUnit com [setup 0]/[teardown 0].

Stage Summary:
- Workspace: 335 testes passam, 0 falham; fmt ok; clippy 0; links ok; compat-matrix sync; readme_inspect ok.
- M6: pendente só get_coverage (AG-09, instrumentação — M8+). M7: pendente gate golden, Moto E5 real, workflow de release.
- Próximo desta sessão: revisar superfície restante (docs/README coerência) ou M7 passo 2 (medições adicionais).

---
Task ID: ag09-get-coverage
Agent: Arena Agent Mode (sessão autônoma 2h — iteração F)
Task: AG-09 metade dinâmica — get_coverage (instrumentação do interpretador). Última pendência do M6.

Work Log:
- rd-vm: Engine.coverage (HashSet<u64> chave dex_idx<<32|method_idx) registrado em Engine::call (funil único de corpos DEX); coverage_snapshot() enumera classpath — denominador = métodos com code_off != 0 (abstract/native e classes host fora); structs ClassCoverage/CoverageSnapshot exportados.
- rd-agent MCP: tool get_coverage (32ª) — require_engine (NOT_RUNNING sem launch), resposta {package, analysis: method-entry-instrumentation, classes_total, methods_total, methods_executed, coverage_pct, classes[]}; descrição do get_api_usage atualizada (cross-ref).
- Catálogo 31→32: mcp.rs (catálogo/batch/defs.len), mcp_cli.rs (4 pontos), README (2 pontos), docs/mcp-tools.md regenerada (32 seções).
- ROADMAP: "Fora do M6" → nada pendente; nova seção ✅ get_coverage com escopo honesto (contagem por processo, relançar zera).
- Testes: rd-vm coverage_snapshot_counts_executed_method_bodies (abstract fora do denominador); MCP get_coverage_reports_executed_methods (NOT_RUNNING sem app e sem launch; launch→onCreate contado; pct 0-100; classe Main presente).
- Smoke real: describe-tools → 32 tools; stdio JSON-RPC → get_coverage NOT_RUNNING tipado.

Stage Summary:
- Workspace: 337 testes passam, 0 falham; fmt ok; clippy 0; links ok; compat-matrix sync; readme_inspect ok.
- M6 COMPLETO: todos os adiamentos entregues (get_api_usage, timeout por passo, AG-07, get_coverage).
- M7 pendente: gate golden (APKs fora do repo), Moto E5 real, workflow de release.

---
Task ID: suite-contract-ag07-coverage
Agent: Arena Agent Mode (sessão autônoma 2h — iteração G)
Task: CI exercitar de verdade AG-07 (hooks/retries/timeout) e get_coverage — suíte de contrato.

Work Log:
- DIVULGAÇÃO (iteração F): o commit de get_coverage (9a73cfa) foi feito e pushado DIRETO na main por engano (branch não criada antes do commit — erro de processo meu). Sem force-push para desfazer; a CI do push na main rodou e ficou 6/6 verde (mesmas verificações de um PR). A partir daqui o fluxo branch+PR está restabelecido.
- tests/suites/contract-no-app.json: +2 cenários (6→8) — "hooks e retries (AG-07) + get_coverage sem app" (setup device_profile; launch expect NO_APP com retries:1; get_coverage expect NOT_RUNNING com timeout_ms:5000; teardown runtime_health) e "teardown com expect (AG-07) roda após os passos".
- docs/compat-matrix.md regenerada (seção Suítes: 8 cenários/10 passos).
- xtask: teste suites_are_collected_with_counts atualizado (trava derivação das contagens).

Stage Summary:
- rd test contract-no-app: 8/8 PASS exit 0 local; a CI passa a exercitar hooks, retries, timeout_ms e get_coverage NOT_RUNNING em todo push.
- Workspace: 337 testes passam, 0 falham; fmt/clippy/links/matrix/readme_inspect ok.
- GitHub: branch feat/suite-contract-ag07-coverage → PR.

---
Task ID: m8-atomic-reference
Agent: Arena Agent Mode (sessão autônoma 2h — iteração H)
Task: M8 sondagem com app real + fix do 1º gap de launch (AtomicReference host).

Work Log:
- Sondagem (sem commit de binário, GH-05): fetch.sh org.primftpd → primitive ftpd 8.2, minSdk 26 (PISO OK), 11 MB, 5 DEX, Kotlin/AndroidX. rd inspect completo; install MCP ok (permissões 5 auto/2 runtime/1 special).
- Launch real falhou NOT_FOUND em AtomicReference.<init>(Object) → implementado host intrínseco completo (<init>()V/<init>(Object)/get/set/getAndSet/compareAndSet/weakCompareAndSet/compareAndExchange): estado no campo heap "value" (padrão StringBuilder); CAS por identidade de referência (Null==Null; Obj(i)==Obj(j) sse i==j); VM single-threaded ⇒ CAS exato.
- Re-sondagem no app real: gap do AtomicReference SUMIU; próximo gap honesto = java.util.WeakHashMap.<init>()V (coleções java.util não são host). Registrado no catálogo compat_gaps (java-util-collections, not-implemented) e na nova seção ROADMAP "M8 sondagem".
- Teste: atomic_reference_intrinsics (vm_exec) — init/get, CAS errado→0 (valor intacto), CAS certo→1 (troca), getAndSet devolve anterior, CAS Null==Null, <init>()V→Null.

Stage Summary:
- Workspace: 338 testes passam, 0 falham; fmt/clippy/links/matrix/readme_inspect ok.
- Próxima iteração natural (M8): coleções java.util como host (WeakHashMap/HashMap/ArrayList) — é um grupo grande, merece iteração dedicada.

---
Task ID: m8-java-util-collections
Agent: Arena Agent Mode (sessão autônoma 2h — iteração I)
Task: M8 — coleções java.util host (gap do WeakHashMap achado na sondagem real).

Work Log:
- rd-vm framework: HostObj::Map{entries}/HostObj::List{elems}.
- rd-vm intrinsics: HashMap/WeakHashMap (<init>()V/(I)V/(IF)V/(Map)V, put/get/remove/containsKey/containsValue/size/isEmpty/clear) e ArrayList/LinkedList (<init>, add/add(i)/get/set/remove(i)/remove(obj)/contains/indexOf/size/isEmpty/clear). Estado lazy-defensivo; OOB → IndexOutOfBoundsException materializada; values_equal: Null==Null, String por conteúdo, boxes (Integer/Long/Boolean/Double/Float) por valor, senão identidade — equals de usuário NÃO consultado (limitação documentada). Sem GC na arena ⇒ WeakHashMap ≡ HashMap (documentado).
- Braços do match só capturam (name,sig) suportados → métodos não cobertos seguem para Object fallback/NOT_FOUND tipado (iterators/entrySet ainda NÃO existem).
- compat_gaps: java-util-collections not-implemented → partial (sem iteradores; próximo gap real: java.lang.Enum).
- ROADMAP "M8 sondagem": coleções ✅ + próximo gap honesto (Enum.<init> via EIIE no launch do primftpd).
- Testes: hash_map_host_intrinsics (put substitui por conteúdo de chave, chave Null, remove/clear) + array_list_host_intrinsics (add/insert/set/remove/indexOf + OOB exceção Java).
- Re-sondagem app real: WeakHashMap resolvido; launch avança até ExceptionInInitializerError em java.lang.Enum.<init>(String,I) — registrado.

Stage Summary:
- Workspace: 340 testes passam, 0 falham; gates todos verdes.
- Sequência de gaps do primftpd: AtomicReference ✅ → WeakHashMap ✅ → Enum (próximo).

---
Task ID: m8-enum-host
Agent: Arena Agent Mode (sessão autônoma 2h — iteração J)
Task: M8 — java.lang.Enum host (3º gap do launch real do org.primftpd).

Work Log:
- INCIDENTE DE INFRAESTRUTURA (divulgação): no meio desta iteração o sandbox reiniciou — ~/.cargo (toolchain), .git e target/ foram perdidos; o worktree com os edits sobreviveu no snapshot. Recuperação sem force-push: rustup reinstalado (stable 1.99.0 + shims), repo re-clonado e .git restaurado no worktree, bits de execução dos scripts restaurados (snapshot perdeu modos 755). Nenhum trabalho mergeado foi perdido; o timer de parede (.timer_start) marca muito mais que 2h de trabalho efetivo por causa da parada.
- rd-vm intrinsics: Enum host — <init>(String,I)V/name()/ordinal()/toString() (default = name). Estado nos campos de heap name/ordinal (padrão StringBuilder/Throwable).
- Dispatch correto: nova API call_instance_intrinsic_mref(recv_class, mref_class, …) — o interpretador agora passa a classe DECLARANTE do method ref. Super-chamada Enum.<init> (mref = Enum, receiver = classe do app) cai no host; construtor da classe de enum do app (mref = classe do app, corpo DEX) NÃO é sequestrado; toString default só quando resolve_method não acha declaração na cadeia DEX. call_instance_intrinsic antiga delega (mref = recv_class) — chamadores/testes inalterados.
- compat_gaps: nota atualizada (Enum host; próximo gap java.lang.ref).
- ROADMAP "M8 sondagem": Enum ✅ + próximo gap honesto (WeakReference.<init> — re-sondagem real confirmou).
- Teste: enum_host_intrinsics (super-chamada preenche estado; name/ordinal/toString; guarda anti-sequestro do construtor do app).

Stage Summary:
- Workspace: 341 testes passam, 0 falham; fmt/clippy(0)/links/matrix/readme_inspect verdes.
- Sequência de gaps do primftpd: AtomicReference ✅ → WeakHashMap/coleções ✅ → Enum ✅ → java.lang.ref.WeakReference (próximo).

---
Task ID: m8-java-lang-ref
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração K)
Task: M8 — família java.lang.ref host (4º gap do launch real do org.primftpd).

Work Log:
- Sessão nova: timer reiniciado (epoch 1791863243); baseline 341/0, zero issues/PRs, main local = remoto (ea1f7be).
- rd-vm intrinsics: WeakReference/SoftReference host — <init>()V/(Object)V/(Object,ReferenceQueue)V (fila ignorada), get/clear com estado no campo de heap "referent". ReferenceQueue: <init>/poll/remove/remove(J) → null (sem GC a fila está permanentemente vazia; remove() bloqueante da JVM vira null honesto).
- Dispatch: <init> só quando o method ref DECLARA a classe (super-chamada de subclasse cai no host; construtor da subclasse do app segue para o DEX — mesma disciplina do Enum via call_instance_intrinsic_mref); get/clear só sem declaração na cadeia DEX (override de usuário vence).
- Semântica honesta: arena sem GC ⇒ referent vive até clear() (documentado, mesma política do WeakHashMap).
- compat_gaps: nota de java-util-collections atualizada + nova entrada java-lang-ref (partial).
- ROADMAP "M8 sondagem": java.lang.ref ✅ + próximo gap honesto (AtomicReferenceFieldUpdater.newUpdater, NOT_IMPLEMENTED tipado; exige reflexão de campo por nome).
- Teste: weak_soft_reference_host_intrinsics (init/get/clear; SoftReference vazio; variante com fila; ReferenceQueue poll → null; super-chamada de subclasse capturada; construtor da subclasse NÃO sequestrado).
- Re-sondagem app real: gap do WeakReference resolvido; launch avança até EIIE em AtomicReferenceFieldUpdater.newUpdater (registrado).

Stage Summary:
- Workspace: 342 testes passam, 0 falham; fmt/clippy(0)/links/matrix/readme_inspect verdes.
- Sequência de gaps do primftpd: AtomicReference ✅ → coleções ✅ → Enum ✅ → java.lang.ref ✅ → AtomicReferenceFieldUpdater.newUpdater (próximo).
- Próxima iteração: newUpdater host (updater que opera get_field/put_field pelo nome capturado) OU Object[].clone()/iteradores.

---
Task ID: m8-atomic-field-updater
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração L)
Task: M8 — AtomicReferenceFieldUpdater host (5º gap do launch real do org.primftpd).

Work Log:
- rd-vm intrinsics: newUpdater(Class,Class,String) static — classes IGNORADAS (sem access check; volatile não se aplica a VM single-thread), captura o nome do campo em campo de heap "arfu_name" e devolve instância host ARFU; fieldName null → NPE materializado.
- Métodos de instância (dispatch c==ARFU || mref==ARFU, só sigs suportadas): get/set/getAndSet/compareAndSet/weakCompareAndSet operando via get_field/put_field no alvo; campo ausente → null; alvo null → NPE; CAS por identidade de referência (ref_identity_eq, mesma política do AtomicReference).
- Antes: newUpdater caía no NOT_IMPLEMENTED tipado do catch-all Ljava/ — agora é host.
- compat_gaps: java-lang-ref atualizado (ARFU host; próximo gap = coleções concorrentes).
- ROADMAP "M8 sondagem": ARFU ✅ + próximo gap honesto (CopyOnWriteArraySet.<init> — grupo java.util.concurrent; pode reusar HostObj::List/Map).
- Teste: atomic_field_updater_host_intrinsics (newUpdater+get ausente→null, set/get, CAS errado/certo, getAndSet devolve anterior, alvo null→NPE, fieldName null→NPE).
- Re-sondagem app real: gap do ARFU resolvido; launch avança até CopyOnWriteArraySet.<init>()V (registrado).

Stage Summary:
- Workspace: 343 testes passam, 0 falham; fmt/clippy(0)/links/matrix/readme_inspect verdes.
- Sequência de gaps do primftpd: AtomicReference ✅ → coleções ✅ → Enum ✅ → java.lang.ref ✅ → ARFU ✅ → CopyOnWriteArraySet (próximo).

---
Task ID: m8-cow-collections
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração M)
Task: M8 — coleções java.util.concurrent host (6º gap) + bug de subtipagem das classes host.

Work Log:
- rd-vm intrinsics: CopyOnWriteArraySet host — mesmo estado HostObj::List, mas add DEDUPLICA por igualdade Java (find_elem/values_equal) e devolve false se presente; remove/contains/size/isEmpty/clear; <init>. CopyOnWriteArrayList entra na guarda do arm de List (≡ ArrayList; single-threaded ⇒ cópia-em-escrita irrelevante).
- BUG REAL corrigido (descoberto pela re-sondagem): check-cast COWSet → java/util/Set estourava ClassCastException — builtin_hierarchy/builtin_interfaces do is_subtype não conheciam as classes host do M8. Adicionadas cadeias completas (Abstract{Collection,List,SequentialList,Map}, HashMap/WeakHashMap, ArrayList/LinkedList, COW*, AtomicReference(+ARFU), Enum, Reference/Weak/Soft/ReferenceQueue) e closures de interface (List/Set→Collection→Iterable, Map, Deque/Queue, RandomAccess, Cloneable, Serializable, Comparable).
- Testes: cow_collections_host_intrinsics (dedupe do Set vs duplicatas da List, contains/remove/size/clear, indexOf) + host_collections_subtype_their_interfaces (unit no classpath: positivos e negativos).
- Re-sondagem app real: COWSet+CCE resolvidos; launch avança até LinkedHashMap.<init>()V (registrado como próximo gap).

Stage Summary:
- Workspace: 345 testes passam, 0 falham; fmt/clippy(0)/links/matrix verdes.
- Sequência de gaps do primftpd: AtomicReference ✅ → coleções ✅ → Enum ✅ → java.lang.ref ✅ → ARFU ✅ → COW+casts ✅ → LinkedHashMap (próximo).

---
Task ID: m8-linked-hash-map
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração N)
Task: M8 — LinkedHashMap host (7º gap do launch real do org.primftpd).

Work Log:
- rd-vm intrinsics: LINKED_MAP entra na guarda do arm de Map (map_supported: <init> variantes, put/get/remove/containsKey/containsValue/size/isEmpty/clear) — comportamento ≡ HashMap host; ordem de iteração NÃO garantida (BTreeMap por chave string, documentado).
- classpath: builtin_hierarchy (LinkedHashMap <: HashMap <: AbstractMap <: Object) + builtin_interfaces (Map).
- Teste: linked_hash_map_host_intrinsics (init/isEmpty/put→null/size/get/containsKey/remove→valor + subtipagem positiva e negativa).
- Re-sondagem app real: gap resolvido; launch atravessa todo o grupo java.util e avança até android.os.SystemClock.uptimeMillis()J (família nova: framework Android — registrado como próximo gap).

Stage Summary:
- Sequência de gaps do primftpd: AtomicReference ✅ → coleções ✅ → Enum ✅ → java.lang.ref ✅ → ARFU ✅ → COW+casts ✅ → LinkedHashMap ✅ → SystemClock (próximo).

---
Task ID: m8-system-clock
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração O)
Task: M8 — android.os.SystemClock host sobre relógio virtual (8º gap do launch real).

Work Log:
- rd-vm framework call_host_static: SYSTEM_CLOCK — uptimeMillis/elapsedRealtime/currentThreadTimeMillis → Long(clock virtual); elapsedRealtimeNanos → clock×1e6; sleep(J)V → advance_clock(ms) (avança E drena o Looper; ms<0 ignorado). Relógio = HostState.clock_ms (determinístico, mesmo contrato do postDelayed — nada de tempo de parede).
- compat_gaps: nova entrada "android-framework-clock" (partial); java-lang-ref: próximo gap atualizado (AtomicInteger).
- ROADMAP "M8 sondagem": SystemClock ✅ + próximo gap honesto (AtomicInteger.<init> — mesma família do AtomicReference host).
- Teste: system_clock_host_statics (clock nasce 0; reflete advance_clock; nanos ×1e6; sleep avança; sleep negativo não retrocede).
- Re-sondagem app real: gap resolvido; launch avança até AtomicInteger.<init>()V (registrado).

Stage Summary:
- Sequência de gaps do primftpd: AtomicReference ✅ → coleções ✅ → Enum ✅ → java.lang.ref ✅ → ARFU ✅ → COW+casts ✅ → LinkedHashMap ✅ → SystemClock ✅ → AtomicInteger (próximo).

---
Task ID: m8-atomic-integer
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração P)
Task: M8 — AtomicInteger host (9º gap do launch real do org.primftpd).

Work Log:
- rd-vm intrinsics: ATOMIC_INT — mesmo padrão do AtomicReference (campo de heap "value", Int): <init>()V/(I)V, get/intValue, set, getAndSet, compareAndSet/weakCompareAndSet (exatos — single-threaded), increment/decrement/add nas 6 variantes com overflow WRAPPING (semântica Java), longValue/floatValue/doubleValue (alargam), toString. Helpers atomic_int_cur (tolerante a Long) e arg_i32.
- classpath: builtin_hierarchy (Number → Object; AtomicInteger → Number → Object) + builtin_interfaces (AtomicInteger: Serializable).
- Teste: atomic_integer_intrinsics (init default 0, init(7), get/set, 4 variantes inc/dec, getAndAdd/addAndGet, CAS errado/certo, getAndSet, overflow MAX→MIN, conversões Number, toString "42", subtipagem).
- Re-sondagem app real: gap resolvido; launch avança até Executors.newFixedThreadPool(I,ThreadFactory) (registrado; substituto honesto documentado: ExecutorService inline/síncrono).

Stage Summary:
- Sequência de gaps do primftpd: AtomicReference ✅ → coleções ✅ → Enum ✅ → java.lang.ref ✅ → ARFU ✅ → COW+casts ✅ → LinkedHashMap ✅ → SystemClock ✅ → AtomicInteger ✅ → Executors (próximo).

---
Task ID: m8-executors
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração Q)
Task: M8 — Executors/ExecutorService host com execução inline (10º gap do launch real).

Work Log:
- rd-vm intrinsics: EXECUTORS fábricas (newFixedThreadPool(I)/(I,ThreadFactory), newSingleThreadExecutor, newCachedThreadPool — pool/ThreadFactory ignorados, VM single-threaded) → instância host ExecutorService.
- ExecutorService: execute/submit(Runnable)/submit(Runnable,result)/submit(Callable) rodam INLINE via resolve_method+Engine.call (mesmo mecanismo do drain do Looper); shutdown/shutdownNow (ArrayList vazia — nada pendente)/isShutdown/isTerminated (campo heap "exec_shutdown").
- Future host: já completado — get/get(J,TimeUnit) leem campo heap "future_result"; isDone=1; isCancelled=0; cancel=0. Desvio documentado: exceções do Runnable propagam imediatamente (Java capturaria no Future).
- classpath: ExecutorService implementa Executor (builtin_interfaces).
- Teste: executors_host_inline_execution (DEX real: LTickX run() t++ / call() t+=10 retorna "ok-call"; fábrica, execute síncrono, submit Runnable→Future null, submit Callable→resultado capturado, submit(Runnable,result), isDone/isCancelled/cancel, shutdown/isTerminated/shutdownNow vazio, subtipagem).
- Re-sondagem app real: gap resolvido; launch avança até Looper.getThread()Ljava/lang/Thread; (registrado).

Stage Summary:
- Sequência de gaps do primftpd: AtomicReference ✅ → coleções ✅ → Enum ✅ → java.lang.ref ✅ → ARFU ✅ → COW+casts ✅ → LinkedHashMap ✅ → SystemClock ✅ → AtomicInteger ✅ → Executors ✅ → Looper.getThread (próximo).

---
Task ID: m8-looper-thread
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração R)
Task: M8 — Looper.getThread + Thread host single-threaded (11º gap do launch real).

Work Log:
- framework: HostState.main_thread (lazy, como main_looper) + main_thread_obj() pub(crate); call_host_instance (LOOPER,"getThread") → thread "main". THREAD arms que estavam no framework foram REMOVIDOS (Ljava/lang/Thread; despacha via intrinsics, não Landroid/) — aprendizado de dispatch registrado.
- intrinsics: statics Thread.currentThread (→ mesma ref), sleep(J)V/(JI)V (avança relógio virtual, ms<0 ignorado), yield (no-op); instância (c == THREAD, whitelist): getName "main", setName ignorado, getId 1L, threadId 1, isDaemon 0, isAlive 1, toString "Thread[main,1,main]".
- classpath: Thread <: Object + implements Runnable.
- Teste: looper_thread_host_single_thread (getThread ≡ currentThread por identidade; todos os getters; sleep/yield sobre o relógio virtual; sleep negativo não retrocede).
- Re-sondagem app real: gap resolvido; launch atravessa android.os e avança até androidx.lifecycle.Lifecycle$State.compareTo(Ljava/lang/Enum;)I — enum do APP herda compareTo do Enum host sem DEX (registrado: fallback de subtipagem + compareTo).

Stage Summary:
- Sequência de gaps do primftpd: … → AtomicInteger ✅ → Executors ✅ → Thread ✅ → Enum.compareTo herdado (próximo).

---
Task ID: m8-enum-compareto
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração S)
Task: M8 — Enum.compareTo + herança de métodos do Enum host por enums do app (12º gap).

Work Log:
- BUG de resolução corrigido: enums do APP (classe DEX <: Ljava/lang/Enum;) que chamam métodos herdados com mref declarando a SUBCLASSE (caso real: androidx Lifecycle$State.compareTo) caíam em NOT_FOUND — Enum host não tem DEX. Fallback novo: mref != ENUM && (name,sig) ∈ {name, ordinal, toString, compareTo} && is_subtype(recv, ENUM) && resolve_method(recv,name,sig) == None → enum_intrinsic. Override de usuário vence (resolve Some); classes não-enum não são sequestradas.
- compareTo(Ljava/lang/Enum;)I implementado (também no arm mref==ENUM, que é final): diferença de ordinais wrapping_sub (comportamento JDK documentado); null → NPE materializado.
- Teste: enum_inherited_methods_and_compareto (DEX real: LMyEnum <: Enum com A/B; compareTo -1/1/0 via mref da subclasse; name/ordinal/toString herdados; NPE; LNotEnum não sequestrado).
- Re-sondagem app real: gap resolvido; launch atravessa androidx.lifecycle e avança até LinkedHashMap.entrySet()Ljava/util/Set; — família de iteradores confirmada como próximo gap (prevista desde a iteração I).

Stage Summary:
- Sequência de gaps do primftpd: … → Executors ✅ → Thread ✅ → Enum.compareTo ✅ → entrySet/iteradores (próximo).

---
Task ID: m8-collection-iterators
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração T)
Task: M8 — views (entrySet/keySet/values) + Iterator/Map.Entry host (13º gap) + bug do Context.

Work Log:
- rd-vm framework: HostObj::Iter { elems, pos } (iterador = snapshot + cursor).
- rd-vm intrinsics: Map.entrySet/keySet/values → view HashSet host (SNAPSHOT independente — em Java a view é viva, desvio documentado; ordem de inserção); Map.Entry host (campos heap entry_key/entry_value; getKey/getValue/setValue devolve anterior); iterator() para ArrayList/LinkedList/COW*/views → Iterator host: hasNext/next (NoSuchElementException quando esgota)/remove (UnsupportedOperationException). Hierarquia: AbstractSet/HashSet + interfaces Set/Collection/Iterable/Cloneable/Serializable.
- BUG PRÉ-EXISTENTE (achado pelo app real, CCE em check-cast MainTabsActivity→Context): builtin_hierarchy tinha typo "Ljava/content/Context;" na cadeia de Activity/Application — nunca coberto por teste. Corrigido para "Landroid/content/Context;" + teste android_activity_is_android_context. Debug com binário temporário sobre o APK real (removido antes do commit).
- Doc: comentário do LINKED_MAP corrigido (estado é Vec/ordem de inserção, não BTreeMap).
- Teste: collection_views_and_iterators (mapa 2 pares; entrySet size/iteração for-each com getKey/getValue na ordem k1,k2; NSE no fim; UOE no remove; keySet contains; values size; setValue devolve anterior; iterator de List).
- Re-sondagem app real: entrySet+CCE resolvidos; launch avança até Collections.synchronizedMap (registrado).

Stage Summary:
- Sequência de gaps do primftpd: … → Thread ✅ → Enum.compareTo ✅ → iteradores+Context ✅ → Collections statics (próximo).
- 13 gaps do app real fechados nesta sessão (K–T); launch já entra no código do app (androidx lifecycle atravessado).

---
Task ID: m8-collections-sync
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração U)
Task: M8 — Collections.synchronized* por identidade (14º gap do launch real).

Work Log:
- rd-vm intrinsics: statics Collections.synchronizedMap/SortedMap/List/Set/SortedSet/Collection — VM single-threaded ⇒ wrapper devolve a PRÓPRIA coleção (identidade); null → NPE materializado; whitelist exata de (name,sig) — sort etc. seguem para o catch-all tipado.
- Teste: collections_synchronized_wrappers_are_identity (identidade da ref; mapa segue funcional; synchronizedList; NPE; sort fora da whitelist → erro tipado).
- Re-sondagem app real: gap resolvido; launch avança até ArrayDeque.<init>()V (registrado — Deque host sobre estado List).

Stage Summary:
- Sequência de gaps do primftpd: … → iteradores+Context ✅ → Collections.synchronized* ✅ → ArrayDeque (próximo).

---
Task ID: m8-array-deque
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração V)
Task: M8 — ArrayDeque host (15º gap do launch real do org.primftpd).

Work Log:
- rd-vm intrinsics: ARRAY_DEQUE sobre HostObj::List (frente = índice 0) — addFirst/addLast/offerFirst/offerLast/push/pop/add/offer; removeFirst/removeLast (NSE em vazio), pollFirst/pollLast/getFirst/getLast/peekFirst/peekLast; remove(Object)/removeFirstOccurrence/removeLastOccurrence (varredura reversa nova: find_elem_last); contains/size/isEmpty/clear; iterator() (guard estendida). Null REJEITADO (NPE, como na JDK).
- classpath: ArrayDeque <: AbstractCollection <: Object; interfaces Deque/Queue/Collection/Iterable/Cloneable/Serializable.
- Teste: array_deque_host_intrinsics (vazio: peek/poll null vs get/remove NSE; NPE no null; ordem [-1,0,1] via addLast/addFirst/push; pop; add/offer → true; removeLast/FirstOccurrence; contains/remove; iterator frente→fundo; clear; subtipagem + negativo List).
- Re-sondagem app real: gap resolvido; launch avança até Object.getClass (MainTabsActivity.getClass) — registrado (Class host).

Stage Summary:
- Sequência de gaps do primftpd: … → Collections.synchronized* ✅ → ArrayDeque ✅ → getClass (próximo).

---
Task ID: m8-getclass
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração W)
Task: M8 — Object.getClass + Class host (16º gap do launch real do org.primftpd).

Work Log:
- framework: HostState.class_objs (mapa descritor → ObjRef) + class_object_for() pub(crate) — objeto Class CANÔNICO por classe (identidade estável, como na JVM); campo heap "class_name" com o descritor.
- intrinsics: arm wildcard (_, "getClass", "()Ljava/lang/Class;") — final ⇒ captura irrestrita correta; métodos de Class host (getName descritor→binário, getSimpleName descarta pacote+externa, toString "class X") com guarda defensiva resolve_method none (anti-shadowing). Helper descriptor_to_class_name.
- classpath: Class <: Object.
- Teste: get_class_and_class_host (classe do Class obj; identidade na 2ª chamada; getName/getSimpleName/toString; aninhada La/b/C$D; → a.b.C$D / D; subtipagem).
- Re-sondagem app real: gap resolvido; launch avança até ConcurrentHashMap.<init>()V (EIIE-wrapped; registrado — reuso da guarda de Map).

Stage Summary:
- Sequência de gaps do primftpd: … → ArrayDeque ✅ → getClass/Class ✅ → ConcurrentHashMap (próximo).

---
Task ID: m8-concurrent-hash-map
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração X)
Task: M8 — ConcurrentHashMap host (17º gap do launch real do org.primftpd).

Work Log:
- rd-vm intrinsics: CONCURRENT_MAP entra na guarda do Map host (mesmos métodos essenciais + views; single-threaded ⇒ sem segmentação). putIfAbsent/computeIfAbsent fora (não sondados).
- classpath: hierarquia JDK correta — ConcurrentHashMap NÃO estende HashMap (AbstractMap direto); interfaces Map/ConcurrentMap/Cloneable/Serializable.
- Teste: concurrent_hash_map_host_intrinsics (init/put/size/get/entrySet; subtipagem: <: AbstractMap, NÃO <: HashMap, <: ConcurrentMap/Map).
- Re-sondagem app real: gap resolvido; launch avança até LinkedBlockingQueue.<init>()V (EIIE-wrapped; registrado — bloqueantes exigem substituto honesto).

Stage Summary:
- Sequência de gaps do primftpd: … → getClass/Class ✅ → ConcurrentHashMap ✅ → LinkedBlockingQueue (próximo).

---
Task ID: m8-blocking-queue
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração Y)
Task: M8 — LinkedBlockingQueue host (18º gap do launch real do org.primftpd).

Work Log:
- rd-vm intrinsics: BLOCKING_Q — FIFO unbounded sobre HostObj::List (capacidade ignorada; remainingCapacity=MAX_VALUE): offer/add/put (null → NPE como na JDK), poll()/peek (null em vazia), element (NSE), remove/contains/size/isEmpty/clear. BLOQUEANTES fail-fast: take/poll(J,TimeUnit) com fila vazia → IllegalStateException materializado (bloquear sem outra thread = deadlock; falha visível > pendurar — documentado).
- classpath: LinkedBlockingQueue <: AbstractQueue <: AbstractCollection; interfaces BlockingQueue/Queue/Collection/Iterable/Serializable.
- Teste: linked_blocking_queue_host_intrinsics (vazia: poll/peek null, take/poll(J)→ISE, element→NSE; NPE no null; FIFO offer/put/add → poll/take/peek; size/contains/remove; subtipagem + negativo List).
- Re-sondagem app real: gap resolvido; launch avança até ThreadLocal.<init>()V (registrado — single-threaded: um valor por VM).

Stage Summary:
- Sequência de gaps do primftpd: … → ConcurrentHashMap ✅ → LinkedBlockingQueue ✅ → ThreadLocal (próximo).

---
Task ID: m8-thread-local
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração Z)
Task: M8 — ThreadLocal + InheritableThreadLocal host (19º/20º gaps do launch real).

Work Log:
- rd-vm intrinsics: THREAD_LOCAL/INHERITABLE_TL — um valor por instância (campos heap tl_value/tl_set): get (chama initialValue() via resolve_method+Engine.call na 1ª leitura e memoiza — como na JDK; default null), set, remove, <init>. Guard: <init> só com mref declarando a classe host (ctor de subclasse segue DEX); get/set/remove por is_subtype(recv, ThreadLocal) E resolve_method none (override de usuário vence). Inheritable ≡ ThreadLocal (single-threaded).
- classpath: ThreadLocal <: Object; InheritableThreadLocal <: ThreadLocal <: Object.
- Teste: thread_local_host_intrinsics (plain get→null, set/get/remove; subclasse LMyTL com initialValue() DEX → "inicial" memoizado, set sobrescreve; InheritableThreadLocal init/set/get; subtipagem).
- Re-sondagem app real (×2): ThreadLocal e InheritableThreadLocal resolvidos; launch avança até System.getProperty(String) (registrado — tabela de propriedades da plataforma).

Stage Summary:
- Sequência de gaps do primftpd: … → LinkedBlockingQueue ✅ → ThreadLocal(+Inheritable) ✅ → System.getProperty (próximo).
- 20 gaps do app real fechados/registrados na sessão 2 (K–Z: 10 merges).

---
Task ID: m8-system-properties
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração AA)
Task: M8 — System.getProperty host (21º gap do launch real do org.primftpd).

Work Log:
- rd-vm intrinsics: statics System.getProperty(String) e (String,String) — tabela de propriedades coerente com a VM (os.name=Linux, os.arch=arm64-v8a, os.version, line.separator=LF REAL, file.separator=/, path.separator=:, java.io.tmpdir=/tmp, user.dir//language/region/timezone, java.vendor/version/specification/vm/runtime/class.path/library.path/home, file.encoding=UTF-8, http.agent). Desconhecida → null (JDK); (key,def) → def; key null → NPE. user.home/user.name REMOVIDOS da tabela (Android não define → null honesto).
- BUG de design pego no teste: primeira versão devolvia Ok(None) para chave desconhecida = "sem intrínseco" → fall-through NOT_FOUND (errado); reestruturado para Some(Null)/def nos arms.
- Teste: system_get_property_host (os.name=Linux; line.separator=LF real; desconhecida→null; (key,def)→def; NPE).
- Re-sondagem app real: gap resolvido; launch avança até Class.getClassLoader() (registrado — extensão do Class host).

Stage Summary:
- Sequência de gaps do primftpd: … → ThreadLocal(+Inheritable) ✅ → System.getProperty ✅ → Class.getClassLoader (próximo).

---
Task ID: m8-classloader
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração AB)
Task: M8 — Class.getClassLoader + ClassLoader canônico (22º gap do launch real).

Work Log:
- framework: HostState.class_loader + class_loader_obj() pub(crate) (lazy, identidade estável — um ClassLoader por VM; runtime single-classloader, bootstrap=null não se aplica ao host).
- intrinsics: class_supported += getClassLoader; class_intrinsic trata antes de ler o descritor.
- classpath: ClassLoader <: Object.
- Teste: class_get_class_loader_host (duas classes distintas → MESMO loader; classe do host obj; identidade na repetição; subtipagem).
- Re-sondagem app real: gap resolvido; launch avança até System.getSecurityManager() (registrado — Android não tem SecurityManager; semântica correta é null).

Stage Summary:
- Sequência de gaps do primftpd: … → System.getProperty ✅ → getClassLoader ✅ → getSecurityManager (próximo).

---
Task ID: m8-security-manager
Agent: Arena Agent Mode (sessão autônoma 2h #2 — iteração AC)
Task: M8 — System.getSecurityManager → null (23º gap do launch real).

Work Log:
- rd-vm intrinsics: static System.getSecurityManager() → Null sempre (Android não tem SecurityManager; deprecated na JDK — semântica correta é null).
- Teste: system_get_security_manager_is_null.
- Re-sondagem app real: gap resolvido; launch avança até ServiceLoader.load(Class,ClassLoader) (SPI — registrado: substituto honesto é loader vazio, sem varredura de META-INF/services).

Stage Summary:
- Sequência de gaps do primftpd: … → getClassLoader ✅ → getSecurityManager ✅ → ServiceLoader (próximo).
- Sessão 2: 12 iterações (K–AC… 13), 12 PRs (#125–#143 pendente), 23 gaps do app real endereçados.

---
Task ID: m8-service-loader
Agent: Arena Agent Mode
Task: M8 — `java.util.ServiceLoader` host para o 24º gap do launch real de `org.primftpd`.

Work Log:
- Avaliação: árvore inicial limpa em `main` (47aa7ce), zero issues/PRs abertas e CI do `main` verde. `cargo test --workspace` de baseline passou; o ambiente não tinha Rust instalado, então instalei as toolchains stable e MSRV 1.76 localmente antes dos gates.
- `rd-vm`: `ServiceLoader.load(Class)` e `load(Class, ClassLoader)` retornam um host `ServiceLoader` vazio. `service == null` materializa `NullPointerException`; `ClassLoader == null` é aceito como bootstrap.
- `ServiceLoader.iterator()` retorna o `Iterator` host vazio existente: `hasNext()` é falso e `next()` materializa `NoSuchElementException`. A classe agora é `Iterable` na hierarquia builtin. Não há leitura/instanciação de `META-INF/services`, portanto o runtime não inventa providers.
- Teste novo `service_loader_host_is_empty_and_iterable`: ambas as sobrecargas, class/Iterable, iteração vazia, `NoSuchElementException` e NPE de service null.
- Catálogo `compat_gaps` e `docs/ROADMAP.md` atualizados com a limitação explícita. Re-sondagem real após o build sobre `golden-apks/org.primftpd.apk` atravessou ServiceLoader e chegou ao próximo gap estruturado: `java.util.LinkedHashSet.<init>()V`.

Checks:
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets -- -D warnings` — PASS.
- `cargo test --workspace` — PASS (0 falhas).
- `rustup run 1.76.0 cargo check --workspace` — PASS.
- `python3 scripts/check_md_links.py` — PASS (10 links / 12 arquivos).
- `cargo xtask compat-report /tmp/compat-matrix.generated.md` + `diff` — PASS.
- `rd app run golden-apks/org.primftpd.apk --json` — executou além de ServiceLoader e falhou somente no próximo gap registrado; APKs são locais/ignorados, nunca commitados.
- Revisão final: `git diff --check` e scan de token-like secrets fora de `.git`/`target` — PASS.

GitHub status:
- Branch `feat/m8-service-loader`, commit funcional `42db612`, enviado com o PAT fornecido pelo proprietário.
- Pull request aberto: #144 — https://github.com/nadabgfccvd/RustyDroid/pull/144 (aguarda revisão humana e CI).

Stage Summary:
- `ServiceLoader` deixa de bloquear a inicialização do APK real com um fallback determinístico e honesto, sem alegar suporte a SPI completo.
- Próximo foco recomendado: implementar `LinkedHashSet` host (reaproveitando o estado `List`/semântica de `Set` já usada por `CopyOnWriteArraySet`), com construtores e operações mínimas sondadas, depois reexecutar o APK real.

---
Task ID: m8-linked-hash-set
Agent: Arena Agent Mode
Task: M8 — `java.util.LinkedHashSet` host para o 25º gap de launch real de `org.primftpd`.

Work Log:
- Operação solicitada pelo proprietário: substituí o loop de shell ruidoso por `/home/user/work_timer.py`, um timer Python durável e sem dependências que grava estado JSON atomicamente em `/home/user/agent_work_timer_state.json`. Ele suporta `start`, `status`, `watch` e `clear`; o ciclo temporário de 2 s (running → complete → clear) passou e uma sessão nova de 4 h foi iniciada às 2026-10-13 08:15:13 UTC (deadline 12:15:13 UTC). O script é uma ferramenta local fora do Git, portanto não polui a PR do runtime.
- Avaliação: PR #144 (`ServiceLoader`) está limpa, aberta e com todos os checks concluídos em verde; a revisão humana continua pendente. Árvore limpa e `cargo test -p rd-vm` de baseline passou antes das mudanças.
- `rd-vm`: `LinkedHashSet` agora é Set host sobre `HostObj::List`: `<init>()`, add/remove/contains/size/isEmpty/clear e iterator. O `Vec` mantém ordem de inserção e remove+add move o elemento ao fim; igualdade segue a política existente de coleções. A hierarquia builtin declara `LinkedHashSet → HashSet → AbstractSet` e as interfaces Set/Collection/Iterable/Cloneable/Serializable.
- Teste novo `linked_hash_set_host_intrinsics`: deduplicação por conteúdo, contains, ordem do iterator, remoção+reinserção e subtipagem positiva/negativa.
- Documentação e catálogo MCP atualizados. Re-sondagem real sobre `golden-apks/org.primftpd.apk` ultrapassou LinkedHashSet e encontrou o próximo gap tipado: `ClassLoader.getResources(String) → Enumeration`.

Checks:
- `python3 -m py_compile /home/user/work_timer.py` + ciclo de estado temporário — PASS.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets -- -D warnings` — PASS.
- `cargo test --workspace` — PASS (0 falhas; novo teste LinkedHashSet incluído).
- `rustup run 1.76.0 cargo check --workspace` — PASS.
- `python3 scripts/check_md_links.py` + `cargo xtask compat-report`/`diff` — PASS.
- `rd app run golden-apks/org.primftpd.apk --json` — chega ao próximo gap estruturado de ClassLoader; APK local continua ignorado.
- Revisão final: `git diff --check` e scan de token-like secrets fora de `.git`/`target` — PASS.

GitHub status:
- Branch `feat/m8-linked-hash-set`, commit funcional `ae3ab03`, enviado com o PAT fornecido pelo proprietário.
- Pull request empilhada aberta: #145 — https://github.com/nadabgfccvd/RustyDroid/pull/145, com base em `feat/m8-service-loader`/#144 para manter cada compat-gap revisável. #144 deve receber revisão humana/merge antes de #145 seguir para `main`.

Stage Summary:
- LinkedHashSet não bloqueia mais o fluxo de inicialização do APK real e sua ordem de iteração é determinística.
- Próximo foco recomendado: `ClassLoader.getResources(String)` com uma `Enumeration` host vazia ou baseada no APK, decidindo antes como expor somente recursos permitidos. O suporte SPI completo (`META-INF/services` + instanciação segura) permanece deliberadamente fora do escopo desta iteração.

---
Task ID: m8-classloader-getresources
Agent: Arena Agent Mode (sessão 2026-10-13, início 09:30 UTC)
Task: M8 — ClassLoader.getResources(String) + Enumeration host (26º gap do launch real do org.primftpd); integração das PRs #144/#145 já verificadas.

Work Log:
- Timer: registrado timestamps UTC reais por etapa (loop `while; sleep` travaria a sessão não-interativa do sandbox; técnica equivalente adotada — início 09:30:06 UTC).
- Avaliação: main verde (cargo test --workspace EXIT 0); 0 issues abertas. Achadas 2 PRs já implementadas, com CI 6/6 verde, aguardando merge: #144 (ServiceLoader, 24º gap) e #145 (LinkedHashSet, 25º gap, empilhada). Dep combina cargo 1.99 + MSRV 1.76 instaladas no sandbox (não havia Rust).
- Integração pipeline: #144 mergida (merge commit c20c471 — padrão histórico), #145 retarguetada service-loader→main via API (mergeable: clean) e mergida (504950a). rd-vm re-testado no main mesclado: 82 testes ok.
- rd-vm intrinsics (26º gap): consts ENUMERATION/CLASS_LOADER; `ClassLoader.getResources(String)` devolve Enumeration host VAZIA (nunca null — contrato JDK; runtime não enumera entries do APK como classpath e não varre META-INF/services → não inventa recursos); name null → NPE (Objects.requireNonNull JDK 9+). Enumeration host = cursor legado reutilizando HostObj::Iter: hasMoreElements→hasNext, nextElement→next (NoSuchElementException em vazio). Cada chamada devolve cursor novo.
- Reuso deliberado: enumeration_intrinsic delega ao iterator_intrinsic (mesmo cursor, só mudam os nomes JDK 1.0) — zero duplicação de estado.
- classpath: sem entrada nova (Enumeration é interface — mesmo precedente do Iterator, que não aparece na hierarquia builtin; uma assertion is_subtype(Enumeration, Object) minha falhou no teste exatamente por isso e foi removida — o modelo é o estabelecido).
- Teste novo class_loader_get_resources_returns_empty_enumeration: loader canônico via getClass+getClassLoader; getResources → Enumeration vazia; hasMoreElements=0; nextElement → NoSuchElementException; cursor independente por chamada; NPE com name null.
- Re-sondagem app real: launch atravessou getResources e avançou até o 27º gap NOT_FOUND: `ArrayList.<init>(Ljava/util/Collection;)V` (construtor de cópia a partir de Collection). App já executa init SLF4J no caminho (avisos SLF4J(W) no stderr).
- Catálogo compat_gaps (mcp.rs) e docs/ROADMAP.md atualizados (26º ✅ / 27º re-sondado ⬜).

Checks:
- `cargo fmt --all -- --check` — PASS (1 reformatação aplicada antes).
- `cargo clippy --workspace --all-targets -- -D warnings` — PASS.
- `cargo test --workspace` — PASS (31 suites ok, 0 falhas; rd-vm vm_exec 66 testes — +1 novo).
- `rustup run 1.76.0 cargo check --workspace` — PASS (MSRV).
- `python3 scripts/check_md_links.py` — PASS (10 links / 12 arquivos).
- `cargo xtask compat-report` + diff docs/compat-matrix.md — PASS (vazio).
- `git diff --check` + scan de segredos tipo token fora de .git/target — PASS (limpo; PAT nunca gravado em arquivo).
- `rd app run golden-apks/org.primftpd.apk --json` — avança além de getResources; para no 27º gap registrado. APKs locais ignorados (GH-05).

GitHub status:
- Base: main 504950a (pós-merge #144/#145). Branch feat/m8-classloader-getresources a ser enviada com o PAT do proprietário; PR a abrir; merge após CI verde, seguindo o fluxo histórico.

Stage Summary:
- Sequência de gaps do primftpd: … → ServiceLoader ✅ (#144) → LinkedHashSet ✅ (#145) → ClassLoader.getResources+Enumeration ✅ (esta) → ArrayList.<init>(Collection) (próximo, 27º).
- Próximo foco: ArrayList.<init>(Collection) — snapshot do iterador da Collection origem sobre o List host; depois re-sondar.

---
Task ID: m8-arraylist-collection-ctor
Agent: Arena Agent Mode (sessão 2026-10-13, início 09:30 UTC — continuação)
Task: M8 — ArrayList.<init>(Ljava/util/Collection;)V host (27º gap do launch real do org.primftpd).

Work Log:
- Contexto: PR #146 (26º gap, ClassLoader.getResources+Enumeration) mergida nesta sessão (859da4ba) após CI 6/6 verde. Esta iteração partiu da sondagem dela, que parou em ArrayList.<init>(Collection).
- rd-vm: list_supported += ("<init>", "(Ljava/util/Collection;)V"); list_intrinsic copia snapshot dos elems da fonte host (ordem do iterador; duplicatas preservadas — dedup é da fonte; sem aliasing). Fonte null → NPE (c.toArray() JDK); coleção do app sem estado host → VM_TYPE_ERROR tipado (zero cópia errada silenciosa). Vale também p/ LinkedList/CopyOnWriteArrayList (helper compartilhado — ambos têm o ctor na JDK).
- Edição por posição (python) necessária: a guarda de lazy-init do list_intrinsic é textualmente idêntica ao bloco <init> — anchors de texto puro eram ambíguos.
- Teste novo array_list_collection_ctor_snapshots_source: ordem de inserção da fonte LinkedHashSet ("b","a"); get/size por conteúdo; snapshot independente (remove na fonte não altera cópia); duplicatas de ArrayList preservadas (3 elems); NPE null; VM_TYPE_ERROR para coleção do app.
- Re-sondagem app real: launch atravessou ArrayList(Collection) e parou no 28º gap NOT_FOUND: `LinkedBlockingQueue.drainTo(Ljava/util/Collection;I)I`.
- Catálogo compat_gaps (mcp.rs) e ROADMAP atualizados (27º ✅ / 28º re-sondado ⬜).

Checks:
- `cargo fmt --all -- --check` — PASS (aplicado antes).
- `cargo clippy --workspace --all-targets -- -D warnings` — PASS.
- `cargo test --workspace` — PASS (31 suites ok, 0 falhas; vm_exec 67 testes — +1 novo).
- `rustup run 1.76.0 cargo check --workspace` — PASS (MSRV).
- `python3 scripts/check_md_links.py` — PASS. `cargo xtask compat-report` + diff — PASS (vazio).
- `rd app run golden-apks/org.primftpd.apk --json` — avança além do ctor; para no 28º gap registrado.
- Scan de segredos/diff — limpo; PAT nunca gravado em arquivo.

GitHub status:
- Branch feat/m8-arraylist-collection-ctor empilhada sobre d234e88 (já mergeado via #146); base da PR = main. Push + PR + merge após CI verde, fluxo histórico.

Stage Summary:
- Sequência: … → getResources+Enumeration ✅ (#146) → ArrayList(Collection) ✅ (esta) → LinkedBlockingQueue.drainTo(Collection,int) (próximo, 28º).

---
Task ID: m8-lbq-drainto
Agent: Arena Agent Mode (sessão 2026-10-13, início 09:30 UTC — continuação)
Task: M8 — LinkedBlockingQueue.drainTo(Collection)/(Collection,int) host (28º gap do launch real do org.primftpd).

Work Log:
- Contexto: PR #147 (27º, ArrayList(Collection)) mergida nesta sessão (d85db284) após CI 6/6 verde; sondagem dela parou em drainTo. Trabalho movido para branch própria (feat/m8-lbq-drainto).
- rd-vm: queue_supported += drainTo (2 sobrecargas); queue_intrinsic drena até max (ou tudo) na ordem FIFO p/ destino host, respeitando o add do destino (Set deduplica via find_elem — JDK chama c.add). Retorno = RETIRADOS da fila (dup rejeitado por Set.add já saiu — semântica JDK). Contratos: null → NPE; drainTo(this) → IllegalArgumentException; max<=0 → 0. Destino aceito = subtipo conhecido de Collection (lazy-init no estilo dos outros intrínsecos); coleção do app → VM_TYPE_ERROR (From<String> → vm_error — sem drenar nada primeiro: validação ANTES da mutação).
- Bug de processo pego pelos gates: clippy useless_format na 1ª versão (format! com literal) → trocado para to_string; gates re-rodados.
- Teste novo linked_blocking_queue_drain_to_host: ordem FIFO + retorno 3 e fila vazia; max=2 deixa 1 na fila; max=0 → 0; Set dedup com retorno 3 (retirados) e set size 2; destino lazy-init ArrayList; NPE/IAE/VM_TYPE_ERROR com fila INTACTA após falha de contrato.
- Re-sondagem app real: launch atravessou drainTo e COMPLETOU toda a sequência java.util do bootstrap — agora para no 29º gap NOT_FOUND, o primeiro em android.app: `Activity.getFragmentManager()` (embrião do item Fragments do roadmap).
- Catálogo compat_gaps (mcp.rs) e ROADMAP atualizados (28º ✅ / 29º re-sondado ⬜ android.app).

Checks:
- `cargo fmt --all -- --check` — PASS. `cargo clippy --workspace --all-targets -- -D warnings` — PASS (após fix useless_format).
- `cargo test --workspace` — PASS (31 suites, 0 falhas; vm_exec 68 testes — +1 novo).
- `rustup run 1.76.0 cargo check --workspace` — PASS. md links — PASS. compat-report diff — PASS (vazio).
- `rd app run golden-apks/org.primftpd.apk --json` — avança além de drainTo; para no 29º gap registrado. Scan de segredos — limpo.

GitHub status:
- Branch feat/m8-lbq-drainto a ser enviada/PR; merge após CI verde.

Stage Summary:
- Sequência: … → ArrayList(Collection) ✅ (#147) → drainTo ✅ (esta) → Activity.getFragmentManager (próximo, 29º — PRIMEIRO gap android.app; transição java.util → framework).
- Nota arquitetural p/ o próximo: getFragmentManager abre o item Fragments (M8): FM canônico por Activity (estilo class_loader_obj), FragmentTransaction com add/replace/commit gravando estado; beginTransactionFragment/Attach de preference-fragment é o uso provável do primftpd. Escopo mínimo honesto: devolver FM host vazio + tipar o que ainda não for tratado.

---
Task ID: m8-get-fragment-manager
Agent: Arena Agent Mode (sessão 2026-10-13, início 09:30 UTC — continuação)
Task: M8 — Activity.getFragmentManager host (29º gap do launch real do org.primftpd; PRIMEIRO gap android.app — embrião do item Fragments).

Work Log:
- Contexto: PR #148 (28º, drainTo) aberta nesta sessão; sondagem dela completou toda a sequência java.util do bootstrap e parou em Activity.getFragmentManager — transição de fase java.util → framework.
- rd-vm: FRAGMENT_MANAGER const + HostState.fragment_manager (canônico por VM, lazy — espelha class_loader_obj; identidade estável pois o runtime executa uma activity por vez, como o FM por Activity no Android real) + fragment_manager_obj() + arm no call_host_instance com guard is_activity + sig. classpath: FragmentManager <: Object (objeto simples do host). Escopo mínimo honesto: ESTADO de fragments (back stack/transações) segue tipado/sondado.
- Bug de teste pego e corrigido durante a iteração: classe declarada no DexBuilder APÓS engine_of(&b) não existe no classpath do engine (third call devolvia None) — ambas as classes declaradas antes do engine.
- Teste novo activity_get_fragment_manager_canonical: classe + canonical (fm1==fm2), subclasse de Activity resolve via is_subtype e compartilha o mesmo FM (fm1==fm3), FragmentManager <: Object.
- Re-sondagem app real: launch atravessou getFragmentManager e parou no 30º gap NOT_FOUND: `FragmentManager.findFragmentByTag(Ljava/lang/String;)Landroid/app/Fragment;` — com FM do host vazio, resposta honesta é null.
- Catálogo compat_gaps (mcp.rs) e ROADMAP atualizados (29º ✅ android.app / 30º re-sondado ⬜).

Checks:
- fmt — PASS. clippy -D warnings — PASS. `cargo test --workspace` — PASS (31 suites, 0 falhas; vm_exec 69 — +1 novo). MSRV 1.76 — PASS. md links — PASS. compat-report diff — PASS (vazio).
- `rd app run golden-apks/org.primftpd.apk --json` — avança além de getFragmentManager; para no 30º gap registrado. Scan de segredos — limpo.

GitHub status:
- Branch feat/m8-fragment-manager a ser enviada/PR; merge após CI verde.

Stage Summary:
- Sequência: … → drainTo ✅ (#148) → getFragmentManager ✅ (esta) → findFragmentByTag (próximo, 30º — null honesto com FM vazio).
- Fragments (M8) agora TEM embrião: após findFragmentByTag/Id (null), o caminho provável é beginTransaction → FragmentTransaction (add/replace/commit → estado mínimo de back stack) — avaliar o tamanho real na próxima sondagem antes de escopar.

---
Task ID: m8-find-fragment-null
Agent: Arena Agent Mode (sessão 2026-10-13, início 09:30 UTC — continuação)
Task: M8 — FragmentManager.findFragmentByTag/findFragmentById → null com FM vazio (30º gap do launch real do org.primftpd).

Work Log:
- Contexto: PR #149 (29º, getFragmentManager) aberta; sondagem dela parou em findFragmentByTag. Iteração empilhada.
- rd-vm: arms (FRAGMENT_MANAGER, findFragmentByTag) sig (String)Fragment e (FRAGMENT_MANAGER, findFragmentById) sig (I)Fragment → Value::Null enquanto não há transações committadas (contrato Android p/ fragment não encontrado; tag null → null igual FragmentManagerImpl). findFragmentById entrou junto por ser da mesma família de "consulta a registro vazio" (evita re-sondagem redundante; documentado).
- Teste novo fragment_manager_vazio_find_devolve_null: tag presente → null; tag null → null; findFragmentById → null.
- Re-sondagem app real: launch atravessou os finds e parou no 31º gap NOT_FOUND: `FragmentManager.beginTransaction()Landroid/app/FragmentTransaction;` — abre FragmentTransaction + estado mínimo de fragments.
- Catálogo compat_gaps (mcp.rs) e ROADMAP atualizados (30º ✅ / 31º re-sondado ⬜).

Checks:
- fmt — PASS. clippy -D warnings — PASS. `cargo test --workspace` — PASS (31 suites, 0 falhas; vm_exec 70 — +1 novo). MSRV 1.76 — PASS. md links — PASS. compat-report diff — PASS (vazio).
- Probe — avança além dos finds; para no 31º gap registrado. Scan de segredos — limpo.

GitHub status:
- Branch feat/m8-find-fragment-null a ser enviada/PR; merge após CI verde.

Stage Summary:
- Sequência Fragments: getFragmentManager ✅ (#149) → find null ✅ (esta) → beginTransaction (próximo, 31º — FragmentTransaction com add/replace/commit + registro no FM).

---
Task ID: m8-fragment-transaction
Agent: Arena Agent Mode (sessão 2026-10-13, início 09:30 UTC — continuação)
Task: M8 — FragmentManager.beginTransaction + FragmentTransaction host com registro de fragments (31º gap do launch real do org.primftpd).

Work Log:
- Contexto: PR #150 (30º, finds null) mergida nesta sessão (7284792c); sondagem dela parou em beginTransaction. Decisão de escopo: transação só tem sentido como kernel coerente (add/replace/remove/commit + registro) — evitaria 4 PRs serializados sobre a mesma feature acoplada, então entrou como UMA iteração bounded.
- rd-vm: FragRecord/FragOp/FragTxState + HostState.fragments (registro por commit) + frag_txs (transações abertas; sem variante nova em HostObj → zero impacto em matches exhaustivos). arms: beginTransaction (tx nova + estado), (FRAGMENT_TX,_) → fragment_tx_intrinsic; finds (30º) promovidos de null fixo para CONSULTA ao registro (rev — última registrada vence; findById ignora containerId 0, convenção AOSP; tag null → null).
- fragment_tx_intrinsic: add(3 sobrecargas)/replace(2)/remove fluentes (ops pendentes — sem commit não há efeito observável); commit/commitAllowingStateLoss (state loss N/A — sem ciclo de estado no host): valida TUDO antes (Add de obj já registrada → ISE "already added") e aplica (Add: push; Replace: retain container≠ + push; Remove: retain obj≠); marca committed; 2º commit/uso pós-commit → ISE (frag_tx_open); fragment null → NPE (AOSP dereferencia mFragmentManager); retorno id opaco 0.
- 2 fixes de compilação pego no check: heap.as_str devolve &str emprestado (tag/want precisavam de String próprio; E0308/E0658 str_as_str).
- Teste novo fragment_transaction_commits_registered: tx nova por chamada; fluência; sem commit → find null; commit → findTag/findId veem; replace no mesmo container remove a anterior; commitAllowingStateLoss; remove; ISE em 2º commit/uso pós-commit/already-added (sem aplicação parcial); NPE add null.
- Compat: teste do 30º (finds null c/ FM vazio) segue passando — registro vazio ⇒ lookup = null.
- Re-sondagem app real: launch atravessou beginTransaction e parou no 32º gap NOT_FOUND: `Landroid/app/Fragment;-><init>()V` — o app INSTANCIA a própria fragment (ctor do super). Sondagem seguinte provável: métodos da fragment (getString/setArguments) ou commit da transação que a recebe.
- Catálogo compat_gaps (mcp.rs) e ROADMAP atualizados (31º ✅ / 32º re-sondado ⬜).

Checks:
- fmt — PASS. clippy -D warnings — PASS. `cargo test --workspace` — PASS (31 suites, 0 falhas; vm_exec 71 — +1 novo; +2 fixes de compile). MSRV 1.76 — PASS. md links — PASS. compat-report diff — PASS (vazio).
- Probe — avança além de beginTransaction; para no 32º gap registrado. Scan de segredos — limpo.

GitHub status:
- Branch feat/m8-fragment-transaction a ser enviada/PR; merge após CI verde.

Stage Summary:
- Sequência Fragments: find null ✅ (#150) → beginTransaction+tx+registro ✅ (esta) → Fragment.<init> (próximo, 32º — ctor no-op + hierarquia).
- Fragments M8 agora tem NÚCLEO REAL: FM canônico, transações com commit e registro consultável. Pendências documentadas: lifecycle DEX (Fragment.onCreate/onCreateView/onAttach), back stack/popBackStack, animations, methods da própria fragment (getString/getActivity/setArguments).

---
Task ID: m8-fragment-init
Agent: Arena Agent Mode (sessão 2026-10-13, início 09:30 UTC — continuação)
Task: M8 — Fragment.<init>()V host (32º gap do launch real do org.primftpd).

Work Log:
- Contexto: PR #151 (31º, beginTransaction+tx) aberta; sondagem dela parou em Fragment.<init>()V (super ctor da fragment do app).
- rd-vm: FRAGMENT_CLS const; arm (FRAGMENT_CLS, "<init>") sig ()V → no-op (padrão Activity/Application/Context — host não tem estado de ctor); classpath: Fragment <: Object (super das fragments do app).
- Teste novo fragment_init_e_subtipagem: ctor no-op; subtipagem MiniFrag <: Fragment <: Object.
- Re-sondagem app real: launch atravessou o ctor e parou no 33º gap NOT_FOUND: `FragmentManager.executePendingTransactions()Z` — no host commit já aplica sincronamente → nunca há pendentes → false (semântica AOSP "nothing was executed").
- Catálogo compat_gaps (mcp.rs) e ROADMAP atualizados (32º ✅ / 33º re-sondado ⬜).

Checks:
- fmt — PASS. clippy -D warnings — PASS. `cargo test --workspace` — PASS (31 suites, 0 falhas; vm_exec 72 — +1 novo). MSRV 1.76 — PASS. md links — PASS. compat-report — PASS (vazio).
- Probe — avança além do ctor; para no 33º gap registrado. Scan de segredos — limpo.

GitHub status:
- Branch feat/m8-fragment-init a ser enviada/PR; merge após CI verde.

Stage Summary:
- Sequência Fragments: beginTransaction+tx ✅ (#151) → Fragment.<init> ✅ (esta) → executePendingTransactions (próximo, 33º — false honesto: commit é síncrono).

---
Task ID: m8-execute-pending
Agent: Arena Agent Mode (sessão 2026-10-13, início 09:30 UTC — continuação)
Task: M8 — FragmentManager.executePendingTransactions → false (33º gap do launch real do org.primftpd).

Work Log:
- Contexto: PR #152 (32º, Fragment.<init>) mergida nesta sessão (6e178c68); sondagem dela parou em executePendingTransactions.
- rd-vm: arm (FRAGMENT_MANAGER, executePendingTransactions) sig ()Z → Int(0) sempre — no host o commit aplica SINCRONAMENTE, não existe fila pendente; "false" = semântica AOSP "nothing was executed" (honesto — nunca NOT_IMPLEMENTED num caminho real).
- Teste novo fragment_manager_execute_pending_sempre_false: false sem nenhuma transação e false APÓS um commit real (o commit já aplicou — nada fica pendente).
- Re-sondagem app real (com array.clone já implementado na working tree — ver próxima entrada): launch atravessou executePending e expôs o 34º gap; após ele, MUDANÇA DE REGIME: exceção Java materializada (NPE "invoke em receiver null") em vez de NOT_FOUND — o bootstrap estrutural COMPLETOU; sondagens seguintes pedem tracing de call site.
- Catálogo compat_gaps (mcp.rs) e ROADMAP atualizados (33º ✅ / 34º+ regime registrado).

Checks:
- fmt — PASS. clippy -D warnings — PASS. `cargo test --workspace` (working tree c/ 34º também presente) — PASS (31 suites, 0 falhas; vm_exec 73 nesta árvore). MSRV 1.76 — PASS. md links — PASS. compat-report — PASS (vazio).
- Probe — regime novo documentado. Scan de segredos — limpo.

GitHub status:
- Branch feat/m8-execute-pending a ser enviada/PR; merge após CI verde.

Stage Summary:
- Sequência Fragments: Fragment.<init> ✅ (#152) → executePendingTransactions ✅ (esta) → array.clone (próximo, 34º — JLS 10.7 shallow copy) → pós-bootstrap = exceções materializadas.

---
Task ID: m8-array-clone
Agent: Arena Agent Mode (sessão 2026-10-13, início 09:30 UTC — continuação)
Task: M8 — array.clone() host (+Enum.clone → CNSE) (34º gap do launch real do org.primftpd).

Work Log:
- Contexto: PR #153 (33º, executePending) aberta; sondagem dela falhava em `[Landroidx/lifecycle/Lifecycle$Event;->clone()`.
- DIAGNÓSTICO aninhado: o `[` inicial no descritor (quase despercebido na 1ª leitura) revelou method ref de TIPO ARRAY — o gap real era array.clone() (rastro de values()/$VALUES.clone() de enum), NÃO Enum.clone. 1ª tentativa (só Enum.clone → CNSE) manteve o probe parado no mesmo ponto — provada pela re-sondagem antes do commit; investigação de interp.rs mapeou o caminho real (intrinsics ← call_instance_intrinsic_mref; androidx não entra no dispatch Landroid/*).
- rd-vm: arm (c.starts_with('['), clone, ()Ljava/lang/Object;) em call_instance_intrinsic_mref → shallow copy JLS 10.7: HeapObj::Array clonado (elem/elem_class/elems — descritor real preservado, rastro issue #37) via alloc_array + extend; receiver não-array com mref de array devolve None (cai no erro de resolução normal).
- Bônus de correção semântica (mesma história do probe, mantido): Enum.clone materializa CloneNotSupportedException capturável nos DOIS matchers de enum (mref declara Enum | mref declara subclasse do app) — final na JDK/libcore, sempre lança; name/ordinal intactos após a exceção.
- Testes novos: array_clone_e_shallow_copy_com_descritor (objeto novo ≠; descritor preservado; mesmas referências; mutação no clone não toca original) e enum_clone_lanca_clone_not_supported (ambos os mref paths; mensagem; estado intacto).
- Re-sondagem app real: atravessou array.clone(); REGIME NOVO confirmado — `VM_EXCEPTION: java.lang.NullPointerException: invoke em receiver null` (exceção materializada, não NOT_FOUND). Bootstrap estrutural completo; próximas investigações pedem TRACING do call site (quem chamou com receiver null — e o que o app esperava receber).
- Catálogo compat_gaps (mcp.rs) e ROADMAP atualizados (34º ✅ / regime pós-bootstrap registrado).

Checks:
- fmt — PASS. clippy -D warnings — PASS. `cargo test --workspace` — PASS (31 suites, 0 falhas; vm_exec 75 — +2 novos). MSRV 1.76 — PASS. md links — PASS. compat-report — PASS (vazio).
- Probe — regime novo documentado. Scan de segredos — limpo.

GitHub status:
- Branch feat/m8-array-clone a ser enviada/PR; merge após CI verde.

Stage Summary:
- 34º ✅ (esta) → PRÓXIMO INVESTIGAR (não é gap de API): NPE materializado no launch; recomenda-se iteração de OBSERVABILIDADE primeiro: modo trace em `rd app run` registrando a pilha DEX (classe/método/pc) no ponto da exceção não capturada — aí sim decidir qual estado do host falta. Semântica JDK bônus entregue: Enum.clone → CNSE.

---
Task ID: m8-exception-trace
Agent: Arena Agent Mode (sessão 2026-10-13, início 09:30 UTC — continuação)
Task: M8 — Observabilidade: stack trace Java opt-in (VmConfig::exception_trace + `rd app run --trace`) — resposta ao regime pós-bootstrap registrado na iteração anterior.

Work Log:
- Contexto: iteração 34º (array.clone) registrou o regime novo: exceções Java MATERIALIZADAS em vez de NOT_FOUND; investigar pede call site, não sondagem cega.
- rd-vm: VmConfig.exception_trace (default FALSE — golden vectors e todas as saídas existentes intactas; construções de teste via ..Default::default() seguem compilando); Engine::call anota `\n  at <method_label>` por frame ao desempilhar VmExit::Exception (estilo JVM, innermost-first; erros VM estruturados não são Java e não ganham frames).
- rd-cli: flag `--trace` em `rd app run` (boot_app(+trace) → VmConfig); dump/shot seguem false; `rd vm exec` explicitamente com exception_trace default OFF (harness golden).
- Teste exception_trace_anota_frames_ao_desempilhar: default limpo (sem "at"); trace: NPE com "at LCaso;->bomba()V" ANTES de "at LCaso;->chama()V" e mensagem original preservada no início.
- PAYOFF real: `rd app run primftpd --json --trace` localizou o NPE em androidx/fragment/app/FragmentManager.dispatchCreate()V ← FragmentController ← FragmentActivity.onCreate ← MainTabsActivity.onCreate — o app usa androidx FragmentActivity (mundo androidx, ≠ FM legacy android.app entregue). Próximo passo documentado no ROADMAP: rd dex disasm do dispatchCreate para achar o receiver null (mLifecycleCallbacks/mHost).
- Catálogo/ROADMAP atualizados com o regime e o achado.

Checks:
- fmt — PASS. clippy -D warnings — PASS. `cargo test --workspace` — PASS (31 suites, 0 falhas; vm_exec 76 — +1 novo). MSRV 1.76 — PASS. md links — PASS. compat-report — PASS (vazio).
- `rd app run --help` — flag renderizada ok. Probe com --trace — call site identificado acima. Scan de segredos — limpo.

GitHub status:
- Branch feat/m8-exception-trace a ser enviada/PR; merge após CI verde.

Stage Summary:
- Observabilidade ✅ (esta) → próximo: diagnóstico do dispatchCreate androidx (disasm + estado do host ausente — provável embrião androidx.fragment.app.FragmentManager).

---
Task ID: m8-exception-trace (continuação — fase 2: postmortem AG-09)
Agent: Arena Agent Mode (sessão 2026-10-13, início 09:30 UTC)
Task: M8 — Observabilidade fase 2: postmortem de cobertura (Engine::executed_labels) no --trace; sondagem decisiva do NPE androidx; decisão arquitetural documentada.

Work Log:
- rd-vm::engine: `pub fn executed_labels()` — resolve o conjunto de cobertura (AG-09, sempre gravada na entrada de método) para rótulos `Lcls;->nome(proto)`, ordenados/dedup (determinístico). Zero custo de config; host não entra.
- rd-cli: `rd app run --trace` com VM_EXCEPTION anexa ao cause o bloco `postmortem (--trace)`: N métodos DEX executados (teto 256 no payload). Foi descoberto que o teto inicial 64 truncava e produzia leituras falsas (suspeito cortado fora da lista) — ajustado e documentado.
- Teste estendido: exception_trace_anota_frames_ao_desempilhar agora também asserta labels executados (bomba+chama presentes, saída ordenada).
- Sondagem REAL decisiva (disasm estatico já tinha esgotado): postmortem provou que `FragmentController;->attachHost`, `FragmentManager;->attachController`, `ContextAwareHelper;->dispatchOnContextAvailable` e SINTANAM os lambdas de FragmentActivity.init NUNCA executaram — o `init()` da FragmentActivity que registra OnContextAvailableListener→attachHost (diferido por androidx.activity) nunca rodou; o NPE morre em dispatchCreate com mNonConfig null exatamente porque attachController não executou. Pai androidx/activity ComponentActivity.<init> AUSENTE da cobertura → cadeia de ctor androidx é a investigação seguinte.
- Decisão arquitetural registrada: suporte a androidX Fragments (attachController e cia.) ADIADO para M9 — demanda real é pós-NPE, escopo não-elementar (ViewModelStore/Lifecycle/Listeners), e o regime atual prova com dados que o launch não depende da conclusão desse bloco até o ponto do NPE.
- ROADMAP + catálogo mcp.rs atualizados com a decisão e o próximo gap real: cadeia de ctor androidx (ComponentActivity.<init> ausente) ou retomada de fragment launch pós-NPE.

Checks:
- fmt — PASS. clippy -D warnings — PASS. `cargo test --workspace` — PASS (31 suites; vm_exec 76 — o teste robustecido). MSRV 1.76 — PASS. md links — PASS. compat-report — PASS (vazio). Sondagem real com postmortem 256 — dados acima.

GitHub status:
- PR #155 (branch feat/m8-exception-trace) atualizada com fase 2.

Stage Summary:
- Observabilidade COMPLETA ✅ (trace + postmortem + decisão androidx documentada) → próximo: NPE dispatchCreate (o engine chega a despachar sem attachController — ou a classe da FragmentActivity carregada NÃO é a mesma do disasm… ou o launcher framework pula ctor DEX — próxima investigação: confirmar qual classe MainTabsActivity estende na cadeia DEX do APK e por que init() não rodou).

---
Task ID: m8-invoke-super-range
Agent: Arena Agent Mode (sessão 2026-10-13/14, continuação; token PAT renovado em 2026-10-14)
Task: M8 — Semântica DEX de invokes (36º/37º gaps reais do launch): invoke-super com busca na super da classe definidora; variantes /range direct×virtual destrocadas; <init>()V no-op de coleções abstratas host. NOTA OPERACIONAL: o .git local foi perdido na recriação do sandbox — repo re-inicializado via fetch (origin/main=01cf1a7), mudanças da árvore preservadas de /tmp/patch; toolchain rust reinstalada (permissão rustup corrigida via chmod +x).

Work Log:
- Diagnóstico com observabilidade nova (trace+postmortem entregues em #155): disasm estático esgotado (cadeia androidx inteira ROUDA e mesmo assim mNonConfig null) → postmortem 256 revelou FragmentActivity.init/ComponentActivity.<init> RODARAM e dispatchOnContextAvailable NÃO — contradição resolvida via instrumentação temporária local de do_invoke (revertida antes do commit): invoke-super de FragmentActivity.onCreate resolvia DIRETO para androidx/core/app/ComponentActivity.onCreate, pulando androidx/activity/ComponentActivity.
- 36º gap (semântica, bug na VM): do_invoke usava superclass_of(mref.class) como start do invoke-super. Semântica DEX (Dalvik): busca inicia na SUPER da CLASSE DEFINIDORA do frame. Fix: Engine::call calcula frame_super a partir do ClassDef (type_str(superclass_idx), "" /"?"→None) e interp::exec_frame/do_invoke ganham o parâmetro; fallback antigo conservado se None. Teste de regressão invoke_super_comeca_na_super_da_classe_definidora (avo=7/pai=8 → esperado 8; sob o bug daria 7).
- 37º gap (semântica, bug na VM): máscaras de invoke/range direto BINÁRIO errado — is_direct era 0x70|0x74 e o 0x76 (invoke-direct/range) ficava como virtual. Resultado ao executar onCreate androidx: OnBackPressedDispatcher.addCallback da activity criou objeto Kotlin (lambda→FunctionReferenceImpl) cujo <init> faz invoke-direct/range {..} para FunctionReference.<init> — despachado VIRTUALMENTE na classe do receiver re-executava Impl.<init> em loop infinito → VM_STACK_OVERFLOW a 512 (guard), e com pilha padrão ABORT NATIVO (pior que erro estruturado). Fix: máscaras 0x70|0x76=direct (spec Dalvik: 6E v, 6F s, 70 d, 71 st, 72 i, 74=virtual/range, 75=super/range, 76=direct/range, 77=static/range, 78=interface/range). Efeito colateral CORRIGIDO: invoke-virtual/range agora despacha de verdade (antes ia para a classe declarada). Teste invoke_range_direct_ao_declarado_e_virtual_com_dispatch cobre as duas direções (ctor avo com iput + override 7 vs 9).
- Bug adjacente de <init> ctor abstrato: classe DEX estendendo diretamente java.util.AbstractList chama super() via invoke-direct → NOT_FOUND "método não resolvido". Arm host: ABSTRACT_COLLECTION_CTORS (AbstractCollection/List/SequentialList/Set/Map/Queue) com <init>()V no-op (sem estado; linha do <init> de Object/framework). Teste subclasse_dex_de_abstractlist_chama_super_init_noop.
- Erros próprios corrigidos no caminho: bytecode de teste com op errado (0x0B clobberava receiver), const/4 overflow (lit 4-bit com sinal — 8 vira -8 → const/16), duplicações de b.class/truncamento via python; sed com $ literal corrompeu engine.rs (reparado).
- Re-sondagem real: UI SLF4J → launch sai do NPE e do overflow e para em `método Lorg/primftpd/ui/MainTabsActivity;->getApplication()Landroid/app/Application; não resolvido` (NOT_FOUND tipado, sem exception) — próximo gap mapeado no ROADMAP.

Checks:
- fmt — PASS. clippy -D warnings — PASS. `cargo test --workspace` — PASS (31 suites; vm_exec 78 — +1 novo + ajustes já em membro). MSRV 1.76 — PASS. md links — PASS. compat-report — PASS. Sondagem real — dado acima.

GitHub status:
- Branch feat/m8-invoke-super-range → PR; merge após CI verde.

Stage Summary:
- Semântica DEX de invokes ✅ (36º+37º + adjacente abstrato) → próximo: Activity.getApplication() host; Ljava.util.* host seguem completos.

## 2026-10-14 02:17-04:50 UTC — Cadeia do launch real primftpd: Application + Collections + SharedPreferences + ServiceLoader

Sondagem→implementação→teste→re-sondagem em 8 gaps reais consecutivos (38º–45º) até a cadeia do launch encerrar em comportamento fiel ao dispositivo:

- **38º gap — Activity.getApplication()**: HostState ganha `application: Option<ObjRef>` + `application_class`; `Engine::application_obj()` lazy canônico; em `launch_app` instancia a classe do manifest (ou fallback AOSP `Landroid/app/Application;` quando a classe não está no DEX), roda `<init>`/`onCreate` DEX ANTES do da activity; arms no framework (mref Landroid/) e em `call_instance_intrinsic_mref` (mref = classe do app — o gate Landroid/ do despacho host não captura). `Engine::set_application_class` + wiring CLI/MCP a partir de `manifest.application.name` (relativo `.X` → absoluto). Teste: ordem Application(41)→Activity(42), canônica.
- **39º gap — Context.getApplicationContext()**: mesma Application (arm duplo ACTIVITY|CONTEXT; subtype-guard incluindo APPLICATION/CONTEXT).
- **40º–43º gaps — cadeia java.util.Collections do LogController**: `Collections.unmodifiableMap` (40º), `singletonList` (41º), `HashSet.<init>(Collection)` (42º — snapshot dedup), `Collections.unmodifiableSet` (43º). Views imutáveis de SNAPSHOT em classes-marcadoras JDK (`Collections$UnmodifiableMap/$UnmodifiableSet/$SingletonList`); leituras delegam; mutações → UOE tipada; null → NPE (exceto singletonList(null) — JDK aceita); coleção do app sem estado host → VM_TYPE_ERROR tipado (honestidade M8). Desvio snapshot≠view documentado (regime de synchronized*).
- **44º gap — PreferenceManager.getDefaultSharedPreferences**: objeto canônico `Landroid/content/SharedPreferencesImpl;` (AOSP); prefs começam VAZIAS (getXxx → default da chamada; contains → false; key null → NPE); sem persistência em arquivo nesta milestone; `edit()` continua fora (falta tipada se o app escrever).
- **45º gap — ServiceLoader discovery real**: `load(Class[,ClassLoader])` lê `META-INF/services/<nome binário>` do APK anexado (formato JDK: `#` comenta); `iterator()` INSTANCIA providers via DEX (helper `new_dex_instance`: alloc+<clinit>+<init>()V) no snapshot do Iter (eager ≠ lazy JDK — documentado); provider ausente → `ServiceConfigurationError` (JLS); sem APK/arquivo → loader vazio (contrato). Teste com APK sintético: provider instanciado ANTES do erro do provider ausente.
- **Fim da cadeia (fiel)**: o launch executa `PftpdApp.onCreate()` inteiro, `LogController.<clinit>/init/readPrefs` e `setActiveConfig` e morre em `ClassCastException: NOPLoggerFactory → LoggerContext` — leitura do código-fonte upstream (github wolpi/prim-ftpd `LogController.setActiveConfig`: cast INCONDICIONAL); APK golden não embarca `META-INF/services` (strip do gradle) → slf4j 2.x cai no fallback NOP → cast explode exatamente como num dispositivo real com esse APK. Não é gap da VM: **fidelidade incorporada pelo launch cadeia-completo**.

Erros próprios corrigidos no caminho: path errado em mk()/alloc (Result vs ObjRef), borrow parcial movido em matches! do teste (ref t), E0433 em intrinsics.rs (rd_vm:: vs Value::), regex python insegura (script com heredoc escapando mal), regex de rename clippy aplicada em demasia (vec!std::slice::... revertida), sed p_void renomeado em demasia (revertido; underscore só no teste novo).

Checks:
- fmt — PASS. clippy -D warnings — PASS. `cargo test --workspace` — PASS (31 suites; vm_exec 86 = 79 pós-PR#156 +7 novos: getApplication, unmodifiableMap, singletonList, hashset_init, unmodifiableSet, preferenceManager, serviceLoader_descobre). MSRV 1.76 — PASS. md — PASS. compat-report — PASS. Sondagem real — CCE tipada final acima.

GitHub status:
- Branch nova → PR; merge após CI verde.

Stage Summary:
- Launch primftpd cadeia-completo (Application→Prefs→SLF4J→NOP faithful) ✅ → próximas rodadas: novos alvos/psi de apps + gaps estruturais pendentes do ROADMAP (getResources → Enumeration, unmodifiable* restantes — empty*, edit() de prefs quando pedido).


---
Task: Apêndice G — registro e detecção dos 28 engines
Agent: Arena.ai (contribuição assistida por IA; revisão humana obrigatória)
Date: 2026-10-14

Work Log:
- Escopo confirmado com o dono: registro/detecção completos, não execução de todos os runtimes. Preservados checkboxes/seleção LANG e os estados pendentes M9/M11; nenhuma engine ausente foi marcada como executável.
- ADR 003 antes do código: separar catálogo/detecção de execução; formatos DEX/ELF são candidatos, não atribuição de linguagem; assinatura não valida payload. Nenhum código de ATL/Robolectric copiado e nenhum unsafe adicionado.
- data/engines.toml: 28 IDs estáveis, 12 módulos/features, dependências/estratégias/limites/status e regras declarativas (all/any). Nova crate rd-engine-registry valida schema/IDs/deps/ciclos/regras; o tipo público Registry só nasce do caminho validado (documento de Deserialize privado).
- Análise limitada por nomes ZIP canônicos + classes/metadados de componentes do manifest; no asset/lib payload reads, no extraction, no network, no runtime. Híbridos preservam múltiplos achados; duplicatas/paths hostis/ABIs desconhecidas são diagnósticos explícitos; 4 amostras por predicado, 100k entradas, catálogo 1 MiB.
- Features opt-in lang-01…12 com transitividade e forwarding rd-cli/rd-agent; build mínimo conserva todo o catálogo sem downloads de engines. EngineAdapter parse/event/syscall: os 28 adaptadores do registro SEMPRE retornam NOT_IMPLEMENTED(LANG-xx); VM DEX existente continua separada e parcial.
- CLI rd engine list|detect|check; engines em inspect --json e MCP app_info (32 tools, sem nova tool); --data/RD_DATA_DIR explícitos não têm fallback silencioso. Runtime não é lançado pela detecção.
- 28 casos golden de DETECÇÃO em tests/engines/signatures.json, fixtures APK/AXML/ZIP geradas em memória; testes unitários, integração, contratos, teto ZIP64, ambiguidades, híbridos, samples, CLI/MCP e drift. CI adicionado para build mínimo/12 features/all-features e MSRV all-features.
- xtask engine-report / engine-spec (só tabela marcada) / compat-report saem da mesma fonte. Spec, README, ROADMAP, manifest (registries separado de selected), mcp-tools e compat-matrix atualizados; relatório em docs/appendix-g-validation.md.

Stage Summary:
- Registro/detecção dos 28 itens ENTREGUES; novos runtimes NÃO implementados. M9/M11 continuam pendentes e budgets de execução Moto E5 não certificados nesta etapa.
- Baseline 385 → 416 testes automatizados (+31), default e all-features: 0 falhas. 1 teste OSS explicitamente ignored no comando padrão, executado separadamente: PASS.
- Mínimo + cada uma das 12 features isoladamente: 3 unitários + 17 integrações PASS em cada configuração. CLI no-default/all-features compila; fmt + clippy -D warnings (default/all) + MSRV 1.76 all + docs/drift + suite JUnit (8 cenários) verdes localmente.
- APK real F-Droid 2.0.1: 12.537.183 bytes, 973 entradas ZIP, minSdk 24; SHA-256 83d3fe522281c3cb89fce3bc05038f81b3c3b32c108536941f184c5f9bb53778. Inventário detectado e comparado ao inspect.engines (idêntico); abaixo do piso de execução, logo NENHUMA execução alegada. Binário não commitado (GH-05).
- Publicação planejada em branch separada + PR, nunca merge cego. Este log registra gates locais; status de GitHub Actions/revisão humana é separado. Nenhuma credencial em arquivos/commits.
