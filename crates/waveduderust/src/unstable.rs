//! The library's internals for first-party frontends: they may break in any release and the API diff skips them.
//! Each module is public with its `unstable-<area>` feature and crate-private without it.

#[cfg(feature = "unstable-engine")]
#[path = "engine/mod.rs"]
pub mod engine;
#[cfg(not(feature = "unstable-engine"))]
#[path = "engine/mod.rs"]
pub(crate) mod engine;

#[cfg(feature = "unstable-engine")]
#[path = "render/mod.rs"]
pub mod render;
#[cfg(not(feature = "unstable-engine"))]
#[path = "render/mod.rs"]
pub(crate) mod render;

#[cfg(feature = "unstable-engine")]
#[path = "rng.rs"]
pub mod rng;
#[cfg(not(feature = "unstable-engine"))]
#[path = "rng.rs"]
pub(crate) mod rng;

#[cfg(feature = "unstable-sync")]
#[path = "sync.rs"]
pub mod sync;
#[cfg(not(feature = "unstable-sync"))]
#[path = "sync.rs"]
pub(crate) mod sync;

#[cfg(feature = "unstable-scene")]
#[path = "scene/mod.rs"]
pub mod scene;
#[cfg(not(feature = "unstable-scene"))]
#[path = "scene/mod.rs"]
pub(crate) mod scene;

#[cfg(feature = "unstable-run")]
#[path = "spec.rs"]
pub mod spec;
#[cfg(not(feature = "unstable-run"))]
#[path = "spec.rs"]
pub(crate) mod spec;

#[cfg(feature = "unstable-export")]
#[path = "export/mod.rs"]
pub mod export;
#[cfg(all(feature = "export", not(feature = "unstable-export")))]
#[path = "export/mod.rs"]
pub(crate) mod export;

#[cfg(feature = "unstable-view")]
#[path = "view/mod.rs"]
pub mod view;
#[cfg(all(feature = "view", not(feature = "unstable-view")))]
#[path = "view/mod.rs"]
pub(crate) mod view;
