//! Regression tests for issue #363: `Parcelable.ClassLoaderCreator` must be a
//! `Parcelable.Creator` (Compose `ParcelableSnapshotMutableState` cast), and
//! `LinkedHashMap`/`WeakHashMap`/`IdentityHashMap` must not claim
//! `ConcurrentMap` (only `ConcurrentHashMap` implements it).

mod common;

use common::*;

const CREATOR: &str = "Landroid/os/Parcelable$Creator;";
const CLASS_LOADER_CREATOR: &str = "Landroid/os/Parcelable$ClassLoaderCreator;";
const CONCURRENT_MAP: &str = "Ljava/util/concurrent/ConcurrentMap;";

#[test]
fn class_loader_creator_is_a_creator() {
    let engine = engine_of(&DexBuilder::new());
    assert!(engine.cp.is_subtype(CLASS_LOADER_CREATOR, CREATOR));
}

#[test]
fn only_concurrent_hash_map_is_a_concurrent_map() {
    let engine = engine_of(&DexBuilder::new());
    assert!(engine
        .cp
        .is_subtype("Ljava/util/concurrent/ConcurrentHashMap;", CONCURRENT_MAP));
    for class in [
        "Ljava/util/LinkedHashMap;",
        "Ljava/util/WeakHashMap;",
        "Ljava/util/IdentityHashMap;",
        "Ljava/util/HashMap;",
    ] {
        assert!(
            !engine.cp.is_subtype(class, CONCURRENT_MAP),
            "{class} must not be a ConcurrentMap"
        );
    }
    // The Map contract is kept.
    assert!(engine
        .cp
        .is_subtype("Ljava/util/LinkedHashMap;", "Ljava/util/Map;"));
}
