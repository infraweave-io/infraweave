use tera::{Tera, Context, Result as TeraResult};
use crate::def::Module;
use serde_json;

pub fn generate_crd_from_module(module: &Module) -> TeraResult<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("crd_template.yaml", include_str!("../templates/crd_template.yaml"))?;

    let mut context = Context::new();
    context.insert("group", "example.com");
    context.insert("plural", "s3buckets");
    context.insert("kind", &module.spec.moduleName);
    context.insert("listKind", &format!("{}List", &module.spec.moduleName));
    context.insert("singular", "s3bucket");

    // Dynamically adding parameters to the context
    let params: Vec<_> = module.spec.parameters.iter().map(|p| {
        let mut prop = serde_json::Map::new();
        prop.insert("name".to_string(), serde_json::Value::String(p.name.clone()));
        prop.insert("type".to_string(), serde_json::Value::String(p.type_.clone()));
        serde_json::Value::Object(prop)
    }).collect();
    context.insert("parameters", &params);

    tera.render("crd_template.yaml", &context)
}
