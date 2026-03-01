#[cfg(test)]
mod tests {
    use crate::ModuleResp;
    use std::fs;

    const SCHEMA_TEST_DIR: &str = "schema_test/module_resp";

    #[test]
    fn schema_evolution_module_resp() {
        let dir = fs::read_dir(SCHEMA_TEST_DIR).unwrap_or_else(|e| {
            panic!("failed to read schema test dir {}: {}", SCHEMA_TEST_DIR, e)
        });
        let mut entries: Vec<_> = dir
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        entries.sort_by_key(|p| p.file_name().unwrap().to_owned());

        if entries.is_empty() {
            panic!("no files in {}", SCHEMA_TEST_DIR);
        }

        let mut last_contents = None;
        for path in &entries {
            let name = path.file_name().unwrap().to_string_lossy();
            let contents = fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("failed to read {}: {}", name, e));
            let _resp: ModuleResp = serde_json::from_str(&contents)
                .unwrap_or_else(|e| panic!("failed to deserialize {}: {}", name, e));
            last_contents = Some(contents);
        }

        let last_json = last_contents.unwrap();
        let default_json =
            serde_json::to_string_pretty(&ModuleResp::default()).expect("serialize default");
        if last_json != default_json {
            panic!(
                "last file {} does not match ModuleResp::default() (as serialized JSON)",
                entries
                    .last()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
            );
        }
    }

    #[test]
    #[ignore]
    fn write_new_default_module_resp_json() {
        let resp = ModuleResp::default();
        let json = serde_json::to_string_pretty(&resp).expect("serialize ModuleResp");
        fs::create_dir_all(SCHEMA_TEST_DIR).expect("create schema_test/module_resp dir");

        let last_file = fs::read_dir(SCHEMA_TEST_DIR)
            .expect("read schema_test/module_resp dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .filter_map(|e| {
                let binding = e.file_name();
                let name = binding.to_string_lossy();
                let num = name
                    .strip_prefix("module_resp_gen_")?
                    .strip_suffix(".json")?;
                num.parse::<u32>().ok().map(|n| (n, e.path()))
            })
            .max_by_key(|(n, _)| *n);

        if let Some((_, path)) = &last_file {
            let existing = fs::read_to_string(path).expect("read last module_resp file");
            if existing == json {
                panic!(
                    "file already exists: default ModuleResp matches {}",
                    path.display()
                );
            }
        }

        let next_num = last_file.map(|(n, _)| n + 1).unwrap_or(0);
        let name = format!("module_resp_gen_{:04}.json", next_num);
        let path = format!("{}/{}", SCHEMA_TEST_DIR, name);
        fs::write(&path, json).expect("write module_resp");
    }
}
