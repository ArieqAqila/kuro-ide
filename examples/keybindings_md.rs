//! Prints the generated keybinding tables (`make docs` pastes them into docs/KEYBINDINGS.md).
fn main() {
    print!("{}", kuro::keymap::markdown());
}
