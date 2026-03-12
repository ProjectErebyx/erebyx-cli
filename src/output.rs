use colored::Colorize;
use serde_json::Value;

/// Print the response from an MCP tool call.
/// In JSON mode, prints raw JSON. Otherwise, prints colored terminal output.
pub fn print_response(content: &Value, is_error: bool, json_mode: bool) {
    if json_mode {
        println!("{}", serde_json::to_string_pretty(content).unwrap_or_default());
        return;
    }

    if is_error {
        print_error_content(content);
        return;
    }

    print_value(content, 0);
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
            // Multi-line strings get printed as-is
            if s.contains('\n') {
                println!("{}", s);
            } else {
                println!("{}", s);
            }
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
