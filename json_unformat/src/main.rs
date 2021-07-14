use regex::Regex;
use serde_json::Value;
use std::{env, fs::File, io::prelude::*};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let src = &args[1];
    let dest = &args[2];

    let mut file = File::open(src)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let mut json: Value = serde_json::from_str(&contents)?;

    // remove these regexp matches
    // let re = Regex::new(r"(\$([wnoitsgz\$]|[0123456789abcdefABCDEF]{3}|l\[[^\]]*\]))").unwrap();
    let re = Regex::new(r"(\$[0123456789abcdefABCDEF]{1,3})|(\$[ a-zA-Z])|(\$)\$").unwrap();

    if let Value::Array(array) = &mut json {
        println!("parsed {} elements.", array.len());

        for element in array {
            if let Value::Object(element) = element {
                if element.contains_key("name") {
                    let name_value = &mut element["name"];

                    if let Value::String(name) = name_value {
                        let name = re.replace_all(&name, "");
                        *name_value = Value::String(name.to_string());
                    }
                }
            }
        }
    }

    let mut file = File::create(dest)?;
    file.write_all(json.to_string().as_bytes())?;

    Ok(())
}
