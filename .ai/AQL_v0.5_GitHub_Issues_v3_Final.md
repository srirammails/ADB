# AQL v0.5 — ADB Implementation Bug Report (v3 — Final)
> Generated from 154 tests across all spec-defined statement types, modifiers, and validation rules.
> Spec: https://github.com/srirammails/AQL/blob/main/spec/AQL_SPEC_v0.5.md

---

## Summary

| Priority | Count |
|----------|-------|
| 🔴 P0 — Agent learning loop blocked | 14 |
| 🟠 P1 — Multi-agent, context, correctness | 9 |
| 🟡 P2 — Grammar / spec polish | 6 |
| **Total** | **29** |

**Tests run:** 154 | **Passing:** 113 | **Bugs:** 29 | **Observations:** 12

---

## Memory Type Schema Reference

### PROCEDURAL — Writable fields

| Field | STORE | UPDATE | Notes |
|-------|-------|--------|-------|
| `pattern` | ✅ | ✅ | **Use as human-readable key** (workaround for B4) |
| `severity` | ✅ | ✅ | `"info"`, `"high"`, `"critical"`, `"preventive"` |
| `source` | ✅ | ✅ | Free string |
| `confidence` | ✅ | ✅ | Float |
| `pattern_id` | ❌ UUID | N/A | Always overwritten with UUID — B4 |
| `steps` | ❌ `[]` | ❌ | Write-blocked — B24 |
| `success_count` | ❌ `0` | ❌ | Write-blocked — B25 |
| `failure_count` | ❌ `0` | ❌ | Write-blocked — B25 |
| `variables` | ❌ `{}` | ❌ | Write-blocked — B21 |
| `metadata` | ❌ `null` | ❌ | Write-blocked |

**Workaround for B4:** Use `pattern` as human-readable key. Query with `WHERE pattern = "my_pattern_name"`. FORGET works via `WHERE confidence < threshold` for threshold-based pruning.

### TOOLS — Writable fields

| Field | STORE | UPDATE | Notes |
|-------|-------|--------|-------|
| `ranking` | ✅ | ✅ | Float |
| `schema` | ✅ | ✅ | String |
| `name` | ❌ UUID | ✅ | Writable via UPDATE only |
| `category` | ❌ `"general"` | ✅ | Writable via UPDATE only |
| `description` | ❌ `""` | ✅ | Writable via UPDATE only |
| `tool_id` | ❌ UUID | ❌ | Always UUID — B5 |
| `task` | ❌ dropped | ❌ | B5 |
| `ad_format` | ❌ dropped | ❌ | B5 |
| `call_count` | ❌ `0` | ❌ | Write-blocked |

**Workaround for B5:** `STORE INTO TOOLS (ranking=..., schema=...)` then immediately `UPDATE INTO TOOLS WHERE tool_id = "{uuid}" (name=..., category=..., description=...)`. Human-readable fields then queryable via `WHERE name = "..."` or `WHERE category = "..."`.

### AGGREGATE support by memory type

| Memory Type | COUNT | SUM | AVG | MIN | MAX |
|-------------|-------|-----|-----|-----|-----|
| EPISODIC | ✅ | ❌ `-0.0` | ❌ null | ❌ null | ❌ null |
| SEMANTIC | ❌ raw records | ❌ | ❌ | ❌ | ❌ |
| PROCEDURAL | ❌ raw records | ❌ | ❌ | ❌ | ❌ |
| WORKING | ❌ raw records | ❌ | ❌ | ❌ | ❌ |
| TOOLS | ❌ raw records | ❌ | ❌ | ❌ | ❌ |

---

## 🔴 P0 — Critical (14 bugs)

---

### [BUG-01] `RECALL FROM ALL` not implemented

**Spec:** §5 — *"ALL is a valid memory target for reads"*

```aql
RECALL FROM ALL WHERE bid_id = "br-test-001" RETURN bid_id, campaign
-- Planning error: Invalid memory type: ALL cannot be used as target memory type
```

**Expected:** Fan-out RECALL across all 5 memory types, merged result set.
**Root cause:** ALL routing absent from query planner. Same root as BUG-02 and BUG-15.

---

### [BUG-02] `REFLECT FROM ALL` not implemented

**Spec:** §11

```aql
REFLECT FROM ALL WHERE campaign = "summer_2026"
-- Planning error: Invalid memory type: ALL cannot be used as target memory type
```

---

### [BUG-03] `AGGREGATE SUM` returns `-0.0`; `AVG/MIN/MAX` return `null`

**Spec:** §13 — all five agg ops required

