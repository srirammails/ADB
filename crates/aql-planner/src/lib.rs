//! AQL Planner - Query routing and execution plan generation
//!
//! Converts parsed AST into executable plans that can be run against backends.

mod error;
mod plan;
mod planner;

pub use error::{PlanError, PlanResult};
pub use plan::{
    ExecutionPlan, FanOutPlan, LinkData, LinkOptions, Operation, PipelinePlan, PipelineStage,
    ReflectPlan, ReflectSourcePlan, StepPlan, StoreData,
};
pub use planner::Planner;
