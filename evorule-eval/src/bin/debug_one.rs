// SPDX-License-Identifier: AGPL-3.0-or-later
// Debug: 单用例诊断 execute_transition 返回

use evorule_tcb::{execute_transition, JsonValue};

fn main() {
    let rules = evorule_eval::load_rules_file(std::path::Path::new(
        r"D:\evorule-console-cloud\static\rules\20_finance_rules.json",
    ))
    .unwrap_or_else(|e| {
        eprintln!("load error: {e}");
        std::process::exit(1);
    });
    println!("rules loaded: {} rules", rules.len());
    println!(
        "rule[0] type = {:?}",
        rules.first().map(|r| {
            r.as_object()
                .and_then(|o| o.get("type"))
                .and_then(|t| t.as_str())
                .map(String::from)
        })
    );

    let instruction = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("finance_expense_limit_check")),
        (
            "params",
            JsonValue::object_from_pairs(&[(
                "expense",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("travel")),
                    ("amount", JsonValue::Integer(5000)),
                ]),
            )]),
        ),
    ]);
    let payload = JsonValue::empty_object();
    let result = execute_transition(&rules, &instruction, &payload, &[]);
    match result {
        Ok(r) => println!("result: {r:?}"),
        Err(e) => println!("error: {e}"),
    }
}
