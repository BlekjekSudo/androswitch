//! Precise GC root extraction, ephemeron tracing, and sweep purging for
//! subsystem host states (`M8State`, `m9::State`, `AndroidxState`, `GmsState`).
//!
//! Why this exists (Issue #231):
//! Formatting `HostState.{m8, m9, androidx, gms}` through `RefScanner` via
//! `#[derive(Debug)]` caused two severe defects under the Moto E5 256 MB budget:
//! 1. Every per-object map (`tasks`, `array_maps`, `content_values`, `cursors`,
//!    `builders`, `event_objects`, …) acted as a permanent strong root rather
//!    than an ephemeron, making unreachable guest objects immortal and causing
//!    `O(historical allocations)` GC pause growth until `VM_OOM`.
//! 2. Scalar integers (`target_sdk`, `availability_code`, `nav_dest_ids`,
//!    `fit_step_objs`, SQLite row/column counts, HTTP codes, timestamps) were
//!    tokenized as `ObjRef`s and falsely pinned arbitrary heap slots.

use crate::framework::HostState;
use crate::gms::Pending as GmsPending;
use crate::heap::ObjRef;
use crate::Value;
use alloc::vec::Vec;

#[inline]
fn push_val(v: &Value, out: &mut Vec<ObjRef>) {
    if let Value::Obj(r) = v {
        if *r != 0 {
            out.push(*r);
        }
    }
}

#[inline]
fn push_opt(r: Option<ObjRef>, out: &mut Vec<ObjRef>) {
    if let Some(r) = r {
        if r != 0 {
            out.push(r);
        }
    }
}

#[cfg(feature = "advanced-components")]
fn trace_m9_notification(n: &crate::m9::Notification, out: &mut Vec<ObjRef>) {
    push_opt(n.content_intent, out);
    push_opt(n.delete_intent, out);
    for act in &n.actions {
        push_opt(act.pending, out);
    }
}

#[cfg(feature = "advanced-components")]
fn trace_m9_remote_views(rv: &crate::m9::RemoteViews, out: &mut Vec<ObjRef>) {
    for op in &rv.operations {
        if let crate::m9::RemoteOp::Click(_, r) = op {
            if *r != 0 {
                out.push(*r);
            }
        }
    }
}

