//! Panic-free contract tests for tier0-tcb v6.0.0.
//!
//! # Goal
//!
//! For every public API function, verify it **NEVER panics** on any input.
//! This is the **core TCB contract**: trusted code must handle arbitrary input gracefully.
//!
//! # Why this is beyond line coverage
//!
//! 100% line coverage does NOT prove panic-freedom. A function can hit every line
//! under normal inputs yet panic on edge cases (deeply nested structures, empty
//! input, unicode boundaries, integer overflow, etc.). These tests turn the
//! "trusted" promise into a testable invariant.
//!
//! # Strategy
//!
//! Each test:
//! 1. Constructs a matrix of adversarial inputs (empty, boundary, unicode, deep, etc.)
//! 2. Calls the public function with each input
//! 3. Wraps the call in `std::panic::catch_unwind`
//! 4. On panic, fails with a descriptive message identifying the input
//!
//! # What this does NOT verify
//!
//! - Correctness of results (covered by unit tests)
//! - Memory safety (covered by Miri / sanitizers; see docs/09)
//! - Performance (not in scope)
//! - Infinite loop / hang detection (not possible without timeout)

use std::any::Any;
use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};

use tier0_tcb::domain::evaluate_domain;
use tier0_tcb::executor::execute_meta_instruction;
use tier0_tcb::path::{resolve_path, resolve_path_mut};
use tier0_tcb::transition::execute_transition;
use tier0_tcb::JsonValue;

// ============================================================================
// Helpers
// ============================================================================

/// Run `f` and assert it does NOT panic. On panic, fail with the input description.
#[track_caller]
fn assert_no_panic<F: FnOnce()>(input_desc: &str, f: F) {
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    if let Err(payload) = result {
        let msg = payload_to_string(&payload);
        panic!(
            "[panic-free violation] function panicked on input `{}`: {}",
            input_desc, msg
        );
    }
}

/// Convert a `Box<dyn Any + Send>` panic payload to a String for diagnostics.
fn payload_to_string(payload: &Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<unknown panic payload>".to_string()
}

/// Short variant name for diagnostic messages.
fn describe_variant(v: &JsonValue) -> &'static str {
    match v {
        JsonValue::Null => "Null",
        JsonValue::Bool(_) => "Bool",
        JsonValue::Integer(_) => "Integer",
        JsonValue::String(_) => "String",
        JsonValue::Array(_) => "Array",
        JsonValue::Object(_) => "Object",
    }
}

/// All `JsonValue` variants, plus a few shape variants for coverage of state args.
fn all_jsonvalues() -> Vec<JsonValue> {
    vec![
        JsonValue::Null,
        JsonValue::Bool(true),
        JsonValue::Bool(false),
        JsonValue::Integer(0),
        JsonValue::Integer(1),
        JsonValue::Integer(-1),
        JsonValue::Integer(i64::MAX),
        JsonValue::Integer(i64::MIN),
        JsonValue::String(String::new()),
        JsonValue::String("ascii".into()),
        JsonValue::String("中文\0\n\r\t🔑".into()),
        JsonValue::array(vec![]),
        JsonValue::array(vec![JsonValue::Null, JsonValue::Integer(1)]),
        JsonValue::array((0..32).map(JsonValue::Integer).collect()),
        JsonValue::object(BTreeMap::new()),
    ]
}

/// Adversarial string inputs for paths, keys, and string values.
const ADVERSARIAL_STRINGS: &[&str] = &[
    "",                          // empty
    ".",                         // single dot
    "..",                        // double dot
    "...",                       // triple dot
    "a",                         // single char
    "a.b",                       // normal path
    "a.b.c.d.e.f",               // deep path
    ".a",                        // leading dot
    "a.",                        // trailing dot
    "a..b",                      // double dot mid
    "a...b",                     // triple dot mid
    "\\",                        // backslash
    "\\\\",                      // double backslash
    "\0",                        // NUL
    "\n",                        // newline
    "\r",                        // carriage return
    "\t",                        // tab
    "a\0b",                      // embedded NUL
    "a\nb",                      // embedded newline
    "a/b/c",                     // slashes
    "user.name@domain.com",      // email
    "key with spaces",           // spaces
    "\u{1F511}unicode\u{1F511}", // emoji
    "\u{200B}zero\u{200B}width", // zero-width space
    "a[0]",                      // bracket
    "a.0",                       // numeric segment
    "a[invalid]",                // malformed bracket
    "a.[0]",                     // dot before bracket
    "\u{4E2D}\u{6587}\u{952E}\u{540D}", // Chinese
    "\u{03A9}",                  // Greek
    "\u{1D54F}",                 // math alphanumeric
    "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_-=",  // 64 chars
    "a[",                          // unclosed bracket (parse_path_segments L201)
    "a[]",                         // empty brackets (parse_path_segments L205)
    "a[0",                         // unclosed bracket with idx_str="0" (L201)
];

