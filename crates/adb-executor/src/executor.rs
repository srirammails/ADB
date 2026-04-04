//! Query Executor
//!
//! Executes planned queries against ADB backends.

use std::sync::Arc;
use std::time::Instant;

use std::collections::HashMap;

use adb_backends::Adb;
use adb_core::{Condition, MemoryType, Predicate, Scope, Value};
use aql_parser::parse;
use aql_planner::{ExecutionPlan, FanOutPlan, Operation, PipelinePlan, PipelineStage, Planner, ReflectPlan, StepPlan};

use crate::error::{ExecutorError, ExecutorResult};
use crate::result::{QueryResult, ResultSet, SourceResult};

/// The query executor
pub struct Executor {
    /// The ADB instance
    adb: Arc<Adb>,
    /// The planner
    planner: Planner,
}

impl Executor {
    /// Create a new executor
    pub fn new(adb: Arc<Adb>) -> Self {
        Self {
            adb,
            planner: Planner::new(),
        }
    }

    /// Execute an AQL query string
    pub async fn execute(&self, query: &str) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        // Parse
        let stmt = parse(query).map_err(|e| ExecutorError::Parse(e.to_string()))?;

        // Plan
        let plan = self.planner.plan(&stmt)?;

        // Execute
        let result = self.execute_plan(&plan).await?;

        // Update timing
        let mut result = result;
        result.metadata.execution_time_ms = start.elapsed().as_millis() as u64;

