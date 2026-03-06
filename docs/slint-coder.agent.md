# Slint App Development Agent Guide

You are helping a user build a desktop/embedded GUI application using Slint.
Slint uses a declarative `.slint` UI markup language paired with a host language (Rust, C++, Python, or JavaScript/Node.js).

## Quick Start: Creating a New Rust + Slint Project

```bash
cargo init my-app && cd my-app
cargo add slint
cargo add slint-build --build
```

Create `ui/app.slint`:
```slint
import { Button, VerticalBox } from "std-widgets.slint";

export component App inherits Window {
    title: "My App";
    preferred-width: 400px;
    preferred-height: 300px;

    callback button-clicked;

    VerticalBox {
        Text { text: "Hello, Slint!"; }
        Button {
            text: "Click me";
            clicked => { root.button-clicked(); }
        }
    }
}
```

Create `build.rs`:
```rust
fn main() {
    slint_build::compile("ui/app.slint").unwrap();
}
```

Write `src/main.rs`:
```rust
slint::include_modules!();

fn main() {
    let app = App::new().unwrap();
    app.on_button_clicked({
        let weak = app.as_weak();
        move || {
            let app = weak.unwrap();
            // handle click
        }
    });
    app.run().unwrap();
}
```

Run: `cargo run`

## Quick Start: Python + Slint

```bash
pip install slint
```

```python
import slint

# Load the .slint file
class App(slint.loader.app.App):
    pass

app = App()
app.run()
```

Or inline:
```python
import slint

App = slint.load_file("ui/app.slint")
app = App()
app.run()
```

## Quick Start: Node.js + Slint

```bash
npm init -y
npm install slint-ui
```

```javascript
import * as slint from "slint-ui";
const ui = slint.loadFile("ui/app.slint");
const app = new ui.App();
app.run();
```

---

## Slint Language Reference

### Component Structure

```slint
// Import standard widgets
import { Button, CheckBox, LineEdit } from "std-widgets.slint";

// Import from other .slint files
import { MyWidget } from "my-widget.slint";

// Define a reusable component
component MyButton inherits Rectangle {
    in property <string> label: "Default";
    callback clicked;

    TouchArea {
        clicked => { root.clicked(); }
    }
    Text { text: root.label; }
}

// The main exported component (entry point)
export component App inherits Window {
    preferred-width: 800px;
    preferred-height: 600px;
    title: "My Application";

    MyButton {
        label: "Press me";
        clicked => { debug("pressed"); }
    }
}
```

### Property Qualifiers

- `private property <T> name;` - internal only (default if no qualifier)
- `in property <T> name;` - set by parent/user, read internally
- `out property <T> name;` - set internally, read by parent/user
- `in-out property <T> name;` - read/write by both sides

### Primitive Types

| Type | Default | Example |
|------|---------|---------|
| `bool` | `false` | `true` |
| `int` | `0` | `42` |
| `float` | `0` | `3.14`, `30%` (= 0.30) |
| `string` | `""` | `"hello"`, `"value: \{my_prop}"` |
| `length` | `0px` | `100px`, `2cm`, `1in` |
| `physical-length` | `0phx` | `1phx` |
| `duration` | `0ms` | `250ms`, `1s` |
| `angle` | `0deg` | `90deg`, `1.2rad`, `0.25turn` |
| `color` | `transparent` | `#ff0000`, `red`, `Colors.blue` |
| `brush` | `transparent` | color or gradient |
| `image` | empty | `@image-url("path/to/img.png")` |
| `percent` | `0%` | `50%` |
| `easing` | `linear` | `ease-in-out` |

### Structs and Enums

```slint
export struct TodoItem {
    title: string,
    checked: bool,
}

export enum Status {
    active,
    completed,
    archived,
}
```

### Arrays and Models

```slint
in property <[TodoItem]> items: [
    { title: "First", checked: false },
    { title: "Second", checked: true },
];

// Iteration
for item[index] in root.items: Text {
    text: item.title;
}
```

### Callbacks

```slint
export component App inherits Window {
    callback my-action(string, int);
    callback my-query(string) -> bool;

    // Invoke
    Button {
        clicked => {
            root.my-action("hello", 42);
            if (root.my-query("test")) {
                debug("yes");
            }
        }
    }
}
```

### Two-Way Bindings

```slint
CheckBox {
    checked <=> root.my-bool-property;
}
```

### Change Callbacks

```slint
changed my-property => {
    debug("property changed to: " + my-property);
}
```

### Conditional Elements

```slint
if condition : Text { text: "shown when true"; }
```

### Global Singletons

```slint
export global AppState {
    in-out property <int> counter: 0;
    callback increment();
    increment() => { self.counter += 1; }
}

// Usage anywhere:
Text { text: "Count: " + AppState.counter; }
```

