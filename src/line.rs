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

    pub fn render_position(&self, pos: usize) -> usize {
        self.actual.chars().take(pos).fold(0, |rx, c| {
            if c == '\t' {
                rx + (TAB_STOP - 1) - (rx % TAB_STOP)
            } else {
                rx + 1
            }
        })
    }

    fn update(&mut self) {
        self.rendered.clear();
        self.rendered.extend(self.actual.chars().enumerate().map(|(n, c)| {
            if c == '\t' {
                std::iter::repeat(' ')
                    .take(TAB_STOP - (n % TAB_STOP))
                    .collect()
            } else {
                c.to_string()
            }
        }));
    }
}