```aql
RECALL FROM EPISODIC WHERE campaign = "summer_2026"
  AGGREGATE COUNT(*) AS n, SUM(cpm_paid) AS spend, AVG(ctr) AS avg_ctr
-- { count: 6, spend: -0.0, avg_ctr: null }
```

**Root cause (confirmed via T148):** SUM accumulator initialised to IEEE 754 `-0.0` (Rust `f64` uninitialised state) — field values never fed into it. AVG/MIN/MAX accumulators never initialised at all. Fix: single field extraction pass feeding all accumulators after record retrieval.

---

### [BUG-04] PROCEDURAL `pattern_id` overwritten with UUID; open fields dropped on STORE

**Spec:** §7 — open `field_value_list` payload

```aql
STORE INTO PROCEDURAL (pattern_id = "tech_news_premium", action = "bid + 22%", success_count = 3)
-- { pattern_id: "uuid-...", action: null, success_count: 0 }
```

**Workaround:** See schema table above — use `pattern` as key, `severity`/`source`/`confidence` writable.

---

### [BUG-05] TOOLS `tool_id`, `task`, `ad_format` overwritten/dropped on STORE

**Spec:** §7, §4

```aql
STORE INTO TOOLS (tool_id = "bidder_v1", task = "bidding", ad_format = "display", ranking = 0.9)
-- { tool_id: "uuid-...", task: null, ad_format: null }
```

**Workaround:** See schema table above.

---

### [BUG-06] `FOLLOW LINKS TYPE` returns source record, not target memory type records

**Spec:** §10 — *"result set contains records from the TARGET memory type"*

```aql
RECALL FROM SEMANTIC WHERE concept = "ai_enterprise_content"
  FOLLOW LINKS TYPE "triggers"
  RETURN pattern_id, confidence
-- Returns: SEMANTIC source record, not linked PROCEDURAL targets
```

---

### [BUG-15] `FORGET FROM ALL WHERE` not implemented

**Spec:** §2, §8

```aql
FORGET FROM ALL WHERE campaign = "ns_isolation_test"
-- Planning error: Invalid memory type: ALL cannot be used as target memory type
```

---

### [BUG-21] PROCEDURAL `variables` map write-blocked

**Spec:** §17 — *"UPDATE INTO PROCEDURAL ... tune procedural variables"*

```aql
UPDATE INTO PROCEDURAL WHERE pattern_id = "uuid-..." (variables = "bid_multiplier=1.18")
-- count: 1, but variables: {} unchanged
```

---

### [BUG-23] Integer literals don't coerce against stored float in ordered comparators

```aql
-- cpm_paid stored as 3.1 (float64)
RECALL FROM EPISODIC WHERE cpm_paid > 3    -- 0 records ❌
RECALL FROM EPISODIC WHERE cpm_paid >= 3   -- 0 records ❌
RECALL FROM EPISODIC WHERE cpm_paid < 3    -- 0 records ❌
RECALL FROM EPISODIC WHERE cpm_paid > 3.0  -- 1 record ✅
RECALL FROM EPISODIC WHERE cpm_paid != 3   -- all records ✅ (equality coerces)
```

**Confirmed:** Integer vs stored integer works fine in all memory types (T150).
**Workaround:** Always use float literals (`3.0` not `3`) for float fields.

---

### [BUG-24] PROCEDURAL `steps` field write-blocked (always `[]`)

```aql
STORE INTO PROCEDURAL (steps = "1. scale memory to 768Mi")
-- steps: [] in stored record
UPDATE INTO PROCEDURAL WHERE ... (steps = "updated steps")
-- count: 1, steps still []
```

---

### [BUG-25] PROCEDURAL `success_count` and `failure_count` write-blocked (always `0`)

```aql
STORE INTO PROCEDURAL (success_count = 10, failure_count = 2)
UPDATE INTO PROCEDURAL WHERE ... (success_count = 10)
-- Both: count/failure_count remain 0
```

---

### [BUG-27] AGGREGATE silently ignored on WORKING, SEMANTIC, PROCEDURAL, TOOLS

**Spec:** §13 — AGGREGATE valid on any read statement

**Confirmed across all 5 memory types (T153):**
- EPISODIC: ✅ Returns aggregation type
- SEMANTIC: ❌ Returns raw records with `data: {}` stripped
- PROCEDURAL: ❌ Returns raw records
- WORKING: ❌ Returns raw records with full data
- TOOLS: ❌ Returns raw records with full data

```aql
RECALL FROM WORKING WHERE bid_id != "x" AGGREGATE COUNT(*) AS total RETURN total
-- Returns raw records, not { count: N }
```

