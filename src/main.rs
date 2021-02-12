use core::format_args;
use std::cmp::min;
use std::fmt::Write;
use std::fs::File;
use std::io::Result;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::Instant;
use txtdt::line::Line;
use txtdt::terminal::{Key, Motion, Terminal};

const STATUS_HEIGHT: usize = 2; // 1 for Status bar. 1 for Status Message
const TOTAL_QUIT_COUNT: usize = 3;
const FILE_NAME_WIDTH: usize = 20;
const STATUS_LINE_BLANK: char = ' ';

macro_rules! editor_set_status_message {
    ($e:expr, $($arg:tt)*) => {{
        $e.status_msg.clear();
        if let Ok(_) = $e.status_msg.write_fmt($crate::format_args!($($arg)*)) {
            $e.status_msg_ts = Instant::now();
        }
    }}
}

struct EditorState {
    terminal: Terminal,
    render_col: usize,
    cursor_col: usize,
    cursor_row: usize,
    keep_alive: bool,
    lines: Vec<Line>,
    row_offset: usize,
    col_offset: usize,
    filename: Option<PathBuf>,
    dirty: bool,
    status_msg: String,
    status_msg_ts: Instant,
    quit_count: usize,
}

impl EditorState {
    pub fn new() -> Result<Self> {
        Ok(Self {
            terminal: Terminal::new_raw()?,
            render_col: 0,
            cursor_col: 0,
            cursor_row: 0,
            keep_alive: true,
            lines: Vec::new(),
            row_offset: 0,
            col_offset: 0,
            dirty: false,
            filename: None,
            status_msg: String::new(),
            status_msg_ts: Instant::now(),
            quit_count: TOTAL_QUIT_COUNT,
        })
    }

    pub fn rows(&self) -> usize {
        self.terminal.rows() - STATUS_HEIGHT
    }

    pub fn cols(&self) -> usize {
        self.terminal.cols()
    }
}

fn editor_move_cursor(e: &mut EditorState, motion: Motion) {
    match motion {
        Motion::Up => e.cursor_row = e.cursor_row.saturating_sub(1),
        Motion::Left => {
            if e.cursor_col != 0 {
                e.cursor_col -= 1;
            } else if e.cursor_row > 0 {
                e.cursor_row -= 1;
                e.cursor_col = e.lines[e.cursor_row].len();
            }
        }
        Motion::Down => e.cursor_row = min(e.lines.len().saturating_sub(1), e.cursor_row + 1),
        Motion::Right => {
            if let Some(row) = e.lines.get(e.cursor_row) {
                if e.cursor_col < row.len() {
                    e.cursor_col += 1;
                } else if e.cursor_row < e.lines.len() - 1 {
                    e.cursor_row += 1;
                    e.cursor_col = 0;
                }
            }
        }
        Motion::PgUp => e.cursor_row = e.cursor_row.saturating_sub(e.rows()),
        Motion::PgDn => {
            e.cursor_row = min(e.lines.len().saturating_sub(1), e.cursor_row + e.rows())
        }
        Motion::Home => e.cursor_col = 0,
        Motion::End => e.cursor_col = e.cols() - 1,
    }

    if let Some(row) = e.lines.get(e.cursor_row) {
        e.cursor_col = min(row.len(), e.cursor_col);
    }
}

fn editor_process_keypress(e: &mut EditorState) -> Result<()> {
    let key = e.terminal.read_key()?;

    match key {
        Key::Control('Q') => {
            if e.dirty && e.quit_count > 0 {
                editor_set_status_message!(
                    e,
                    "WARNING!!! Press Ctrl-Q {} more times to quit. File has unsaved changes.",
                    e.quit_count
                );
                e.quit_count -= 1;
            } else {
                e.keep_alive = false;
            }
            return Ok(()); // To prevent resetting QUIT_COUNT
        }
        Key::Control('S') => editor_save(e)?,
        Key::Move(motion) => editor_move_cursor(e, motion),
        Key::Printable(ch) => editor_insert_char(e, ch as char),
        Key::Newline | Key::Escape | Key::Control('L') => {}
        Key::Backspace | Key::Control('H') => editor_delete_char(e),
        Key::Delete => {
            editor_move_cursor(e, Motion::Right);
            editor_delete_char(e);
        }
        _key => {}
    };
    e.quit_count = TOTAL_QUIT_COUNT;
    Ok(())
}

