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