// ============================================================================
// JsonValue: type predicates
// ============================================================================

#[test]
fn panic_free_type_predicates_on_all_variants() {
    for v in all_jsonvalues() {
        let desc = describe_variant(&v);
        assert_no_panic(&format!("is_object on {}", desc), || {
            let _ = v.is_object();
        });
        assert_no_panic(&format!("is_array on {}", desc), || {
            let _ = v.is_array();
        });
        assert_no_panic(&format!("is_integer on {}", desc), || {
            let _ = v.is_integer();
        });
        assert_no_panic(&format!("is_string on {}", desc), || {
            let _ = v.is_string();
        });
        assert_no_panic(&format!("is_bool on {}", desc), || {
            let _ = v.is_bool();
        });
        assert_no_panic(&format!("is_null on {}", desc), || {
            let _ = v.is_null();
        });
    }
}

// ============================================================================
// JsonValue: type extractors
// ============================================================================

#[test]
fn panic_free_type_extractors_on_all_variants() {
    for v in all_jsonvalues() {
        let desc = describe_variant(&v);
        assert_no_panic(&format!("as_i64 on {}", desc), || {
            let _ = v.as_i64();
        });
        assert_no_panic(&format!("as_str on {}", desc), || {
            let _ = v.as_str();
        });
        assert_no_panic(&format!("as_bool on {}", desc), || {
            let _ = v.as_bool();
        });
        assert_no_panic(&format!("as_array on {}", desc), || {
            let _ = v.as_array();
        });
        assert_no_panic(&format!("as_object on {}", desc), || {
            let _ = v.as_object();
        });
    }
}

#[test]
fn panic_free_mut_extractors_on_all_variants() {
    for mut v in all_jsonvalues() {
        let desc = describe_variant(&v);
        assert_no_panic(&format!("as_array_mut on {}", desc), || {
            let _ = v.as_array_mut();
        });
        assert_no_panic(&format!("as_object_mut on {}", desc), || {
            let _ = v.as_object_mut();
        });
    }
}

// ============================================================================
// JsonValue: object operations (get / get_mut / insert / remove)
// ============================================================================

#[test]
fn panic_free_object_get_with_adversarial_keys() {
    for v in all_jsonvalues() {
        for &key in ADVERSARIAL_STRINGS {
            assert_no_panic(
                &format!("get({:?}) on {}", key, describe_variant(&v)),
                || {
                    let _ = v.get(key);
                },
            );
        }
    }
}

#[test]
fn panic_free_object_get_mut_with_adversarial_keys() {
    for mut v in all_jsonvalues() {
        for &key in ADVERSARIAL_STRINGS {
            assert_no_panic(&format!("get_mut({:?})", key), || {
                let _ = v.get_mut(key);
            });
        }
    }
}

#[test]
fn panic_free_object_insert_with_adversarial_keys() {
    for mut v in all_jsonvalues() {
        for &key in ADVERSARIAL_STRINGS {
            let key_owned = key.to_string();
            assert_no_panic(&format!("insert({:?})", key), || {
                let _ = v.insert(key_owned, JsonValue::Null);
            });
        }
    }
}

#[test]
fn panic_free_object_remove_with_adversarial_keys() {
    for mut v in all_jsonvalues() {
        for &key in ADVERSARIAL_STRINGS {
            assert_no_panic(&format!("remove({:?})", key), || {
                let _ = v.remove(key);
            });
        }
    }
}

// ============================================================================
// JsonValue: constructors
// ============================================================================

#[test]
fn panic_free_string_constructor_with_adversarial_inputs() {
    for &s in ADVERSARIAL_STRINGS {
        assert_no_panic(&format!("string({:?})", s), || {
            let _ = JsonValue::string(s);
        });
        assert_no_panic(&format!("From<&str>({:?})", s), || {
            let _: JsonValue = s.into();
        });
        assert_no_panic(&format!("From<String>({:?})", s), || {
            let _: JsonValue = s.to_string().into();
        });
        assert_no_panic(&format!("JsonValue::String({:?})", s), || {
            let _ = JsonValue::String(s.to_string());
        });
    }
}

#[test]
fn panic_free_i64_constructor_with_boundary_values() {
    for &v in &[
        0i64,
        1,
        -1,
        i64::MAX,
        i64::MIN,
        i64::MAX - 1,
        i64::MIN + 1,
    ] {
        let desc = v.to_string();
        assert_no_panic(&format!("Integer({})", desc), || {
            let _ = JsonValue::Integer(v);
        });
        assert_no_panic(&format!("From<i64>({})", desc), || {
            let _: JsonValue = v.into();
        });
    }
}