/// Emits genuine long-lived `ObjRef` roots (canonical singletons, active
/// system services, registered service listeners, and persisted host tables
/// keyed by name rather than by guest object identity). Zero scalar integers
/// are emitted.
pub(super) fn collect_strong_roots(fw: &HostState, out: &mut Vec<ObjRef>) {
    // ── M8 strong roots ──────────────────────────────────────────────────
    let m8 = &fw.m8;
    for db in m8.databases.values() {
        for tbl in db.tables.values() {
            for row in &tbl.rows {
                for v in row {
                    push_val(v, out);
                }
            }
        }
    }
    out.extend(m8.receivers.iter().copied().filter(|r| *r != 0));
    push_opt(m8.work_manager, out);
    out.extend(m8.prefs_objs.values().copied().filter(|r| *r != 0));
    for (frag, _, _) in &m8.ax_fragments {
        if *frag != 0 {
            out.push(*frag);
        }
    }
    push_opt(m8.ax_fragment_manager, out);
    push_opt(m8.last_dialog, out);

    // ── M9 strong roots ──────────────────────────────────────────────────
    #[cfg(feature = "advanced-components")]
    {
        let m9 = &fw.m9;
        for n in m9.active.values() {
            trace_m9_notification(n, out);
        }
        out.extend(m9.listeners.iter().copied().filter(|r| *r != 0));
        for (&sb_obj, n) in &m9.statusbar {
            if sb_obj != 0 {
                out.push(sb_obj);
            }
            trace_m9_notification(n, out);
        }
        out.extend(m9.pending_keys.values().copied().filter(|r| *r != 0));
        push_opt(m9.focused, out);
        push_opt(m9.ime_service, out);
        for w in m9.widgets.values() {
            push_opt(w.provider, out);
            push_opt(w.root, out);
            if let Some(rv) = &w.remote {
                trace_m9_remote_views(rv, out);
            }
            out.extend(w.clicks.values().copied().filter(|r| *r != 0));
        }
        push_opt(m9.widget_provider, out);
        out.extend(
            m9.accessibility_services
                .iter()
                .copied()
                .filter(|r| *r != 0),
        );
        for ev in &m9.accessibility_events {
            if ev.source != 0 {
                out.push(ev.source);
            }
        }
        push_opt(m9.accessibility_focus, out);
        for sc in m9.shortcuts.values() {
            push_opt(sc.intent, out);
        }
        out.extend(m9.service_objects.values().copied().filter(|r| *r != 0));
    }

    // ── AndroidX (Appendix D) strong roots ───────────────────────────────
    let ax = &fw.androidx;
    out.extend(
        ax.lifecycle_state_objs
            .values()
            .copied()
            .filter(|r| *r != 0),
    );
    out.extend(
        ax.lifecycle_event_objs
            .values()
            .copied()
            .filter(|r| *r != 0),
    );
    push_opt(ax.last_nav_controller, out);
    push_opt(ax.app_initializer, out);
    for v in ax.startup_instances.values() {
        push_val(v, out);
    }

    // ── GMS / Firebase (Appendix E) strong roots ─────────────────────────
    let g = &fw.gms;
    push_opt(g.availability_obj, out);
    out.extend(g.fb_apps.values().copied().filter(|r| *r != 0));
    push_opt(g.fb_context, out);
    push_opt(g.analytics_obj, out);
    push_opt(g.auth_obj, out);
    push_opt(g.auth_current_user, out);
    out.extend(g.auth_listeners.iter().copied().filter(|r| *r != 0));
    out.extend(g.gms_signin_clients.iter().copied().filter(|r| *r != 0));
    push_opt(g.messaging_obj, out);
    out.extend(g.messaging_services.iter().copied().filter(|r| *r != 0));
    push_opt(g.crashlytics_obj, out);
    push_opt(g.firestore_obj, out);
    for fields in g.fs_docs.values() {
        for (_, v) in fields {
            push_val(v, out);
        }
    }
    for (_, listener) in &g.fs_doc_listeners {
        if *listener != 0 {
            out.push(*listener);
        }
    }
    for (q, listener) in &g.fs_query_listeners {
        if *listener != 0 {
            out.push(*listener);
        }
        for (_, v) in &q.filters {
            push_val(v, out);
        }
    }
    push_opt(g.rtdb_obj, out);
    for v in g.rtdb_values.values() {
        push_val(v, out);
    }
    for (_, listener) in &g.rtdb_listeners {
        if *listener != 0 {
            out.push(*listener);
        }
    }
    push_opt(g.rc_obj, out);
    push_opt(g.storage_obj, out);
    push_opt(g.fused_client, out);
    for (cb, _) in &g.location_requests {
        if *cb != 0 {
            out.push(*cb);
        }
    }
    push_opt(g.cast_context, out);
    push_opt(g.cast_session_manager, out);
    push_opt(g.cast_session, out);
    out.extend(g.cast_listeners.iter().copied().filter(|r| *r != 0));
    push_opt(g.safetynet_client, out);
    out.extend(g.integrity_managers.iter().copied().filter(|r| *r != 0));
}

