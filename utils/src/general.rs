use serde_json::Value;

pub fn merge_json_dicts(dict1: &mut Value, dict2: &Value) {
    if let Value::Object(map1) = dict1 {
        if let Value::Object(map2) = dict2 {
            for (key, value) in map2 {
                map1.insert(key.clone(), value.clone());
            }
        }
    }
}