#[test]
fn panic_free_bool_constructor() {
    for &b in &[true, false] {
        assert_no_panic(&format!("Bool({})", b), || {
            let _ = JsonValue::Bool(b);
        });
        assert_no_panic(&format!("From<bool>({})", b), || {
            let _: JsonValue = b.into();
        });
    }
}

#[test]
fn panic_free_array_constructor_with_edge_cases() {
    let cases: Vec<Vec<JsonValue>> = vec![
        vec![],
        vec![JsonValue::Null],
        vec![JsonValue::Null; 1],
        vec![JsonValue::Null; 1000],
        (0..256).map(JsonValue::Integer).collect(),
        // nested arrays
        vec![JsonValue::array(vec![]); 100],
        // mixed
        vec![
            JsonValue::Integer(1),
            JsonValue::String("two".into()),
            JsonValue::Bool(true),
            JsonValue::Null,
            JsonValue::array(vec![JsonValue::Integer(5)]),
            JsonValue::object(BTreeMap::new()),
        ],
    ];
    for (i, v) in cases.into_iter().enumerate() {
        assert_no_panic(&format!("array case {}", i), || {
            let _ = JsonValue::array(v.clone());
        });
        assert_no_panic(&format!("From<Vec<_>> case {}", i), || {
            let _: JsonValue = v.clone().into();
        });
    }
}

#[test]
fn panic_free_object_constructor_with_edge_cases() {
    let cases: Vec<BTreeMap<String, JsonValue>> = vec![
        BTreeMap::new(),
        {
            let mut m = BTreeMap::new();
            m.insert("k".to_string(), JsonValue::Null);
            m
        },
        {
            let mut m = BTreeMap::new();
            for &s in ADVERSARIAL_STRINGS {
                m.insert(s.to_string(), JsonValue::Integer(42));
            }
            m
        },
        {
            // 1000 keys
            let mut m = BTreeMap::new();
            for i in 0..1000 {
                m.insert(format!("k_{}", i), JsonValue::Integer(i));
            }
            m
        },
    ];
    for (i, m) in cases.into_iter().enumerate() {
        assert_no_panic(&format!("object case {}", i), || {
            let _ = JsonValue::object(m.clone());
        });
        assert_no_panic(&format!("From<BTreeMap<_>> case {}", i), || {
            let _: JsonValue = m.clone().into();
        });
    }
}

#[test]
fn panic_free_object_from_pairs_with_adversarial_keys() {
    for &key in ADVERSARIAL_STRINGS {
        let pairs: &[(&str, JsonValue)] = &[(key, JsonValue::Null)];
        assert_no_panic(&format!("object_from_pairs({:?})", key), || {
            let _ = JsonValue::object_from_pairs(pairs);
        });
    }
    // empty slice
    let empty: &[(&str, JsonValue)] = &[];
    assert_no_panic("object_from_pairs(empty)", || {
        let _ = JsonValue::object_from_pairs(empty);
    });
}

#[test]
fn panic_free_empty_constructors() {
    assert_no_panic("empty_object", || {
        let _ = JsonValue::empty_object();
    });
    assert_no_panic("empty_array", || {
        let _ = JsonValue::empty_array();
    });
}

// ============================================================================
// JsonValue: Display / Debug / Clone / Ord
// ============================================================================

#[test]
fn panic_free_display_all_variants() {
    for v in all_jsonvalues() {
        let desc = describe_variant(&v);
        assert_no_panic(&format!("Display on {}", desc), || {
            let _ = v.to_string();
        });
        assert_no_panic(&format!("Debug on {}", desc), || {
            let _ = format!("{:?}", v);
        });
        assert_no_panic(&format!("Clone on {}", desc), || {
            let _ = v.clone();
        });
        assert_no_panic(&format!("PartialEq on {}", desc), || {
            let _ = v == v;
        });
        assert_no_panic(&format!("PartialOrd on {}", desc), || {
            let _ = v.partial_cmp(&v);
        });
    }
}

// ============================================================================
// Path::resolve_path / resolve_path_mut
// ============================================================================

#[test]
fn panic_free_resolve_path_on_all_states_and_paths() {
    for v in all_jsonvalues() {
        for &path in ADVERSARIAL_STRINGS {
            assert_no_panic(
                &format!("resolve_path(state={}, path={:?})", describe_variant(&v), path),
                || {
                    let _ = resolve_path(&v, path);
                },
            );
        }
    }
}

#[test]
fn panic_free_resolve_path_mut_on_all_states_and_paths() {
    for mut v in all_jsonvalues() {
        for &path in ADVERSARIAL_STRINGS {
            assert_no_panic(
                &format!("resolve_path_mut(path={:?})", path),
                || {
                    let _ = resolve_path_mut(&mut v, path);
                },
            );
        }
    }
}