/// Collects every `ObjRef` owner key that holds outgoing `ObjRef` / `Value`
/// references in an ephemeron table so `has_side` marks them (including old
/// owners scanned on `gc_minor` without write barriers).
pub(super) fn collect_ephemeron_keys(fw: &HostState, out: &mut Vec<ObjRef>) {
    let m8 = &fw.m8;
    out.extend(m8.recyclers.keys().copied());
    out.extend(m8.holders.keys().copied());
    out.extend(m8.dialogs.keys().copied());
    out.extend(m8.builders.keys().copied());
    out.extend(m8.cursors.keys().copied());
    out.extend(m8.content_values.keys().copied());
    out.extend(m8.intents.keys().copied());
    out.extend(m8.prefs_listeners.keys().copied());
    out.extend(m8.ax_frag_txs.keys().copied());

    #[cfg(feature = "advanced-components")]
    {
        let m9 = &fw.m9;
        out.extend(m9.action_objects.keys().copied());
        out.extend(m9.notification_objects.keys().copied());
        out.extend(m9.builders.keys().copied());
        out.extend(m9.pending.keys().copied());
        out.extend(m9.connections.keys().copied());
        out.extend(m9.remote_views.keys().copied());
        out.extend(m9.nodes.keys().copied());
        out.extend(m9.event_objects.keys().copied());
        out.extend(m9.shortcut_builders.keys().copied());
        out.extend(m9.shortcut_objects.keys().copied());
    }

    let ax = &fw.androidx;
    out.extend(ax.lifecycles.keys().copied());
    out.extend(ax.livedata.keys().copied());
    out.extend(ax.vm_stores.keys().copied());
    out.extend(ax.vm_providers.keys().copied());
    out.extend(ax.saved_state.keys().copied());
    out.extend(ax.array_maps.keys().copied());
    out.extend(ax.array_sets.keys().copied());
    out.extend(ax.sparse.keys().copied());
    out.extend(ax.lru.keys().copied());
    out.extend(ax.preferences.keys().copied());
    out.extend(ax.work_data.keys().copied());
    out.extend(ax.cancellation.keys().copied());
    out.extend(ax.biometric_prompts.keys().copied());
    out.extend(ax.futures.keys().copied());

    let g = &fw.gms;
    out.extend(g.tasks.keys().copied());
    out.extend(g.task_sources.keys().copied());
    out.extend(g.pending.keys().copied());
    out.extend(g.api_exceptions.keys().copied());
    out.extend(g.api_clients.keys().copied());
    out.extend(g.api_client_builders.keys().copied());
    out.extend(g.auth_result_users.keys().copied());
    out.extend(g.fs_queries.keys().copied());
    out.extend(g.fs_doc_snaps.keys().copied());
    out.extend(g.fs_query_snaps.keys().copied());
    out.extend(g.storage_list_results.keys().copied());
    out.extend(g.loc_results.keys().copied());
    out.extend(g.billing_clients.keys().copied());
    out.extend(g.billing_builders.keys().copied());
    out.extend(g.purchases_results.keys().copied());
    out.extend(g.ad_views.keys().copied());
    out.extend(g.full_screen_ads.keys().copied());
    out.extend(g.google_maps.keys().copied());
    out.extend(g.map_by_fragment.keys().copied());
    out.extend(g.marker_map.keys().copied());
}

