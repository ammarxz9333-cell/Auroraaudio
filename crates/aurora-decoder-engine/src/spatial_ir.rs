//! Backwards-compatible re-export of Aurora's codec-neutral Spatial IR.
//!
//! The canonical contract lives in `aurora-spatial-ir` so codec adapters such
//! as TrueHD, AC-4, IAMF and MPEG-H can emit one shared representation without
//! depending on decoder orchestration.

pub use aurora_spatial_ir::*;