---

### [BUG-28] HAVING is a no-op — aggregation results never filtered

**Spec:** §13 — *"HAVING filters on aggregate results"*

```aql
-- count is 6, HAVING total > 999 should suppress result
RECALL FROM EPISODIC WHERE campaign = "summer_2026"
  AGGREGATE COUNT(*) AS total HAVING total > 999 RETURN total
-- { count: 6 } — HAVING not evaluated ❌

-- Same result regardless of HAVING value:
HAVING total > 0    -- { count: 6 }
HAVING total > 999  -- { count: 6 }
```

---

### [BUG-29] PIPELINE `{var}` inter-stage variable binding not implemented

**Spec:** §12 — *"Each stage's output is available to subsequent stages. Variables from earlier stages can be referenced using {identifier} syntax"*

```aql
PIPELINE var_test TIMEOUT 100ms
  RECALL FROM WORKING WHERE bid_id = "br-test-002" RETURN bid_id, campaign
  | RECALL FROM EPISODIC WHERE campaign = {campaign} RETURN bid_id, cpm_paid
-- Planning error: Variable '$campaign' is not bound
```

**Confirmed:** Variable error is a planning error (not parse error) — the `{var}` syntax is parsed but the binding mechanism from prior step output is absent.
**Impact:** The entire composable pipeline value — using output from step N as input to step N+1 — is non-functional. All pipeline stages currently run as independent parallel queries, not a chain.

---

## 🟠 P1 — Important (9 bugs)

---

### [BUG-07] `WITH LINKS` and REFLECT `links: []` — link index never surfaces in responses

**Spec:** §10 — *"WITH LINKS returns link metadata: link types, count, avg_weight"*

```aql
-- After creating 4+ LINK edges on a record:
LOOKUP FROM PROCEDURAL WHERE ... WITH LINKS ALL RETURN pattern_id, link_type, count
-- Returns base record only, no links[] field

REFLECT FROM EPISODIC WHERE ..., FROM SEMANTIC WHERE ...
-- links: [] always empty
```

**Confirmed (T145):** REFLECT `links: []` remains empty even after multiple LINK statements to/from both source records. The link index is written but never read into any response payload.

---

### [BUG-08] `REFLECT … THEN STORE INTO` parse error

**Spec:** §11 — `then_clause ::= "THEN" write_stmt`

```aql
REFLECT FROM EPISODIC WHERE campaign = "summer_2026"
  THEN STORE INTO SEMANTIC (concept = "insight", confidence = 0.75)
-- Parse error: Unexpected rule: store_stmt
```

---

### [BUG-09] `REFLECT` rejected as PIPELINE stage

**Spec:** §12 — `pipeline_stage ::= (read_stmt | reflect_stmt)`

```aql
PIPELINE full TIMEOUT 150ms
  SCAN FROM WORKING WINDOW LAST 5 RETURN bid_id
  | REFLECT FROM SEMANTIC WHERE concept = "x", FROM EPISODIC WHERE campaign = "y"
-- Planning error: Unsupported operation 'REFLECT in PIPELINE'
```

---

### [BUG-10] SCOPE on reads not enforced

```aql
STORE INTO SEMANTIC (concept = "private_concept") -- stored as scope: private
RECALL FROM SEMANTIC WHERE concept = "private_concept" SCOPE shared
-- Returns the record ❌ — scope not filtered
```

---

### [BUG-11] NAMESPACE on reads not enforced

```aql
STORE INTO EPISODIC (...) SCOPE shared NAMESPACE "agent-b-namespace"
RECALL FROM EPISODIC WHERE campaign = "ns_test" NAMESPACE "agent-a-namespace"
-- Returns the record ❌ — namespace not filtered
```

---

### [BUG-16] LINK on nonexistent records silently succeeds

```aql
LINK FROM EPISODIC WHERE bid_id = "nonexistent-999"
  TO EPISODIC WHERE bid_id = "real-001"
  TYPE "phantom" WEIGHT 0.5
-- { type: "empty", success: true } ❌
```

**Expected:** Error or `linked: 0`.

---

### [BUG-20] Dotted field paths fail in WHERE and RETURN

**Spec:** §15 — `field ::= identifier ("." identifier)?`

```aql
RECALL FROM EPISODIC WHERE metadata.namespace = "pubcontext-bidder"
-- Parse error: expected operator (at ".")

RECALL FROM EPISODIC WHERE bid_id != "x" RETURN data.bid_id
-- Parse error: expected EOI or modifier
```

---

