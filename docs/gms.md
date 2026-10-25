# Google Play Services / Firebase — modo "sem GMS" honesto (Apêndice E)

<!-- GERADO por `cargo xtask gms-report` a partir de `data/gms.toml` — não editar à mão. -->

Fonte: `RustyDroid.md Appendix E`. Modo: `no-gms-honest`. Janela: API 26–36. Serviços registrados: **21** em **3** categorias (supported 0, partial 0, stub 0, not_implemented 21).

**Modo honesto (validado):** não há processo GMS, conta Google nem backend real. Todo serviço declara `network = "none"`; as respostas são shims locais estruturados, determinísticos e programáveis pelo agente, e eventos são capturados localmente — nunca enviados. `supported`/`partial` medem o shim para o subconjunto documentado, jamais paridade com os servidores do Google. `GoogleApiAvailability` programável com default `SUCCESS` significa apenas "o shim responde". Firestore/RTDB são stores locais (record/replay NET-03), sem sincronização.

## Resumo por categoria

| Categoria | Serviços (shim sem GMS) | Status dos serviços | Módulos |
|---|---|---|---|
| GMS core | GoogleApiAvailability / common, Tasks API | not_implemented 2 | `none` |
| Play Services | Location (fused), Maps, Billing, Auth (Google Sign-In), Ads, Fit/Health, Cast, Nearby, SafetyNet / Play Integrity | not_implemented 9 | `none` |
| Firebase | Firebase core (FirebaseApp), Firebase Analytics, Firebase Auth, Firebase Messaging (FCM), Firebase Crashlytics, Cloud Firestore, Realtime Database, Firebase Remote Config (extra além da spec), Firebase Storage (extra além da spec), ML Kit (extra além da spec) | not_implemented 10 | `none` |

## Serviços

