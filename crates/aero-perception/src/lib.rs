//! Read-only semantic page observation boundary.
//!
//! D01: [`axtree`] parses one frame's accessibility tree into a pruned tree in
//! DOM order; [`observation`] merges frame trees, assigns observation-scoped
//! element handles and renders bounded, escaped text for the planner.

pub mod axtree;
pub mod observation;
