const TAB_STOP: usize = 8;

pub struct Line {
    actual: String,
    rendered: String,
}

impl Line {
    pub fn new(actual: String) -> Self {
        let mut ret = Self {
            actual,
            rendered: String::new(),
        };
        ret.update();
        ret
    }

    pub fn len(&self) -> usize {
        self.actual.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actual.is_empty()
    }

    pub fn insert(&mut self, pos: usize, ch: char) {
        self.actual.insert(pos, ch);
        self.update();
    }

    pub fn remove(&mut self, pos: usize) {
        self.actual.remove(pos);
        self.update();
    }

    pub fn push_str(&mut self, content: &str) {
        self.actual.push_str(content);
        self.update();
    }

    pub fn content(&self) -> &str {
        self.actual.as_str()
    }

    pub fn rendered(&self) -> &str {
        self.rendered.as_str()
    }

    pub fn match_indices(&self, query: &str) -> Vec<(usize, &str)> {
        self.rendered.match_indices(query).collect()
    }

    pub fn render_position(&self, pos: usize) -> usize {
        self.actual.chars().take(pos).fold(0, |rx, c| {
            if c == '\t' {
                rx + TAB_STOP - (rx % TAB_STOP)
            } else {
                rx + 1
            }
        })
    }

    pub fn split_off(&mut self, index: usize) -> String {
        let tail = self.actual.split_off(index);
        self.update();
        tail
    }

    fn update(&mut self) {
        self.rendered.clear();
        for ch in self.actual.chars() {
            if ch == '\t' {
                self.rendered.push(' ');
                while self.rendered.len() % TAB_STOP != 0 {
                    self.rendered.push(' ');
                }
            } else {
                self.rendered.push(ch);
            }
        }
    }
}
