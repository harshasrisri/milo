use core::format_args;
use std::fmt::Write;
use std::fs::File;
use std::io::Result;
use std::io::{BufRead, BufReader};
use std::time::Instant;
use txtdt::buffer::Buffer;
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

struct Editor {
    terminal: Terminal,
    buffer: Buffer,
    status_msg: String,
    status_msg_ts: Instant,
    quit_count: usize,
}

impl Editor {
    pub fn new() -> Result<Self> {
        Ok(Self {
            terminal: Terminal::new_raw()?,
            buffer: Buffer::new(),
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

    pub fn keep_alive(&self) -> bool {
        self.quit_count > 0
    }
}

fn editor_process_keypress(e: &mut Editor) -> Result<()> {
    let key = e.terminal.read_key()?;

    match key {
        Key::Control('Q') => {
            if e.buffer.is_dirty() && e.quit_count > 0 {
                editor_set_status_message!(
                    e,
                    "WARNING!!! Press Ctrl-Q {} more times to quit. File has unsaved changes.",
                    e.quit_count
                );
                e.quit_count -= 1;
            }
            return Ok(()); // To prevent resetting QUIT_COUNT
        }
        Key::Control('S') => editor_save(e)?,
        Key::Move(motion) => e.buffer.move_cursor(motion, e.rows(), e.cols()),
        Key::Printable(ch) => e.buffer.insert_char(ch),
        Key::Tab => e.buffer.insert_char('\t'),
        Key::Newline => e.buffer.insert_new_line(),
        Key::Escape | Key::Control('L') => {}
        Key::Backspace | Key::Control('H') => e.buffer.delete_char(),
        Key::Delete => {
            e.buffer.move_cursor(Motion::Right, e.rows(), e.cols());
            e.buffer.delete_char();
        }
        _key => {}
    };
    e.quit_count = TOTAL_QUIT_COUNT;
    Ok(())
}

fn editor_home_screen(e: &Editor) -> String {
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
        .collect()
}

fn editor_draw_rows(e: &mut Editor) {
    let content = if e.buffer.is_empty() {
        editor_home_screen(e)
    } else {
        e.buffer.frame_content(e.rows(), e.cols())
    };
    e.terminal.append(content.as_str());
}

fn editor_prompt(e: &mut Editor, prompt: &str) -> Result<Option<String>> {
    let mut reply = String::new();
    loop {
        editor_set_status_message!(e, "{}{}", prompt, reply);
        editor_refresh_screen(e);
        match e.terminal.read_key()? {
            Key::Printable(ch) => reply.push(ch),
            Key::Escape => {
                editor_set_status_message!(e, "");
                return Ok(None);
            }
            Key::Newline => {
                if !reply.is_empty() {
                    editor_set_status_message!(e, "");
                    return Ok(Some(reply));
                }
            }
            Key::Delete | Key::Backspace | Key::Control('H') => {
                reply.pop();
            }
            _ => {}
        }
    }
}

fn editor_draw_status_bar(e: &mut Editor) {
    let filename = e
        .buffer
        .filename()
        .as_ref()
        .map(|file| file.to_str().unwrap_or("<file-name-not-utf8>"))
        .unwrap_or("[No Name]");
    let status_left = format!(
        "{name:<.*} - {lc} lines {dirty}",
        FILE_NAME_WIDTH,
        name = filename,
        lc = e.buffer.line_count(),
        dirty = if e.buffer.is_dirty() {
            "(modified)"
        } else {
            ""
        },
    );
    let (c_row, _) = e.buffer.cursor_position();
    let status_right = format!("{}/{}", c_row + 1, e.buffer.line_count());
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

    e.terminal.append("\x1b[7m");
    e.terminal.append(status.as_str());
    e.terminal.append("\x1b[m");
    e.terminal.append("\r\n");
}

fn editor_draw_message_bar(e: &mut Editor) {
    e.terminal.append("\x1b[K");
    if e.status_msg_ts.elapsed().as_secs() < 5 {
        e.status_msg.truncate(e.cols());
        let msg = e.status_msg.clone();
        e.terminal.append(msg.as_str());
    }
}

fn editor_refresh_screen(e: &mut Editor) {
    e.buffer.scroll(e.rows(), e.cols());

    e.terminal.append("\x1b[?25l");
    e.terminal.append("\x1b[H");

    editor_draw_rows(e);
    editor_draw_status_bar(e);
    editor_draw_message_bar(e);

    let (c_row, c_col) = e.buffer.cursor_placement();
    e.terminal
        .append(format!("\x1b[{};{}H", c_row, c_col).as_str());
    e.terminal.append("\x1b[?25h");
    e.terminal.flush();
}

fn editor_save(e: &mut Editor) -> Result<()> {
    if e.buffer.filename().is_none() {
        if let Ok(some_name) = editor_prompt(e, "Save as (ESC to cancel): ") {
            e.buffer.set_filename(some_name);
        }
    }
    if let Some(filename) = &e.buffer.filename() {
        let content = e.buffer.rows_to_string();
        if let Err(err) = std::fs::write(filename, content.as_bytes()) {
            editor_set_status_message!(e, "Can't save! I/O error: {}", err);
            return Err(err);
        }
        editor_set_status_message!(e, "{} bytes written to disk", content.len());
        e.buffer.not_dirty();
    } else {
        editor_set_status_message!(e, "Filename not set!!!");
    }
    Ok(())
}

fn editor_open(e: &mut Editor, file_arg: Option<String>) -> Result<()> {
    if let Some(file) = file_arg {
        e.buffer.set_filename(Some(file.clone()));
        let line_iter = BufReader::new(File::open(file)?).lines();
        for line in line_iter {
            e.buffer.append_row(line?);
        }
    }
    e.buffer.not_dirty();
    Ok(())
}

fn main() -> Result<()> {
    let mut editor = Editor::new()?;

    editor_open(&mut editor, std::env::args().nth(1))?;
    editor_set_status_message!(&mut editor, "HELP: Ctrl-S = save | Ctrl-Q = quit");

    while editor.keep_alive() {
        editor_refresh_screen(&mut editor);
        editor_process_keypress(&mut editor)?;
    }

    Ok(())
}
