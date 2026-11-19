# EvoRule Rule Taxonomy

> **Version**: v1.0
> **Last updated**: 2026-08-18
> **Goal**: clearly distinguish rules at different layers and eliminate terminological confusion

---

## 1. Background

Multiple kinds of rules exist in the EvoRule ecosystem, but they are all loosely called "rules":

- transform rules in `core_eval.json`
- business rules in `rules/*.json`
- hardcoded rules in Rust code
- rules dynamically generated at runtime

This ambiguity causes problems:
1. At design time it is easy to confuse the responsibility boundaries of different rule types
2. At implementation time it is easy to mistakenly treat project-party rules as system rules
3. At maintenance time it is hard to understand a rule's source, purpose and modification permission
4. In detection logic it is impossible to correctly distinguish "the rule's structure defines instruction filtering" from "the rule actually processed the current instruction"

---

## 2. Rule Taxonomy

### 2.1 Four-layer rule architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Application layer (evorule-server)        │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Project-party rules (User Rules)                    │    │
│  │  - file: rules/*.json                               │    │
│  │  - hot-reloadable                                   │    │
│  │  - defines: how concrete business responds to instructions  │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
                              │ instruction + payload
                              ▼
┌─────────────────────────────────────────────────────────────┐
│              Kernel layer (evorule-tcb + evorule-reactor)    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Business Rules                                     │    │
│  │  - dynamically generated at runtime                 │    │
│  │  - source: project-party rules + instruction params + payload    │    │
│  │  - role: dynamic decision + data driven             │    │
│  └─────────────────────────────────────────────────────┘    │
│                              │                              │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Constitution Rules                                 │    │
│  │  - file: core_eval.json                             │    │
│  │  - not hot-reloadable (loaded at TCB startup)       │    │
│  │  - defines: basic instruction handling logic supported by the kernel │    │
│  │  - examples: increment, set, while_loop, etc.       │    │
│  └─────────────────────────────────────────────────────┘    │
│                              │                              │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  System Rules                                       │    │
│  │  - hardcoded in Rust code                           │    │
│  │  - not configurable                                 │    │
│  │  - defines: runtime constraints and detection logic of the TCB │    │
│  │  - examples: noop exemption, max_transform_rules, Ignored detection │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Detailed definition of each layer

#### Layer 1: System Rules

**Definition**:
- runtime constraints and detection logic hardcoded in TCB Rust code
- not configurable; changes with the TCB version
- not "data", but "behavioral constraints"

**Examples**:
```rust
// System rule 1: noop instruction exemption (does not report Ignored)
// detection logic in transition.rs

// System rule 2: max_transform_rules constraint (at most N rules per transform)
// validation logic in transition.rs

// System rule 3: Ignored detection logic
// has_matching_business_rule detection in transition.rs
```

**Modification permission**: only TCB core developers may modify

**Current code location**:
- `transition.rs` - constraint logic in the `execute_transition()` function
- `transition.rs` - `has_matching_business_rule` detection logic

---

#### Layer 2: Constitution Rules

**Definition**:
- JSON rules stored in `core_eval.json`
- loaded at TCB startup, not hot-reloadable at runtime
- defines all basic instruction handling logic supported by the kernel
- has system-level binding force; project parties must comply

**Currently defined instructions**:
| Instruction type | Handling logic | Description |
|---------|---------|------|
| `increment` | set(attr, "add", delta) | atomic increment |
| `decrement` | set(attr, "sub", delta) | atomic decrement |
| `set` | set(attr, "set", value) | overwrite assignment |
| `sequence` | push(instructions) | sequential execution |
| `conditional` | branch(domain, then, else) | conditional branch |
| `while_loop` | branch(condition, [body, self]) | loop execution |
| `call_external` | branch(exists, ...) / io_request | external call (ReAct) |
| `call_service` | branch(exists, ...) / io_request | service call (ReAct; as of v0.6.0 multi-round orchestration moved to the application layer) |
| `all([])` | noop (empty operation) | fallback rule |

**Modification permission**: TCB core developers may modify (when upgrading the version)

**Current file location**:
- `evorule-tcb/core_eval.json` - the `transform` array

---

#### Layer 3: Business Rules

**Definition**:
- rules dynamically generated at runtime
- composed from project-party rules + instruction parameters + payload
- dynamic decision-making, data-driven
- the TCB executes these rules and does not care about their origin

**Example**:
```json
// business rule dynamically generated at runtime (generated by the application layer and submitted to the TCB)
{
  "type": "branch",
  "params": {
    "domain": { "type": "instruction", "instruction_type": "set" },
    "on_true": [
      {
        "type": "set",
        "params": {
          "attr": "counter",
          "operation": "add",
          "value": 5
        }
      }
    ]
  }
}
```

**Modification permission**: generated by application-layer code; the project-party rules control its structure

**Currently involved code**:
- `evorule-reactor/src/reactor.rs` - receives business rules and submits them to the TCB
- application layer - logic that generates business rules

---

#### Layer 4: Project-party Rules (User Rules)

**Definition**:
- JSON rules stored in `rules/*.json`
- hot-reloadable, modifiable at runtime
- defines how concrete business responds to instructions
- aimed at business analysts, no programming knowledge required

**Example**:
```json
// rules/counter.json
{
  "rules": [
    {
      "when": { "type": "command", "instruction_type": "set" },
      "do": [
        {
          "type": "set",
          "operation": "set",
          "attr": "${params.attr}",
          "value": "${params.value}"
        }
      ]
    }
  ]
}
```

**Modification permission**: business analysts may modify

**Current file location**:
- `evorule-server/resources/rules/*.json`

---

## 3. Terminology conventions

### 3.1 Terminology mapping table

| Term | Usage context |
|------|---------|
| system rules | used when discussing TCB hardcoded behavior |
| constitution rules | used when discussing core_eval.json |
| business rules | used when discussing runtime dynamic rules |
| project-party rules (user rules) | used when discussing rules/*.json |
| instruction | used when discussing commands received by the TCB |
| transition | used when discussing execute_transition() |

### 3.2 Naming recommendations

#### Code variable naming

```rust
// ❌ unclear naming
let rule = ...;           // which layer's rule?
let business_rule = ...;  // inaccurate, could be a constitution rule

// ✅ clear naming
let const_rule = ...;     // constitution rule
let sys_rule = ...;       // system rule (but usually not present in rule variables)
let runtime_rule = ...;   // runtime business rule
let user_rule = ...;      // project-party rule
```

#### Function naming

```rust
// ❌ unclear naming
fn has_matching_business_rule(...) -> bool  // actually checks constitution rules

// ✅ clear naming
fn has_matching_constitution_rule(...) -> bool  // explicitly checks constitution rules
fn has_side_effect(...) -> bool  // checks whether there is an actual side effect
```

### 3.3 Documentation naming

Explicitly identify the rule layer in docs and comments:

```markdown
## Rule taxonomy

### Constitution Rules
- file: core_eval.json
- description: defines the basic instruction handling supported by the kernel

### Business Rules
- source: dynamically generated at runtime
- description: composed from project-party rules + instruction params + payload
```

---

## 4. Problem diagnosis and solution

### 4.1 Current problems

#### Problem 1: `has_matching_business_rule` naming is inaccurate

```rust
// transition.rs#L168-L192
let has_matching_business_rule = core_eval.iter().any(|rule| {
    // what is checked here is the rules in core_eval = constitution rules
    // not "business rules"
    ...
});
```

**Problem**: the variable is named `business_rule`, but what is actually checked is constitution rules. This leads to:
- Scenario B4: a dynamic domain rule (lt) is treated as a "business rule" even though it does not match the current instruction
- Scenario B5: a direct rule (set) is treated as a "business rule", without distinguishing instruction type

#### Problem 2: detection logic conflates two dimensions

The current detection logic mixes two independent questions:
1. **Does the rule match the current instruction type** → check the constitution rule's domain
2. **Does execution have an actual side effect** → check whether payload/queue changed

These two dimensions should be judged independently:
- rule matches + no side effect → legal no-op (e.g. while_loop condition=false)
- rule does not match + has side effect → cannot happen
- rule does not match + no side effect → report Ignored

### 4.2 Solution

#### Solution 1: rename + split detection logic (implemented ✅)

```rust
// Step 1: check whether a constitution rule matching the current instruction exists
let has_matching_constitution_rule = core_eval.iter().any(|rule| {
    match domain_type {
        "instruction" => {
            // exact match on instruction_type
            return expected_type == instruction_type;
        }
        "all" => false, // the all fallback is not a business rule
        _ => {
            // dynamic domain (eq, lt, gt, ge, not, exists, has_fields, etc.)
            // treated as a general rule, can handle any instruction
            return true;
        }
    }
    // direct rule (e.g. set, increment), check whether rule_type matches instruction_type
    if matches!(rule_type, "set" | "increment" | ...) {
        return rule_type == instruction_type;
    }
});

// Step 2: check side effects after executing the transition
let has_side_effect = new_payload != payload || new_queue != queue;

// Step 3: combined judgement
if !has_matching_constitution_rule && !has_side_effect {
    // rule does not match and no side effect → report Ignored
    return Ok(TransitionResult::Ignored { ... });
}

// all other cases return State (including legal no-ops where the rule matches but has no side effect)
Ok(TransitionResult::State { new_payload, new_queue })
```

#### Solution 2: analysis of resolved problems

| Scenario | Rule type | Old judgement | New judgement | Description |
|------|---------|--------|--------|------|
| B4: lt rule + while_loop instruction | dynamic domain | ✅ matches | ✅ matches | dynamic domain treated as a general rule |
| B5: set rule + while_loop instruction | direct rule | ✅ matches | ❌ does not match | direct rule checks rule_type |
| B5b: set rule + set instruction | direct rule | ✅ matches | ✅ matches | direct rule exact match |
| E2: rule matches + on_true empty | constitution rule | ❌ may misjudge | ✅ returns State | separated side-effect detection |

**Resolved problems**:
1. ✅ variable renamed from `has_matching_business_rule` to `has_matching_constitution_rule`
2. ✅ dynamic domain rules treated as general rules (can handle any instruction)
3. ✅ direct rules check `rule_type == instruction_type`
4. ✅ separated the two judgements "rule match" and "side-effect detection"

---

## 5. Follow-up implementation plan

### Phase 1: documentation conventions (completed ✅)
- [x] create the rule taxonomy document `rule_taxonomy.md`
- [x] clarify the four-layer classification: system rules, constitution rules, business rules, project-party rules
- [x] define terminology conventions and naming recommendations

### Phase 2: code refactor (completed ✅)
- [x] rename `has_matching_business_rule` → `has_matching_constitution_rule`
- [x] improve dynamic domain rules: treat as general rules, can handle any instruction
- [x] improve direct rules: check `rule_type == instruction_type`
- [x] separate "rule match" and "side-effect detection" logic
- [x] add the `has_side_effect` variable

### Phase 3: test updates (completed ✅)
- [x] update B4 scenario tests: verify dynamic domain rules as general rules
- [x] update B5 scenario tests: verify exact matching of direct rules
- [x] add `test_set_direct_rule_matches_set_instruction` test
- [x] all tests pass (220 unit tests + 5 proptest + 21 integration tests)

### Phase 4: documentation refinement (in progress)
- [ ] explicitly mark "constitution rules" in core_eval.json
- [x] explicitly identify rule layers in code comments
- [x] create the terminology mapping table (section 3 of this file)

### Phase 5: code audit (optional)
- [ ] audit the naming of the `StepOutcome` enum in evorule-reactor
- [ ] audit the naming conventions of business rules in evorule-server
- [ ] add code documentation comments that explicitly identify rule layers

---

## 6. Related files

### Core definitions
- [README.md](../../README.md) - key project concept definitions
- [core_eval.json](../core_eval.json) - constitution rule definitions

### Code implementation
- [transition.rs](../src/transition.rs) - TCB transition logic
- [pure.rs](../../evorule-reactor/src/pure.rs) - Reactor pure functions
- [reactor.rs](../../evorule-reactor/src/reactor.rs) - Reactor main loop

### Design specs
- [TCB_SPEC.md](../TCB_SPEC.md) - TCB system spec
- [CLI_SPEC.md](../../evorule-cli/CLI_SPEC.md) - CLI design spec

---

## 7. Summary

The current evorule rule system has a classification ambiguity problem, leading to:
1. confusion over rule responsibility boundaries at design time
2. misjudgement of detection logic at implementation time (B4/B5 scenarios)
3. difficulty understanding rule sources and modification permissions at maintenance time

Through this taxonomy, the following are made explicit:
1. **System rules**: hardcoded behavior, not configurable
2. **Constitution rules**: basic instruction handling defined by core_eval.json
3. **Business rules**: rules dynamically generated at runtime
4. **Project-party rules**: business responses defined by rules/*.json

It is recommended to implement in the order "documentation conventions → code refactor → test updates" to make rule classification clear.
