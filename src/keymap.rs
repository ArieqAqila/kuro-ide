//! The list of every documented key binding, command and mouse gesture.
//!
//! This is the single source of truth that `docs/KEYBINDINGS.md` is checked against (both directions), and
//! `tests/keybindings.rs` requires a behaviour probe for every entry, so a binding cannot be added,
//! removed or documented without the tests noticing.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    /// Stable identifier used by the behaviour probes.
    pub id: &'static str,
    /// What the user presses / types, exactly as printed in the docs table.
    pub keys: &'static str,
    /// Where it applies: Everywhere, Normal, Visual, Insert, Find bar, Result table, Picker, Suggestions, Command line, Mouse.
    pub context: &'static str,
    pub action: &'static str,
}

const fn b(id: &'static str, keys: &'static str, context: &'static str, action: &'static str) -> Binding {
    Binding { id, keys, context, action }
}

pub const BINDINGS: &[Binding] = &[
    // ---- everywhere
    b("save", "Ctrl+S", "Everywhere", "Save (asks for a name if the buffer has none)"),
    b("quit", "Ctrl+Q", "Everywhere", "Quit; asks to save when there are unsaved changes"),
    b("find", "Ctrl+F", "Everywhere", "Open the find bar (top-right)"),
    b("replace", "Ctrl+H", "Everywhere", "Open find and replace"),
    b("undo", "Ctrl+Z", "Everywhere", "Undo"),
    b("redo", "Ctrl+Y", "Everywhere", "Redo"),
    b("redo2", "Ctrl+Shift+Z", "Everywhere", "Redo (needs the kitty keyboard protocol)"),
    b("copy", "Ctrl+C", "Everywhere", "Copy the selection (no selection: the current line)"),
    b("cut", "Ctrl+X", "Everywhere", "Cut the selection (no selection: the current line)"),
    b("paste", "Ctrl+V", "Everywhere", "Paste"),
    b("select_all", "Ctrl+A", "Everywhere", "Select all"),
    b("alt_copy", "Alt+C", "Everywhere", "Copy (for terminals that reserve Ctrl+C)"),
    b("alt_cut", "Alt+X", "Everywhere", "Cut (for terminals that reserve Ctrl+X)"),
    b("alt_paste", "Alt+V", "Everywhere", "Paste (for terminals that reserve Ctrl+V)"),
    b("cmdline", "Ctrl+P", "Everywhere", "Open the command line (like `:`; works in Insert mode)"),
    b("snippets", "Alt+S", "Everywhere", "Open the SQL snippet picker"),
    b("next_match", "F3", "Everywhere", "Next match of the last search"),
    b("prev_match", "Shift+F3", "Everywhere", "Previous match of the last search"),
    b("focus_table", "F4", "Everywhere", "Focus the SQL result table (and back)"),
    b("run_buffer", "F6", "Everywhere", "SQL: run the whole buffer"),
    b("cancel_query", "F7", "Everywhere", "SQL: cancel the running query"),
    b("run", "Ctrl+Enter", "Everywhere", "SQL: run the selection, or the statement under the cursor"),
    b("run_fallback", "Alt+Enter", "Everywhere", "SQL: same as Ctrl+Enter for terminals that cannot send it (`sql.run_fallback_key`)"),
    b("run_line", "Alt+L", "Everywhere", "SQL: run only the current line (`sql.run_line_key`)"),
    // ---- normal mode
    b("n_left", "h", "Normal", "Move left (also ←)"),
    b("n_down", "j", "Normal", "Move down (also ↓)"),
    b("n_up", "k", "Normal", "Move up (also ↑)"),
    b("n_right", "l", "Normal", "Move right (also →)"),
    b("n_word", "w", "Normal", "Next word start"),
    b("n_back", "b", "Normal", "Previous word start"),
    b("n_end", "e", "Normal", "End of word"),
    b("n_line0", "0", "Normal", "Start of line"),
    b("n_first", "^", "Normal", "First non-blank of the line"),
    b("n_eol", "$", "Normal", "End of line"),
    b("n_top", "gg", "Normal", "First line (`5gg`: line 5)"),
    b("n_bottom", "G", "Normal", "Last line (`5G`: line 5)"),
    b("n_insert", "i", "Normal", "Insert before the cursor"),
    b("n_append", "a", "Normal", "Insert after the cursor"),
    b("n_insert_bol", "I", "Normal", "Insert at the first non-blank"),
    b("n_append_eol", "A", "Normal", "Insert at the end of the line"),
    b("n_open_below", "o", "Normal", "New line below, enter Insert mode"),
    b("n_open_above", "O", "Normal", "New line above, enter Insert mode"),
    b("n_x", "x", "Normal", "Delete the character under the cursor (`3x`)"),
    b("n_X", "X", "Normal", "Delete the character before the cursor"),
    b("n_dd", "dd", "Normal", "Delete line(s) (`3dd`)"),
    b("n_yy", "yy", "Normal", "Yank line(s)"),
    b("n_cc", "cc", "Normal", "Change line(s)"),
    b("n_dmotion", "d{motion}", "Normal", "Delete over a motion (`dw`, `d$`, `dG`…)"),
    b("n_ymotion", "y{motion}", "Normal", "Yank over a motion"),
    b("n_cmotion", "c{motion}", "Normal", "Change over a motion (`cw` changes to the end of the word)"),
    b("n_D", "D", "Normal", "Delete to the end of the line"),
    b("n_C", "C", "Normal", "Change to the end of the line"),
    b("n_Y", "Y", "Normal", "Yank the line"),
    b("n_p", "p", "Normal", "Paste after"),
    b("n_P", "P", "Normal", "Paste before"),
    b("n_u", "u", "Normal", "Undo"),
    b("n_ctrl_r", "Ctrl+R", "Normal", "Redo"),
    b("n_v", "v", "Normal", "Visual mode (characters)"),
    b("n_V", "V", "Normal", "Visual mode (lines)"),
    b("n_colon", ":", "Normal", "Command line"),
    b("n_slash", "/", "Normal", "Open the find bar"),
    b("n_n", "n", "Normal", "Next match"),
    b("n_N", "N", "Normal", "Previous match"),
    b("n_ctrl_d", "Ctrl+D", "Normal", "Half page down"),
    b("n_ctrl_u", "Ctrl+U", "Normal", "Half page up"),
    b("n_count", "1-9", "Normal", "Count prefix (`5j`, `3dd`)"),
    b("n_esc", "Esc", "Normal", "Cancel a pending command, clear the selection, close the find bar"),
    // ---- visual mode
    b("v_motion", "h j k l w b e 0 $ gg G", "Visual", "Extend the selection"),
    b("v_d", "d", "Visual", "Delete the selection (also x)"),
    b("v_y", "y", "Visual", "Yank the selection"),
    b("v_c", "c", "Visual", "Change the selection"),
    b("v_p", "p", "Visual", "Replace the selection with the last yank"),
    b("v_o", "o", "Visual", "Jump to the other end of the selection"),
    b("v_switch", "v / V", "Visual", "Toggle character / line selection; leave Visual mode"),
    b("v_esc", "Esc", "Visual", "Leave Visual mode"),
    // ---- insert mode and modeless editing
    b("i_esc", "Esc", "Insert", "Leave Insert mode (modal) / clear the selection"),
    b("i_enter", "Enter", "Insert", "New line, keeping the indentation"),
    b("i_tab", "Tab", "Insert", "Insert an indent; with a selection indent the lines"),
    b("i_backtab", "Shift+Tab", "Insert", "Outdent the current line or the selected lines"),
    b("i_backspace", "Backspace", "Insert", "Delete back; in indentation one indent step; on a whitespace run the whole run"),
    b("i_delete", "Delete", "Insert", "Delete forward; on whitespace the whole run"),
    b("i_alt_bs", "Alt+Backspace", "Insert", "Delete the previous word"),
    b("i_arrows", "← ↑ → ↓", "Insert", "Move (with Shift: select)"),
    b("i_ctrl_arrows", "Ctrl+← Ctrl+→", "Insert", "Move by word (with Shift: select)"),
    b("i_home_end", "Home End", "Insert", "Line start (first non-blank first) / line end"),
    b("i_ctrl_home_end", "Ctrl+Home Ctrl+End", "Insert", "Start / end of the file"),
    b("i_page", "PageUp PageDown", "Insert", "Move a page"),
    // ---- find bar
    b("f_next", "Enter", "Find bar", "Next match (in the replace field: replace this one)"),
    b("f_prev", "Shift+Enter", "Find bar", "Previous match"),
    b("f_esc", "Esc", "Find bar", "Close the find bar"),
    b("f_case", "Alt+C", "Find bar", "Toggle Match case"),
    b("f_word", "Alt+W", "Find bar", "Toggle Whole word"),
    b("f_regex", "Alt+R", "Find bar", "Toggle Regex"),
    b("f_tab", "Tab", "Find bar", "Switch between the find and replace fields"),
    b("f_replace_all", "Alt+A", "Find bar", "Replace all (one undo step)"),
    b("f_updown", "↑ ↓", "Find bar", "Previous / next match"),
    b("f_kill", "Ctrl+U", "Find bar", "Clear the field before the cursor"),
    // ---- result table
    b("t_rows", "j k ↓ ↑", "Result table", "Scroll one row"),
    b("t_page", "PageUp PageDown", "Result table", "Scroll a page"),
    b("t_ends", "g G", "Result table", "First / last row"),
    b("t_cols", "h l ← →", "Result table", "Select the previous / next column"),
    b("t_home_end", "Home End", "Result table", "First / last column"),
    b("t_cells", "H L Ctrl+← Ctrl+→", "Result table", "Scroll sideways by 8 cells"),
    b("t_expand", "e Enter Space", "Result table", "Expand / collapse the current column"),
    b("t_back", "Esc Tab", "Result table", "Back to the editor"),
    b("t_close", "q", "Result table", "Hide the result pane"),
    b("t_copy", "Ctrl+C", "Result table", "Copy the result as TSV"),
    // ---- pickers and the suggestion popup
    b("p_move", "↑ ↓ Ctrl+N Ctrl+P", "Picker", "Move the selection"),
    b("p_accept", "Enter Tab", "Picker", "Choose the highlighted item"),
    b("p_esc", "Esc", "Picker", "Cancel"),
    b("p_filter", "(typing)", "Picker", "Filter the list"),
    b("s_move", "↑ ↓ Ctrl+N Ctrl+P", "Suggestions", "Move the selection"),
    b("s_accept", "Tab Enter", "Suggestions", "Accept: replaces the typed prefix"),
    b("s_esc", "Esc", "Suggestions", "Dismiss (stays in Insert mode)"),
    // ---- command line
    b("c_w", ":w [file]", "Command line", "Write"),
    b("c_q", ":q  :q!", "Command line", "Quit (`!` discards changes)"),
    b("c_wq", ":wq  :x", "Command line", "Write and quit"),
    b("c_e", ":e file", "Command line", "Open a file"),
    b("c_line", ":N", "Command line", "Go to line N"),
    b("c_setlang", ":setlang [mode]", "Command line", "Set the language mode (picker without an argument; `auto` to detect)"),
    b("c_snippets", ":snippets", "Command line", "Open the SQL snippet picker"),
    b("c_connect", ":connect NAME", "Command line", "Connect with a profile from connections.toml"),
    b("c_disconnect", ":disconnect", "Command line", "Disconnect"),
    b("c_run", ":run", "Command line", "Run the selection / current statement"),
    b("c_runline", ":runline", "Command line", "Run the current line"),
    b("c_runbuffer", ":runbuffer", "Command line", "Run the whole buffer"),
    b("c_cancel", ":cancel", "Command line", "Cancel the running query"),
    b("c_sql", ":sql close / show / profiles / warnings", "Command line", "Result pane, profile list and warnings"),
    b("c_warnings", ":warnings", "Command line", "Show configuration warnings"),
    // ---- mouse
    b("m_click", "Click", "Mouse", "Place the cursor"),
    b("m_drag", "Drag", "Mouse", "Select"),
    b("m_double", "Double-click", "Mouse", "Select the word"),
    b("m_triple", "Triple-click", "Mouse", "Select the line"),
    b("m_gutter", "Click the line number", "Mouse", "Select the whole line"),
    b("m_wheel", "Wheel", "Mouse", "Scroll (Shift+wheel: sideways)"),
    b("m_bar", "Click find-bar buttons", "Mouse", "Aa / ab / .* toggles, ↑ ↓, ✕, ↵ replace, ⇉ replace all"),
    b("m_table_wheel", "Wheel over the table", "Mouse", "Scroll rows (Shift+wheel: columns)"),
    b("m_table_header", "Click / double-click a column header", "Mouse", "Select / expand the column"),
    b("m_picker", "Click a picker row", "Mouse", "Choose it"),
];