### [BUG-22] UPDATE/TTL race condition — false `count: 1` on expiring record

```aql
STORE INTO WORKING (fresh = "true") TTL 10000ms
-- [TTL expires]
UPDATE INTO WORKING WHERE fresh = "true" (processed = true)
-- count: 1 (false positive)
RECALL FROM WORKING WHERE processed = true
-- 0 records
```

---

### [BUG-26] PROCEDURAL `version` doesn't increment on UPDATE

```aql
UPDATE INTO PROCEDURAL WHERE ... (confidence = 0.91)
-- count: 1, but version stays 1
```

SEMANTIC and EPISODIC both increment `version` correctly. PROCEDURAL does not.

---

## 🟡 P2 — Grammar / spec polish (6 bugs)

---

### [BUG-12] OR conditions completely unimplemented

**Spec:** §6 — `condition ::= condition "OR" condition | "(" condition ")"`

```aql
-- All variants fail:
WHERE campaign = "a" OR campaign = "b"
WHERE (campaign = "a" OR campaign = "b")
WHERE campaign = "a" AND (won = true OR won = false)
FORGET FROM EPISODIC WHERE campaign = "a" OR campaign = "b"
```

**Confirmed on:** RECALL, FORGET (all memory types). **Workaround:** Multiple sequential queries.

---

### [BUG-13] PIPELINE TIMEOUT not enforced at runtime

**Spec:** §12 — *"TIMEOUT is a hard constraint"*

`PIPELINE tight TIMEOUT 1ms ...` completes fully with no partial results. May be test-environment artefact — verify under load.

---

### [BUG-14] `LINK FROM ALL` error says "target" not "source"

```
Planning error: Invalid memory type: ALL cannot be used as **target** memory type
```
Should say: `ALL is not valid as LINK source.`

---

### [BUG-17] PIPELINE anonymous form rejected — `identifier?` should be optional

```aql
PIPELINE TIMEOUT 100ms SCAN FROM WORKING RETURN bid_id | ...
-- Parse error: expected pipeline_stage or timeout_mod
```

---

### [BUG-18] PIPELINE without TIMEOUT silently executes — should parse error

```aql
PIPELINE no_timeout SCAN FROM WORKING RETURN bid_id | ...
-- Executes successfully ❌
```

---

### [BUG-19] LOOKUP accepted on WORKING and EPISODIC

**Spec:** §5 — LOOKUP valid on PROCEDURAL, TOOLS, SEMANTIC only.

```aql
LOOKUP FROM WORKING KEY bid_id = "x"    -- succeeds ❌
LOOKUP FROM EPISODIC WHERE bid_id = "x" -- succeeds ❌
```

---

## Observations (Non-bugs)

| # | Observation |
|---|-------------|
| OBS-01 | `store_working(key="k")` sets `id: "k"`; `STORE INTO WORKING` always UUIDs. No `AS key` syntax in AQL. |
| OBS-02 | `accessed_at` updated inconsistently: native tool field-filter touches it; key-filter doesn't; SCAN doesn't; RECALL WHERE does. |
| OBS-03 | LINK WEIGHT >1.0 accepted silently — no [0.0,1.0] range validation. |
| OBS-04 | `event_type` on `recall_episodic` native tool is dead — no write path exists for this field. |
| OBS-05 | `STORE INTO WORKING` is always INSERT, never upsert. Duplicate field-value records created silently. Use `UPDATE INTO WORKING WHERE` for state mutation. |
| OBS-06 | `pattern` field is the correct human-readable key for PROCEDURAL records — not `pattern_id`. |
| OBS-07 | TOOLS `name`/`category`/`description` writable via UPDATE only, ignored on STORE. |
| OBS-08 | 6-stage PIPELINE confirmed — no stage count limit found. LOAD+LOOKUP+RECALL+RECALL+RECALL+SCAN all execute. |
| OBS-09 | `ORDER BY created_at DESC LIMIT N` is the correct pattern for "most recent N episodes". Metadata fields (`created_at`, `accessed_at`) are sortable. |
| OBS-10 | Integer comparators work correctly against stored integer fields in all memory types. B23 is strictly integer-literal vs stored float64. |
| OBS-11 | `ORDER BY` on non-existent field silently falls back to insertion order — no warning, consistent across all memory types. |
| OBS-12 | `LIMIT` applies **before** `AGGREGATE COUNT` — `LIMIT 2 AGGREGATE COUNT(*) AS n` returns `n: 2` not full dataset count. Omit LIMIT when counting total records. |

---

## Fix Roadmap

### Sprint 1 — Unblock agent learning loop (P0)

