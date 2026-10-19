//! Deterministic advanced-component APK fixture. Source-built, not a real APK gate.
use crate::apkfix::*;
use crate::vm::*;
pub const PACKAGE: &str = "test.m9";
pub const MAIN: &str = "Ltest/m9/Main;";
pub const WIDGET: &str = "Ltest/m9/Widget;";
pub const IME: &str = "Ltest/m9/Keyboard;";
pub const A11Y: &str = "Ltest/m9/Accessibility;";
pub const LISTENER: &str = "Ltest/m9/Listener;";
pub fn apk() -> Vec<u8> {
    let mut arsc = ArscFix::new();
    let layout = arsc.add_layout("main");
    let id_first = arsc.add_id("first");
    let id_second = arsc.add_id("second");
    let id_text = arsc.add_id("text");
    let xml = AxNode::new(
        "LinearLayout",
        vec![a(true, "orientation", AxVal::Int(1))],
        vec![
            AxNode::new(
                "EditText",
                vec![
                    a(true, "id", AxVal::Ref(id_first)),
                    a(true, "text", AxVal::Str("A😀B".into())),
                ],
                vec![],
            ),
            AxNode::new(
                "EditText",
                vec![
                    a(true, "id", AxVal::Ref(id_second)),
                    a(true, "text", AxVal::Str("second".into())),
                ],
                vec![],
            ),
            AxNode::new(
                "TextView",
                vec![
                    a(true, "id", AxVal::Ref(id_text)),
                    a(true, "text", AxVal::Str("widget".into())),
                ],
                vec![],
            ),
        ],
    );
    let mut b = DexBuilder::new();
    let setp = b.proto_idx("V", vec!["I".into()]);
    let set = b.method_idx("Landroid/app/Activity;", setp, "setContentView");
    let main = b.class(MAIN, "Landroid/app/Activity;");
    let mut code = op31i_local(0x14, 0, layout);
    code.extend(op35c(0x6e, 2, set, [1, 0, 0, 0, 0]));
    code.extend(op10x(0x0e));
    let code = b.code(3, 2, 2, code);
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(code),
    );
    for (class, base, methods) in [
        (
            WIDGET,
            "Landroid/appwidget/AppWidgetProvider;",
            vec![
                ("onEnabled", vec!["Landroid/content/Context;"]),
                (
                    "onUpdate",
                    vec![
                        "Landroid/content/Context;",
                        "Landroid/appwidget/AppWidgetManager;",
                        "[I",
                    ],
                ),
                ("onDeleted", vec!["Landroid/content/Context;", "[I"]),
                ("onDisabled", vec!["Landroid/content/Context;"]),
            ],
        ),
        (
            IME,
            "Landroid/inputmethodservice/InputMethodService;",
            vec![
                ("onInitializeInterface", vec![]),
                (
                    "onStartInput",
                    vec!["Landroid/view/inputmethod/EditorInfo;", "Z"],
                ),
            ],
        ),
        (
            A11Y,
            "Landroid/accessibilityservice/AccessibilityService;",
            vec![
                ("onServiceConnected", vec![]),
                (
                    "onAccessibilityEvent",
                    vec!["Landroid/view/accessibility/AccessibilityEvent;"],
                ),
            ],
        ),
        (
            LISTENER,
            "Landroid/service/notification/NotificationListenerService;",
            vec![
                ("onListenerConnected", vec![]),
                (
                    "onNotificationPosted",
                    vec!["Landroid/service/notification/StatusBarNotification;"],
                ),
                (
                    "onNotificationRemoved",
                    vec!["Landroid/service/notification/StatusBarNotification;"],
                ),
            ],
        ),
    ] {
        let c = b.class(class, base);
        for (name, params) in methods {
            b.static_field(c, name, "I", SVal::Int(0));
            let field = b.field_idx(class, "I", name);
            let mut units = op21c(0x60, 0, field);
            units.extend(op22b(0xd8, 0, 0, 1));
            units.extend(op21c(0x67, 0, field));
            units.extend(op10x(0x0e));
            let code = b.code(2 + params.len() as u16, 1 + params.len() as u16, 0, units);
            b.r#virtual(c, name, "V", params, ACC_PUBLIC, Some(code));
        }
    }
    let manifest = AxNode::new(
        "manifest",
        vec![a(false, "package", AxVal::Str(PACKAGE.into()))],
        vec![
            AxNode::new(
                "uses-sdk",
                vec![
                    a(true, "minSdkVersion", AxVal::Int(26)),
                    a(true, "targetSdkVersion", AxVal::Int(34)),
                ],
                vec![],
            ),
            AxNode::new(
                "uses-permission",
                vec![a(
                    true,
                    "name",
                    AxVal::Str("android.permission.POST_NOTIFICATIONS".into()),
                )],
                vec![],
            ),
            AxNode::new(
                "application",
                vec![],
                vec![
                    AxNode::new(
                        "activity",
                        vec![
                            a(true, "name", AxVal::Str("test.m9.Main".into())),
                            a(true, "exported", AxVal::Bool(true)),
                        ],
                        vec![AxNode::new(
                            "intent-filter",
                            vec![],
                            vec![
                                AxNode::new(
                                    "action",
                                    vec![a(
                                        true,
                                        "name",
                                        AxVal::Str("android.intent.action.MAIN".into()),
                                    )],
                                    vec![],
                                ),
                                AxNode::new(
                                    "category",
                                    vec![a(
                                        true,
                                        "name",
                                        AxVal::Str("android.intent.category.LAUNCHER".into()),
                                    )],
                                    vec![],
                                ),
                            ],
                        )],
                    ),
                    AxNode::new(
                        "receiver",
                        vec![a(true, "name", AxVal::Str("test.m9.Widget".into()))],
                        vec![],
                    ),
                    AxNode::new(
                        "service",
                        vec![a(true, "name", AxVal::Str("test.m9.Keyboard".into()))],
                        vec![],
                    ),
                    AxNode::new(
                        "service",
                        vec![a(true, "name", AxVal::Str("test.m9.Accessibility".into()))],
                        vec![],
                    ),
                    AxNode::new(
                        "service",
                        vec![a(true, "name", AxVal::Str("test.m9.Listener".into()))],
                        vec![],
                    ),
                ],
            ),
        ],
    );
    let mut z = ZipFix::new();
    z.add("AndroidManifest.xml", build_axml(&manifest));
    z.add("resources.arsc", arsc.build());
    z.add("classes.dex", b.finish());
    z.add("res/layout/main.xml", build_axml(&xml));
    z.build()
}
fn op31i_local(op: u8, r: u8, v: u32) -> Vec<u16> {
    vec![
        u16::from(op) | (u16::from(r) << 8),
        v as u16,
        (v >> 16) as u16,
    ]
}