### Animations

```slint
Rectangle {
    background: area.has-hover ? blue : red;
    animate background { duration: 250ms; easing: ease-in-out; }
}
```

### States

```slint
states [
    pressed when touch.pressed: {
        background: darkblue;
        animate background { duration: 100ms; }
    }
    hovered when touch.has-hover: {
        background: lightblue;
    }
]
```

### Timers

```slint
Timer {
    interval: 1s;
    running: true;
    triggered => { /* called every second */ }
}
```

---

## Layout Containers

| Container | Description |
|-----------|-------------|
| `VerticalLayout` / `VerticalBox` | Stack children vertically |
| `HorizontalLayout` / `HorizontalBox` | Stack children horizontally |
| `GridLayout` / `GridBox` | Place children in a grid (`row`, `col`, `colspan`, `rowspan`) |

Layout properties: `spacing`, `padding`, `padding-left/right/top/bottom`, `alignment` (start, end, center, space-between, space-around, stretch).

Child sizing in layouts: `min-width`, `max-width`, `preferred-width`, `horizontal-stretch`, `vertical-stretch` (and height equivalents).

---

## Standard Widgets (import from "std-widgets.slint")

**Input:** Button, CheckBox, Switch, ComboBox, LineEdit, TextEdit, Slider, SpinBox, DatePickerPopup, TimePickerPopup
**Display:** Text (built-in), Image (built-in), ProgressIndicator, Spinner
**Containers:** ScrollView, ListView, StandardListView, StandardTableView, TabWidget, GroupBox
**Layout helpers:** VerticalBox, HorizontalBox, GridBox
**Dialogs:** Dialog, PopupWindow (built-in), StandardButton
**Theming:** Palette (global), StyleMetrics (global)

### Commonly Used Widget Properties

**Button:** `text`, `icon`, `enabled`, `clicked =>`
**CheckBox:** `text`, `checked`, `toggled =>`
**LineEdit:** `text`, `placeholder-text`, `accepted(text) =>`, `edited(text) =>`
**Slider:** `value`, `minimum`, `maximum`, `changed(value) =>`
**ComboBox:** `model: ["A", "B"]`, `current-value`, `current-index`, `selected(value) =>`
**ListView:** container for `for` loops with virtual scrolling
**StandardListView:** `model: [{ text: "Item" }]`, `current-item`, `item-pointer-event(index, event, point) =>`

---

## Rust API Patterns

### Access properties from Rust
```rust
// Getter: get_<property_name>() (hyphens become underscores)
let val = app.get_my_property();
// Setter: set_<property_name>(value)
app.set_my_property(42);
```

### Handle callbacks from Rust
```rust
app.on_my_callback(move |arg: slint::SharedString| {
    println!("callback with: {}", arg);
});

// Invoke a callback from Rust
app.invoke_my_callback("hello".into());
```

### Models (dynamic lists)
```rust
use slint::{Model, ModelRc, VecModel, SharedString};
use std::rc::Rc;

let model = Rc::new(VecModel::<TodoItem>::from(vec![
    TodoItem { title: "Item 1".into(), checked: false },
]));
app.set_todo_model(model.clone().into());

// Modify:
model.push(TodoItem { title: "New".into(), checked: false });
model.remove(0);
model.set_row_data(0, new_item);

// Read:
let count = model.row_count();
let item = model.row_data(0).unwrap();
```

### Global singletons from Rust
```rust
app.global::<AppState>().set_counter(5);
app.global::<AppState>().on_increment(move || { /* ... */ });
```

### Weak references (required for closures)
```rust
let weak = app.as_weak();
app.on_something(move || {
    let app = weak.unwrap();
    app.set_status("done".into());
});
```

### Run code on the UI thread from another thread
```rust
let weak = app.as_weak();
std::thread::spawn(move || {
    // ... do work ...
    weak.upgrade_in_event_loop(move |app| {
        app.set_result("computed".into());
    }).unwrap();
});
```

---

## Common Patterns

### Color helpers
```slint
background: Colors.red;
background: #1e1e2e;
background: rgba(255, 0, 0, 128);    // RGBA with 0-255 alpha
background: white.transparentize(50%);
background: base-color.darker(20%);
background: base-color.brighter(20%);
background: @linear-gradient(180deg, #e66465 0%, #9198e5 100%);
background: @radial-gradient(circle, #e66465 0%, #9198e5 100%);
```

### Image element
```slint
Image {
    source: @image-url("assets/logo.png");
    image-fit: contain; // contain, cover, fill, preserve
    width: 64px;
    height: 64px;
}
```

### TouchArea (mouse/touch interaction)
```slint
TouchArea {
    clicked => { }
    has-hover  // bool, read-only
    pressed    // bool, read-only
    mouse-x, mouse-y  // length, read-only
    mouse-cursor: pointer; // default, none, pointer, crosshair, text, etc.
}
```