| Bug | Fix |
|-----|-----|
| B1, B2, B15 | Add ALL routing to query planner — fan-out across memory backends, merge results |
| B3 | Fix field extraction in aggregation engine — initialise accumulators from `0.0`, feed record field values |
| B27 | Extend aggregation dispatch to WORKING, SEMANTIC, PROCEDURAL, TOOLS (same fix path as B3) |
| B28 | Evaluate HAVING predicate against aggregation output values |
| B4 | PROCEDURAL: store all payload fields; preserve `pattern_id` string; remove UUID override |
| B24 | PROCEDURAL: unmarshal string/array into `steps` field |
| B25 | PROCEDURAL: unmarshal integer writes into `success_count`/`failure_count` |
| B21 | PROCEDURAL: unmarshal object/kv writes into `variables` map |
| B5 | TOOLS: preserve `tool_id`, `task`, `ad_format` on STORE |
| B6 | FOLLOW LINKS: traverse link edges, return target-type records not source |
| B23 | Ordered comparators: coerce integer literal to float when LHS field is float64 |
| B29 | PIPELINE variable binding: after each stage completes, bind RETURN fields from output into `{var}` resolution context for subsequent stages |

### Sprint 2 — Multi-agent, context, correctness (P1)

| Bug | Fix |
|-----|-----|
| B10 | Filter reads by `scope` value |
| B11 | Filter reads by `namespace` value |
| B7 | Include `links[]` array in WITH LINKS and REFLECT responses from link index |
| B8 | Parse and execute `THEN write_stmt` after REFLECT completes |
| B9 | Remove `REFLECT in PIPELINE` unsupported-operation guard |
| B16 | Return error or `linked: 0` when LINK source/target records not found |
| B20 | Implement `identifier.identifier` dotted path in WHERE and RETURN parsers |
| B22 | Return `count: 0` when UPDATE target has TTL-expired between match and write |
| B26 | Increment `version` on successful PROCEDURAL UPDATE |

### Sprint 3 — Grammar polish (P2)

| Bug | Fix |
|-----|-----|
| B12 | Implement `condition OR condition` and `(condition)` in parser |
| B17 | Make `identifier?` truly optional in PIPELINE grammar |
| B18 | Reject PIPELINE without TIMEOUT as parse error |
| B19 | Reject LOOKUP on WORKING and EPISODIC at planning layer |
| B14 | Fix LINK FROM ALL error message: "source" not "target" |
| B13 | Implement TIMEOUT budget allocation across PIPELINE stages |

---

## Test Coverage Matrix

| Statement | Tested | Passing | Issues |
|-----------|--------|---------|--------|
| SCAN | ✅ | WINDOW LAST/TOP/SINCE/duration | ALL predicate (B1 adjacent) |
| RECALL | ✅ | All memory types, all comparators | ALL target (B1), OR (B12), AGGREGATE non-EPISODIC (B27) |
| LOOKUP | ✅ | KEY, WHERE, PATTERN, WITH LINKS | WITH LINKS payload (B7), FOLLOW LINKS (B6) |
| LOAD | ✅ | TOOLS, ORDER BY, LIMIT | open fields (B5) |
| STORE | ✅ | All 5 types, all value types, TTL, SCOPE, NS | PROCEDURAL/TOOLS schema (B4/B5) |
| UPDATE | ✅ | All 5 types, bulk, additive fields | PROCEDURAL fixed fields (B21/B24/B25), version (B26), TTL race (B22) |
| FORGET | ✅ | All 5 types, compound AND, numeric, boolean | OR (B12), ALL target (B15) |
| LINK | ✅ | All directions, same-type, cross-type, multi-TYPE | phantom success (B16), WITH LINKS (B7) |
| REFLECT | ✅ | Multi-source, WORKING, empty results | THEN (B8), PIPELINE (B9), FROM ALL (B2), links[] (B7) |
| PIPELINE | ✅ | 2–6 stages, AGGREGATE, LOAD first, all types | {var} binding (B29), REFLECT stage (B9), anon form (B17), no-TIMEOUT (B18) |
| AGGREGATE | ✅ | COUNT (EPISODIC only), HAVING | AVG/SUM/MIN/MAX (B3), non-EPISODIC (B27), HAVING (B28) |
| LINK+WINDOW | ✅ | All duration units, LAST/TOP/SINCE | — |

---

*AQL v0.5 Test Report v3 (Final) — 154 tests, 29 bugs, 113 passing*
*8 sessions, ~4 hours of live ADB testing*
*2026-04-03 — github.com/srirammails/AQL*