fn editor_draw_home_screen(e: &mut EditorState) {
    let mut banner = format!(
        "{} -- version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    banner.truncate(e.cols());
    let padding = e.cols().saturating_sub(banner.len()) / 2;
    let banner = if padding == 0 {
        banner
    } else {
        let mut centered = "~".to_string();
        centered.extend(std::iter::repeat(" ").take(padding - 1));
        centered.push_str(banner.as_str());
        centered
    };

    e.terminal.append(
        std::iter::repeat("~")
            .take(e.rows())
            .enumerate()
            .map(|(n, buf)| {
                if n == e.rows() / 3 {
                    banner.chars()
                } else {
                    buf.chars()
                }
            })
            .flat_map(|buf| buf.chain("\x1b[K\r\n".chars()))
            .collect::<String>()
            .as_str(),
    );
}

fn editor_draw_content(e: &mut EditorState) {
    e.terminal.append(
        e.lines
            .iter()
            .skip(e.row_offset)
            .map(|line| line.rendered())
            .chain(
                std::iter::repeat("~").take(
                    e.rows()
                        .saturating_sub(e.lines.len().saturating_sub(e.row_offset)),
                ),
            )
            .map(|line| {
                line.chars()
                    .skip(e.col_offset)
                    .take(e.cols())
                    .chain("\x1b[K\r\n".chars())
            })
            .take(e.rows())
            .flatten()
            .collect::<String>()
            .as_str(),
    );
}

fn editor_draw_rows(e: &mut EditorState) {
    if e.lines.is_empty() {
        editor_draw_home_screen(e)
    } else {
        editor_draw_content(e)
    }
}

fn editor_draw_status_bar(e: &mut EditorState) {
    e.terminal.append("\x1b[7m");
    let filename = e
        .filename
        .as_ref()
        .map(|file| file.to_str().unwrap_or("<file-name-not-utf8>"))
        .unwrap_or("[No Name]");
    let status_left = format!(
        "{name:<.*} - {lc} lines {dirty}",
        FILE_NAME_WIDTH,
        name = filename,
        lc = e.lines.len(),
        dirty = if e.dirty { "(modified)" } else { "" },
    );
    let status_right = format!("{}/{}", e.cursor_row + 1, e.lines.len());
    let num_spaces = e
        .cols()
        .saturating_sub(status_left.len())
        .saturating_sub(status_right.len());
    let mut status = format!(
        "{left}{:spaces$}{right}",
        STATUS_LINE_BLANK,
        spaces = num_spaces,
        left = status_left,
        right = status_right
    );
    status.truncate(e.cols());
    e.terminal.append(status.as_str());
    e.terminal.append("\x1b[m");
    e.terminal.append("\r\n");
}

fn editor_scroll(e: &mut EditorState) {
    e.render_col = e.lines[e.cursor_row].render_position(e.cursor_col);

    if e.cursor_row < e.row_offset {
        e.row_offset = e.cursor_row;
    } else if e.cursor_row >= e.row_offset + e.rows() {
        e.row_offset = 1 + e.cursor_row - e.rows();
    }

    if e.render_col < e.col_offset {
        e.col_offset = e.render_col;
    } else if e.render_col >= e.col_offset + e.cols() {
        e.col_offset = 1 + e.render_col - e.cols();
    }
}

fn editor_draw_message_bar(e: &mut EditorState) {
    e.terminal.append("\x1b[K");
    if e.status_msg_ts.elapsed().as_secs() < 5 {
        e.status_msg.truncate(e.cols());
        let msg = e.status_msg.clone();
        e.terminal.append(msg.as_str());
    }
}

fn editor_refresh_screen(e: &mut EditorState) {
    editor_scroll(e);

    e.terminal.append("\x1b[?25l");
    e.terminal.append("\x1b[H");

    editor_draw_rows(e);
    editor_draw_status_bar(e);
    editor_draw_message_bar(e);

    e.terminal.append(
        format!(
            "\x1b[{};{}H",
            e.cursor_row - e.row_offset + 1,
            e.render_col - e.col_offset + 1
        )
        .as_str(),
    );
    e.terminal.append("\x1b[?25h");
    e.terminal.flush();
}

fn editor_append_row(e: &mut EditorState, line: String) {
    e.lines.push(Line::new(line));
    e.dirty = true;
}

fn editor_insert_char(e: &mut EditorState, ch: char) {
    if e.cursor_row == e.lines.len() {
        editor_append_row(e, "".to_string());
    }
    if let Some(line) = e.lines.get_mut(e.cursor_row) {
        line.insert(e.cursor_col, ch);
        e.cursor_col += 1;
        e.dirty = true;
    }
}

fn editor_delete_char(e: &mut EditorState) {
    if (e.cursor_row, e.cursor_col) == (0, 0) {
        return;
    }
    if let Some(line) = e.lines.get_mut(e.cursor_row) {
        if e.cursor_col > 0 {
            line.remove(e.cursor_col - 1);
            e.cursor_col -= 1;
            e.dirty = true;
        } else {
            e.cursor_col = e.lines[e.cursor_row - 1].len();
            let tail = e.lines[e.cursor_row].content().to_string();
            e.lines[e.cursor_row - 1].push_str(&tail);
            editor_delete_row(e);
            e.cursor_row -= 1;
        }
    }
}

fn editor_delete_row(e: &mut EditorState) {
    if e.cursor_row < e.lines.len() {
        e.lines.remove(e.cursor_row);
        e.dirty = true;
    }
}

fn editor_rows_to_string(e: &EditorState) -> String {
    let mut content = e
        .lines
        .iter()
        .map(|line| line.content().to_string())
        .collect::<Vec<String>>()
        .join("\n");
    content.push('\n');
    content
}

fn editor_save(e: &mut EditorState) -> Result<()> {
    if let Some(filename) = &e.filename {
        let content = editor_rows_to_string(e);
        if let Err(err) = std::fs::write(filename, content.as_bytes()) {
            editor_set_status_message!(e, "Can't save! I/O error: {}", err);
            return Err(err);
        }
        editor_set_status_message!(e, "{} bytes written to disk", content.len());
        e.dirty = false;
    } else {
        editor_set_status_message!(e, "Filename not set!!!");
    }
    Ok(())
}

fn editor_open(e: &mut EditorState, file_arg: Option<String>) -> Result<()> {
    if let Some(file) = file_arg {
        e.filename = Some(file.clone().into());
        let line_iter = BufReader::new(File::open(file)?).lines();
        for line in line_iter {
            editor_append_row(e, line?);
        }
    }
    e.dirty = false;
    Ok(())
}

fn main() -> Result<()> {
    let mut editor = EditorState::new()?;

    editor_open(&mut editor, std::env::args().nth(1))?;
    editor_set_status_message!(&mut editor, "HELP: Ctrl-S = save | Ctrl-Q = quit");

    while editor.keep_alive {
        editor_refresh_screen(&mut editor);
        editor_process_keypress(&mut editor)?;
    }

    Ok(())
}
