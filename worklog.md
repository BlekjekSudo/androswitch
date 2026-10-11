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