/// The documentation tables, generated from `BINDINGS` (one section per context, in order of first use).
/// `docs/KEYBINDINGS.md` must contain exactly this text between its BEGIN/END markers.
pub fn markdown() -> String {
    let mut contexts: Vec<&str> = vec![];
    for b in BINDINGS {
        if !contexts.contains(&b.context) {
            contexts.push(b.context);
        }
    }
    let mut out = String::new();
    for c in contexts {
        out.push_str(&format!("### {c}\n\n| Keys | Action |\n|---|---|\n"));
        for b in BINDINGS.iter().filter(|b| b.context == c) {
            out.push_str(&format!("| `{}` | {} |\n", b.keys, b.action));
        }
        out.push('\n');
    }
    out
}

pub const MD_BEGIN: &str = "<!-- BEGIN KEYBINDINGS (generated: make docs) -->";
pub const MD_END: &str = "<!-- END KEYBINDINGS -->";

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_rows_are_unique_per_context() {
        let ids: HashSet<_> = BINDINGS.iter().map(|b| b.id).collect();
        assert_eq!(ids.len(), BINDINGS.len(), "duplicate id");
        let rows: HashSet<_> = BINDINGS.iter().map(|b| (b.keys, b.context)).collect();
        assert_eq!(rows.len(), BINDINGS.len(), "the same keys listed twice in one context");
    }

    #[test]
    fn markdown_has_a_row_per_binding_and_a_section_per_context() {
        let md = markdown();
        assert_eq!(md.matches("\n| `").count(), BINDINGS.len());
        for c in ["Everywhere", "Normal", "Visual", "Insert", "Find bar", "Result table", "Picker", "Suggestions", "Command line", "Mouse"]
        {
            assert!(md.contains(&format!("### {c}\n")), "{c}");
        }
    }

    #[test]
    fn every_field_is_filled_and_table_safe() {
        for b in BINDINGS {
            assert!(!b.id.is_empty() && !b.keys.is_empty() && !b.context.is_empty() && !b.action.is_empty(), "{b:?}");
            assert!(!b.keys.contains('|') && !b.action.contains('|'), "a `|` would break the markdown table: {b:?}");
        }
    }
}
