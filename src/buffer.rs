use crate::line::Line;
use crate::terminal::Motion;
use std::cmp::min;
use std::path::PathBuf;

#[derive(Default)]
pub struct Buffer {
    render_col: usize,
    cursor_col: usize,
    cursor_row: usize,
    lines: Vec<Line>,
    row_offset: usize,
    col_offset: usize,
    filename: Option<PathBuf>,
    dirty: bool,
}

impl Buffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn not_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn filename(&self) -> &Option<PathBuf> {
        &self.filename
    }

    pub fn set_filename(&mut self, filename: Option<String>) {
        self.filename = filename.map(|filename| filename.into());
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn cursor_position(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    pub fn cursor_placement(&self) -> (usize, usize) {
        (
            self.cursor_row - self.row_offset + 1,
            self.render_col - self.col_offset + 1,
        )
    }

    pub fn move_cursor(&mut self, motion: Motion, rows: usize, cols: usize) {
        match motion {
            Motion::Up => self.cursor_row = self.cursor_row.saturating_sub(1),
            Motion::Left => {
                if self.cursor_col != 0 {
                    self.cursor_col -= 1;
                } else if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.cursor_col = self.lines[self.cursor_row].len();
                }
            }
            Motion::Down => {
                self.cursor_row = min(self.lines.len().saturating_sub(1), self.cursor_row + 1)
            }
            Motion::Right => {
                if let Some(row) = self.lines.get(self.cursor_row) {
                    if self.cursor_col < row.len() {
                        self.cursor_col += 1;
                    } else if self.cursor_row < self.lines.len() - 1 {
                        self.cursor_row += 1;
                        self.cursor_col = 0;
                    }
                }
            }
            Motion::PgUp => self.cursor_row = self.cursor_row.saturating_sub(rows),
            Motion::PgDn => {
                self.cursor_row = min(self.lines.len().saturating_sub(1), self.cursor_row + rows)
            }
            Motion::Home => self.cursor_col = 0,
            Motion::End => self.cursor_col = cols - 1,
        }

        if let Some(row) = self.lines.get(self.cursor_row) {
            self.cursor_col = min(row.len(), self.cursor_col);
        }
    }

    pub fn scroll(&mut self, rows: usize, cols: usize) {
        self.render_col = self
            .lines
            .get(self.cursor_row)
            .map(|line| line.cursor_to_render_position(self.cursor_col))
            .unwrap_or_default();

        if self.cursor_row < self.row_offset {
            self.row_offset = self.cursor_row;
        } else if self.cursor_row >= self.row_offset + rows {
            self.row_offset = 1 + self.cursor_row - rows;
        }

        if self.render_col < self.col_offset {
            self.col_offset = self.render_col;
        } else if self.render_col >= self.col_offset + cols {
            self.col_offset = 1 + self.render_col - cols;
        }
    }

    pub fn place_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row;
        self.cursor_col = col;
        self.row_offset = self.lines.len();
    }

    pub fn frame_content(&self, rows: usize, cols: usize) -> String {
        self.lines
            .iter()
            .skip(self.row_offset)
            .map(|line| line.rendered())
            .chain(
                std::iter::repeat("~")
                    .take(rows.saturating_sub(self.lines.len().saturating_sub(self.row_offset))),
            )
            .map(|line| {
                line.chars()
                    .skip(self.col_offset)
                    .take(cols)
                    .chain("\x1b[K\r\n".chars())
            })
            .take(rows)
            .flatten()
            .collect::<String>()
    }

    pub fn rows_to_string(&self) -> String {
        let mut content = self
            .lines
            .iter()
            .map(|line| line.content().to_string())
            .collect::<Vec<String>>()
            .join("\n");
        content.push('\n');
        content
    }

    fn insert_row(&mut self, index: usize, line: String) {
        if index > self.lines.len() {
            return;
        }
        self.lines.insert(index, Line::new(line));
        self.dirty = true;
    }

    pub fn append_row(&mut self, line: String) {
        self.insert_row(self.lines.len(), line);
    }

    pub fn insert_new_line(&mut self) {
        if self.cursor_col == 0 {
            self.insert_row(self.cursor_row, String::new());
        } else {
            let tail = self.lines[self.cursor_row].split_off(self.cursor_col);
            self.insert_row(self.cursor_row + 1, tail);
        }
        self.cursor_row += 1;
        self.cursor_col = 0;
    }

    pub fn insert_char(&mut self, ch: char) {
        if self.cursor_row == self.lines.len() {
            self.insert_row(self.cursor_row, String::new());
        }
        if let Some(line) = self.lines.get_mut(self.cursor_row) {
            line.insert(self.cursor_col, ch);
            self.cursor_col += 1;
            self.dirty = true;
        }
    }

    fn delete_row(&mut self) {
        if self.cursor_row < self.lines.len() {
            self.lines.remove(self.cursor_row);
            self.dirty = true;
        }
    }

    pub fn delete_char(&mut self) {
        if (self.cursor_row, self.cursor_col) == (0, 0) {
            return;
        }
        if let Some(line) = self.lines.get_mut(self.cursor_row) {
            if self.cursor_col > 0 {
                line.remove(self.cursor_col - 1);
                self.cursor_col -= 1;
                self.dirty = true;
            } else {
                self.cursor_col = self.lines[self.cursor_row - 1].len();
                let tail = self.lines[self.cursor_row].content().to_string();
                self.lines[self.cursor_row - 1].push_str(&tail);
                self.delete_row();
                self.cursor_row -= 1;
            }
        }
    }

    pub fn find(&self, query: String) -> (usize, usize) {
        for (n, line) in self.lines.iter().enumerate() {
            let matches = line.match_indices(&query);
            if !matches.is_empty() {
                return (n, matches[0].0);
            }
        }
        (self.cursor_row, self.cursor_row)
    }
}