/// Traces outgoing `ObjRef`s from all ephemeron entries owned by reachable `r`.
pub(super) fn trace_ephemeron_owner(fw: &HostState, r: ObjRef, out: &mut Vec<ObjRef>) {
    // ── M8 ephemerons ────────────────────────────────────────────────────
    let m8 = &fw.m8;
    if let Some(rec) = m8.recyclers.get(&r) {
        push_opt(rec.adapter, out);
        push_opt(rec.layout_manager, out);
    }
    if let Some(&item_view) = m8.holders.get(&r) {
        if item_view != 0 {
            out.push(item_view);
        }
    }
    for dlg in [m8.dialogs.get(&r), m8.builders.get(&r)]
        .into_iter()
        .flatten()
    {
        push_opt(dlg.view, out);
        for (_, listener) in dlg.buttons.values() {
            push_opt(*listener, out);
        }
    }
    if let Some(cur) = m8.cursors.get(&r) {
        for row in &cur.rows {
            for v in row {
                push_val(v, out);
            }
        }
    }
    if let Some(kvs) = m8.content_values.get(&r) {
        for (_, v) in kvs {
            push_val(v, out);
        }
    }
    if let Some(ist) = m8.intents.get(&r) {
        push_opt(ist.data, out);
    }
    if let Some(listeners) = m8.prefs_listeners.get(&r) {
        out.extend(listeners.iter().copied().filter(|x| *x != 0));
    }
    if let Some(ops) = m8.ax_frag_txs.get(&r) {
        for (frag, _, _) in ops {
            if *frag != 0 {
                out.push(*frag);
            }
        }
    }

    // ── M9 ephemerons ────────────────────────────────────────────────────
    #[cfg(feature = "advanced-components")]
    {
        let m9 = &fw.m9;
        if let Some(act) = m9.action_objects.get(&r) {
            push_opt(act.pending, out);
        }
        if let Some(n) = m9.notification_objects.get(&r) {
            trace_m9_notification(n, out);
        }
        if let Some(n) = m9.builders.get(&r) {
            trace_m9_notification(n, out);
        }
        if let Some(p) = m9.pending.get(&r) {
            if p.intent != 0 {
                out.push(p.intent);
            }
            if p.context != 0 {
                out.push(p.context);
            }
        }
        if let Some(&target) = m9.connections.get(&r) {
            if target != 0 {
                out.push(target);
            }
        }
        if let Some(rv) = m9.remote_views.get(&r) {
            trace_m9_remote_views(rv, out);
        }
        if let Some(&view) = m9.nodes.get(&r) {
            if view != 0 {
                out.push(view);
            }
        }
        if let Some(ev) = m9.event_objects.get(&r) {
            if ev.source != 0 {
                out.push(ev.source);
            }
        }
        if let Some(sc) = m9.shortcut_builders.get(&r) {
            push_opt(sc.intent, out);
        }
        if let Some(sc) = m9.shortcut_objects.get(&r) {
            push_opt(sc.intent, out);
        }
    }

    // ── AndroidX ephemerons ──────────────────────────────────────────────
    let ax = &fw.androidx;
    if let Some(lh) = ax.lifecycles.get(&r) {
        push_opt(lh.owner, out);
        out.extend(lh.observers.iter().copied().filter(|x| *x != 0));
    }
    if let Some(ld) = ax.livedata.get(&r) {
        push_val(&ld.value, out);
        for obs in &ld.observers {
            push_opt(obs.owner, out);
            push_opt(obs.registry, out);
            if obs.observer != 0 {
                out.push(obs.observer);
            }
        }
    }
    if let Some(vms) = ax.vm_stores.get(&r) {
        for (_, vm_obj) in vms {
            if *vm_obj != 0 {
                out.push(*vm_obj);
            }
        }
    }
    if let Some(&(owner_or_store, factory)) = ax.vm_providers.get(&r) {
        push_opt(owner_or_store, out);
        push_opt(factory, out);
    }
    if let Some(entries) = ax.saved_state.get(&r) {
        for (_, v) in entries {
            push_val(v, out);
        }
    }
    if let Some(entries) = ax.array_maps.get(&r) {
        for (k, v) in entries {
            push_val(k, out);
            push_val(v, out);
        }
    }
    if let Some(entries) = ax.array_sets.get(&r) {
        for v in entries {
            push_val(v, out);
        }
    }
    if let Some(entries) = ax.sparse.get(&r) {
        for v in entries.values() {
            push_val(v, out);
        }
    }
    if let Some(lru) = ax.lru.get(&r) {
        for (k, v, _) in &lru.entries {
            push_val(k, out);
            push_val(v, out);
        }
    }
    if let Some(entries) = ax.preferences.get(&r) {
        for (_, v) in entries {
            push_val(v, out);
        }
    }
    if let Some(entries) = ax.work_data.get(&r) {
        for (_, v) in entries {
            push_val(v, out);
        }
    }
    if let Some(ch) = ax.cancellation.get(&r) {
        push_opt(ch.listener, out);
    }
    if let Some(&cb) = ax.biometric_prompts.get(&r) {
        push_opt(cb, out);
    }
    if let Some(fh) = ax.futures.get(&r) {
        push_val(&fh.result, out);
        out.extend(fh.listeners.iter().copied().filter(|x| *x != 0));
    }

    // ── GMS / Firebase ephemerons ────────────────────────────────────────
    let g = &fw.gms;
    if let Some(th) = g.tasks.get(&r) {
        push_val(&th.result, out);
        if let Some(exc) = &th.exception {
            push_val(exc, out);
        }
        out.extend(th.success_listeners.iter().copied().filter(|x| *x != 0));
        out.extend(th.failure_listeners.iter().copied().filter(|x| *x != 0));
        out.extend(th.complete_listeners.iter().copied().filter(|x| *x != 0));
        out.extend(th.canceled_listeners.iter().copied().filter(|x| *x != 0));
    }
    if let Some(&task) = g.task_sources.get(&r) {
        if task != 0 {
            out.push(task);
        }
    }
    if let Some(pendings) = g.pending.get(&r) {
        for p in pendings {
            match p {
                GmsPending::Cont { cont, out: o, .. } => {
                    if *cont != 0 {
                        out.push(*cont);
                    }
                    if *o != 0 {
                        out.push(*o);
                    }
                }
                GmsPending::Flatten { out: o } => {
                    if *o != 0 {
                        out.push(*o);
                    }
                }
                GmsPending::WhenAll { out: o, all, .. } => {
                    if *o != 0 {
                        out.push(*o);
                    }
                    out.extend(all.iter().copied().filter(|x| *x != 0));
                }
            }
        }
    }
    if let Some(&status_obj) = g.api_exceptions.get(&r) {
        if status_obj != 0 {
            out.push(status_obj);
        }
    }
    if let Some(client) = g.api_clients.get(&r) {
        push_opt(client.listener, out);
    }
    if let Some(listeners) = g.api_client_builders.get(&r) {
        out.extend(listeners.iter().copied().filter(|x| *x != 0));
    }
    if let Some(&user) = g.auth_result_users.get(&r) {
        if user != 0 {
            out.push(user);
        }
    }
    if let Some(q) = g.fs_queries.get(&r) {
        for (_, v) in &q.filters {
            push_val(v, out);
        }
    }
    if let Some(snap) = g.fs_doc_snaps.get(&r) {
        for (_, v) in &snap.fields {
            push_val(v, out);
        }
    }
    if let Some(docs) = g.fs_query_snaps.get(&r) {
        out.extend(docs.iter().copied().filter(|x| *x != 0));
    }
    if let Some(items) = g.storage_list_results.get(&r) {
        out.extend(items.iter().copied().filter(|x| *x != 0));
    }
    if let Some(locs) = g.loc_results.get(&r) {
        out.extend(locs.iter().copied().filter(|x| *x != 0));
    }
    if let Some(bc) = g.billing_clients.get(&r) {
        push_opt(bc.listener, out);
    }
    if let Some(bb) = g.billing_builders.get(&r) {
        push_opt(bb.listener, out);
        push_opt(bb.sku_details, out);
    }
    if let Some((_, list)) = g.purchases_results.get(&r) {
        out.extend(list.iter().copied().filter(|x| *x != 0));
    }
    if let Some(av) = g.ad_views.get(&r) {
        push_opt(av.listener, out);
    }
    if let Some(fsa) = g.full_screen_ads.get(&r) {
        push_opt(fsa.callback, out);
    }
    if let Some(mh) = g.google_maps.get(&r) {
        out.extend(mh.markers.iter().copied().filter(|x| *x != 0));
        push_opt(mh.marker_click_listener, out);
        push_opt(mh.ui_settings, out);
    }
    if let Some(&map_obj) = g.map_by_fragment.get(&r) {
        if map_obj != 0 {
            out.push(map_obj);
        }
    }
    if let Some(&map_obj) = g.marker_map.get(&r) {
        if map_obj != 0 {
            out.push(map_obj);
        }
    }
}

