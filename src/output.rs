// SPDX-License-Identifier: MIT OR Apache-2.0
use colored::Colorize;
use serde_json::{Value, json};

/// Print the response from an MCP tool call.
/// In JSON mode, prints raw JSON. Otherwise, prints colored terminal output.
pub fn print_response(content: &Value, is_error: bool, json_mode: bool) {
    print_response_with_hints(content, is_error, json_mode, &[], &[]);
}

/// Print response with lifecycle hints surfaced.
///
/// JSON mode: hints + auto_fired are merged into the top-level JSON
/// object as ``hints`` and ``auto_fired`` arrays (alongside the tool's
/// own content), so ``jq '.hints'`` / ``jq '.auto_fired'`` work as the
/// public docs promise. If ``content`` is not a JSON object, the JSON
/// output wraps it under ``content``.
///
/// Terminal mode: hints render as a dim note after the response if
/// any are present. Auto-fired tools are surfaced only when present.
pub fn print_response_with_hints(
    content: &Value,
    is_error: bool,
    json_mode: bool,
    hints: &[String],
    auto_fired: &[String],
) {
    if json_mode {
        let mut envelope = match content {
            Value::Object(_) => content.clone(),
            other => json!({ "content": other }),
        };
        if let Value::Object(ref mut map) = envelope {
            if !hints.is_empty() {
                map.insert("hints".into(), json!(hints));
            }
            if !auto_fired.is_empty() {
                map.insert("auto_fired".into(), json!(auto_fired));
            }
        }
        println!("{}", serde_json::to_string_pretty(&envelope).unwrap_or_default());
        return;
    }

    if is_error {
        print_error_content(content);
        return;
    }

    print_value(content, 0);

    if !hints.is_empty() {
        eprintln!("{} {}", "hint:".dimmed(), hints.join(", ").dimmed());
    }
    if !auto_fired.is_empty() {
        eprintln!("{} {}", "auto-fired:".dimmed(), auto_fired.join(", ").dimmed());
    }
}

/// Print an error response with red formatting
pub fn print_error(msg: &str) {
    eprintln!("{} {}", "error:".red().bold(), msg);
}

fn print_error_content(content: &Value) {
    match content {
        Value::String(s) => eprintln!("{} {}", "error:".red().bold(), s),
        Value::Object(map) => {
            if let Some(error) = map.get("error").and_then(|e| e.as_str()) {
                eprintln!("{} {}", "error:".red().bold(), error);
            } else {
                eprintln!(
                    "{} {}",
                    "error:".red().bold(),
                    serde_json::to_string_pretty(content).unwrap_or_default()
                );
            }
        }
        _ => eprintln!(
            "{} {}",
            "error:".red().bold(),
            serde_json::to_string_pretty(content).unwrap_or_default()
        ),
    }
}

/// Recursively print a JSON value with colored formatting
fn print_value(value: &Value, depth: usize) {
    match value {
        Value::String(s) => {
            // Strings render as-is (preserves embedded newlines).
            println!("{}", s);
        }
        Value::Object(map) => {
            for (key, val) in map {
                let indent = "  ".repeat(depth);
                match val {
                    Value::String(s) => {
                        if s.contains('\n') {
                            println!("{}{}", indent, format_key(key));
                            for line in s.lines() {
                                println!("{}  {}", indent, line);
                            }
                        } else {
                            println!("{}{} {}", indent, format_key(key), s);
                        }
                    }
                    Value::Number(n) => {
                        println!("{}{} {}", indent, format_key(key), n.to_string().cyan());
                    }
                    Value::Bool(b) => {
                        let colored = if *b {
                            "true".green().to_string()
                        } else {
                            "false".red().to_string()
                        };
                        println!("{}{} {}", indent, format_key(key), colored);
                    }
                    Value::Null => {
                        println!("{}{} {}", indent, format_key(key), "null".dimmed());
                    }
                    Value::Array(arr) => {
                        println!("{}{}", indent, format_key(key));
                        for item in arr {
                            print_array_item(item, depth + 1);
                        }
                    }
                    Value::Object(_) => {
                        println!("{}{}", indent, format_key(key));
                        print_value(val, depth + 1);
                    }
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                print_array_item(item, depth);
            }
        }
        Value::Number(n) => println!("{}", n.to_string().cyan()),
        Value::Bool(b) => println!("{}", if *b { "true".green() } else { "false".red() }),
        Value::Null => println!("{}", "null".dimmed()),
    }
}

fn print_array_item(item: &Value, depth: usize) {
    let indent = "  ".repeat(depth);
    match item {
        Value::String(s) => println!("{}- {}", indent, s),
        Value::Object(_) => {
            println!("{}---", indent);
            print_value(item, depth + 1);
        }
        _ => println!("{}- {}", indent, item),
    }
}

fn format_key(key: &str) -> String {
    format!("{}:", key.bold())
}
