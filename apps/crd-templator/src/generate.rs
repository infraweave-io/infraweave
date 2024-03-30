use tera::{Tera, Context, Result as TeraResult};
use env_defs::ModuleManifest;
use serde_json;

pub fn generate_crd_from_module(module: &ModuleManifest) -> TeraResult<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("module_crd_template.yaml", include_str!("../templates/module_crd_template.yaml"))?;

    let singular = module.spec.module_name.to_lowercase();
    let plural = format!("{}s", singular);

    let mut context = Context::new();
    // TODO: use context.insert("group", &module.spec.group);
    context.insert("group", "infrabridge.io");
    context.insert("plural", &plural);
    context.insert("kind", &module.spec.module_name);
    context.insert("listKind", &format!("{}List", &module.spec.module_name));
    context.insert("singular", &singular);

    // Dynamically adding parameters to the context
    let params: Vec<_> = module.spec.parameters.iter().map(|p| {
        let mut prop = serde_json::Map::new();
        prop.insert("name".to_string(), serde_json::Value::String(p.name.clone()));
        prop.insert("type".to_string(), serde_json::Value::String(p.type_.clone()));
        serde_json::Value::Object(prop)
    }).collect();
    context.insert("parameters", &params);

    tera.render("module_crd_template.yaml", &context)
}