| Serviço | Coordenadas | Prefixos | Status | Módulo | Marco | Gap | Programável pelo agente | Notas |
|---|---|---|---|---|---|---|---|---|
| GoogleApiAvailability / common | `com.google.android.gms:play-services-base` | `com.google.android.gms.common` | not_implemented | `none` | future | gap-gms-common | Código de disponibilidade devolvido (SUCCESS/SERVICE_MISSING/...), textos de erro. | GoogleApiAvailability/GoogleApiClient/ConnectionResult sem shim: apps que checam disponibilidade no startup não recebem resposta host. |
| Tasks API | `com.google.android.gms:play-services-tasks` | `com.google.android.gms.tasks` | not_implemented | `none` | future | gap-gms-tasks | Resultados/exceções de Tasks de todos os serviços E nascem aqui; listeners disparam deterministicamente. | Task/TaskCompletionSource/Tasks sem modelo host — é a fundação de Location/Auth/Firestore/FCM e precisa existir antes deles. |
| Ads | `com.google.android.gms:play-services-ads` | `com.google.android.gms.ads` | not_implemented | `none` | future | gap-gms-ads | Sucesso/falha de load por formato; eventos estruturados capturados, sem rede nem criativos. | MobileAds/AdView/Interstitial sem shim; nenhum evento de anúncio é registrado. |
| Auth (Google Sign-In) | `com.google.android.gms:play-services-auth` | `com.google.android.gms.auth` | not_implemented | `none` | future | gap-gms-auth | Conta fake (id/email/nome/token) ou ausência de conta; provedores fake. | GoogleSignIn sem shim; getLastSignedInAccount/signInIntent não respondem via host. |
| Billing | `com.android.billingclient:billing` | `com.android.billingclient` | not_implemented | `none` | future | gap-gms-billing | Catálogo (produtos/preços), resultado do fluxo de compra, purchases possuídas, erros BillingResponseCode. | BillingClient sem mock: o fluxo de compras completo (conexão, query, launch, acknowledge/consume) não é testável. |
| Cast | `com.google.android.gms:play-services-cast` | `com.google.android.gms.cast` | not_implemented | `none` | future | gap-gms-cast | Dispositivos/sessões fake opcionais (NET-08); default: nenhum dispositivo descoberto. | CastContext/MediaRouter GMS sem shim; descoberta não retorna rotas host. |
| Fit/Health | `com.google.android.gms:play-services-fitness` | `com.google.android.gms.fitness` | not_implemented | `none` | future | gap-gms-fit-health | Datasets fake (passos, frequência, sessões) devolvidos às leituras; gravações capturadas. | Google Fit sem shim. Health Connect é API de plataforma/androidx.health — fora deste serviço (ver Apêndice C). |
| Location (fused) | `com.google.android.gms:play-services-location` | `com.google.android.gms.location` | not_implemented | `none` | future | gap-gms-location | Fix GPS (lat/lng/accuracy/tempo) devolvido por getLastLocation/getCurrentLocation; callbacks registrados (MEDIA-05). | FusedLocationProviderClient sem shim; nenhum fix virtual é entregue. |
| Maps | `com.google.android.gms:play-services-maps` | `com.google.android.gms.maps` | not_implemented | `none` | future | gap-gms-maps | Câmera e marcadores registrados num modelo consultável; sem tiles de rede (UI-07 offline). | GoogleMap/SupportMapFragment sem shim; getMapAsync nunca chama o callback via host. |
| Nearby | `com.google.android.gms:play-services-nearby` | `com.google.android.gms.nearby` | not_implemented | `none` | future | gap-gms-nearby | Endpoints fake programáveis (NET-08); default: descoberta vazia, advertising registrado. | Nearby Connections/Messages sem shim; startDiscovery/startAdvertising não respondem via host. |
| SafetyNet / Play Integrity | `com.google.android.play:integrity` | `com.google.android.gms.safetynet`, `com.google.android.play.core.integrity` | not_implemented | `none` | future | gap-gms-integrity | Token/veredito determinístico fake, rotulado como shim; nonce registrado. NUNCA atesta um dispositivo real. | SafetyNetClient/IntegrityManager sem shim; apps com gate de integridade não recebem token (nem fake) via host. |
| Cloud Firestore | `com.google.firebase:firebase-firestore` | `com.google.firebase.firestore` | not_implemented | `none` | future | gap-firebase-firestore | Store local record/replay (NET-03): documentos semeados pelo agente; listeners disparam nas escritas locais. | FirebaseFirestore sem store local; collection/document get/set não respondem via host. |
| Firebase Analytics | `com.google.firebase:firebase-analytics` | `com.google.firebase.analytics` | not_implemented | `none` | future | gap-firebase-analytics | Nada a programar: eventos/user properties/user id são CAPTURADOS e consultáveis pelo agente. | FirebaseAnalytics.logEvent sem captura host; eventos se perdem (ou quebram) sem GMS. |
| Firebase Auth | `com.google.firebase:firebase-auth` | `com.google.firebase.auth` | not_implemented | `none` | future | gap-firebase-auth | Usuário fake (uid/email), sucesso/falha de sign-in por método, sessão atual. | FirebaseAuth sem provedores fake; signIn*/createUser* retornam Tasks que o host não completa. |
| Firebase Crashlytics | `com.google.firebase:firebase-crashlytics` | `com.google.firebase.crashlytics` | not_implemented | `none` | future | gap-firebase-crashlytics | Captura: recordException/log/custom keys + crashes REAIS (exceções não tratadas da VM), nunca upload. | FirebaseCrashlytics sem captura host; nem exceções registradas nem crashes da sessão são coletados. |
| Firebase Messaging (FCM) | `com.google.firebase:firebase-messaging` | `com.google.firebase.messaging` | not_implemented | `none` | future | gap-firebase-messaging | Token FCM fake; tópicos inscritos; injeção de RemoteMessage pelo agente (SYS-08). | FirebaseMessaging.getToken/subscribe sem shim; mensagens push não podem ser injetadas no app. |
| Firebase Remote Config (extra além da spec) | `com.google.firebase:firebase-config` | `com.google.firebase.remoteconfig` | not_implemented | `none` | future | gap-firebase-remoteconfig | (Futuro) mapa chave→valor programável; fetch completa sem rede. | Extra além das balas do Apêndice E (apps reais o usam em todo startup): registrado para classificação honesta, sem shim. |
| Firebase Storage (extra além da spec) | `com.google.firebase:firebase-storage` | `com.google.firebase.storage` | not_implemented | `none` | future | gap-firebase-storage | (Futuro) blobs locais por path; URLs de download fake determinísticas. | Extra além das balas do Apêndice E: registrado para classificação, sem shim. |
| Firebase core (FirebaseApp) | `com.google.firebase:firebase-common` | `com.google.firebase` | not_implemented | `none` | future | gap-firebase-core | FirebaseOptions fake (project/app id, api key) do app default; initializeApp registrado. | FirebaseApp.initializeApp/getInstance sem shim; serviços Firebase nem chegam a inicializar sem o DEX + google-services. |
| ML Kit (extra além da spec) | `com.google.mlkit:*:*` | `com.google.mlkit` | not_implemented | `none` | future | gap-mlkit | (Futuro) resultados fake de OCR/barcode programáveis sobre imagens de entrada. | Extra: ML Kit (OCR/barcode) aparece em apps reais; sem shim, Tasks de reconhecimento nunca completam via host. |
| Realtime Database | `com.google.firebase:firebase-database` | `com.google.firebase.database` | not_implemented | `none` | future | gap-firebase-rtdb | Árvore local record/replay (NET-03): valores semeados; ValueEventListeners disparam nas escritas locais. | FirebaseDatabase sem store local; getReference/setValue não respondem via host. |

## Como atualizar

1. Ao mudar um shim GMS/Firebase, atualize a linha correspondente em `data/gms.toml` no mesmo PR (mantendo `network = "none"` e o contrato `programmable`).
2. Rode `rd gms check --json` (ou os testes de `rd-framework`).
3. Regenere este arquivo e a tabela da spec com `cargo xtask gms-report` e `cargo xtask gms-spec`.

Contexto e ledger: [appendix-d-e.md](appendix-d-e.md).