### FocusScope (keyboard input)
```slint
FocusScope {
    key-pressed(event) => {
        if (event.text == Key.Return) {
            // handle enter
            accept
        }
        reject
    }
}
```

### PopupWindow
```slint
my-popup := PopupWindow {
    x: 10px; y: 10px;
    Rectangle {
        background: Palette.background;
        Text { text: "Popup content"; }
    }
}
// Show it:
Button {
    clicked => { my-popup.show(); }
}
```

---

## Available Themes/Styles

Slint renders widgets in one of these styles (selected at compile time or via `SLINT_STYLE` env var):
- `fluent` (Microsoft Fluent Design)
- `cupertino` (Apple style)
- `material` (Google Material)
- `cosmic` (GNOME Cosmic)
- `qt` (native Qt)
- `native` (platform default)

Use `Palette` global to read/set the current color scheme:
```slint
Rectangle {
    background: Palette.background;
    Text {
        color: Palette.foreground;
        text: "Themed text";
    }
}
```

Set dark/light mode: `Palette.color-scheme: ColorScheme.dark;`

---

## Examples to Read for Inspiration

When you need reference patterns for a Slint app, read these files from the repository:

### Simple apps
- **Todo app (Rust):** `examples/todo/ui/todo.slint` + `examples/todo/rust/lib.rs` + `examples/todo/rust/Cargo.toml` + `examples/todo/rust/build.rs`
- **Memory game:** `examples/memory/memory.slint`
- **Slide puzzle:** `examples/slide_puzzle/slide_puzzle.slint`

### Widget showcase
- **Gallery (all widgets):** `examples/gallery/gallery.slint`

### Layouts
- **Layout examples:** `examples/layouts/` (multiple .slint files demonstrating different layout approaches)

### Rich/complex UIs
- **Printer demo:** `demos/printerdemo/ui/printerdemo.slint`
- **IoT dashboard:** `examples/iot-dashboard/iot-dashboard.slint`
- **Energy monitor:** `demos/energy-monitor/ui/desktop_main.slint`
- **Weather demo:** `demos/weather-demo/ui/main.slint`
- **Home automation:** `demos/home-automation/ui/main.slint`

### Custom drawing / animations
- **Fancy demo:** `examples/fancy_demo/main.slint`
- **Speedometer:** `examples/speedometer/demo.slint`
- **Fancy switches:** `examples/fancy-switches/fancy-switches.slint`
- **Orbit animation:** `examples/orbit-animation/orbit-animation.slint`

### Multi-language examples
- **Todo (Node.js):** `examples/todo/node/main.js`
- **Memory (Python):** `examples/memory/main.py`
- **Memory (Node.js):** `examples/memory/main.js`

### Standard widget source (for understanding widget APIs)
- `internal/compiler/widgets/common/` (shared widget logic)

---

## Typical Rust Project File Structure

```
my-app/
  Cargo.toml          # slint + slint-build deps
  build.rs            # calls slint_build::compile("ui/app.slint")
  src/
    main.rs           # slint::include_modules!(); + app logic
  ui/
    app.slint          # main UI file
    components/        # optional: reusable .slint components
      sidebar.slint
      header.slint
  assets/              # images, icons
```

## build.rs Template (Rust)

```rust
fn main() {
    slint_build::compile("ui/app.slint").unwrap();
}
```

## Cargo.toml Dependencies (Rust)

```toml
[dependencies]
slint = "1.16"

[build-dependencies]
slint-build = "1.16"
```

Optional Slint features: `"serde"`, `"gettext"`, `"image-default-formats"`, `"backend-winit"`, `"renderer-femtovg"`, `"renderer-skia"`.

---

## Tips for the Agent

- Always use `export component` for the main window and any types/globals accessed from host code.
- Hyphens and underscores are interchangeable in .slint identifiers (`my-prop` = `my_prop`), but Rust API uses underscores.
- Use `root.` to refer to the current component's root properties from nested elements.
- Use `self.` to refer to the current element's own properties.
- Use `parent.` to refer to the parent element.
- Element IDs use `:=` syntax: `my-btn := Button { }`, then reference as `my-btn.text`.
- `slint::include_modules!()` generates Rust types for all `export`ed components, structs, enums, and globals.
- `SharedString` is Slint's string type in Rust. Convert with `.into()` from `&str`/`String`.
- `ModelRc` wraps models. Convert `Rc<VecModel<T>>` with `.into()`.
- Prefer declarative bindings over imperative `changed` callbacks when possible.
- The `@tr("...")` macro is for internationalization/translation.
- `debug("msg")` prints to stderr for debugging during development.
