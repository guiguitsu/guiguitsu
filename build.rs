fn main() {
    #[cfg(feature = "gui")]
    {
        // Enable experimental Slint elements (DragArea, DropArea).
        unsafe { std::env::set_var("SLINT_ENABLE_EXPERIMENTAL_FEATURES", "1") };
        slint_build::compile("ui/app.slint").unwrap();
    }
}