/// Purges dead owner keys from every per-object table across `m8`, `m9`,
/// `androidx`, and `gms` so freed slots never retain host state or poison a
/// recycled `ObjRef`.
pub(super) fn purge_dead<F>(fw: &mut HostState, alive: &F)
where
    F: Fn(&ObjRef) -> bool,
{
    // ── M8 purge ─────────────────────────────────────────────────────────
    let m8 = &mut fw.m8;
    m8.recyclers.retain(|r, _| alive(r));
    m8.holders.retain(|r, _| alive(r));
    m8.dialogs.retain(|r, _| alive(r));
    m8.builders.retain(|r, _| alive(r));
    m8.db_by_obj.retain(|r, _| alive(r));
    m8.cursors.retain(|r, _| alive(r));
    m8.content_values.retain(|r, _| alive(r));
    m8.helpers_db.retain(|r, _| alive(r));
    m8.uris.retain(|r, _| alive(r));
    m8.intents.retain(|r, _| alive(r));
    m8.prefs_name_by_obj.retain(|r, _| alive(r));
    m8.prefs_editors.retain(|r, _| alive(r));
    m8.prefs_clear.retain(|r, _| alive(r));
    m8.prefs_listeners.retain(|r, _| alive(r));
    m8.room_builders.retain(|r, _| alive(r));
    m8.ax_frag_txs.retain(|r, _| alive(r));

    // ── M9 purge ─────────────────────────────────────────────────────────
    #[cfg(feature = "advanced-components")]
    {
        let m9 = &mut fw.m9;
        m9.channel_objects.retain(|r, _| alive(r));
        m9.channel_values.retain(|r, _| alive(r));
        m9.action_objects.retain(|r, _| alive(r));
        m9.notification_objects.retain(|r, _| alive(r));
        m9.builders.retain(|r, _| alive(r));
        m9.pending.retain(|r, _| alive(r));
        m9.editors.retain(|r, _| alive(r));
        m9.connections.retain(|r, _| alive(r));
        m9.remote_views.retain(|r, _| alive(r));
        m9.widget_hosts.retain(|r, _| alive(r));
        m9.nodes.retain(|r, _| alive(r));
        m9.event_objects.retain(|r, _| alive(r));
        m9.descriptions.retain(|r, _| alive(r));
        m9.shortcut_builders.retain(|r, _| alive(r));
        m9.shortcut_objects.retain(|r, _| alive(r));
    }

    // ── AndroidX purge ───────────────────────────────────────────────────
    let ax = &mut fw.androidx;
    ax.lifecycles.retain(|r, _| alive(r));
    ax.state_name_by_obj.retain(|r, _| alive(r));
    ax.event_name_by_obj.retain(|r, _| alive(r));
    ax.livedata.retain(|r, _| alive(r));
    ax.vm_stores.retain(|r, _| alive(r));
    ax.vm_providers.retain(|r, _| alive(r));
    ax.saved_state.retain(|r, _| alive(r));
    ax.array_maps.retain(|r, _| alive(r));
    ax.array_sets.retain(|r, _| alive(r));
    ax.sparse.retain(|r, _| alive(r));
    ax.lru.retain(|r, _| alive(r));
    ax.pref_keys.retain(|r, _| alive(r));
    ax.preferences.retain(|r, _| alive(r));
    ax.work_data.retain(|r, _| alive(r));
    ax.nav.retain(|r, _| alive(r));
    ax.nav_dest_ids.retain(|r, _| alive(r));
    ax.cancellation.retain(|r, _| alive(r));
    ax.prompt_builders.retain(|r, _| alive(r));
    ax.prompt_infos.retain(|r, _| alive(r));
    ax.biometric_prompts.retain(|r, _| alive(r));
    ax.futures.retain(|r, _| alive(r));

    // ── GMS / Firebase purge ─────────────────────────────────────────────
    let g = &mut fw.gms;
    g.tasks.retain(|r, _| alive(r));
    g.task_sources.retain(|r, _| alive(r));
    g.pending.retain(|r, _| alive(r));
    g.statuses.retain(|r, _| alive(r));
    g.conn_results.retain(|r, _| alive(r));
    g.api_exceptions.retain(|r, _| alive(r));
    g.api_clients.retain(|r, _| alive(r));
    g.api_client_builders.retain(|r, _| alive(r));
    g.fb_app_names.retain(|r, _| alive(r));
    g.fb_app_options.retain(|r, _| alive(r));
    g.fb_options_objs.retain(|r, _| alive(r));
    g.fb_options_builders.retain(|r, _| alive(r));
    g.auth_user_by_obj.retain(|r, _| alive(r));
    g.auth_result_users.retain(|r, _| alive(r));
    g.token_results.retain(|r, _| alive(r));
    g.gms_account_by_obj.retain(|r, _| alive(r));
    g.remote_messages.retain(|r, _| alive(r));
    g.rm_builders.retain(|r, _| alive(r));
    g.ckav_builders.retain(|r, _| alive(r));
    g.ckav_objs.retain(|r, _| alive(r));
    g.fs_refs.retain(|r, _| alive(r));
    g.fs_queries.retain(|r, _| alive(r));
    g.fs_doc_snaps.retain(|r, _| alive(r));
    g.fs_query_snaps.retain(|r, _| alive(r));
    g.fs_set_options.retain(|r, _| alive(r));
    g.rtdb_refs.retain(|r, _| alive(r));
    g.rtdb_snaps.retain(|r, _| alive(r));
    g.rc_value_objs.retain(|r, _| alive(r));
    g.storage_refs.retain(|r, _| alive(r));
    g.storage_list_results.retain(|r, _| alive(r));
    g.upload_snaps.retain(|r, _| alive(r));
    g.loc_request_objs.retain(|r, _| alive(r));
    g.loc_results.retain(|r, _| alive(r));
    g.loc_availability.retain(|r, _| alive(r));
    g.billing_clients.retain(|r, _| alive(r));
    g.billing_builders.retain(|r, _| alive(r));
    g.billing_results.retain(|r, _| alive(r));
    g.sku_details_objs.retain(|r, _| alive(r));
    g.product_details_objs.retain(|r, _| alive(r));
    g.purchase_by_obj.retain(|r, _| alive(r));
    g.purchases_results.retain(|r, _| alive(r));
    g.ad_views.retain(|r, _| alive(r));
    g.full_screen_ads.retain(|r, _| alive(r));
    g.ad_errors.retain(|r, _| alive(r));
    g.reward_items.retain(|r, _| alive(r));
    g.latlngs.retain(|r, _| alive(r));
    g.camera_updates.retain(|r, _| alive(r));
    g.camera_positions.retain(|r, _| alive(r));
    g.camera_pos_builders.retain(|r, _| alive(r));
    g.google_maps.retain(|r, _| alive(r));
    g.map_by_fragment.retain(|r, _| alive(r));
    g.marker_options.retain(|r, _| alive(r));
    g.markers.retain(|r, _| alive(r));
    g.marker_map.retain(|r, _| alive(r));
    g.fit_clients.retain(|r, _| alive(r));
    g.fit_step_objs.retain(|r, _| alive(r));
    g.nearby_clients.retain(|r, _| alive(r));
    g.payload_objs.retain(|r, _| alive(r));
    g.endpoint_info.retain(|r, _| alive(r));
    g.nearby_messages.retain(|r, _| alive(r));
    g.attestations.retain(|r, _| alive(r));
    g.integrity_requests.retain(|r, _| alive(r));
    g.integrity_responses.retain(|r, _| alive(r));
}
