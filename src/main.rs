mod repo;
mod stacks;
mod models;

slint::include_modules!();

fn main() {
    use stacks::{StackProvider, DummyStackProvider};

    let provider = DummyStackProvider;
    let stacks = provider.get_stacks();
    let model = models::build_stacks_model(&stacks);

    let app = App::new().unwrap();
    app.set_stacks(model);
    app.run().unwrap();
}