        Ok(result)
    }

    /// Execute a pre-planned query
    pub async fn execute_plan(&self, plan: &ExecutionPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        let result = match plan {
            ExecutionPlan::Single(step) => self.execute_step(step).await?,
            ExecutionPlan::Pipeline(pipeline) => self.execute_pipeline(pipeline).await?,
            ExecutionPlan::Reflect(reflect) => self.execute_reflect(reflect).await?,
            ExecutionPlan::FanOut(fanout) => self.execute_fanout(fanout).await?,
        };

        let mut result = result;
        result.metadata.execution_time_ms = start.elapsed().as_millis() as u64;
        Ok(result)
    }

    /// Execute a single step
    async fn execute_step(&self, step: &StepPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        let result = match step.operation {
            Operation::Scan => self.execute_scan(step).await?,
            Operation::Recall => self.execute_recall(step).await?,
            Operation::Lookup => self.execute_lookup(step).await?,
            Operation::Load => self.execute_load(step).await?,
            Operation::Store => self.execute_store(step).await?,
            Operation::Update => self.execute_update(step).await?,
            Operation::Forget => self.execute_forget(step).await?,
            Operation::Link => self.execute_link(step).await?,
        };

        let mut result = result;
        result.metadata.execution_time_ms = start.elapsed().as_millis() as u64;
        Ok(result)
    }

    /// Execute a pipeline
    async fn execute_pipeline(&self, pipeline: &PipelinePlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();
        let mut step_results = Vec::new();
        let mut var_bindings: HashMap<String, Value> = HashMap::new();

        for (i, stage) in pipeline.stages.iter().enumerate() {
            // Bind variables in the stage's predicate from previous results
            let bound_stage = match stage {
                PipelineStage::Step(step) => {
                    let bound_step = self.bind_variables_in_step(step, &var_bindings);
                    PipelineStage::Step(bound_step)
                }
                PipelineStage::Reflect(reflect) => PipelineStage::Reflect(reflect.clone()),
            };

            let result = match &bound_stage {
                PipelineStage::Step(step) => self.execute_step(step).await,
                PipelineStage::Reflect(reflect) => self.execute_reflect(reflect).await,
            };

            match result {
                Ok(r) => {
                    // Extract variable bindings from this stage's results
                    self.extract_bindings(&r, &mut var_bindings);
                    step_results.push(r);
                }
                Err(e) => {
                    return Err(ExecutorError::PipelineError {
                        step: i,
                        message: e.to_string(),
                    });
                }
            }
        }

        Ok(QueryResult::pipeline(
            step_results,
            start.elapsed().as_millis() as u64,
        ))
    }

    /// Bind variables in a step's predicate from bindings
    fn bind_variables_in_step(&self, step: &StepPlan, bindings: &HashMap<String, Value>) -> StepPlan {
        let mut bound_step = step.clone();
        bound_step.predicate = self.bind_variables_in_predicate(&step.predicate, bindings);
        bound_step
    }

    /// Bind variables in a predicate
    fn bind_variables_in_predicate(&self, predicate: &Predicate, bindings: &HashMap<String, Value>) -> Predicate {
        match predicate {
            Predicate::Where { conditions } => {
                let bound_conditions: Vec<Condition> = conditions
                    .iter()
                    .map(|c| self.bind_variables_in_condition(c, bindings))
                    .collect();
                Predicate::Where { conditions: bound_conditions }
            }
            Predicate::Key { field, value } => Predicate::Key {
                field: field.clone(),
                value: self.bind_value(value, bindings),
            },
            _ => predicate.clone(),
        }
    }

    /// Bind variables in a condition (handles both Simple and Group)
    fn bind_variables_in_condition(&self, condition: &Condition, bindings: &HashMap<String, Value>) -> Condition {
        match condition {
            Condition::Simple { field, operator, value, logical_op } => {
                Condition::Simple {
                    field: field.clone(),
                    operator: *operator,
                    value: self.bind_value(value, bindings),
                    logical_op: *logical_op,
                }
            }
            Condition::Group { conditions, logical_op } => {
                Condition::Group {
                    conditions: conditions.iter()
                        .map(|c| self.bind_variables_in_condition(c, bindings))
                        .collect(),
                    logical_op: *logical_op,
                }
            }
        }
    }

    /// Bind a value if it's a variable
    fn bind_value(&self, value: &Value, bindings: &HashMap<String, Value>) -> Value {
        match value {
            Value::Variable(var_name) => {
                bindings.get(var_name).cloned().unwrap_or_else(|| value.clone())
            }
            _ => value.clone(),
        }
    }

    /// Extract variable bindings from query results
    fn extract_bindings(&self, result: &QueryResult, bindings: &mut HashMap<String, Value>) {
        match &result.data {
            ResultSet::Records { records } => {
                // Extract from the first record (for pipeline use)
                if let Some(record) = records.first() {
                    if let Some(obj) = record.data.as_object() {
                        for (key, val) in obj {
                            let value: Value = json_to_value(val);
                            bindings.insert(key.clone(), value);
                        }
                    }
                }
            }
            ResultSet::Reflect { sources, .. } => {
                // Extract from first source's first record
                if let Some(source) = sources.first() {
                    if let Some(record) = source.records.first() {
                        if let Some(obj) = record.data.as_object() {
                            for (key, val) in obj {
                                let value: Value = json_to_value(val);
                                bindings.insert(key.clone(), value);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Execute a fan-out query across ALL memory types
    async fn execute_fanout(&self, fanout: &FanOutPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        match fanout.operation {
            Operation::Recall => {
                // Fan-out RECALL across all memory types
                let mut all_records = Vec::new();
                let mut source_results = Vec::new();

                for mem_type in MemoryType::all() {
                    let records = self
                        .adb
                        .recall_with_modifiers(*mem_type, &fanout.predicate, &fanout.modifiers)
                        .await?;

                    source_results.push(SourceResult {
                        memory_type: format!("{:?}", mem_type),
                        records: records.clone(),
                    });
                    all_records.extend(records);
                }

                // Apply limit to merged results if specified
                if let Some(limit) = fanout.modifiers.limit {
                    all_records.truncate(limit);
                }

                // Return as Reflect-style multi-source result for visibility
                Ok(QueryResult {
                    success: true,
                    data: ResultSet::Reflect {
                        sources: source_results,
                        links: Vec::new(),
                    },
                    metadata: crate::result::ResultMetadata {
                        execution_time_ms: start.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                })
            }
            Operation::Forget => {
                // Fan-out FORGET across all memory types
                let mut total_count = 0u64;

                for mem_type in MemoryType::all() {
                    let count = self.adb.forget(*mem_type, &fanout.predicate).await?;
                    total_count += count;
                }

                Ok(QueryResult::count(total_count, start.elapsed().as_millis() as u64))
            }
            Operation::Update => {
                // Fan-out UPDATE across all memory types
                let mut total_count = 0u64;

                if let Some(data) = &fanout.data {
                    for mem_type in MemoryType::all() {
                        let count = self
                            .adb
                            .update(*mem_type, &fanout.predicate, data.clone())
                            .await?;
                        total_count += count;
                    }
                }

                Ok(QueryResult::count(total_count, start.elapsed().as_millis() as u64))
            }
            _ => Err(ExecutorError::UnsupportedOperation(format!(
                "{} FROM ALL",
                fanout.operation
            ))),
        }
    }

    /// Execute a reflect query
    async fn execute_reflect(&self, reflect: &ReflectPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();
        let mut source_results = Vec::new();
        let mut all_links: Vec<adb_core::Link> = Vec::new();

        // Execute each source
        for source in &reflect.sources {
            let records = self
                .adb
                .recall_with_modifiers(source.memory_type, &source.predicate, &source.modifiers)
                .await?;

            source_results.push(SourceResult {
                memory_type: format!("{:?}", source.memory_type),
                records,
            });
        }

        // Handle link options if set
        if let Some(link_opts) = &reflect.link_options {
            // If FOLLOW LINKS is set, traverse links and return target records
            if link_opts.follow {
                let mut target_results = Vec::new();

                for source_result in &source_results {
                    for record in &source_result.records {
                        // Get the source memory type
                        let source_mem_type = record.memory_type;

                        // Get links from this record
                        let links = self
                            .adb
                            .get_links_from(
                                source_mem_type,
                                &record.id,
                                link_opts.link_type.as_deref(),
                            )
                            .await?;

                        // Collect target record IDs by memory type
                        let mut targets_by_type: std::collections::HashMap<MemoryType, Vec<String>> =
                            std::collections::HashMap::new();

                        for link in links {
                            targets_by_type
                                .entry(link.to_type)
                                .or_default()
                                .push(link.to_id.clone());

                            // Also collect the link
                            all_links.push(link);
                        }

                        // Fetch target records from each memory type
                        for (mem_type, ids) in targets_by_type {
                            for id in ids {
                                // Lookup each target record
                                let target_records = self
                                    .adb
                                    .lookup(mem_type, &adb_core::Predicate::Key {
                                        field: "id".to_string(),
                                        value: adb_core::Value::String(id),
                                    })
                                    .await?;

                                if !target_records.is_empty() {
                                    target_results.push(SourceResult {
                                        memory_type: format!("{:?}", mem_type),
                                        records: target_records,
                                    });
                                }
                            }
                        }
                    }
                }

                // Return target records instead of source records when following links
                if !target_results.is_empty() {
                    source_results = target_results;
                }
            } else if link_opts.with_links {
                // WITH LINKS - include link metadata with source records
                for source_result in &source_results {
                    for record in &source_result.records {
                        let links = self
                            .adb
                            .get_links_from(
                                record.memory_type,
                                &record.id,
                                link_opts.link_type.as_deref(),
                            )
                            .await?;

                        all_links.extend(links);
                    }
                }
            }
        }

        // Execute then clause if present
        if let Some(then_step) = &reflect.then_step {
            let _then_result = self.execute_step(then_step).await?;
        }

        Ok(QueryResult {
            success: true,
            data: ResultSet::Reflect {
                sources: source_results,
                links: all_links,
            },
            metadata: crate::result::ResultMetadata {
                execution_time_ms: start.elapsed().as_millis() as u64,
                ..Default::default()
            },
        })
    }

    // =========================================================================
    // Individual operation implementations
    // =========================================================================

    async fn execute_scan(&self, step: &StepPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        let records = if let Some(window) = &step.modifiers.window {
            self.adb.scan_window(window).await?
        } else {
            self.adb.scan().await?
        };

        // Apply limit if present
        let records = if let Some(limit) = step.modifiers.limit {
            records.into_iter().take(limit).collect()
        } else {
            records
        };

        Ok(QueryResult::records(
            records,
            start.elapsed().as_millis() as u64,
        ))
    }

    async fn execute_recall(&self, step: &StepPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        // For aggregation, we need records WITHOUT field projection applied
        // So we recall with predicate and filters, but apply RETURN later
        let has_aggregate = step.modifiers.aggregate.is_some();

        let records = if has_aggregate {
            // Create modifiers without return_fields to preserve all fields for aggregation
            let mut mods_for_recall = step.modifiers.clone();
            mods_for_recall.return_fields = None;
            self.adb
                .recall_with_modifiers(step.memory_type, &step.predicate, &mods_for_recall)
                .await?
        } else {
            self.adb
                .recall_with_modifiers(step.memory_type, &step.predicate, &step.modifiers)
                .await?
        };

        // Handle aggregation if present - works for all memory types
        if let Some(agg_funcs) = &step.modifiers.aggregate {
            let agg_result = self.adb.episodic().aggregate(records, agg_funcs).await?;

            // Apply HAVING filter if present
            if let Some(having_conditions) = &step.modifiers.having {
                // Check if aggregation results match HAVING conditions
                let matches_having = having_conditions.iter().all(|cond| {
                    cond.matches(&agg_result)
                });

                // If HAVING fails, return the aggregation with a flag or empty
                // Based on SQL semantics, HAVING filters groups - if condition fails,
                // the entire group is excluded, returning no data
                if !matches_having {
                    return Ok(QueryResult::aggregation(
                        serde_json::json!(null),
                        start.elapsed().as_millis() as u64,
                    ));
                }
            }

            return Ok(QueryResult::aggregation(
                agg_result,
                start.elapsed().as_millis() as u64,
            ));
        }

        // Handle FOLLOW LINKS - traverse links and return target records
        if let Some(follow_links) = &step.modifiers.follow_links {
            let mut target_records = Vec::new();

            for record in &records {
                // Get links from this record
                let links = self
                    .adb
                    .get_links_from(
                        record.memory_type,
                        &record.id,
                        Some(&follow_links.link_type),
                    )
                    .await?;

                // Fetch target records for each link
                for link in links {
                    let targets = self
                        .adb
                        .lookup(link.to_type, &adb_core::Predicate::Key {
                            field: "id".to_string(),
                            value: adb_core::Value::String(link.to_id.clone()),
                        })
                        .await?;

                    target_records.extend(targets);
                }
            }

            // Apply field projection if RETURN clause specified
            let final_records = if let Some(ref return_fields) = step.modifiers.return_fields {
                target_records
                    .into_iter()
                    .map(|mut r| {
                        r.data = r.project_fields(return_fields);
                        r
                    })
                    .collect()
            } else {
                target_records
            };

            return Ok(QueryResult::records(
                final_records,
                start.elapsed().as_millis() as u64,
            ));
        }

        // Handle WITH LINKS - add links array to each record
        if let Some(with_links) = &step.modifiers.with_links {
            let mut records_with_links = Vec::new();

            for mut record in records {
                // Get links for this record
                let link_type_filter = match with_links {
                    adb_core::WithLinks::All => None,
                    adb_core::WithLinks::Type { link_type } => Some(link_type.as_str()),
                };

                let links = self
                    .adb
                    .get_links_from(record.memory_type, &record.id, link_type_filter)
                    .await?;

                // Add links array to record data
                if let Some(obj) = record.data.as_object_mut() {
                    let links_json: Vec<serde_json::Value> = links
                        .iter()
                        .map(|l| {
                            serde_json::json!({
                                "link_type": l.link_type,
                                "to_type": format!("{:?}", l.to_type),
                                "to_id": l.to_id,
                                "weight": l.weight
                            })
                        })
                        .collect();
                    obj.insert("links".to_string(), serde_json::Value::Array(links_json));
                }

                records_with_links.push(record);
            }

            return Ok(QueryResult::records(
                records_with_links,
                start.elapsed().as_millis() as u64,
            ));
        }

        Ok(QueryResult::records(
            records,
            start.elapsed().as_millis() as u64,
        ))
    }

    async fn execute_lookup(&self, step: &StepPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        let records = self
            .adb
            .lookup_with_modifiers(step.memory_type, &step.predicate, &step.modifiers)
            .await?;

        // Handle WITH LINKS - add links array to each record
        if let Some(with_links) = &step.modifiers.with_links {
            let mut records_with_links = Vec::new();

            for mut record in records {
                let link_type_filter = match with_links {
                    adb_core::WithLinks::All => None,
                    adb_core::WithLinks::Type { link_type } => Some(link_type.as_str()),
                };

                let links = self
                    .adb
                    .get_links_from(record.memory_type, &record.id, link_type_filter)
                    .await?;

                if let Some(obj) = record.data.as_object_mut() {
                    let links_json: Vec<serde_json::Value> = links
                        .iter()
                        .map(|l| {
                            serde_json::json!({
                                "link_type": l.link_type,
                                "to_type": format!("{:?}", l.to_type),
                                "to_id": l.to_id,
                                "weight": l.weight
                            })
                        })
                        .collect();
                    obj.insert("links".to_string(), serde_json::Value::Array(links_json));
                }

                records_with_links.push(record);
            }

            return Ok(QueryResult::records(
                records_with_links,
                start.elapsed().as_millis() as u64,
            ));
        }

        Ok(QueryResult::records(
            records,
            start.elapsed().as_millis() as u64,
        ))
    }

    async fn execute_load(&self, step: &StepPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        let limit = step.modifiers.limit.unwrap_or(10);
        let records = self.adb.load(&step.predicate, limit).await?;

        Ok(QueryResult::records(
            records,
            start.elapsed().as_millis() as u64,
        ))
    }

    async fn execute_store(&self, step: &StepPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        let data = step
            .data
            .as_ref()
            .ok_or_else(|| ExecutorError::MissingData("store data".to_string()))?;

        // Extract store data
        let key = data
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutorError::MissingData("key".to_string()))?;

        let payload = data
            .get("payload")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        let scope = data
            .get("scope")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "Private" => Some(Scope::Private),
                "Shared" => Some(Scope::Shared),
                "Cluster" => Some(Scope::Cluster),
                _ => None,
            })
            .unwrap_or(Scope::Private);

        let namespace = data.get("namespace").and_then(|v| v.as_str());

        let ttl = data
            .get("ttl_ms")
            .and_then(|v| v.as_u64())
            .map(std::time::Duration::from_millis);

        let record = self
            .adb
            .store_with_options(step.memory_type, key, payload, scope, namespace, ttl)
            .await?;

        Ok(QueryResult::stored(
            record,
            start.elapsed().as_millis() as u64,
        ))
    }

    async fn execute_update(&self, step: &StepPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        let data = step
            .data
            .as_ref()
            .ok_or_else(|| ExecutorError::MissingData("update data".to_string()))?
            .clone();

        let count = self
            .adb
            .update(step.memory_type, &step.predicate, data)
            .await?;

        Ok(QueryResult::count(count, start.elapsed().as_millis() as u64))
    }

    async fn execute_forget(&self, step: &StepPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        let count = self.adb.forget(step.memory_type, &step.predicate).await?;

        Ok(QueryResult::count(count, start.elapsed().as_millis() as u64))
    }

    async fn execute_link(&self, step: &StepPlan) -> ExecutorResult<QueryResult> {
        let start = Instant::now();

        // Extract link data from step.data
        let data = step
            .data
            .as_ref()
            .ok_or_else(|| ExecutorError::MissingData("link data".to_string()))?;

        // Get link metadata
        let link_type = data
            .get("link_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutorError::MissingData("link_type".to_string()))?;

        let weight = data
            .get("weight")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32;

        // Parse target memory type
        let to_type_str = data
            .get("to_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutorError::MissingData("to_type".to_string()))?;

        let to_type = match to_type_str {
            "Working" => MemoryType::Working,
            "Tools" => MemoryType::Tools,
            "Procedural" => MemoryType::Procedural,
            "Semantic" => MemoryType::Semantic,
            "Episodic" => MemoryType::Episodic,
            _ => return Err(ExecutorError::InvalidOperation(format!("Invalid target memory type: {}", to_type_str))),
        };

        // Build target predicate from to_conditions
        let to_predicate = self.build_predicate_from_conditions(data.get("to_conditions"))?;

        // Look up source records using step.memory_type and step.predicate
        let source_records = self
            .adb
            .lookup(step.memory_type, &step.predicate)
            .await?;

        if source_records.is_empty() {
            return Err(ExecutorError::LinkValidation(
                format!("No source records found in {:?} matching predicate", step.memory_type)
            ));
        }

        // Look up target records
        let target_records = self
            .adb
            .lookup(to_type, &to_predicate)
            .await?;

        if target_records.is_empty() {
            return Err(ExecutorError::LinkValidation(
                format!("No target records found in {:?} matching predicate", to_type)
            ));
        }

        // Create links between all matching source and target records
        let mut link_count = 0u64;
        for source in &source_records {
            for target in &target_records {
                self.adb
                    .link(step.memory_type, &source.id, to_type, &target.id, link_type, weight)
                    .await?;
                link_count += 1;
            }
        }

        Ok(QueryResult::count(link_count, start.elapsed().as_millis() as u64))
    }

    /// Build a predicate from serialized conditions array
    fn build_predicate_from_conditions(&self, conditions_value: Option<&serde_json::Value>) -> ExecutorResult<Predicate> {
        let conditions_arr = conditions_value
            .and_then(|v| v.as_array())
            .ok_or_else(|| ExecutorError::MissingData("to_conditions".to_string()))?;

        if conditions_arr.is_empty() {
            return Ok(Predicate::All);
        }

        let mut conditions = Vec::new();
        for cond_val in conditions_arr {
            let field = cond_val
                .get("field")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ExecutorError::MissingData("condition field".to_string()))?
                .to_string();

            let operator_str = cond_val
                .get("operator")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ExecutorError::MissingData("condition operator".to_string()))?;

            let operator = match operator_str {
                "Eq" => adb_core::Operator::Eq,
                "Ne" => adb_core::Operator::Ne,
                "Gt" => adb_core::Operator::Gt,
                "Gte" => adb_core::Operator::Gte,
                "Lt" => adb_core::Operator::Lt,
                "Lte" => adb_core::Operator::Lte,
                "Contains" => adb_core::Operator::Contains,
                "StartsWith" => adb_core::Operator::StartsWith,
                "EndsWith" => adb_core::Operator::EndsWith,
                "In" => adb_core::Operator::In,
                _ => return Err(ExecutorError::InvalidOperation(format!("Unknown operator: {}", operator_str))),
            };

            let value = cond_val
                .get("value")
                .map(|v| json_to_value(v))
                .unwrap_or(Value::Null);

            conditions.push(Condition::Simple {
                field,
                operator,
                value,
                logical_op: None,
            });
        }

        Ok(Predicate::Where { conditions })
    }
}

/// Convert a JSON value to a Value type
fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Value::Array(arr.iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(_) => {
            // Convert object to string representation
            Value::String(json.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_executor() -> Executor {
        let adb = Arc::new(Adb::new());
        Executor::new(adb)
    }

    #[tokio::test]
    async fn test_execute_scan() {
        let executor = create_executor();

        // Store some data first
        executor
            .adb
            .store(MemoryType::Working, "task-1", json!({"status": "active"}))
            .await
            .unwrap();

        // Execute scan
        let result = executor.execute("SCAN FROM WORKING").await.unwrap();
        assert!(result.success);

        if let ResultSet::Records { records } = result.data {
            assert_eq!(records.len(), 1);
        } else {
            panic!("Expected records result");
        }
    }

    #[tokio::test]
    async fn test_execute_recall() {
        let executor = create_executor();

        // Store some data
        executor
            .adb
            .store(
                MemoryType::Working,
                "task-1",
                json!({"status": "active", "priority": 5}),
            )
            .await
            .unwrap();
        executor
            .adb
            .store(
                MemoryType::Working,
                "task-2",
                json!({"status": "completed", "priority": 3}),
            )
            .await
            .unwrap();

        // Execute recall with filter
        let result = executor
            .execute(r#"RECALL FROM WORKING WHERE status = "active""#)
            .await
            .unwrap();

        assert!(result.success);
        if let ResultSet::Records { records } = result.data {
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].id, "task-1");
        } else {
            panic!("Expected records result");
        }
    }

    #[tokio::test]
    async fn test_execute_lookup() {
        let executor = create_executor();

        // Store data
        executor
            .adb
            .store(MemoryType::Working, "task-1", json!({"name": "Test Task"}))
            .await
            .unwrap();

        // Execute lookup by key
        let result = executor
            .execute(r#"LOOKUP FROM WORKING KEY id = "task-1""#)
            .await
            .unwrap();

        assert!(result.success);
        if let ResultSet::Records { records } = result.data {
            assert_eq!(records.len(), 1);
        } else {
            panic!("Expected records result");
        }
    }

    #[tokio::test]
    async fn test_execute_store() {
        let executor = create_executor();

        let result = executor
            .execute(r#"STORE INTO WORKING (key = "new-task", name = "New Task", priority = 1)"#)
            .await
            .unwrap();

        assert!(result.success);
        if let ResultSet::Stored { record } = result.data {
            assert_eq!(record.id, "new-task");
        } else {
            panic!("Expected stored result");
        }

        // Verify it was stored
        assert_eq!(executor.adb.count(MemoryType::Working).await, 1);
    }

    #[tokio::test]
    async fn test_execute_forget() {
        let executor = create_executor();

        // Store some data
        executor
            .adb
            .store(
                MemoryType::Working,
                "temp-1",
                json!({"temp": true}),
            )
            .await
            .unwrap();
        executor
            .adb
            .store(
                MemoryType::Working,
                "temp-2",
                json!({"temp": true}),
            )
            .await
            .unwrap();
        executor
            .adb
            .store(
                MemoryType::Working,
                "keep",
                json!({"temp": false}),
            )
            .await
            .unwrap();

        assert_eq!(executor.adb.count(MemoryType::Working).await, 3);

        // Execute forget
        let result = executor
            .execute(r#"FORGET FROM WORKING WHERE temp = true"#)
            .await
            .unwrap();

        assert!(result.success);
        if let ResultSet::Count { count } = result.data {
            assert_eq!(count, 2);
        } else {
            panic!("Expected count result");
        }

        assert_eq!(executor.adb.count(MemoryType::Working).await, 1);
    }

    #[tokio::test]
    async fn test_execute_pipeline() {
        let executor = create_executor();

        // Store some data
        executor
            .adb
            .store(
                MemoryType::Working,
                "task-1",
                json!({"status": "active"}),
            )
            .await
            .unwrap();

        // Execute pipeline
        let result = executor
            .execute(r#"PIPELINE test_pipe SCAN FROM WORKING | SCAN FROM WORKING LIMIT 1"#)
            .await
            .unwrap();

        assert!(result.success);
        if let ResultSet::Pipeline { steps } = result.data {
            assert_eq!(steps.len(), 2);
        } else {
            panic!("Expected pipeline result");
        }
    }
}
