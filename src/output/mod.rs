pub mod human;

use serde::Serialize;

pub trait HumanOutput {
    fn format_human(&self) -> String;
}

pub fn print_output<T: Serialize + HumanOutput>(value: &T, json_mode: bool) {
    if json_mode {
        println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
    } else {
        print!("{}", value.format_human());
    }
}
