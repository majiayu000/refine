#![allow(dead_code)]

#[path = "../src/cognitive_portrait_data/mod.rs"]
mod cognitive_portrait_data;
#[path = "../src/insights_manifest.rs"]
mod insights_manifest;

use chrono::{Duration, TimeZone, Utc};
use refine_core::knowledge::{
    DocumentId, Item, ItemId, ItemType, ObservationDocumentMeta, ObservationWindowSnapshot,
    RestoreParams, Tag,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct TrackingAllocator;

static TRACK_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static TRACKED_BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every operation delegates to `System` with the unchanged pointer and
// layout. The atomics only count requested bytes during the isolated probe.
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACK_ALLOCATIONS.load(Ordering::Relaxed) {
            TRACKED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        // SAFETY: this forwards the allocator contract unchanged to `System`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` and `layout` came from the delegated allocator.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if TRACK_ALLOCATIONS.load(Ordering::Relaxed) {
            TRACKED_BYTES.fetch_add(new_size, Ordering::Relaxed);
        }
        // SAFETY: this forwards the allocator contract unchanged to `System`.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

fn long_observation(
    id: String,
    document_id: &str,
    content: String,
    timestamp: chrono::DateTime<Utc>,
) -> Item {
    Item::restore(RestoreParams {
        id: ItemId::from(id.as_str()),
        item_type: ItemType::Observation,
        title: "bounded portrait".to_string(),
        summary: String::new(),
        content,
        tags: vec![Tag::new("bounded-project").expect("valid fixture tag")],
        source: None,
        document_id: Some(DocumentId::from(document_id)),
        excerpt: None,
        created_at: timestamp,
        updated_at: timestamp,
    })
    .expect("valid fixture observation")
}

#[test]
#[ignore = "explicit allocator oracle; run with --ignored --exact --test-threads=1"]
fn bounded_projection_does_not_clone_long_unique_cohort_text() {
    const OBSERVATIONS: usize = 5_000;
    const LONG_LINE_BYTES: usize = 16 * 1024;
    const MAX_PROJECTION_ALLOCATIONS: usize = 64 * 1024 * 1024;

    let cutoff = Utc.with_ymd_and_hms(2026, 8, 28, 0, 0, 0).unwrap();
    let event_time = cutoff - Duration::days(1);
    let padding = "x".repeat(LONG_LINE_BYTES);
    let current: Vec<Item> = (0..OBSERVATIONS)
        .map(|index| {
            long_observation(
                format!("long-{index:05}"),
                "current-doc",
                format!("知识:\n- unique-{index:05}-{padding}"),
                event_time,
            )
        })
        .collect();
    let snapshot = ObservationWindowSnapshot {
        current,
        previous: vec![long_observation(
            "previous".to_string(),
            "previous-doc",
            "知识:\n- previous".to_string(),
            cutoff - Duration::days(91),
        )],
        documents: vec![
            ObservationDocumentMeta {
                id: DocumentId::from("current-doc"),
                source: "codex-session".to_string(),
                captured_at: event_time,
            },
            ObservationDocumentMeta {
                id: DocumentId::from("previous-doc"),
                source: "claude-code-session".to_string(),
                captured_at: cutoff - Duration::days(91),
            },
        ],
    };

    TRACKED_BYTES.store(0, Ordering::Relaxed);
    TRACK_ALLOCATIONS.store(true, Ordering::SeqCst);
    let bundle = cognitive_portrait_data::build_bundle_from_snapshot(snapshot, cutoff, 90)
        .expect("long-text fixture must remain bounded");
    TRACK_ALLOCATIONS.store(false, Ordering::SeqCst);
    let allocated = TRACKED_BYTES.load(Ordering::Relaxed);
    eprintln!("tracked projection allocations: {allocated} bytes");

    assert_eq!(
        bundle.current.evidence_selection.eligible_observations,
        OBSERVATIONS
    );
    assert!(bundle.current.evidence_selection.selected_observations <= 2_048);
    assert!(
        allocated <= MAX_PROJECTION_ALLOCATIONS,
        "projection allocated {allocated} bytes after snapshot construction; limit is {MAX_PROJECTION_ALLOCATIONS}"
    );
}
