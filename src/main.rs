use milo::editor::Editor;
use std::io::Result;

fn main() -> Result<()> {
    let mut editor = Editor::new()?;

    editor.open(std::env::args().nth(1))?;
    editor.set_status("HELP: Ctrl-S = save | Ctrl-F = find | Ctrl-Q = quit".to_string());

    while editor.keep_alive() {
        editor.refresh_screen();
        editor.process_keypress()?;
    }

    Ok(())
}