// ============================================================================
// Domain::evaluate_domain
// ============================================================================

#[test]
fn panic_free_evaluate_domain_arbitrary_inputs() {
    let domains = all_jsonvalues();
    let states = all_jsonvalues();
    for d in &domains {
        for s in &states {
            assert_no_panic(
                &format!(
                    "evaluate_domain(domain={}, state={})",
                    describe_variant(d),
                    describe_variant(s)
                ),
                || {
                    let _ = evaluate_domain(d, s);
                },
            );
        }
    }
}

// ============================================================================
// Transition::execute_transition
// ============================================================================

#[test]
fn panic_free_execute_transition_arbitrary_inputs() {
    let core_eval_cases: Vec<Vec<JsonValue>> = vec![
        vec![],
        all_jsonvalues(),
        (0..64).map(JsonValue::Integer).collect(),
    ];
    let instructions = all_jsonvalues();
    let payloads = all_jsonvalues();
    let queues: Vec<Vec<JsonValue>> = vec![vec![], all_jsonvalues(), (0..32).map(JsonValue::Integer).collect()];

    let mut case_count = 0u64;
    for core_eval in &core_eval_cases {
        for instr in &instructions {
            for payload in &payloads {
                for queue in &queues {
                    case_count += 1;
                    assert_no_panic(
                        &format!(
                            "execute_transition case #{} (core_eval.len={}, instr={}, payload={}, queue.len={})",
                            case_count,
                            core_eval.len(),
                            describe_variant(instr),
                            describe_variant(payload),
                            queue.len()
                        ),
                        || {
                            let _ = execute_transition(core_eval, instr, payload, queue);
                        },
                    );
                }
            }
        }
    }
}

// ============================================================================
// Executor::execute_meta_instruction
// ============================================================================

#[test]
fn panic_free_execute_meta_instruction_arbitrary_depths() {
    let depths: &[usize] = &[
        0,
        1,
        2,
        10,
        64,
        65, // just over the 64-deep branch limit
        100,
        1000,
        usize::MAX,
    ];
    let instrs = all_jsonvalues();
    let states = all_jsonvalues();

    for &depth in depths {
        for instr in &instrs {
            for state in &states {
                assert_no_panic(
                    &format!(
                        "execute_meta_instruction(depth={}, instr={}, state={})",
                        depth,
                        describe_variant(instr),
                        describe_variant(state)
                    ),
                    || {
                        let _ = execute_meta_instruction(instr, state.clone(), depth);
                    },
                );
            }
        }
    }
}

// ============================================================================
// TcbError: derived trait coverage (constructed indirectly)
// ============================================================================

#[test]
fn panic_free_tcb_error_traits_via_observed_errors() {
    // Trigger several TcbError variants via execute_transition / execute_meta_instruction
    // and verify Display/Debug/Clone/PartialEq all work without panicking.
    use std::string::ToString;

    let triggers: Vec<(&str, JsonValue, JsonValue)> = vec![
        ("instr=Null", JsonValue::Null, JsonValue::Null),
        ("instr=Integer", JsonValue::Integer(0), JsonValue::Null),
        (
            "instr=String",
            JsonValue::String("not_an_object".into()),
            JsonValue::Null,
        ),
        (
            "instr=Array",
            JsonValue::array(vec![]),
            JsonValue::Null,
        ),
        (
            "instr=Object empty",
            JsonValue::object(BTreeMap::new()),
            JsonValue::Null,
        ),
    ];

    for (_desc, instr, payload) in triggers {
        let res = execute_transition(&[], &instr, &payload, &[]);
        if let Ok(meta) = res {
            // Some instructions succeed → also exercise MetaInstructionResult
            let _ = format!("{:?}", meta);
        }
        // We don't assert Ok or Err — both are valid outcomes.
        // We only care that no panic happened.
    }

    // Direct verify: try to extract errors from meta instructions
    for &depth in &[0usize, 64, 100, usize::MAX] {
        for instr in all_jsonvalues() {
            let res = execute_meta_instruction(&instr, JsonValue::Null, depth);
            if let Err(e) = res {
                assert_no_panic("TcbError::Display", || {
                    let _ = e.to_string();
                });
                assert_no_panic("TcbError::Debug", || {
                    let _ = format!("{:?}", e);
                });
                assert_no_panic("TcbError::Clone", || {
                    let _ = e.clone();
                });
                assert_no_panic("TcbError::PartialEq", || {
                    let _ = e == e;
                });
            }
        }
    }
}

// ============================================================================
// Sanity: assert_no_panic itself doesn't panic on benign closures
// ============================================================================

#[test]
fn sanity_assert_no_panic_helper_works() {
    assert_no_panic("benign closure", || {
        let _ = 1 + 1;
    });
}
